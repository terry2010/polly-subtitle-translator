// IPC 命令注册与 handler
// 对应需求文档 §7 IPC 命令清单

use crate::config;
use crate::db::{Database, HistoryRecord, RecentFile};
use crate::error::{ipc_result, AppError, IpcError, IpcResult};
use crate::ffmpeg;
use crate::subtitle;
use crate::translate::{self, TranslateProvider, ProviderCredentials};
use tauri::{Emitter, Manager, State};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

/// 全局翻译取消令牌
pub type CancelToken = Arc<AtomicBool>;

/// 将 AppError 转为 IpcError（用于 async 命令返回 Result<T, IpcError>）
fn to_ipc_err(e: AppError) -> IpcError {
    e.to_ipc_error()
}

/// 获取所有 IPC 命令 handler
pub fn get_invoke_handlers() -> Box<dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync> {
    Box::new(tauri::generate_handler![
        probe_video,
        extract_subtitle,
        parse_subtitle_file,
        detect_bilingual,
        split_bilingual_subtitle,
        save_subtitle_file_cmd,
        get_recent_files,
        add_recent_file,
        get_history,
        add_history_record,
        get_config,
        set_config,
        get_all_config,
        clear_translate_cache,
        get_supported_target_langs,
        translate_subtitle,
        cancel_translate,
        get_cached_translations,
        test_translate_connection,
        save_credential,
        get_credential,
        delete_credential,
        merge_subtitle,
        search_subtitles_online,
        download_subtitle_online,
        register_video_menu,
        unregister_video_menu,
        register_subtitle_menu,
        unregister_subtitle_menu,
        is_video_menu_registered,
        is_subtitle_menu_registered,
        get_libmpv_status_cmd,
        download_libmpv_cmd,
        open_in_system_player_cmd,
        player_init,
        player_load_cmd,
        player_play_cmd,
        player_pause_cmd,
        player_seek_cmd,
        player_set_volume_cmd,
        player_set_speed_cmd,
        player_get_position_cmd,
        player_resize_cmd,
        player_show_cmd,
        player_hide_cmd,
        player_destroy_cmd,
    ])
}

// === SECTION 1 END ===

/// probe_video：探测视频文件信息
#[tauri::command]
pub async fn probe_video(
    video_path: String,
    ffmpeg_path: Option<String>,
    db: State<'_, Database>,
) -> Result<IpcResult<ffmpeg::ProbeResult>, ()> {
    let vpath = video_path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        ffmpeg::probe_video(&video_path, ffmpeg_path.as_deref())
    }).await;
    match result {
        Ok(Ok(probe)) => {
            let _ = db.add_recent_file(&vpath, "video");
            let _ = db.add_history(&HistoryRecord {
                video_path: Some(vpath.clone()),
                subtitle_path: None,
                source_lang: None,
                target_lang: None,
                provider: None,
                action: "probe".to_string(),
                status: "success".to_string(),
                detail: Some(format!(
                    "streams: {} video, {} audio, {} subtitle",
                    probe.video_stream.is_some() as usize,
                    probe.audio_streams.len(),
                    probe.subtitle_streams.len()
                )),
            });
            Ok(IpcResult::from(Ok(probe)))
        }
        Ok(Err(e)) => Ok(IpcResult::from(Err(e))),
        Err(e) => Ok(IpcResult::from(Err(AppError::FfmpegExecutionFailed { detail: format!("探测任务失败: {}", e) }))),
    }
}

/// extract_subtitle：提取字幕流
#[tauri::command]
pub async fn extract_subtitle(
    video_path: String,
    stream_index: i32,
    output_path: String,
    ffmpeg_path: Option<String>,
    db: State<'_, Database>,
) -> Result<IpcResult<()>, ()> {
    let output_path_clone = output_path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        ffmpeg::extract_subtitle_stream(&video_path, stream_index, &output_path, ffmpeg_path.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => {
            let _ = db.add_recent_file(&output_path_clone, "subtitle");
            let _ = db.add_history(&HistoryRecord {
                video_path: None,
                subtitle_path: Some(output_path_clone.clone()),
                source_lang: None,
                target_lang: None,
                provider: None,
                action: "extract".to_string(),
                status: "success".to_string(),
                detail: Some(format!("stream_index: {}", stream_index)),
            });
            Ok(IpcResult::from(Ok(())))
        }
        Ok(Err(e)) => Ok(IpcResult::from(Err(e))),
        Err(e) => Ok(IpcResult::from(Err(AppError::FfmpegExecutionFailed { detail: format!("提取任务失败: {}", e) }))),
    }
}

/// parse_subtitle_file：解析字幕文件
#[tauri::command]
pub fn parse_subtitle_file(file_path: String) -> IpcResult<subtitle::SubtitleFile> {
    ipc_result(subtitle::load_subtitle_file(&file_path))
}

/// detect_bilingual：检测字幕是否为双语
#[tauri::command]
pub fn detect_bilingual(file: subtitle::SubtitleFile) -> IpcResult<subtitle::BilingualDetectResult> {
    ipc_result(Ok(subtitle::detect_bilingual(&file)))
}

/// split_bilingual_subtitle：拆分双语字幕，将译文行填入 translated 字段
#[tauri::command]
pub fn split_bilingual_subtitle(
    mut file: subtitle::SubtitleFile,
    split_mode: subtitle::SplitMode,
) -> IpcResult<subtitle::SubtitleFile> {
    subtitle::split_bilingual(&mut file, split_mode);
    ipc_result(Ok(file))
}

/// save_subtitle_file：保存字幕文件
#[tauri::command]
pub fn save_subtitle_file_cmd(
    file: subtitle::SubtitleFile,
    output_path: String,
) -> IpcResult<()> {
    ipc_result(subtitle::save_subtitle_file(&file, &output_path))
}

/// get_recent_files：获取最近文件列表
#[tauri::command]
pub fn get_recent_files(
    file_type: Option<String>,
    db: State<'_, Database>,
) -> IpcResult<Vec<RecentFile>> {
    ipc_result(db.get_recent_files(file_type.as_deref()))
}

/// add_recent_file：添加最近文件
#[tauri::command]
pub fn add_recent_file(
    file_path: String,
    file_type: String,
    db: State<'_, Database>,
) -> IpcResult<()> {
    ipc_result(db.add_recent_file(&file_path, &file_type))
}

/// get_history：获取历史记录
#[tauri::command]
pub fn get_history(
    limit: Option<usize>,
    db: State<'_, Database>,
) -> IpcResult<Vec<HistoryRecord>> {
    ipc_result(db.with_conn(|conn| {
        let limit = limit.unwrap_or(100);
        let mut stmt = conn.prepare(
            "SELECT video_path, subtitle_path, source_lang, target_lang, provider, action, status, detail
             FROM history ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(HistoryRecord {
                video_path: row.get(0)?,
                subtitle_path: row.get(1)?,
                source_lang: row.get(2)?,
                target_lang: row.get(3)?,
                provider: row.get(4)?,
                action: row.get(5)?,
                status: row.get(6)?,
                detail: row.get(7)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }))
}

/// add_history_record：添加历史记录
#[tauri::command]
pub fn add_history_record(
    record: HistoryRecord,
    db: State<'_, Database>,
) -> IpcResult<i64> {
    ipc_result(db.add_history(&record))
}

/// get_config：读取配置项
#[tauri::command]
pub fn get_config(key: String, db: State<'_, Database>) -> IpcResult<Option<String>> {
    ipc_result(db.get_config(&key))
}

/// set_config：写入配置项
#[tauri::command]
pub fn set_config(key: String, value: String, db: State<'_, Database>) -> IpcResult<()> {
    ipc_result(db.set_config(&key, &value))
}

/// get_all_config：读取所有配置项
#[tauri::command]
pub fn get_all_config(db: State<'_, Database>) -> IpcResult<Vec<(String, String)>> {
    ipc_result(db.get_all_config())
}

/// clear_translate_cache：清除翻译缓存
#[tauri::command]
pub fn clear_translate_cache(db: State<'_, Database>) -> IpcResult<usize> {
    ipc_result(db.clear_translate_cache())
}

/// get_supported_target_langs：获取支持的目标语言列表
#[tauri::command]
pub async fn get_supported_target_langs(
    _provider: String,
) -> IpcResult<Vec<crate::translate::LanguageInfo>> {
    ipc_result((|| {
        // 返回静态语言列表（实际应从 provider 获取，但需要凭据）
        Ok(vec![
            crate::translate::LanguageInfo {
                code: "zh".into(),
                name: "Chinese".into(),
                native_name: "中文".into(),
            },
            crate::translate::LanguageInfo {
                code: "en".into(),
                name: "English".into(),
                native_name: "English".into(),
            },
            crate::translate::LanguageInfo {
                code: "ja".into(),
                name: "Japanese".into(),
                native_name: "日本語".into(),
            },
            crate::translate::LanguageInfo {
                code: "ko".into(),
                name: "Korean".into(),
                native_name: "한국어".into(),
            },
            crate::translate::LanguageInfo {
                code: "fr".into(),
                name: "French".into(),
                native_name: "Français".into(),
            },
            crate::translate::LanguageInfo {
                code: "de".into(),
                name: "German".into(),
                native_name: "Deutsch".into(),
            },
            crate::translate::LanguageInfo {
                code: "es".into(),
                name: "Spanish".into(),
                native_name: "Español".into(),
            },
            crate::translate::LanguageInfo {
                code: "ru".into(),
                name: "Russian".into(),
                native_name: "Русский".into(),
            },
        ])
    })())
}

// === SECTION 2 END ===

/// translate_subtitle：翻译字幕条目
#[tauri::command]
pub async fn translate_subtitle(
    entries: Vec<subtitle::SubtitleEntry>,
    source_lang: String,
    target_lang: String,
    provider: String,
    db: State<'_, Database>,
    cancel_token: State<'_, CancelToken>,
    app: tauri::AppHandle,
) -> Result<translate::TranslateResult, IpcError> {
    tracing::info!("translate_subtitle 调用: provider={}, entries={}, source={}, target={}", provider, entries.len(), source_lang, target_lang);

    // 重置取消标志
    cancel_token.store(false, Ordering::Relaxed);

    let prov = TranslateProvider::from_str(&provider).ok_or_else(|| {
        AppError::Unknown {
            detail: format!("未知翻译引擎: {}", provider),
        }
    }).map_err(to_ipc_err)?;

    // 从 config 表读取凭据配置
    let config_key = format!("translate_{}_app_id", provider);
    let app_id = db.get_config(&config_key).map_err(to_ipc_err)?;
    tracing::info!("translate_subtitle app_id from config: {:?}", app_id);
    let region_ref = format!("translate_{}_region", provider);
    let region = db.get_config(&region_ref).map_err(to_ipc_err)?;

    // 尝试从 keyring 读取密钥，如果失败则从 config 表读取
    let secret = match config::CredentialStore::load(&provider, "secret") {
        Ok(s) => {
            tracing::info!("translate_subtitle secret from keyring: 已获取");
            Some(s)
        }
        Err(AppError::StorageCredentialNotFound { .. }) => {
            tracing::warn!("translate_subtitle keyring: 凭据未找到, 尝试 config 表");
            let secret_key_ref = format!("translate_{}_secret", provider);
            db.get_config(&secret_key_ref).map_err(to_ipc_err)?
        }
        Err(e) => {
            tracing::warn!("translate_subtitle keyring 读取失败: {}, 尝试 config 表", e);
            let secret_key_ref = format!("translate_{}_secret", provider);
            db.get_config(&secret_key_ref).map_err(to_ipc_err)?
        }
    };
    tracing::info!("translate_subtitle secret: {:?}", if secret.is_some() { "Some" } else { "None" });

    // 验证凭据存在
    if app_id.is_none() && secret.is_none() {
        return Err(IpcError::new(
            "translate.authFailed",
            "error.translate.authFailed",
            "未配置翻译 API 凭据，请先在设置中配置",
            crate::error::Severity::Recoverable,
        ));
    }

    let credentials = ProviderCredentials {
        app_id,
        secret_key: secret,
        region,
    };

    let prov_instance = translate::create_provider(&prov, &credentials).map_err(to_ipc_err)?;
    let scheduler = translate::TranslateScheduler::with_cancel_token(
        &db,
        prov_instance,
        provider.clone(),
        cancel_token.inner().clone(),
    );

    // 进度回调：通过 Tauri 事件推送进度
    let app_handle = app.clone();
    let progress_cb = Box::new(move |progress: usize, total: usize| {
        let _ = app_handle.emit("translate-progress", serde_json::json!({
            "progress": progress,
            "total": total,
            "done": false,
        }));
    });

    // 单条翻译完成回调：通过 Tauri 事件推送单条结果
    let app_handle2 = app.clone();
    let entry_cb = Box::new(move |entry: &translate::TranslateEntry| {
        let _ = app_handle2.emit("translate-entry-done", serde_json::json!({
            "index": entry.index,
            "original": entry.original,
            "translated": entry.translated,
            "from_cache": entry.from_cache,
        }));
    });

    let result = scheduler
        .translate_entries_full(&entries, &source_lang, &target_lang, 5000, Some(progress_cb), Some(entry_cb))
        .await
        .map_err(to_ipc_err)?;

    // 发送翻译完成事件
    let _ = app.emit("translate-progress", serde_json::json!({
        "progress": result.translations.len(),
        "total": entries.len(),
        "done": true,
    }));

    let _ = db.add_history(&HistoryRecord {
        video_path: None,
        subtitle_path: None,
        source_lang: Some(source_lang),
        target_lang: Some(target_lang),
        provider: Some(provider),
        action: "translate".to_string(),
        status: "success".to_string(),
        detail: Some(format!(
            "total: {}, cached: {}",
            result.translations.len(),
            result.cached_count
        )),
    });

    Ok(result)
}

/// cancel_translate：取消正在进行的翻译
#[tauri::command]
pub fn cancel_translate(cancel_token: State<'_, CancelToken>) -> IpcResult<()> {
    cancel_token.store(true, Ordering::Relaxed);
    tracing::info!("收到取消翻译请求");
    ipc_result(Ok(()))
}

/// get_cached_translations：查询已缓存的翻译结果（不调用 API）
#[tauri::command]
pub async fn get_cached_translations(
    entries: Vec<subtitle::SubtitleEntry>,
    source_lang: String,
    target_lang: String,
    provider: String,
    db: State<'_, Database>,
) -> Result<Vec<translate::TranslateEntry>, IpcError> {
    let prov = TranslateProvider::from_str(&provider).ok_or_else(|| {
        crate::error::IpcError {
            code: "invalid_provider".to_string(),
            i18n_key: "error.invalidProvider".to_string(),
            args: None,
            message: format!("不支持的翻译引擎: {}", provider),
            severity: crate::error::Severity::Recoverable,
        }
    })?;

    // 获取凭据（缓存查询不需要凭据，但需要 provider_name）
    let scheduler = translate::TranslateScheduler::new(
        &db,
        Box::new(translate::BaiduProvider::new(String::new(), String::new())) as Box<dyn translate::TranslateProviderTrait>,
        prov.as_str().to_string(),
    );

    let cached = scheduler
        .get_cached_entries(&entries, &source_lang, &target_lang)
        .map_err(to_ipc_err)?;

    tracing::info!("缓存查询: {} 条中命中 {} 条", entries.len(), cached.len());
    Ok(cached)
}

/// test_translate_connection：测试翻译 API 连接
#[tauri::command]
pub async fn test_translate_connection(
    provider: String,
    app_id: Option<String>,
    secret_key: Option<String>,
    region: Option<String>,
) -> Result<(), IpcError> {
    let prov = TranslateProvider::from_str(&provider).ok_or_else(|| {
        AppError::Unknown {
            detail: format!("未知翻译引擎: {}", provider),
        }
    }).map_err(to_ipc_err)?;

    let credentials = ProviderCredentials {
        app_id,
        secret_key,
        region,
    };

    let prov_instance = translate::create_provider(&prov, &credentials).map_err(to_ipc_err)?;
    prov_instance.test_connection().await.map_err(to_ipc_err)
}

/// save_credential：保存凭据到 keyring
#[tauri::command]
pub fn save_credential(
    provider: String,
    key: String,
    value: String,
) -> IpcResult<()> {
    ipc_result(config::CredentialStore::save(&provider, &key, &value))
}

/// get_credential：从 keyring 读取凭据
#[tauri::command]
pub fn get_credential(provider: String, key: String) -> IpcResult<Option<String>> {
    let result = match config::CredentialStore::load(&provider, &key) {
        Ok(v) => Some(v),
        Err(AppError::StorageCredentialNotFound { .. }) => None,
        Err(e) => return ipc_result(Err(e)),
    };
    ipc_result(Ok(result))
}

/// delete_credential：从 keyring 删除凭据
#[tauri::command]
pub fn delete_credential(provider: String, key: String) -> IpcResult<()> {
    ipc_result(config::CredentialStore::delete(&provider, &key))
}

/// merge_subtitle：合并字幕到视频
#[tauri::command]
pub fn merge_subtitle(
    video_path: String,
    subtitle_path: String,
    output_path: String,
    language: Option<String>,
    ffmpeg_path: Option<String>,
    db: State<'_, Database>,
) -> IpcResult<()> {
    ipc_result((|| {
        ffmpeg::merge_subtitle_to_video(
            &video_path,
            &subtitle_path,
            &output_path,
            language.as_deref(),
            ffmpeg_path.as_deref(),
        )?;
        let _ = db.add_history(&HistoryRecord {
            video_path: Some(video_path),
            subtitle_path: Some(subtitle_path),
            source_lang: None,
            target_lang: None,
            provider: None,
            action: "merge".to_string(),
            status: "success".to_string(),
            detail: Some(output_path),
        });
        Ok(())
    })())
}

// === SECTION 3 END ===

/// search_subtitles_online：在线搜索字幕
#[tauri::command]
pub fn search_subtitles_online(
    query: String,
    language: String,
    api_key: String,
) -> IpcResult<Vec<crate::search::SubtitleSearchResult>> {
    ipc_result(crate::search::search_subtitles(&query, &language, &api_key))
}

/// download_subtitle_online：下载在线字幕
#[tauri::command]
pub fn download_subtitle_online(
    subtitle_id: String,
    api_key: String,
    output_path: String,
) -> IpcResult<()> {
    ipc_result(crate::search::download_subtitle(&subtitle_id, &api_key, std::path::Path::new(&output_path)))
}

/// register_video_menu：注册视频右键菜单
#[tauri::command]
pub fn register_video_menu(exe_path: String) -> IpcResult<()> {
    ipc_result(crate::context_menu::register_video_context_menu(&exe_path))
}

/// unregister_video_menu：注销视频右键菜单
#[tauri::command]
pub fn unregister_video_menu() -> IpcResult<()> {
    ipc_result(crate::context_menu::unregister_video_context_menu())
}

/// register_subtitle_menu：注册字幕右键菜单
#[tauri::command]
pub fn register_subtitle_menu(exe_path: String) -> IpcResult<()> {
    ipc_result(crate::context_menu::register_subtitle_context_menu(&exe_path))
}

/// unregister_subtitle_menu：注销字幕右键菜单
#[tauri::command]
pub fn unregister_subtitle_menu() -> IpcResult<()> {
    ipc_result(crate::context_menu::unregister_subtitle_context_menu())
}

/// is_video_menu_registered：检查视频右键菜单是否已注册
#[tauri::command]
pub fn is_video_menu_registered() -> IpcResult<bool> {
    ipc_result(Ok(crate::context_menu::is_video_context_menu_registered()))
}

/// is_subtitle_menu_registered：检查字幕右键菜单是否已注册
#[tauri::command]
pub fn is_subtitle_menu_registered() -> IpcResult<bool> {
    ipc_result(Ok(crate::context_menu::is_subtitle_context_menu_registered()))
}

/// get_libmpv_status_cmd：获取 libmpv 下载状态
#[tauri::command]
pub fn get_libmpv_status_cmd(
    app: tauri::AppHandle,
) -> IpcResult<crate::player::LibmpvStatus> {
    ipc_result((|| {
        let app_data_dir = app.path().app_data_dir().map_err(|e| {
            AppError::Unknown {
                detail: format!("获取数据目录失败: {}", e),
            }
        })?;
        Ok(crate::player::get_libmpv_status(&app_data_dir))
    })())
}

/// download_libmpv_cmd：下载 libmpv 播放组件（异步，emit 进度事件）
#[tauri::command]
pub async fn download_libmpv_cmd(
    app: tauri::AppHandle,
    proxy: Option<String>,
) -> Result<IpcResult<()>, ()> {
    let app_data_dir = app.path().app_data_dir().map_err(|_| ())?;
    let app_handle = app.clone();
    let proxy_clone = proxy.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::player::download_libmpv(&app_data_dir, proxy_clone.as_deref(), &app_handle)
    }).await;
    match result {
        Ok(Ok(())) => Ok(IpcResult::from(Ok(()))),
        Ok(Err(e)) => Ok(IpcResult::from(Err(e))),
        Err(e) => Ok(IpcResult::from(Err(AppError::PlayerLibmpvDownloadFailed {
            detail: format!("下载任务失败: {}", e),
        }))),
    }
}

/// open_in_system_player_cmd：用系统播放器打开视频
#[tauri::command]
pub fn open_in_system_player_cmd(video_path: String) -> IpcResult<()> {
    ipc_result(crate::player::open_in_system_player(&video_path))
}

// === SECTION 4 END ===

// === player_* IPC 命令（libmpv 内嵌播放） ===

use std::sync::Mutex;

/// 全局 Player 状态
static PLAYER: Mutex<Option<crate::player::Player>> = Mutex::new(None);

/// player_init：初始化 libmpv 播放器，创建子窗口嵌入 Tauri 主窗口
#[cfg(windows)]
#[tauri::command]
pub fn player_init(
    app: tauri::AppHandle,
    window: tauri::Window,
    dll_path: String,
    x: i32, y: i32, w: i32, h: i32,
) -> Result<(), ()> {
    use windows::Win32::Foundation::HWND;
    tracing::info!("player_init 开始: dll={}, x={}, y={}, w={}, h={}", dll_path, x, y, w, h);
    let hwnd = window.hwnd().map_err(|e| {
        tracing::error!("获取窗口 HWND 失败: {:?}", e);
    })?;
    let parent = HWND(hwnd.0 as *mut _);
    tracing::info!("父窗口 HWND: {:?}", parent);
    match crate::player::Player::new(&dll_path, parent, app, x, y, w, h) {
        Ok(player) => {
            tracing::info!("player_init 成功");
            *PLAYER.lock().unwrap() = Some(player);
            Ok(())
        }
        Err(e) => {
            tracing::error!("player_init 失败: {:?}", e);
            Err(())
        }
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub fn player_init(_app: tauri::AppHandle, _window: tauri::Window, _dll_path: String, _x: i32, _y: i32, _w: i32, _h: i32) -> Result<(), ()> {
    Err(())
}

/// player_load_cmd：加载视频文件
#[tauri::command]
pub fn player_load_cmd(file_path: String) -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.load(&file_path).map_err(|_| ())
    } else {
        Err(())
    }
}

/// player_play_cmd：播放
#[tauri::command]
pub fn player_play_cmd() -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.play().map_err(|_| ())
    } else { Err(()) }
}

/// player_pause_cmd：暂停
#[tauri::command]
pub fn player_pause_cmd() -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.pause().map_err(|_| ())
    } else { Err(()) }
}

/// player_seek_cmd：跳转到指定时间（秒）
#[tauri::command]
pub fn player_seek_cmd(time_sec: f64) -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.seek(time_sec).map_err(|_| ())
    } else { Err(()) }
}

/// player_set_volume_cmd：设置音量 (0-100)
#[tauri::command]
pub fn player_set_volume_cmd(volume: i32) -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.set_volume(volume).map_err(|_| ())
    } else { Err(()) }
}

/// player_set_speed_cmd：设置倍速
#[tauri::command]
pub fn player_set_speed_cmd(speed: f64) -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.set_speed(speed).map_err(|_| ())
    } else { Err(()) }
}

/// player_get_position_cmd：获取当前播放位置和时长
#[tauri::command]
pub fn player_get_position_cmd() -> Result<(f64, f64), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        let pos = player.get_position().unwrap_or(0.0);
        let dur = player.get_duration().unwrap_or(0.0);
        Ok((pos, dur))
    } else { Err(()) }
}

/// player_resize_cmd：调整子窗口位置和大小
#[tauri::command]
pub fn player_resize_cmd(x: i32, y: i32, w: i32, h: i32) -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.resize(x, y, w, h).map_err(|_| ())
    } else { Err(()) }
}

/// player_show_cmd：显示子窗口
#[tauri::command]
pub fn player_show_cmd() -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.show();
        Ok(())
    } else { Err(()) }
}

/// player_hide_cmd：隐藏子窗口（用于弹窗层级处理）
#[tauri::command]
pub fn player_hide_cmd() -> Result<(), ()> {
    let guard = PLAYER.lock().unwrap();
    if let Some(ref player) = *guard {
        player.hide();
        Ok(())
    } else { Err(()) }
}

/// player_destroy_cmd：销毁播放器
#[tauri::command]
pub fn player_destroy_cmd() -> Result<(), ()> {
    let mut guard = PLAYER.lock().unwrap();
    *guard = None; // Drop 会调用 destroy
    Ok(())
}

// === SECTION 5 END ===
