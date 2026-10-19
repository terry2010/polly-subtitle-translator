// Qwen3-ASR 后端：Python 侧边进程（qwen_asr + transformers）批转切片。
//
// 移植自 srt-converter-demo/src-tauri/src/asr.rs 的 Qwen3 部分（含「停止即杀」修复）：
// - 侧边脚本为独立文件 qwen3_sidecar.py（include_str!），逐行 JSON 协议：
//   每批 {progress, segments_delta} 增量输出，结尾 {summary}——取消时已跑完批次不丢。
// - stdout 走 reader 线程 + channel + recv_timeout(200ms) 轮询：cancel 置位即 kill 子进程，
//   stopped=true 返回已收到的增量段落。
// - stderr 后台排空线程：防管道缓冲写满死锁，同时收集诊断文本。
// - Python/模型定位：托管环境优先（app_data_dir/asr/），env 可覆盖，系统 python3 兜底。
use super::audio::Chunk;
use super::cues::Segment;
use crate::ffmpeg::{app_data_dir, no_window};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

/// Qwen3 侧边脚本（离线、batched、chunk/词级时间戳 + progress/segments_delta 协议）。
const QWEN3_SIDECAR: &str = include_str!("qwen3_sidecar.py");

/// Qwen3 侧边一次运行的结果：分段 + 计时/空切片统计（供诊断日志使用）。
#[derive(Debug, Clone, Default)]
pub struct Qwen3Run {
    pub segments: Vec<Segment>,
    /// 识别结果为空（音乐/静音）的切片索引。
    pub empty_chunk_indices: Vec<usize>,
    /// VAD 判定为无人声（音乐/静音）而**未送模型**的切片索引。
    pub vad_skipped_indices: Vec<usize>,
    /// VAD 预筛耗时（秒）。
    pub vad_seconds: f64,
    /// 纯识别耗时（秒，不含起进程与加载模型）。
    pub recognize_seconds: f64,
    /// 每批耗时（秒）。
    pub batch_seconds: Vec<f64>,
    /// 被 cancel 中断（「停止」）：segments 只含已跑完批次的增量结果。
    pub stopped: bool,
}

/// 可离线加载的 Qwen3-ASR 模型目录名（`asr/models/<name>` 下查找）。
pub const QWEN3_MODEL_NAMES: &[&str] = &["Qwen3-ASR-0.6B", "Qwen3-ASR-1.7B"];
/// ForcedAligner 模型目录名（逐词时间戳；未装则退化为 chunk 级）。
pub const QWEN3_ALIGNER_NAME: &str = "Qwen3-ForcedAligner-0.6B";

/// 托管 ASR 环境根目录：app_data_dir/asr/（env.rs 负责创建）。
pub fn asr_home() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join("asr"))
}

/// 托管 Python：app_data_dir/asr/venv/{bin/python3 | bin/python | python.exe | Scripts/python.exe}。
/// python-build-standalone 解压后解释器在顶层 bin/（unix）或根目录 python.exe（windows）。
pub fn managed_python() -> Option<PathBuf> {
    let venv = asr_home()?.join("venv");
    [
        venv.join("bin/python3"),
        venv.join("bin/python"),
        venv.join("python.exe"),
        venv.join("Scripts/python.exe"),
    ]
    .into_iter()
    .find(|c| c.is_file())
}

/// 定位 Qwen3-ASR 使用的 Python：env QWEN_ASR_PY（需存在）→ 托管 venv → 系统 python3/python。
fn resolve_asr_python() -> String {
    if let Ok(p) = std::env::var("QWEN_ASR_PY") {
        let c = PathBuf::from(&p);
        if !p.is_empty() && c.is_file() {
            return c.to_str().unwrap_or(&p).to_string();
        }
    }
    if let Some(p) = managed_python() {
        return p.to_string_lossy().to_string();
    }
    if cfg!(windows) { "python".to_string() } else { "python3".to_string() }
}

/// 判断某目录是否为完整的 HuggingFace 模型目录：
/// config.json + 至少一个权重文件（*.safetensors / *.bin，含分片命名）。
/// 仅含 config 的目录是下载中断的半成品（权重还躺在 .cache/*.incomplete），
/// 不能视为已安装——否则幂等检查会跳过下载、转写加载时才失败。
pub(crate) fn is_valid_model_dir(dir: &Path) -> bool {
    if !dir.is_dir() || !dir.join("config.json").exists() {
        return false;
    }
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten().any(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.ends_with(".safetensors") || n.ends_with(".bin")
            })
        })
        .unwrap_or(false)
}

/// 解析离线 Qwen3 模型目录：env `QWEN_ASR_MODEL` 优先（覆盖一切），
/// 其次 `pref` 指定的目录名，否则按 [`QWEN3_MODEL_NAMES`] 顺序挑第一个存在的。
/// 搜索位置：app_data_dir/asr/models/<name>。
fn resolve_model_dir(pref: Option<&str>) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QWEN_ASR_MODEL") {
        let c = PathBuf::from(p);
        if c.is_dir() {
            return Some(c);
        }
    }
    let names: Vec<&str> = match pref {
        Some(p) => vec![p],
        None => QWEN3_MODEL_NAMES.to_vec(),
    };
    let models = asr_home()?.join("models");
    for name in &names {
        let c = models.join(name);
        if is_valid_model_dir(&c) {
            return Some(c);
        }
    }
    None
}

/// 解析 ForcedAligner 快照目录（env QWEN_FORCED_ALIGNER 优先，其次托管目录）。
fn resolve_forced_aligner_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QWEN_FORCED_ALIGNER") {
        let c = PathBuf::from(p);
        if is_valid_model_dir(&c) {
            return Some(c);
        }
    }
    let c = asr_home()?.join("models").join(QWEN3_ALIGNER_NAME);
    if is_valid_model_dir(&c) {
        Some(c)
    } else {
        None
    }
}

/// Qwen3-ASR 不可用时的原因（托管模型缺失）；可用时返回 None。
/// `model` 为 None 时按默认顺序挑模型目录。只做廉价的本地目录检查，
/// Python 侧边环境问题留到真正转写时再报详细错误。
pub fn qwen3_unavailable_reason(model: Option<&str>) -> Option<String> {
    if resolve_model_dir(model).is_none() {
        Some(match model {
            Some(m) => format!("model not installed: {m} (expected under app_data/asr/models/)"),
            None => "no Qwen3-ASR model installed (expected under app_data/asr/models/)".to_string(),
        })
    } else if managed_python().is_none() && std::env::var("QWEN_ASR_PY").is_err() {
        Some("managed python env not installed (app_data/asr/venv)".to_string())
    } else {
        None
    }
}

pub(crate) fn unique_tag() -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// 批量转写切片。`chunks` 带每片的绝对起点（offset）与词归属区间（keep_start/keep_end）：
/// 重叠 margin 内的词会在相邻片重复出现，按归属区间丢弃（词中心不在 keep 内即丢）。
/// `model` 指定模型目录名（如 "Qwen3-ASR-1.7B"）；`None` 按默认顺序挑选。
/// `cancel` 置位时杀掉 Python 侧边进程并返回 `stopped=true` 的部分结果
/// （侧边逐批增量输出 segments_delta，已跑完的批次不会丢）。
/// chunk 级（无 aligner）的整段文本无法按词拆分，重叠边界的短语可能重复出现一次，
/// 属已知妥协——配置了 ForcedAligner 时不会走到这个分支。
pub fn transcribe_qwen3_chunks(
    chunks: &[Chunk],
    language: &str,
    use_vad: bool,
    model: Option<&str>,
    device: Option<&str>,
    cancel: Option<&AtomicBool>,
    mut on_progress: impl FnMut(usize, usize, f64),
) -> Result<Qwen3Run, String> {
    let model_dir = resolve_model_dir(model).ok_or_else(|| {
        qwen3_unavailable_reason(model).unwrap_or_else(|| "qwen3 model not found".to_string())
    })?;

    let py = resolve_asr_python();
    // 文件名带唯一后缀：并行任务不会互相覆盖临时脚本。
    let tmp_script = std::env::temp_dir().join(format!("qwen3_asr_script_{}.py", unique_tag()));
    std::fs::write(&tmp_script, QWEN3_SIDECAR).map_err(|e| format!("write sidecar script failed: {e}"))?;
    let out_json = std::env::temp_dir().join(format!("qwen3_segments_{}.json", unique_tag()));
    let specs: Vec<serde_json::Value> = chunks
        .iter()
        .map(|c| serde_json::json!({"path": c.path, "offset": c.start}))
        .collect();
    let audio_json = serde_json::to_string(&specs).unwrap_or_default();

    let mut args: Vec<String> = vec![
        tmp_script.to_str().ok_or("invalid script path")?.to_string(),
        model_dir.to_str().ok_or("invalid model dir")?.to_string(),
        audio_json,
        language.to_string(),
        out_json.to_str().ok_or("invalid out path")?.to_string(),
    ];
    // 可选传入 ForcedAligner 快照目录（若提供，侧边会接入它使 time_stamps 可用）。
    if let Some(aligner_dir) = resolve_forced_aligner_dir() {
        args.push(aligner_dir.to_str().ok_or("invalid aligner dir")?.to_string());
    }
    // 只转录人声：让侧边用 Silero VAD 预筛，跳过音乐/静音切片。
    if use_vad {
        args.push("--vad".to_string());
    }
    // 设备开关：cpu/cuda/mps，传给 transformers 的 device_map。
    if let Some(d) = device {
        if !d.is_empty() && d != "auto" {
            args.push(format!("--device={d}"));
        }
    }
    let mut child = no_window(Command::new(&py))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn python sidecar failed: {e}"))?;
    let stdout = child.stdout.take().ok_or("cannot read python stdout")?;
    // stderr 后台排空：子进程若把管道缓冲写满会阻塞；线程顺便收集诊断文本。
    let stderr_handle = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            buf
        })
    });
    // stdout 逐行进 channel：主循环用 recv_timeout 轮询，才能在两行之间检查
    // cancel 标志并 kill 子进程（「停止」/ cancel_transcribe 同一机制）。
    let (tx, rx) = channel::<std::io::Result<String>>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let stop = line.is_err();
            if tx.send(line).is_err() || stop {
                break;
            }
        }
    });
    // 词级片段带归属区间进待定过滤；chunk 级整段文本（无 aligner）无法按词拆分，直接放行。
    let mut word_items: Vec<(Segment, f64, f64)> = Vec::new();
    let mut loose: Vec<Segment> = Vec::new();
    let mut run = Qwen3Run::default();
    loop {
        let line = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(l)) => l,
            Ok(Err(e)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("read python output failed: {e}"));
            }
            Err(RecvTimeoutError::Timeout) => {
                if !run.stopped && cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    // 停止：杀掉侧边；stdout 随即 EOF，reader 线程退出、channel 断开。
                    // kill 失败说明子进程已自然退出，不算被中断。
                    if child.kill().is_ok() {
                        run.stopped = true;
                    }
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(p) = v.get("progress") {
                let i = p.get("chunk").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
                let n = p.get("total").and_then(|c| c.as_u64()).unwrap_or(0).max(1) as usize;
                let pct = p.get("percent").and_then(|c| c.as_f64()).unwrap_or(0.0);
                on_progress(i, n, pct.clamp(10.0, 90.0));
            }
            // segments_delta = 每批增量输出（新协议）；segments = 结尾全量（旧协议，兜底兼容）。
            if let Some(arr) = v
                .get("segments")
                .or_else(|| v.get("segments_delta"))
                .and_then(|s| s.as_array())
            {
                for seg in arr {
                    let text = seg.get("text").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let start = seg.get("start").or_else(|| seg.get("timestamp")).and_then(|t| t.as_f64()).unwrap_or(0.0);
                    // 侧边脚本现在给 start/end（逐词）；兼容旧的 duration 字段。
                    let mut dur = seg.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0);
                    if dur <= 0.0 {
                        if let Some(end) = seg.get("end").and_then(|d| d.as_f64()) {
                            if end > start {
                                dur = end - start;
                            }
                        }
                    }
                    // 词级片段带 (keep_start, keep_end) 进归属过滤；无 chunk 归属或
                    // chunk 级整段文本直接放行（重叠边界的短语可能重复一次，已知妥协）。
                    let s = Segment { text, start_seconds: start, duration_seconds: dur };
                    let owner = seg
                        .get("chunk")
                        .and_then(|c| c.as_u64())
                        .and_then(|i| chunks.get(i as usize));
                    let is_word_level =
                        seg.get("level").and_then(|l| l.as_str()) != Some("chunk");
                    match (is_word_level, owner) {
                        (true, Some(c)) => word_items.push((s, c.keep_start, c.keep_end)),
                        _ => loose.push(s),
                    }
                }
            }
            // 侧边汇总：空切片索引与计时（用于诊断日志）。
            if let Some(s) = v.get("summary") {
                run.empty_chunk_indices = s
                    .get("empty_chunk_indices")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| x as usize).collect())
                    .unwrap_or_default();
                run.vad_skipped_indices = s
                    .get("vad_skipped_indices")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| x as usize).collect())
                    .unwrap_or_default();
                run.vad_seconds = s.get("vad_seconds").and_then(|x| x.as_f64()).unwrap_or(0.0);
                run.recognize_seconds = s.get("recognize_seconds").and_then(|x| x.as_f64()).unwrap_or(0.0);
                run.batch_seconds = s
                    .get("batch_seconds")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
                    .unwrap_or_default();
            }
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("wait python sidecar failed: {e}"))?;
    let stderr_text = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    // 词级片段按归属区间去重后排序；chunk 级整段文本原样并入。
    let mut segments = super::cues::collect_kept_words(word_items);
    segments.extend(loose);
    segments.sort_by(|a, b| {
        a.start_seconds
            .partial_cmp(&b.start_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 被「停止」杀掉的进程退出码必然非 success，不作为失败处理。
    if !run.stopped && !status.success() && segments.is_empty() {
        if let Ok(text) = std::fs::read_to_string(&out_json) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                    return Err(format!("Qwen3-ASR unavailable: {err}"));
                }
            }
        }
        return Err(format!("Qwen3-ASR sidecar failed. stderr:\n{stderr_text}"));
    }
    run.segments = segments;
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_model_dir_requires_weights_not_just_config() {
        let tmp = std::env::temp_dir().join(format!("asr_validity_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(!is_valid_model_dir(&tmp), "no config.json → invalid");
        std::fs::write(tmp.join("config.json"), "{}").unwrap();
        assert!(!is_valid_model_dir(&tmp), "config only (interrupted download) → invalid");
        std::fs::write(tmp.join("model.safetensors"), b"fake").unwrap();
        assert!(is_valid_model_dir(&tmp), "config + safetensors → valid");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn unavailable_reason_reports_missing_model_or_python() {
        // 无 env 覆盖时，函数必须返回可操作的原因（不 panic）。
        let r = qwen3_unavailable_reason(None);
        // 本机未建托管环境 → 必有原因；装了则是 None。
        if let Some(r) = r {
            assert!(r.contains("model") || r.contains("python"), "{r}");
        }
    }

    #[test]
    fn model_names_cover_both_tiers() {
        assert!(QWEN3_MODEL_NAMES.contains(&"Qwen3-ASR-0.6B"));
        assert!(QWEN3_MODEL_NAMES.contains(&"Qwen3-ASR-1.7B"));
        assert_eq!(QWEN3_ALIGNER_NAME, "Qwen3-ForcedAligner-0.6B");
    }

    #[test]
    fn sidecar_script_embedded_and_valid() {
        // include_str! 内置的脚本应是完整的（含增量协议与 VAD 段）。
        assert!(QWEN3_SIDECAR.contains("segments_delta"));
        assert!(QWEN3_SIDECAR.contains("silero_vad"));
        assert!(QWEN3_SIDECAR.contains("_segs_for"));
        assert!(QWEN3_SIDECAR.contains("forced_aligner"));
    }

    // ===== 假侧边驱动测试（无模型，env 注入 shell 脚本解释器；仅 unix）=====
    #[cfg(unix)]
    mod fake_sidecar {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Mutex;

        // env 是进程级共享状态，env 测试间必须串行。
        static ENV_LOCK: Mutex<()> = Mutex::new(());

        /// 保存并在 drop 时恢复 env 覆盖，防止测试串扰。
        struct EnvGuard {
            vars: Vec<(&'static str, Option<String>)>,
        }
        impl EnvGuard {
            fn set(vars: &[(&'static str, String)]) -> Self {
                let saved: Vec<_> = vars
                    .iter()
                    .map(|(k, _)| (*k, std::env::var(k).ok()))
                    .collect();
                for (k, v) in vars {
                    std::env::set_var(k, v);
                }
                Self { vars: saved }
            }
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                for (k, v) in self.vars.drain(..) {
                    match v {
                        Some(v) => std::env::set_var(k, v),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }

        /// 造一个假的侧边环境：临时模型目录（含 config.json）+ 可执行假 python 脚本。
        /// 返回 (临时根目录, guard)；临时目录靠 leaked TempDir 存活到测试结束。
        fn fake_env(script_body: &str) -> (PathBuf, EnvGuard) {
            let root = std::env::temp_dir().join(format!("asr_test_{}", unique_tag()));
            let model = root.join("model");
            std::fs::create_dir_all(&model).unwrap();
            std::fs::write(model.join("config.json"), "{}").unwrap();
            let py = root.join("fake_python");
            let body = format!("#!/bin/sh\n{script_body}\n");
            std::fs::write(&py, body).unwrap();
            let mut perm = py.metadata().unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&py, perm).unwrap();
            let guard = EnvGuard::set(&[
                ("QWEN_ASR_PY", py.to_string_lossy().to_string()),
                ("QWEN_ASR_MODEL", model.to_string_lossy().to_string()),
            ]);
            (root, guard)
        }

        fn chunks(n: usize) -> Vec<Chunk> {
            (0..n)
                .map(|i| Chunk {
                    path: PathBuf::from(format!("/tmp/chunk_{i}.wav")),
                    start: i as f64 * 10.0,
                    end: i as f64 * 10.0 + 10.0,
                    keep_start: i as f64 * 10.0,
                    keep_end: i as f64 * 10.0 + 10.0,
                })
                .collect()
        }

        /// 正常路径：progress + segments_delta + summary 逐行解析。
        #[test]
        fn parses_incremental_delta_and_summary() {
            let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let (_root, _g) = fake_env(r#"
echo '{"progress":{"chunk":1,"total":2,"percent":50}}'
echo '{"segments_delta":[{"text":"hello","start":0.5,"end":0.9,"chunk":0},{"text":"world","start":0.9,"end":1.4,"chunk":0}]}'
echo '{"segments_delta":[{"text":"late","start":10.5,"end":11.0,"chunk":1,"level":"word"}]}'
echo '{"summary":{"empty_chunk_indices":[2],"vad_seconds":0.11,"recognize_seconds":0.22,"batch_seconds":[0.22]}}'
"#);
            let mut progress_hits = 0;
            let run = transcribe_qwen3_chunks(&chunks(2), "en-US", false, None, None, None, |_, _, _| {
                progress_hits += 1;
            })
            .expect("normal run should succeed");
            assert!(!run.stopped);
            assert_eq!(progress_hits, 1);
            let texts: Vec<&str> = run.segments.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(texts, vec!["hello", "world", "late"], "{:?}", run.segments);
            assert!((run.segments[0].start_seconds - 0.5).abs() < 1e-6);
            assert!((run.segments[0].duration_seconds - 0.4).abs() < 1e-6, "end-start 推导时长");
            assert_eq!(run.empty_chunk_indices, vec![2]);
            assert!((run.vad_seconds - 0.11).abs() < 1e-9);
            assert_eq!(run.batch_seconds, vec![0.22]);
        }

        /// 词级片段按 keep 归属去重（margin 副本丢弃、chunk 级整段放行）。
        #[test]
        fn word_level_kept_owner_filtered_chunk_level_passthrough() {
            let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            // chunk0 keep [0,10) / chunk1 keep [10,20)：
            // "dup" 在两片都出现（chunk0 margin + chunk1 keep）→ margin 副本应被丢；
            // chunk 级 "block"（level:"chunk"）不参与归属过滤，直接放行。
            let (_root, _g) = fake_env(r#"
echo '{"segments_delta":[{"text":"in","start":5.0,"end":5.3,"chunk":0,"level":"word"},{"text":"dup","start":10.2,"end":10.6,"chunk":0,"level":"word"},{"text":"dup","start":10.15,"end":10.55,"chunk":1,"level":"word"},{"text":"block","start":3.0,"end":6.0,"chunk":0,"level":"chunk"}]}'
echo '{"summary":{}}'
"#);
            let run = transcribe_qwen3_chunks(&chunks(2), "en-US", false, None, None, None, |_, _, _| {})
                .unwrap();
            let texts: Vec<&str> = run.segments.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(texts, vec!["block", "in", "dup"], "{:?}", run.segments);
        }

        /// 取消：第一批增量落盘后被杀 → stopped=true 且已收段落保留。
        #[test]
        fn cancel_kills_sidecar_and_keeps_partial() {
            let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let (_root, _g) = fake_env(r#"
echo '{"segments_delta":[{"text":"first","start":0.0,"end":0.4,"chunk":0}]}'
# exec 让 sleep 直接替换 sh：kill 命中的是 sleep 本身，stdout 立刻 EOF。
exec sleep 60
"#);
            let cancel = AtomicBool::new(false);
            let flag = &cancel;
            // 另起线程 800ms 后置位 cancel（等第一批 delta 被消费）。
            std::thread::scope(|s| {
                s.spawn(|| {
                    std::thread::sleep(Duration::from_millis(800));
                    flag.store(true, Ordering::Relaxed);
                });
                let t0 = std::time::Instant::now();
                let run = transcribe_qwen3_chunks(
                    &chunks(2), "en-US", false, None, None, Some(&cancel), |_, _, _| {},
                )
                .expect("stopped run returns Ok with partial segments");
                assert!(run.stopped, "cancel 后应标记 stopped");
                assert!(t0.elapsed() < Duration::from_secs(10), "应快速返回而非等 sleep 60 结束");
                let texts: Vec<&str> = run.segments.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(texts, vec!["first"], "已收到的增量段落应保留: {texts:?}");
            });
        }

        /// 侧边非零退出 + out_json 带 error 字段 → 报出 error 内容。
        #[test]
        fn nonzero_exit_surfaces_out_json_error() {
            let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            // argv: [script, model_dir, audio_json, language, out_json, ...]
            let (_root, _g) = fake_env(r#"
echo '{"error":"model weights corrupted"}' > "$5"
exit 1
"#);
            let e = transcribe_qwen3_chunks(&chunks(1), "en-US", false, None, None, None, |_, _, _| {})
                .expect_err("nonzero exit should fail");
            assert!(e.contains("model weights corrupted"), "{e}");
        }

        /// resolve_* 的 env 覆盖路径。
        #[test]
        fn env_overrides_resolve_paths() {
            let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let (root, _g) = fake_env("true");
            assert_eq!(resolve_asr_python(), root.join("fake_python").to_string_lossy());
            assert_eq!(resolve_model_dir(None), Some(root.join("model")));
            assert_eq!(qwen3_unavailable_reason(None), None, "env 齐全时不应报不可用");
        }
    }
}
