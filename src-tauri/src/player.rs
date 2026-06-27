// libmpv 播放器模块
// 负责下载、检测 libmpv.dll，动态加载并内嵌播放视频。
// 对应需求文档 §2.2 方案 B（原生子窗口嵌入）：libmpv 创建子窗口，通过 wid 嵌入 Tauri 主窗口。

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

// WinEventHook 全局状态（回调函数无法传递上下文，只能用全局变量）
#[cfg(windows)]
static mut HOOK_PARENT: HWND = HWND(std::ptr::null_mut());
#[cfg(windows)]
static mut HOOK_CHILD: HWND = HWND(std::ptr::null_mut());
#[cfg(windows)]
static mut HOOK_LAST_X: i32 = i32::MIN;
#[cfg(windows)]
static mut HOOK_LAST_Y: i32 = i32::MIN;

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
/// SourceForge libmpv RSS feed（获取最新 mpv-dev-x86_64 包）
const SF_LIBMPV_RSS: &str = "https://sourceforge.net/projects/mpv-player-windows/rss?path=/libmpv";

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

/// 从 SourceForge RSS 中解析最新 mpv-dev-x86_64-*.7z 的下载 URL（排除 v3/i686/aarch64）
fn parse_latest_dev_url(rss_text: &str) -> Option<String> {
    // RSS 中 link 格式：https://sourceforge.net/.../libmpv/mpv-dev-x86_64-日期-git-哈希.7z/download
    for line in rss_text.lines() {
        let trimmed = line.trim();
        if trimmed.contains("mpv-dev-x86_64-") && trimmed.contains("/download") && !trimmed.contains("v3") {
            // 提取 URL
            if let Some(start) = trimmed.find("https://") {
                let url = &trimmed[start..];
                let end = url.find(".7z/download").map(|i| i + ".7z/download".len()).unwrap_or(url.len());
                return Some(url[..end].to_string());
            }
        }
    }
    None
}

/// 下载 libmpv：从 SourceForge 获取最新 mpv-dev-x86_64 包，
/// 流式下载（emit 进度事件），解压并提取 libmpv-2.dll，重命名为 libmpv.dll
pub fn download_libmpv(
    app_data_dir: &Path,
    proxy: Option<&str>,
    app_handle: &tauri::AppHandle,
) -> Result<(), AppError> {
    use tauri::Emitter;

    let dir = libmpv_dir(app_data_dir);
    fs::create_dir_all(&dir).map_err(|e| AppError::PlayerLibmpvDownloadFailed {
        detail: format!("创建目录失败: {}", e),
    })?;

    let mut client_builder = reqwest::blocking::Client::builder()
        .user_agent("zimufan/1.0")
        .timeout(std::time::Duration::from_secs(600));
    if let Some(p) = proxy {
        if !p.is_empty() {
            client_builder = client_builder.proxy(
                reqwest::Proxy::all(p).map_err(|e| AppError::PlayerLibmpvDownloadFailed {
                    detail: format!("代理配置失败: {}", e),
                })?,
            );
        }
    }
    let client = client_builder.build().map_err(|e| AppError::PlayerLibmpvDownloadFailed {
        detail: format!("HTTP 客户端构建失败: {}", e),
    })?;

    // 1. 从 SourceForge RSS 获取最新 mpv-dev-x86_64 包 URL
    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "fetching", "progress": 0, "message": "正在获取最新版本信息..."
    }));
    tracing::info!("正在获取 SourceForge libmpv RSS...");
    let rss_resp = client.get(SF_LIBMPV_RSS).send().map_err(|e| AppError::PlayerLibmpvDownloadFailed {
        detail: format!("RSS 请求失败: {}", e),
    })?;
    if !rss_resp.status().is_success() {
        return Err(AppError::PlayerLibmpvDownloadFailed { detail: format!("RSS 状态码异常: {}", rss_resp.status()) });
    }
    let rss_text = rss_resp.text().map_err(|e| AppError::PlayerLibmpvDownloadFailed {
        detail: format!("读取 RSS 失败: {}", e),
    })?;
    let download_url = parse_latest_dev_url(&rss_text)
        .ok_or_else(|| AppError::PlayerLibmpvDownloadFailed { detail: "RSS 中未找到 mpv-dev-x86_64 包".to_string() })?;

    tracing::info!("下载 libmpv: {}", download_url);

    // 2. 流式下载 7z 文件，emit 进度
    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "downloading", "progress": 0, "message": "开始下载..."
    }));
    let response = client.get(&download_url).send().map_err(|e| AppError::PlayerLibmpvDownloadFailed {
        detail: format!("下载请求失败: {}", e),
    })?;
    if !response.status().is_success() {
        return Err(AppError::PlayerLibmpvDownloadFailed { detail: format!("下载 HTTP 状态码异常: {}", response.status()) });
    }
    let total_size = response.content_length().unwrap_or(0);
    let archive_path = libmpv_archive_path(app_data_dir);
    let mut file = fs::File::create(&archive_path).map_err(|e| AppError::PlayerLibmpvDownloadFailed {
        detail: format!("创建文件失败: {}", e),
    })?;
    use std::io::{Read, Write};
    let mut stream = response;
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    loop {
        let n = stream.read(&mut buf).map_err(|e| AppError::PlayerLibmpvDownloadFailed {
            detail: format!("读取下载流失败: {}", e),
        })?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).map_err(|e| AppError::PlayerLibmpvDownloadFailed {
            detail: format!("写入文件失败: {}", e),
        })?;
        downloaded += n as u64;
        // 每 200ms emit 一次进度
        if last_emit.elapsed() > std::time::Duration::from_millis(200) {
            let pct = if total_size > 0 { (downloaded * 100 / total_size) as u8 } else { 0 };
            let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
                "stage": "downloading", "progress": pct,
                "downloaded": downloaded, "total": total_size,
                "message": format!("下载中 {}% ({} / {} MB)",
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
    fs::create_dir_all(&extract_dir).map_err(|e| AppError::PlayerLibmpvDownloadFailed {
        detail: format!("创建解压目录失败: {}", e),
    })?;
    sevenz_rust::decompress_file(&archive_path, &extract_dir)
        .map_err(|e| AppError::PlayerLibmpvDownloadFailed {
            detail: format!("解压 7z 失败: {}", e),
        })?;
    let _ = app_handle.emit("libmpv_download_progress", serde_json::json!({
        "stage": "extracting", "progress": 90, "message": "正在安装 DLL..."
    }));

    // 4. 查找 libmpv-2.dll（dev 包中通常在 dll/ 目录下）
    let dll_source = find_file(&extract_dir, "libmpv-2.dll")
        .or_else(|| find_file(&extract_dir, "mpv-2.dll"))
        .ok_or_else(|| AppError::PlayerLibmpvDownloadFailed { detail: "解压后未找到 libmpv-2.dll 或 mpv-2.dll".to_string() })?;

    // 5. 复制并重命名为 libmpv.dll
    let dll_dest = libmpv_dll_path(app_data_dir);
    fs::copy(&dll_source, &dll_dest).map_err(|e| AppError::PlayerLibmpvDownloadFailed {
        detail: format!("复制 dll 失败: {}", e),
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
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", video_path])
        .status()
        .map_err(|e| AppError::PlayerLoadFailed { video_path: format!("{} ({})", video_path, e) })?;
    if !status.success() {
        return Err(AppError::PlayerLoadFailed { video_path: video_path.to_string() });
    }
    Ok(())
}

// === SECTION 2 END ===

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
        let lib = Library::new(dll_path).map_err(|e| AppError::PlayerLoadFailed {
            video_path: format!("加载 libmpv.dll 失败: {} ({})", dll_path, e),
        })?;
        macro_rules! sym {
            ($name:literal, $type:ty) => {
                *lib.get::<$type>(concat!($name, "\0").as_bytes())
                    .map_err(|e| AppError::PlayerLoadFailed { video_path: format!("符号 {} 未找到: {}", $name, e) })?
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
    _hook_thread: Option<std::thread::JoinHandle<()>>,
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
        unsafe {
            tracing::info!("Player::new: 加载 dll: {}", dll_path);
            let api = MpvApi::load(dll_path)?;
            tracing::info!("Player::new: dll 加载成功");
            // 创建子窗口
            let child_hwnd = create_child_window(parent_hwnd, x, y, w, h)?;
            tracing::info!("Player::new: 子窗口创建成功: {:?}", child_hwnd);
            // 创建 mpv 实例
            tracing::info!("Player::new: 调用 mpv_create");
            let mpv = (api.create)();
            if mpv.is_null() {
                return Err(AppError::PlayerLoadFailed { video_path: "mpv_create 返回 null".to_string() });
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
                return Err(AppError::PlayerLoadFailed { video_path: format!("设置 wid 失败: {}", ret) });
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
            // 设置 vo 为 direct3d（wid 模式下最兼容）
            set_option(&api, mpv, "vo", "direct3d")?;
            // 初始化
            tracing::info!("Player::new: 调用 mpv_initialize");
            let ret = (api.initialize)(mpv);
            if ret < 0 {
                (api.terminate_destroy)(mpv);
                return Err(AppError::PlayerLoadFailed { video_path: format!("mpv_initialize 失败: {}", ret) });
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
                _hook_thread: Some(hook_thread),
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
                return Err(AppError::PlayerLoadFailed { video_path: format!("加载视频失败: {} ({})", file_path, ret) });
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
                return Err(AppError::PlayerLoadFailed { video_path: format!("seek 失败: {}", ret) });
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
            s.parse::<f64>().map_err(|_| AppError::PlayerLoadFailed { video_path: "解析 time-pos 失败".to_string() })
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
            s.parse::<f64>().map_err(|_| AppError::PlayerLoadFailed { video_path: "解析 duration 失败".to_string() })
        }
    }

    /// 调整子窗口位置和大小
    pub fn resize(&self, x: i32, y: i32, w: i32, h: i32) -> Result<(), AppError> {
        unsafe {
            // 悬浮窗口：将父窗口客户区坐标转为屏幕坐标
            let mut point = windows::Win32::Foundation::POINT { x, y };
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(self.parent_hwnd, &mut point);
            tracing::info!("player_resize: 屏幕坐标=({},{}), 大小={}x{}", point.x, point.y, w, h);
            let _ = SetWindowPos(
                self.child_hwnd,
                None,
                point.x, point.y, w, h,
                SWP_NOZORDER | SWP_SHOWWINDOW,
            );
        }
        Ok(())
    }

    /// 显示子窗口
    pub fn show(&self) {
        unsafe { let _ = ShowWindow(self.child_hwnd, SW_SHOW); }
    }

    /// 隐藏子窗口（用于弹窗层级处理）
    pub fn hide(&self) {
        unsafe { let _ = ShowWindow(self.child_hwnd, SW_HIDE); }
    }

    /// 销毁播放器
    pub fn destroy(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        unsafe { (self.api.wakeup)(self.mpv); }
        if let Some(t) = self.poll_thread.take() {
            let _ = t.join();
        }
        unsafe {
            (self.api.terminate_destroy)(self.mpv);
            let _ = DestroyWindow(self.child_hwnd);
        }
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
            return Err(AppError::PlayerLoadFailed {
                video_path: format!("设置选项 {}={} 失败: {}", name, value, ret),
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
            return Err(AppError::PlayerLoadFailed {
                video_path: format!("设置属性 {}={} 失败: {}", name, value, ret),
            });
        }
        Ok(())
    }
}

/// 窗口过程：不擦除背景，让 mpv 直接渲染
#[cfg(windows)]
unsafe extern "system" fn child_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        0x0014 => return LRESULT(1), // WM_ERASEBKGND
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
        ).map_err(|e| AppError::PlayerLoadFailed {
            video_path: format!("创建悬浮窗口失败: {}", e),
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
        // 清理
        let _ = windows::Win32::UI::Accessibility::UnhookWinEvent(hook);
        HOOK_PARENT = HWND::default();
        HOOK_CHILD = HWND::default();
        HOOK_LAST_X = i32::MIN;
        HOOK_LAST_Y = i32::MIN;
    }
}

/// 回退轮询模式
#[cfg(windows)]
fn fallback_poll_loop(parent: HWND, child: HWND, stop_flag: &Arc<AtomicBool>) {
    let mut last_x = i32::MIN;
    let mut last_y = i32::MIN;
    while !stop_flag.load(Ordering::Relaxed) {
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
    let mut log_counter = 0u32;
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
            // 每秒输出一次调试日志
            log_counter += 1;
            if log_counter % 10 == 0 {
                tracing::info!("poll: time-pos={:?}, duration={:?}, paused={}", pos, dur, paused);
            }
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
        assert!(json.contains("\"path\":\"C:\\\\app\\libmpv\\\\libmpv.dll\""));
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
