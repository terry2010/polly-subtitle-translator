// FFmpeg/FFprobe 封装层
// 功能：probe_video（探测视频信息）+ extract_subtitle_stream（提取字幕流）

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 查找 FFmpeg 可执行文件路径
/// 优先级：用户自定义路径 > 系统 PATH 中的 ffmpeg
pub fn find_ffmpeg(custom_path: Option<&str>) -> Result<PathBuf, AppError> {
    if let Some(p) = custom_path {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(AppError::FfmpegNotFound {
            path: p.to_string(),
        });
    }

    // 查找系统 PATH 中的 ffmpeg
    let result = which_ffmpeg("ffmpeg");
    result.ok_or_else(|| AppError::FfmpegNotFound {
        path: "ffmpeg".to_string(),
    })
}

/// 查找 FFprobe 可执行文件路径
pub fn find_ffprobe(custom_path: Option<&str>) -> Result<PathBuf, AppError> {
    if let Some(p) = custom_path {
        // 如果用户指定了 ffmpeg 路径，ffprobe 在同目录
        let ffprobe_path = PathBuf::from(p)
            .with_file_name("ffprobe")
            .with_extension(std::env::consts::EXE_EXTENSION);
        if ffprobe_path.exists() {
            return Ok(ffprobe_path);
        }
    }

    which_ffmpeg("ffprobe").ok_or_else(|| AppError::FfmpegNotFound {
        path: "ffprobe".to_string(),
    })
}

fn which_ffmpeg(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full_path = dir.join(&exe);
            if full_path.is_file() {
                Some(full_path)
            } else {
                None
            }
        })
    })
}

// === SECTION 1 END ===

/// 视频流信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStream {
    pub index: i32,
    pub codec_name: String,
    pub codec_long_name: String,
    pub profile: Option<String>,
    pub width: i32,
    pub height: i32,
    pub pix_fmt: String,
    pub r_frame_rate: String,
    pub avg_frame_rate: String,
    pub duration: Option<f64>,
    pub bit_rate: Option<i64>,
    pub bits_per_raw_sample: Option<String>,
    pub color_space: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub hdr_info: Option<HdrInfo>,
}

/// HDR/杜比信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdrInfo {
    pub is_hdr: bool,
    pub is_dolby_vision: bool,
    pub hdr_format: String, // HDR10 / HDR10+ / Dolby Vision / HLG / SDR
    pub details: String,
}

/// 音频流信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStream {
    pub index: i32,
    pub codec_name: String,
    pub codec_long_name: String,
    pub sample_rate: i32,
    pub channels: i32,
    pub channel_layout: Option<String>,
    pub duration: Option<f64>,
    pub bit_rate: Option<i64>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub disposition_default: bool,
}

/// 字幕流信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleStream {
    pub index: i32,
    pub codec_name: String,
    pub codec_long_name: String,
    pub duration: Option<f64>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub disposition_default: bool,
    pub disposition_forced: bool,
    pub disposition_hearing_impaired: bool,
    pub is_graphic: bool, // PGS/DVB 等图形字幕
}

/// 视频格式信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFormat {
    pub format_name: String,
    pub format_long_name: String,
    pub duration: Option<f64>,
    pub size: Option<i64>,
    pub bit_rate: Option<i64>,
}

/// probe_video 完整返回
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub video_path: String,
    pub format: VideoFormat,
    pub video_stream: Option<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
    pub subtitle_streams: Vec<SubtitleStream>,
}

// === SECTION 2 END ===

/// ffprobe JSON 输出结构（反序列化用）
#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: FfprobeFormat,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    index: i32,
    codec_name: String,
    codec_long_name: String,
    codec_type: String,
    profile: Option<String>,
    #[serde(default)]
    width: i32,
    #[serde(default)]
    height: i32,
    pix_fmt: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
    bits_per_raw_sample: Option<String>,
    sample_rate: Option<serde_json::Value>,
    channels: Option<serde_json::Value>,
    channel_layout: Option<String>,
    color_space: Option<String>,
    color_transfer: Option<String>,
    color_primaries: Option<String>,
    #[serde(default)]
    disposition: serde_json::Value,
    #[serde(default)]
    tags: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    format_name: String,
    format_long_name: String,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
}

fn parse_f64(s: &Option<String>) -> Option<f64> {
    s.as_ref().and_then(|v| v.parse().ok())
}

fn parse_i64(s: &Option<String>) -> Option<i64> {
    s.as_ref().and_then(|v| v.parse().ok())
}

/// 从 serde_json::Value 解析整数（兼容字符串和整数类型）
fn parse_json_int(v: &Option<serde_json::Value>) -> Option<i32> {
    v.as_ref().and_then(|val| {
        if let Some(i) = val.as_i64() {
            Some(i as i32)
        } else if let Some(s) = val.as_str() {
            s.parse().ok()
        } else {
            None
        }
    })
}

fn get_tag(tags: &serde_json::Value, key: &str) -> Option<String> {
    tags.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_disposition(disp: &serde_json::Value, key: &str) -> bool {
    disp.get(key)
        .and_then(|v| v.as_i64())
        .map(|v| v != 0)
        .unwrap_or(false)
}

/// 检测 HDR 信息
fn detect_hdr(stream: &FfprobeStream) -> Option<HdrInfo> {
    let color_transfer = stream.color_transfer.as_deref().unwrap_or("");
    let color_primaries = stream.color_primaries.as_deref().unwrap_or("");
    let color_space = stream.color_space.as_deref().unwrap_or("");

    // HDR10: smpte2084 (PQ) transfer + bt2020 primaries
    let is_hdr10 = color_transfer == "smpte2084" && color_primaries == "bt2020";
    // HLG: ARIB STD-B67 transfer
    let is_hlg = color_transfer == "arib-std-b67";
    // Dolby Vision: 通常有 side_data 包含 RPU，这里简化检测（codec_name 含 dvhe/dav1）
    let is_dolby_vision = stream.codec_name.contains("dv")
        || stream.codec_name.contains("dovi")
        || stream.profile.as_deref().unwrap_or("").contains("Dolby");

    if is_hdr10 || is_hlg || is_dolby_vision {
        let format = if is_dolby_vision {
            "Dolby Vision"
        } else if is_hdr10 {
            // HDR10+ 检测需要 side_data，这里简化为 HDR10
            "HDR10"
        } else {
            "HLG"
        };

        Some(HdrInfo {
            is_hdr: true,
            is_dolby_vision,
            hdr_format: format.to_string(),
            details: format!(
                "color_space={}, transfer={}, primaries={}",
                color_space, color_transfer, color_primaries
            ),
        })
    } else {
        None
    }
}

/// 判断字幕流是否为图形字幕
fn is_graphic_subtitle(codec_name: &str) -> bool {
    matches!(
        codec_name,
        "hdmv_pgs_subtitle"
            | "dvd_subtitle"
            | "dvb_subtitle"
            | "hdmv_text_subtitle"
    )
}

// === SECTION 3 END ===

/// 探测视频文件信息
pub fn probe_video(
    video_path: &str,
    ffmpeg_custom_path: Option<&str>,
) -> Result<ProbeResult, AppError> {
    let ffprobe = find_ffprobe(ffmpeg_custom_path)?;

    if !Path::new(video_path).exists() {
        return Err(AppError::FileNotFound {
            path: video_path.to_string(),
        });
    }

    let output = Command::new(&ffprobe)
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            video_path,
        ])
        .output()
        .map_err(|e| AppError::FfmpegExecutionFailed {
            detail: format!("ffprobe 启动失败: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::FfmpegProbeFailed {
            video_path: video_path.to_string(),
        })
        .map_err(|e| {
            tracing::error!("ffprobe 失败: {}", stderr);
            e
        });
    }

    let ffprobe_output: FfprobeOutput = serde_json::from_slice(&output.stdout).map_err(|e| {
        AppError::FfmpegExecutionFailed {
            detail: format!("ffprobe 输出解析失败: {}", e),
        }
    })?;

    let mut video_stream: Option<VideoStream> = None;
    let mut audio_streams: Vec<AudioStream> = Vec::new();
    let mut subtitle_streams: Vec<SubtitleStream> = Vec::new();

    for stream in &ffprobe_output.streams {
        match stream.codec_type.as_str() {
            "video" => {
                let hdr_info = detect_hdr(stream);
                let vs = VideoStream {
                    index: stream.index,
                    codec_name: stream.codec_name.clone(),
                    codec_long_name: stream.codec_long_name.clone(),
                    profile: stream.profile.clone(),
                    width: stream.width,
                    height: stream.height,
                    pix_fmt: stream.pix_fmt.clone().unwrap_or_default(),
                    r_frame_rate: stream.r_frame_rate.clone().unwrap_or_default(),
                    avg_frame_rate: stream.avg_frame_rate.clone().unwrap_or_default(),
                    duration: parse_f64(&stream.duration),
                    bit_rate: parse_i64(&stream.bit_rate),
                    bits_per_raw_sample: stream.bits_per_raw_sample.clone(),
                    color_space: stream.color_space.clone(),
                    color_transfer: stream.color_transfer.clone(),
                    color_primaries: stream.color_primaries.clone(),
                    hdr_info,
                };
                // 取第一个视频流
                if video_stream.is_none() {
                    video_stream = Some(vs);
                }
            }
            "audio" => {
                let lang = get_tag(&stream.tags, "language");
                let title = get_tag(&stream.tags, "title");
                audio_streams.push(AudioStream {
                    index: stream.index,
                    codec_name: stream.codec_name.clone(),
                    codec_long_name: stream.codec_long_name.clone(),
                    sample_rate: parse_json_int(&stream.sample_rate).unwrap_or(0),
                    channels: parse_json_int(&stream.channels).unwrap_or(0),
                    channel_layout: stream.channel_layout.clone(),
                    duration: parse_f64(&stream.duration),
                    bit_rate: parse_i64(&stream.bit_rate),
                    language: lang,
                    title,
                    disposition_default: get_disposition(&stream.disposition, "default"),
                });
            }
            "subtitle" => {
                let lang = get_tag(&stream.tags, "language");
                let title = get_tag(&stream.tags, "title");
                let is_graphic = is_graphic_subtitle(&stream.codec_name);
                subtitle_streams.push(SubtitleStream {
                    index: stream.index,
                    codec_name: stream.codec_name.clone(),
                    codec_long_name: stream.codec_long_name.clone(),
                    duration: parse_f64(&stream.duration),
                    language: lang,
                    title,
                    disposition_default: get_disposition(&stream.disposition, "default"),
                    disposition_forced: get_disposition(&stream.disposition, "forced"),
                    disposition_hearing_impaired: get_disposition(
                        &stream.disposition,
                        "hearing_impaired",
                    ),
                    is_graphic,
                });
            }
            _ => {}
        }
    }

    let format = VideoFormat {
        format_name: ffprobe_output.format.format_name,
        format_long_name: ffprobe_output.format.format_long_name,
        duration: parse_f64(&ffprobe_output.format.duration),
        size: parse_i64(&ffprobe_output.format.size),
        bit_rate: parse_i64(&ffprobe_output.format.bit_rate),
    };

    Ok(ProbeResult {
        video_path: video_path.to_string(),
        format,
        video_stream,
        audio_streams,
        subtitle_streams,
    })
}

// === SECTION 4 END ===

/// 提取字幕流到文件
/// stream_index: 字幕流索引
/// output_path: 输出文件路径（扩展名决定格式：.srt / .ass / .vtt）
pub fn extract_subtitle_stream(
    video_path: &str,
    stream_index: i32,
    output_path: &str,
    ffmpeg_custom_path: Option<&str>,
) -> Result<(), AppError> {
    let ffmpeg = find_ffmpeg(ffmpeg_custom_path)?;

    if !Path::new(video_path).exists() {
        return Err(AppError::FileNotFound {
            path: video_path.to_string(),
        });
    }

    let output = Command::new(&ffmpeg)
        .args([
            "-y",
            "-i",
            video_path,
            "-map",
            &format!("0:{}", stream_index),
            "-c:s",
            "srt", // 文本字幕统一输出为 srt 格式
            output_path,
        ])
        .output()
        .map_err(|e| AppError::FfmpegExecutionFailed {
            detail: format!("ffmpeg 启动失败: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("字幕提取失败: {}", stderr);
        // 检测是否为图形字幕
        if stderr.contains("subtitle") && stderr.contains("filter") {
            return Err(AppError::FfmpegGraphicSubtitle {
                codec: "unknown".to_string(),
            });
        }
        return Err(AppError::FfmpegExtractFailed {
            detail: stderr.chars().take(500).collect(),
        });
    }

    tracing::info!("字幕提取成功: {} -> {}", video_path, output_path);
    Ok(())
}

/// 合并字幕到视频（-c copy + 字幕流映射）
pub fn merge_subtitle_to_video(
    video_path: &str,
    subtitle_path: &str,
    output_path: &str,
    language: Option<&str>,
    ffmpeg_custom_path: Option<&str>,
) -> Result<(), AppError> {
    let ffmpeg = find_ffmpeg(ffmpeg_custom_path)?;

    if !Path::new(video_path).exists() {
        return Err(AppError::FileNotFound {
            path: video_path.to_string(),
        });
    }
    if !Path::new(subtitle_path).exists() {
        return Err(AppError::FileNotFound {
            path: subtitle_path.to_string(),
        });
    }

    let subtitle_ext = Path::new(subtitle_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("srt");

    let subtitle_codec = match subtitle_ext {
        "ass" | "ssa" => "ass",
        "vtt" => "webvtt",
        _ => "srt",
    };

    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        video_path.to_string(),
        "-i".to_string(),
        subtitle_path.to_string(),
        "-c".to_string(),
        "copy".to_string(),
        "-map".to_string(),
        "0".to_string(),
        "-map".to_string(),
        "1".to_string(),
        format!("-c:s:{}", get_subtitle_stream_count(video_path, ffmpeg_custom_path)?),
        subtitle_codec.to_string(),
    ];

    if let Some(lang) = language {
        args.push(format!("-metadata:s:s:{}", get_subtitle_stream_count(video_path, ffmpeg_custom_path)?));
        args.push(format!("language={}", lang));
    }

    args.push(output_path.to_string());

    let output = Command::new(&ffmpeg)
        .args(&args)
        .output()
        .map_err(|e| AppError::FfmpegExecutionFailed {
            detail: format!("ffmpeg 启动失败: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("字幕合并失败: {}", stderr);
        return Err(AppError::FfmpegMergeFailed {
            detail: stderr.chars().take(500).collect(),
        });
    }

    tracing::info!("字幕合并成功: {} + {} -> {}", video_path, subtitle_path, output_path);
    Ok(())
}

/// 获取视频中已有字幕流数量
fn get_subtitle_stream_count(
    video_path: &str,
    ffmpeg_custom_path: Option<&str>,
) -> Result<i32, AppError> {
    let probe = probe_video(video_path, ffmpeg_custom_path)?;
    Ok(probe.subtitle_streams.len() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_graphic_subtitle() {
        assert!(is_graphic_subtitle("hdmv_pgs_subtitle"));
        assert!(is_graphic_subtitle("dvd_subtitle"));
        assert!(is_graphic_subtitle("dvb_subtitle"));
        assert!(!is_graphic_subtitle("subrip"));
        assert!(!is_graphic_subtitle("ass"));
        assert!(!is_graphic_subtitle("webvtt"));
    }

    #[test]
    fn test_detect_hdr_sdr() {
        let stream = FfprobeStream {
            index: 0,
            codec_name: "h264".to_string(),
            codec_long_name: "H.264".to_string(),
            codec_type: "video".to_string(),
            profile: Some("High".to_string()),
            width: 1920,
            height: 1080,
            pix_fmt: Some("yuv420p".to_string()),
            r_frame_rate: Some("24000/1001".to_string()),
            avg_frame_rate: Some("24000/1001".to_string()),
            duration: Some("1325.5".to_string()),
            bit_rate: Some("4689280".to_string()),
            bits_per_raw_sample: Some("8".to_string()),
            sample_rate: None,
            channels: None,
            channel_layout: None,
            color_space: Some("bt709".to_string()),
            color_transfer: Some("bt709".to_string()),
            color_primaries: Some("bt709".to_string()),
            disposition: serde_json::json!({}),
            tags: serde_json::json!({}),
        };
        let hdr = detect_hdr(&stream);
        assert!(hdr.is_none());
    }

    #[test]
    fn test_detect_hdr10() {
        let stream = FfprobeStream {
            index: 0,
            codec_name: "hevc".to_string(),
            codec_long_name: "H.265".to_string(),
            codec_type: "video".to_string(),
            profile: Some("Main 10".to_string()),
            width: 3840,
            height: 2160,
            pix_fmt: Some("yuv420p10le".to_string()),
            r_frame_rate: Some("24000/1001".to_string()),
            avg_frame_rate: Some("24000/1001".to_string()),
            duration: Some("1325.5".to_string()),
            bit_rate: None,
            bits_per_raw_sample: Some("10".to_string()),
            sample_rate: None,
            channels: None,
            channel_layout: None,
            color_space: Some("bt2020nc".to_string()),
            color_transfer: Some("smpte2084".to_string()),
            color_primaries: Some("bt2020".to_string()),
            disposition: serde_json::json!({}),
            tags: serde_json::json!({}),
        };
        let hdr = detect_hdr(&stream).unwrap();
        assert!(hdr.is_hdr);
        assert!(!hdr.is_dolby_vision);
        assert_eq!(hdr.hdr_format, "HDR10");
    }
}
