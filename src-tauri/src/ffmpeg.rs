// FFmpeg/FFprobe 封装层
// 功能：probe_video（探测视频信息）+ extract_subtitle_stream（提取字幕流）

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

/// 全局 app_data_dir，启动时由 lib.rs 初始化，供 find_ffmpeg 查找下载的 ffmpeg
static APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 初始化 app_data_dir（lib.rs 启动时调用）
pub fn init_app_data_dir(dir: PathBuf) {
    let _ = APP_DATA_DIR.set(dir);
}

/// 获取 ffmpeg 下载目录：app_data_dir/ffmpeg/
fn ffmpeg_download_dir() -> Option<PathBuf> {
    APP_DATA_DIR.get().map(|d| d.join("ffmpeg"))
}

/// 在 Windows release 模式下隐藏子进程控制台窗口
#[cfg(windows)]
fn no_window(mut cmd: Command) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
#[cfg(not(windows))]
fn no_window(cmd: Command) -> Command { cmd }

/// 全局提取取消标志：设为 true 时，正在运行的提取会尽快终止并返回错误
static EXTRACT_CANCELLED: AtomicBool = AtomicBool::new(false);

/// 取消所有正在进行的字幕提取（杀死 ffmpeg 进程）
pub fn cancel_extraction() {
    EXTRACT_CANCELLED.store(true, Ordering::Relaxed);
}

/// 重置取消标志（新的提取开始前调用）
fn reset_cancel_flag() {
    EXTRACT_CANCELLED.store(false, Ordering::Relaxed);
}

/// 杀死 ffmpeg 进程
fn kill_ffmpeg(child_id: u32) {
    if child_id == 0 { return; }
    #[cfg(windows)]
    let _ = no_window(Command::new("taskkill"))
        .args(["/PID", &child_id.to_string(), "/F", "/T"])
        .output();
    #[cfg(not(windows))]
    let _ = Command::new("kill").args(["-9", &child_id.to_string()]).output();
}

/// 查找 FFmpeg 可执行文件路径
/// 优先级：用户自定义路径 > app_data_dir 下载的 ffmpeg
/// 不使用系统 PATH 中的 ffmpeg（避免版本/功能不一致）
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

    // 只查 app_data_dir/ffmpeg/（按需下载的完整版 ffmpeg）
    if let Some(dir) = ffmpeg_download_dir() {
        let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        let downloaded = dir.join(exe_name);
        if downloaded.is_file() {
            return Ok(downloaded);
        }
    }

    Err(AppError::FfmpegNotFound {
        path: "ffmpeg".to_string(),
    })
}

/// 查找 FFprobe 可执行文件路径
/// 优先级：用户自定义路径 > app_data_dir 下载的 ffprobe
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

    // 只查 app_data_dir/ffmpeg/（按需下载的完整版 ffprobe）
    if let Some(dir) = ffmpeg_download_dir() {
        let exe_name = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
        let downloaded = dir.join(exe_name);
        if downloaded.is_file() {
            return Ok(downloaded);
        }
    }

    Err(AppError::FfmpegNotFound {
        path: "ffprobe".to_string(),
    })
}

/// 查找 exe 同目录下 bin/ 子目录中的可执行文件（内置打包）
/// 开发时查 src-tauri/bin/，发布后查 exe 同目录的 bin/
/// 当前未使用（改为按需下载模式），保留供开发模式 fallback
#[allow(dead_code)]
fn find_bundled_exe(name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    // 1. 查当前 exe 同目录的 bin/
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled = exe_dir.join("bin").join(&exe_name);
            if bundled.is_file() {
                return Some(bundled);
            }
            // Tauri resources 打包后可能在 resources/ 子目录
            let bundled_res = exe_dir.join("resources").join(&exe_name);
            if bundled_res.is_file() {
                return Some(bundled_res);
            }
        }
    }

    // 2. 开发模式下查 CARGO_MANIFEST_DIR/bin/（src-tauri/bin/）
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let dev_path = PathBuf::from(manifest_dir).join("bin").join(&exe_name);
        if dev_path.is_file() {
            return Some(dev_path);
        }
    }

    None
}

/// 在系统 PATH 中查找可执行文件
/// 当前未使用（改为按需下载模式），保留供未来需要时使用
#[allow(dead_code)]
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

// === SECTION 1.5: FFmpeg 按需下载 ===

/// FFmpeg 安装状态
#[derive(Debug, Clone, Serialize)]
pub struct FfmpegStatus {
    pub installed: bool,
    pub source: Option<String>, // "downloaded" | "bundled" | "system" | "custom"
    pub path: Option<String>,
}

impl FfmpegStatus {
    pub fn not_installed() -> Self {
        Self { installed: false, source: None, path: None }
    }
}

/// 检查 ffmpeg/ffprobe 是否可用（只检测下载目录）
pub fn get_ffmpeg_status() -> FfmpegStatus {
    if let Some(dir) = ffmpeg_download_dir() {
        let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        let path = dir.join(exe_name);
        if path.is_file() {
            return FfmpegStatus {
                installed: true,
                source: Some("downloaded".to_string()),
                path: Some(path.to_string_lossy().to_string()),
            };
        }
    }
    FfmpegStatus::not_installed()
}

/// FFmpeg 下载源列表（按优先级排序）
/// 1. BtbN GitHub Releases 经 gh-proxy 加速（国内可用）
/// 2. BtbN GitHub Releases 直连（国外/代理环境可用）
/// 3. gyan.dev（FFmpeg 官方推荐，国外源，速度较慢但稳定）
#[cfg(windows)]
const FFMPEG_DOWNLOAD_URLS: &[&str] = &[
    "https://gh-proxy.com/https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-full.7z",
];
/// macOS arm64 (Apple Silicon) 下载源
/// eugeneware/ffmpeg-static 提供单独的 gzip 压缩二进制文件（非归档）
/// release tag b6.1.1，提供 ffmpeg + ffprobe 两个独立 .gz 文件
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const FFMPEG_DOWNLOAD_URLS: &[&str] = &[
    "https://gh-proxy.com/https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-arm64.gz",
    "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-arm64.gz",
];
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const FFPROBE_DOWNLOAD_URLS: &[&str] = &[
    "https://gh-proxy.com/https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffprobe-darwin-arm64.gz",
    "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffprobe-darwin-arm64.gz",
];
/// macOS x86_64 (Intel) 下载源
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const FFMPEG_DOWNLOAD_URLS: &[&str] = &[
    "https://gh-proxy.com/https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-x64.gz",
    "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-x64.gz",
];
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const FFPROBE_DOWNLOAD_URLS: &[&str] = &[
    "https://gh-proxy.com/https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffprobe-darwin-x64.gz",
    "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffprobe-darwin-x64.gz",
];
/// Linux 下载源（保留兼容）
#[cfg(all(not(windows), not(target_os = "macos")))]
const FFMPEG_DOWNLOAD_URLS: &[&str] = &[
    "https://gh-proxy.com/https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz",
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz",
];

/// 下载 FFmpeg：从 gyan.dev 下载完整版，解压提取 ffmpeg.exe + ffprobe.exe
/// 流式下载（emit 进度事件），解压后提取 exe 到 app_data_dir/ffmpeg/
pub fn download_ffmpeg(
    proxy: Option<&str>,
    app_handle: &tauri::AppHandle,
) -> Result<(), AppError> {
    use tauri::Emitter;

    let result = download_ffmpeg_inner(proxy, app_handle);

    if let Err(ref e) = result {
        // 清理半成品
        if let Some(dir) = ffmpeg_download_dir() {
            let _ = fs::remove_dir_all(&dir);
        }
        let ipc_err = e.to_ipc_error();
        let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
            "stage": "failed", "progress": 0,
            "code": ipc_err.code,
            "args": ipc_err.args,
        }));
        tracing::warn!("ffmpeg 下载失败: code={}", ipc_err.code);
    }

    result
}

/// macOS 专用下载：eugeneware/ffmpeg-static 提供单独的 gzip 压缩二进制文件
/// 需要分别下载 ffmpeg.gz 和 ffprobe.gz，gunzip 后放到 ffmpeg 目录
#[cfg(target_os = "macos")]
fn download_ffmpeg_macos(
    dir: &Path,
    proxy: Option<&str>,
    app_handle: &tauri::AppHandle,
) -> Result<(), AppError> {
    use tauri::Emitter;
    use std::io::{Read, Write};

    let mut client_builder = reqwest::blocking::Client::builder()
        .user_agent("zimufan/1.0")
        .timeout(std::time::Duration::from_secs(600));
    if let Some(p) = proxy {
        if !p.is_empty() {
            client_builder = client_builder.proxy(
                reqwest::Proxy::all(p).map_err(|e| AppError::FfmpegDownloadProxyFailed {
                    detail: e.to_string(),
                })?,
            );
        }
    }
    let client = client_builder.build().map_err(|e| AppError::FfmpegDownloadHttpClientFailed {
        detail: e.to_string(),
    })?;

    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "downloading", "progress": 0, "message": "开始下载 FFmpeg..."
    }));

    // 下载单个 .gz 文件，返回下载的文件路径
    let download_gz = |urls: &[&str], out_name: &str, label: &str| -> Result<std::path::PathBuf, AppError> {
        let mut last_err = None;
        for url in urls {
            tracing::info!("尝试下载 {}: {}", label, url);
            let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
                "stage": "downloading", "progress": 0, "message": format!("下载{}...", label),
            }));
            match client.get(*url).send() {
                Ok(resp) if resp.status().is_success() => {
                    let total_size = resp.content_length().unwrap_or(0);
                    let gz_path = dir.join(format!("{}.gz", out_name));
                    let mut file = fs::File::create(&gz_path).map_err(|e| AppError::FfmpegDownloadCreateFileFailed {
                        detail: e.to_string(),
                    })?;
                    let mut stream = resp;
                    let mut buf = [0u8; 65536];
                    let mut downloaded: u64 = 0;
                    let mut last_emit = std::time::Instant::now();
                    let download_start = std::time::Instant::now();
                    loop {
                        let n = stream.read(&mut buf).map_err(|e| AppError::FfmpegDownloadStreamReadFailed {
                            detail: e.to_string(),
                        })?;
                        if n == 0 { break; }
                        file.write_all(&buf[..n]).map_err(|e| AppError::FfmpegDownloadWriteFailed {
                            detail: e.to_string(),
                        })?;
                        downloaded += n as u64;
                        if last_emit.elapsed() > std::time::Duration::from_millis(200) {
                            let pct = if total_size > 0 { (downloaded * 100 / total_size) as u8 } else { 0 };
                            let elapsed = download_start.elapsed().as_secs_f64();
                            let speed_bps = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
                            let speed_mb = speed_bps / 1024.0 / 1024.0;
                            let remaining_bytes = total_size.saturating_sub(downloaded);
                            let eta_secs = if speed_bps > 0.0 { remaining_bytes as f64 / speed_bps } else { 0.0 };
                            let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
                                "stage": "downloading", "progress": pct,
                                "downloaded": downloaded, "total": total_size,
                                "speed_mbps": (speed_mb * 10.0).round() / 10.0,
                                "eta_secs": eta_secs.round() as u64,
                                "message": format!("下载{} {}% ({} / {} MB)",
                                    label, pct, downloaded / 1024 / 1024, total_size / 1024 / 1024)
                            }));
                            last_emit = std::time::Instant::now();
                        }
                    }
                    return Ok(gz_path);
                }
                Ok(resp) => {
                    tracing::warn!("下载源 {} 返回 HTTP {}", url, resp.status());
                    last_err = Some(format!("HTTP {}", resp.status()));
                }
                Err(e) => {
                    tracing::warn!("下载源 {} 连接失败: {}", url, e);
                    last_err = Some(e.to_string());
                }
            }
        }
        Err(AppError::FfmpegDownloadRequestFailed {
            detail: last_err.unwrap_or_else(|| format!("所有{}下载源均不可用", label)),
        })
    };

    // 1. 下载 ffmpeg.gz
    let ffmpeg_gz = download_gz(FFMPEG_DOWNLOAD_URLS, "ffmpeg", "ffmpeg")?;
    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "downloading", "progress": 100, "message": "ffmpeg 下载完成"
    }));

    // 2. 下载 ffprobe.gz
    let ffprobe_gz = download_gz(FFPROBE_DOWNLOAD_URLS, "ffprobe", "ffprobe")?;

    // 3. gunzip 解压
    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "extracting", "progress": -1, "message": "正在解压安装..."
    }));

    let gunzip_file = |gz_path: &Path, out_name: &str| -> Result<(), AppError> {
        let out_path = dir.join(out_name);
        // 用系统 gunzip 命令解压
        let output = no_window(std::process::Command::new("gunzip"))
            .arg("-f")
            .arg(gz_path)
            .output()
            .map_err(|e| AppError::FfmpegDownloadExtractFailed {
                detail: format!("gunzip 失败: {}", e),
            })?;
        if !output.status.success() {
            return Err(AppError::FfmpegDownloadExtractFailed {
                detail: format!("gunzip 失败: {}", String::from_utf8_lossy(&output.stderr)),
            });
        }
        // gunzip 会把 ffmpeg.gz 解压为 ffmpeg（同目录）
        if !out_path.exists() {
            return Err(AppError::FfmpegDownloadExeNotFound);
        }
        Ok(())
    };

    gunzip_file(&ffmpeg_gz, "ffmpeg")?;
    gunzip_file(&ffprobe_gz, "ffprobe")?;

    // 4. 设置可执行权限
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["ffmpeg", "ffprobe"] {
            let path = dir.join(name);
            if path.exists() {
                let mut perms = fs::metadata(&path)
                    .map_err(|e| AppError::FfmpegDownloadCopyFailed { detail: e.to_string() })?
                    .permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }
    }

    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "done", "progress": 100, "message": "安装完成"
    }));
    tracing::info!("ffmpeg 下载完成: {}", dir.display());
    Ok(())
}

fn download_ffmpeg_inner(
    proxy: Option<&str>,
    app_handle: &tauri::AppHandle,
) -> Result<(), AppError> {
    use tauri::Emitter;
    use std::io::{Read, Write};

    let dir = ffmpeg_download_dir().ok_or(AppError::FfmpegDownloadMkdirFailed {
        detail: "app_data_dir 未初始化".to_string(),
    })?;
    fs::create_dir_all(&dir).map_err(|e| AppError::FfmpegDownloadMkdirFailed {
        detail: e.to_string(),
    })?;

    // macOS：eugeneware/ffmpeg-static 提供单独的 gzip 二进制文件，走专用下载路径
    #[cfg(target_os = "macos")]
    {
        return download_ffmpeg_macos(&dir, proxy, app_handle);
    }

    // 以下为 Windows/Linux 的归档下载逻辑
    #[cfg(not(target_os = "macos"))]
    {
    use tauri::Emitter as _;
    use std::io::{Read as _, Write as _};

    let mut client_builder = reqwest::blocking::Client::builder()
        .user_agent("zimufan/1.0")
        .timeout(std::time::Duration::from_secs(600));
    if let Some(p) = proxy {
        if !p.is_empty() {
            client_builder = client_builder.proxy(
                reqwest::Proxy::all(p).map_err(|e| AppError::FfmpegDownloadProxyFailed {
                    detail: e.to_string(),
                })?,
            );
        }
    }
    let client = client_builder.build().map_err(|e| AppError::FfmpegDownloadHttpClientFailed {
        detail: e.to_string(),
    })?;

    // 1. 下载文件（依次尝试多个源）
    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "downloading", "progress": 0, "message": "开始下载 FFmpeg..."
    }));

    let mut response = None;
    let mut last_err = None;
    let mut used_url = "";
    for url in FFMPEG_DOWNLOAD_URLS {
        tracing::info!("尝试下载 ffmpeg: {}", url);
        let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
            "stage": "downloading", "progress": 0, "message": format!("连接下载源..."),
        }));
        match client.get(*url).send() {
            Ok(resp) if resp.status().is_success() => {
                response = Some(resp);
                used_url = url;
                break;
            }
            Ok(resp) => {
                tracing::warn!("下载源 {} 返回 HTTP {}", url, resp.status());
                last_err = Some(format!("HTTP {}", resp.status()));
            }
            Err(e) => {
                tracing::warn!("下载源 {} 连接失败: {}", url, e);
                last_err = Some(e.to_string());
            }
        }
    }
    let response = response.ok_or_else(|| AppError::FfmpegDownloadRequestFailed {
        detail: last_err.unwrap_or_else(|| "所有下载源均不可用".to_string()),
    })?;
    tracing::info!("使用下载源: {}", used_url);
    let total_size = response.content_length().unwrap_or(0);
    // 根据下载源判断压缩格式：zip / 7z / tar.xz
    let is_zip = used_url.ends_with(".zip");
    let is_tar_xz = used_url.ends_with(".tar.xz");
    let archive_ext = if is_zip { "zip" } else if is_tar_xz { "tar.xz" } else { "7z" };
    let archive_path = dir.join(format!("ffmpeg.{}", archive_ext));
    let mut file = fs::File::create(&archive_path).map_err(|e| AppError::FfmpegDownloadCreateFileFailed {
        detail: e.to_string(),
    })?;

    let mut stream = response;
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let download_start = std::time::Instant::now();
    loop {
        let n = stream.read(&mut buf).map_err(|e| AppError::FfmpegDownloadStreamReadFailed {
            detail: e.to_string(),
        })?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).map_err(|e| AppError::FfmpegDownloadWriteFailed {
            detail: e.to_string(),
        })?;
        downloaded += n as u64;
        if last_emit.elapsed() > std::time::Duration::from_millis(200) {
            let pct = if total_size > 0 { (downloaded * 100 / total_size) as u8 } else { 0 };
            let elapsed = download_start.elapsed().as_secs_f64();
            let speed_bps = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
            let speed_mb = speed_bps / 1024.0 / 1024.0;
            let remaining_bytes = total_size.saturating_sub(downloaded);
            let eta_secs = if speed_bps > 0.0 { remaining_bytes as f64 / speed_bps } else { 0.0 };
            let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
                "stage": "downloading", "progress": pct,
                "downloaded": downloaded, "total": total_size,
                "speed_mbps": (speed_mb * 10.0).round() / 10.0,
                "eta_secs": eta_secs.round() as u64,
                "message": format!("下载中 {}% ({} / {} MB)",
                    pct, downloaded / 1024 / 1024, total_size / 1024 / 1024)
            }));
            last_emit = std::time::Instant::now();
        }
    }
    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "downloading", "progress": 100, "message": "下载完成"
    }));

    // 2. 解压（根据下载源判断 zip 或 7z 格式）
    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "extracting", "progress": -1, "message": "正在解压安装..."
    }));
    tracing::info!("解压 ffmpeg 包 ({}): {}", archive_ext, archive_path.display());
    let extract_dir = dir.join("extract");
    let _ = fs::remove_dir_all(&extract_dir);
    fs::create_dir_all(&extract_dir).map_err(|e| AppError::FfmpegDownloadExtractFailed {
        detail: e.to_string(),
    })?;
    if is_zip {
        // BtbN zip 格式：内结构 ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe
        let file = std::fs::File::open(&archive_path).map_err(|e| AppError::FfmpegDownloadExtractFailed {
            detail: e.to_string(),
        })?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| AppError::FfmpegDownloadExtractFailed {
            detail: e.to_string(),
        })?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| AppError::FfmpegDownloadExtractFailed {
                detail: e.to_string(),
            })?;
            let outpath = match entry.enclosed_name() {
                Some(path) => extract_dir.join(path),
                None => continue,
            };
            if entry.is_dir() {
                fs::create_dir_all(&outpath).map_err(|e| AppError::FfmpegDownloadExtractFailed {
                    detail: e.to_string(),
                })?;
            } else {
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent).map_err(|e| AppError::FfmpegDownloadExtractFailed {
                        detail: e.to_string(),
                    })?;
                }
                let mut outfile = std::fs::File::create(&outpath).map_err(|e| AppError::FfmpegDownloadExtractFailed {
                    detail: e.to_string(),
                })?;
                std::io::copy(&mut entry, &mut outfile).map_err(|e| AppError::FfmpegDownloadExtractFailed {
                    detail: e.to_string(),
                })?;
            }
        }
    } else if is_tar_xz {
        // BtbN macOS/Linux tar.xz 格式：用系统 tar 命令解压
        // 内结构 ffmpeg-master-latest-macos-arm64-gpl/bin/ffmpeg
        let output = no_window(std::process::Command::new("tar"))
            .args(["-xf", &archive_path.to_string_lossy(), "-C", &extract_dir.to_string_lossy()])
            .output()
            .map_err(|e| AppError::FfmpegDownloadExtractFailed {
                detail: format!("tar 解压失败: {}", e),
            })?;
        if !output.status.success() {
            return Err(AppError::FfmpegDownloadExtractFailed {
                detail: format!("tar 解压失败: {}", String::from_utf8_lossy(&output.stderr)),
            });
        }
    } else {
        // gyan.dev 7z 格式
        sevenz_rust::decompress_file(&archive_path, &extract_dir)
            .map_err(|e| AppError::FfmpegDownloadExtractFailed {
                detail: e.to_string(),
            })?;
    }

    // 3. 查找 ffmpeg 和 ffprobe（在解压目录的 bin/ 子目录下）
    // Windows: ffmpeg.exe / ffprobe.exe，macOS/Linux: ffmpeg / ffprobe
    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "extracting", "progress": 90, "message": "正在安装..."
    }));
    let ffmpeg_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let ffprobe_name = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    let ffmpeg_src = find_file_in_dir(&extract_dir, ffmpeg_name)
        .ok_or(AppError::FfmpegDownloadExeNotFound)?;
    let ffprobe_src = find_file_in_dir(&extract_dir, ffprobe_name)
        .ok_or(AppError::FfmpegDownloadExeNotFound)?;

    // 4. 复制到 ffmpeg 目录
    fs::copy(&ffmpeg_src, dir.join(ffmpeg_name)).map_err(|e| AppError::FfmpegDownloadCopyFailed {
        detail: e.to_string(),
    })?;
    fs::copy(&ffprobe_src, dir.join(ffprobe_name)).map_err(|e| AppError::FfmpegDownloadCopyFailed {
        detail: e.to_string(),
    })?;

    // 4.5 macOS/Linux：设置可执行权限
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in [ffmpeg_name, ffprobe_name] {
            let path = dir.join(name);
            if path.exists() {
                let mut perms = fs::metadata(&path)
                    .map_err(|e| AppError::FfmpegDownloadCopyFailed { detail: e.to_string() })?
                    .permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }
    }

    // 5. 清理解压目录和 zip 文件
    let _ = fs::remove_dir_all(&extract_dir);
    let _ = fs::remove_file(&archive_path);

    let _ = app_handle.emit("ffmpeg_download_progress", serde_json::json!({
        "stage": "done", "progress": 100, "message": "安装完成"
    }));
    tracing::info!("ffmpeg 下载完成: {}", dir.display());
    Ok(())
    } // end #[cfg(not(target_os = "macos"))]
}

/// 在目录中递归查找指定文件名
fn find_file_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    if !dir.is_dir() { return None; }
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_in_dir(&path, name) {
                return Some(found);
            }
        } else if path.file_name()?.to_str() == Some(name) {
            return Some(path);
        }
    }
    None
}

/// 删除下载的 ffmpeg
pub fn delete_ffmpeg() -> Result<(), AppError> {
    if let Some(dir) = ffmpeg_download_dir() {
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| AppError::FfmpegDownloadDeleteFailed {
                detail: e.to_string(),
            })?;
            tracing::info!("已删除下载的 ffmpeg: {}", dir.display());
        }
    }
    Ok(())
}

// === SECTION 1.5 END ===

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

/// 判断字幕流是否为图形字幕（PGS/VOBSub/DVB 等位图字幕）
fn is_graphic_subtitle(codec_name: &str) -> bool {
    matches!(
        codec_name,
        "hdmv_pgs_subtitle"
            | "dvd_subtitle"
            | "dvb_subtitle"
            | "hdmv_text_subtitle"
    )
}

/// 判断字幕流是否为可提取的文本字幕（ffmpeg 能转为 SRT 的 codec）
/// 白名单方式：只有已知能提取为文本的 codec 才返回 true
fn is_extractable_text_subtitle(codec_name: &str) -> bool {
    matches!(
        codec_name,
        "subrip"           // SRT
            | "ass"        // ASS/SSA
            | "ssa"        // SSA
            | "webvtt"     // WebVTT
            | "mov_text"   // MP4 tx3g
            | "subviewer"  // SubViewer
            | "subviewer1" // SubViewer v1
            | "jacosub"    // JACOsub
            | "microdvd"   // MicroDVD
            | "sami"       // SAMI
            | "realtext"   // RealText
            | "stl"        // EBU STL
            | "aqt"        // AQTitle
            | "pjs"        // PJS
    )
}

// === SECTION 3 END ===

/// 将路径中的反斜杠替换为正斜杠
/// ffmpeg/ffprobe 在 Windows 上遇到路径中 ".mkv\" 这类模式时（文件名中含点+扩展名后跟目录分隔符）
/// 会报 "Invalid argument"，用正斜杠可避免此问题
fn normalize_path_for_ffmpeg(path: &str) -> String {
    path.replace('\\', "/")
}

/// 解析 ffmpeg -progress 输出行，返回进度百分比（0.0-100.0）
/// 输入示例：out_time_us=12345678 或 out_time=00:00:12.345
fn parse_ffmpeg_progress(line: &str, duration_sec: Option<f64>) -> Option<f64> {
    let duration = duration_sec?;
    if duration <= 0.0 {
        return None;
    }

    // 优先解析 out_time_us=（微秒）
    if let Some(rest) = line.strip_prefix("out_time_us=") {
        if let Ok(us) = rest.trim().parse::<f64>() {
            let pct = (us / 1_000_000.0 / duration * 100.0).min(100.0);
            return Some(pct.max(0.0));
        }
    }

    // 备选：解析 out_time=HH:MM:SS.xx
    if let Some(rest) = line.strip_prefix("out_time=") {
        let t = rest.trim();
        let parts: Vec<&str> = t.split(':').collect();
        if parts.len() == 3 {
            let h: f64 = parts[0].parse().ok()?;
            let m: f64 = parts[1].parse().ok()?;
            let s: f64 = parts[2].parse().ok()?;
            let total = h * 3600.0 + m * 60.0 + s;
            let pct = (total / duration * 100.0).min(100.0);
            return Some(pct.max(0.0));
        }
    }

    None
}

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

    let ffmpeg_path = normalize_path_for_ffmpeg(video_path);
    let output = no_window(Command::new(&ffprobe))
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            &ffmpeg_path,
        ])
        .output()
        .map_err(|e| AppError::FfmpegProbeStartFailed {
            detail: e.to_string(),
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
        AppError::FfmpegProbeParseFailed {
            detail: e.to_string(),
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
                // 不可提取的流（图形字幕或未知 codec）都标记为 graphic，
                // 前端会禁用按钮，避免触发 ffmpeg 提取浪费时间
                let is_graphic = is_graphic_subtitle(&stream.codec_name)
                    || !is_extractable_text_subtitle(&stream.codec_name);
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

/// 根据输出路径扩展名推断 ffmpeg 字幕编码器名称。
/// 返回 (codec_arg, format_arg?)：format_arg 仅在扩展名与原 codec 不一致时需要显式指定。
/// 扩展名 → codec：.srt→srt，.ass/.ssa→ass，.vtt→webvtt，其余默认 srt。
fn subtitle_codec_for_path(output_path: &str) -> &'static str {
    let ext = Path::new(output_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "ass" | "ssa" => "ass",
        "vtt" => "webvtt",
        _ => "srt",
    }
}

/// 提取字幕流到文件
/// stream_index: 字幕流索引
/// output_path: 输出文件路径（扩展名决定格式：.srt / .ass / .vtt）
pub fn extract_subtitle_stream(
    video_path: &str,
    stream_index: i32,
    output_path: &str,
    ffmpeg_custom_path: Option<&str>,
    duration_sec: Option<f64>,
    on_progress: Option<&dyn Fn(f64)>,
) -> Result<(), AppError> {
    // 重置取消标志（新的提取开始）
    reset_cancel_flag();
    let ffmpeg = find_ffmpeg(ffmpeg_custom_path)?;

    if !Path::new(video_path).exists() {
        return Err(AppError::FileNotFound {
            path: video_path.to_string(),
        });
    }

    // 路径规范化：反斜杠 → 正斜杠，避免 ffmpeg 在 Windows 上遇到 ".mkv\" 模式时报错
    let ffmpeg_video_path = normalize_path_for_ffmpeg(video_path);
    let ffmpeg_output_path = normalize_path_for_ffmpeg(output_path);

    // 根据输出扩展名选择字幕编码器，保留 ass/vtt 原格式
    let subtitle_codec = subtitle_codec_for_path(output_path);
    tracing::info!(
        "字幕提取输出格式: ext-based codec={} (output={})",
        subtitle_codec,
        output_path
    );

    // 启动 ffmpeg 进程（加 -progress pipe:1 让 ffmpeg 输出进度到 stdout）
    let mut child = no_window(Command::new(&ffmpeg))
        .args([
            "-y",
            "-progress", "pipe:1",
            "-i",
            &ffmpeg_video_path,
            "-map",
            &format!("0:{}", stream_index),
            "-c:s",
            subtitle_codec,
            &ffmpeg_output_path,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| AppError::FfmpegStartFailed {
            detail: e.to_string(),
        })?;

    let child_id = child.id();
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");

    // 子线程：读 stderr 收集错误信息
    let (err_tx, err_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        let _ = err_tx.send(buf);
    });

    // 主线程：逐行读 stdout 解析进度，同时检查取消/超时
    use std::io::BufRead;
    let stdout_reader = std::io::BufReader::new(&mut stdout);
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(300); // NAS 慢，5 分钟超时

    for line_result in stdout_reader.lines() {
        // 检查取消
        if EXTRACT_CANCELLED.load(Ordering::Relaxed) {
            tracing::info!("字幕提取被取消，杀死 ffmpeg 进程");
            kill_ffmpeg(child_id);
            return Err(AppError::FfmpegExtractCancelled);
        }
        // 检查超时
        if start.elapsed() >= timeout {
            tracing::error!("字幕提取超时（{}秒），杀死 ffmpeg 进程", timeout.as_secs());
            kill_ffmpeg(child_id);
            return Err(AppError::FfmpegExtractTimeout { timeout: timeout.as_secs() });
        }

        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        // 解析 -progress 输出：out_time_us=12345678 或 out_time=00:00:12.345
        if let Some(progress) = parse_ffmpeg_progress(&line, duration_sec) {
            if let Some(cb) = on_progress {
                cb(progress);
            }
        }
        // progress=end 表示完成
        if line.starts_with("progress=end") {
            break;
        }
    }

    // 等待进程退出
    let status = child.wait().map_err(|e| AppError::FfmpegWaitFailed {
        detail: e.to_string(),
    })?;

    if !status.success() {
        let stderr = err_rx.recv().unwrap_or_default();
        tracing::error!("字幕提取失败: {}", stderr);
        if stderr.contains("subtitle") && stderr.contains("filter") {
            return Err(AppError::FfmpegGraphicSubtitle {
                codec: "unknown".to_string(),
            });
        }
        return Err(AppError::FfmpegExtractFailed {
            detail: stderr.chars().take(500).collect(),
        });
    }

    // 检查输出文件是否为空（ffmpeg 可能成功退出但输出空文件）
    let output_path_obj = Path::new(output_path);
    if !output_path_obj.exists() || output_path_obj.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
        tracing::warn!("字幕提取完成但输出文件为空: {}", output_path);
        return Err(AppError::FfmpegExtractEmptyStream);
    }

    tracing::info!("字幕提取成功: {} -> {}", video_path, output_path);
    Ok(())
}

/// 获取文件大小（字节）
pub fn get_file_size(path: &str) -> Result<u64, AppError> {
    let meta = std::fs::metadata(path).map_err(AppError::Io)?;
    Ok(meta.len())
}

/// 获取指定路径所在磁盘的剩余空间（字节）
#[cfg(windows)]
pub fn get_disk_free_space(path: &str) -> Result<u64, AppError> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // 取路径所在盘符根目录，如 "C:\"
    let root = Path::new(path)
        .ancestors()
        .nth(2) // 例如 C:\Users\... → C:\
        .and_then(|p| p.to_str())
        .unwrap_or(path);
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();

    let mut free_to_caller: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free_to_caller),
            Some(&mut total),
            Some(&mut total_free),
        )
        .map_err(|e| AppError::Io(std::io::Error::other(format!("GetDiskFreeSpaceExW: {}", e))))?;
    }
    Ok(free_to_caller)
}

#[cfg(not(windows))]
pub fn get_disk_free_space(path: &str) -> Result<u64, AppError> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).map_err(AppError::Io)?;
    Ok(meta.dev().into()) // 简化，实际应 statvfs
}

/// 合并字幕到视频（-c copy + 字幕流映射）
/// output_path = None: 输出到临时文件再替换原文件（需要额外等于视频大小的磁盘空间）
/// output_path = Some: 输出到指定路径，不修改原文件
pub fn merge_subtitle_to_video(
    video_path: &str,
    subtitle_path: &str,
    output_path: Option<&str>,
    language: Option<&str>,
    title: Option<&str>,
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

    // 路径规范化：反斜杠 → 正斜杠
    let ffmpeg_video_path = normalize_path_for_ffmpeg(video_path);
    let ffmpeg_subtitle_path = normalize_path_for_ffmpeg(subtitle_path);

    // map 顺序：新字幕(输入1)排第一，原视频流(输入0)排后面
    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        ffmpeg_video_path,
        "-i".to_string(),
        ffmpeg_subtitle_path,
        "-map".to_string(), "1:s:0".to_string(),  // 新字幕排第一
        "-map".to_string(), "0:v".to_string(),     // 原视频流
        "-map".to_string(), "0:a".to_string(),     // 原音频流
        "-map".to_string(), "0:s?".to_string(),    // 原有字幕流
        "-map".to_string(), "0:t?".to_string(),    // 附件
        "-c".to_string(), "copy".to_string(),
        "-c:s:0".to_string(), subtitle_codec.to_string(),
    ];

    if let Some(lang) = language {
        args.push("-metadata:s:s:0".to_string());
        args.push(format!("language={}", lang));
    }
    if let Some(title) = title {
        args.push("-metadata:s:s:0".to_string());
        args.push(format!("title={}", title));
    }
    args.push("-disposition:s:0".to_string());
    args.push("default".to_string());

    // 决定输出路径（规范化为正斜杠）
    let (final_output, replace_original) = match output_path {
        Some(p) => (normalize_path_for_ffmpeg(p), false),
        None => (normalize_path_for_ffmpeg(&format!("{}.tmp_merging.mkv", video_path)), true),
    };
    args.push(final_output.clone());

    let output = no_window(Command::new(&ffmpeg))
        .args(&args)
        .output()
        .map_err(|e| AppError::FfmpegStartFailed {
            detail: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("字幕合并失败: {}", stderr);
        if replace_original {
            let _ = std::fs::remove_file(&final_output);
        }
        return Err(AppError::FfmpegMergeFailed {
            detail: stderr.chars().take(500).collect(),
        });
    }

    // 替换原文件（仅当 output_path = None 时）
    if replace_original {
        std::fs::rename(&final_output, video_path).map_err(|e| {
            if std::fs::copy(&final_output, video_path).is_ok() {
                let _ = std::fs::remove_file(&final_output);
            }
            AppError::Io(e)
        })?;
        tracing::info!("字幕合并成功（直接修改原文件）: {} + {}", video_path, subtitle_path);
    } else {
        tracing::info!("字幕合并成功: {} + {} -> {}", video_path, subtitle_path, final_output);
    }
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

/// 字幕流编辑操作（由前端传入）
#[derive(Debug, Clone, Deserialize)]
pub struct SubtitleStreamEdit {
    pub original_index: i32,   // 原始视频中的绝对流索引
    pub title: Option<String>, // 新标题（None=保留原标题）
    pub language: Option<String>, // 新语言代码（None=保留原语言）
}

/// 编辑视频内嵌字幕流：重排序、删除、改名
/// output_path = None: 输出到临时文件再替换原文件（需要额外等于视频大小的磁盘空间）
/// output_path = Some: 输出到指定路径，不修改原文件
pub fn edit_subtitle_streams(
    video_path: &str,
    streams: &[SubtitleStreamEdit],
    output_path: Option<&str>,
    ffmpeg_custom_path: Option<&str>,
) -> Result<(), AppError> {
    let ffmpeg = find_ffmpeg(ffmpeg_custom_path)?;

    if !Path::new(video_path).exists() {
        return Err(AppError::FileNotFound { path: video_path.to_string() });
    }
    if streams.is_empty() {
        return Err(AppError::FfmpegNoStreamsKept);
    }

    // 构建 ffmpeg 参数（路径规范化为正斜杠）
    let ffmpeg_video_path = normalize_path_for_ffmpeg(video_path);
    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        ffmpeg_video_path,
        "-map".to_string(), "0:v".to_string(),
        "-map".to_string(), "0:a".to_string(),
        "-map".to_string(), "0:t?".to_string(),
    ];

    // 按新顺序映射字幕流（用绝对索引）
    for s in streams {
        args.push("-map".to_string());
        args.push(format!("0:{}", s.original_index));
    }

    args.push("-c".to_string());
    args.push("copy".to_string());

    // 为每条输出字幕流设置 metadata
    for (i, s) in streams.iter().enumerate() {
        if let Some(title) = &s.title {
            args.push(format!("-metadata:s:s:{}", i));
            args.push(format!("title={}", title));
        }
        if let Some(lang) = &s.language {
            args.push(format!("-metadata:s:s:{}", i));
            args.push(format!("language={}", lang));
        }
    }

    // 决定输出路径（规范化为正斜杠）
    let (final_output, replace_original) = match output_path {
        Some(p) => (normalize_path_for_ffmpeg(p), false),
        None => (normalize_path_for_ffmpeg(&format!("{}.tmp_editing.mkv", video_path)), true),
    };
    args.push(final_output.clone());

    let output = no_window(Command::new(&ffmpeg))
        .args(&args)
        .output()
        .map_err(|e| AppError::FfmpegStartFailed {
            detail: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("字幕流编辑失败: {}", stderr);
        if replace_original {
            let _ = std::fs::remove_file(&final_output);
        }
        return Err(AppError::FfmpegMergeFailed {
            detail: stderr.chars().take(500).collect(),
        });
    }

    // 替换原文件（仅当 output_path = None 时）
    if replace_original {
        std::fs::rename(&final_output, video_path).map_err(|e| {
            if std::fs::copy(&final_output, video_path).is_ok() {
                let _ = std::fs::remove_file(&final_output);
                return AppError::StorageWriteFailed { path: video_path.to_string() };
            }
            AppError::Io(e)
        })?;
        tracing::info!("字幕流编辑成功（直接修改原文件）: {} ({} 条字幕流)", video_path, streams.len());
    } else {
        tracing::info!("字幕流编辑成功: {} -> {} ({} 条字幕流)", video_path, final_output, streams.len());
    }
    Ok(())
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
    fn test_is_extractable_text_subtitle() {
        // 已知可提取的文本字幕
        assert!(is_extractable_text_subtitle("subrip"));
        assert!(is_extractable_text_subtitle("ass"));
        assert!(is_extractable_text_subtitle("ssa"));
        assert!(is_extractable_text_subtitle("webvtt"));
        assert!(is_extractable_text_subtitle("mov_text"));
        // 图形字幕不可提取
        assert!(!is_extractable_text_subtitle("hdmv_pgs_subtitle"));
        assert!(!is_extractable_text_subtitle("dvd_subtitle"));
        // 未知 codec 不可提取（避免白名单外的 codec 触发 ffmpeg 浪费时间）
        assert!(!is_extractable_text_subtitle("unknown_codec"));
        assert!(!is_extractable_text_subtitle(""));
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
