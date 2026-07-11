// AI-SubTrans (zimufan) - AI 字幕翻译与编辑工具
// 后端主入口

pub mod db;
pub mod error;
pub mod ffmpeg;
pub mod subtitle;
pub mod translate;
pub mod config;
pub mod ipc;
pub mod search;
pub mod context_menu;
pub mod player;
pub mod batch;

use tauri::{Manager, Listener};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use crate::db::Database;

/// 解析命令行参数，提取运行模式和文件路径
/// --mode=quick: 右键视频静默流程（提取→翻译→合并）
/// --mode=edit: 右键字幕静默编辑模式
/// 无 --mode: 正常启动
pub struct CliArgs {
    pub mode: Option<String>,
    pub file_path: Option<String>,
}

pub fn parse_cli_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut mode: Option<String> = None;
    let mut file_path: Option<String> = None;

    for arg in &args {
        if let Some(m) = arg.strip_prefix("--mode=") {
            mode = Some(m.to_string());
        } else if !arg.starts_with("--") && !arg.ends_with(".exe") && std::path::Path::new(arg).exists() {
            file_path = Some(arg.to_string());
        }
    }

    tracing::info!("CLI args: mode={:?}, file_path={:?}", mode, file_path);
    CliArgs { mode, file_path }
}

/// 初始化日志系统（按天滚动，保留 7 天）
pub fn init_logging(app_data_dir: &std::path::Path) -> tracing_appender::non_blocking::WorkerGuard {
    let log_dir = app_data_dir.join("logs");
    std::fs::create_dir_all(&log_dir).ok();

    // 清理 7 天前的旧日志文件
    cleanup_old_logs(&log_dir, 7);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "zimufan.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,zimufan_lib=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_writer(non_blocking))
        .init();

    guard
}

/// 清理超过指定天数的日志文件
fn cleanup_old_logs(log_dir: &std::path::Path, retain_days: u64) {
    if !log_dir.exists() {
        return;
    }
    let now = std::time::SystemTime::now();
    let cutoff = std::time::Duration::from_secs(retain_days * 24 * 3600);
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            // 只清理文件（不递归子目录），且文件名以 zimufan.log 开头
            if !path.is_file() {
                continue;
            }
            if let Ok(name) = entry.file_name().into_string() {
                if !name.starts_with("zimufan.log") {
                    continue;
                }
            } else {
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age > cutoff && std::fs::remove_file(&path).is_ok() {
                            removed += 1;
                        }
                    }
                }
            }
        }
    }
    if removed > 0 {
        // 用 eprintln 避免在 tracing 初始化前调用 tracing
        eprintln!("[cleanup_old_logs] 已清理 {} 个超过 {} 天的旧日志文件", removed, retain_days);
    }
}

/// 安装 panic hook：将崩溃信息写入崩溃日志文件
/// 崩溃日志位于 app_data_dir/crashes/crash_YYYY-MM-DD_HH-MM-SS.log
/// 包含：时间戳、panic 位置、消息、调用栈（如果可用）
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // 获取崩溃日志目录
        let crash_dir = get_crash_dir();
        std::fs::create_dir_all(&crash_dir).ok();

        // 生成崩溃日志文件名（精确到秒）
        let now = chrono::Local::now();
        let filename = format!("crash_{}.log", now.format("%Y-%m-%d_%H-%M-%S"));
        let crash_file = crash_dir.join(&filename);

        // 构建崩溃报告
        let location = panic_info.location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };

        let report = format!(
            "=== AI-SubTrans 崩溃报告 ===\n时间: {}\n版本: {}\n\nPanic 位置: {}\nPanic 消息: {}\n\n--- 调用栈 ---\n{}\n",
            now.format("%Y-%m-%d %H:%M:%S"),
            env!("CARGO_PKG_VERSION"),
            location,
            msg,
            capture_backtrace(),
        );

        // 写入崩溃日志文件
        if let Err(e) = std::fs::write(&crash_file, &report) {
            eprintln!("[panic_hook] 写入崩溃日志失败: {}, 内容:\n{}", e, report);
        } else {
            eprintln!("[panic_hook] 崩溃日志已写入: {:?}", crash_file);
        }

        // 同时输出到 stderr（保留默认行为）
        default_hook(panic_info);
    }));
}

/// 获取崩溃日志目录
fn get_crash_dir() -> std::path::PathBuf {
    // 优先使用 app data dir，回退到当前目录
    if let Some(app_data) = get_app_data_dir() {
        app_data.join("crashes")
    } else {
        std::path::PathBuf::from("crashes")
    }
}

/// 获取 prompt 失败日志目录
pub fn get_prompt_fail_dir() -> std::path::PathBuf {
    if let Some(app_data) = get_app_data_dir() {
        app_data.join("prompt_fails")
    } else {
        std::path::PathBuf::from("prompt_fails")
    }
}

/// 全局开发者模式标志（由前端通过 IPC 设置）
static DEV_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 全量记录翻译数据开关（由前端通过 IPC 设置）
/// 仅在 devMode 开启且此开关开启时才记录 API 请求/响应
static LOG_API_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 设置开发者模式（前端 devModeStore 同步调用）
pub fn set_dev_mode(enabled: bool) {
    DEV_MODE.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// 查询开发者模式是否开启
pub fn is_dev_mode() -> bool {
    DEV_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 设置"全量记录翻译数据"开关
pub fn set_log_api_enabled(enabled: bool) {
    LOG_API_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// 查询是否应记录 API 调试日志：devMode 开启 **且** 全量记录开关开启
pub fn should_log_api() -> bool {
    is_dev_mode() && LOG_API_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// 获取 API 调试日志目录
pub fn get_api_debug_dir() -> std::path::PathBuf {
    if let Some(app_data) = get_app_data_dir() {
        app_data.join("api_debug")
    } else {
        std::path::PathBuf::from("api_debug")
    }
}

/// 记录 API 调试日志
/// 仅在 devMode 开启 **且** "全量记录翻译数据"开关开启时写入
pub fn log_api_debug(
    provider: &str,
    model: &str,
    source_lang: &str,
    target_lang: &str,
    request_body: &str,
    response_body: &str,
    status_code: u16,
) {
    if !should_log_api() {
        return;
    }
    let dir = get_api_debug_dir();
    std::fs::create_dir_all(&dir).ok();

    let now = chrono::Local::now();
    let filename = format!("api_{}.log", now.format("%Y-%m-%d_%H-%M-%S-%3f"));
    let file = dir.join(&filename);

    let report = format!(
        "=== API 调试日志 ===\n\
         时间: {}\n\
         Provider: {}\n\
         Model: {}\n\
         源语言: {}\n\
         目标语言: {}\n\
         HTTP 状态: {}\n\n\
         --- 请求体 ---\n{}\n\n\
         --- 响应体 ---\n{}\n",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        provider,
        model,
        source_lang,
        target_lang,
        status_code,
        request_body,
        response_body,
    );

    if let Err(e) = std::fs::write(&file, &report) {
        eprintln!("[log_api_debug] 写入失败: {}", e);
    }
}

/// 获取流式实时日志目录
fn get_stream_log_dir() -> std::path::PathBuf {
    if let Some(app_data) = get_app_data_dir() {
        app_data.join("api_debug")
    } else {
        std::path::PathBuf::from("api_debug")
    }
}

// task_local：存储当前并发槽位的流式日志文件句柄（保持打开，实时 flush）
// 在并发调度层设置，translate_single_batch_stream 中读取
tokio::task_local! {
    pub static STREAM_LOG_FILE: std::sync::Arc<std::sync::Mutex<std::fs::File>>;
}

/// 为一次翻译任务预创建 N 个复用的流式日志文件（N = 并发数）
/// 文件名格式：stream_<时间戳>_slot<序号>.log
/// 返回文件句柄列表（保持打开），供并发槽位复用
pub fn create_stream_log_slots(concurrency: usize) -> Vec<std::sync::Arc<std::sync::Mutex<std::fs::File>>> {
    let dir = get_stream_log_dir();
    std::fs::create_dir_all(&dir).ok();
    let now = chrono::Local::now();
    let ts = now.format("%Y-%m-%d_%H-%M-%S-%3f").to_string();
    (0..concurrency)
        .filter_map(|slot| {
            let filename = format!("stream_{}_slot{}.log", ts, slot);
            let path = dir.join(filename);
            // 创建/截断文件，保持打开
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .ok()
                .map(|f| std::sync::Arc::new(std::sync::Mutex::new(f)))
        })
        .collect()
}

/// 流式实时日志：追加写入到已打开的文件句柄，写入后立即 sync_all 确保实时性
/// 注意：std::fs::File 的 flush() 是空操作（no-op），必须用 sync_all() 才能强制刷盘
/// 仅在 devMode 开启且"全量记录翻译数据"开关开启时写入
pub fn log_stream_to_file(file: &std::sync::Mutex<std::fs::File>, text: &str) {
    if !should_log_api() {
        return;
    }
    if let Ok(mut f) = file.lock() {
        use std::io::Write;
        if let Err(e) = f.write_all(text.as_bytes()) {
            eprintln!("[log_stream_to_file] 写入失败: {}", e);
        }
        // flush() 对 File 是 no-op，必须用 sync_all() 强制 OS 刷盘
        // 否则数据留在 OS 缓存里，tail -f 看不到实时输出
        if let Err(e) = f.sync_all() {
            eprintln!("[log_stream_to_file] sync_all失败: {}", e);
        }
    }
}

/// 记录 prompt 失败日志（翻译对齐失败时调用）
/// 将 system prompt、user prompt、模型返回内容写入单独日志文件
pub fn log_prompt_fail(
    provider: &str,
    model: &str,
    source_lang: &str,
    target_lang: &str,
    system_prompt: &str,
    user_prompt: &str,
    model_response: &str,
    error: &str,
) {
    let dir = get_prompt_fail_dir();
    std::fs::create_dir_all(&dir).ok();

    let now = chrono::Local::now();
    let filename = format!("prompt_fail_{}.log", now.format("%Y-%m-%d_%H-%M-%S"));
    let file = dir.join(&filename);

    let report = format!(
        "=== Prompt 失败日志 ===\n\
         时间: {}\n\
         版本: {}\n\
         Provider: {}\n\
         Model: {}\n\
         源语言: {}\n\
         目标语言: {}\n\
         错误: {}\n\n\
         --- System Prompt ---\n{}\n\n\
         --- User Prompt ---\n{}\n\n\
         --- 模型返回内容 ---\n{}\n",
        now.format("%Y-%m-%d %H:%M:%S"),
        env!("CARGO_PKG_VERSION"),
        provider,
        model,
        source_lang,
        target_lang,
        error,
        system_prompt,
        user_prompt,
        model_response,
    );

    if let Err(e) = std::fs::write(&file, &report) {
        eprintln!("[log_prompt_fail] 写入失败: {}, 内容:\n{}", e, report);
    } else {
        eprintln!("[log_prompt_fail] 已写入: {:?}", file);
    }
}

/// 尝试获取 app data 目录（与 Tauri 的 app_data_dir 一致）
/// 跨平台：
/// - Windows: %APPDATA%\com.zimufan.ai-subtrans
/// - macOS:   ~/Library/Application Support/com.zimufan.ai-subtrans
/// - Linux:   $XDG_CONFIG_HOME/com.zimufan.ai-subtrans 或 ~/.config/com.zimufan.ai-subtrans
pub fn get_app_data_dir() -> Option<std::path::PathBuf> {
    // Windows: %APPDATA%
    #[cfg(windows)]
    {
        if let Ok(dir) = std::env::var("APPDATA") {
            return Some(std::path::PathBuf::from(dir).join("com.zimufan.ai-subtrans"));
        }
        return None;
    }
    // macOS: ~/Library/Application Support
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return Some(
                std::path::PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("com.zimufan.ai-subtrans"),
            );
        }
        None
    }
    // Linux/其他: XDG_CONFIG_HOME 或 ~/.config
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
            return Some(std::path::PathBuf::from(dir).join("com.zimufan.ai-subtrans"));
        }
        if let Ok(home) = std::env::var("HOME") {
            return Some(
                std::path::PathBuf::from(home)
                    .join(".config")
                    .join("com.zimufan.ai-subtrans"),
            );
        }
        None
    }
}

/// 捕获调用栈
fn capture_backtrace() -> String {
    // 使用 std::backtrace::Backtrace（Rust 1.65+ 稳定）
    let bt = std::backtrace::Backtrace::force_capture();
    format!("{:#}", bt)
}

/// Windows 原生异常代码转可读字符串
#[cfg(windows)]
fn exception_code_name(code: i32) -> &'static str {
    match code as u32 {
        0xC0000005 => "ACCESS_VIOLATION (内存访问违规)",
        0xC000001D => "ILLEGAL_INSTRUCTION (非法指令)",
        0xC0000025 => "NONCONTINUABLE_EXCEPTION (不可继续异常)",
        0xC0000094 => "INT_DIVIDE_BY_ZERO (整数除零)",
        0xC00000FD => "STACK_OVERFLOW (栈溢出)",
        0xC0000142 => "DLL_INIT_FAILED (DLL 初始化失败)",
        0xC0000409 => "STACK_BUFFER_OVERRUN (栈缓冲区溢出)",
        0xC000041D => "FATAL_USER_CALLBACK_EXCEPTION (用户回调致命异常)",
        0x40000015 => "FATAL_APP_EXIT (应用致命退出)",
        0xE06D7363 => "C++ EXCEPTION (C++ 异常)",
        0xCFFFFFFF => "UNKNOWN_NATIVE_CRASH (未知原生崩溃，可能是 WebView2 崩溃)",
        _ => "UNKNOWN (未知异常)",
    }
}

// Windows FFI 声明（避免依赖额外的 crate）
#[cfg(windows)]
#[repr(C)]
struct ExceptionRecord {
    exception_code: i32,
    _rest: [u8; 152 - 4], // 填充到 EXCEPTION_RECORD 的大小
}

#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct ExceptionPointers {
    ExceptionRecord: *mut ExceptionRecord,
    _ContextRecord: *mut std::ffi::c_void,
}

#[cfg(windows)]
type UnhandledExceptionFilter = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;

#[cfg(windows)]
extern "system" {
    fn SetUnhandledExceptionFilter(
        filter: Option<UnhandledExceptionFilter>,
    ) -> *mut std::ffi::c_void;

    fn AddVectoredExceptionHandler(
        first: u32,
        handler: Option<unsafe extern "system" fn(*mut ExceptionPointers) -> i32>,
    ) -> *mut std::ffi::c_void;
}

/// 安装 Windows 异常捕获
/// 同时安装 Vectored Exception Handler 和 Unhandled Exception Filter：
/// - VEH 在异常发生的第一时间触发，比 UEF 更可靠（不会被第三方库覆盖）
///   但 VEH 会捕获所有异常（包括 WebView2/libmpv 内部正常使用的 C++ 异常），
///   所以只处理致命异常码，避免对正常控制流异常写日志导致无限循环
/// - UEF 作为后备，在异常未被处理时触发
#[cfg(windows)]
fn install_exception_filter() {
    unsafe {
        // Vectored Exception Handler：优先级最高，在异常分发链最前端触发
        let _ = AddVectoredExceptionHandler(1, Some(vectored_exception_handler));
        // Unhandled Exception Filter：作为后备
        SetUnhandledExceptionFilter(Some(unhandled_exception_handler));
    }
}

/// 全局标志：确保崩溃日志只写一次
/// VEH 可能对同一个致命异常被多次调用（异常重新抛出时），用此标志防止无限写日志
#[cfg(windows)]
static CRASH_REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 判断异常码是否为致命异常（需要写崩溃日志的）
/// 排除 C++ 异常 (0xE06D7363)、RPC 异常 (0x6BA) 等正常控制流异常
#[cfg(windows)]
fn is_fatal_exception(code: i32) -> bool {
    matches!(code as u32,
        0xC0000005 | // ACCESS_VIOLATION
        0xC000001D | // ILLEGAL_INSTRUCTION
        0xC0000025 | // NONCONTINUABLE_EXCEPTION
        0xC0000094 | // INT_DIVIDE_BY_ZERO
        0xC00000FD | // STACK_OVERFLOW
        0xC0000142 | // DLL_INIT_FAILED
        0xC0000409 | // STACK_BUFFER_OVERRUN
        0xC000041D | // FATAL_USER_CALLBACK_EXCEPTION
        0x40000015 | // FATAL_APP_EXIT
        0xCFFFFFFF   // UNKNOWN_NATIVE_CRASH
    )
}

/// 写入崩溃日志和 minidump（VEH 和 UEF 共用）
/// 用 CRASH_REPORTED 标志确保只写一次，防止 VEH/UEF 重复触发导致无限循环
#[cfg(windows)]
unsafe fn write_crash_report(exception_info: *mut ExceptionPointers, source: &str) {
    // 原子操作：如果已经写过了，直接返回，防止无限循环
    if CRASH_REPORTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    let code = if exception_info.is_null() {
        -1i32
    } else {
        (*exception_info).ExceptionRecord.read().exception_code
    };

    let crash_dir = get_crash_dir();
    std::fs::create_dir_all(&crash_dir).ok();

    let now = chrono::Local::now();
    let ts = now.format("%Y-%m-%d_%H-%M-%S");
    let filename = format!("crash_{}.log", ts);
    let crash_file = crash_dir.join(&filename);

    let report = format!(
        "=== AI-SubTrans 崩溃报告（原生异常） ===\n\
         时间: {}\n\
         版本: {}\n\
         来源: {}\n\
         异常代码: 0x{:08X}\n\
         异常类型: {}\n\n\
         --- 调用栈 ---\n{}\n",
        now.format("%Y-%m-%d %H:%M:%S"),
        env!("CARGO_PKG_VERSION"),
        source,
        code as u32,
        exception_code_name(code),
        capture_backtrace(),
    );

    if let Err(e) = std::fs::write(&crash_file, &report) {
        eprintln!("[exception_handler] 写入崩溃日志失败: {}, 内容:\n{}", e, report);
    } else {
        eprintln!("[exception_handler] 崩溃日志已写入: {:?}", crash_file);
    }

    // 写 minidump（供事后用 WinDbg/VS 分析调用栈）
    let dump_file = crash_dir.join(format!("crash_{}.dmp", ts));
    write_minidump(exception_info, &dump_file);
}

/// 写 minidump 文件
#[cfg(windows)]
unsafe fn write_minidump(exception_info: *mut ExceptionPointers, path: &std::path::Path) {
    use windows::Win32::Storage::FileSystem::{CreateFileW, CREATE_ALWAYS, FILE_SHARE_READ, FILE_ATTRIBUTE_NORMAL};
    use windows::Win32::System::Diagnostics::Debug::{
        MiniDumpWriteDump, MiniDumpNormal, MINIDUMP_EXCEPTION_INFORMATION,
    };
    use windows::Win32::Foundation::{GENERIC_WRITE, CloseHandle, FALSE};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();
    let handle = CreateFileW(
        windows::core::PCWSTR(wide.as_ptr()),
        GENERIC_WRITE.0,
        FILE_SHARE_READ,
        None,
        CREATE_ALWAYS,
        FILE_ATTRIBUTE_NORMAL,
        None,
    );
    match handle {
        Ok(h) => {
            let mut mei = MINIDUMP_EXCEPTION_INFORMATION {
                ThreadId: windows::Win32::System::Threading::GetCurrentThreadId(),
                ExceptionPointers: exception_info as *const _ as *mut _,
                ClientPointers: FALSE,
            };
            let process = windows::Win32::System::Threading::GetCurrentProcess();
            let pid = windows::Win32::System::Threading::GetCurrentProcessId();
            let ret = MiniDumpWriteDump(
                process,
                pid,
                h,
                MiniDumpNormal,
                Some(&mut mei),
                None,
                None,
            );
            if ret.is_ok() {
                eprintln!("[exception_handler] minidump 已写入: {:?}", path);
            } else {
                eprintln!("[exception_handler] minidump 写入失败: {:?}", ret);
            }
            let _ = CloseHandle(h);
        }
        Err(e) => {
            eprintln!("[exception_handler] 创建 minidump 文件失败: {:?}", e);
        }
    }
}

/// Vectored Exception Handler
/// 在异常发生的第一时间触发，比 UEF 更可靠
/// **重要**：VEH 会捕获所有异常（包括 WebView2/libmpv 内部正常使用的 C++ 异常），
/// 所以只对致命异常码写崩溃日志，其他异常直接放行，避免无限循环写日志
/// 返回 EXCEPTION_CONTINUE_SEARCH (0) 让异常继续传递给应用自身的 handler/UEF
#[cfg(windows)]
unsafe extern "system" fn vectored_exception_handler(
    exception_info: *mut ExceptionPointers,
) -> i32 {
    let code = if exception_info.is_null() {
        -1i32
    } else {
        (*exception_info).ExceptionRecord.read().exception_code
    };
    // 只对致命异常写崩溃日志，非致命异常（C++ 异常、RPC 异常等）直接放行
    if is_fatal_exception(code) {
        write_crash_report(exception_info, "VectoredExceptionHandler");
    }
    // 始终放行：让应用自身的异常处理逻辑正常工作，UEF 作为最后兜底
    0 // EXCEPTION_CONTINUE_SEARCH
}

/// Windows 未处理异常回调（后备）
#[cfg(windows)]
unsafe extern "system" fn unhandled_exception_handler(
    exception_info: *mut ExceptionPointers,
) -> i32 {
    write_crash_report(exception_info, "UnhandledExceptionFilter");
    // 返回 EXCEPTION_EXECUTE_HANDLER 让进程终止
    1
}

#[cfg(not(windows))]
fn install_exception_filter() {
    // 非 Windows 平台无操作
}

/// Tauri 应用入口
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 安装 panic hook：捕获 Rust panic，写入崩溃日志文件
    install_panic_hook();
    // 安装 Windows 异常过滤器：捕获原生异常（如内存访问违规、栈溢出等），
    // 这些不会触发 panic hook
    install_exception_filter();

    // 在主线程初始化 OLE，永不卸载。
    // mpv 内部可能调用 OleUninitialize/CoUninitialize，如果 OLE 引用计数降到 0，
    // Tauri 主窗口的拖放系统会失效。这里额外持有一个引用，确保 OLE 永不卸载。
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::System::Ole::OleInitialize(None);
        tracing::info!("OLE 已在应用启动时初始化（永久引用）");
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(drop) = event {
                use tauri::Emitter;
                match drop {
                    tauri::DragDropEvent::Enter { paths, .. } => {
                        tracing::info!("DragDropEvent::Enter, paths={:?}", paths);
                        let _ = window.emit("tauri://file-drop-hover", paths);
                    }
                    tauri::DragDropEvent::Drop { paths, .. } => {
                        tracing::info!("DragDropEvent::Drop, paths={:?}", paths);
                        let paths: Vec<String> = paths.iter()
                            .filter_map(|p| p.to_str().map(|s| s.to_string()))
                            .collect();
                        let _ = window.emit("app://file-drop", paths);
                    }
                    tauri::DragDropEvent::Leave => {
                        tracing::info!("DragDropEvent::Leave");
                        let _ = window.emit("tauri://file-drop-cancelled", ());
                    }
                    _ => {
                        tracing::info!("DragDropEvent::other: {:?}", drop);
                    }
                }
            }
        })
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            std::fs::create_dir_all(&app_data_dir).ok();

            let _log_guard = init_logging(&app_data_dir);
            app.manage(_log_guard);

            // 初始化数据库
            let db_path = app_data_dir.join("zimufan.db");
            let db = db::Database::open(&db_path)?;
            db.migrate()?;
            // 启动时清理"假翻译"缓存（译文=原文，AI 未实际翻译）
            match db.purge_fake_translate_cache() {
                Ok(n) if n > 0 => tracing::info!("启动清理：删除 {} 条假翻译缓存（译文=原文）", n),
                Ok(_) => {}
                Err(e) => tracing::warn!("启动清理假翻译缓存失败: {:?}", e),
            }

            // 在 manage(db) 之前读取批量配置（manage 会 move db）
            let saved_batch_config = db.get_config("batch_config").ok().flatten()
                .and_then(|json| serde_json::from_str::<batch::BatchConfig>(&json).ok())
                .unwrap_or_default();

            app.manage(db);

            // 初始化翻译取消令牌
            app.manage(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)) as crate::ipc::CancelToken);

            // 初始化 ffmpeg 的 app_data_dir（供 find_ffmpeg 查找下载的 ffmpeg）
            crate::ffmpeg::init_app_data_dir(app_data_dir.clone());

            // 初始化批量翻译队列
            let (tx, rx) = tokio::sync::mpsc::channel::<batch::BatchCmd>(256);
            let batch_tasks = std::sync::Arc::new(std::sync::Mutex::new(Vec::<batch::BatchTask>::new()));
            // 从 DB 读取已保存的批量配置，fallback 到默认（已在 manage 前读取）
            let batch_config = std::sync::Arc::new(std::sync::Mutex::new(saved_batch_config));
            let batch_watcher = std::sync::Arc::new(std::sync::Mutex::new(None::<batch::FolderWatcher>));
            let batch_paused = std::sync::Arc::new(std::sync::Mutex::new(false));
            let batch_scan_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

            // V2 持久化：从 DB 恢复未完成的批量任务
            // 处理中的任务标记为 Failed（重启后无法恢复处理上下文）
            {
                let db_state = app.state::<Database>();
                match db_state.load_batch_tasks() {
                    Ok(saved_tasks) => {
                        let mut tasks = batch_tasks.lock().unwrap();
                        for mut task in saved_tasks {
                            // 处理中的任务标记为 Failed（重启后无法恢复）
                            let is_processing = matches!(
                                task.status,
                                batch::BatchStatus::Probing
                                    | batch::BatchStatus::CheckingSubtitle
                                    | batch::BatchStatus::Extracting(_)
                                    | batch::BatchStatus::Parsing
                                    | batch::BatchStatus::Translating(_)
                                    | batch::BatchStatus::Exporting
                            );
                            if is_processing {
                                task.status = batch::BatchStatus::Failed(
                                    "应用重启，任务中断".to_string()
                                );
                                task.error = Some("应用重启，任务中断".to_string());
                                task.finished_at = Some(batch::now_ts());
                                let _ = db_state.upsert_batch_task(&task);
                            }
                            // 已完成/已失败/已跳过的任务保留，Queued 的任务保留等待重新调度
                            tasks.push(task);
                        }
                        tracing::info!("批量翻译：从 DB 恢复 {} 个任务", tasks.len());
                    }
                    Err(e) => {
                        tracing::warn!("批量翻译：从 DB 恢复任务失败: {}", e);
                    }
                }
            }

            let batch_queue = batch::BatchQueue {
                tx,
                tasks: batch_tasks.clone(),
                config: batch_config.clone(),
                watcher: batch_watcher.clone(),
                paused: batch_paused.clone(),
                scan_cancel: batch_scan_cancel.clone(),
            };
            app.manage(batch_queue);

            // 启动 BatchWorker 后台 task
            let app_handle_for_batch = app.handle().clone();
            // 在 move 之前先读取配置判断是否需要自动启动监视
            let need_auto_watch = !batch_config.lock().unwrap().watch_paths.is_empty();
            batch::spawn_batch_worker(
                app_handle_for_batch,
                rx,
                batch_tasks,
                batch_config,
                batch_paused,
                batch_scan_cancel,
            );

            // 启动时清理 batch_tmp/ 残留临时文件
            batch::cleanup_batch_tmp();

            // 若配置了 watch_paths，延迟 2 秒自动启动监视
            if need_auto_watch {
                let app_handle_for_watch = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if let Err(e) = batch::auto_start_watch(&app_handle_for_watch).await {
                        tracing::warn!("批量翻译：自动恢复文件夹监视失败: {}", e);
                    }
                });
            }

            tracing::info!("AI-SubTrans 启动完成，数据目录: {:?}", app_data_dir);

            // 窗口初始位置：根据鼠标所在显示器居中计算（先定位不显示）
            let _initial_position: Option<(i32, i32, i32, i32)> = {
                #[cfg(windows)]
                {
                    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
                    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST};
                    use windows::Win32::Foundation::POINT;

                    unsafe {
                        let mut cursor = POINT { x: 0, y: 0 };
                        if GetCursorPos(&mut cursor).is_ok() {
                            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
                            let mut mi = MONITORINFO {
                                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                                ..Default::default()
                            };
                            if GetMonitorInfoW(monitor, &mut mi).as_bool() {
                                let mon_left = mi.rcMonitor.left;
                                let mon_top = mi.rcMonitor.top;
                                let mon_w = mi.rcMonitor.right - mi.rcMonitor.left;
                                let mon_h = mi.rcMonitor.bottom - mi.rcMonitor.top;
                                Some((mon_left, mon_top, mon_w, mon_h))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    None::<(i32, i32, i32, i32)>
                }
            };

            if let Some(window) = app.get_webview_window("main") {
                #[cfg(windows)]
                {
                    if let Some((mon_left, mon_top, mon_w, mon_h)) = initial_position {
                        let scale = window.scale_factor().unwrap_or(1.0);
                        let win_w = 520.0 * scale;
                        let win_h = 325.0 * scale;
                        let x = mon_left + ((mon_w as f64 - win_w) / 2.0).round() as i32;
                        let y = mon_top + ((mon_h as f64 - win_h) / 2.0).round() as i32;
                        let _ = window.set_position(tauri::PhysicalPosition {
                            x: x.max(0),
                            y: y.max(0),
                        });
                    }
                }
                #[cfg(not(windows))]
                {
                    // macOS/Linux：用 Tauri 跨平台 API 居中到主显示器
                    if let Ok(monitors) = window.available_monitors() {
                        if let Some(monitor) = monitors.first() {
                            let pos = monitor.position();
                            let size = monitor.size();
                            let scale = window.scale_factor().unwrap_or(1.0);
                            let win_w = 520.0 * scale;
                            let win_h = 325.0 * scale;
                            let x = pos.x as f64 + ((size.width as f64 - win_w) / 2.0).round();
                            let y = pos.y as f64 + ((size.height as f64 - win_h) / 2.0).round();
                            let _ = window.set_position(tauri::PhysicalPosition {
                                x: x.max(0.0) as i32,
                                y: y.max(0.0) as i32,
                            });
                        }
                    }
                }
                // 启动时先显示窗口（不置顶），让用户看到白框加载状态
                let _ = window.show();
                // 前端页面加载完成后再置顶，避免加载完成前抢占其他窗口焦点
                let app_handle = app.handle().clone();
                let app_handle_for_closure = app_handle.clone();
                let _ = app_handle.listen("app://ready", move |_event| {
                    if let Some(window) = app_handle_for_closure.get_webview_window("main") {
                        let _ = window.set_focus();
                        tracing::info!("前端页面加载完成，主窗口已置顶");
                    }
                });
            }

            // 解析命令行参数
            let cli_args = parse_cli_args();
            if let Some(mode) = &cli_args.mode {
                tracing::info!("静默模式: {}, 文件: {:?}", mode, cli_args.file_path);
                // 静默模式通过事件通知前端
                if let Some(file_path) = &cli_args.file_path {
                    use tauri::Emitter;
                    app.emit("cli-args", serde_json::json!({
                        "mode": mode,
                        "filePath": file_path,
                    })).ok();
                }
            }

            Ok(())
        })
        .invoke_handler(ipc::get_invoke_handlers());

    // 单实例插件（桌面端）
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        tracing::info!("单实例转发: argv={:?}", argv);
        // 解析第二个实例的 argv，转发文件路径到主窗口
        let mut mode: Option<String> = None;
        let mut file_path: Option<String> = None;
        for arg in &argv {
            if let Some(m) = arg.strip_prefix("--mode=") {
                mode = Some(m.to_string());
            } else if !arg.starts_with("--") && !arg.ends_with(".exe") && std::path::Path::new(arg).exists() {
                file_path = Some(arg.to_string());
            }
        }
        if mode.is_some() || file_path.is_some() {
            use tauri::Emitter;
            app.emit("cli-args", serde_json::json!({
                "mode": mode,
                "filePath": file_path,
            })).ok();
        }
        // 将窗口置前
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// === 跨平台单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;

    /// get_app_data_dir 应返回含 bundle id 的路径（跨平台）
    /// 验证不再只依赖 Windows 的 APPDATA
    #[test]
    fn test_get_app_data_dir_contains_bundle_id() {
        let dir = get_app_data_dir();
        // 各平台都应能获取到 app data 目录（CI 环境可能有 HOME 缺失，但开发机不会）
        if let Some(d) = dir {
            let s = d.to_string_lossy();
            assert!(
                s.contains("com.zimufan.ai-subtrans"),
                "app data 目录应含 bundle id，实际: {}",
                s
            );
        }
        // None 时说明 HOME/APPDATA 都缺失（罕见），不视为失败
    }

    /// get_crash_dir 应返回 crashes 子目录
    #[test]
    fn test_get_crash_dir_ends_with_crashes() {
        let dir = get_crash_dir();
        let s = dir.to_string_lossy();
        assert!(
            s.ends_with("crashes") || s.ends_with("crashes/"),
            "crash 目录应以 crashes 结尾，实际: {}",
            s
        );
    }

    /// get_prompt_fail_dir 应返回 prompt_fails 子目录
    #[test]
    fn test_get_prompt_fail_dir_ends_with_prompt_fails() {
        let dir = get_prompt_fail_dir();
        let s = dir.to_string_lossy();
        assert!(
            s.ends_with("prompt_fails") || s.ends_with("prompt_fails/"),
            "prompt_fail 目录应以 prompt_fails 结尾，实际: {}",
            s
        );
    }
}
