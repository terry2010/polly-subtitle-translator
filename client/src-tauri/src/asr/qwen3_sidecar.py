import json, os, sys, time, wave
from pathlib import Path
model_dir, audio_json, lang, out = sys.argv[1:5]
# 本地模型：直接以扁平目录路径离线加载（transformers 对已存在目录跳过 repo 校验）。
_model_path = str(model_dir)
os.environ.setdefault("TRANSFORMERS_CACHE", _model_path)
os.environ.setdefault("HF_HOME", _model_path)
os.environ.setdefault("LOCAL_PATHS", _model_path)
# 第 5 个位置参数之后是可选开关：ForcedAligner 路径（目录）、以及 `--vad`
# （只转录人声：用 Silero VAD 预筛，跳过音乐/静音切片，不送模型）。
aligner_arg = None
use_vad = "--vad" in sys.argv
# 可选 --device=cpu|cuda|mps：transformers 后端的 device_map；缺省自动选择。
device_arg = None
for _a in sys.argv[5:]:
    if _a.startswith("--device="):
        device_arg = _a.split("=", 1)[1]
    elif _a.startswith("--"):
        continue
    else:
        aligner_arg = _a
# chunks 是 [{"path": ..., "offset": ...}]：offset 为该片在整段音频中的绝对起点
# （静音对齐 + 重叠切片后不再是等长偏移，必须由调用方显式给出）。
chunk_specs = json.loads(audio_json)
paths = [c["path"] for c in chunk_specs]
offsets = [float(c.get("offset", 0.0)) for c in chunk_specs]
# 将 locale 码（en-US/zh-CN/...）映射为 qwen_asr 所需的语言名（English/Chinese/...）。
_LANG_MAP = {
    "en-us": "English", "en": "English",
    "zh-cn": "Chinese", "zh": "Chinese",
    "yue-cn": "Cantonese",
    "ja-jp": "Japanese", "ja": "Japanese",
    "ko-kr": "Korean", "ko": "Korean",
}
lang = _LANG_MAP.get(lang.lower(), lang)
total = len(paths)
durations = []
for p in paths:
    try:
        w = wave.open(str(p), "rb")
        durations.append(w.getnframes() / w.getframerate())
        w.close()
    except Exception:
        durations.append(8.0)
from qwen_asr import Qwen3ASRModel  # noqa
BATCH = 16
# 可选接入本地已缓存的 ForcedAligner：启用后 return_time_stamps=True，可拿到逐词时间戳。
# 注意：Qwen3ASRModel.from_pretrained 的 forced_aligner 需要**路径/仓库名字符串**，
# 它内部会自己调用 Qwen3ForcedAligner.from_pretrained；若直接把 Qwen3ForcedAligner
# 实例传进去，会被当成 repo id 去校验并抛 HFValidationError。
aligner_path = None
if aligner_arg:
    a = Path(aligner_arg)
    if a.is_dir() and (a / "config.json").exists():
        aligner_path = str(a)
        sys.stderr.write("已接入 ForcedAligner: %s\n" % a)
    else:
        sys.stderr.write("ForcedAligner 路径无效，跳过: %s\n" % a)
init_kwargs = {}
if aligner_path:
    init_kwargs["forced_aligner"] = aligner_path
# 限制单次生成 token 上限：默认 512，遇到背景音乐/静音时模型可能陷入重复幻觉
# 一路生成到上限，单批耗时能膨胀到几十秒（实测 30s 切片会有 100s+ 的批次）。
init_kwargs["max_new_tokens"] = 192
if device_arg:
    init_kwargs["device_map"] = device_arg
    sys.stderr.write("指定设备: %s\n" % device_arg)
model = Qwen3ASRModel.from_pretrained(_model_path, local_files_only=True, **init_kwargs)
# 只有接入 forced_aligner 时才用 return_time_stamps（否则 qwen_asr 会报错）；
# 未接入时按 chunk 用 durations 给 chunk-level 时间戳（仍完全离线、产物含 -->）。
use_aligner = aligner_path is not None
# VAD 预筛（可选）：逐切片用 Silero VAD 估计人声时长，低于阈值视为音乐/静音，
# 不送模型（既减少 "Music/Oh" 之类幻觉，也省下这些切片的解码时间）。VAD 模型随
# silero_vad wheel 一起分发，完全离线；不可用时自动退化为全量识别。
vad_seconds_per_chunk = []  # 每片人声秒数；None 表示该片未能判定
vad_seconds = 0.0
if use_vad:
    try:
        import torch
        import soundfile as sf
        from silero_vad import load_silero_vad, get_speech_timestamps
        _vad = load_silero_vad(onnx=False)
        _tv0 = time.time()
        for _p in paths:
            try:
                data, sr = sf.read(str(_p), dtype="float32")
                if data.ndim > 1:
                    data = data.mean(axis=1)
                if sr != 16000:
                    vad_seconds_per_chunk.append(None)
                    continue
                _vad.reset_states()
                ts = get_speech_timestamps(torch.from_numpy(data), _vad,
                                           sampling_rate=16000, return_seconds=True)
                vad_seconds_per_chunk.append(sum(float(t["end"]) - float(t["start"]) for t in ts))
            except Exception:
                vad_seconds_per_chunk.append(None)
        vad_seconds = round(time.time() - _tv0, 3)
        sys.stderr.write("VAD 预筛完成: %.1fs\n" % vad_seconds)
    except Exception as _e:
        sys.stderr.write("VAD 不可用，退化为全量识别: %s\n" % _e)
        vad_seconds_per_chunk = []
VAD_MIN_SPEECH = 0.30  # 切片中人声不足 0.3s 视为音乐/静音
vad_skip = set()
if vad_seconds_per_chunk:
    for _idx, _secs in enumerate(vad_seconds_per_chunk):
        if _secs is not None and _secs < VAD_MIN_SPEECH:
            vad_skip.add(_idx)
active = [i for i in range(total) if i not in vad_skip]
per_chunk = [None] * total  # 每片 (text, words)；VAD 跳过的片保持 None
def _segs_for(idxs):
    # 把指定切片展开成段落列表（逐词或 chunk 级）：每批后增量输出（segments_delta），
    # 让 Rust 侧在「停止」杀掉本进程时已拿到已识别部分，不必等到全部跑完。
    out = []
    for idx in idxs:
        d = durations[idx] if idx < len(durations) else 8.0
        entry = per_chunk[idx]
        if entry is None:
            # VAD 判定为音乐/静音，未送模型；不产出字幕。
            continue
        text, words = entry
        base = offsets[idx]
        if words:
            for wt, ws, we in words:
                out.append({"text": wt, "start": base + ws, "end": base + we,
                            "chunk": idx, "level": "word"})
        elif text:
            # 无 aligner 时只能给出 chunk 级时间戳（整块文本一条）。
            out.append({"text": text, "start": base, "end": base + d,
                        "chunk": idx, "level": "chunk"})
    return out
# 计时统计：写进运行日志，便于判断"慢在哪一段"。
batch_seconds = []
empty_chunks = 0
recognize_seconds = 0.0
try:
    for _b in range(0, len(active), BATCH):
        idxs = active[_b:_b + BATCH]
        batch = [paths[i] for i in idxs]
        _t0 = time.time()
        res = model.transcribe(batch, language=lang,
                               return_time_stamps=use_aligner)
        batch_seconds.append(round(time.time() - _t0, 3))
        recognize_seconds = round(sum(batch_seconds), 3)
        for i, r in zip(idxs, res):
            text = (getattr(r, "text", None) or "").strip()
            stamps = getattr(r, "time_stamps", None)
            words = []
            if use_aligner and stamps is not None:
                # time_stamps 是 ForcedAlignResult（可迭代），元素含 text / start_time / end_time，
                # 粒度为逐词或逐字（CJK）。这里展开成逐词片段，交给 Rust 侧 merge_into_cues 合并。
                items = []
                for it in stamps:
                    wt = str(getattr(it, "text", "") or "").strip()
                    if not wt:
                        continue
                    try:
                        ws = float(getattr(it, "start_time", 0.0))
                        we = float(getattr(it, "end_time", 0.0))
                    except Exception:
                        continue
                    items.append([wt, ws, we])
                # 对齐器偶尔给出 start == end 的零长度片段；直接丢弃会丢词，
                # 这里借用后一段起点、否则给最小时长 80ms。
                for k, item in enumerate(items):
                    if item[2] <= item[1]:
                        nxt = items[k + 1][1] if k + 1 < len(items) else None
                        item[2] = nxt if (nxt is not None and nxt > item[1]) else item[1] + 0.08
                words = [(wt, ws, we) for wt, ws, we in items]
            per_chunk[i] = (text, words)
        if active:
            # total 用全部切片数（含 VAD 跳过的），与其它阶段的进度分母一致；
            # percent 仍按实际工作量算。
            # 增量输出本批段落（segments_delta）：Rust 侧逐批收集，中途「停止」
            # 也能保存已识别部分；结尾的完整 segments 只写 out 文件，不再上 stdout。
            print(json.dumps({"progress": {"chunk": min(_b + BATCH, len(paths)), "total": len(paths),
                                          "percent": round(100.0 * _b / len(active), 1),
                                          "elapsed": recognize_seconds},
                              "segments_delta": _segs_for(idxs)}))
            sys.stdout.flush()
    # 用逐词时间戳（片内起止 + 该片绝对 offset）替代整块粒度；空文本自动过滤。
    # 每段带 chunk 索引与 level（word/chunk）：重叠切片时 Rust 侧按归属区间去重。
    segs = _segs_for(range(total))
    # 逐切片统计：空结果=音乐/静音（Rust 侧据此写"无人声"日志）。
    empty_idx = [i for i, e in enumerate(per_chunk)
                 if e is not None and not e[0] and not e[1]]
    summary = {"total": total, "empty_chunks": len(empty_idx), "empty_chunk_indices": empty_idx,
               "vad_skipped_indices": sorted(vad_skip), "vad_seconds": vad_seconds,
               "recognize_seconds": recognize_seconds, "batch_seconds": batch_seconds}
    json.dump({"segments": segs, "summary": summary}, open(out, "w", encoding="utf-8"), ensure_ascii=False)
    # stdout 只留 summary：segments 已按批增量输出过（避免 Rust 侧重复收集）。
    sys.stdout.write(json.dumps({"summary": summary}, ensure_ascii=False) + "\n")
    sys.stdout.flush()
except Exception as e:
    json.dump({"error": "Qwen3 转写失败: {}".format(e)}, open(out, "w", encoding="utf-8"), ensure_ascii=False)
    sys.exit(2)