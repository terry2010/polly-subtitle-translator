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
                        if age > cutoff {
                            if std::fs::remove_file(&path).is_ok() {
                                removed += 1;
                            }
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
            app.manage(db);

            // 初始化翻译取消令牌
            app.manage(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)) as crate::ipc::CancelToken);

            // 初始化 ffmpeg 的 app_data_dir（供 find_ffmpeg 查找下载的 ffmpeg）
            crate::ffmpeg::init_app_data_dir(app_data_dir.clone());

            tracing::info!("AI-SubTrans 启动完成，数据目录: {:?}", app_data_dir);

            // 显式显示窗口并获取焦点，居中到鼠标所在显示器
            if let Some(window) = app.get_webview_window("main") {
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
                    }
                }
                let _ = window.show();
                let _ = window.set_focus();
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
