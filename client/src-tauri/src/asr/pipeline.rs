// 转写流水线编排：视频音轨 → 16k WAV → 静音检测 → 停顿对齐切片 → 识别后端
// → 重叠去重 → 断句合并 → SRT 写盘。进度经 `asr_progress` 事件回前端。
//
// 移植自 srt-converter-demo/src-tauri/src/commands.rs 的 run_generate_pipeline：
// 保留 speechnorm 归一化、缝隙回填（>8s 无词区间补识别）、stopped 语义；
// 差异：进度数据结构为前端约定的 {phase, progress, processed_secs, total_secs,
// speed, eta_secs, engine}，取消用调用方传入的 AtomicBool（IPC 层持有）。
use super::audio::{self, Chunk};
use super::cues::{self, Segment};
use super::engine::{self, Backend};
use super::qwen3;
use crate::error::AppError;
use crate::ffmpeg::{find_ffmpeg, find_ffprobe};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tauri::Emitter;

/// 弱对白在背景音乐下易被识别器漏掉：切片时统一做响度归一化。
const SPEECHNORM_FILTER: &str = "speechnorm";
/// 切片目标时长（秒）。
const CHUNK_TARGET_SECONDS: f64 = 10.0;
/// 缝隙回填：相邻词之间超过该秒数视为「可能有漏识别」，单独补识别。
const GAP_FILL_MIN_SECONDS: f64 = 8.0;
/// 回填片段两端各放宽的秒数。
const GAP_FILL_PAD_SECONDS: f64 = 0.5;

/// 转写参数（IPC 层组装后传入）。
pub struct TranscribeRequest {
    pub video_path: PathBuf,
    /// ffprobe 全局流索引（AudioStream.index）。
    pub stream_index: i64,
    pub backend: Backend,
    /// 已归一化的 locale（如 "en-US"）；IPC 层用 normalize_language 处理。
    pub language: String,
    /// Qwen3：VAD 人声预筛开关。
    pub use_vad: bool,
    /// Qwen3：模型目录名（None = 默认 0.6B）。
    pub model: Option<String>,
    /// Qwen3：device_map（cpu/cuda/mps/auto）。
    pub device: Option<String>,
    /// 切片 wav 输出目录（IPC 层给 extract_cache 或临时目录）。
    pub work_dir: PathBuf,
}

/// 转写结果。
#[derive(Debug, Clone, Serialize)]
pub struct TranscribeOutcome {
    /// 生成的 SRT 文件绝对路径（前端 loadSubtitle 加载）。
    pub srt_path: String,
    pub cue_count: usize,
    /// 被停止（cancel）时为 true——已识别部分照常落盘。
    pub stopped: bool,
    /// 已识别音频时长（秒）。
    pub recognized_secs: f64,
}

/// 进度事件载荷（前端按 phase 显示阶段文案）。
#[derive(Debug, Clone, Serialize)]
struct Progress {
    /// extract / chunk / recognize / gapfill / merge / stopped / done
    phase: &'static str,
    /// 0-100。
    progress: f64,
    /// 已处理音频秒数。
    processed_secs: f64,
    total_secs: f64,
    /// 识别速度倍率（音频秒/墙钟秒）；无法计算时为 0。
    speed: f64,
    /// 预计剩余秒数；无法计算时为 null。
    eta_secs: Option<f64>,
    engine: String,
}

fn emit(app: &tauri::AppHandle, p: &Progress) {
    let _ = app.emit("asr_progress", p);
}

fn cancelled(cancel: &AtomicBool) -> Result<(), AppError> {
    if cancel.load(Ordering::Relaxed) {
        Err(AppError::AsrCancelled)
    } else {
        Ok(())
    }
}

/// 主流水线。后端可用性检查（prepare / 模型检查）在切片前做——
/// 避免切片跑完才报"没授权/模型不存在"。
pub fn run_transcribe(
    req: &TranscribeRequest,
    app: &tauri::AppHandle,
    cancel: &AtomicBool,
) -> Result<TranscribeOutcome, AppError> {
    let result = run_transcribe_inner(req, app, cancel);
    // 工作目录（提取的 wav + 切片）是临时产物，成功/失败/取消都清理，避免磁盘泄漏。
    let _ = std::fs::remove_dir_all(&req.work_dir);
    result
}

fn run_transcribe_inner(
    req: &TranscribeRequest,
    app: &tauri::AppHandle,
    cancel: &AtomicBool,
) -> Result<TranscribeOutcome, AppError> {
    let engine_tag = req.backend.as_str().to_string();
    let ffmpeg = find_ffmpeg(None)?;
    let ffprobe = find_ffprobe(None)?;

    // ---- 1. 提取音轨 → 16k 单声道 wav ----
    emit(app, &Progress { phase: "extract", progress: 2.0, processed_secs: 0.0, total_secs: 0.0, speed: 0.0, eta_secs: None, engine: engine_tag.clone() });
    let wav = audio::extract_audio_track(&req.video_path, req.stream_index, &req.work_dir, &ffmpeg)?;
    cancelled(cancel)?;
    let total_secs = audio::audio_duration(&wav, &ffprobe).unwrap_or(0.0);

    // ---- 2. 静音检测（自适应阈值；失败时降级为空表走 1s 间隔兜底）----
    emit(app, &Progress { phase: "detect", progress: 6.0, processed_secs: 0.0, total_secs, speed: 0.0, eta_secs: None, engine: engine_tag.clone() });
    let silences = audio::detect_silences_adaptive(&wav, 0.25, &ffmpeg, &ffprobe);
    cancelled(cancel)?;

    // ---- 3. 切片 ----
    emit(app, &Progress { phase: "chunk", progress: 10.0, processed_secs: 0.0, total_secs, speed: 0.0, eta_secs: None, engine: engine_tag.clone() });
    let chunks = audio::split_chunks_at_silences(
        &wav, CHUNK_TARGET_SECONDS, &silences, Some(SPEECHNORM_FILTER), &ffmpeg, &ffprobe,
    )?;
    cancelled(cancel)?;

    // ---- 4. 识别 ----
    let started = Instant::now();
    let mut progress_cb = |processed: f64, phase: &'static str| {
        let elapsed = started.elapsed().as_secs_f64();
        let speed = if elapsed > 0.5 { processed / elapsed } else { 0.0 };
        let eta = if speed > 0.0 && total_secs > processed {
            Some((total_secs - processed) / speed)
        } else {
            None
        };
        let pct = 12.0 + (if total_secs > 0.0 { processed / total_secs } else { 0.0 }) * 76.0;
        emit(app, &Progress {
            phase, progress: pct.min(88.0), processed_secs: processed, total_secs,
            speed, eta_secs: eta, engine: engine_tag.clone(),
        });
    };

    let (mut all, mut stopped) = match req.backend {
        Backend::MacNative => {
            // 后端预检（授权 + 识别器可用性），失败/不可用立即报错。
            engine::prepare_mac_native(&req.language)
                .map_err(|detail| AppError::AsrUnavailable { detail })?;
            let mut items: Vec<(Segment, f64, f64)> = Vec::new();
            let mut stopped_mac = false;
            for (i, c) in chunks.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    stopped_mac = true;
                    break;
                }
                match engine::transcribe_file(&c.path, &req.language, Backend::MacNative) {
                    Ok(words) => {
                        // 词时间戳相对切片起点 → 平移到绝对时间。
                        for mut w in words {
                            w.start_seconds += c.start;
                            items.push((w, c.keep_start, c.keep_end));
                        }
                    }
                    // 单片失败不整体失败（静音片/边界片常见），记录后继续。
                    Err(e) => {
                        tracing::warn!("asr chunk {} failed: {}", i, e);
                    }
                }
                progress_cb(c.end.min(total_secs), "recognize");
            }
            (cues::collect_kept_words(items), stopped_mac)
        }
        Backend::Qwen3 => {
            if let Some(reason) = qwen3::qwen3_unavailable_reason(req.model.as_deref()) {
                return Err(AppError::AsrUnavailable { detail: reason });
            }
            let cb_processed = |chunk_idx: usize, _total: usize, _pct: f64| {
                // 侧边按批次回报 chunk 序号 → 折算成已处理音频秒数。
                let secs = chunks
                    .get(chunk_idx.saturating_sub(1).min(chunks.len().saturating_sub(1)))
                    .map(|c| c.end)
                    .unwrap_or(0.0);
                progress_cb(secs.min(total_secs), "recognize");
            };
            let run = qwen3::transcribe_qwen3_chunks(
                &chunks,
                &req.language,
                req.use_vad,
                req.model.as_deref(),
                req.device.as_deref(),
                Some(cancel),
                cb_processed,
            )
            .map_err(|detail| AppError::AsrTranscribeFailed { detail })?;
            (run.segments, run.stopped)
        }
    };

    if stopped {
        emit(app, &Progress { phase: "stopped", progress: 88.0, processed_secs: total_secs, total_secs, speed: 0.0, eta_secs: Some(0.0), engine: engine_tag.clone() });
    }

    // ---- 5. 缝隙回填（未停止时）：>8s 无词区间单独补识别 ----
    if !stopped {
        all = gapfill(&mut all, &wav, req, cancel, &mut progress_cb, total_secs)?;
        if cancel.load(Ordering::Relaxed) {
            stopped = true;
        }
    }

    // ---- 6. 合并 → SRT ----
    emit(app, &Progress { phase: "merge", progress: 92.0, processed_secs: total_secs, total_secs, speed: 0.0, eta_secs: Some(0.0), engine: engine_tag.clone() });
    let cue_list = cues::merge_into_cues(all, &silences);
    let recognized_secs = cue_list.iter().map(|c| c.duration_seconds).sum();
    if cue_list.is_empty() && !stopped {
        return Err(AppError::AsrTranscribeFailed {
            detail: "no speech recognized (audio may be silent or unsupported)".to_string(),
        });
    }
    let srt_path = cues::asr_cache_srt_path(
        &req.video_path,
        req.backend.output_tag(),
        req.model.as_deref(),
        &req.language,
    );
    let srt_text = cues::render_srt(&cue_list);
    std::fs::write(&srt_path, srt_text).map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
    emit(app, &Progress { phase: "done", progress: 100.0, processed_secs: total_secs, total_secs, speed: 0.0, eta_secs: Some(0.0), engine: engine_tag });
    Ok(TranscribeOutcome {
        srt_path: srt_path.to_string_lossy().to_string(),
        cue_count: cue_list.len(),
        stopped,
        recognized_secs,
    })
}

/// 纯函数：找出词间 ≥GAP_FILL_MIN_SECONDS 的空隙（含开头/结尾大段无词）。
fn find_gaps(segments: &[Segment], total_secs: f64) -> Vec<(f64, f64)> {
    let mut sorted: Vec<&Segment> = segments.iter().collect();
    sorted.sort_by(|a, b| a.start_seconds.partial_cmp(&b.start_seconds).unwrap_or(std::cmp::Ordering::Equal));
    let mut gaps = Vec::new();
    let mut prev_end = 0.0;
    for w in &sorted {
        if w.start_seconds - prev_end >= GAP_FILL_MIN_SECONDS {
            gaps.push((prev_end, w.start_seconds));
        }
        prev_end = prev_end.max(w.end_seconds());
    }
    if total_secs - prev_end >= GAP_FILL_MIN_SECONDS {
        gaps.push((prev_end, total_secs));
    }
    gaps
}

/// 缝隙回填：扫描已识别词之间 >8s 的无词区间，单独切音频补识别。
/// 设备端/小模型对 BGM 下对白保守（"No speech detected"），回填能救回不少漏识别。
/// 返回合并后的段落列表（原列表 + 回填词，按时间排序）。
#[allow(clippy::too_many_arguments)]
fn gapfill(
    all: &mut Vec<Segment>,
    wav: &Path,
    req: &TranscribeRequest,
    cancel: &AtomicBool,
    progress_cb: &mut dyn FnMut(f64, &'static str),
    total_secs: f64,
) -> Result<Vec<Segment>, AppError> {
    let gaps = find_gaps(all, total_secs);
    if gaps.is_empty() {
        return Ok(std::mem::take(all));
    }

    let ffmpeg = find_ffmpeg(None)?;
    let tag = qwen3::unique_tag();
    let gap_dir = wav.parent().unwrap_or_else(|| Path::new(".")).join(format!("gapfill_{tag}"));
    std::fs::create_dir_all(&gap_dir).map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    let mut gap_chunks: Vec<Chunk> = Vec::new();
    for (i, (a, b)) in gaps.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let start = (a - GAP_FILL_PAD_SECONDS).max(0.0);
        let end = (b + GAP_FILL_PAD_SECONDS).min(total_secs);
        let out = gap_dir.join(format!("gap_{i}.wav"));
        if let Err(e) = audio::cut_audio(wav, start, end, &out, Some(SPEECHNORM_FILTER), &ffmpeg) {
            tracing::warn!("gapfill cut failed [{start:.1}-{end:.1}]: {e}");
            continue;
        }
        gap_chunks.push(Chunk { path: out, start, end, keep_start: start, keep_end: end });
    }

    if !gap_chunks.is_empty() && !cancel.load(Ordering::Relaxed) {
        match req.backend {
            Backend::MacNative => {
                for c in &gap_chunks {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(words) = engine::transcribe_file(&c.path, &req.language, Backend::MacNative) {
                        for mut w in words {
                            w.start_seconds += c.start;
                            all.push(w);
                        }
                    }
                    progress_cb(c.end.min(total_secs), "gapfill");
                }
            }
            Backend::Qwen3 => {
                let run = qwen3::transcribe_qwen3_chunks(
                    &gap_chunks, &req.language, req.use_vad,
                    req.model.as_deref(), req.device.as_deref(), Some(cancel),
                    |i, _n, _p| {
                        let secs = gap_chunks.get(i.saturating_sub(1)).map(|c| c.end).unwrap_or(0.0);
                        progress_cb(secs.min(total_secs), "gapfill");
                    },
                );
                match run {
                    Ok(r) => all.extend(r.segments),
                    Err(e) => tracing::warn!("gapfill qwen3 failed: {e}"),
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&gap_dir);
    all.sort_by(|a, b| a.start_seconds.partial_cmp(&b.start_seconds).unwrap_or(std::cmp::Ordering::Equal));
    Ok(std::mem::take(all))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, dur: f64) -> Segment {
        Segment { text: "w".into(), start_seconds: start, duration_seconds: dur }
    }

    #[test]
    fn find_gaps_covers_middle_and_tail() {
        // 词：0-2s、40-42s；total 60 → 中段 [2,40] + 尾段 [42,60]。
        let gaps = find_gaps(&[seg(0.0, 2.0), seg(40.0, 2.0)], 60.0);
        assert_eq!(gaps, vec![(2.0, 40.0), (42.0, 60.0)]);
    }

    #[test]
    fn find_gaps_leading_silence_and_dense_audio() {
        // 开头 20s 无词 → [0,20]；之后词密集 → 无其他 gap。
        let gaps = find_gaps(&[seg(20.0, 1.0), seg(21.0, 1.0), seg(22.0, 1.0)], 25.0);
        assert_eq!(gaps, vec![(0.0, 20.0)]);
        // 无词 → 整段一个 gap [0,total]。
        assert_eq!(find_gaps(&[], 30.0), vec![(0.0, 30.0)]);
        // 词布满 → 无 gap。
        assert!(find_gaps(&[seg(0.0, 1.0), seg(1.0, 20.0), seg(21.0, 4.0)], 25.0).is_empty());
    }

    #[test]
    fn find_gaps_unsorted_input_is_sorted() {
        // 乱序输入也应正确（Qwen3 段落合并前可能乱序）。
        let gaps = find_gaps(&[seg(30.0, 1.0), seg(0.0, 1.0)], 40.0);
        assert_eq!(gaps, vec![(1.0, 30.0), (31.0, 40.0)]);
    }
}
