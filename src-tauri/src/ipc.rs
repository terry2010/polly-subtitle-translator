// IPC 命令注册与 handler
// 对应需求文档 §7 IPC 命令清单

use crate::config;
use crate::db::{Database, HistoryRecord, RecentFile};
use crate::error::{ipc_result, AppError, IpcError, IpcResult, Severity};
use crate::ffmpeg;
use crate::subtitle;
use crate::translate::{self, TranslateProvider, ProviderCredentials, ProxyConfig, TestConnectionResult};
use tauri::{Emitter, Manager, State};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

/// 全局翻译取消令牌（代际计数器）
/// 每次新翻译 fetch_add 获得唯一代际号，cancel 时 fetch_add 使旧代际失效
/// 运行中的翻译检查 counter != my_gen 则视为被取消
pub type CancelToken = Arc<AtomicU64>;

/// 将 AppError 转为 IpcError（用于 async 命令返回 Result<T, IpcError>）
fn to_ipc_err(e: AppError) -> IpcError {
    e.to_ipc_error()
}

/// 计算翻译（含人名预扫描）的实际并发数
/// 规则：用户可覆盖 provider 的默认限流策略（config key 形如 translate_openai_{sid}_qps），
/// 再与全局 translate_concurrency 取最小值。Qps 模式下强制串行（1 并发）。
fn resolve_translation_concurrency(
    db: &Database,
    prov: &TranslateProvider,
    service_id: &Option<String>,
) -> Result<usize, IpcError> {
    let default_policy = prov.rate_limit_policy();
    let config_key = if *prov == TranslateProvider::OpenAi {
        service_id.as_ref()
            .map(|sid| format!("translate_openai_{}_qps", sid))
            .unwrap_or_else(|| "translate_openai_qps".to_string())
    } else {
        format!("translate_{}_qps", prov.as_str())
    };
    let user_override = db.get_config(&config_key)
        .ok().flatten()
        .and_then(|v| v.parse::<usize>().ok());
    let rate_limit = match (default_policy, user_override) {
        (translate::RateLimitPolicy::Qps(_), Some(q)) => translate::RateLimitPolicy::Qps(q),
        (translate::RateLimitPolicy::Concurrency(_), Some(c)) => translate::RateLimitPolicy::Concurrency(c),
        (policy, None) => policy,
    };

    let user_concurrency = db.get_config("translate_concurrency")
        .map_err(to_ipc_err)?
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(3);
    let final_concurrency = match &rate_limit {
        translate::RateLimitPolicy::Qps(_) => 1,
        translate::RateLimitPolicy::Concurrency(max_n) => user_concurrency.min(*max_n).max(1),
    };
    tracing::info!(
        "翻译并发解析: 用户配置并发={}, 限流策略={:?}, 最终并发={}, config_key={}",
        user_concurrency, rate_limit, final_concurrency, config_key
    );
    Ok(final_concurrency)
}

/// 获取所有 IPC 命令 handler
pub fn get_invoke_handlers() -> Box<dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync> {
    Box::new(tauri::generate_handler![
        probe_video,
        extract_subtitle,
        cancel_extract_subtitle,
        parse_subtitle_file,
        detect_bilingual,
        split_bilingual_subtitle,
        save_subtitle_file_cmd,
        export_subtitle_cmd,
        edit_subtitle_streams_cmd,
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
        extract_names,
        get_cached_translations,
        test_translate_connection,
        list_openai_models,
        save_credential,
        get_credential,
        delete_credential,
        merge_subtitle,
        check_merge_space,
        search_subtitles_online,
        download_subtitle_online,
        simplify_search_keyword,
        search_subtitles_with_captcha,
        register_video_menu,
        unregister_video_menu,
        register_subtitle_menu,
        unregister_subtitle_menu,
        is_video_menu_registered,
        is_subtitle_menu_registered,
        get_libmpv_status_cmd,
        download_libmpv_cmd,
        delete_libmpv_cmd,
        get_ffmpeg_status_cmd,
        download_ffmpeg_cmd,
        delete_ffmpeg_cmd,
        open_in_system_player_cmd,
        list_installed_players_cmd,
        open_with_player_cmd,
        reveal_in_explorer_cmd,
        open_path_cmd,
        get_crash_log_dir_cmd,
        clear_crash_logs_cmd,
        get_prompt_fail_dir_cmd,
        list_prompt_fail_logs_cmd,
        read_prompt_fail_log_cmd,
        delete_prompt_fail_log_cmd,
        clear_prompt_fail_logs_cmd,
        set_dev_mode_cmd,
        set_log_api_enabled_cmd,
        get_api_debug_dir_cmd,
        list_api_debug_logs_cmd,
        clear_api_debug_logs_cmd,
        extract_player_icons_cmd,
        clear_player_icons_cache_cmd,
        player_init,
        player_load_cmd,
        player_play_cmd,
        player_pause_cmd,
        player_seek_cmd,
        player_set_volume_cmd,
        player_set_speed_cmd,
        player_set_audio_track_cmd,
        player_get_position_cmd,
        player_resize_cmd,
        dev_log_cmd,
        set_space_disabled_cmd,
        is_cursor_in_window_cmd,
        player_show_cmd,
        player_hide_cmd,
        player_destroy_cmd,
        set_proxy,
        get_proxy,
        get_translate_use_proxy,
        set_translate_use_proxy,
        test_proxy,
        get_system_lang,
        get_work_area,
        toggle_devtools,
        check_for_update,
        download_and_install_update,
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
        Err(e) => Ok(IpcResult::from(Err(AppError::FfmpegProbeTaskFailed { detail: e.to_string() }))),
    }
}

/// extract_subtitle：提取字幕流（带进度推送）
#[tauri::command]
pub async fn extract_subtitle(
    video_path: String,
    stream_index: i32,
    output_path: String,
    ffmpeg_path: Option<String>,
    duration_sec: Option<f64>,
    app: tauri::AppHandle,
    db: State<'_, Database>,
) -> Result<IpcResult<()>, ()> {
    use tauri::Emitter;
    let output_path_clone = output_path.clone();
    let app_handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let on_progress: Box<dyn Fn(f64)> = Box::new(move |pct: f64| {
            let _ = app_handle.emit("extract_progress", serde_json::json!({
                "progress": (pct * 10.0).round() / 10.0, // 保留 1 位小数
            }));
        });
        ffmpeg::extract_subtitle_stream(
            &video_path,
            stream_index,
            &output_path,
            ffmpeg_path.as_deref(),
            duration_sec,
            Some(&on_progress),
        )
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
        Err(e) => Ok(IpcResult::from(Err(AppError::FfmpegExtractTaskFailed { detail: e.to_string() }))),
    }
}

/// cancel_extract_subtitle：取消正在进行的字幕提取
#[tauri::command]
pub fn cancel_extract_subtitle() -> Result<(), ()> {
    ffmpeg::cancel_extraction();
    Ok(())
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
    // 拆分后 entry.text 变为单语原文，内容已改变，需重新计算 hash
    file.file_hash = subtitle::compute_subtitle_hash(&file.entries);
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

/// export_subtitle_cmd：按导出选项渲染并保存字幕（export-dialog-plan.md §4.5）
#[tauri::command]
pub fn export_subtitle_cmd(
    file: subtitle::SubtitleFile,
    output_path: String,
    options: subtitle::ExportOptions,
) -> IpcResult<()> {
    ipc_result(subtitle::export_subtitle_file(&file, &output_path, &options))
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
/// skip_cache: true 时跳过缓存查询，强制重新请求 API（用于"重新翻译"）
/// glossary: 译名表 [(EnglishName, ChineseTranslation)]，注入到 AI 翻译的 system prompt
/// name_tagging: 是否要求 AI 在译文中用 <name=En>Zh</name> 标记人名（用于后处理一致性检查）
/// file_hash 由后端从 entries 直接计算（compute_subtitle_hash），不依赖前端传参，
/// 确保前端编辑条目后 hash 自动更新，避免陈旧 hash 污染缓存。
#[tauri::command]
pub async fn translate_subtitle(
    entries: Vec<subtitle::SubtitleEntry>,
    source_lang: String,
    target_lang: String,
    provider: String,
    model: Option<String>,
    model_type: Option<String>,
    service_id: Option<String>,
    skip_cache: Option<bool>,
    glossary: Option<Vec<(String, String)>>,
    name_tagging: Option<bool>,
    db: State<'_, Database>,
    cancel_token: State<'_, CancelToken>,
    app: tauri::AppHandle,
) -> Result<translate::TranslateResult, IpcError> {
    tracing::info!("translate_subtitle 调用: provider={}, model={:?}, model_type={:?}, service_id={:?}, entries={}, source={}, target={}, glossary={}, name_tagging={:?}", provider, model, model_type, service_id, entries.len(), source_lang, target_lang, glossary.as_ref().map(|g| g.len()).unwrap_or(0), name_tagging.unwrap_or(false));

    // 代际计数器：获取唯一代际号，使之前的翻译任务自动失效
    let my_gen = cancel_token.fetch_add(1, Ordering::Relaxed) + 1;

    // comingSoon 拦截：在 TranslateProvider::from_str 之前，返回友好错误

    let prov = TranslateProvider::from_str(&provider).ok_or_else(|| {
        AppError::TranslateUnknownProvider { provider: provider.clone() }
    }).map_err(to_ipc_err)?;

    // 从 config 表读取凭据配置
    let config_key = format!("translate_{}_app_id", provider);
    let app_id = db.get_config(&config_key).map_err(to_ipc_err)?;
    tracing::info!("translate_subtitle app_id from config: {:?}", app_id);
    let region_ref = format!("translate_{}_region", provider);
    let region = db.get_config(&region_ref).map_err(to_ipc_err)?;

    // OpenAi 专属配置：base_url / model / model_type（per-service 读取）
    let base_url = if prov == TranslateProvider::OpenAi {
        let key = match &service_id {
            Some(sid) => format!("translate_openai_{}_base_url", sid),
            None => "translate_openai_base_url".to_string(),
        };
        db.get_config(&key).map_err(to_ipc_err)?
    } else { None };

    // model：AI 翻译时前端必传，不再从 db fallback（避免读到其他服务的模型）
    let model = if prov == TranslateProvider::OpenAi { model } else { None };

    // model_type：per-service 从 db 读取映射，再 fallback from_model_id
    let model_type = if prov == TranslateProvider::OpenAi {
        if let Some(mt) = model_type {
            Some(mt)
        } else if let Some(m) = &model {
            let types_key = match &service_id {
                Some(sid) => format!("translate_openai_{}_selected_model_types", sid),
                None => "translate_openai_selected_model_types".to_string(),
            };
            if let Ok(Some(json)) = db.get_config(&types_key) {
                if let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json) {
                    if let Some(val) = map.get(m) {
                        if let Some(s) = val.as_str() { Some(s.to_string()) } else { None }
                    } else {
                        Some(translate::ModelType::from_model_id(m).as_str().to_string())
                    }
                } else {
                    Some(translate::ModelType::from_model_id(m).as_str().to_string())
                }
            } else {
                Some(translate::ModelType::from_model_id(m).as_str().to_string())
            }
        } else { None }
    } else { None };

    // 从 keyring 读取密钥：AI 服务用 openai_{service_id} 作为 keyring provider key
    let keyring_provider = if prov == TranslateProvider::OpenAi {
        match &service_id {
            Some(sid) => format!("openai_{}", sid),
            None => "openai".to_string(),
        }
    } else {
        provider.clone()
    };
    let secret = match config::CredentialStore::load(&keyring_provider, "secret", "翻译字幕") {
        Ok(s) => {
            tracing::info!("translate_subtitle secret from keyring: 已获取");
            Some(s)
        }
        Err(AppError::StorageCredentialNotFound { .. }) => {
            tracing::info!("translate_subtitle keyring: 凭据未配置");
            None
        }
        Err(e) => {
            tracing::warn!("translate_subtitle keyring 读取失败: {}", e);
            None
        }
    };
    tracing::info!("translate_subtitle secret: {:?}", if secret.is_some() { "Some" } else { "None" });

    // 验证凭据存在
    // OpenAi 只要求 base_url 存在（api_key 可选，局域网无认证）
    if prov == TranslateProvider::OpenAi {
        if base_url.is_none() {
            return Err(AppError::TranslateCredentialsNotConfigured.to_ipc_error());
        }
    } else if app_id.is_none() && secret.is_none() {
        return Err(AppError::TranslateCredentialsNotConfigured.to_ipc_error());
    }

    let credentials = ProviderCredentials {
        app_id,
        secret_key: secret,
        region,
        base_url,
        model: model.clone(),
        model_type: model_type.clone(),
    };

    // 代理：per-service 读取
    let proxy_config = ProxyConfig::load_from_db(&db);
    let use_proxy_key = if prov == TranslateProvider::OpenAi {
        match &service_id {
            Some(sid) => format!("translate_openai_{}_use_proxy", sid),
            None => format!("translate_{}_use_proxy", provider),
        }
    } else {
        format!("translate_{}_use_proxy", provider)
    };
    let use_proxy = db.get_config(&use_proxy_key).ok().flatten();
    let effective_proxy = match use_proxy.as_deref() {
        Some("false") => ProxyConfig::default(),
        _ => proxy_config,
    };
    tracing::info!("translate_subtitle proxy: use_proxy={:?}, mode={}", use_proxy, effective_proxy.mode);

    // AI 服务：用真实服务商名作为 service_name（错误消息中显示）
    let service_name = if prov == translate::TranslateProvider::OpenAi {
        service_id.as_deref().map(translate::ai_service_display_name)
    } else {
        None
    };
    // 构造 AI 翻译附加选项（glossary / name_tagging），传统翻译忽略
    let glossary_vec = glossary.clone().unwrap_or_default();
    let provider_options = translate::ProviderOptions {
        glossary: glossary_vec,
        name_tagging: name_tagging.unwrap_or(false),
    };
    let prov_instance = translate::create_provider_with_proxy(&prov, &credentials, &effective_proxy, service_name, &provider_options).map_err(to_ipc_err)?;

    // 缓存 key 隔离：provider_name 纳入 service_id + model
    let provider_name = if prov == TranslateProvider::OpenAi {
        match (&service_id, &model) {
            (Some(sid), Some(m)) => translate::build_cache_provider_name(&["openai", sid, m]),
            _ => "openai".to_string(),
        }
    } else {
        provider.clone()
    };

    // 计算实际并发数（限流策略 + 用户覆盖 + 全局并发数取最小）
    let final_concurrency = resolve_translation_concurrency(&db, &prov, &service_id)?;
    let rate_limit = prov.rate_limit_policy();
    let config_key = if prov == TranslateProvider::OpenAi {
        service_id.as_ref()
            .map(|sid| format!("translate_openai_{}_qps", sid))
            .unwrap_or_else(|| "translate_openai_qps".to_string())
    } else {
        format!("translate_{}_qps", prov.as_str())
    };
    let user_override = db.get_config(&config_key)
        .ok().flatten()
        .and_then(|v| v.parse::<usize>().ok());
    let rate_limit = match (rate_limit, user_override) {
        (translate::RateLimitPolicy::Qps(_), Some(q)) => translate::RateLimitPolicy::Qps(q),
        (translate::RateLimitPolicy::Concurrency(_), Some(c)) => translate::RateLimitPolicy::Concurrency(c),
        (policy, None) => policy,
    };

    // 从 entries 直接计算字幕内容 hash，用于缓存隔离
    // 不依赖前端传参，确保前端编辑条目后 hash 自动更新
    let file_hash = subtitle::compute_subtitle_hash(&entries);

    let scheduler = translate::TranslateScheduler::with_cancel_token(
        &db,
        prov_instance,
        provider_name,
        cancel_token.inner().clone(),
        my_gen,
    )
    .with_file_hash(file_hash)
    .with_concurrency_and_rate_limit(
        db.get_config("translate_concurrency")
            .map_err(to_ipc_err)?
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(3),
        rate_limit,
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
            "failed": entry.failed,
        }));
    });

    let mut result = scheduler
        .translate_entries_full(&entries, &source_lang, &target_lang, 5000, Some(progress_cb), Some(entry_cb), skip_cache.unwrap_or(false))
        .await
        .map_err(to_ipc_err)?;

    // 人名标记后处理：剥离 <name> 标签 + 检测不一致 + 全局替换
    if name_tagging.unwrap_or(false) {
        let mut translations: Vec<String> = result.translations.iter().map(|t| t.translated.clone()).collect();
        let pre_scan_glossary: Vec<(String, String)> = glossary.clone().unwrap_or_default();
        let consistency = translate::post_process_name_tags(&mut translations, &pre_scan_glossary);

        if !consistency.inconsistencies.is_empty() {
            tracing::info!("人名一致性后处理: 发现 {} 个不一致人名", consistency.inconsistencies.len());
            for inc in &consistency.inconsistencies {
                tracing::info!("  不一致: {} → {} (候选: {:?})", inc.english, inc.chosen, inc.translations);
            }
        }
        if !consistency.corrected_indices.is_empty() {
            tracing::info!("人名一致性后处理: 修正了 {} 条翻译", consistency.corrected_indices.len());
        }

        // 回写修正后的翻译
        for (i, corrected) in &consistency.corrected_indices {
            if let Some(entry) = result.translations.get_mut(*i) {
                entry.translated = corrected.clone();
            }
        }
    }

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
    cancel_token.fetch_add(1, Ordering::Relaxed);
    tracing::info!("收到取消翻译请求");
    ipc_result(Ok(()))
}

/// extract_names：从字幕文本中预扫描提取人名（仅 AI 翻译支持）
/// 返回人名译名表，用于注入翻译 prompt 保证跨 batch 一致性
#[tauri::command]
pub async fn extract_names(
    texts: Vec<String>,
    source_lang: String,
    target_lang: String,
    provider: String,
    model: Option<String>,
    model_type: Option<String>,
    service_id: Option<String>,
    db: State<'_, Database>,
    cancel_token: State<'_, CancelToken>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<translate::ExtractedName>, IpcError> {
    tracing::info!("extract_names 调用: provider={}, model={:?}, texts={}, source={}, target={}", provider, model, texts.len(), source_lang, target_lang);
    let extract_start = std::time::Instant::now();
    let my_gen = cancel_token.fetch_add(1, Ordering::Relaxed) + 1;

    let prov = TranslateProvider::from_str(&provider).ok_or_else(|| {
        AppError::TranslateUnknownProvider { provider: provider.clone() }
    }).map_err(to_ipc_err)?;

    // 仅 AI 翻译支持人名提取
    if prov != TranslateProvider::OpenAi {
        return Err(AppError::TranslateNotConfigured.to_ipc_error());
    }

    // 读取配置（与 translate_subtitle 相同的逻辑）
    let base_url_key = match &service_id {
        Some(sid) => format!("translate_openai_{}_base_url", sid),
        None => "translate_openai_base_url".to_string(),
    };
    let base_url = db.get_config(&base_url_key).map_err(to_ipc_err)?;
    let model = model.ok_or_else(|| AppError::TranslateNotConfigured.to_ipc_error())?;
    let model_type = if let Some(mt) = model_type {
        translate::ModelType::from_str(&mt)
    } else {
        Some(translate::ModelType::from_model_id(&model))
    };
    let model_type = model_type.unwrap_or(translate::ModelType::Generic);

    let keyring_provider = match &service_id {
        Some(sid) => format!("openai_{}", sid),
        None => "openai".to_string(),
    };
    let api_key = match config::CredentialStore::load(&keyring_provider, "secret", "人名预扫描") {
        Ok(s) => Some(s),
        Err(_) => None,
    };

    let base_url = base_url.ok_or_else(|| AppError::TranslateNotConfigured.to_ipc_error())?;
    let proxy_config = ProxyConfig::load_from_db(&db);
    let use_proxy_key = match &service_id {
        Some(sid) => format!("translate_openai_{}_use_proxy", sid),
        None => format!("translate_{}_use_proxy", provider),
    };
    let use_proxy = db.get_config(&use_proxy_key).ok().flatten();
    let effective_proxy = match use_proxy.as_deref() {
        Some("false") => ProxyConfig::default(),
        _ => proxy_config,
    };

    let credentials = ProviderCredentials {
        app_id: None,
        secret_key: api_key,
        region: None,
        base_url: Some(base_url),
        model: Some(model),
        model_type: Some(model_type.as_str().to_string()),
    };
    let service_name = service_id.as_deref().map(translate::ai_service_display_name);
    let provider_options = translate::ProviderOptions::default();
    let prov_instance = translate::create_provider_with_proxy(
        &prov, &credentials, &effective_proxy, service_name, &provider_options
    ).map_err(to_ipc_err)?;

    let max_input_tokens = model_type.max_input_tokens();
    // 人名预扫描使用与翻译阶段相同的最终并发数（QPS 上限/全局并发数取最小）
    let final_concurrency = resolve_translation_concurrency(&db, &prov, &service_id)?;
    tracing::info!("人名预扫描并发度: {}", final_concurrency);
    let result = translate::extract_names_from_subtitles(
        prov_instance, &texts, &source_lang, &target_lang, max_input_tokens,
        cancel_token.inner().clone(),
        my_gen,
        Some(app_handle),
        final_concurrency,
    ).await.map_err(to_ipc_err)?;

    let elapsed = extract_start.elapsed();
    tracing::info!("extract_names 完成: {} 个人名, 耗时 {:.2}s", result.len(), elapsed.as_secs_f64());
    Ok(result)
}

/// get_cached_translations：查询已缓存的翻译结果（不调用 API）
/// file_hash 由后端从 entries 直接计算，确保与 translate_subtitle 使用相同的 hash
#[tauri::command]
pub async fn get_cached_translations(
    entries: Vec<subtitle::SubtitleEntry>,
    source_lang: String,
    target_lang: String,
    provider: String,
    service_id: Option<String>,
    model: Option<String>,
    db: State<'_, Database>,
) -> Result<Vec<translate::TranslateEntry>, IpcError> {
    let prov = TranslateProvider::from_str(&provider).ok_or_else(|| {
        AppError::TranslateUnknownProvider { provider: provider.clone() }.to_ipc_error()
    })?;

    // 缓存 key 隔离：必须与 translate_subtitle 构造一致的 provider_name
    let provider_name = if prov == TranslateProvider::OpenAi {
        match (&service_id, &model) {
            (Some(sid), Some(m)) => translate::build_cache_provider_name(&["openai", sid, m]),
            _ => "openai".to_string(),
        }
    } else {
        prov.as_str().to_string()
    };

    // 从 entries 直接计算 hash，与 translate_subtitle 一致
    let file_hash = subtitle::compute_subtitle_hash(&entries);

    // 获取凭据（缓存查询不需要凭据，但需要 provider_name）
    let provider_name_log = provider_name.clone();
    let scheduler = translate::TranslateScheduler::new(
        &db,
        std::sync::Arc::new(translate::BaiduProvider::new(String::new(), String::new()))
            as std::sync::Arc<dyn translate::TranslateProviderTrait + Send + Sync>,
        provider_name,
    )
    .with_file_hash(file_hash);

    let cached = scheduler
        .get_cached_entries(&entries, &source_lang, &target_lang)
        .map_err(to_ipc_err)?;

    tracing::info!(
        "缓存查询: {} 条中命中 {} 条 (provider={}, source={}, target={})",
        entries.len(), cached.len(), provider_name_log, source_lang, target_lang
    );
    Ok(cached)
}

/// test_translate_connection：测试翻译 API 连接
/// 返回 TestConnectionResult（OpenAi 包含原文+译文，其他 provider 字段为 None）
#[tauri::command]
pub async fn test_translate_connection(
    provider: String,
    app_id: Option<String>,
    secret_key: Option<String>,
    region: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    model_type: Option<String>,
    service_id: Option<String>,
    db: State<'_, Database>,
) -> Result<TestConnectionResult, IpcError> {
    let prov = TranslateProvider::from_str(&provider).ok_or_else(|| {
        AppError::TranslateUnknownProvider { provider: provider.clone() }
    }).map_err(to_ipc_err)?;

    // OpenAi 专属：删除 db fallback，前端必传当前编辑值
    let (base_url, model, model_type) = if prov == TranslateProvider::OpenAi {
        (base_url, model, model_type)
    } else {
        (None, None, None)
    };

    // 密钥 fallback：前端传 None/空（掩码状态）时从 keyring 加载
    let secret_key = if secret_key.is_none() || secret_key.as_deref() == Some("") {
        let keyring_provider = if prov == TranslateProvider::OpenAi {
            match &service_id {
                Some(sid) => format!("openai_{}", sid),
                None => "openai".to_string(),
            }
        } else {
            provider.clone()
        };
        config::CredentialStore::load(&keyring_provider, "secret", "测试翻译连接")
            .ok()
            .filter(|s| !s.is_empty())
    } else {
        secret_key
    };

    let credentials = ProviderCredentials {
        app_id,
        secret_key,
        region,
        base_url,
        model,
        model_type,
    };

    // 测试连接也按 per-provider 代理开关决定是否用代理
    let proxy_config = ProxyConfig::load_from_db(&db);
    let use_proxy_key = if prov == TranslateProvider::OpenAi {
        match &service_id {
            Some(sid) => format!("translate_openai_{}_use_proxy", sid),
            None => format!("translate_{}_use_proxy", provider),
        }
    } else {
        format!("translate_{}_use_proxy", provider)
    };
    let use_proxy = db.get_config(&use_proxy_key).ok().flatten();
    let effective_proxy = match use_proxy.as_deref() {
        Some("false") => ProxyConfig::default(),
        _ => proxy_config,
    };

    // AI 服务：用真实服务商名作为 service_name（错误消息中显示）
    let service_name = if prov == TranslateProvider::OpenAi {
        service_id.as_deref().map(translate::ai_service_display_name)
    } else {
        None
    };
    let prov_instance = translate::create_provider_with_proxy(&prov, &credentials, &effective_proxy, service_name, &translate::ProviderOptions::default()).map_err(to_ipc_err)?;

    // OpenAi：直接翻译测试文本，返回原文+译文
    if prov == TranslateProvider::OpenAi {
        let test_text = "Hello";
        let translated = prov_instance.translate(&[test_text.to_string()], "en", "zh").await.map_err(to_ipc_err)?;
        let translated_text = translated.into_iter().next().unwrap_or_default();
        return Ok(TestConnectionResult {
            original: Some(test_text.to_string()),
            translated: Some(translated_text),
        });
    }

    // 其他 provider：仅测试连通性
    prov_instance.test_connection().await.map_err(to_ipc_err)?;
    Ok(TestConnectionResult { original: None, translated: None })
}

/// list_openai_models：调用 GET {base_url}/models 拉取可用模型列表
/// 用于设置页"刷新模型列表"按钮，让用户下拉选择模型
#[tauri::command]
pub async fn list_openai_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<Vec<String>, IpcError> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| IpcError::new("openai.modelsFetchFailed", Severity::Recoverable)
            .with_args(serde_json::json!({ "detail": e.to_string() })))?;

    let mut req = client.get(&url);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    let resp = req.send().await.map_err(|e| {
        IpcError::new("openai.modelsFetchFailed", Severity::Recoverable)
            .with_args(serde_json::json!({ "detail": e.to_string() }))
    })?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(IpcError::new("translate.authFailed", Severity::Recoverable)
            .with_args(serde_json::json!({ "provider": "openai" })));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(IpcError::new("openai.modelsFetchFailed", Severity::Recoverable)
            .with_args(serde_json::json!({ "detail": format!("HTTP {}: {}", status, body) })));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        IpcError::new("openai.modelsFetchFailed", Severity::Recoverable)
            .with_args(serde_json::json!({ "detail": e.to_string() }))
    })?;

    // OpenAI 标准响应：{ object: "list", data: [{ id, ... }] }
    let models: Vec<String> = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if models.is_empty() {
        return Err(IpcError::new("openai.noModels", Severity::Recoverable));
    }

    Ok(models)
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
pub fn get_credential(provider: String, key: String, reason: Option<String>) -> IpcResult<Option<String>> {
    let reason = reason.unwrap_or_else(|| "前端查询凭据".to_string());
    let result = match config::CredentialStore::load(&provider, &key, &reason) {
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
/// output_path = None: 直接修改原视频（临时文件+替换）
/// output_path = Some: 输出到指定路径
/// async + spawn_blocking：ffmpeg 处理大视频耗时，避免阻塞 Tauri 命令线程导致 UI 卡死
#[tauri::command]
pub async fn merge_subtitle(
    video_path: String,
    subtitle_path: String,
    output_path: Option<String>,
    language: Option<String>,
    title: Option<String>,
    ffmpeg_path: Option<String>,
    db: State<'_, Database>,
) -> Result<(), IpcError> {
    let vp = video_path.clone();
    let sp = subtitle_path.clone();
    let op = output_path.clone();
    let lang = language.clone();
    let ttl = title.clone();
    let fp = ffmpeg_path.clone();
    tokio::task::spawn_blocking(move || {
        ffmpeg::merge_subtitle_to_video(
            &vp,
            &sp,
            op.as_deref(),
            lang.as_deref(),
            ttl.as_deref(),
            fp.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::FfmpegMergeTaskFailed { detail: e.to_string() }.to_ipc_error())?
    .map_err(|e| e.to_ipc_error())?;

    let _ = db.add_history(&HistoryRecord {
        video_path: Some(video_path),
        subtitle_path: Some(subtitle_path),
        source_lang: None,
        target_lang: None,
        provider: None,
        action: "merge".to_string(),
        status: "success".to_string(),
        detail: output_path,
    });
    Ok(())
}

/// check_merge_space：检测原视频所在磁盘剩余空间是否足够合并
/// 返回 { video_size, free_space, enough }
#[tauri::command]
pub fn check_merge_space(video_path: String) -> Result<serde_json::Value, IpcError> {
    let video_size = ffmpeg::get_file_size(&video_path).map_err(|e| e.to_ipc_error())?;
    let free_space = ffmpeg::get_disk_free_space(&video_path).map_err(|e| e.to_ipc_error())?;
    // 需要额外空间 ≈ 视频大小（临时文件和原文件同时存在），留 1GB 余量
    let need = video_size.saturating_add(1024 * 1024 * 1024);
    let enough = free_space >= need;
    Ok(serde_json::json!({
        "video_size": video_size,
        "free_space": free_space,
        "enough": enough,
    }))
}

// === SECTION 3 END ===

/// search_subtitles_online：在线搜索字幕
/// source: "opensubtitles" | "subhd" | "zimuku"
#[tauri::command]
pub async fn search_subtitles_online(
    query: String,
    language: String,
    api_key: String,
    source: Option<String>,
    db: State<'_, Database>,
) -> Result<IpcResult<Vec<crate::search::SubtitleSearchResult>>, ()> {
    let src = source.unwrap_or_else(|| "opensubtitles".to_string());
    // 读取代理配置
    let proxy = crate::translate::ProxyConfig::load_from_db(&db);
    // SubHD/zimuku 使用 blocking HTTP，放到 spawn_blocking 避免阻塞 UI 线程
    let result = tokio::task::spawn_blocking(move || {
        crate::search::search_subtitles_multi(&query, &language, &api_key, &src, &proxy)
    })
    .await
    .map_err(|e| crate::error::AppError::SearchNetworkError {
        provider: "search".to_string(),
        detail: format!("spawn_blocking 失败: {}", e),
    });
    Ok(match result {
        Ok(inner) => ipc_result(inner),
        Err(e) => ipc_result(Err(e)),
    })
}

/// download_subtitle_online：下载在线字幕
/// subtitle_id 中带 provider 前缀（如 "subhd:..." / "zimuku:..."），OpenSubtitles 为纯数字
#[tauri::command]
pub async fn download_subtitle_online(
    subtitle_id: String,
    api_key: String,
    output_path: String,
) -> IpcResult<()> {
    let result = tokio::task::spawn_blocking(move || {
        crate::search::download_subtitle_multi(&subtitle_id, &api_key, std::path::Path::new(&output_path))
    })
    .await
    .map_err(|_| crate::error::AppError::SearchDownloadFailed {
        provider: "search".to_string(),
    });
    match result {
        Ok(inner) => ipc_result(inner),
        Err(e) => ipc_result(Err(e)),
    }
}

/// simplify_search_keyword：简化视频文件名为搜索关键词
#[tauri::command]
pub fn simplify_search_keyword(filename: String) -> IpcResult<String> {
    ipc_result(Ok(crate::search::simplify_keyword(&filename)))
}

/// search_subtitles_with_captcha：带验证码继续搜索（zimuku 云锁验证码）
#[tauri::command]
pub async fn search_subtitles_with_captcha(
    query: String,
    source: String,
    captcha: String,
    session_cookie: String,
    verify_path: String,
    db: State<'_, Database>,
) -> Result<IpcResult<Vec<crate::search::SubtitleSearchResult>>, ()> {
    let proxy = crate::translate::ProxyConfig::load_from_db(&db);
    let result = tokio::task::spawn_blocking(move || {
        crate::search::search_subtitles_with_captcha(&query, &source, &captcha, &session_cookie, &verify_path, &proxy)
    })
    .await
    .map_err(|e| crate::error::AppError::SearchNetworkError {
        provider: "search".to_string(),
        detail: format!("spawn_blocking 失败: {}", e),
    });
    Ok(match result {
        Ok(inner) => ipc_result(inner),
        Err(e) => ipc_result(Err(e)),
    })
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
            AppError::GetDataDirFailed { detail: e.to_string() }
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
        Err(e) => Ok(IpcResult::from(Err(AppError::DownloadTaskFailed {
            detail: e.to_string(),
        }))),
    }
}

/// delete_libmpv_cmd：删除已下载的 libmpv 组件
#[tauri::command]
pub fn delete_libmpv_cmd(
    app: tauri::AppHandle,
) -> IpcResult<()> {
    ipc_result((|| {
        let app_data_dir = app.path().app_data_dir().map_err(|e| {
            AppError::GetDataDirFailed { detail: e.to_string() }
        })?;
        crate::player::delete_libmpv(&app_data_dir)
    })())
}

/// get_ffmpeg_status_cmd：获取 ffmpeg 安装状态
#[tauri::command]
pub fn get_ffmpeg_status_cmd() -> IpcResult<crate::ffmpeg::FfmpegStatus> {
    ipc_result(Ok(crate::ffmpeg::get_ffmpeg_status()))
}

/// download_ffmpeg_cmd：下载 ffmpeg 完整版（异步，emit 进度事件）
#[tauri::command]
pub async fn download_ffmpeg_cmd(
    app: tauri::AppHandle,
    proxy: Option<String>,
) -> Result<IpcResult<()>, ()> {
    let app_handle = app.clone();
    let proxy_clone = proxy.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::ffmpeg::download_ffmpeg(proxy_clone.as_deref(), &app_handle)
    }).await;
    match result {
        Ok(Ok(())) => Ok(IpcResult::from(Ok(()))),
        Ok(Err(e)) => Ok(IpcResult::from(Err(e))),
        Err(e) => Ok(IpcResult::from(Err(AppError::DownloadTaskFailed {
            detail: e.to_string(),
        }))),
    }
}

/// delete_ffmpeg_cmd：删除已下载的 ffmpeg
#[tauri::command]
pub fn delete_ffmpeg_cmd() -> IpcResult<()> {
    ipc_result(crate::ffmpeg::delete_ffmpeg())
}

/// open_in_system_player_cmd：用系统播放器打开视频
#[tauri::command]
pub fn open_in_system_player_cmd(video_path: String) -> IpcResult<()> {
    ipc_result(crate::player::open_in_system_player(&video_path))
}

/// list_installed_players_cmd：列出已安装的视频播放器（按最近使用顺序）
#[tauri::command]
pub fn list_installed_players_cmd(video_path: String) -> IpcResult<Vec<crate::player::InstalledPlayer>> {
    ipc_result(crate::player::list_installed_players(&video_path))
}

/// open_with_player_cmd：用指定播放器打开视频
#[tauri::command]
pub fn open_with_player_cmd(exe_path: String, video_path: String) -> IpcResult<()> {
    ipc_result(crate::player::open_with_player(&exe_path, &video_path))
}

/// reveal_in_explorer_cmd：在资源管理器中定位文件
#[tauri::command]
pub fn reveal_in_explorer_cmd(file_path: String) -> IpcResult<()> {
    ipc_result(crate::player::reveal_in_explorer(&file_path))
}

/// open_path_cmd：在系统文件管理器中打开目录
#[tauri::command]
pub fn open_path_cmd(path: String) -> IpcResult<()> {
    #[cfg(windows)]
    {
        let result = std::process::Command::new("explorer.exe")
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|e| crate::error::AppError::PlayerLoadFailed {
                detail: format!("explorer ({})", e),
            });
        return ipc_result(result);
    }
    #[cfg(target_os = "macos")]
    {
        let result = std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|e| crate::error::AppError::PlayerLoadFailed {
                detail: format!("open ({})", e),
            });
        return ipc_result(result);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let result = std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|e| crate::error::AppError::PlayerLoadFailed {
                detail: format!("xdg-open ({})", e),
            });
        return ipc_result(result);
    }
    #[cfg(not(any(windows, target_os = "macos", unix)))]
    {
        Ok(())
    }
}

/// get_crash_log_dir_cmd：获取崩溃日志目录路径
#[tauri::command]
pub fn get_crash_log_dir_cmd(app: tauri::AppHandle) -> IpcResult<String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.map(|dir| {
        let crash_dir = dir.join("crashes");
        std::fs::create_dir_all(&crash_dir).ok();
        crash_dir.to_string_lossy().to_string()
    });
    ipc_result(result)
}

/// clear_crash_logs_cmd：清空崩溃日志目录
#[tauri::command]
pub fn clear_crash_logs_cmd(app: tauri::AppHandle) -> IpcResult<usize> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.and_then(|dir| {
        let crash_dir = dir.join("crashes");
        let mut count = 0usize;
        if let Ok(read_dir) = std::fs::read_dir(&crash_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if std::fs::remove_file(&path).is_ok() {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    });
    ipc_result(result)
}

/// get_prompt_fail_dir_cmd：获取 prompt 失败日志目录路径
#[tauri::command]
pub fn get_prompt_fail_dir_cmd(app: tauri::AppHandle) -> IpcResult<String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.map(|dir| {
        let fail_dir = dir.join("prompt_fails");
        std::fs::create_dir_all(&fail_dir).ok();
        fail_dir.to_string_lossy().to_string()
    });
    ipc_result(result)
}

/// list_prompt_fail_logs_cmd：列出 prompt 失败日志文件
#[tauri::command]
pub fn list_prompt_fail_logs_cmd(app: tauri::AppHandle) -> IpcResult<Vec<PromptFailLogEntry>> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.map(|dir| {
        let fail_dir = dir.join("prompt_fails");
        let mut entries = Vec::new();
        if fail_dir.exists() {
            if let Ok(dir_entries) = std::fs::read_dir(&fail_dir) {
                for entry in dir_entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("log") {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        let modified = entry.metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        entries.push(PromptFailLogEntry { name, size, modified });
                    }
                }
            }
        }
        // 按修改时间倒序（最新的在前）
        entries.sort_by(|a, b| b.modified.cmp(&a.modified));
        entries
    });
    ipc_result(result)
}

/// read_prompt_fail_log_cmd：读取单个 prompt 失败日志内容
#[tauri::command]
pub fn read_prompt_fail_log_cmd(app: tauri::AppHandle, name: String) -> IpcResult<String> {
    // 安全检查：文件名不能包含路径分隔符
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return ipc_result(Err(crate::error::AppError::PlayerLoadFailed {
            detail: "无效的文件名".to_string(),
        }));
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.and_then(|dir| {
        let file_path = dir.join("prompt_fails").join(&name);
        std::fs::read_to_string(&file_path).map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("读取文件失败: {}", e),
        })
    });
    ipc_result(result)
}

/// delete_prompt_fail_log_cmd：删除单个 prompt 失败日志
#[tauri::command]
pub fn delete_prompt_fail_log_cmd(app: tauri::AppHandle, name: String) -> IpcResult<()> {
    // 安全检查：文件名不能包含路径分隔符
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return ipc_result(Err(crate::error::AppError::PlayerLoadFailed {
            detail: "无效的文件名".to_string(),
        }));
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.and_then(|dir| {
        let file_path = dir.join("prompt_fails").join(&name);
        std::fs::remove_file(&file_path).map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("删除文件失败: {}", e),
        })
    });
    ipc_result(result)
}

/// clear_prompt_fail_logs_cmd：清空 prompt 失败日志目录
#[tauri::command]
pub fn clear_prompt_fail_logs_cmd(app: tauri::AppHandle) -> IpcResult<usize> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.and_then(|dir| {
        let fail_dir = dir.join("prompt_fails");
        let mut count = 0usize;
        if let Ok(read_dir) = std::fs::read_dir(&fail_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if std::fs::remove_file(&path).is_ok() {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    });
    ipc_result(result)
}

/// prompt 失败日志条目
#[derive(serde::Serialize)]
pub struct PromptFailLogEntry {
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

/// set_dev_mode_cmd：同步开发者模式状态到后端
/// 后端在 devMode 开启时记录所有翻译 API 请求/响应
#[tauri::command]
pub fn set_dev_mode_cmd(enabled: bool) -> IpcResult<()> {
    crate::set_dev_mode(enabled);
    Ok(()).into()
}

/// set_log_api_enabled_cmd：同步"全量记录翻译数据"开关到后端
/// 仅在 devMode 开启且此开关开启时才记录 API 请求/响应
#[tauri::command]
pub fn set_log_api_enabled_cmd(enabled: bool) -> IpcResult<()> {
    crate::set_log_api_enabled(enabled);
    Ok(()).into()
}

/// get_api_debug_dir_cmd：获取 API 调试日志目录路径
#[tauri::command]
pub fn get_api_debug_dir_cmd(app: tauri::AppHandle) -> IpcResult<String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.map(|dir| {
        let debug_dir = dir.join("api_debug");
        std::fs::create_dir_all(&debug_dir).ok();
        debug_dir.to_string_lossy().to_string()
    });
    ipc_result(result)
}

/// list_api_debug_logs_cmd：列出 API 调试日志文件
#[tauri::command]
pub fn list_api_debug_logs_cmd(app: tauri::AppHandle) -> IpcResult<Vec<PromptFailLogEntry>> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.and_then(|dir| {
        let debug_dir = dir.join("api_debug");
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&debug_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("log") {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    let modified = entry.metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    entries.push(PromptFailLogEntry { name, size, modified });
                }
            }
        }
        // 按修改时间降序（最新的在前）
        entries.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(entries)
    });
    ipc_result(result)
}

/// clear_api_debug_logs_cmd：清空 API 调试日志目录
#[tauri::command]
pub fn clear_api_debug_logs_cmd(app: tauri::AppHandle) -> IpcResult<usize> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::PlayerLoadFailed {
            detail: format!("获取 app_data_dir 失败: {}", e),
        });
    let result = app_data_dir.and_then(|dir| {
        let debug_dir = dir.join("api_debug");
        let mut count = 0usize;
        if let Ok(read_dir) = std::fs::read_dir(&debug_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if std::fs::remove_file(&path).is_ok() {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    });
    ipc_result(result)
}

/// extract_player_icons_cmd：提取播放器图标（异步，已存在的跳过）
/// 在加载视频时调用，后台提取所有播放器的图标到 app_data_dir/player_icons/ 目录
#[tauri::command]
pub async fn extract_player_icons_cmd(
    video_path: String,
    app: tauri::AppHandle,
) -> Result<Vec<crate::player::PlayerIcon>, IpcError> {
    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        IpcError::new("PLAYER_ICON_FAILED", Severity::Recoverable)
    })?;
    let icons_dir = app_data_dir.join("player_icons");
    crate::player::extract_player_icons(&video_path, &icons_dir).map_err(to_ipc_err)
}

/// clear_player_icons_cache_cmd：清除播放器图标缓存
#[tauri::command]
pub async fn clear_player_icons_cache_cmd(
    app: tauri::AppHandle,
) -> Result<usize, IpcError> {
    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        IpcError::new("PLAYER_ICON_FAILED", Severity::Recoverable)
    })?;
    let icons_dir = app_data_dir.join("player_icons");
    crate::player::clear_player_icons_cache(&icons_dir).map_err(to_ipc_err)
}

// === SECTION 4 END ===

// === player_* IPC 命令（libmpv 内嵌播放） ===

use std::sync::Mutex;

/// 全局 Player 状态
static PLAYER: Mutex<Option<crate::player::Player>> = Mutex::new(None);

/// player_init：初始化 libmpv 播放器，创建子窗口嵌入 Tauri 主窗口
/// Windows 版保持同步：Win32 子窗口有线程亲和性（CreateWindowExW 必须在主线程调用），
/// 且 Windows 的 vo (d3d11) 不会 dispatch_sync 到主线程，不存在 macOS 那样的死锁问题。
///
/// **重要**：创建新 Player 前先销毁旧 Player（在当前线程）。
/// Win32 子窗口和 mpv 实例有线程亲和性，DestroyWindow 必须在创建窗口的同一线程调用。
/// 如果不先销毁旧 Player，直接 `*PLAYER = Some(new)` 会触发旧 Player 的 Drop，
/// 但 Drop 在新 Player 已创建后执行，可能导致窗口句柄冲突和 mpv 实例泄漏。
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

    // 先销毁旧 Player（在当前线程，即创建窗口的线程）
    // 防止前端并发调用 player_init 导致旧 Player 被 Drop 覆盖时 DestroyWindow 在错误线程执行
    let old_player = {
        let mut guard = PLAYER.lock().unwrap();
        guard.take()
    };
    if old_player.is_some() {
        tracing::info!("player_init: 检测到旧 Player，先销毁（当前线程）");
        drop(old_player);
        tracing::info!("player_init: 旧 Player 已销毁");
    }

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

/// player_init (macOS)：初始化 libmpv 播放器，创建 NSView 子视图嵌入 Tauri 主窗口
/// async + spawn_blocking：mpv_initialize 和 mpv_set_property_string 不能在主线程调用，
/// 因为 macOS 的 vo_cocoa 线程在 init/exit 时会 dispatch_sync 到主线程，
/// 如果主线程被 mpv API 阻塞（等待 core lock），就会与 vo 线程形成死锁。
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn player_init(
    app: tauri::AppHandle,
    window: tauri::Window,
    dll_path: String,
    x: i32, y: i32, w: i32, h: i32,
) -> Result<(), ()> {
    use objc::runtime::Object;
    tracing::info!("player_init (macOS) 开始: dylib={}, x={}, y={}, w={}, h={}", dll_path, x, y, w, h);
    let ns_window_ptr = window.ns_window().map_err(|e| {
        tracing::error!("获取 NSWindow 失败: {:?}", e);
    })?;
    let ns_window = ns_window_ptr as *mut Object;
    if ns_window.is_null() {
        tracing::error!("NSWindow 指针为 null");
        return Err(());
    }
    // raw pointer 本身是 !Send，转为 usize 以便跨线程传递
    let ns_window_addr = ns_window as usize;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let ns_window = ns_window_addr as *mut Object;
        unsafe { crate::player::Player::new(&dll_path, ns_window, app, x, y, w, h) }
    }).await;
    match result {
        Ok(Ok(player)) => {
            tracing::info!("player_init (macOS) 成功");
            *PLAYER.lock().unwrap() = Some(player);
            Ok(())
        }
        Ok(Err(e)) => {
            tracing::error!("player_init (macOS) 失败: {:?}", e);
            Err(())
        }
        Err(e) => {
            tracing::error!("player_init (macOS): spawn_blocking 任务失败: {:?}", e);
            Err(())
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
#[tauri::command]
pub fn player_init(_app: tauri::AppHandle, _window: tauri::Window, _dll_path: String, _x: i32, _y: i32, _w: i32, _h: i32) -> Result<(), ()> {
    Err(())
}

/// player_load_cmd：加载视频文件
/// async + spawn_blocking：mpv_command 不能在主线程调用，
/// 因为 macOS vo_cocoa 线程在 init 时会 dispatch_sync 到主线程，
/// 如果主线程被 mpv API 阻塞就会死锁。
#[tauri::command]
pub async fn player_load_cmd(file_path: String) -> Result<(), ()> {
    tauri::async_runtime::spawn_blocking(move || {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.load(&file_path).map_err(|_| ())
        } else {
            Err(())
        }
    }).await.map_err(|_| ())?
}

/// player_play_cmd：播放
#[tauri::command]
pub async fn player_play_cmd() -> Result<(), ()> {
    tracing::info!("播放器: 开始播放");
    tauri::async_runtime::spawn_blocking(|| {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.play().map_err(|_| ())
        } else { Err(()) }
    }).await.map_err(|_| ())?
}

/// player_pause_cmd：暂停
#[tauri::command]
pub async fn player_pause_cmd() -> Result<(), ()> {
    tracing::info!("播放器: 暂停播放");
    tauri::async_runtime::spawn_blocking(|| {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.pause().map_err(|_| ())
        } else { Err(()) }
    }).await.map_err(|_| ())?
}

/// player_seek_cmd：跳转到指定时间（秒）
#[tauri::command]
pub async fn player_seek_cmd(time_sec: f64) -> Result<(), ()> {
    tracing::info!("播放器: 跳转到 {:.1}s", time_sec);
    tauri::async_runtime::spawn_blocking(move || {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.seek(time_sec).map_err(|_| ())
        } else { Err(()) }
    }).await.map_err(|_| ())?
}

/// player_set_volume_cmd：设置音量 (0-100)
#[tauri::command]
pub async fn player_set_volume_cmd(volume: i32) -> Result<(), ()> {
    tauri::async_runtime::spawn_blocking(move || {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.set_volume(volume).map_err(|_| ())
        } else { Err(()) }
    }).await.map_err(|_| ())?
}

/// player_set_speed_cmd：设置倍速
#[tauri::command]
pub async fn player_set_speed_cmd(speed: f64) -> Result<(), ()> {
    tauri::async_runtime::spawn_blocking(move || {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.set_speed(speed).map_err(|_| ())
        } else { Err(()) }
    }).await.map_err(|_| ())?
}

/// player_set_audio_track_cmd：切换音频轨道（mpv aid，1-based）
#[tauri::command]
pub async fn player_set_audio_track_cmd(audio_id: i32) -> Result<(), ()> {
    tauri::async_runtime::spawn_blocking(move || {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.set_audio_track(audio_id).map_err(|_| ())
        } else { Err(()) }
    }).await.map_err(|_| ())?
}

/// player_get_position_cmd：获取当前播放位置和时长
#[tauri::command]
pub async fn player_get_position_cmd() -> Result<(f64, f64), ()> {
    tauri::async_runtime::spawn_blocking(|| {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            let pos = player.get_position().unwrap_or(0.0);
            let dur = player.get_duration().unwrap_or(0.0);
            Ok((pos, dur))
        } else { Err(()) }
    }).await.map_err(|_| ())?
}

/// dev_log_cmd：前端调试日志输出到 Rust tracing（临时调试用）
#[tauri::command]
pub async fn dev_log_cmd(msg: String) -> Result<(), ()> {
    tracing::info!("[Frontend] {}", msg);
    Ok(())
}

/// set_space_disabled_cmd：设置空格键是否被禁用（焦点在输入框时前端设置 true）
#[tauri::command]
pub async fn set_space_disabled_cmd(disabled: bool) -> Result<(), ()> {
    crate::player::SPACE_DISABLED.store(disabled, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// is_cursor_in_window_cmd：检查鼠标光标是否在主窗口范围内
/// 用于前端空格键播放/暂停的判断（避免鼠标在窗口外时误触发）
///
/// 平台实现：
/// - Windows: GetCursorPos（屏幕坐标原点在左上角）
/// - macOS:   NSEvent::mouseLocation（屏幕坐标原点在左下角，需翻转 y）
/// - Linux/其他: 无 libmpv 内嵌播放支持，fallback 返回 true
#[cfg(windows)]
#[tauri::command]
pub async fn is_cursor_in_window_cmd(window: tauri::Window) -> Result<bool, ()> {
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    use windows::Win32::Foundation::POINT;

    let pos = window.outer_position().map_err(|_| ())?;
    let size = window.outer_size().map_err(|_| ())?;
    unsafe {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return Ok(true); // 获取失败时 fallback 为 true
        }
        let left = pos.x;
        let top = pos.y;
        let right = pos.x + size.width as i32;
        let bottom = pos.y + size.height as i32;
        let inside = point.x >= left && point.x <= right && point.y >= top && point.y <= bottom;
        Ok(inside)
    }
}

/// is_cursor_in_window_cmd (macOS)：用 NSEvent::mouseLocation 获取全局鼠标坐标
/// macOS 屏幕坐标原点在左下角，Tauri outer_position 原点在左上角，
/// 需要用主显示器高度翻转 y 轴后再比较。
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn is_cursor_in_window_cmd(window: tauri::Window) -> Result<bool, ()> {
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::Object;

    // NSPoint / NSRect / NSSize 在 64 位 macOS 上均为 f64 字段
    #[repr(C)]
    struct NSPoint { x: f64, y: f64 }
    #[repr(C)]
    struct NSSize { width: f64, height: f64 }
    #[repr(C)]
    struct NSRect { origin: NSPoint, size: NSSize }

    let pos = window.outer_position().map_err(|_| ())?;
    let size = window.outer_size().map_err(|_| ())?;
    unsafe {
        // [NSEvent mouseLocation] 返回当前鼠标在屏幕坐标系（原点左下角）的位置
        let point: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        // 主显示器高度，用于把左下角原点的 y 翻转为左上角原点
        let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
        let frame: NSRect = msg_send![screen, frame];
        let screen_height = frame.size.height;
        let cursor_x = point.x as i32;
        let cursor_y_top = (screen_height - point.y) as i32; // 翻转为左上角原点
        let left = pos.x;
        let top = pos.y;
        let right = pos.x + size.width as i32;
        let bottom = pos.y + size.height as i32;
        let inside = cursor_x >= left && cursor_x <= right
            && cursor_y_top >= top && cursor_y_top <= bottom;
        Ok(inside)
    }
}

/// is_cursor_in_window_cmd (Linux/其他平台)：无 libmpv 内嵌播放支持，fallback true
#[cfg(not(any(windows, target_os = "macos")))]
#[tauri::command]
pub async fn is_cursor_in_window_cmd(_window: tauri::Window) -> Result<bool, ()> {
    Ok(true)
}

/// player_resize_cmd：调整子窗口位置和大小
#[tauri::command]
pub async fn player_resize_cmd(x: i32, y: i32, w: i32, h: i32) -> Result<(), ()> {
    tauri::async_runtime::spawn_blocking(move || {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.resize(x, y, w, h).map_err(|_| ())
        } else { Err(()) }
    }).await.map_err(|_| ())?
}

/// player_show_cmd：显示子窗口
#[tauri::command]
pub async fn player_show_cmd() -> Result<(), ()> {
    tracing::info!("player_show_cmd 调用");
    tauri::async_runtime::spawn_blocking(|| {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.show();
            tracing::info!("player_show_cmd 执行完成, HOOK_HIDDEN=false");
            Ok(())
        } else {
            tracing::warn!("player_show_cmd: 播放器未初始化");
            Err(())
        }
    }).await.map_err(|_| ())?
}

/// player_hide_cmd：隐藏子窗口（用于弹窗层级处理）
#[tauri::command]
pub async fn player_hide_cmd() -> Result<(), ()> {
    tracing::info!("player_hide_cmd 调用");
    tauri::async_runtime::spawn_blocking(|| {
        let guard = PLAYER.lock().unwrap();
        if let Some(ref player) = *guard {
            player.hide();
            tracing::info!("player_hide_cmd 执行完成, HOOK_HIDDEN=true");
            Ok(())
        } else {
            tracing::warn!("player_hide_cmd: 播放器未初始化");
            Err(())
        }
    }).await.map_err(|_| ())?
}

/// player_destroy_cmd：销毁播放器
/// Windows 版保持同步：DestroyWindow 必须在创建窗口的线程调用。
/// 之前用 spawn_blocking 在线程池执行 destroy，导致 DestroyWindow 始终失败（拒绝访问），
/// 子窗口泄漏。改为同步执行，确保在正确的线程销毁。
#[cfg(windows)]
#[tauri::command]
pub async fn player_destroy_cmd() -> Result<(), ()> {
    tracing::info!("player_destroy_cmd 开始");
    let player = {
        let mut guard = PLAYER.lock().unwrap();
        guard.take()
    };
    if player.is_none() {
        tracing::info!("player_destroy_cmd: 播放器未初始化，跳过");
        return Ok(());
    }
    // Windows：同步 drop（在当前线程，即创建窗口的线程）
    // mpv_terminate_destroy 在 Windows 上约 100ms 完成，不会长时间阻塞主线程
    drop(player);
    tracing::info!("player_destroy_cmd 完成");
    Ok(())
}

/// player_destroy_cmd (macOS)：销毁播放器
/// macOS 用 spawn_blocking：mpv 的 vo 线程在销毁时会 dispatch_sync 到主线程，
/// 因此 mpv_terminate_destroy 不能运行在主线程。将 Player 的 drop/destroy
/// 放到独立的 blocking 线程执行，destroy_cmd 返回的 Future 仍可供前端 await，
/// 保证 destroy 完成后再执行 player_init，同时主线程可继续处理事件循环。
#[cfg(not(windows))]
#[tauri::command]
pub async fn player_destroy_cmd() -> Result<(), ()> {
    tracing::info!("player_destroy_cmd 开始");
    let player = {
        let mut guard = PLAYER.lock().unwrap();
        guard.take()
    };
    if player.is_none() {
        tracing::info!("player_destroy_cmd: 播放器未初始化，跳过");
        return Ok(());
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        drop(player);
    }).await;
    if result.is_err() {
        tracing::error!("player_destroy_cmd: spawn_blocking 任务失败");
        return Err(());
    }
    tracing::info!("player_destroy_cmd 完成");
    Ok(())
}

// === SECTION 5 END ===

// === SECTION 6: 字幕流编辑 ===

/// edit_subtitle_streams_cmd：编辑视频内嵌字幕流（重排序、删除、改名）
/// output_path = None: 直接修改原视频（临时文件+替换）
/// output_path = Some: 输出到指定路径，不修改原文件
/// async + spawn_blocking：ffmpeg 处理大视频耗时，避免阻塞 Tauri 命令线程导致 UI 卡死
#[tauri::command]
pub async fn edit_subtitle_streams_cmd(
    video_path: String,
    streams: Vec<ffmpeg::SubtitleStreamEdit>,
    output_path: Option<String>,
    ffmpeg_path: Option<String>,
) -> Result<(), IpcError> {
    tokio::task::spawn_blocking(move || {
        ffmpeg::edit_subtitle_streams(
            &video_path,
            &streams,
            output_path.as_deref(),
            ffmpeg_path.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::FfmpegExecutionFailed { detail: e.to_string() }.to_ipc_error())?
    .map_err(|e| e.to_ipc_error())
}

// === SECTION 6 END ===

/// set_proxy：保存代理配置到 config 表
#[tauri::command]
pub fn set_proxy(
    mode: String,
    host: String,
    port: String,
    username: Option<String>,
    password: Option<String>,
    db: State<'_, Database>,
) -> IpcResult<()> {
    let _ = db.set_config("proxy_mode", &mode);
    let _ = db.set_config("proxy_host", &host);
    let _ = db.set_config("proxy_port", &port);
    let _ = db.set_config("proxy_user", &username.clone().unwrap_or_default());
    // 密码走 keyring，非敏感的 host/port/user 走 config 表
    if let Some(pw) = password {
        if !pw.is_empty() && pw != "••••••••" {
            let _ = config::CredentialStore::save("proxy", "pass", &pw);
        }
    }
    tracing::info!("代理配置已保存: mode={}, host={}, port={}", mode, host, port);
    ipc_result(Ok(()))
}

/// get_proxy：读取代理配置
#[tauri::command]
pub fn get_proxy(db: State<'_, Database>) -> IpcResult<serde_json::Value> {
    let mode = db.get_config("proxy_mode").ok().flatten().unwrap_or_else(|| "none".to_string());
    let host = db.get_config("proxy_host").ok().flatten().unwrap_or_default();
    let port = db.get_config("proxy_port").ok().flatten().unwrap_or_default();
    let user = db.get_config("proxy_user").ok().flatten().unwrap_or_default();
    let has_password = config::CredentialStore::load("proxy", "pass", "读取代理配置").is_ok();
    ipc_result(Ok(serde_json::json!({
        "mode": mode,
        "host": host,
        "port": port,
        "username": user,
        "hasPassword": has_password,
    })))
}

/// get_translate_use_proxy：读取某 provider 的"使用软件代理"开关
/// 返回三态：null=未设置（默认跟随代理），true=强制使用代理，false=强制不使用代理
#[tauri::command]
pub fn get_translate_use_proxy(provider: String, db: State<'_, Database>) -> IpcResult<Option<bool>> {
    let key = format!("translate_{}_use_proxy", provider);
    let val = db.get_config(&key).ok().flatten();
    let result = match val.as_deref() {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None, // 未设置 = 默认跟随软件代理
    };
    ipc_result(Ok(result))
}

/// set_translate_use_proxy：设置某 provider 的"使用软件代理"开关
/// value: null=清除设置（恢复默认），true/false=显式设置
#[tauri::command]
pub fn set_translate_use_proxy(provider: String, value: Option<bool>, db: State<'_, Database>) -> IpcResult<()> {
    let key = format!("translate_{}_use_proxy", provider);
    match value {
        None => { let _ = db.delete_config(&key); }
        Some(v) => { let _ = db.set_config(&key, if v { "true" } else { "false" }); }
    }
    tracing::info!("translate_use_proxy: provider={}, value={:?}", provider, value);
    ipc_result(Ok(()))
}

/// test_proxy：通过当前代理配置访问指定 URL，测试代理是否可用
/// 返回 Ok(响应耗时ms) 或错误信息
#[tauri::command]
pub async fn test_proxy(url: String, db: State<'_, Database>) -> Result<serde_json::Value, IpcError> {
    let proxy_config = ProxyConfig::load_from_db(&db);
    if proxy_config.mode == "none" || proxy_config.host.is_empty() || proxy_config.port == 0 {
        return Err(AppError::Unknown { detail: "代理未配置".to_string() }.to_ipc_error());
    }

    let client = proxy_config.build_client();
    let test_url = if url.is_empty() { "https://www.google.com".to_string() } else { url };

    let start = std::time::Instant::now();
    match client.get(&test_url).timeout(std::time::Duration::from_secs(15)).send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_millis();
            let status = resp.status().as_u16();
            tracing::info!("代理测试: url={}, status={}, elapsed={}ms", test_url, status, elapsed);
            Ok(serde_json::json!({
                "success": true,
                "status": status,
                "elapsed_ms": elapsed,
                "url": test_url,
            }))
        }
        Err(e) => {
            let elapsed = start.elapsed().as_millis();
            tracing::warn!("代理测试失败: url={}, error={}, elapsed={}ms", test_url, e, elapsed);
            Err(AppError::Unknown { detail: format!("代理连接失败: {}", e) }.to_ipc_error())
        }
    }
}

/// get_system_lang：探测系统语言，返回归一化后的 ISO 639-1 码
/// Windows: GetUserDefaultLocaleName
/// macOS/Linux: LANG / LC_ALL / LC_MESSAGES 环境变量
#[tauri::command]
pub fn get_system_lang() -> IpcResult<String> {
    let lang = detect_system_lang();
    tracing::info!("系统语言探测: {}", lang);
    ipc_result(Ok(lang))
}

/// 探测系统语言并归一化为 ISO 639-1 码
fn detect_system_lang() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::GetUserDefaultLocaleName;
        unsafe {
            let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
            let len = GetUserDefaultLocaleName(&mut buf);
            if len > 0 {
                let locale = String::from_utf16_lossy(&buf[..len as usize]);
                return normalize_locale(&locale);
            }
        }
        "zh".to_string()
    }
    #[cfg(not(windows))]
    {
        // macOS/Linux：从 LANG / LC_ALL 环境变量探测
        for var in &["LANG", "LC_ALL", "LC_MESSAGES"] {
            if let Ok(val) = std::env::var(var) {
                if !val.is_empty() {
                    return normalize_locale(&val);
                }
            }
        }
        "zh".to_string()
    }
}

/// 将 locale 标识符归一化为 ISO 639-1 两字母码
/// zh-CN/zh-Hans → zh, zh-TW/zh-Hant → zh, en-US/en_US.UTF-8 → en, ja-JP → ja, ko-KR → ko
/// 兼容 Windows（GetUserDefaultLocaleName 返回 "en-US" 连字符格式）
/// 和 macOS/Linux（LANG 环境变量 "en_US.UTF-8" 下划线格式）
fn normalize_locale(locale: &str) -> String {
    let lower = locale.to_lowercase();
    // 同时按 '-' 和 '_' 分割，取第一段作为主语言码
    // 兼容 "en-US"、"en_US.UTF-8"、"zh_Hans_CN" 等格式
    let lang = lower
        .split(|c| c == '-' || c == '_')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("zh");
    // 归一化映射：仅取主语言码（一期不区分简繁）
    match lang {
        "zh" | "en" | "ja" | "ko" | "fr" | "de" | "es" | "ru" | "it" | "pt" | "th" | "vi" | "ar" => lang.to_string(),
        _ => "zh".to_string(), // fallback 中文
    }
}

/// toggle_devtools：打开/关闭 WebView2 DevTools（开发者模式用）
#[tauri::command]
pub fn toggle_devtools(app: tauri::AppHandle, open: bool) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        if open {
            window.open_devtools();
            tracing::info!("DevTools 已打开");
        } else {
            window.close_devtools();
            tracing::info!("DevTools 已关闭");
        }
    }
}

/// get_work_area：获取当前窗口所在显示器的工作区（排除任务栏），物理像素
/// 返回 { x, y, width, height }，前端用于约束窗口位置不超出可见区域
#[tauri::command]
pub fn get_work_area(app: tauri::AppHandle) -> IpcResult<serde_json::Value> {
    #[cfg(windows)]
    {
        use tauri::Manager;
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITOR_DEFAULTTONEAREST, MONITORINFO,
        };

        let hwnd = if let Some(window) = app.get_webview_window("main") {
            // 获取窗口 HWND
            match window.hwnd() {
                Ok(h) => h,
                Err(_) => {
                    return ipc_result(Err(crate::error::AppError::Unknown {
                        detail: "无法获取窗口句柄".to_string(),
                    }))
                }
            }
        } else {
            // fallback：前台窗口
            unsafe { GetForegroundWindow() }
        };

        unsafe {
            let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmonitor, &mut mi).as_bool() {
                let rc = mi.rcWork;
                tracing::info!(
                    "工作区: x={}, y={}, w={}, h={}",
                    rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top
                );
                return ipc_result(Ok(serde_json::json!({
                    "x": rc.left,
                    "y": rc.top,
                    "width": rc.right - rc.left,
                    "height": rc.bottom - rc.top,
                })));
            }
        }
        ipc_result(Err(crate::error::AppError::Unknown {
            detail: "GetMonitorInfoW 失败".to_string(),
        }))
    }
    #[cfg(not(windows))]
    {
        // macOS/Linux：用 Tauri 跨平台 API 获取主显示器尺寸
        use tauri::Manager;
        let _ = app;
        if let Some(window) = app.get_webview_window("main") {
            if let Ok(monitors) = window.available_monitors() {
                if let Some(monitor) = monitors.first() {
                    let pos = monitor.position();
                    let size = monitor.size();
                    return ipc_result(Ok(serde_json::json!({
                        "x": pos.x,
                        "y": pos.y,
                        "width": size.width,
                        "height": size.height,
                    })));
                }
            }
        }
        // fallback
        ipc_result(Ok(serde_json::json!({
            "x": 0, "y": 0, "width": 1920, "height": 1080,
        })))
    }
}

// === SECTION 7: 自动更新 ===

/// 更新信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub version: String,
    pub notes: String,
    pub pub_date: String,
}

/// 根据配置的更新通道构建 Updater
/// stable → latest.json，nightly → nightly.json
fn build_updater_by_channel(
    app: &tauri::AppHandle,
    db: &Database,
) -> Result<tauri_plugin_updater::Updater, IpcError> {
    use tauri_plugin_updater::UpdaterExt;
    let channel = db
        .get_config("update_channel")
        .ok()
        .flatten()
        .unwrap_or_else(|| "stable".to_string());

    if channel == "nightly" {
        tracing::info!("更新通道: nightly，使用 nightly.json");
        // nightly 通道：覆盖 endpoint 指向 nightly.json
        let endpoint = "https://terry2010.github.io/polly-subtitle-translator/nightly.json";
        // 自定义版本比较器：nightly 版本号格式为 1.0.1-nightly.20260706.143025
        // 客户端版本是基础版本号（如 1.0.1），nightly.json 的 version 含时间戳
        // 只要远程版本号和当前版本不同就认为有更新（因为 nightly 每次构建版本号都不同）
        let builder = app
            .updater_builder()
            .endpoints(vec![url::Url::parse(endpoint).map_err(|e| {
                IpcError::new("update.check_failed", Severity::Recoverable)
                    .with_args(serde_json::json!({ "detail": e.to_string() }))
            })?])
            .map_err(|e| {
                IpcError::new("update.check_failed", Severity::Recoverable)
                    .with_args(serde_json::json!({ "detail": e.to_string() }))
            })?
            .version_comparator(|current, remote| {
                // nightly 通道：远程版本和当前版本不同就有更新
                // 当前版本是基础版本号(如 1.0.1)，远程是 1.0.1-nightly.时间戳
                let remote_str = remote.version.to_string();
                let current_str = current.to_string();
                tracing::info!(
                    "nightly 版本比较: current={} remote={} -> {}",
                    current_str,
                    remote_str,
                    remote_str != current_str
                );
                remote_str != current_str
            });
        builder.build().map_err(|e| {
            IpcError::new("update.check_failed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": e.to_string() }))
        })
    } else {
        // stable 通道：用 tauri.conf.json 配置的默认 endpoint（latest.json）
        app.updater().map_err(|e| {
            IpcError::new("update.check_failed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": e.to_string() }))
        })
    }
}

/// check_for_update：检查是否有新版本
#[tauri::command]
pub async fn check_for_update(
    app: tauri::AppHandle,
    db: State<'_, Database>,
) -> Result<UpdateInfo, IpcError> {
    let updater = build_updater_by_channel(&app, &db)?;
    match updater.check().await {
        Ok(Some(update)) => {
            tracing::info!("发现新版本: {}", update.version);
            let pub_date = update.date.map(|d| d.to_string()).unwrap_or_default();
            Ok(UpdateInfo {
                available: true,
                version: update.version.clone(),
                notes: update.body.clone().unwrap_or_default(),
                pub_date,
            })
        }
        Ok(None) => {
            tracing::info!("当前已是最新版本");
            Ok(UpdateInfo {
                available: false,
                version: String::new(),
                notes: String::new(),
                pub_date: String::new(),
            })
        }
        Err(e) => {
            tracing::warn!("检查更新失败: {}", e);
            Err(IpcError::new("update.check_failed", Severity::Recoverable).with_args(serde_json::json!({ "detail": e.to_string() })))
        }
    }
}

/// download_and_install_update：下载并安装更新
/// 通过 emit "update_download_progress" 事件推送进度
#[tauri::command]
pub async fn download_and_install_update(
    app: tauri::AppHandle,
    db: State<'_, Database>,
) -> Result<(), IpcError> {
    let updater = build_updater_by_channel(&app, &db)?;
    let update = updater.check().await
        .map_err(|e| IpcError::new("update.check_failed", Severity::Recoverable).with_args(serde_json::json!({ "detail": e.to_string() })))?
        .ok_or_else(|| IpcError::new("update.no_update", Severity::Recoverable))?;

    let _ = app.emit("update_download_progress", serde_json::json!({
        "stage": "downloading", "progress": 0, "message": "开始下载更新..."
    }));

    let app_handle = app.clone();
    let mut downloaded: u64 = 0;
    let mut total_size: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let download_start = std::time::Instant::now();

    let result = update.download_and_install(
        |chunk_len, content_length| {
            downloaded += chunk_len as u64;
            if total_size == 0 {
                if let Some(cl) = content_length { total_size = cl; }
            }
            if last_emit.elapsed() > std::time::Duration::from_millis(200) {
                let pct = if total_size > 0 { (downloaded * 100 / total_size) as u8 } else { 0 };
                let elapsed = download_start.elapsed().as_secs_f64();
                let speed_bps = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
                let speed_mb = speed_bps / 1024.0 / 1024.0;
                let remaining_bytes = total_size.saturating_sub(downloaded);
                let eta_secs = if speed_bps > 0.0 { remaining_bytes as f64 / speed_bps } else { 0.0 };
                let _ = app_handle.emit("update_download_progress", serde_json::json!({
                    "stage": "downloading", "progress": pct,
                    "speed_mbps": (speed_mb * 10.0).round() / 10.0,
                    "eta_secs": eta_secs.round() as u64,
                    "message": format!("下载中 {}% ({} / {} MB)",
                        pct, downloaded / 1024 / 1024, total_size / 1024 / 1024)
                }));
                last_emit = std::time::Instant::now();
            }
        },
        || {
            let _ = app_handle.emit("update_download_progress", serde_json::json!({
                "stage": "done", "progress": 100, "message": "下载完成，正在安装..."
            }));
        },
    ).await;

    match result {
        Ok(_) => {
            tracing::info!("更新下载安装完成，即将重启");
            Ok(())
        }
        Err(e) => {
            tracing::error!("更新下载失败: {}", e);
            let _ = app.emit("update_download_progress", serde_json::json!({
                "stage": "failed", "progress": 0, "message": e.to_string()
            }));
            Err(IpcError::new("update.download_failed", Severity::Recoverable).with_args(serde_json::json!({ "detail": e.to_string() })))
        }
    }
}

// === SECTION 7 END ===

// === SECTION 8: 跨平台单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;

    /// normalize_locale：常见 locale 应归一化为 ISO 639-1 两字母码
    #[test]
    fn normalize_locale_common_locales() {
        assert_eq!(normalize_locale("zh-CN"), "zh");
        assert_eq!(normalize_locale("zh-Hans"), "zh");
        assert_eq!(normalize_locale("zh-TW"), "zh");
        assert_eq!(normalize_locale("zh_Hans_CN"), "zh");
        assert_eq!(normalize_locale("en-US"), "en");
        assert_eq!(normalize_locale("en_US.UTF-8"), "en");
        assert_eq!(normalize_locale("ja-JP"), "ja");
        assert_eq!(normalize_locale("ko-KR"), "ko");
        assert_eq!(normalize_locale("fr_FR"), "fr");
        assert_eq!(normalize_locale("de_DE"), "de");
        assert_eq!(normalize_locale("es-ES"), "es");
    }

    /// normalize_locale：大小写不敏感
    #[test]
    fn normalize_locale_case_insensitive() {
        assert_eq!(normalize_locale("EN-us"), "en");
        assert_eq!(normalize_locale("JA"), "ja");
        assert_eq!(normalize_locale("Ko-KR"), "ko");
    }

    /// normalize_locale：未知语言 fallback 为 zh
    #[test]
    fn normalize_locale_unknown_fallback_zh() {
        assert_eq!(normalize_locale("xx-YY"), "zh");
        assert_eq!(normalize_locale("foobar"), "zh");
        assert_eq!(normalize_locale(""), "zh");
    }

    /// normalize_locale：只取主语言码，忽略地区后缀
    #[test]
    fn normalize_locale_takes_primary_tag() {
        assert_eq!(normalize_locale("pt-BR"), "pt");
        assert_eq!(normalize_locale("vi-VN"), "vi");
        assert_eq!(normalize_locale("th"), "th");
        assert_eq!(normalize_locale("ar-SA"), "ar");
    }

    /// detect_system_lang：应返回归一化后的合法语言码（跨平台）
    /// 不依赖具体系统语言，只校验返回值在已知集合内
    #[test]
    fn detect_system_lang_returns_valid_code() {
        let lang = detect_system_lang();
        let valid = ["zh", "en", "ja", "ko", "fr", "de", "es", "ru", "it", "pt", "th", "vi", "ar"];
        assert!(
            valid.contains(&lang.as_str()),
            "detect_system_lang 返回了非预期值: {}",
            lang
        );
    }
}

