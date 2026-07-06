// 翻译执行器：调用 9b 模型翻译字幕
use zimufan_lib::subtitle::{SubtitleFile, SubtitleEntry};
use zimufan_lib::translate::{
    self, OpenAiProvider, TranslateProviderTrait, TranslateScheduler,
    ModelType, ExtractedName,
};
use zimufan_lib::db::Database;
use std::path::Path;
use std::sync::Arc;

/// 翻译结果
pub struct TranslationOutput {
    pub file: SubtitleFile,
    pub failed_count: usize,
    pub cached_count: usize,
    pub total_tokens: u64,
    pub names: Vec<ExtractedName>,
}

/// 创建临时数据库（用于翻译缓存）
pub fn create_test_db(cache_dir: &Path) -> Database {
    let db_path = cache_dir.join("test_cache.db");
    let db = Database::open(&db_path).expect("打开测试数据库失败");
    db.migrate().expect("数据库迁移失败");
    db
}

// === SECTION 1 END ===

/// 构建人名提取 system prompt（复用 translate.rs 的逻辑）
fn build_name_extraction_system_prompt(source_lang: &str, target_lang: &str) -> String {
    let src = translate::lang_full_name(source_lang);
    let tgt = translate::lang_full_name(target_lang);
    format!(
        "Extract proper nouns from {src} subtitles and translate each to {tgt}.\n\
         ONLY extract: person names, place/farm/field names, brand/product names, movie/TV/song/band titles, named animals, bird species.\n\
         Do NOT extract: crops, generic animals, colors, months, seasons, units, weather, numbers, dates, adjectives, verbs, common nouns, farm terms.\n\
         If unsure, do NOT include it.\n\
         You MUST translate every name to {tgt}. Never output English as the translation.\n\n\
         Output a JSON array. Each element is {{\"en\": \"EnglishName\", \"zh\": \"{tgt}Translation\"}}.\n\
         For brand names (all-caps or containing numbers), zh = \"EnglishName（中文翻译）\".\n\n\
         Example:\n\
         [\n\
           {{\"en\": \"Jeremy\", \"zh\": \"杰里米\"}},\n\
           {{\"en\": \"Endgame\", \"zh\": \"终结者\"}},\n\
           {{\"en\": \"Skylark\", \"zh\": \"云雀\"}},\n\
           {{\"en\": \"GS4\", \"zh\": \"GS4（农业系统4）\"}},\n\
           {{\"en\": \"Countryfile\", \"zh\": \"乡村档案\"}}\n\
         ]\n\n\
         Output ONLY the JSON array. No text before or after. No explanations.",
        src = src, tgt = tgt
    )
}

/// 构建人名提取 user prompt
fn build_name_extraction_user_prompt(texts: &[String]) -> String {
    texts
        .iter()
        .enumerate()
        .map(|(i, txt)| format!("{}. {}", i + 1, txt))
        .collect::<Vec<_>>()
        .join("\n")
}

// === SECTION 2 END ===

/// 人名预扫描（简化版：直接用 extract_names_raw）
pub async fn extract_names(
    file: &SubtitleFile,
    cfg: &super::config::TestConfig,
    source_lang: &str,
    target_lang: &str,
) -> Vec<ExtractedName> {
    let model_type = ModelType::from_model_id(&cfg.model_9b);
    let provider = OpenAiProvider::with_client(
        cfg.api_base.clone(),
        cfg.model_9b.clone(),
        model_type,
        None,
        reqwest::Client::new(),
    )
    .with_service_name("LM Studio".to_string());

    // 分段提取（每段最多 150 条）
    const MAX_LINES_PER_SEGMENT: usize = 150;
    let texts: Vec<String> = file.entries.iter().map(|e| e.text.clone()).collect();
    let mut all_names = Vec::new();

    for chunk in texts.chunks(MAX_LINES_PER_SEGMENT) {
        let system_prompt = build_name_extraction_system_prompt(source_lang, target_lang);
        let user_prompt = build_name_extraction_user_prompt(chunk);

        match provider.extract_names_raw(&system_prompt, &user_prompt).await {
            Ok(response) => {
                let names = translate::parse_name_extraction_response(&response);
                eprintln!("  [人名精译] 本段提取 {} 个人名", names.len());
                all_names.extend(names);
            }
            Err(e) => {
                eprintln!("  [人名精译] 本段提取失败: {:?}", e);
            }
        }
    }

    // 去重（同名取第一个）
    let mut seen = std::collections::HashSet::new();
    all_names.retain(|n| seen.insert(n.english.to_lowercase()));
    eprintln!("  [人名精译] 合并后 {} 个人名", all_names.len());
    all_names
}

// === SECTION 3 END ===

/// 用 9b 模型翻译字幕
pub async fn translate_with_9b(
    file: &SubtitleFile,
    cfg: &super::config::TestConfig,
    db: &Database,
    name_precision: bool,
    source_lang: &str,
    target_lang: &str,
) -> TranslationOutput {
    let model_type = ModelType::from_model_id(&cfg.model_9b);

    // 人名预扫描
    let names = if name_precision {
        eprintln!("  [人名精译] 预扫描提取人名...");
        extract_names(file, cfg, source_lang, target_lang).await
    } else {
        Vec::new()
    };

    let glossary: Vec<(String, String)> = names.iter()
        .map(|n| (n.english.clone(), n.chinese.clone()))
        .collect();

    // 创建带 glossary 和 name_tagging 的 provider
    let provider = OpenAiProvider::with_client(
        cfg.api_base.clone(),
        cfg.model_9b.clone(),
        model_type,
        None,
        reqwest::Client::new(),
    )
    .with_service_name("LM Studio".to_string())
    .with_glossary(glossary)
    .with_name_tagging(name_precision);

    let provider: Arc<dyn TranslateProviderTrait + Send + Sync> = Arc::new(provider);
    let provider_name = format!("openai-lmstudio-{}", cfg.model_9b);
    eprintln!("  [翻译] provider_name={}, file_hash={:?}", provider_name, file.file_hash);

    // 创建 scheduler
    let scheduler = TranslateScheduler::new(db, provider, provider_name)
        .with_file_hash(file.file_hash.clone());

    // 执行翻译
    eprintln!("  [翻译] 开始翻译 {} 条字幕...", file.entries.len());
    let max_single_length = 500;
    let result = scheduler
        .translate_entries(
            &file.entries,
            source_lang,
            target_lang,
            max_single_length,
        )
        .await
        .expect("翻译失败");

    // 构造翻译后的 SubtitleFile
    let mut translated_file = file.clone();
    let mut failed_count = 0;
    let mut matched = 0;
    for te in &result.translations {
        if let Some(entry) = translated_file.entries.iter_mut().find(|e| e.index == te.index) {
            entry.translated = te.translated.clone();
            entry.failed = te.failed;
            matched += 1;
            if te.failed {
                failed_count += 1;
            }
        }
    }
    eprintln!("  [翻译] 回填: {} 条翻译结果, {} 条匹配, {} 条失败", result.translations.len(), matched, failed_count);
    if !result.translations.is_empty() {
        let sample = &result.translations[0];
        eprintln!("  [翻译] 样本: index={}, translated='{}', failed={}", sample.index, truncate_chars(&sample.translated, 50), sample.failed);
    }

    let total_tokens = result.token_usage.as_ref().map(|t| t.total_tokens).unwrap_or(0);
    eprintln!("  [翻译] 完成: {} 条, 失败 {} 条, token: {}", result.translations.len(), failed_count, total_tokens);

    TranslationOutput {
        file: translated_file,
        failed_count,
        cached_count: result.cached_count,
        total_tokens,
        names,
    }
}

// === SECTION 4 END ===

/// 批次翻译+验证：每翻完一批就立即检查该批，输出问题条目
/// 返回完整的翻译结果和每批的检查报告
pub async fn translate_with_9b_batched(
    file: &SubtitleFile,
    cfg: &super::config::TestConfig,
    db: &Database,
    name_precision: bool,
    source_lang: &str,
    target_lang: &str,
) -> TranslationOutput {
    let model_type = ModelType::from_model_id(&cfg.model_9b);

    // 人名预扫描
    let names = if name_precision {
        eprintln!("  [人名精译] 预扫描提取人名...");
        extract_names(file, cfg, source_lang, target_lang).await
    } else {
        Vec::new()
    };

    let glossary: Vec<(String, String)> = names.iter()
        .map(|n| (n.english.clone(), n.chinese.clone()))
        .collect();

    let provider = OpenAiProvider::with_client(
        cfg.api_base.clone(),
        cfg.model_9b.clone(),
        model_type,
        None,
        reqwest::Client::new(),
    )
    .with_service_name("LM Studio".to_string())
    .with_glossary(glossary)
    .with_name_tagging(name_precision);

    let provider: Arc<dyn TranslateProviderTrait + Send + Sync> = Arc::new(provider);
    let provider_name = format!("openai-lmstudio-{}", cfg.model_9b);

    let scheduler = TranslateScheduler::new(db, provider, provider_name)
        .with_file_hash(file.file_hash.clone());

    // 按批次翻译
    const BATCH_SIZE: usize = 30;
    let total = file.entries.len();
    let mut translated_file = file.clone();
    let mut total_failed = 0usize;
    let mut total_cached = 0usize;
    let mut total_tokens = 0u64;
    let mut batch_num = 0usize;

    eprintln!("  [批次翻译] 共 {} 条，每批 {} 条", total, BATCH_SIZE);

    for start in (0..total).step_by(BATCH_SIZE) {
        let end = (start + BATCH_SIZE).min(total);
        batch_num += 1;
        let batch_entries = &file.entries[start..end];

        eprintln!("\n  --- 批次 {batch_num}: #{start}..#{end} ({len} 条) ---",
            batch_num = batch_num, start = start, end = end - 1, len = batch_entries.len());

        let result = scheduler
            .translate_entries(batch_entries, source_lang, target_lang, 500)
            .await
            .expect("翻译失败");

        // 回填
        let mut batch_failed = 0;
        let mut batch_cached = 0;
        for te in &result.translations {
            if let Some(entry) = translated_file.entries.iter_mut().find(|e| e.index == te.index) {
                entry.translated = te.translated.clone();
                entry.failed = te.failed;
                if te.from_cache { batch_cached += 1; }
                if te.failed { batch_failed += 1; }
            }
        }

        let batch_tokens = result.token_usage.as_ref().map(|t| t.total_tokens).unwrap_or(0);
        total_failed += batch_failed;
        total_cached += batch_cached;
        total_tokens += batch_tokens;

        eprintln!("  [批次 {batch_num}] 翻译完成: {ok} 成功, {fail} 失败, {cache} 缓存, {tok} token",
            batch_num = batch_num,
            ok = result.translations.len() - batch_failed,
            fail = batch_failed,
            cache = batch_cached,
            tok = batch_tokens);

        // 立即对该批运行 L1/L2 检查
        let batch_orig = extract_batch_subfile(file, start, end);
        let batch_trans = extract_batch_subfile(&translated_file, start, end);
        verify_batch(&batch_orig, &batch_trans, target_lang, batch_num);

        // 打印该批的翻译样本（前3条）
        for (i, te) in result.translations.iter().take(3).enumerate() {
            let orig_text = &file.entries[start + i].text;
            let trans_text = &te.translated;
            eprintln!("    #{}: {} → {}{}",
                te.index,
                truncate_chars(orig_text, 30).replace('\n', " "),
                truncate_chars(trans_text, 30).replace('\n', " "),
                if te.failed { " [FAILED]" } else { "" });
        }
    }

    eprintln!("\n  [批次翻译] 全部完成: {} 条, 失败 {} 条, 缓存 {} 条, token {}",
        total, total_failed, total_cached, total_tokens);

    TranslationOutput {
        file: translated_file,
        failed_count: total_failed,
        cached_count: total_cached,
        total_tokens,
        names,
    }
}

/// 从 SubtitleFile 中提取指定范围的条目，构造一个新的 SubtitleFile
fn extract_batch_subfile(file: &SubtitleFile, start: usize, end: usize) -> SubtitleFile {
    let mut sub = file.clone();
    sub.entries = file.entries[start..end].to_vec();
    sub
}

/// 对单个批次运行 L1/L2 检查并输出结果
fn verify_batch(orig: &SubtitleFile, trans: &SubtitleFile, target_lang: &str, batch_num: usize) {
    use super::checks_l1;
    use super::checks_l2;

    let mut issues = Vec::new();

    // L1 翻译后检查
    for cr in checks_l1::run_l1_checks_translated(orig, trans) {
        if cr.status != checks_l1::CheckStatus::Pass {
            issues.push(format!("L1 {}: {}", cr.name, cr.detail));
        }
    }

    // L2 检查
    for cr in checks_l2::run_l2_checks(orig, trans, target_lang) {
        if cr.status != checks_l1::CheckStatus::Pass {
            issues.push(format!("L2 {}: {}", cr.name, cr.detail));
        }
    }

    if issues.is_empty() {
        eprintln!("  [批次 {batch_num} 验证] ✅ 全部通过");
    } else {
        eprintln!("  [批次 {batch_num} 验证] ⚠️ 发现 {} 个问题:", issues.len());
        for issue in &issues {
            eprintln!("    - {issue}");
        }
    }
}

// === SECTION 5 END ===

/// 按字符数安全截取字符串（避免切到 UTF-8 字符中间）
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect::<String>()
}

// === SECTION 6 END ===

// ===== 状态机模式：可恢复的批次翻译+验证 =====

use super::state::{TestState, BatchStatus, TranslationRecord, NameRecord};

/// 阶段 0：专有名词预扫描
pub async fn stage_name_scan(
    state: &mut TestState,
    file: &SubtitleFile,
    cfg: &super::config::TestConfig,
) {
    if state.names_status != BatchStatus::Pending {
        eprintln!("  [阶段0] 名词预扫描已完成，跳过（{} 个名词）", state.names.len());
        return;
    }

    eprintln!("\n  === 阶段 0：专有名词预扫描 ===");
    let names = extract_names(file, cfg, &state.source_lang, &state.target_lang).await;

    state.names = names.iter().map(|n| NameRecord {
        english: n.english.clone(),
        chinese: n.chinese.clone(),
    }).collect();
    state.names_status = BatchStatus::Passed;
    eprintln!("  [阶段0] 完成：提取 {} 个名词", state.names.len());

    state.save().expect("保存状态失败");
}

/// 阶段 N：翻译第 N 批 + 27b 验证
/// 返回 true 表示该批通过，false 表示发现 bug 需修复
///
/// 恢复逻辑：
/// - Passed：跳过（已通过，不重跑）
/// - 其他状态（Pending/BugFound/Translated/Judged）：重新翻译 + 验证
///   （9b 模型不稳定，重跑时必须重新翻译以获得最新结果）
pub async fn stage_translate_batch(
    state: &mut TestState,
    file: &SubtitleFile,
    cfg: &super::config::TestConfig,
    db: &Database,
) -> bool {
    let batch_idx = state.current_stage - 1;

    // 先拷贝需要的值，避免借用冲突
    let (batch_num, batch_start, batch_end, batch_status) = {
        let b = &state.batches[batch_idx];
        (b.batch_num, b.start, b.end, b.status.clone())
    };

    if batch_status == BatchStatus::Passed {
        eprintln!("  [阶段{}] 批次 {} 已通过，跳过", state.current_stage, batch_num);
        return true;
    }

    eprintln!("\n  === 阶段 {}：批次 {} ({}-{}) 状态={:?} ===",
        state.current_stage, batch_num, batch_start, batch_end - 1, batch_status);

    // 非 Passed 状态：重新翻译 + 验证（9b 模型不稳定，重跑时必须重新翻译）
    eprintln!("  [上下文] 已有 {} 条翻译作为上下文", state.context_translations(batch_idx).len());

    let glossary: Vec<(String, String)> = state.names.iter()
        .map(|n| (n.english.clone(), n.chinese.clone()))
        .collect();

    let model_type = ModelType::from_model_id(&cfg.model_9b);
    let provider = OpenAiProvider::with_client(
        cfg.api_base.clone(),
        cfg.model_9b.clone(),
        model_type,
        None,
        reqwest::Client::new(),
    )
    .with_service_name("LM Studio".to_string())
    .with_glossary(glossary)
    .with_name_tagging(state.name_precision);

    let provider: Arc<dyn TranslateProviderTrait + Send + Sync> = Arc::new(provider);
    let provider_name = format!("openai-lmstudio-{}", cfg.model_9b);
    let scheduler = TranslateScheduler::new(db, provider, provider_name)
        .with_file_hash(file.file_hash.clone());

    let batch_entries = &file.entries[batch_start..batch_end];
    eprintln!("  [9b翻译] 开始翻译 {} 条...", batch_entries.len());

    let result = scheduler
        .translate_entries(batch_entries, &state.source_lang, &state.target_lang, 500)
        .await
        .expect("翻译失败");

    let batch_tokens = result.token_usage.as_ref().map(|t| t.total_tokens).unwrap_or(0);
    let batch_cached = result.cached_count;
    let batch_failed = result.translations.iter().filter(|t| t.failed).count();

    state.total_tokens += batch_tokens;
    state.total_cached += batch_cached;
    state.total_failed += batch_failed;

    let translations: Vec<TranslationRecord> = result.translations.iter().map(|te| {
        TranslationRecord {
            index: te.index,
            original: te.original.clone(),
            translated: te.translated.clone(),
            failed: te.failed,
        }
    }).collect();

    // 打印翻译样本
    for te in result.translations.iter().take(5) {
        eprintln!("    #{}: {} → {}{}",
            te.index,
            truncate_chars(&te.original, 40).replace('\n', " "),
            truncate_chars(&te.translated, 40).replace('\n', " "),
            if te.failed { " [FAILED]" } else { "" });
    }

    eprintln!("  [9b翻译] 完成: {} 成功, {} 失败, {} 缓存, {} token",
        result.translations.len() - batch_failed, batch_failed, batch_cached, batch_tokens);

    // 更新批次状态
    {
        let batch_state = &mut state.batches[batch_idx];
        batch_state.translations = translations.clone();
        batch_state.status = BatchStatus::Translated;
    }

    state.save().expect("保存状态失败");

    // 27b 验证
    eprintln!("  [27b验证] 开始验证批次 {}...", batch_num);
    let mut translated_file = file.clone();
    for tr in &translations {
        if let Some(entry) = translated_file.entries.iter_mut().find(|e| e.index == tr.index) {
            entry.translated = tr.translated.clone();
            entry.failed = tr.failed;
        }
    }

    let judge_result = super::judge::judge_batch(
        file,
        &translated_file,
        cfg,
        batch_start,
        batch_end,
    ).await;

    let pass_count = judge_result.results.iter().filter(|r| r.verdict == "pass").count();
    let fail_count = judge_result.results.iter().filter(|r| r.verdict == "fail").count();
    let shift_count = judge_result.results.iter().filter(|r| r.verdict == "shift").count();

    eprintln!("  [27b验证] 结果: {} pass, {} fail, {} shift", pass_count, fail_count, shift_count);

    // 保存验证记录
    let judge_records: Vec<super::state::JudgeRecord> = judge_result.results.iter().map(|r| {
        super::state::JudgeRecord {
            index: r.index,
            verdict: r.verdict.clone(),
            reason: r.reason.clone(),
            suggestion: r.suggestion.clone(),
        }
    }).collect();

    // 打印失败条目详情
    if fail_count > 0 || shift_count > 0 {
        eprintln!("  [27b验证] ⚠️ 发现问题条目:");
        for r in &judge_records {
            if r.verdict != "pass" {
                let orig = file.entries.iter().find(|e| e.index == r.index);
                let trans = translations.iter().find(|t| t.index == r.index);
                if let (Some(o), Some(t)) = (orig, trans) {
                    eprintln!("    #{} [{}]: {} → {} | reason: {}",
                        r.index, r.verdict,
                        truncate_chars(&o.text, 40).replace('\n', " "),
                        truncate_chars(&t.translated, 40).replace('\n', " "),
                        r.reason.as_deref().unwrap_or("?"));
                }
            }
        }
        // 更新批次状态
        {
            let batch_state = &mut state.batches[batch_idx];
            batch_state.judge_results = judge_records;
            batch_state.judge_pass = pass_count;
            batch_state.judge_fail = fail_count;
            batch_state.judge_shift = shift_count;
            batch_state.status = BatchStatus::BugFound;
        }
        state.save().expect("保存状态失败");
        eprintln!("  [状态] 批次 {} 标记为 BugFound，修复后重跑继续", batch_num);
        return false;
    }

    // 0 结果时判为失败（27b 验证未生效）
    let total_judged = pass_count + fail_count + shift_count;
    if total_judged == 0 {
        eprintln!("  [27b验证] ⚠️ 27b 返回 0 条判定结果，验证未生效");
        {
            let batch_state = &mut state.batches[batch_idx];
            batch_state.judge_results = judge_records;
            batch_state.judge_pass = pass_count;
            batch_state.judge_fail = fail_count;
            batch_state.judge_shift = shift_count;
            batch_state.status = BatchStatus::BugFound;
        }
        state.save().expect("保存状态失败");
        eprintln!("  [状态] 批次 {} 标记为 BugFound（27b 验证未生效），重跑会重试", batch_num);
        return false;
    }

    // 更新批次状态
    {
        let batch_state = &mut state.batches[batch_idx];
        batch_state.judge_results = judge_records;
        batch_state.judge_pass = pass_count;
        batch_state.judge_fail = fail_count;
        batch_state.judge_shift = shift_count;
        batch_state.status = BatchStatus::Passed;
    }
    state.save().expect("保存状态失败");
    eprintln!("  [27b验证] ✅ 批次 {} 通过", batch_num);
    true
}

// === SECTION 6 END ===