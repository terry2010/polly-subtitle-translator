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

use tauri::Manager;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

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

/// Tauri 应用入口
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
            app.manage(db);

            // 初始化翻译取消令牌
            app.manage(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)) as crate::ipc::CancelToken);

            // 初始化 ffmpeg 的 app_data_dir（供 find_ffmpeg 查找下载的 ffmpeg）
            crate::ffmpeg::init_app_data_dir(app_data_dir.clone());

            tracing::info!("AI-SubTrans 启动完成，数据目录: {:?}", app_data_dir);

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
