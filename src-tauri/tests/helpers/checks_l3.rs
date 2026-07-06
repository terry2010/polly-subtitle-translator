// L3 持久化往返验证：双语字幕保存/加载、缓存恢复、跨格式一致性
// 对应用户手工流程：
//   翻译完毕 → 保存双语字幕 → 加载双语字幕 → 验证失败数一致
//   打开原始字幕 → 从缓存恢复 → 验证失败数一致
//   SRT/ASS/VTT 三种格式各执行一次
use zimufan_lib::subtitle::{
    self, detect_bilingual, split_bilingual, AssBilingualStyle, ExportMode, ExportOptions,
    SubtitleEntry, SubtitleFile, SubtitleFormat,
};
use super::checks_l1::CheckResult;

/// CJK 字符检测（与前端 hasCjk 一致）
fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        let code = c as u32;
        (0x4E00..=0x9FFF).contains(&code)
    })
}

/// 音效标记检测（与前端 looksLikeSoundEffect 一致）
fn looks_like_sound_effect(s: &str) -> bool {
    // 先去掉 ASS 定位/样式标签（如 {\an8}），与 translate.rs 的实现一致
    // 否则含 {\an8} 前缀的音效标记（如 {\an8}[phone buzzing]）会被误判为非音效标记，
    // 导致翻译时 is_untranslated 与导出往返后 is_untranslated 不一致
    let stripped = strip_ass_tags(s);
    let s = stripped.trim();
    if s.is_empty() {
        return false;
    }
    if s.starts_with('[') && s.ends_with(']') {
        return true;
    }
    // 去掉 [Name] 前缀后，剩余部分仍被 [] 包裹
    let re = regex::Regex::new(r"^\s*\[[^\]]+\]\s*(.*)$").unwrap();
    if let Some(caps) = re.captures(s) {
        let rest = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if !rest.is_empty() && rest.starts_with('[') && rest.ends_with(']') {
            return true;
        }
    }
    false
}

/// 去掉 ASS 覆盖标签（{...} 包裹的部分，如 {\an8}、{\b1}、{\pos(x,y)} 等）
fn strip_ass_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_brace = false;
    for c in s.chars() {
        match c {
            '{' => in_brace = true,
            '}' => in_brace = false,
            _ if !in_brace => result.push(c),
            _ => {}
        }
    }
    result
}

/// "未翻译"判定（与前端 isUntranslated 一致）
fn is_untranslated(e: &SubtitleEntry, target_lang: &str) -> bool {
    e.translated.trim().is_empty()
        || e.translated.trim() == e.text.trim()
        || (target_lang.starts_with("zh") && !has_cjk(&e.translated) && !has_cjk(&e.text))
        || looks_like_sound_effect(&e.text) != looks_like_sound_effect(&e.translated)
}

/// 失败统计（与前端工具栏一致）：返回 (failed, missing, translated, total)
fn count_status(file: &SubtitleFile, target_lang: &str) -> (usize, usize, usize, usize) {
    let total = file.entries.len();
    let failed = file.entries.iter().filter(|e| e.failed).count();
    let missing = file.entries.iter().filter(|e| is_untranslated(e, target_lang)).count();
    let translated = file.entries.iter().filter(|e| {
        !e.translated.is_empty() && !e.failed && !is_untranslated(e, target_lang)
    }).count();
    (failed, missing, translated, total)
}

// === SECTION 1 END ===

/// L3.1 双语字幕往返验证（单种格式）
/// 流程：翻译结果 → 导出双语字幕 → 重新加载 → detect_bilingual → split_bilingual → 验证失败数
fn check_bilingual_roundtrip_single(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    format: SubtitleFormat,
    target_lang: &str,
) -> CheckResult {
    let format_name = match format {
        SubtitleFormat::Srt => "SRT",
        SubtitleFormat::Ass => "ASS",
        SubtitleFormat::Vtt => "VTT",
        SubtitleFormat::Ssa => "SSA",
    };
    let check_name = format!("bilingual_roundtrip_{}", format_name.to_lowercase());

    // 翻译时的状态（与前端工具栏一致）
    let (orig_failed, orig_missing, orig_translated, orig_total) = count_status(translated, target_lang);
    // 总问题条目数 = failed ∪ missing（不能直接相加，因为 failed 和 missing 可能重叠）
    let orig_problematic = translated.entries.iter()
        .filter(|e| e.failed || is_untranslated(e, target_lang))
        .count();

    // 1. 导出双语字幕
    let parse_format = format.clone();
    let options = ExportOptions {
        format,
        mode: ExportMode::Bilingual,
        monolingual_lang: None,
        bilingual_translated_first: Some(true),
        ass_style: Some(AssBilingualStyle::default()),
        video_width: Some(1920),
        video_height: Some(1080),
    };
    let content = subtitle::export_subtitle(translated, &options);

    // 2. 重新加载（parse）
    let mut reloaded = match subtitle::parse_subtitle(&content, &parse_format) {
        Ok(f) => f,
        Err(e) => {
            return CheckResult::fail(
                &check_name,
                &format!("{} 双语字幕重新解析失败: {:?}", format_name, e),
                "subtitle.rs parse_subtitle",
            );
        }
    };

    // 3. 检测双语
    let detect = detect_bilingual(&reloaded);
    if !detect.is_bilingual {
        return CheckResult::fail(
            &check_name,
            &format!("{} 双语字幕检测失败: is_bilingual=false, matched={}, total={}",
                format_name, detect.matched_count, detect.total_count),
            "subtitle.rs detect_bilingual",
        );
    }

    // 4. 拆分双语 → text + translated
    split_bilingual(&mut reloaded, zimufan_lib::subtitle::SplitMode::EvenFirst);

    // 5. 验证条目数一致
    if reloaded.entries.len() != original.entries.len() {
        return CheckResult::fail(
            &check_name,
            &format!("{} 双语字幕条目数不一致: 原始={}, 重新加载={}",
                format_name, original.entries.len(), reloaded.entries.len()),
            "subtitle.rs split_bilingual",
        );
    }

    // 6. 验证失败数一致（与前端工具栏逻辑一致）
    // 注意：重新加载后 failed 标志丢失（字幕格式不存储 failed），
    // 原来的 failed 条目变为 missing（translated 为空），所以：
    //   reloaded_failed = 0, reloaded_missing = orig_failed + orig_missing
    // 总问题条目数 (failed ∪ missing) 应一致
    // 注意：不能直接用 failed + missing，因为 failed 和 missing 可能重叠
    // （如 failed=true 且 translated 为空的条目会被两者都计数）
    let (rel_failed, rel_missing, rel_translated, rel_total) = count_status(&reloaded, target_lang);
    let rel_problematic = rel_failed + rel_missing;

    if rel_problematic != orig_problematic {
        // 找出差异条目
        let mut diff_indices = Vec::new();
        for (i, (orig, rel)) in translated.entries.iter().zip(&reloaded.entries).enumerate() {
            let orig_bad = orig.failed || is_untranslated(orig, target_lang);
            let rel_bad = rel.failed || is_untranslated(rel, target_lang);
            if orig_bad != rel_bad {
                diff_indices.push(i);
            }
        }
        return CheckResult::fail(
            &check_name,
            &format!("{} 双语字幕问题数不一致: 翻译时 failed={}+missing={}={}, 重新加载 failed={}+missing={}={}, 差异条目: {:?}",
                format_name, orig_failed, orig_missing, orig_problematic,
                rel_failed, rel_missing, rel_problematic,
                &diff_indices[..diff_indices.len().min(10)]),
            "subtitle.rs export_subtitle / split_bilingual",
        );
    }

    // 7. 验证译文内容一致
    let mut content_mismatch = 0;
    for (orig, rel) in translated.entries.iter().zip(&reloaded.entries) {
        if !orig.translated.is_empty() && !rel.translated.is_empty() {
            let orig_norm = strip_and_collapse(&orig.translated);
            let rel_norm = strip_and_collapse(&rel.translated);
            if orig_norm != rel_norm {
                content_mismatch += 1;
            }
        }
    }
    if content_mismatch > 0 {
        return CheckResult::warn(
            &check_name,
            &format!("{} 双语字幕译文内容不一致: {} 条 (问题数一致: {})",
                format_name, content_mismatch, orig_problematic),
            "subtitle.rs export_subtitle / split_bilingual",
        );
    }

    CheckResult::pass(
        &check_name,
        &format!("{} 双语字幕往返一致: translated={}, failed={}, missing={} (翻译时 failed={}, missing={})",
            format_name, rel_translated, rel_failed, rel_missing, orig_failed, orig_missing),
    )
}

/// 去除标签和换行，用于译文内容比较
fn strip_and_collapse(s: &str) -> String {
    // 简单去除 ASS 标签和 HTML 标签，合并换行
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '{' | '<' => in_tag = true,
            '}' | '>' => in_tag = false,
            '\n' | '\\' if in_tag => continue,
            '\n' => result.push(' '),
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

// === SECTION 2 END ===

/// L3.1 双语字幕往返验证（SRT + ASS + VTT 三种格式）
pub fn check_bilingual_roundtrip_all(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    target_lang: &str,
) -> Vec<CheckResult> {
    vec![
        check_bilingual_roundtrip_single(original, translated, SubtitleFormat::Srt, target_lang),
        check_bilingual_roundtrip_single(original, translated, SubtitleFormat::Ass, target_lang),
        check_bilingual_roundtrip_single(original, translated, SubtitleFormat::Vtt, target_lang),
    ]
}

// === SECTION 3 END ===

/// L3.2 缓存恢复验证（全量）
/// 流程：翻译结果 → 清空内存中的 translated → 从缓存查询 → 验证失败数和译文一致
/// 注意：每批次的缓存恢复已在 check_batch_l3 中完成，此函数用于全量验证
#[allow(dead_code)]
pub fn check_cache_recovery(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    db: &zimufan_lib::db::Database,
    source_lang: &str,
    target_lang: &str,
    provider_name: &str,
    file_hash: &str,
) -> CheckResult {
    // 1. 构建一个"未翻译"的字幕文件（只有原文，无译文）
    let mut untranslated = original.clone();
    for entry in &mut untranslated.entries {
        entry.translated = String::new();
        entry.failed = false;
        entry.from_cache = false;
    }

    // 2. 从缓存查询
    let scheduler = zimufan_lib::translate::TranslateScheduler::new(
        db,
        std::sync::Arc::new(zimufan_lib::translate::BaiduProvider::new(
            String::new(),
            String::new(),
        )) as std::sync::Arc<dyn zimufan_lib::translate::TranslateProviderTrait + Send + Sync>,
        provider_name.to_string(),
    )
    .with_file_hash(file_hash.to_string());

    let cached = match scheduler.get_cached_entries(&untranslated.entries, source_lang, target_lang) {
        Ok(c) => c,
        Err(e) => {
            return CheckResult::fail(
                "cache_recovery",
                &format!("缓存查询失败: {:?}", e),
                "translate.rs get_cached_entries",
            );
        }
    };

    // 3. 将缓存结果填回
    let mut recovered = untranslated.clone();
    for entry in &mut recovered.entries {
        if let Some(c) = cached.iter().find(|c| c.index == entry.index) {
            entry.translated = c.translated.clone();
            entry.from_cache = true;
            entry.failed = c.failed;
        }
    }

    // 4. 验证缓存命中数
    let cached_count = recovered.entries.iter().filter(|e| e.from_cache).count();
    if cached_count == 0 {
        return CheckResult::fail(
            "cache_recovery",
            "缓存恢复 0 条，翻译结果未写入缓存或 file_hash 不一致",
            "translate.rs set_translate_cache / get_cached_entries",
        );
    }

    // 5. 验证问题数一致（与前端工具栏逻辑一致）
    // 注意：缓存恢复后 failed 标志丢失（TranslateEntry.failed 始终为 false），
    // 原来的 failed 条目变为 missing，所以总问题数 (failed + missing) 应一致
    let (orig_failed, orig_missing, _, _) = count_status(translated, target_lang);
    let (rec_failed, rec_missing, _, _) = count_status(&recovered, target_lang);
    let orig_problematic = orig_failed + orig_missing;
    let rec_problematic = rec_failed + rec_missing;

    if rec_problematic != orig_problematic {
        return CheckResult::fail(
            "cache_recovery",
            &format!("缓存恢复问题数不一致: 翻译时 failed={}+missing={}={}, 恢复后 failed={}+missing={}={} (缓存命中 {} 条)",
                orig_failed, orig_missing, orig_problematic,
                rec_failed, rec_missing, rec_problematic, cached_count),
            "translate.rs get_cached_entries 缓存质量校验",
        );
    }

    // 6. 验证译文内容一致
    let mut content_mismatch = 0;
    for (orig, rec) in translated.entries.iter().zip(&recovered.entries) {
        if orig.translated.trim() != rec.translated.trim() {
            content_mismatch += 1;
        }
    }
    if content_mismatch > 0 {
        return CheckResult::warn(
            "cache_recovery",
            &format!("缓存恢复译文内容不一致: {} 条 (问题数一致: {})",
                content_mismatch, orig_problematic),
            "translate.rs get_cached_entries 缓存质量校验",
        );
    }

    CheckResult::pass(
        "cache_recovery",
        &format!("缓存恢复一致: {} 条命中, failed={}, missing={} (翻译时 failed={}, missing={})",
            cached_count, rec_failed, rec_missing, orig_failed, orig_missing),
    )
}

// === SECTION 4 END ===

/// L3.3 多次打开关闭验证（模拟用户打开关闭 3 次）
/// 每次都从缓存恢复，验证问题数一致
pub fn check_repeated_open(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    db: &zimufan_lib::db::Database,
    source_lang: &str,
    target_lang: &str,
    provider_name: &str,
    file_hash: &str,
) -> Vec<CheckResult> {
    let (orig_failed, orig_missing, _, _) = count_status(translated, target_lang);
    // 总问题条目数 = failed ∪ missing（不能直接相加，因为 failed 和 missing 可能重叠）
    let orig_problematic = translated.entries.iter()
        .filter(|e| e.failed || is_untranslated(e, target_lang))
        .count();
    let mut results = Vec::new();

    for round in 1..=3 {
        let mut untranslated = original.clone();
        for entry in &mut untranslated.entries {
            entry.translated = String::new();
            entry.failed = false;
            entry.from_cache = false;
        }

        let scheduler = zimufan_lib::translate::TranslateScheduler::new(
            db,
            std::sync::Arc::new(zimufan_lib::translate::BaiduProvider::new(
                String::new(),
                String::new(),
            )) as std::sync::Arc<dyn zimufan_lib::translate::TranslateProviderTrait + Send + Sync>,
            provider_name.to_string(),
        )
        .with_file_hash(file_hash.to_string());

        let cached = match scheduler.get_cached_entries(&untranslated.entries, source_lang, target_lang) {
            Ok(c) => c,
            Err(e) => {
                results.push(CheckResult::fail(
                    &format!("repeated_open_{}", round),
                    &format!("第 {} 次打开缓存查询失败: {:?}", round, e),
                    "translate.rs get_cached_entries",
                ));
                continue;
            }
        };

        let mut recovered = untranslated.clone();
        for entry in &mut recovered.entries {
            if let Some(c) = cached.iter().find(|c| c.index == entry.index) {
                entry.translated = c.translated.clone();
                entry.from_cache = true;
                entry.failed = c.failed;
            }
        }

        let (rec_failed, rec_missing, _, _) = count_status(&recovered, target_lang);
        let rec_problematic = recovered.entries.iter()
            .filter(|e| e.failed || is_untranslated(e, target_lang))
            .count();
        let cached_count = recovered.entries.iter().filter(|e| e.from_cache).count();

        if rec_problematic != orig_problematic {
            // 找出差异条目
            let mut diff_indices = Vec::new();
            for (orig, rec) in translated.entries.iter().zip(&recovered.entries) {
                let orig_bad = orig.failed || is_untranslated(orig, target_lang);
                let rec_bad = rec.failed || is_untranslated(rec, target_lang);
                if orig_bad != rec_bad {
                    diff_indices.push(orig.index);
                }
            }
            results.push(CheckResult::fail(
                &format!("repeated_open_{}", round),
                &format!("第 {} 次打开问题数不一致: 翻译时={}, 恢复后={} (缓存命中 {} 条), 差异条目: {:?}",
                    round, orig_problematic, rec_problematic, cached_count,
                    &diff_indices[..diff_indices.len().min(20)]),
                "translate.rs get_cached_entries",
            ));
        } else {
            results.push(CheckResult::pass(
                &format!("repeated_open_{}", round),
                &format!("第 {} 次打开一致: {} 条命中, failed={}, missing={}",
                    round, cached_count, rec_failed, rec_missing),
            ));
        }
    }

    results
}

// === SECTION 5 END ===

/// L3 批次级检查：对单个批次的翻译结果做缓存恢复 + SRT 双语往返验证
/// 返回 Vec<CheckResult>，调用方根据是否有 fail 决定是否标记 BugFound
pub fn check_batch_l3(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    db: &zimufan_lib::db::Database,
    source_lang: &str,
    target_lang: &str,
    provider_name: &str,
    file_hash: &str,
    batch_start: usize,
    batch_end: usize,
) -> Vec<CheckResult> {
    let mut results = Vec::new();

    // L3.1 批次缓存恢复验证
    results.push(check_batch_cache_recovery(
        original, translated, db,
        source_lang, target_lang, provider_name, file_hash,
        batch_start, batch_end,
    ));

    // L3.2 批次 SRT 双语往返验证
    results.push(check_batch_bilingual_srt(
        original, translated, target_lang,
        batch_start, batch_end,
    ));

    results
}

// === SECTION 6 END ===

/// 批次缓存恢复验证：清空这个批次的译文 → 从缓存恢复 → 验证一致
fn check_batch_cache_recovery(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    db: &zimufan_lib::db::Database,
    source_lang: &str,
    target_lang: &str,
    provider_name: &str,
    file_hash: &str,
    batch_start: usize,
    batch_end: usize,
) -> CheckResult {
    let check_name = "batch_cache_recovery";

    // 构建未翻译版本（只清空这个批次的译文）
    let mut untranslated = original.clone();
    for entry in untranslated.entries.iter_mut().take(batch_end).skip(batch_start) {
        entry.translated = String::new();
        entry.failed = false;
        entry.from_cache = false;
    }

    // 从缓存查询
    let scheduler = zimufan_lib::translate::TranslateScheduler::new(
        db,
        std::sync::Arc::new(zimufan_lib::translate::BaiduProvider::new(
            String::new(),
            String::new(),
        )) as std::sync::Arc<dyn zimufan_lib::translate::TranslateProviderTrait + Send + Sync>,
        provider_name.to_string(),
    )
    .with_file_hash(file_hash.to_string());

    let cached = match scheduler.get_cached_entries(&untranslated.entries, source_lang, target_lang) {
        Ok(c) => c,
        Err(e) => {
            return CheckResult::fail(
                check_name,
                &format!("批次缓存查询失败: {:?}", e),
                "translate.rs get_cached_entries",
            );
        }
    };

    // 将缓存结果填回
    let mut recovered = untranslated.clone();
    for entry in recovered.entries.iter_mut() {
        if let Some(c) = cached.iter().find(|c| c.index == entry.index) {
            entry.translated = c.translated.clone();
            entry.from_cache = true;
            entry.failed = c.failed;
        }
    }

    // 只检查这个批次的条目
    let batch_orig: Vec<&SubtitleEntry> = translated.entries.iter()
        .filter(|e| e.index >= batch_start && e.index < batch_end)
        .collect();
    let batch_rec: Vec<&SubtitleEntry> = recovered.entries.iter()
        .filter(|e| e.index >= batch_start && e.index < batch_end)
        .collect();

    let cached_count = batch_rec.iter().filter(|e| e.from_cache).count();
    if cached_count == 0 {
        return CheckResult::fail(
            check_name,
            "批次缓存恢复 0 条，翻译结果未写入缓存或 file_hash 不一致",
            "translate.rs set_translate_cache / get_cached_entries",
        );
    }

    // 验证译文内容一致
    // 注意：缓存按 source_text 哈希存储，同一文件内若有重复原文（如 "[Kaleb] Go on."），
    // 后写入的译文会覆盖先写入的。因此对重复原文条目，只要缓存译文匹配其中任一条即可。
    use std::collections::HashMap;
    let mut text_to_translations: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &translated.entries {
        text_to_translations.entry(e.text.trim()).or_default().push(e.translated.trim());
    }
    let mut mismatch_indices = Vec::new();
    for (orig, rec) in batch_orig.iter().zip(batch_rec.iter()) {
        let orig_tr = orig.translated.trim();
        let rec_tr = rec.translated.trim();
        if orig_tr == rec_tr {
            continue;
        }
        // 重复原文条目：缓存可能返回另一条目的译文，检查是否匹配任一条
        if let Some(translations) = text_to_translations.get(orig.text.trim()) {
            if translations.len() > 1 && translations.iter().any(|t| *t == rec_tr) {
                continue;
            }
        }
        mismatch_indices.push(orig.index);
    }

    if !mismatch_indices.is_empty() {
        return CheckResult::fail(
            check_name,
            &format!("批次缓存恢复译文不一致: {} 条 (缓存命中 {} 条), 差异条目: {:?}",
                mismatch_indices.len(), cached_count,
                &mismatch_indices[..mismatch_indices.len().min(10)]),
            "translate.rs get_cached_entries 缓存质量校验",
        );
    }

    CheckResult::pass(
        check_name,
        &format!("批次缓存恢复一致: {} 条命中", cached_count),
    )
}

// === SECTION 7 END ===

/// 批次 SRT 双语往返验证：导出双语 SRT → 重新加载 → 拆分 → 验证一致
fn check_batch_bilingual_srt(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    target_lang: &str,
    batch_start: usize,
    batch_end: usize,
) -> CheckResult {
    let check_name = "batch_bilingual_srt";

    // 导出双语 SRT
    let options = ExportOptions {
        format: SubtitleFormat::Srt,
        mode: ExportMode::Bilingual,
        monolingual_lang: None,
        bilingual_translated_first: Some(true),
        ass_style: None,
        video_width: None,
        video_height: None,
    };
    let content = subtitle::export_subtitle(translated, &options);

    // 重新加载
    let mut reloaded = match subtitle::parse_subtitle(&content, &SubtitleFormat::Srt) {
        Ok(f) => f,
        Err(e) => {
            return CheckResult::fail(
                check_name,
                &format!("批次双语 SRT 重新解析失败: {:?}", e),
                "subtitle.rs parse_subtitle",
            );
        }
    };

    // 检测双语
    let detect = detect_bilingual(&reloaded);
    if !detect.is_bilingual {
        return CheckResult::fail(
            check_name,
            &format!("批次双语 SRT 检测失败: matched={}, total={}",
                detect.matched_count, detect.total_count),
            "subtitle.rs detect_bilingual",
        );
    }

    // 拆分双语
    split_bilingual(&mut reloaded, zimufan_lib::subtitle::SplitMode::EvenFirst);

    // 验证条目数一致
    if reloaded.entries.len() != original.entries.len() {
        return CheckResult::fail(
            check_name,
            &format!("批次双语 SRT 条目数不一致: 原始={}, 重新加载={}",
                original.entries.len(), reloaded.entries.len()),
            "subtitle.rs split_bilingual",
        );
    }

    // 只检查这个批次的条目
    let batch_orig: Vec<&SubtitleEntry> = translated.entries.iter()
        .filter(|e| e.index >= batch_start && e.index < batch_end)
        .collect();
    let batch_rel: Vec<&SubtitleEntry> = reloaded.entries.iter()
        .filter(|e| e.index >= batch_start && e.index < batch_end)
        .collect();

    // 验证问题数一致
    let (orig_failed, orig_missing, _, _) = count_status(translated, target_lang);
    let (rel_failed, rel_missing, _, _) = count_status(&reloaded, target_lang);

    // 只统计这个批次的问题数
    let orig_batch_problematic = batch_orig.iter().filter(|e| {
        e.failed || is_untranslated(e, target_lang)
    }).count();
    let rel_batch_problematic = batch_rel.iter().filter(|e| {
        e.failed || is_untranslated(e, target_lang)
    }).count();

    if rel_batch_problematic != orig_batch_problematic {
        let mut diff_indices = Vec::new();
        for (o, r) in batch_orig.iter().zip(batch_rel.iter()) {
            let o_bad = o.failed || is_untranslated(o, target_lang);
            let r_bad = r.failed || is_untranslated(r, target_lang);
            if o_bad != r_bad {
                diff_indices.push(o.index);
            }
        }
        return CheckResult::fail(
            check_name,
            &format!("批次双语 SRT 问题数不一致: 翻译时={}, 重新加载={}, 差异条目: {:?}",
                orig_batch_problematic, rel_batch_problematic,
                &diff_indices[..diff_indices.len().min(10)]),
            "subtitle.rs export_subtitle / split_bilingual",
        );
    }

    // 验证译文内容一致
    let mut content_mismatch = 0;
    let mut mismatch_indices = Vec::new();
    for (o, r) in batch_orig.iter().zip(batch_rel.iter()) {
        if !o.translated.is_empty() && !r.translated.is_empty() {
            let o_norm = strip_and_collapse(&o.translated);
            let r_norm = strip_and_collapse(&r.translated);
            if o_norm != r_norm {
                content_mismatch += 1;
                mismatch_indices.push(o.index);
            }
        }
    }
    if content_mismatch > 0 {
        return CheckResult::fail(
            check_name,
            &format!("批次双语 SRT 译文内容不一致: {} 条, 差异条目: {:?}",
                content_mismatch, &mismatch_indices[..mismatch_indices.len().min(10)]),
            "subtitle.rs export_subtitle / split_bilingual",
        );
    }

    CheckResult::pass(
        check_name,
        &format!("批次双语 SRT 往返一致: {} 条", batch_orig.len()),
    )
}

// === SECTION 8 END ===
