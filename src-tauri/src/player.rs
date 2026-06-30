// libmpv 播放器模块
// 负责下载、检测 libmpv.dll，动态加载并内嵌播放视频。
// 对应需求文档 §2.2 方案 B（原生子窗口嵌入）：libmpv 创建子窗口，通过 wid 嵌入 Tauri 主窗口。
// 下载源：zhongfly/mpv-winbuild 的 GPL build，功能完整（含 D3D11/GPU 渲染、DXVA2 硬件解码）。
// 下载源：zhongfly/mpv-winbuild 的 LGPL build（-Dgpl=false），许可证 LGPLv2.1+，允许闭源应用动态链接。

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::ffi::{c_char, c_int, c_void, CString};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(windows)]
use libloading::Library;
#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::*;
#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::HBRUSH;
#[cfg(windows)]
use windows::Win32::UI::Accessibility::*;
use windows::Win32::System::Threading::GetCurrentThreadId;

// WinEventHook 全局状态（回调函数无法传递上下文，只能用全局变量）
#[cfg(windows)]
static mut HOOK_PARENT: HWND = HWND(std::ptr::null_mut());
#[cfg(windows)]
static mut HOOK_CHILD: HWND = HWND(std::ptr::null_mut());
#[cfg(windows)]
static mut HOOK_LAST_X: i32 = i32::MIN;
#[cfg(windows)]
static mut HOOK_LAST_Y: i32 = i32::MIN;
/// 子窗口是否被主动隐藏（用于弹窗层级处理）。
/// 位置同步线程据此跳过 SetWindowPos(SWP_SHOWWINDOW)，避免刚 hide 就被拉回。
#[cfg(windows)]
static HOOK_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 全局 AppHandle，供 child_wnd_proc 发送点击事件（wnd_proc 无法传递上下文）
#[cfg(windows)]
static GLOBAL_APP_HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

// === libmpv FFI 类型定义 ===

/// mpv 实例句柄（不透明指针）
#[repr(C)]
pub struct MpvHandle {
    _private: [u8; 0],
}

/// mpv 事件 ID
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum MpvEventId {
    None = 0,
    Shutdown = 1,
    LogMessage = 2,
    GetPropertyReply = 3,
    SetPropertyReply = 4,
    CommandReply = 5,
    StartFile = 6,
    EndFile = 7,
    FileLoaded = 8,
    Idle = 11,
    Tick = 14,
    ClientMessage = 16,
    VideoReconfig = 17,
    AudioReconfig = 18,
    Seek = 20,
    PlaybackRestart = 21,
    PropertyChange = 22,
    QueueOverflow = 24,
    Hook = 25,
}

/// mpv 属性格式
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum MpvFormat {
    None = 0,
    String = 1,
    OsdString = 2,
    Flag = 3,
    Int64 = 4,
    Double = 5,
    Node = 6,
    NodeArray = 7,
    NodeMap = 8,
    ByteArray = 9,
}

/// mpv 事件结构
#[repr(C)]
pub struct MpvEvent {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

// === SECTION 1 END ===

/// libmpv 下载状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibmpvStatus {
    pub downloaded: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

impl LibmpvStatus {
    pub fn not_downloaded() -> Self {
        Self { downloaded: false, path: None, version: None }
    }
}

const LIBMPV_DIR_NAME: &str = "libmpv";
const LIBMPV_ARCHIVE_NAME: &str = "libmpv.7z";
const LIBMPV_DLL_NAME: &str = "libmpv.dll";
/// GitHub Releases API（zhongfly/mpv-winbuild，GPL build 的 libmpv）
/// GPL build 功能完整，含 D3D11/GPU 渲染器和 DXVA2/D3D11VA 硬件解码。
/// 资产命名：mpv-dev-x86_64-日期-git-哈希.7z
const LIBMPV_RELEASES_API: &str = "https://api.github.com/repos/zhongfly/mpv-winbuild/releases/latest";

fn libmpv_dir(app_data_dir: &Path) -> std::path::PathBuf {
    app_data_dir.join(LIBMPV_DIR_NAME)
}
fn libmpv_dll_path(app_data_dir: &Path) -> std::path::PathBuf {
    libmpv_dir(app_data_dir).join(LIBMPV_DLL_NAME)
}
fn libmpv_archive_path(app_data_dir: &Path) -> std::path::PathBuf {
    libmpv_dir(app_data_dir).join(LIBMPV_ARCHIVE_NAME)
}

pub fn get_libmpv_status(app_data_dir: &Path) -> LibmpvStatus {
    let dll_path = libmpv_dll_path(app_data_dir);
    if dll_path.exists() {
        LibmpvStatus { downloaded: true, path: Some(dll_path.to_string_lossy().to_string()), version: None }
    } else {
        LibmpvStatus::not_downloaded()
    }
}

pub fn get_libmpv_path(app_data_dir: &Path) -> Option<std::path::PathBuf> {
    let dll_path = libmpv_dll_path(app_data_dir);
    if dll_path.exists() { Some(dll_path) } else { None }
}

/// 从 GitHub Releases JSON 中解析最新 mpv-dev-x86_64-*.7z 的下载 URL（排除 lgpl/v3/aarch64）
/// JSON 中 assets 数组每项含 name 与 browser_download_url 字段
fn parse_latest_dev_url(releases_json: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(releases_json).ok()?;
    let assets = parsed.get("assets")?.as_array()?;
    for asset in assets {
        let name = asset.get("name")?.as_str()?;
        // 匹配 mpv-dev-x86_64-*.7z，排除 lgpl（缺渲染器）、v3（需要 AVX2，兼容性窄）和 aarch64
        if name.starts_with("mpv-dev-x86_64-") && name.ends_with(".7z") && !name.contains("v3") && !name.contains("lgpl") {
            let url = asset.get("browser_download_url")?.as_str()?;
            return Some(url.to_string());
        }
    }
    None
}

/// 下载 libmpv：从 GitHub zhongfly/mpv-winbuild releases 获取最新 mpv-dev-x86_64 包，
/// 流式下载（emit 进度事件），解压并提取 libmpv-2.dll，重命名为 libmpv.dll
/// 失败时清理半成品文件（7z、extract 目录、不完整的 dll）并 emit 失败事件
pub fn download_libmpv(
    app_data_dir: &Path,
    proxy: Option<&str>,
    app_handle: &tauri::AppHandle,
) -> Result<(), AppError> {
    use tauri::Emitter;

    // 内部主逻辑：返回 Err 时外层负责清理半成品
    let result = download_libmpv_inner(app_data_dir, proxy, app_handle);

    if let Err(ref e) = result {
        // 清理半成品文件，避免下次 get_libmpv_status 误判或残留垃圾
        let archive_path = libmpv_archive_path(app_data_dir);
        let extract_dir = libmpv_dir(app_data_dir).join("extract");
        let dll_path = libmpv_dll_path(app_data_dir);
        let _ = fs::remove_file(&archive_path);
        let _ = fs::remove_dir_all(&extract_dir);
        // 仅当 dll 不完整时删除（下载中途断网不会产生 dll，但解压后失败可能产生部分 dll）
        // 安全起见：删除 dll，让用户重新下载
        if dll_path.exists() {
            // 检查 dll 是否可加载，不可加载则删除
            // 简化：下载流程中 dll 是最后一步复制产生的，若 result 是 Err 则 dll 一定不完整
            let _ = fs::remove_file(&dll_path);
        }
        // emit 失败事件，前端据此显示错误提示（发送错误码 + 参数，前端自行翻译）
        let ipc_err = e.to_ipc_error();
        let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
            "stage": "failed", "progress": 0,
            "code": ipc_err.code,
            "args": ipc_err.args,
        }));
        tracing::warn!("libmpv 下载失败，已清理半成品文件: code={}", ipc_err.code);
    }

    result
}

/// 下载主逻辑（不含清理）
fn download_libmpv_inner(
    app_data_dir: &Path,
    proxy: Option<&str>,
    app_handle: &tauri::AppHandle,
) -> Result<(), AppError> {
    use tauri::Emitter;

    let dir = libmpv_dir(app_data_dir);
    fs::create_dir_all(&dir).map_err(|e| AppError::PlayerDownloadMkdirFailed {
        detail: e.to_string(),
    })?;

    let mut client_builder = reqwest::blocking::Client::builder()
        .user_agent("zimufan/1.0")
        .timeout(std::time::Duration::from_secs(600));
    if let Some(p) = proxy {
        if !p.is_empty() {
            client_builder = client_builder.proxy(
                reqwest::Proxy::all(p).map_err(|e| AppError::PlayerDownloadProxyFailed {
                    detail: e.to_string(),
                })?,
            );
        }
    }
    let client = client_builder.build().map_err(|e| AppError::PlayerDownloadHttpClientFailed {
        detail: e.to_string(),
    })?;

    // 1. 从 GitHub Releases API 获取最新 mpv-dev-x86_64 包 URL
    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "fetching", "progress": 0, "message": "正在获取最新版本信息..."
    }));
    tracing::info!("正在获取 zhongfly/mpv-winbuild 最新 release...");
    let api_resp = client.get(LIBMPV_RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| AppError::PlayerDownloadRssRequestFailed {
            detail: e.to_string(),
        })?;
    if !api_resp.status().is_success() {
        return Err(AppError::PlayerDownloadRssStatusFailed { status: api_resp.status().to_string() });
    }
    let releases_json = api_resp.text().map_err(|e| AppError::PlayerDownloadRssReadFailed {
        detail: e.to_string(),
    })?;
    let download_url = parse_latest_dev_url(&releases_json)
        .ok_or(AppError::PlayerDownloadPackageNotFound)?;

    tracing::info!("下载 libmpv: {}", download_url);

    // 2. 流式下载 7z 文件，emit 进度
    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "downloading", "progress": 0, "message": "开始下载..."
    }));
    let response = client.get(&download_url).send().map_err(|e| AppError::PlayerDownloadRequestFailed {
        detail: e.to_string(),
    })?;
    if !response.status().is_success() {
        return Err(AppError::PlayerDownloadHttpStatusFailed { status: response.status().to_string() });
    }
    let total_size = response.content_length().unwrap_or(0);
    let archive_path = libmpv_archive_path(app_data_dir);
    let mut file = fs::File::create(&archive_path).map_err(|e| AppError::PlayerDownloadCreateFileFailed {
        detail: e.to_string(),
    })?;
    use std::io::{Read, Write};
    let mut stream = response;
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let download_start = std::time::Instant::now();
    loop {
        let n = stream.read(&mut buf).map_err(|e| AppError::PlayerDownloadStreamReadFailed {
            detail: e.to_string(),
        })?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).map_err(|e| AppError::PlayerDownloadWriteFailed {
            detail: e.to_string(),
        })?;
        downloaded += n as u64;
        // 每 200ms emit 一次进度
        if last_emit.elapsed() > std::time::Duration::from_millis(200) {
            let pct = if total_size > 0 { (downloaded * 100 / total_size) as u8 } else { 0 };
            let elapsed = download_start.elapsed().as_secs_f64();
            let speed_bps = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
            let speed_mb = speed_bps / 1024.0 / 1024.0;
            let remaining_bytes = total_size.saturating_sub(downloaded);
            let eta_secs = if speed_bps > 0.0 { remaining_bytes as f64 / speed_bps } else { 0.0 };
            let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
                "stage": "downloading", "progress": pct,
                "downloaded": downloaded, "total": total_size,
                "speed_mbps": (speed_mb * 10.0).round() / 10.0,
                "eta_secs": eta_secs.round() as u64,
                "message": format!("Downloading {}% ({} / {} MB)",
                    pct,
                    downloaded / 1024 / 1024,
                    total_size / 1024 / 1024)
            }));
            last_emit = std::time::Instant::now();
        }
    }
    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "downloading", "progress": 100, "message": "下载完成"
    }));

    // 3. 解压 7z 并提取 libmpv-2.dll
    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "extracting", "progress": -1, "message": "正在解压安装...（约 30MB）"
    }));
    tracing::info!("解压 libmpv 7z 包...");
    let extract_dir = libmpv_dir(app_data_dir).join("extract");
    // 清理旧解压目录
    let _ = fs::remove_dir_all(&extract_dir);
    fs::create_dir_all(&extract_dir).map_err(|e| AppError::PlayerDownloadExtractMkdirFailed {
        detail: e.to_string(),
    })?;
    sevenz_rust::decompress_file(&archive_path, &extract_dir)
        .map_err(|e| AppError::PlayerDownloadExtractFailed {
            detail: e.to_string(),
        })?;
    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "extracting", "progress": 90, "message": "正在安装 DLL..."
    }));

    // 4. 查找 libmpv-2.dll（dev 包中通常在 dll/ 目录下）
    let dll_source = find_file(&extract_dir, "libmpv-2.dll")
        .or_else(|| find_file(&extract_dir, "mpv-2.dll"))
        .ok_or(AppError::PlayerDownloadDllNotFound)?;

    // 5. 复制并重命名为 libmpv.dll
    let dll_dest = libmpv_dll_path(app_data_dir);
    fs::copy(&dll_source, &dll_dest).map_err(|e| AppError::PlayerDownloadCopyDllFailed {
        detail: e.to_string(),
    })?;

    // 6. 清理解压目录和 7z 文件
    let _ = fs::remove_dir_all(&extract_dir);
    let _ = fs::remove_file(&archive_path);

    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "done", "progress": 100, "message": "安装完成"
    }));
    tracing::info!("libmpv 下载完成: {}", dll_dest.display());
    Ok(())
}

/// 删除已下载的 libmpv 组件（整个 libmpv 目录）
pub fn delete_libmpv(app_data_dir: &Path) -> Result<(), AppError> {
    let dir = libmpv_dir(app_data_dir);
    if !dir.exists() {
        return Ok(());
    }
    fs::remove_dir_all(&dir).map_err(|e| AppError::PlayerDownloadDeleteFailed {
        detail: e.to_string(),
    })?;
    tracing::info!("libmpv 目录已删除: {}", dir.display());
    Ok(())
}

/// 递归查找指定文件名的文件
fn find_file(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find_file(&path, name) {
                    return Some(found);
                }
            } else if path.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false) {
                return Some(path);
            }
        }
    }
    None
}

/// 使用系统默认播放器打开视频文件（降级路径）
pub fn open_in_system_player(video_path: &str) -> Result<(), AppError> {
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "start", "", video_path]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let status = cmd.status()
        .map_err(|e| AppError::PlayerLoadFailed { detail: format!("{} ({})", video_path, e) })?;
    if !status.success() {
        return Err(AppError::PlayerLoadFailed { detail: video_path.to_string() });
    }
    Ok(())
}

// === SECTION 2 END ===

// === 已安装播放器枚举（右键菜单"用播放器打开"用） ===

/// 已安装的播放器信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlayer {
    /// 显示名称（如 "QQ影音"、"VLC media player"）
    pub name: String,
    /// exe 完整路径
    pub exe_path: String,
    /// 是否为该扩展名的默认播放器
    pub is_default: bool,
}

/// 播放器图标信息（前端用 convertFileSrc 加载 icon_path 显示图标）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerIcon {
    /// 对应的 exe 完整路径（用于和 InstalledPlayer.exe_path 匹配）
    pub exe_path: String,
    /// 图标 PNG 文件的完整路径（前端用 convertFileSrc 转为可加载的 URL）
    pub icon_path: String,
}

/// 视频扩展名 → 注册表查询用的扩展名（带点）
fn video_ext_with_dot(video_path: &str) -> Option<String> {
    let path = std::path::Path::new(video_path);
    path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
}

/// 用 Windows Shell API SHAssocEnumHandlers 枚举文件关联处理器
/// 这是资源管理器右键"打开方式"子菜单使用的同一个 API，返回的列表和顺序完全一致。
/// 返回 Vec<(ui_name, prog_id_or_exe_name, is_recommended)>
#[cfg(windows)]
fn enum_assoc_handlers(ext: &str) -> Vec<(String, String, bool)> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::{
        IAssocHandler, SHAssocEnumHandlers, ASSOC_FILTER_RECOMMENDED,
    };

    let mut result = Vec::new();
    let ext_hstring = HSTRING::from(ext);

    unsafe {
        // ASSOC_FILTER_RECOMMENDED：和资源管理器右键"打开方式"展开的子菜单一致
        // 只返回系统推荐的处理程序（默认程序 + 常用/推荐程序），不会把所有注册了关联的程序都列出来。
        let enum_handlers = match SHAssocEnumHandlers(&ext_hstring, ASSOC_FILTER_RECOMMENDED) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("enum_assoc_handlers: SHAssocEnumHandlers 失败: {:?}", e);
                return result;
            }
        };

        loop {
            // IAssocHandler 不是 Copy，不能用 [None; 16]，改用 Vec
            let mut handlers: Vec<Option<IAssocHandler>> = (0..16).map(|_| None).collect();
            let mut fetched: u32 = 0;
            let hr = enum_handlers.Next(&mut handlers, Some(std::ptr::from_mut(&mut fetched)));
            if hr.is_err() || fetched == 0 {
                break;
            }
            for i in 0..fetched as usize {
                if let Some(handler) = handlers[i].take() {
                    // GetUIName/GetName 返回 PWSTR，需要手动转 String
                    let ui_name = handler.GetUIName()
                        .ok()
                        .and_then(|pwstr| pwstr.to_string().ok())
                        .unwrap_or_default();
                    let name = handler.GetName()
                        .ok()
                        .and_then(|pwstr| pwstr.to_string().ok())
                        .unwrap_or_default();
                    // IsRecommended 返回 HRESULT，ok() 判断是否成功
                    let is_recommended = handler.IsRecommended().is_ok();
                    tracing::debug!(
                        "enum_assoc_handlers: ui_name='{}', name='{}', recommended={}",
                        ui_name, name, is_recommended
                    );
                    if !ui_name.is_empty() {
                        result.push((ui_name, name, is_recommended));
                    }
                }
            }
        }
    }
    tracing::info!("enum_assoc_handlers: ext={}, 共 {} 个关联处理器", ext, result.len());
    result
}

#[cfg(not(windows))]
fn enum_assoc_handlers(_ext: &str) -> Vec<(String, String, bool)> {
    Vec::new()
}

/// 从 ProgID 解析出 exe 路径和显示名
/// ProgID 注册在 HKCR\<ProgID>\shell\open\command，默认值形如:
///   "C:\Program Files\VLC\vlc.exe" --started-from-file "%1"
/// HKCR\<ProgID> 的默认值通常是友好名称（如 "VLC media player"）
/// 但也可能是文件类型描述（如 "mkv - Matroska 电影文件"），此时用 exe 文件名作为显示名
#[cfg(windows)]
fn resolve_prog_id(prog_id: &str) -> Option<(String, String)> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hcr = RegKey::predef(HKEY_CLASSES_ROOT);
    // 友好名称
    let friendly_name = hcr.open_subkey(prog_id)
        .ok()
        .and_then(|key| key.get_value::<String, _>("").ok());
    // command 行
    let cmd_key = hcr.open_subkey(&format!("{}\\shell\\open\\command", prog_id)).ok()?;
    let cmd_line: String = cmd_key.get_value("").ok()?;
    let exe_path = parse_exe_from_command(&cmd_line)?;
    // exe 必须存在且以 .exe 结尾，否则跳过（可能是文件类型描述而非播放器）
    if !exe_path.to_lowercase().ends_with(".exe") || !std::path::Path::new(&exe_path).exists() {
        tracing::debug!("resolve_prog_id: {} → exe 不存在或非 exe: {}", prog_id, exe_path);
        return None;
    }
    // 判断 friendly_name 是否是文件类型描述（含"文件"、扩展名等关键词）
    // 如果是，用 exe 文件名作为显示名
    let exe_stem = std::path::Path::new(&exe_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| prog_id.to_string());
    let name = match friendly_name {
        Some(ref n) if !n.is_empty() => {
            let lower = n.to_lowercase();
            // 文件类型描述特征：含"文件"、"file"、扩展名（如 ".mkv"）、或就是 ProgID 本身
            let is_filetype_desc = lower.contains("文件")
                || lower.contains("file")
                || lower.contains(".mkv")
                || lower.contains(".mp4")
                || lower.contains(".avi")
                || lower.contains("matroska")
                || lower.contains("video clip")
                || n == prog_id;
            if is_filetype_desc {
                exe_stem // 用 exe 文件名代替文件类型描述
            } else {
                n.clone()
            }
        }
        _ => exe_stem,
    };
    tracing::debug!("resolve_prog_id: {} → name={}, exe={}", prog_id, name, exe_path);
    Some((name, exe_path))
}

/// 从命令行字符串中提取 exe 路径
/// 处理: "C:\path\app.exe" args...  或  C:\path\app.exe args...
fn parse_exe_from_command(cmd: &str) -> Option<String> {
    let trimmed = cmd.trim();
    if trimmed.starts_with('"') {
        // 带引号：找到闭合引号
        let end = trimmed[1..].find('"')?;
        Some(trimmed[1..1 + end].to_string())
    } else {
        // 不带引号：取第一个空格前的部分
        let end = trimmed.find(' ').unwrap_or(trimmed.len());
        Some(trimmed[..end].to_string())
    }
}

/// 从 OpenWithList 的 exe 名称（如 "PotPlayerMini64.exe"）解析完整路径
/// 查 HKCR\Applications\<exe>\shell\open\command
#[cfg(windows)]
fn resolve_app_exe(exe_name: &str) -> Option<(String, String)> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let key = hcr.open_subkey(&format!("Applications\\{}\\shell\\open\\command", exe_name)).ok()?;
    let cmd_line: String = key.get_value("").ok()?;
    let exe_path = parse_exe_from_command(&cmd_line)?;
    let name = std::path::Path::new(&exe_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| exe_name.to_string());
    Some((name, exe_path))
}

/// 已知播放器路径探测（补充注册表 OpenWithList/Progids 没覆盖到的安装版播放器）
/// 多管齐下：注册表 InstallPath + WOW6432Node + Uninstall 枚举 + App Paths + 常见路径
#[cfg(windows)]
fn detect_known_players() -> Vec<InstalledPlayer> {
    use winreg::enums::*;
    use winreg::RegKey;
    let mut result: Vec<InstalledPlayer> = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // helper：检查 exe 是否存在，存在则 push
    let mut try_push = |name: &str, exe_path: &str| {
        if std::path::Path::new(exe_path).exists() {
            result.push(InstalledPlayer {
                name: name.to_string(),
                exe_path: exe_path.to_string(),
                is_default: false,
            });
            tracing::debug!("detect_known_players: found {} at {}", name, exe_path);
        }
    };

    // --- PotPlayer ---
    // 注册表路径可能有多个变体，含 WOW6432Node（32 位 PotPlayer 安装在 64 位系统）
    // 值名也可能是 InstallPath 或 Path
    for subkey in &[
        "SOFTWARE\\PotPlayer", "SOFTWARE\\PotPlayer64", "SOFTWARE\\Daum\\PotPlayer",
        "SOFTWARE\\WOW6432Node\\PotPlayer", "SOFTWARE\\WOW6432Node\\PotPlayer64",
        "SOFTWARE\\WOW6432Node\\Daum\\PotPlayer",
    ] {
        if let Ok(key) = hklm.open_subkey(subkey) {
            // 尝试多种值名
            for value_name in &["InstallPath", "Path", "InstallDir"] {
                if let Ok(install_path) = key.get_value::<String, _>(value_name) {
                    tracing::debug!("detect_known_players: PotPlayer {}\\{} = {}", subkey, value_name, install_path);
                    // PotPlayerMini64.exe (64位) 或 PotPlayerMini.exe (32位)
                    for exe_name in &["PotPlayerMini64.exe", "PotPlayerMini.exe"] {
                        let exe = format!("{}\\{}", install_path, exe_name);
                        try_push("PotPlayer", &exe);
                    }
                }
            }
        }
    }

    // --- VLC ---
    for subkey in &[
        "SOFTWARE\\VideoLAN\\VLC",
        "SOFTWARE\\WOW6432Node\\VideoLAN\\VLC",
    ] {
        if let Ok(key) = hklm.open_subkey(subkey) {
            for value_name in &["InstallDir", "InstallPath", "Path"] {
                if let Ok(install_dir) = key.get_value::<String, _>(value_name) {
                    tracing::debug!("detect_known_players: VLC {}\\{} = {}", subkey, value_name, install_dir);
                    let exe = format!("{}\\vlc.exe", install_dir);
                    try_push("VLC media player", &exe);
                }
            }
        }
    }

    // --- MPC-HC / MPC-BE ---
    for exe_path in &[
        r"C:\Program Files\MPC-HC\mpc-hc64.exe",
        r"C:\Program Files\MPC-HC\mpc-hc.exe",
        r"C:\Program Files (x86)\MPC-HC\mpc-hc.exe",
        r"C:\Program Files\MPC-BE\mpc-be64.exe",
        r"C:\Program Files\MPC-BE\mpc-be.exe",
        r"C:\Program Files (x86)\MPC-BE\mpc-be.exe",
    ] {
        if std::path::Path::new(exe_path).exists() {
            let name = std::path::Path::new(exe_path)
                .file_stem().map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "MPC".to_string());
            try_push(&name, exe_path);
        }
    }

    // --- mpv ---
    // mpv 通常解压即用，但有些安装版会注册 App Paths
    for subkey in &[
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\mpv.exe",
        "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths\\mpv.exe",
    ] {
        for root in [&hklm, &hkcu] {
            if let Ok(key) = root.open_subkey(subkey) {
                if let Ok(path) = key.get_value::<String, _>("") {
                    let clean = path.trim_matches('"').to_string();
                    try_push("mpv", &clean);
                }
            }
        }
    }

    // --- 通过 Uninstall 注册表枚举补充（覆盖 QQ影音、迅雷播放器、恒星播放器等） ---
    // 匹配 DisplayName 含关键词的条目，从 InstallLocation 或 DisplayIcon 推断 exe 路径
    // 同时查 HKLM 和 HKCU（现代播放器常装在用户目录，注册在 HKCU Uninstall）
    let player_keywords = [
        "potplayer", "vlc", "mpc-hc", "mpc-be", "mpc hc", "mpc be",
        "qqplayer", "qq player", "qq影音", "迅雷播放器", "thunder player",
        "stellarplayer", "恒星播放器", "wmplayer", "media player",
        "mpv", "pot player", "pot player", "qq 影音",
    ];
    for (root_name, root) in &[("HKLM", &hklm), ("HKCU", &hkcu)] {
        for uninstall_path in &[
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ] {
            if let Ok(uninstall_key) = root.open_subkey(uninstall_path) {
                for subkey_name in uninstall_key.enum_keys().filter_map(|k| k.ok()) {
                    if let Ok(sub) = uninstall_key.open_subkey(&subkey_name) {
                        let display_name: Option<String> = sub.get_value("DisplayName").ok();
                        let install_location: Option<String> = sub.get_value("InstallLocation").ok();
                        let display_icon: Option<String> = sub.get_value("DisplayIcon").ok();
                        let name_lower = display_name.as_deref().unwrap_or("").to_lowercase();
                        if !player_keywords.iter().any(|kw| name_lower.contains(kw)) {
                            continue;
                        }
                        tracing::debug!(
                            "detect_known_players: Uninstall 匹配 {}\\{}\\{}: name={:?}, loc={:?}, icon={:?}",
                            root_name, uninstall_path, subkey_name, display_name, install_location, display_icon
                        );
                        // 从 InstallLocation 找 exe
                        if let Some(loc) = &install_location {
                            if !loc.is_empty() && std::path::Path::new(loc).is_dir() {
                                // 尝试常见 exe 名
                                for exe_name in &[
                                    "PotPlayerMini64.exe", "PotPlayerMini.exe", "vlc.exe",
                                    "mpc-hc64.exe", "mpc-hc.exe", "mpc-be64.exe", "mpc-be.exe",
                                    "QQPlayer.exe", "ThunderPlayer.exe", "mpv.exe",
                                    "PotPlayer.exe", "QQPlayerMini.exe", "StellarPlayer.exe",
                                ] {
                                    let exe = format!("{}\\{}", loc, exe_name);
                                    if std::path::Path::new(&exe).exists() {
                                        let display = display_name.as_deref().unwrap_or(exe_name);
                                        try_push(display, &exe);
                                        break;
                                    }
                                }
                            }
                        }
                        // 从 DisplayIcon 找 exe（格式可能是 "C:\path\app.exe,0"）
                        if let Some(icon) = &display_icon {
                            let exe_path = icon.split(',').next().unwrap_or("").trim_matches('"').trim();
                            if exe_path.to_lowercase().ends_with(".exe") && std::path::Path::new(exe_path).exists() {
                                let display = display_name.as_deref().unwrap_or("");
                                try_push(display, exe_path);
                            }
                        }
                    }
                }
            }
        }
    }

    // --- Windows Media Player（系统自带，兜底） ---
    let wmp = r"C:\Windows\System32\wmplayer.exe";
    try_push("Windows Media Player", wmp);

    // 去重（按 exe_path，不区分大小写，全局去重）
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    result.retain(|p| {
        let key = p.exe_path.to_lowercase();
        if seen.contains(&key) {
            false
        } else {
            seen.insert(key);
            true
        }
    });
    tracing::info!("detect_known_players: 共找到 {} 个播放器", result.len());
    result
}

/// 列出已安装的视频播放器，与 Windows 资源管理器右键"打开方式"子菜单完全一致。
///
/// 主方案：用 SHAssocEnumHandlers Shell API 枚举关联处理器
///   - 这是资源管理器"打开方式"子菜单使用的同一个 API
///   - 返回顺序与资源管理器完全一致（推荐项在前，然后按最近使用排序）
///
/// 默认程序判定：HKCU\...\FileExts\.ext\UserChoice 的 ProgID 才是真正的默认程序，
/// 与 SHAssocEnumHandlers 枚举结果匹配后只设置一个 is_default=true。
///
/// 补充方案：detect_known_players 探测已知播放器（覆盖未注册关联的安装版播放器）
///
/// 去重：按 exe_path 不区分大小写去重
#[cfg(windows)]
pub fn list_installed_players(video_path: &str) -> Result<Vec<InstalledPlayer>, AppError> {
    use winreg::enums::*;
    use winreg::RegKey;

    let ext = match video_ext_with_dot(video_path) {
        Some(e) => e,
        None => return Ok(vec![]),
    };

    // 1. 先确定真正的默认程序 exe 路径（UserChoice）
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let user_choice_path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{}\\UserChoice",
        ext
    );
    let default_exe = hkcu
        .open_subkey(&user_choice_path)
        .ok()
        .and_then(|k| k.get_value::<String, _>("ProgId").ok())
        .and_then(|prog_id| {
            tracing::debug!("list_installed_players: UserChoice ProgID for {} = {}", ext, prog_id);
            resolve_prog_id(&prog_id)
                .or_else(|| resolve_app_exe(&prog_id))
                .map(|(_, exe_path)| exe_path.to_lowercase())
        });
    tracing::debug!("list_installed_players: 默认程序 exe = {:?}", default_exe);

    // exe_path（小写）→ InstalledPlayer，用于去重
    let mut by_exe: std::collections::HashMap<String, InstalledPlayer> = std::collections::HashMap::new();
    // 有序列表（按插入顺序）
    let mut ordered: Vec<String> = Vec::new();

    let mut insert = |player: InstalledPlayer| {
        let key = player.exe_path.to_lowercase();
        if !by_exe.contains_key(&key) {
            ordered.push(key.clone());
            by_exe.insert(key, player);
        }
    };

    // === 主方案：SHAssocEnumHandlers ===
    // 枚举结果与资源管理器"打开方式"子菜单完全一致
    let assoc_handlers = enum_assoc_handlers(&ext);
    for (ui_name, prog_id_or_exe, _is_recommended) in assoc_handlers {
        // 尝试从 ProgID 或 exe 名解析出 exe 路径
        let resolved = resolve_prog_id(&prog_id_or_exe)
            .or_else(|| resolve_app_exe(&prog_id_or_exe));

        if let Some((_, exe_path)) = resolved {
            if std::path::Path::new(&exe_path).exists() {
                // 是否是真正的默认程序：与 UserChoice 解析出的 exe 路径匹配
                let is_default = default_exe.as_ref().map(|d| d.eq_ignore_ascii_case(&exe_path.to_lowercase())).unwrap_or(false);
                insert(InstalledPlayer {
                    name: ui_name,
                    exe_path,
                    is_default,
                });
                continue;
            }
        }

        // 如果 prog_id_or_exe 本身是完整路径
        if prog_id_or_exe.to_lowercase().ends_with(".exe")
            && std::path::Path::new(&prog_id_or_exe).exists()
        {
            let exe_path = prog_id_or_exe;
            let is_default = default_exe.as_ref().map(|d| d.eq_ignore_ascii_case(&exe_path.to_lowercase())).unwrap_or(false);
            insert(InstalledPlayer {
                name: ui_name,
                exe_path,
                is_default,
            });
            continue;
        }

        // 解析失败但 UIName 有效：记录日志，后续 detect_known_players 可能补上
        tracing::debug!(
            "list_installed_players: 无法解析 exe 路径: ui_name='{}', name='{}'",
            ui_name, prog_id_or_exe
        );
    }

    // === 补充方案：已知播放器探测 ===
    // 覆盖 SHAssocEnumHandlers 未返回的安装版播放器（未注册文件关联的）
    for player in detect_known_players() {
        insert(player);
    }

    // 转为有序列表
    let result: Vec<InstalledPlayer> = ordered
        .into_iter()
        .map(|key| by_exe.remove(&key).unwrap())
        .collect();

    tracing::info!(
        "list_installed_players: ext={}, 共 {} 个播放器: {:?}",
        ext,
        result.len(),
        result.iter().map(|p| format!("{}({})", p.name, p.is_default)).collect::<Vec<_>>()
    );

    Ok(result)
}

#[cfg(not(windows))]
pub fn list_installed_players(_video_path: &str) -> Result<Vec<InstalledPlayer>, AppError> {
    Ok(vec![])
}

/// 用指定播放器打开视频文件
pub fn open_with_player(exe_path: &str, video_path: &str) -> Result<(), AppError> {
    let status = std::process::Command::new(exe_path)
        .arg(video_path)
        .spawn()
        .map_err(|e| AppError::PlayerLoadFailed {
            detail: format!("{} ({})", exe_path, e),
        })?;
    let _ = status; // 不等待，spawn 后立即返回
    Ok(())
}

/// 在资源管理器中定位视频文件（explorer /select,"path"）
/// 注意：explorer 的 /select 参数对路径中的空格/特殊字符敏感，
/// 必须把 /select,"path" 作为单个参数传递（用 raw_arg 避免 Rust 自动加引号）。
/// 直接用 .arg() 传含空格的参数时 Rust 会自动加引号，导致 explorer 解析失败打开"我的文档"。
pub fn reveal_in_explorer(file_path: &str) -> Result<(), AppError> {
    // 将路径中的正斜杠统一为反斜杠（explorer 对正斜杠支持不佳）
    let normalized = file_path.replace('/', "\\");
    let select_arg = format!("/select,\"{}\"", normalized);
    let mut cmd = std::process::Command::new("explorer.exe");
    // raw_arg 不自动加引号，直接原样传递参数
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(select_arg);
    }
    #[cfg(not(windows))]
    {
        cmd.arg(select_arg);
    }
    cmd.spawn()
        .map_err(|e| AppError::PlayerLoadFailed {
            detail: format!("explorer ({})", e),
        })?;
    Ok(())
}

// === SECTION 2.5 END ===

// === SECTION 2.6: 播放器图标提取 ===

/// 计算 exe_path 的 hash，用作图标文件名（避免路径中的特殊字符问题）
fn icon_filename(exe_path: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    exe_path.to_lowercase().hash(&mut hasher);
    format!("{:016x}.png", hasher.finish())
}

/// 从 exe 文件提取图标并保存为 PNG
/// 用 SHGetFileInfo 获取 HICON，GetDIBits 获取像素数据，png crate 编码
#[cfg(windows)]
fn extract_icon_to_png(exe_path: &str, output_path: &Path) -> Result<(), AppError> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};

    unsafe {
        // 1. SHGetFileInfo 获取大图标 HICON
        let exe_wide: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut shfi = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(exe_wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if result == 0 || shfi.hIcon.is_invalid() {
            return Err(AppError::PlayerLoadFailed {
                detail: format!("SHGetFileInfo 失败: {}", exe_path),
            });
        }
        let hicon = shfi.hIcon;

        // 2. GetIconInfo 获取 HBITMAP
        let mut icon_info = ICONINFO::default();
        let ok = GetIconInfo(hicon, &mut icon_info).is_ok();
        if !ok {
            let _ = DestroyIcon(hicon);
            return Err(AppError::PlayerLoadFailed {
                detail: format!("GetIconInfo 失败: {}", exe_path),
            });
        }

        let hbm_color = icon_info.hbmColor;
        let hbm_mask = icon_info.hbmMask;

        // 如果 hbmColor 为空（单色图标），用 hbmMask 代替
        let hbm = if hbm_color.is_invalid() { hbm_mask } else { hbm_color };

        // 3. GetObject 获取位图尺寸
        let mut bmp = BITMAP::default();
        let bytes = GetObjectW(hbm.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bmp as *mut _ as *mut std::ffi::c_void));
        if bytes == 0 {
            if !hbm_color.is_invalid() { let _ = DeleteObject(hbm_color.into()); }
            if !hbm_mask.is_invalid() { let _ = DeleteObject(hbm_mask.into()); }
            let _ = DestroyIcon(hicon);
            return Err(AppError::PlayerLoadFailed {
                detail: format!("GetObject 失败: {}", exe_path),
            });
        }

        let width = bmp.bmWidth as u32;
        let height = bmp.bmHeight as u32;
        if width == 0 || height == 0 {
            if !hbm_color.is_invalid() { let _ = DeleteObject(hbm_color.into()); }
            if !hbm_mask.is_invalid() { let _ = DeleteObject(hbm_mask.into()); }
            let _ = DestroyIcon(hicon);
            return Err(AppError::PlayerLoadFailed {
                detail: format!("图标尺寸为 0: {}", exe_path),
            });
        }

        // 4. GetDIBits 获取 RGBA 像素数据（top-down）
        let hdc = GetDC(None);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // 负值 = top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD::default()],
        };

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let lines = GetDIBits(
            hdc,
            hbm,
            0,
            height,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);

        // 清理 GDI 资源
        if !hbm_color.is_invalid() { let _ = DeleteObject(hbm_color.into()); }
        if !hbm_mask.is_invalid() { let _ = DeleteObject(hbm_mask.into()); }
        let _ = DestroyIcon(hicon);

        if lines == 0 {
            return Err(AppError::PlayerLoadFailed {
                detail: format!("GetDIBits 失败: {}", exe_path),
            });
        }

        // 5. BGRA → RGBA 转换（Windows 位图是 BGRA，PNG 需要 RGBA）
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2); // B <-> R
        }

        // 6. 用 png crate 编码
        let file = std::fs::File::create(output_path).map_err(|e| AppError::PlayerLoadFailed {
            detail: format!("创建图标文件失败: {} ({})", output_path.display(), e),
        })?;
        let w = std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(w, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| AppError::PlayerLoadFailed {
            detail: format!("PNG 编码失败: {}", e),
        })?;
        writer.write_image_data(&pixels).map_err(|e| AppError::PlayerLoadFailed {
            detail: format!("PNG 写入失败: {}", e),
        })?;
        writer.finish().map_err(|e| AppError::PlayerLoadFailed {
            detail: format!("PNG 完成失败: {}", e),
        })?;
    }

    Ok(())
}

#[cfg(not(windows))]
fn extract_icon_to_png(_exe_path: &str, _output_path: &Path) -> Result<(), AppError> {
    Err(AppError::PlayerLoadFailed { detail: "不支持的平台".to_string() })
}

/// 提取播放器图标（异步，已存在的跳过）
/// 在加载视频时调用，后台提取所有播放器的图标到 icons_dir 目录
pub fn extract_player_icons(video_path: &str, icons_dir: &Path) -> Result<Vec<PlayerIcon>, AppError> {
    // 确保目录存在
    std::fs::create_dir_all(icons_dir).map_err(|e| AppError::PlayerLoadFailed {
        detail: format!("创建图标目录失败: {} ({})", icons_dir.display(), e),
    })?;

    // 获取播放器列表
    let players = list_installed_players(video_path)?;
    let mut result = Vec::new();

    for player in &players {
        let filename = icon_filename(&player.exe_path);
        let icon_path = icons_dir.join(&filename);

        // 已存在则跳过提取
        if !icon_path.exists() {
            tracing::debug!("extract_player_icons: 提取图标 {} → {}", player.exe_path, icon_path.display());
            if let Err(e) = extract_icon_to_png(&player.exe_path, &icon_path) {
                tracing::warn!("extract_player_icons: 提取图标失败 {} : {}", player.exe_path, e);
                continue;
            }
        }

        result.push(PlayerIcon {
            exe_path: player.exe_path.clone(),
            icon_path: icon_path.to_string_lossy().to_string(),
        });
    }

    tracing::info!("extract_player_icons: 共提取 {} 个图标", result.len());
    Ok(result)
}

/// 清除播放器图标缓存
pub fn clear_player_icons_cache(icons_dir: &Path) -> Result<usize, AppError> {
    if !icons_dir.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in std::fs::read_dir(icons_dir).map_err(|e| AppError::PlayerLoadFailed {
        detail: format!("读取图标目录失败: {} ({})", icons_dir.display(), e),
    })? {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.extension().map(|e| e == "png").unwrap_or(false) {
                if std::fs::remove_file(&path).is_ok() {
                    count += 1;
                }
            }
        }
    }
    tracing::info!("clear_player_icons_cache: 清除 {} 个图标", count);
    Ok(count)
}

// === SECTION 2.6 END ===

// === libmpv FFI 函数签名 + 动态加载 ===

/// 定义 libmpv 函数指针类型
type FnMpvCreate = unsafe extern "C" fn() -> *mut MpvHandle;
type FnMpvInitialize = unsafe extern "C" fn(*mut MpvHandle) -> c_int;
type FnMpvTerminateDestroy = unsafe extern "C" fn(*mut MpvHandle);
type FnMpvSetOptionString = unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type FnMpvSetPropertyString = unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type FnMpvGetPropertyString = unsafe extern "C" fn(*mut MpvHandle, *const c_char) -> *mut c_char;
type FnMpvGetPropertyDouble = unsafe extern "C" fn(*mut MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
type FnMpvCommand = unsafe extern "C" fn(*mut MpvHandle, *const *const c_char) -> c_int;
type FnMpvFree = unsafe extern "C" fn(*mut c_void);
type FnMpvWaitEvent = unsafe extern "C" fn(*mut MpvHandle, f64) -> *mut MpvEvent;
type FnMpvWakeup = unsafe extern "C" fn(*mut MpvHandle);

/// 从 libmpv.dll 中加载的函数集合
#[cfg(windows)]
struct MpvApi {
    _lib: Library,
    create: FnMpvCreate,
    initialize: FnMpvInitialize,
    terminate_destroy: FnMpvTerminateDestroy,
    set_option_string: FnMpvSetOptionString,
    set_property_string: FnMpvSetPropertyString,
    get_property_string: FnMpvGetPropertyString,
    get_property: FnMpvGetPropertyDouble,
    command: FnMpvCommand,
    free: FnMpvFree,
    wait_event: FnMpvWaitEvent,
    wakeup: FnMpvWakeup,
}

#[cfg(windows)]
impl MpvApi {
    /// 从 libmpv.dll 动态加载所有需要的函数
    unsafe fn load(dll_path: &str) -> Result<Self, AppError> {
        let lib = Library::new(dll_path).map_err(|e| AppError::PlayerLibmpvDllLoadFailed {
            detail: format!("{} ({})", dll_path, e),
        })?;
        macro_rules! sym {
            ($name:literal, $type:ty) => {
                *lib.get::<$type>(concat!($name, "\0").as_bytes())
                    .map_err(|e| AppError::PlayerSymbolNotFound { name: $name.to_string(), detail: e.to_string() })?
            };
        }
        Ok(MpvApi {
            create: sym!("mpv_create", FnMpvCreate),
            initialize: sym!("mpv_initialize", FnMpvInitialize),
            terminate_destroy: sym!("mpv_terminate_destroy", FnMpvTerminateDestroy),
            set_option_string: sym!("mpv_set_option_string", FnMpvSetOptionString),
            set_property_string: sym!("mpv_set_property_string", FnMpvSetPropertyString),
            get_property_string: sym!("mpv_get_property_string", FnMpvGetPropertyString),
            get_property: sym!("mpv_get_property", FnMpvGetPropertyDouble),
            command: sym!("mpv_command", FnMpvCommand),
            free: sym!("mpv_free", FnMpvFree),
            wait_event: sym!("mpv_wait_event", FnMpvWaitEvent),
            wakeup: sym!("mpv_wakeup", FnMpvWakeup),
            _lib: lib,
        })
    }
}

// === SECTION 3 END ===

// === Player 结构体：内嵌 libmpv 播放 ===

/// libmpv 内嵌播放器
/// 使用 wid 方式将 libmpv 渲染嵌入子窗口
#[cfg(windows)]
pub struct Player {
    api: MpvApi,
    mpv: *mut MpvHandle,
    child_hwnd: HWND,
    parent_hwnd: HWND,
    /// 位置轮询线程停止标志
    stop_flag: Arc<AtomicBool>,
    /// 位置轮询线程句柄
    poll_thread: Option<std::thread::JoinHandle<()>>,
    /// 窗口位置同步线程句柄
    hook_thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
unsafe impl Send for Player {}
#[cfg(windows)]
unsafe impl Sync for Player {}

#[cfg(windows)]
impl Player {
    /// 创建新的 libmpv 播放器
    /// - `dll_path`: libmpv.dll 的绝对路径
    /// - `parent_hwnd`: 父窗口（Tauri 主窗口）的 HWND
    /// - `app_handle`: Tauri AppHandle，用于 emit 事件
    /// - `x, y, w, h`: 子窗口在父窗口中的位置和大小（物理像素）
    pub fn new(
        dll_path: &str,
        parent_hwnd: HWND,
        app_handle: tauri::AppHandle,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Self, AppError> {
        // 存储 AppHandle 供 child_wnd_proc 发送点击事件
        let _ = GLOBAL_APP_HANDLE.set(app_handle.clone());
        unsafe {
            let tid = GetCurrentThreadId();
            tracing::info!("Player::new: 线程ID={}, 加载 dll: {}", tid, dll_path);
            let api = MpvApi::load(dll_path)?;
            tracing::info!("Player::new: dll 加载成功");
            // 创建子窗口
            let child_hwnd = create_child_window(parent_hwnd, x, y, w, h)?;
            tracing::info!("Player::new: 子窗口创建成功: HWND={:?}, 创建线程={}", child_hwnd, tid);
            // 创建 mpv 实例
            tracing::info!("Player::new: 调用 mpv_create");
            let mpv = (api.create)();
            if mpv.is_null() {
                return Err(AppError::PlayerInitFailed { code: "mpv_create returned null".to_string() });
            }
            tracing::info!("Player::new: mpv_create 成功: {:?}", mpv);
            // 设置 wid（将 libmpv 渲染嵌入子窗口）
            let wid_str = format!("{}", child_hwnd.0 as isize);
            tracing::info!("Player::new: 设置 wid={}", wid_str);
            let wid_c = CString::new(wid_str).unwrap();
            let name_c = CString::new("wid").unwrap();
            let ret = (api.set_option_string)(mpv, name_c.as_ptr(), wid_c.as_ptr());
            if ret < 0 {
                (api.terminate_destroy)(mpv);
                return Err(AppError::PlayerSetWidFailed { code: ret.to_string() });
            }
            // 禁用 mpv 自带 OSD
            set_option(&api, mpv, "osd-level", "0")?;
            // 禁用 mpv 自带屏幕控制器（osc）
            set_option(&api, mpv, "osc", "no")?;
            // 禁用 mpv 自带字幕渲染
            set_option(&api, mpv, "sid", "no")?;
            // 禁用 mpv 默认输入绑定（避免拦截键盘）
            set_option(&api, mpv, "input-default-bindings", "no")?;
            // 硬件解码
            set_option(&api, mpv, "hwdec", "auto")?;
            // 设置 vo 为 gpu（GPL 版支持完整 GPU 渲染，wid 模式下用 d3d11 后端）
            set_option(&api, mpv, "vo", "gpu")?;
            // 初始化
            tracing::info!("Player::new: 调用 mpv_initialize");
            let ret = (api.initialize)(mpv);
            if ret < 0 {
                (api.terminate_destroy)(mpv);
                return Err(AppError::PlayerInitFailed { code: ret.to_string() });
            }
            tracing::info!("Player::new: mpv_initialize 成功");
            // 初始化后再次确认禁用 osc（有些选项需要运行时设置）
            set_property(&api, mpv, "osc", "no")?;
            set_property(&api, mpv, "osd-level", "0")?;
            // 启动位置轮询线程
            let stop_flag = Arc::new(AtomicBool::new(false));
            let poll_handle = app_handle.clone();
            let mpv_addr = mpv as usize;
            let api_get_prop = api.get_property_string as usize;
            let api_free = api.free as usize;
            let stop_clone = stop_flag.clone();
            let thread = std::thread::spawn(move || {
                poll_position_loop(poll_handle, mpv_addr, api_get_prop, api_free, stop_clone);
            });

            // 启动窗口位置同步线程：高频轮询父窗口位置，实时同步悬浮窗口
            let parent_addr = parent_hwnd.0 as usize;
            let child_addr = child_hwnd.0 as usize;
            let stop_for_hook = stop_flag.clone();
            let hook_thread = std::thread::spawn(move || {
                let parent = HWND(parent_addr as *mut _);
                let child = HWND(child_addr as *mut _);
                position_sync_loop(parent, child, stop_for_hook);
            });

            Ok(Player {
                api,
                mpv,
                child_hwnd,
                parent_hwnd,
                stop_flag,
                poll_thread: Some(thread),
                hook_thread: Some(hook_thread),
            })
        }
    }

    /// 加载视频文件
    pub fn load(&self, file_path: &str) -> Result<(), AppError> {
        tracing::info!("player load: {}", file_path);
        unsafe {
            let cmd = "loadfile";
            let path_c = CString::new(file_path).unwrap();
            let cmd_c = CString::new(cmd).unwrap();
            let null_ptr: *const c_char = ptr::null();
            let args: [*const c_char; 3] = [cmd_c.as_ptr(), path_c.as_ptr(), null_ptr];
            let ret = (self.api.command)(self.mpv, args.as_ptr());
            tracing::info!("player load: mpv_command 返回 {}", ret);
            if ret < 0 {
                return Err(AppError::PlayerLoadVideoFailed { path: file_path.to_string(), code: ret.to_string() });
            }
            Ok(())
        }
    }

    /// 播放
    pub fn play(&self) -> Result<(), AppError> {
        tracing::info!("player play: 设置 pause=no");
        set_property(&self.api, self.mpv, "pause", "no")
    }

    /// 暂停
    pub fn pause(&self) -> Result<(), AppError> {
        tracing::info!("player pause: 设置 pause=yes");
        set_property(&self.api, self.mpv, "pause", "yes")
    }

    /// 跳转到指定时间（秒）
    pub fn seek(&self, time_sec: f64) -> Result<(), AppError> {
        tracing::info!("player seek: {}", time_sec);
        unsafe {
            let cmd = "seek";
            let time_str = format!("{}", time_sec);
            let cmd_c = CString::new(cmd).unwrap();
            let time_c = CString::new(time_str).unwrap();
            let mode_c = CString::new("absolute").unwrap();
            let null_ptr: *const c_char = ptr::null();
            let args: [*const c_char; 4] = [cmd_c.as_ptr(), time_c.as_ptr(), mode_c.as_ptr(), null_ptr];
            let ret = (self.api.command)(self.mpv, args.as_ptr());
            tracing::info!("player seek: ret={}", ret);
            if ret < 0 {
                return Err(AppError::PlayerSeekFailed { code: ret.to_string() });
            }
            Ok(())
        }
    }

    /// 设置音量 (0-100)
    pub fn set_volume(&self, vol: i32) -> Result<(), AppError> {
        tracing::info!("player set_volume: {}", vol);
        set_property(&self.api, self.mpv, "volume", &vol.to_string())
    }

    /// 设置倍速
    pub fn set_speed(&self, speed: f64) -> Result<(), AppError> {
        tracing::info!("player set_speed: {}", speed);
        set_property(&self.api, self.mpv, "speed", &speed.to_string())
    }

    /// 设置音频轨道（mpv aid，1-based 音频流序号）
    pub fn set_audio_track(&self, audio_id: i32) -> Result<(), AppError> {
        tracing::info!("player set_audio_track: {}", audio_id);
        set_property(&self.api, self.mpv, "aid", &audio_id.to_string())
    }

    /// 获取当前播放位置（秒）
    pub fn get_position(&self) -> Result<f64, AppError> {
        unsafe {
            let name_c = CString::new("time-pos").unwrap();
            let ptr = (self.api.get_property_string)(self.mpv, name_c.as_ptr());
            if ptr.is_null() {
                return Ok(0.0);
            }
            let s = std::ffi::CStr::from_ptr(ptr).to_string_lossy().to_string();
            (self.api.free)(ptr as *mut c_void);
            s.parse::<f64>().map_err(|_| AppError::PlayerLoadFailed { detail: "parse time-pos failed".to_string() })
        }
    }

    /// 获取视频时长（秒）
    pub fn get_duration(&self) -> Result<f64, AppError> {
        unsafe {
            let name_c = CString::new("duration").unwrap();
            let ptr = (self.api.get_property_string)(self.mpv, name_c.as_ptr());
            if ptr.is_null() {
                return Ok(0.0);
            }
            let s = std::ffi::CStr::from_ptr(ptr).to_string_lossy().to_string();
            (self.api.free)(ptr as *mut c_void);
            s.parse::<f64>().map_err(|_| AppError::PlayerLoadFailed { detail: "parse duration failed".to_string() })
        }
    }

    /// 调整子窗口位置和大小
    pub fn resize(&self, x: i32, y: i32, w: i32, h: i32) -> Result<(), AppError> {
        unsafe {
            // 悬浮窗口：将父窗口客户区坐标转为屏幕坐标
            let mut point = windows::Win32::Foundation::POINT { x, y };
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(self.parent_hwnd, &mut point);
            // 子窗口被主动隐藏时（弹窗层级处理），只更新位置不恢复显示，
            // 避免 SWP_SHOWWINDOW 把刚 hide 的窗口又拉回。show() 恢复时窗口已在正确位置。
            let flags = if HOOK_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
                SWP_NOZORDER
            } else {
                SWP_NOZORDER | SWP_SHOWWINDOW
            };
            let _ = SetWindowPos(
                self.child_hwnd,
                None,
                point.x, point.y, w, h,
                flags,
            );
        }
        Ok(())
    }

    /// 显示子窗口
    pub fn show(&self) {
        #[cfg(windows)]
        HOOK_HIDDEN.store(false, std::sync::atomic::Ordering::Relaxed);
        unsafe { let _ = ShowWindow(self.child_hwnd, SW_SHOW); }
    }

    /// 隐藏子窗口（用于弹窗层级处理）
    pub fn hide(&self) {
        #[cfg(windows)]
        HOOK_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
        unsafe { let _ = ShowWindow(self.child_hwnd, SW_HIDE); }
    }

    /// 销毁播放器
    pub fn destroy(&mut self) {
        let tid = unsafe { GetCurrentThreadId() };
        tracing::info!("Player::destroy 开始, child_hwnd={:?}, 销毁线程={}", self.child_hwnd, tid);
        // 在 join 线程之前先隐藏窗口并设置 HOOK_HIDDEN，
        // 防止 position_sync_loop 的钩子回调在销毁过程中用 SWP_SHOWWINDOW
        // 把子窗口移动到错误位置（导航切换时 DOM 变化触发 LOCATIONCHANGE 事件）。
        #[cfg(windows)]
        {
            HOOK_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
            unsafe { let _ = ShowWindow(self.child_hwnd, SW_HIDE); }
        }
        self.stop_flag.store(true, Ordering::Relaxed);
        unsafe { (self.api.wakeup)(self.mpv); }
        if let Some(t) = self.poll_thread.take() {
            let _ = t.join();
            tracing::info!("Player::destroy: poll_thread 已 join");
        }
        if let Some(t) = self.hook_thread.take() {
            let _ = t.join();
            tracing::info!("Player::destroy: hook_thread 已 join");
        }
        unsafe {
            // 隐藏窗口
            let _ = ShowWindow(self.child_hwnd, SW_HIDE);
            // 在 mpv_terminate_destroy 前后都调用 OleInitialize，且永不调用 OleUninitialize。
            // mpv 内部可能多次调用 CoUninitialize/OleUninitialize，
            // 我们通过不断增加 OLE 引用计数来抵消，确保 OLE 永不卸载。
            let _ = windows::Win32::System::Ole::OleInitialize(None);
            // 销毁 mpv
            (self.api.terminate_destroy)(self.mpv);
            // mpv 销毁后再次增加 OLE 引用计数
            let _ = windows::Win32::System::Ole::OleInitialize(None);
            // 销毁窗口
            let ret = DestroyWindow(self.child_hwnd);
            if ret.is_err() {
                tracing::warn!("DestroyWindow 失败: {:?}, 错误码={:?}", ret, windows::Win32::Foundation::GetLastError());
            } else {
                tracing::info!("DestroyWindow 成功, 销毁线程={}", tid);
            }
        }
        tracing::info!("Player::destroy 完成");
    }
}

#[cfg(windows)]
impl Drop for Player {
    fn drop(&mut self) {
        self.destroy();
    }
}

// === SECTION 4 END ===

// === 辅助函数 ===

#[cfg(windows)]
fn set_option(api: &MpvApi, mpv: *mut MpvHandle, name: &str, value: &str) -> Result<(), AppError> {
    unsafe {
        let name_c = CString::new(name).unwrap();
        let value_c = CString::new(value).unwrap();
        let ret = (api.set_option_string)(mpv, name_c.as_ptr(), value_c.as_ptr());
        if ret < 0 {
            return Err(AppError::PlayerSetOptionFailed {
                name: name.to_string(), value: value.to_string(), code: ret.to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(windows)]
fn set_property(api: &MpvApi, mpv: *mut MpvHandle, name: &str, value: &str) -> Result<(), AppError> {
    unsafe {
        let name_c = CString::new(name).unwrap();
        let value_c = CString::new(value).unwrap();
        let ret = (api.set_property_string)(mpv, name_c.as_ptr(), value_c.as_ptr());
        tracing::info!("set_property {}={}: ret={}", name, value, ret);
        if ret < 0 {
            return Err(AppError::PlayerSetPropertyFailed {
                name: name.to_string(), value: value.to_string(), code: ret.to_string(),
            });
        }
        Ok(())
    }
}

/// 窗口过程：不擦除背景，让 mpv 直接渲染
/// 捕获 WM_LBUTTONDOWN：单击视频区域切换播放/暂停（WS_EX_TRANSPARENT 穿透不可靠，
/// 改为在子窗口直接捕获点击并通过 Tauri 事件通知前端）
#[cfg(windows)]
unsafe extern "system" fn child_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        0x0014 => return LRESULT(1), // WM_ERASEBKGND
        0x0201 => { // WM_LBUTTONDOWN
            // 通知前端切换播放/暂停
            use tauri::Emitter;
            if let Some(app) = GLOBAL_APP_HANDLE.get() {
                let _ = app.emit("player-click", ());
            }
            return LRESULT(0);
        }
        0x0204 => { // WM_RBUTTONDOWN：右键视频区域，emit 屏幕坐标给前端弹自定义菜单
            use tauri::Emitter;
            // lparam 的低 16 位是客户区 x，高 16 位是 y
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            // 转为屏幕坐标，前端用屏幕坐标定位菜单（fixed 定位 + window.screenX/Y）
            let mut pt = windows::Win32::Foundation::POINT { x, y };
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut pt);
            if let Some(app) = GLOBAL_APP_HANDLE.get() {
                let _ = app.emit("player-right-click", (pt.x, pt.y));
            }
            return LRESULT(0);
        }
        0x0205 => return LRESULT(0), // WM_RBUTTONUP：吞掉，防止 DefWindowProc 生成 WM_CONTEXTMENU
        0x007B => return LRESULT(0), // WM_CONTEXTMENU：吞掉，阻止系统默认右键菜单
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn create_child_window(parent: HWND, x: i32, y: i32, w: i32, h: i32) -> Result<HWND, AppError> {
    unsafe {
        // 将父窗口客户区坐标转为屏幕坐标
        let mut point = windows::Win32::Foundation::POINT { x, y };
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(parent, &mut point);

        // 注册自定义窗口类
        let class_name = windows::core::w!("MpvFloatWnd");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(child_wnd_proc),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            style: CS_HREDRAW | CS_VREDRAW,
            hbrBackground: HBRUSH::default(),
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        // WS_POPUP 悬浮窗口：不受 WebView2 遮挡
        // WS_EX_TRANSPARENT: 鼠标事件穿透，前端控制器可点击
        let hwnd = CreateWindowExW(
            WS_EX_TRANSPARENT,
            class_name,
            windows::core::w!("MpvPlayer"),
            WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS,
            point.x, point.y, w, h,
            Some(parent),
            None,
            None,
            None,
        ).map_err(|e| AppError::PlayerCreateWindowFailed {
            detail: e.to_string(),
        })?;
        tracing::info!("悬浮窗口创建成功: 屏幕坐标=({},{}), 大小={}x{}", point.x, point.y, w, h);
        Ok(hwnd)
    }
}

/// 窗口位置同步：用 SetWinEventHook 监听父窗口位置变化，
/// 事件驱动，无轮询延迟，悬浮窗口实时跟随。
#[cfg(windows)]
fn position_sync_loop(parent: HWND, child: HWND, stop_flag: Arc<AtomicBool>) {
    // 保存 parent/child 到全局，供回调使用
    unsafe {
        HOOK_PARENT = parent;
        HOOK_CHILD = child;
    }
    // 回调函数：父窗口位置变化时，用 delta 移动悬浮窗口
    unsafe extern "system" fn hook_callback(
        _hWinEventHook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        _idObject: i32,
        _idChild: i32,
        _dwEventThread: u32,
        _dwmsEventTime: u32,
    ) {
        // EVENT_OBJECT_LOCATIONCHANGE = 0x800B
        // 只处理父窗口的位置变化
        if event != 0x800B || hwnd.0 != HOOK_PARENT.0 {
            return;
        }
        // 子窗口被主动隐藏时（弹窗层级处理），跳过位置同步，
        // 避免 SetWindowPos(SWP_SHOWWINDOW) 把刚 hide 的窗口又拉回。
        if HOOK_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let parent = HOOK_PARENT;
        let child = HOOK_CHILD;
        let mut tl = windows::Win32::Foundation::POINT { x: 0, y: 0 };
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(parent, &mut tl);
        if HOOK_LAST_X != i32::MIN {
            let dx = tl.x - HOOK_LAST_X;
            let dy = tl.y - HOOK_LAST_Y;
            if dx != 0 || dy != 0 {
                let mut rect = windows::Win32::Foundation::RECT::default();
                if GetWindowRect(child, &mut rect).is_ok() {
                    let _ = SetWindowPos(
                        child, None,
                        rect.left + dx, rect.top + dy, 0, 0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
                    );
                }
            }
        }
        HOOK_LAST_X = tl.x;
        HOOK_LAST_Y = tl.y;
    }

    unsafe {
        // 注册 Win32 事件钩子：监听父窗口的位置变化
        let hook = windows::Win32::UI::Accessibility::SetWinEventHook(
            0x800B, // EVENT_OBJECT_LOCATIONCHANGE
            0x800B,
            None,
            Some(hook_callback),
            0, // 所有进程
            0,
            0x0000, // WINEVENT_OUTOFCONTEXT
        );
        if hook.is_invalid() {
            tracing::warn!("SetWinEventHook 注册失败，回退到轮询模式");
            // 回退到轮询
            fallback_poll_loop(parent, child, &stop_flag);
            return;
        }
        tracing::info!("SetWinEventHook 注册成功，事件驱动位置同步");
        // 消息循环：Win32 钩子回调需要消息循环来分发
        let mut msg = MSG::default();
        while !stop_flag.load(Ordering::Relaxed) {
            // 处理消息，非阻塞
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // 清理：先重置全局变量，让回调停止处理事件，再 UnhookWinEvent
        HOOK_PARENT = HWND::default();
        HOOK_CHILD = HWND::default();
        HOOK_LAST_X = i32::MIN;
        HOOK_LAST_Y = i32::MIN;
        HOOK_HIDDEN.store(false, Ordering::Relaxed);
        let unhook_ret = windows::Win32::UI::Accessibility::UnhookWinEvent(hook);
        tracing::info!("UnhookWinEvent 返回: {:?}", unhook_ret);
    }
}

/// 回退轮询模式
#[cfg(windows)]
fn fallback_poll_loop(parent: HWND, child: HWND, stop_flag: &Arc<AtomicBool>) {
    let mut last_x = i32::MIN;
    let mut last_y = i32::MIN;
    while !stop_flag.load(Ordering::Relaxed) {
        // 子窗口被主动隐藏时（弹窗层级处理），跳过位置同步，
        // 避免 SetWindowPos(SWP_SHOWWINDOW) 把刚 hide 的窗口又拉回。
        if HOOK_HIDDEN.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(8));
            continue;
        }
        unsafe {
            let mut tl = windows::Win32::Foundation::POINT { x: 0, y: 0 };
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(parent, &mut tl);
            if last_x != i32::MIN {
                let dx = tl.x - last_x;
                let dy = tl.y - last_y;
                if dx != 0 || dy != 0 {
                    let mut rect = windows::Win32::Foundation::RECT::default();
                    if GetWindowRect(child, &mut rect).is_ok() {
                        let _ = SetWindowPos(
                            child, None,
                            rect.left + dx, rect.top + dy, 0, 0,
                            SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
                        );
                    }
                }
            }
            last_x = tl.x;
            last_y = tl.y;
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

/// 位置轮询线程：10Hz 推送 player_position 事件
#[cfg(windows)]
fn poll_position_loop(
    app: tauri::AppHandle,
    mpv_addr: usize,
    api_get_prop: usize,
    api_free: usize,
    stop_flag: Arc<AtomicBool>,
) {
    let get_prop: FnMpvGetPropertyString = unsafe { std::mem::transmute(api_get_prop) };
    let free_fn: FnMpvFree = unsafe { std::mem::transmute(api_free) };
    let mpv = mpv_addr as *mut MpvHandle;
    while !stop_flag.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(100)); // 10Hz
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        unsafe {
            // 获取 time-pos
            let name_c = CString::new("time-pos").unwrap();
            let pos_ptr = (get_prop)(mpv, name_c.as_ptr());
            let pos = if pos_ptr.is_null() {
                None
            } else {
                let s = std::ffi::CStr::from_ptr(pos_ptr).to_string_lossy().to_string();
                (free_fn)(pos_ptr as *mut c_void);
                s.parse::<f64>().ok()
            };
            // 获取 duration
            let name_c2 = CString::new("duration").unwrap();
            let dur_ptr = (get_prop)(mpv, name_c2.as_ptr());
            let dur = if dur_ptr.is_null() {
                None
            } else {
                let s = std::ffi::CStr::from_ptr(dur_ptr).to_string_lossy().to_string();
                (free_fn)(dur_ptr as *mut c_void);
                s.parse::<f64>().ok()
            };
            // 获取 pause 状态
            let name_c3 = CString::new("pause").unwrap();
            let pause_ptr = (get_prop)(mpv, name_c3.as_ptr());
            let paused = if pause_ptr.is_null() {
                false
            } else {
                let s = std::ffi::CStr::from_ptr(pause_ptr).to_string_lossy().to_string();
                (free_fn)(pause_ptr as *mut c_void);
                s == "yes"
            };
            // emit 事件
            use tauri::Emitter;
            let _ = app.emit("player_position", serde_json::json!({
                "position": pos.unwrap_or(0.0),
                "duration": dur.unwrap_or(0.0),
                "paused": paused,
            }));
        }
    }
}

// === SECTION 5 END ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_libmpv_status_not_downloaded() {
        let status = LibmpvStatus::not_downloaded();
        assert!(!status.downloaded);
        assert!(status.path.is_none());
        assert!(status.version.is_none());
    }

    #[test]
    fn test_libmpv_status_serialize() {
        let status = LibmpvStatus {
            downloaded: true,
            path: Some("C:\\app\\libmpv\\libmpv.dll".to_string()),
            version: Some("2.1".to_string()),
        };
        let json = serde_json::to_string(&status).expect("序列化失败");
        assert!(json.contains("\"downloaded\":true"));
        assert!(status.path.as_deref() == Some("C:\\app\\libmpv\\libmpv.dll"));
        assert!(json.contains("\"version\":\"2.1\""));
    }

    #[test]
    fn test_libmpv_status_deserialize() {
        let json = r#"{"downloaded":false,"path":null,"version":null}"#;
        let status: LibmpvStatus = serde_json::from_str(json).expect("反序列化失败");
        assert!(!status.downloaded);
        assert!(status.path.is_none());
        assert!(status.version.is_none());
    }

    #[test]
    fn test_libmpv_status_roundtrip() {
        let original = LibmpvStatus {
            downloaded: true,
            path: Some("/tmp/libmpv.dll".to_string()),
            version: None,
        };
        let json = serde_json::to_string(&original).expect("序列化失败");
        let parsed: LibmpvStatus = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(original, parsed);
    }
}
