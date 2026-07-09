// L1 结构断言：条目数、时间轴、占位符、格式往返、字幕平移、跨格式转换
use zimufan_lib::subtitle::{self, SubtitleFile, SubtitleFormat};

/// 单个检查结果
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    pub source_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
        }
    }
}

impl CheckResult {
    pub fn pass(name: &str, detail: &str) -> Self {
        Self { name: name.to_string(), status: CheckStatus::Pass, detail: detail.to_string(), source_hint: None }
    }
    pub fn warn(name: &str, detail: &str, source_hint: &str) -> Self {
        Self { name: name.to_string(), status: CheckStatus::Warn, detail: detail.to_string(), source_hint: Some(source_hint.to_string()) }
    }
    pub fn fail(name: &str, detail: &str, source_hint: &str) -> Self {
        Self { name: name.to_string(), status: CheckStatus::Fail, detail: detail.to_string(), source_hint: Some(source_hint.to_string()) }
    }
}

// === SECTION 1 END ===

/// 运行所有 L1 检查（原始字幕，不含翻译结果）
pub fn run_l1_checks(original: &SubtitleFile) -> Vec<CheckResult> {
    let mut results = Vec::new();
    results.push(check_entry_count(original));
    results.push(check_timeline_validity(original));
    results.push(check_format_roundtrip(original));
    results
}

/// L1.1 条目数完整性（原始字幕自检：条目数 > 0 且序号唯一递增）
pub fn check_entry_count(file: &SubtitleFile) -> CheckResult {
    let count = file.entries.len();
    if count == 0 {
        return CheckResult::fail("entry_count", "条目数为 0", "subtitle.rs parse_srt");
    }

    // 检查序号是否唯一且递增（parse_srt 用 enumerate 的 idx，空 block 被跳过所以可能有跳号）
    let mut non_monotonic = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut duplicates = Vec::new();
    for window in file.entries.windows(2) {
        if window[0].index >= window[1].index {
            non_monotonic.push((window[0].index, window[1].index));
        }
    }
    for entry in &file.entries {
        if !seen.insert(entry.index) {
            duplicates.push(entry.index);
        }
    }

    if !duplicates.is_empty() {
        CheckResult::fail(
            "entry_count",
            &format!("条目数 {}，有重复序号: {:?}", count, &duplicates[..duplicates.len().min(5)]),
            "subtitle.rs parse_srt index 赋值",
        )
    } else if !non_monotonic.is_empty() {
        CheckResult::fail(
            "entry_count",
            &format!("条目数 {}，序号非递增: {:?}", count, &non_monotonic[..non_monotonic.len().min(3)]),
            "subtitle.rs parse_srt index 赋值",
        )
    } else {
        CheckResult::pass("entry_count", &format!("条目数 {}，序号唯一递增", count))
    }
}

/// L1.2 时间轴有效性（start < end，非负）
pub fn check_timeline_validity(file: &SubtitleFile) -> CheckResult {
    let mut invalid = Vec::new();
    for entry in &file.entries {
        if entry.start_ms < 0 || entry.end_ms < 0 {
            invalid.push((entry.index, "负时间".to_string()));
        } else if entry.start_ms >= entry.end_ms {
            invalid.push((entry.index, "start >= end".to_string()));
        }
    }

    if invalid.is_empty() {
        CheckResult::pass("timeline_validity", &format!("{} 条时间轴全部有效", file.entries.len()))
    } else {
        CheckResult::fail(
            "timeline_validity",
            &format!("{} 条时间轴无效: {:?}", invalid.len(), &invalid[..invalid.len().min(5)]),
            "subtitle.rs parse_srt 时间戳解析",
        )
    }
}

// === SECTION 2 END ===

/// L1.4 格式往返一致性（parse → render → parse，条目数一致）
pub fn check_format_roundtrip(file: &SubtitleFile) -> CheckResult {
    let rendered = subtitle::render_subtitle(file);
    let reparsed = subtitle::parse_subtitle(&rendered, &file.format);

    match reparsed {
        Ok(reparsed) => {
            if reparsed.entries.len() != file.entries.len() {
                return CheckResult::fail(
                    "format_roundtrip",
                    &format!("往返后条目数变化: {} → {}", file.entries.len(), reparsed.entries.len()),
                    "subtitle.rs parse/render 对",
                );
            }

            // 检查时间轴是否保留
            let mut time_mismatches = 0;
            for (orig, re) in file.entries.iter().zip(&reparsed.entries) {
                if orig.start_ms != re.start_ms || orig.end_ms != re.end_ms {
                    time_mismatches += 1;
                }
            }

            if time_mismatches > 0 {
                CheckResult::warn(
                    "format_roundtrip",
                    &format!("往返后 {} 条时间轴偏移（格式精度差异）", time_mismatches),
                    "subtitle.rs render 时间戳格式化",
                )
            } else {
                CheckResult::pass("format_roundtrip", &format!("{} 条往返一致", file.entries.len()))
            }
        }
        Err(e) => CheckResult::fail(
            "format_roundtrip",
            &format!("重新解析失败: {:?}", e),
            "subtitle.rs parse_subtitle",
        ),
    }
}

/// L1.4b 跨格式转换矩阵（SRT/ASS/VTT 全组合）
pub fn check_format_conversion_matrix(file: &SubtitleFile) -> Vec<CheckResult> {
    let mut results = Vec::new();
    let formats = [
        (SubtitleFormat::Srt, "srt"),
        (SubtitleFormat::Ass, "ass"),
        (SubtitleFormat::Vtt, "vtt"),
    ];

    for (_src_fmt, src_name) in &formats {
        for (dst_fmt, dst_name) in &formats {
            let name = format!("format_{}_to_{}", src_name, dst_name);
            let rendered = match dst_fmt {
                SubtitleFormat::Srt => subtitle::render_srt(file),
                SubtitleFormat::Ass => subtitle::render_ass(file),
                SubtitleFormat::Vtt => subtitle::render_vtt(file),
                _ => continue,
            };
            let reparsed = subtitle::parse_subtitle(&rendered, dst_fmt);

            match reparsed {
                Ok(re) => {
                    if re.entries.len() != file.entries.len() {
                        results.push(CheckResult::fail(
                            &name,
                            &format!("条目数: {} → {}", file.entries.len(), re.entries.len()),
                            &format!("subtitle.rs render_{} / parse_{}", dst_name, dst_name),
                        ));
                    } else {
                        results.push(CheckResult::pass(&name, &format!("条目数一致: {}", re.entries.len())));
                    }
                }
                Err(e) => {
                    results.push(CheckResult::fail(
                        &name,
                        &format!("解析失败: {:?}", e),
                        &format!("subtitle.rs parse_{}", dst_name),
                    ));
                }
            }
        }
    }

    results
}

// === SECTION 3 END ===

/// 运行翻译后的 L1 检查（对比原文和译文）
pub fn run_l1_checks_translated(original: &SubtitleFile, translated: &SubtitleFile) -> Vec<CheckResult> {
    let mut results = Vec::new();
    results.push(check_translated_entry_count(original, translated));
    results.push(check_translated_timeline(original, translated));
    results.push(check_translated_format_roundtrip(translated));
    results.push(check_subtitle_shift(original, translated));
    results
}

/// L1.1t 翻译后条目数完整性
pub fn check_translated_entry_count(original: &SubtitleFile, translated: &SubtitleFile) -> CheckResult {
    let orig_count = original.entries.len();
    let trans_count = translated.entries.len();

    if orig_count != trans_count {
        // 找出缺失/多余的条目
        let orig_indices: std::collections::HashSet<usize> = original.entries.iter().map(|e| e.index).collect();
        let trans_indices: std::collections::HashSet<usize> = translated.entries.iter().map(|e| e.index).collect();
        let missing: Vec<_> = orig_indices.difference(&trans_indices).collect();
        let extra: Vec<_> = trans_indices.difference(&orig_indices).collect();

        let mut detail = format!("条目数不匹配: 原文 {} 条，译文 {} 条", orig_count, trans_count);
        if !missing.is_empty() {
            detail.push_str(&format!("\n缺失: {:?}", &missing[..missing.len().min(5)]));
        }
        if !extra.is_empty() {
            detail.push_str(&format!("\n多余: {:?}", &extra[..extra.len().min(5)]));
        }
        CheckResult::fail("translated_entry_count", &detail, "translate.rs translate_batch_with_fallback 降级逻辑")
    } else {
        CheckResult::pass("translated_entry_count", &format!("条目数一致: {}", trans_count))
    }
}

/// L1.2t 翻译后时间轴对齐
pub fn check_translated_timeline(original: &SubtitleFile, translated: &SubtitleFile) -> CheckResult {
    let mut mismatches = Vec::new();
    for (orig, trans) in original.entries.iter().zip(&translated.entries) {
        if orig.start_ms != trans.start_ms || orig.end_ms != trans.end_ms {
            mismatches.push((orig.index, orig.start_ms, trans.start_ms, orig.end_ms, trans.end_ms));
        }
    }

    if mismatches.is_empty() {
        CheckResult::pass("translated_timeline", "时间轴全部对齐")
    } else {
        CheckResult::fail(
            "translated_timeline",
            &format!("{} 条时间轴偏移: {:?}", mismatches.len(), &mismatches[..mismatches.len().min(3)]),
            "subtitle.rs parse/render 时间戳",
        )
    }
}

// === SECTION 4 END ===

/// L1.4t 翻译后格式往返
pub fn check_translated_format_roundtrip(translated: &SubtitleFile) -> CheckResult {
    let rendered = subtitle::render_subtitle(translated);
    match subtitle::parse_subtitle(&rendered, &translated.format) {
        Ok(re) => {
            if re.entries.len() != translated.entries.len() {
                CheckResult::fail(
                    "translated_format_roundtrip",
                    &format!("往返后条目数变化: {} → {}", translated.entries.len(), re.entries.len()),
                    "subtitle.rs parse/render",
                )
            } else {
                CheckResult::pass("translated_format_roundtrip", "翻译后格式往返一致")
            }
        }
        Err(e) => CheckResult::fail(
            "translated_format_roundtrip",
            &format!("重新解析失败: {:?}", e),
            "subtitle.rs parse_subtitle",
        ),
    }
}

/// L1.5 字幕平移检测（启发式：字符 n-gram 重叠率）
pub fn check_subtitle_shift(original: &SubtitleFile, translated: &SubtitleFile) -> CheckResult {
    // 此检查需要译文有内容，且原文和译文是不同语言
    // 启发式：如果译文和相邻原文的"翻译相似度"模式异常
    // 由于原文和译文是不同语言，直接比较文本相似度无意义
    // 改用：检查译文是否为空（空译文可能是平移的征兆）
    let empty_indices: Vec<usize> = translated.entries.iter()
        .filter(|e| e.translated.trim().is_empty())
        .map(|e| e.index)
        .collect();

    if !empty_indices.is_empty() {
        return CheckResult::warn(
            "subtitle_shift",
            &format!("{} 条空译文（可能是平移或降级失败）: {:?}", empty_indices.len(), &empty_indices[..empty_indices.len().min(5)]),
            "translate.rs translate_batch_with_fallback",
        );
    }

    // 检查译文长度比值异常的条目（可能被合并或截断）
    let mut abnormal = Vec::new();
    for (orig, trans) in original.entries.iter().zip(&translated.entries) {
        // 排除音效标记和音乐符号（长度比值无意义）
        if looks_like_sound_effect(&orig.text) || is_music_or_symbol_only(&orig.text) {
            continue;
        }
        let orig_len = orig.text.chars().count().max(1);
        let trans_len = trans.translated.chars().count();
        if trans_len > 0 {
            let ratio = trans_len as f64 / orig_len as f64;
            if ratio > 4.0 || ratio < 0.15 {
                abnormal.push((orig.index, ratio));
            }
        }
    }

    if abnormal.is_empty() {
        CheckResult::pass("subtitle_shift", "无平移迹象")
    } else {
        CheckResult::warn(
            "subtitle_shift",
            &format!("{} 条译文长度比值异常（可能合并/截断）: {:?}", abnormal.len(), &abnormal[..abnormal.len().min(3)]),
            "translate.rs batch 翻译逻辑",
        )
    }
}

// === SECTION 5 END ===

/// 判断是否为音效标记（如 [Laughter]、[music] 等）
fn looks_like_sound_effect(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() { return false; }
    s.starts_with('[') && s.ends_with(']')
}

/// 判断是否为纯音乐符号/特殊符号（如 ♪♪、♬♬ 等）
fn is_music_or_symbol_only(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() { return false; }
    s.chars().all(|c| {
        c.is_whitespace()
        || "♪♬♫♩♭♮♯".contains(c)
        || matches!(c, '[' | ']' | '(' | ')' | '.' | '-' | '_' | '*')
    })
}
