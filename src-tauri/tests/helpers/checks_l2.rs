// L2 语言正确性：空译文、假翻译、CJK、音效标记、人名一致性、长度
use super::checks_l1::{CheckResult, CheckStatus};
use zimufan_lib::subtitle::{SubtitleEntry, SubtitleFile};

/// 运行所有 L2 检查
pub fn run_l2_checks(original: &SubtitleFile, translated: &SubtitleFile, target_lang: &str) -> Vec<CheckResult> {
    let mut results = Vec::new();
    results.push(check_empty_translations(translated));
    results.push(check_fake_translations(original, translated));
    if target_lang == "zh" {
        results.push(check_cjk(translated));
    }
    results.push(check_sound_effects(original, translated));
    results.push(check_name_consistency(translated));
    results.push(check_length_ratio(original, translated));
    results.push(check_alignment(original, translated));
    results.push(check_truncation(original, translated));
    results
}

/// L2.1 空译文检测
pub fn check_empty_translations(translated: &SubtitleFile) -> CheckResult {
    let empty_indices: Vec<usize> = translated.entries.iter()
        .filter(|e| {
            // 跳过已标记为 failed 的条目（系统已知翻译失败，用户会重新翻译）
            !e.failed && e.translated.trim().is_empty()
        })
        .map(|e| e.index)
        .collect();

    if empty_indices.is_empty() {
        CheckResult::pass("empty_translations", "无空译文")
    } else {
        CheckResult::fail(
            "empty_translations",
            &format!("{} 条空译文: {:?}", empty_indices.len(), &empty_indices[..empty_indices.len().min(5)]),
            "translate.rs translate_batch_with_fallback 降级重试",
        )
    }
}

// === SECTION 1 END ===

/// L2.2 假翻译检测（译文=原文）
pub fn check_fake_translations(original: &SubtitleFile, translated: &SubtitleFile) -> CheckResult {
    let mut fake_indices = Vec::new();
    for (orig, trans) in original.entries.iter().zip(&translated.entries) {
        if !trans.translated.trim().is_empty() && trans.translated.trim() == orig.text.trim() {
            // 排除纯音效标记（如 [music] 翻译后保持原样是合理的）
            // 排除纯音乐符号（如 ♪♪ 翻译后保持原样是合理的）
            let stripped = orig.text.trim();
            if !looks_like_sound_effect(stripped) && !is_music_or_symbol_only(stripped) {
                fake_indices.push(orig.index);
            }
        }
    }

    let total = translated.entries.len();
    let ratio = if total > 0 { fake_indices.len() as f64 / total as f64 } else { 0.0 };

    if ratio > 0.05 {
        CheckResult::fail(
            "fake_translations",
            &format!("假翻译 {} 条 ({:.1}%): {:?}", fake_indices.len(), ratio * 100.0, &fake_indices[..fake_indices.len().min(5)]),
            "translate.rs prompt 模板（强化必须翻译）",
        )
    } else if !fake_indices.is_empty() {
        CheckResult::warn(
            "fake_translations",
            &format!("假翻译 {} 条 ({:.2}%)", fake_indices.len(), ratio * 100.0),
            "translate.rs prompt 模板",
        )
    } else {
        CheckResult::pass("fake_translations", "无假翻译")
    }
}

/// L2.3 CJK 字符检测（目标语言=中文时）
pub fn check_cjk(translated: &SubtitleFile) -> CheckResult {
    let no_cjk: Vec<usize> = translated.entries.iter()
        .filter(|e| {
            // 跳过已标记为 failed 的条目（系统已知翻译失败，用户会重新翻译）
            !e.failed
            && {
                let t = e.translated.trim();
                !t.is_empty()
                && !has_cjk_chars(&e.translated)
                // 排除音效标记 [xxx] 和音乐符号 ♪♪ 等（非文字内容不需要 CJK）
                && !looks_like_sound_effect(&e.translated)
                && !is_music_or_symbol_only(&e.translated)
                // 排除含音乐符号的歌词/拟声词（如 "♪ Da-da da da ♪"）
                // 9b 保持原样是正确行为，因为音乐拟声词无法翻译
                && !has_music_symbols(&e.translated)
                // 排除非英语原文（如拼写字母 G-O-R、祖鲁语歌词等，保持原样是正确行为）
                && !is_non_english_source(&e.text)
            }
        })
        .map(|e| e.index)
        .collect();

    if no_cjk.is_empty() {
        CheckResult::pass("cjk_check", "译文均含 CJK 字符")
    } else {
        CheckResult::fail(
            "cjk_check",
            &format!("{} 条译文无 CJK 字符: {:?}", no_cjk.len(), &no_cjk[..no_cjk.len().min(5)]),
            "translate.rs prompt 或模型不支持中文",
        )
    }
}

/// 判断是否为纯音乐符号/特殊符号（如 ♪♪、♬♬ 等，无文字内容）
fn is_music_or_symbol_only(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() { return false; }
    // 全部由音乐符号、标点、空白组成
    s.chars().all(|c| {
        c.is_whitespace()
        || "♪♬♫♩♭♮♯".contains(c)
        || matches!(c, '[' | ']' | '(' | ')' | '.' | '-' | '_' | '*')
    })
}

/// 判断文本是否包含音乐符号（♪♬♫♩ 等）
/// 含音乐符号的条目是歌词/拟声词（如 "♪ Da-da da da ♪"），
/// 9b 保持原样是正确行为，因为音乐拟声词无法翻译
fn has_music_symbols(s: &str) -> bool {
    s.chars().any(|c| "♪♬♫♩♭♮♯".contains(c))
}

// === SECTION 2 END ===

/// L2.4 音效标记一致性
pub fn check_sound_effects(original: &SubtitleFile, translated: &SubtitleFile) -> CheckResult {
    let mut mismatches = Vec::new();
    for (orig, trans) in original.entries.iter().zip(&translated.entries) {
        let orig_is_sfx = looks_like_sound_effect(&orig.text);
        let trans_is_sfx = looks_like_sound_effect(&trans.translated);
        if orig_is_sfx != trans_is_sfx && !trans.translated.trim().is_empty() {
            mismatches.push((orig.index, orig_is_sfx, trans_is_sfx));
        }
    }

    if mismatches.is_empty() {
        CheckResult::pass("sound_effect_consistency", "音效标记一致")
    } else {
        CheckResult::warn(
            "sound_effect_consistency",
            &format!("{} 条音效标记不一致: {:?}", mismatches.len(), &mismatches[..mismatches.len().min(3)]),
            "translate.rs prompt 音效标记规则",
        )
    }
}

/// L2.5 人名一致性
pub fn check_name_consistency(translated: &SubtitleFile) -> CheckResult {
    // 检查译文中是否残留 <name=...> 标签
    let mut tag_residuals = Vec::new();
    let mut name_translations: std::collections::HashMap<String, std::collections::HashSet<String>> = std::collections::HashMap::new();

    for entry in &translated.entries {
        // 检查残留标签
        if entry.translated.contains("<name=") || entry.translated.contains("</name>") {
            tag_residuals.push(entry.index);
        }
        // 提取人名标签
        for (en, zh) in extract_name_tags(&entry.translated) {
            name_translations.entry(en).or_default().insert(zh);
        }
    }

    let conflicts: Vec<_> = name_translations.iter()
        .filter(|(_, zh_set)| zh_set.len() > 1)
        .collect();

    let mut issues = Vec::new();

    if !tag_residuals.is_empty() {
        issues.push(format!("{} 条译文残留 <name> 标签: {:?}", tag_residuals.len(), &tag_residuals[..tag_residuals.len().min(3)]));
    }

    if !conflicts.is_empty() {
        let conflict_detail: Vec<String> = conflicts.iter()
            .map(|(en, zh_set)| format!("{} → {:?}", en, zh_set))
            .take(3)
            .collect();
        issues.push(format!("人名不一致: {}", conflict_detail.join(", ")));
    }

    if issues.is_empty() {
        CheckResult::pass("name_consistency", "人名一致，无残留标签")
    } else {
        CheckResult::warn(
            "name_consistency",
            &issues.join("\n"),
            "translate.rs post_process_name_tags / extract_name_tags",
        )
    }
}

/// L2.6 译文长度合理性
pub fn check_length_ratio(original: &SubtitleFile, translated: &SubtitleFile) -> CheckResult {
    let mut out_of_range = Vec::new();
    for (orig, trans) in original.entries.iter().zip(&translated.entries) {
        if trans.translated.trim().is_empty() {
            continue;
        }
        // 排除音效标记和音乐符号（长度比值无意义）
        if looks_like_sound_effect(&orig.text) || is_music_or_symbol_only(&orig.text) {
            continue;
        }
        let orig_len = orig.text.chars().count().max(1);
        let trans_len = trans.translated.chars().count();
        let ratio = trans_len as f64 / orig_len as f64;
        if ratio > 4.0 || ratio < 0.15 {
            out_of_range.push((orig.index, orig_len, trans_len, ratio));
        }
    }

    if out_of_range.is_empty() {
        CheckResult::pass("length_ratio", "译文长度全部在合理范围")
    } else {
        CheckResult::warn(
            "length_ratio",
            &format!("{} 条译文长度异常: {:?}", out_of_range.len(), &out_of_range[..out_of_range.len().min(3)]),
            "translate.rs prompt 或 batch 逻辑",
        )
    }
}

// === SECTION 3 END ===

/// 检查字符串是否含 CJK 字符
fn has_cjk_chars(s: &str) -> bool {
    s.chars().any(|c| {
        let code = c as u32;
        (0x4E00..=0x9FFF).contains(&code) || (0x3400..=0x4DBF).contains(&code)
    })
}

/// 检查原文是否为非英语内容（如拼写字母 G-O-R、祖鲁语歌词等）
/// 与 translate.rs 的 has_english_word(text, 3) 判定一致：
/// 如果原文不含至少 3 个英语单词，视为非英语内容，保持原样是正确行为
fn is_non_english_source(s: &str) -> bool {
    let word_count = s.split_whitespace()
        .filter(|w| {
            let cleaned: String = w.chars().filter(|c| c.is_ascii_alphabetic()).collect();
            cleaned.len() >= 2
        })
        .count();
    word_count < 3
}

/// 检查是否像音效标记（[xxx] 或 (xxx)）
fn looks_like_sound_effect(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s.starts_with('[') && s.ends_with(']') {
        return true;
    }
    if s.starts_with('(') && s.ends_with(')') {
        return true;
    }
    false
}

/// 从文本中提取 <name=En>Zh</name> 标签
fn extract_name_tags(text: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("<name=") {
        if let Some(gt) = remaining[start..].find('>') {
            let tag = &remaining[start..start + gt];
            let en = tag.strip_prefix("<name=").unwrap_or(tag).trim();
            if let Some(end) = remaining[start + gt + 1..].find("</name>") {
                let zh = &remaining[start + gt + 1..start + gt + 1 + end];
                result.push((en.to_string(), zh.to_string()));
                remaining = &remaining[start + gt + 1 + end + 7..];
            } else {
                break;
            }
        } else {
            break;
        }
    }
    result
}

// === SECTION 4: L2.7 错位检测 + L2.8 截断检测 ===

/// L2.7 译文错位检测（数字匹配启发式）
/// 原理：提取原文和译文中的数字，如果译文[N]含有原文[N]没有的数字，
/// 且这些"外来"数字出现在原文[N-1]或原文[N+1]中，标记为疑似错位。
/// 同时要求原文[N]有数字不在译文中（双向不匹配才可疑）。
pub fn check_alignment(original: &SubtitleFile, translated: &SubtitleFile) -> CheckResult {
    let mut suspected = Vec::new();

    for i in 0..translated.entries.len() {
        let trans = &translated.entries[i];

        // 跳过：failed、空译文、音效、音乐符号
        if trans.failed || trans.translated.trim().is_empty() {
            continue;
        }
        if looks_like_sound_effect(&trans.translated) || is_music_or_symbol_only(&trans.translated) {
            continue;
        }

        // 提取译文和原文[N]的数字
        let trans_numbers = extract_numbers(&trans.translated);
        if trans_numbers.is_empty() {
            continue;
        }
        if i >= original.entries.len() {
            continue;
        }
        let orig_numbers = extract_numbers(&original.entries[i].text);
        if orig_numbers.is_empty() {
            continue;
        }

        // 双向不匹配：译文有外来数字 AND 原文有缺失数字
        let trans_has_foreign = trans_numbers.iter().any(|t| !orig_numbers.contains(t));
        let orig_has_missing = orig_numbers.iter().any(|o| !trans_numbers.contains(o));
        if !trans_has_foreign || !orig_has_missing {
            continue;
        }

        // 检查外来数字是否匹配邻居
        let foreign_nums: Vec<u64> = trans_numbers.iter()
            .filter(|t| !orig_numbers.contains(t))
            .copied()
            .collect();

        let prev_match = if i > 0 && i - 1 < original.entries.len() {
            let prev_numbers = extract_numbers(&original.entries[i - 1].text);
            foreign_nums.iter().all(|t| prev_numbers.contains(t))
        } else {
            false
        };

        let next_match = if i + 1 < original.entries.len() {
            let next_numbers = extract_numbers(&original.entries[i + 1].text);
            foreign_nums.iter().all(|t| next_numbers.contains(t))
        } else {
            false
        };

        if prev_match || next_match {
            let direction = if prev_match { "↑(N-1)" } else { "↓(N+1)" };
            suspected.push((trans.index, direction));
        }
    }

    if suspected.is_empty() {
        CheckResult::pass("alignment_check", "无错位迹象")
    } else {
        CheckResult::fail(
            "alignment_check",
            &format!("{} 条疑似译文错位: {:?}", suspected.len(), &suspected[..suspected.len().min(5)]),
            "translate.rs batch 翻译结果对齐逻辑",
        )
    }
}

/// L2.8 译文截断检测
/// 三重信号：1) 原文有句末标点但译文没有 2) 严格长度比 < 0.3 3) 原文句子数 > 译文句子数
pub fn check_truncation(original: &SubtitleFile, translated: &SubtitleFile) -> CheckResult {
    let mut truncated = Vec::new();

    for (orig, trans) in original.entries.iter().zip(&translated.entries) {
        // 跳过：failed、空译文、音效、音乐符号、非英语原文
        if trans.failed || trans.translated.trim().is_empty() {
            continue;
        }
        if looks_like_sound_effect(&orig.text) || is_music_or_symbol_only(&orig.text) {
            continue;
        }
        if is_non_english_source(&orig.text) {
            continue;
        }

        let orig_text = orig.text.trim();
        let trans_text = trans.translated.trim();
        let mut flags: Vec<String> = Vec::new();

        // 信号 1：原文以句末标点结尾，译文不以句末标点结尾
        let orig_ends_punct = ends_with_sentence_punct(orig_text);
        let trans_ends_punct = ends_with_sentence_punct(trans_text);
        if orig_ends_punct && !trans_ends_punct {
            flags.push("句末标点缺失".to_string());
        }

        // 信号 2：严格长度比 < 0.3（仅对原文 ≥ 10 字符检查，避免短句误报）
        let orig_len = orig_text.chars().count().max(1);
        let trans_len = trans_text.chars().count();
        if orig_len >= 10 {
            let ratio = trans_len as f64 / orig_len as f64;
            if ratio < 0.3 {
                flags.push(format!("长度比 {:.2}", ratio));
            }
        }

        // 信号 3：原文句子数 > 译文句子数（原文至少 2 句）
        let orig_sentences = count_sentences(orig_text);
        let trans_sentences = count_sentences(trans_text);
        if orig_sentences > trans_sentences && orig_sentences >= 2 {
            flags.push(format!("句子数 {}→{}", orig_sentences, trans_sentences));
        }

        if !flags.is_empty() {
            truncated.push((orig.index, flags.join(", ")));
        }
    }

    if truncated.is_empty() {
        CheckResult::pass("truncation_check", "无截断迹象")
    } else {
        CheckResult::warn(
            "truncation_check",
            &format!("{} 条疑似截断: {:?}", truncated.len(), &truncated[..truncated.len().min(5)]),
            "translate.rs prompt 或 batch 翻译逻辑",
        )
    }
}

// === SECTION 4 END ===

/// 从文本中提取所有数字（整数，至少 1 位）
fn extract_numbers(s: &str) -> Vec<u64> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            current.push(c);
        } else if !current.is_empty() {
            if let Ok(n) = current.parse::<u64>() {
                numbers.push(n);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(n) = current.parse::<u64>() {
            numbers.push(n);
        }
    }
    numbers
}

/// 检查文本是否以句末标点结尾
fn ends_with_sentence_punct(s: &str) -> bool {
    let s = s.trim_end();
    if s.is_empty() {
        return false;
    }
    s.ends_with(|c: char| matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '；' | ';'))
}

/// 统计文本中的句子数（按句末标点分割）
fn count_sentences(s: &str) -> usize {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    let sentences: Vec<&str> = s.split(|c: char| matches!(c, '.' | '!' | '?' | '。' | '！' | '？'))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    sentences.len().max(1)
}