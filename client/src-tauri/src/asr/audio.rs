// ASR 音频管线：音轨提取为 16kHz 单声道 WAV + 自适应静音检测 + 停顿对齐切片。
//
// 移植自 srt-converter-demo/src-tauri/src/audio.rs，适配点：
// - ffmpeg/ffprobe 二进制走 `crate::ffmpeg::{find_ffmpeg, find_ffprobe}`（下载版/自定义路径），
//   由调用方解析一次后以 `&Path` 传入，不在函数内硬编码 PATH 查找；
// - Windows 下子进程经 `no_window` 包装隐藏控制台窗口；
// - 错误用 AppError（IPC 错误码驱动），探测部分复用 ffmpeg.probe*/extract* 错误码。
use crate::error::AppError;
use crate::ffmpeg::{no_window, normalize_path_for_ffmpeg};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 一个待识别切片：送给 ASR 的音频区间 + 词归属区间。
#[derive(Debug, Clone)]
pub struct Chunk {
    pub path: PathBuf,
    /// 音频区间（秒，相对整段音频起点）。含重叠 margin 时会超出归属区间。
    pub start: f64,
    pub end: f64,
    /// 词归属区间：词中心落在 [keep_start, keep_end) 才计入结果。
    /// 静音中点切出的片 keep == (start, end)；被迫硬切的边界两侧各留重叠 margin，
    /// margin 里的词会在邻片再次出现，按归属区间丢弃去重。
    pub keep_start: f64,
    pub keep_end: f64,
}

/// 一段静音（语音停顿），秒、相对音频起点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Silence {
    pub start: f64,
    pub end: f64,
}

/// 硬切兜底时边界两侧各加的上下文重叠秒数（词归属去重 margin）。
const CHUNK_OVERLAP_SECONDS: f64 = 1.0;
/// 静音切点首选搜索窗口下界：距理想切点 -50% 以内。
const CHUNK_SEARCH_LO_FACTOR: f64 = 0.5;
/// 单片最长不超过该倍数（相对目标时长）；超过即硬切 + 重叠。
const CHUNK_MAX_FACTOR: f64 = 2.0;
/// 静音切出的尾巴片至少留该倍数时长，避免碎尾。
const CHUNK_MIN_TAIL_FACTOR: f64 = 0.5;

fn io_err(msg: impl Into<String>) -> AppError {
    AppError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, msg.into()))
}

/// Path → 可直接传给 ffmpeg/ffprobe 的参数字符串：
/// 先 UTF-8 校验，再做 Windows 反斜杠→正斜杠归一化（`".mkv\"` 类路径会触发 Invalid argument）。
fn ffmpeg_arg(p: &Path) -> Result<String, AppError> {
    let s = p.to_str().ok_or_else(|| {
        io_err(format!("non-UTF-8 path: {}", p.display()))
    })?;
    Ok(normalize_path_for_ffmpeg(s))
}

/// 把选定音轨提取为 16kHz 单声道 WAV（供 ASR 使用）。
///
/// `stream_index` 是 ffprobe 的全局流索引（与 `AudioStream.index` 一致），按 `0:{index}` 直接映射。
pub fn extract_audio_track(
    video_path: &Path,
    stream_index: i64,
    out_dir: &Path,
    ffmpeg: &Path,
) -> Result<PathBuf, AppError> {
    std::fs::create_dir_all(out_dir)?;
    let stem = video_path
        .file_stem()
        .unwrap_or_else(|| "audio".as_ref())
        .to_string_lossy();
    let wav_path = out_dir.join(format!("{stem}_track{stream_index}.wav"));
    let arg = ffmpeg_arg(video_path)?;
    let out_arg = ffmpeg_arg(&wav_path)?;
    let status = no_window(Command::new(ffmpeg))
        .args([
            "-hide_banner", "-v", "error", "-nostdin",
            "-y",
            "-i", &arg,
            // stream_index 是全局流索引（与 probe 的 stream.index 一致），按全局索引直接映射。
            "-map", &format!("0:{stream_index}"),
            "-ar", "16000", "-ac", "1",
            "-c:a", "pcm_s16le",
            "-f", "wav",
            &out_arg,
        ])
        .status()
        .map_err(|e| AppError::FfmpegStartFailed { detail: format!("extract audio track: {e}") })?;
    if !status.success() {
        return Err(AppError::FfmpegExtractFailed {
            detail: format!("extract audio stream={stream_index} failed (no such track?)"),
        });
    }
    Ok(wav_path)
}

/// 纯函数：按静音对齐规划切片区间 (audio_start, audio_end, keep_start, keep_end)。
///
/// 切点优先取「距 pos+target 最近、落在 [pos+0.5T, pos+2T] 内的静音中点」；
/// 窗口内没有静音时在 pos+2T 硬切（余量 ≤2.5T 则对半切），并把该边界标记为
/// forced——forced 边界两侧各加 CHUNK_OVERLAP_SECONDS 的音频 margin，下游按
/// keep 区间去重。单片时长被限制在 ≤2T，尾部 ≤2T 直接收为最后一片。
fn plan_chunks(duration: f64, target: f64, silences: &[Silence]) -> Vec<(f64, f64, f64, f64)> {
    if duration <= 0.0 {
        return Vec::new();
    }
    let target = target.max(1.0);
    let mut cuts: Vec<(f64, bool)> = Vec::new(); // (切点, 是否被迫硬切)
    let mut pos = 0.0;
    loop {
        let remaining = duration - pos;
        if remaining <= target * CHUNK_MAX_FACTOR {
            break; // 尾部长度 ≤2T：不再制造切点，整段收为最后一片
        }
        let lo = pos + target * CHUNK_SEARCH_LO_FACTOR;
        let hi = pos + target * CHUNK_MAX_FACTOR;
        // 距理想切点最近的静音中点；要求切完后尾巴 ≥0.5T（否则会留出碎尾片）。
        let best = silences
            .iter()
            .map(|s| 0.5 * (s.start + s.end))
            .filter(|m| {
                *m > lo && *m < hi && duration - *m > target * CHUNK_MIN_TAIL_FACTOR
            })
            .min_by(|a, b| {
                (a - (pos + target))
                    .abs()
                    .partial_cmp(&(b - (pos + target)).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        match best {
            Some(m) => {
                cuts.push((m, false));
                pos = m;
            }
            None => {
                // 找不到静音：硬切 + 重叠兜底。余量 ≤2.5T 时对半切，避免碎尾。
                let c = if remaining <= target * 2.5 {
                    pos + remaining / 2.0
                } else {
                    pos + target * CHUNK_MAX_FACTOR
                };
                cuts.push((c, true));
                pos = c;
            }
        }
    }

    let n = cuts.len() + 1;
    (0..n)
        .map(|i| {
            let keep_start = if i == 0 { 0.0 } else { cuts[i - 1].0 };
            let keep_end = if i == n - 1 { duration } else { cuts[i].0 };
            let mut start = keep_start;
            let mut end = keep_end;
            // 被迫硬切的边界：左侧片尾、右侧片头各加 margin（clamp 到音频范围）。
            if i > 0 && cuts[i - 1].1 {
                start = (keep_start - CHUNK_OVERLAP_SECONDS).max(0.0);
            }
            if i < n - 1 && cuts[i].1 {
                end = (keep_end + CHUNK_OVERLAP_SECONDS).min(duration);
            }
            (start, end, keep_start, keep_end)
        })
        .collect()
}

/// 用 ffmpeg 把 wav 的 [start, end) 区间导出为 16k 单声道 wav。
/// `filter` 可选（如 "speechnorm"）：送给 ASR 前对切片做响度归一化，
/// 把压在 BGM 下的弱对白抬起来，缓解识别器「整片判无声」。
pub fn cut_audio(
    wav_path: &Path,
    start: f64,
    end: f64,
    out: &Path,
    filter: Option<&str>,
    ffmpeg: &Path,
) -> Result<(), AppError> {
    let location = ffmpeg_arg(wav_path)?;
    let mut args: Vec<String> = vec![
        "-hide_banner".into(), "-v".into(), "error".into(), "-nostdin".into(), "-y".into(),
        "-i".into(), location,
        "-ss".into(), format!("{start:.3}"),
        "-to".into(), format!("{end:.3}"),
    ];
    if let Some(f) = filter {
        args.push("-af".into());
        args.push(f.to_string());
    }
    for a in ["-acodec", "pcm_s16le", "-ar", "16000", "-ac", "1"] {
        args.push(a.into());
    }
    args.push(ffmpeg_arg(out)?);
    let status = no_window(Command::new(ffmpeg))
        .args(args)
        .status()
        .map_err(|e| AppError::FfmpegStartFailed { detail: format!("cut audio: {e}") })?;
    if !status.success() {
        return Err(AppError::FfmpegExtractFailed {
            detail: format!("cut audio failed [{start:.1}-{end:.1}]"),
        });
    }
    Ok(())
}

/// 按语音停顿对齐切片，找不到停顿的长段退回「重叠切 + 词归属去重」。
///
/// 每片音频区间由 ffmpeg `-ss/-to` 提取为 `{prefix}_{nnn}.wav`；返回的 [`Chunk`]
/// 里 `start/end` 是实际音频区间，`keep_start/keep_end` 是词归属区间。
/// `filter` 透传给 [`cut_audio`]（响度归一化等）。
pub fn split_chunks_at_silences(
    wav_path: &Path,
    chunk_seconds: f64,
    silences: &[Silence],
    filter: Option<&str>,
    ffmpeg: &Path,
    ffprobe: &Path,
) -> Result<Vec<Chunk>, AppError> {
    let prefix = wav_path
        .file_stem()
        .unwrap_or_else(|| "chunk".as_ref())
        .to_string_lossy();
    let dir = wav_path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let duration = audio_duration(wav_path, ffprobe)?;

    let plan = plan_chunks(duration, chunk_seconds, silences);
    let mut out = Vec::new();
    for (i, &(start, end, keep_start, keep_end)) in plan.iter().enumerate() {
        let f = dir.join(format!("{prefix}_{:03}.wav", i));
        let _ = std::fs::remove_file(&f);
        cut_audio(wav_path, start, end, &f, filter, ffmpeg)?;
        out.push(Chunk { path: f, start, end, keep_start, keep_end });
    }
    Ok(out)
}

/// 用 ffprobe 精确提取文件的 format.duration（秒）。
pub fn audio_duration(wav_path: &Path, ffprobe: &Path) -> Result<f64, AppError> {
    let raw = ffmpeg_arg(wav_path)?;
    // 注意：ffprobe 不支持 -nostdin（那是 ffmpeg 的参数），加上会直接报错退出。
    let out = no_window(Command::new(ffprobe))
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "json",
            &raw,
        ])
        .output()
        .map_err(|e| AppError::FfmpegProbeStartFailed { detail: e.to_string() })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr.chars().rev().take(200).collect::<String>().chars().rev().collect();
        return Err(AppError::FfmpegProbeParseFailed {
            detail: format!("ffprobe exited {}: {}", out.status.code().unwrap_or(-1), tail.trim()),
        });
    }
    let text = String::from_utf8(out.stdout)
        .map_err(|e| AppError::FfmpegProbeParseFailed { detail: format!("non-UTF-8 output: {e}") })?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| AppError::FfmpegProbeParseFailed { detail: e.to_string() })?;
    // ffprobe 把 duration 输出为 JSON 字符串（如 "1322.656000"），需用 as_str + parse 解析。
    value
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str().and_then(|s| s.parse::<f64>().ok()))
        .ok_or_else(|| AppError::FfmpegProbeParseFailed { detail: "missing format.duration".into() })
}

/// 解析 ffmpeg `silencedetect` 在 stderr 输出的静音区间（纯函数，便于测试）。
/// `total_duration` 用于补齐"到文件结束都在静音"的尾段。
fn parse_silences(text: &str, total_duration: f64) -> Vec<Silence> {
    let mut out: Vec<Silence> = Vec::new();
    for line in text.lines() {
        if let Some(pos) = line.find("silence_start:") {
            if let Some(v) = line[pos + "silence_start:".len()..]
                .split_whitespace()
                .next()
                .and_then(|t| t.parse::<f64>().ok())
            {
                // 先占位，等对应的 silence_end。
                out.push(Silence { start: v, end: f64::NAN });
            }
        } else if let Some(pos) = line.find("silence_end:") {
            if let Some(v) = line[pos + "silence_end:".len()..]
                .split_whitespace()
                .next()
                .and_then(|t| t.parse::<f64>().ok())
            {
                if let Some(last) = out.iter_mut().rev().find(|s| s.end.is_nan()) {
                    last.end = v;
                }
            }
        }
    }
    for s in out.iter_mut() {
        if s.end.is_nan() {
            s.end = total_duration.max(s.start);
        }
    }
    out.retain(|s| s.end > s.start);
    out
}

/// 自适应静音阈值检测：先量整段音频的平均响度，再定 silencedetect 阈值。
///
/// 固定 -35dB 的问题：BGM 重的片子底噪常在 -30dB 上下，停顿被 BGM「埋住」检不出，
/// 切点被迫落在说话中。这里按 `mean_volume + 4dB` 取阈值并钳到 [-45, -28]dB——
/// 底噪高时阈值抬高（BGM 间隙也算停顿），安静片源时阈值保持宽松。
/// 测量失败退回固定 -35dB。
pub fn detect_silences_adaptive(
    wav_path: &Path,
    min_silence_seconds: f64,
    ffmpeg: &Path,
    ffprobe: &Path,
) -> Vec<Silence> {
    let noise_db = mean_volume_db(wav_path, ffmpeg)
        .map(|m| (m + 4.0).clamp(-45.0, -28.0))
        .unwrap_or(-35.0);
    detect_silences(wav_path, noise_db, min_silence_seconds, ffmpeg, ffprobe)
}

/// 用 ffmpeg volumedetect 读整段音频的平均响度（dB）。
fn mean_volume_db(wav_path: &Path, ffmpeg: &Path) -> Option<f64> {
    let arg = ffmpeg_arg(wav_path).ok()?;
    let out = no_window(Command::new(ffmpeg))
        .args(["-hide_banner", "-v", "info", "-nostdin", "-i", &arg, "-af", "volumedetect", "-f", "null", "-"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    for line in text.lines() {
        if let Some(pos) = line.find("mean_volume:") {
            if let Some(v) = line[pos + "mean_volume:".len()..]
                .split_whitespace()
                .next()
                .and_then(|t| t.parse::<f64>().ok())
            {
                return Some(v);
            }
        }
    }
    None
}

/// 用 ffmpeg silencedetect 检测语音停顿，供断句使用。
///
/// 失败（没有 ffmpeg / 解析不出）时返回空列表：调用方退化为按词间间隔断句，不影响主流程。
pub fn detect_silences(
    wav_path: &Path,
    noise_db: f64,
    min_silence_seconds: f64,
    ffmpeg: &Path,
    ffprobe: &Path,
) -> Vec<Silence> {
    let Ok(arg) = ffmpeg_arg(wav_path) else {
        return Vec::new();
    };
    let filter = format!("silencedetect=noise={noise_db}dB:d={min_silence_seconds}");
    let Ok(out) = no_window(Command::new(ffmpeg))
        .args(["-hide_banner", "-v", "info", "-nostdin", "-i", &arg, "-af", &filter, "-f", "null", "-"])
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stderr);
    let total = audio_duration(wav_path, ffprobe).unwrap_or(0.0);
    parse_silences(&text, total)
}

#[cfg(test)]
mod silence_tests {
    use super::*;

    const SAMPLE: &str = "\
[silencedetect @ 0x7f] silence_start: 0
[silencedetect @ 0x7f] silence_end: 0.342813 | silence_duration: 0.342813
[silencedetect @ 0x7f] silence_start: 1.930063
[silencedetect @ 0x7f] silence_end: 4.53725 | silence_duration: 2.607188
[silencedetect @ 0x7f] silence_start: 1300.5
";

    #[test]
    fn parses_pairs_and_open_tail() {
        let s = parse_silences(SAMPLE, 1322.656);
        assert_eq!(s.len(), 3);
        assert!((s[0].start - 0.0).abs() < 1e-6 && (s[0].end - 0.342813).abs() < 1e-6);
        assert!((s[1].start - 1.930063).abs() < 1e-6 && (s[1].end - 4.53725).abs() < 1e-6);
        // 结尾仍在静音：延伸到音频末尾。
        assert!((s[2].start - 1300.5).abs() < 1e-6 && (s[2].end - 1322.656).abs() < 1e-6);
    }

    #[test]
    fn ignores_garbage_lines() {
        assert!(parse_silences("no silences here\n", 10.0).is_empty());
    }

    /// plan_chunks：优先静音中点切；超长无静音段走 forced 重叠；片长 ≤2T。
    #[test]
    fn plan_chunks_prefers_silence_midpoints() {
        let silences = vec![
            Silence { start: 9.8, end: 10.4 },   // 中点 10.1，贴近理想切点 10
            Silence { start: 19.0, end: 19.6 },  // 中点 19.3
        ];
        let plan = plan_chunks(35.0, 10.0, &silences);
        assert_eq!(plan.len(), 3, "{plan:?}");
        // 边界落在静音中点，无重叠（keep == audio 区间）。
        assert!((plan[0].0 - 0.0).abs() < 1e-6 && (plan[0].2 - 0.0).abs() < 1e-6);
        assert!((plan[0].1 - 10.1).abs() < 1e-6 && (plan[0].3 - 10.1).abs() < 1e-6);
        assert!((plan[1].0 - 10.1).abs() < 1e-6 && (plan[1].2 - 10.1).abs() < 1e-6);
        assert!((plan[1].1 - 19.3).abs() < 1e-6 && (plan[1].3 - 19.3).abs() < 1e-6);
        // 尾部 15.7s ≤ 2T：最后一片直接收到结尾。
        assert!((plan[2].0 - 19.3).abs() < 1e-6 && (plan[2].1 - 35.0).abs() < 1e-6);
    }

    /// plan_chunks：窗口内无静音时在 2T 硬切，两侧各留 1s 重叠 margin。
    #[test]
    fn plan_chunks_forced_overlap_when_no_silence() {
        let plan = plan_chunks(50.0, 10.0, &[]);
        // 50s / 无静音 → 切点 20、40（各 forced），共 3 片。
        assert_eq!(plan.len(), 3, "{plan:?}");
        // chunk0：音频 [0,21]，keep [0,20]
        assert!((plan[0].0 - 0.0).abs() < 1e-6 && (plan[0].1 - 21.0).abs() < 1e-6);
        assert!((plan[0].2 - 0.0).abs() < 1e-6 && (plan[0].3 - 20.0).abs() < 1e-6);
        // chunk1：音频 [19,41]，keep [20,40]
        assert!((plan[1].0 - 19.0).abs() < 1e-6 && (plan[1].1 - 41.0).abs() < 1e-6);
        assert!((plan[1].2 - 20.0).abs() < 1e-6 && (plan[1].3 - 40.0).abs() < 1e-6);
        // chunk2（尾片）：音频 [39,50]，keep [40,50]
        assert!((plan[2].0 - 39.0).abs() < 1e-6 && (plan[2].1 - 50.0).abs() < 1e-6);
        assert!((plan[2].2 - 40.0).abs() < 1e-6 && (plan[2].3 - 50.0).abs() < 1e-6);
    }

    /// plan_chunks：余量 2.5T 内无静音时对半切而不是切出碎尾。
    #[test]
    fn plan_chunks_halves_tail_without_silence() {
        let plan = plan_chunks(24.0, 10.0, &[]); // 剩余 24 ∈ (2T, 2.5T]
        assert_eq!(plan.len(), 2, "{plan:?}");
        assert!((plan[0].3 - 12.0).abs() < 1e-6, "对半切在 12s: {plan:?}");
        assert!((plan[1].2 - 12.0).abs() < 1e-6);
    }

    /// plan_chunks：歌剧式连续音频——单片仍 ≤2T，边界全部 forced 带重叠。
    #[test]
    fn plan_chunks_bounded_on_continuous_audio() {
        let plan = plan_chunks(300.0, 10.0, &[]);
        for (s, e, ks, ke) in &plan {
            assert!(ke - ks <= 20.0 + 1e-6, "keep 区间超长: {ks}-{ke}");
            assert!(e - s <= 22.0 + 1e-6, "音频区间超长: {s}-{e}");
        }
        // keep 区间无缝覆盖全片。
        assert!((plan[0].2 - 0.0).abs() < 1e-6);
        for w in plan.windows(2) {
            assert!((w[0].3 - w[1].2).abs() < 1e-6, "keep 区间断开: {:?}", w);
        }
    }

    /// 测试内自行从 PATH 解析 ffmpeg/ffprobe——`find_ffmpeg` 依赖 `APP_DATA_DIR`，
    /// 该 OnceLock 只在 `run()` 里初始化，测试进程里恒返回 NotFound。
    fn path_lookup(name: &str) -> Option<PathBuf> {
        let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|d| d.join(&exe))
            .find(|p| p.is_file())
    }

    /// 真实音频上的冒烟测试（ffmpeg/ffprobe 或素材缺失时跳过）。
    #[test]
    fn detects_silences_on_extracted_audio() {
        let wav = Path::new("/Volumes/2t/code/polly-subtitle-translator/srt-converter-demo/extracted/_track1.wav");
        if !wav.exists() {
            eprintln!("skip: extracted audio missing");
            return;
        }
        let (Some(ffmpeg), Some(ffprobe)) = (path_lookup("ffmpeg"), path_lookup("ffprobe")) else {
            eprintln!("skip: ffmpeg/ffprobe not on PATH");
            return;
        };
        let s = detect_silences(wav, -35.0, 0.25, &ffmpeg, &ffprobe);
        assert!(s.len() > 50, "22 分钟对白应检出大量停顿，实际 {}", s.len());
        for x in &s {
            assert!(x.end > x.start);
        }
    }
}
