// 字幕解析与编辑模块
// srt/vtt 自写解析器，ass 使用 ass-rs（ass-core + ass-editor）
// 统一内部结构 SubtitleEntry，对应需求文档 §7 parse_subtitle 返回值

use crate::error::AppError;
use crate::translate::{cleanup_cjk_spaces, looks_like_sound_effect};
use serde::{Deserialize, Serialize};

/// 字幕格式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    Ass,
    Ssa,
}

/// 字幕条目（统一内部结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleEntry {
    pub index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,        // 原文（含 ass 样式标记或 srt/vtt 纯文本）
    pub translated: String,  // 译文（翻译后填充，初始为空）
    pub style: Option<String>, // ass 样式名（仅 ass 格式）
    /// 翻译是否失败（仅内存状态，不写入字幕文件）
    #[serde(default)]
    pub failed: bool,
    /// 译文是否来自缓存（仅内存状态，用于统计显示）
    #[serde(default)]
    pub from_cache: bool,
}

/// 字幕文件（解析后的统一结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleFile {
    pub format: SubtitleFormat,
    pub entries: Vec<SubtitleEntry>,
    pub raw_header: Option<String>, // ass 的 [Script Info] + [V4+ Styles]，srt/vtt 为 None
    pub source_path: Option<String>,
    /// 字幕内容 hash（sha256），基于所有条目的 index+时间轴+文本拼接计算。
    /// 用于翻译缓存隔离：不同字幕（即使含相同句子）不会命中彼此的缓存。
    /// 不用文件路径（文件可能被移动/重命名/导出）。
    #[serde(default)]
    pub file_hash: String,
}

// === SECTION 1 END ===

// === 双语字幕检测模块 ===

/// 剥离 ass 样式标记 {\...}，返回纯文本内容
/// 用于双语检测时避免样式名中的文字（如"60字体 美剧 中文"）干扰语言分类
fn strip_ass_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_brace = false; // ASS override block {...}
    let mut in_html = false;  // HTML tag <...>（部分字幕组用 <font> 区分双语）
    for c in s.chars() {
        if c == '{' {
            in_brace = true;
        } else if c == '}' {
            in_brace = false;
        } else if c == '<' {
            in_html = true;
        } else if c == '>' {
            in_html = false;
        } else if !in_brace && !in_html {
            result.push(c);
        }
    }
    result
}

/// 判断 text 是否为 ass 矢量绘图指令（含 \p1 标记开启绘图模式）
/// 绘图模式下的内容是路径命令（m/l/c 等），不是字幕文本，应跳过
fn is_ass_drawing(text: &str) -> bool {
    text.contains("\\p1")
}

/// 判断 ass 条目是否为非字幕内容（LOGO/水印/特效等），应跳过
/// 规则：
/// 1. 含 \p1 绘图指令
/// 2. 样式名含 LOGO/logo/水印/特效 等关键字
/// 3. 样式名为"金流光"等明显非字幕样式（含"光"字且非 Default）
fn is_non_subtitle_ass_entry(text: &str, style: &str) -> bool {
    // 绘图指令
    if is_ass_drawing(text) {
        return true;
    }
    let style_lower = style.to_lowercase();
    // 样式名含 LOGO/logo/水印/特效/watermark 等关键字
    if style_lower.contains("logo") || style.contains("水印") || style.contains("特效") || style_lower.contains("watermark") {
        return true;
    }
    // "金流光"等特效样式（含"光"字且非 Default）
    if style.contains("光") && !style.eq_ignore_ascii_case("default") {
        return true;
    }
    false
}

/// 语言类别（按 Unicode 范围粗分）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LangClass {
    Cjk,      // 中日韩汉字
    Hiragana, // 平假名
    Katakana, // 片假名
    Hangul,   // 韩文
    Latin,    // 拉丁字母（英/法/德等）
    Cyrillic, // 西里尔文（俄等）
    Arabic,   // 阿拉伯文
    Other,    // 其他/纯符号
}

/// 检测单个字符的语言类别
fn classify_char(c: char) -> LangClass {
    let code = c as u32;
    match code {
        // CJK 统一汉字
        0x4E00..=0x9FFF => LangClass::Cjk,
        // CJK 扩展 A
        0x3400..=0x4DBF => LangClass::Cjk,
        // CJK 兼容汉字
        0xF900..=0xFAFF => LangClass::Cjk,
        // 平假名
        0x3040..=0x309F => LangClass::Hiragana,
        // 片假名
        0x30A0..=0x30FF => LangClass::Katakana,
        // 韩文音节
        0xAC00..=0xD7AF => LangClass::Hangul,
        // 韩文兼容字母
        0x1100..=0x11FF => LangClass::Hangul,
        0x3130..=0x318F => LangClass::Hangul,
        // 拉丁字母（含扩展）
        0x0041..=0x005A => LangClass::Latin,
        0x0061..=0x007A => LangClass::Latin,
        0x00C0..=0x024F => LangClass::Latin,
        // 西里尔文
        0x0400..=0x04FF => LangClass::Cyrillic,
        // 阿拉伯文
        0x0600..=0x06FF => LangClass::Arabic,
        _ => LangClass::Other,
    }
}

/// 检测一行文本的主导语言（忽略数字、标点、空白、ass 样式标记）
pub fn detect_line_lang(line: &str) -> LangClass {
    // 剥离 ass 样式标记 {\...}，避免样式名中的文字干扰语言分类
    let stripped = strip_ass_tags(line);
    let mut counts: std::collections::HashMap<LangClass, usize> = std::collections::HashMap::new();
    for c in stripped.chars() {
        if c.is_alphabetic() {
            let cls = classify_char(c);
            if cls != LangClass::Other {
                *counts.entry(cls).or_insert(0) += 1;
            }
        }
    }
    if counts.is_empty() {
        return LangClass::Other;
    }
    // 返回出现次数最多的语言类别
    counts.into_iter().max_by_key(|(_, v)| *v).unwrap().0
}

/// 判断两个语言类别是否属于不同语言（用于双语检测）
#[allow(dead_code)]
fn is_different_lang(a: LangClass, b: LangClass) -> bool {
    // Hiragana/Katakana/Cjk 都算日语系，不互相区分
    let group = |c: LangClass| match c {
        LangClass::Cjk | LangClass::Hiragana | LangClass::Katakana => 0, // CJK/日
        LangClass::Hangul => 1, // 韩
        LangClass::Latin => 2,  // 拉丁
        LangClass::Cyrillic => 3,
        LangClass::Arabic => 4,
        LangClass::Other => 5,
    };
    group(a) != group(b) && group(a) != 5 && group(b) != 5
}

/// 双语检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BilingualDetectResult {
    pub is_bilingual: bool,
    /// 拆分模式：even_first = 偶数行(0,2,4...)为语言A，奇数行为语言B；odd_first = 反之
    pub split_mode: SplitMode,
    /// 语言A的类别（通常是原文）
    pub lang_a: String,
    /// 语言B的类别（通常是译文）
    pub lang_b: String,
    /// 匹配的条目数（用于判断置信度）
    pub matched_count: usize,
    /// 总条目数
    pub total_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SplitMode {
    /// 偶数行(第0,2,4...行)为语言A
    EvenFirst,
    /// 奇数行(第1,3,5...行)为语言A
    OddFirst,
}

/// 检测字幕是否为双语字幕
/// 策略：对每个条目的多行文本，检测是否存在交替语言模式
/// 注意：使用与 split_bilingual 相同的 classify_line 三态分类逻辑，
/// 避免检测计数与实际拆分结果不一致
pub fn detect_bilingual(file: &SubtitleFile) -> BilingualDetectResult {
    let total = file.entries.len();
    if total == 0 {
        return BilingualDetectResult {
            is_bilingual: false,
            split_mode: SplitMode::EvenFirst,
            lang_a: "unknown".to_string(),
            lang_b: "unknown".to_string(),
            matched_count: 0,
            total_count: 0,
        };
    }

    // 检测条目内"按语言分块"模式：前N行是语言A，后M行是语言B
    let mut matched_count = 0usize;
    let mut lang_a_class = LangClass::Other;
    let mut lang_b_class = LangClass::Other;
    // 统计"可能为双语的条目数"：多行且含至少两种不同语言的条目
    // 纯单语言的多行条目（如纯英文多行）不可能是双语，不应计入阈值分母
    let mut bilingual_candidate_count = 0usize;

    for entry in &file.entries {
        // 跳过 ass 矢量绘图指令（如 LOGO 绘制），不是字幕文本
        if is_ass_drawing(&entry.text) {
            continue;
        }
        // ass 用 \N 表示硬换行，转为 \n 后再按行分割做语言检测
        let normalized = entry.text.replace("\\N", "\n").replace("\\n", "\n");
        let lines: Vec<&str> = normalized.lines().collect();
        if lines.len() < 2 {
            continue;
        }
        // 使用 classify_line（三态分类）而非 detect_line_lang（主导语言），
        // 与 split_bilingual 保持一致：有任意 CJK 字符即判为 Cjk
        let line_langs: Vec<LineLang> = lines.iter().map(|l| classify_line(l)).collect();

        // 将 LineLang 映射为 LangClass 用于后续逻辑
        let to_lang_class = |ll: LineLang| match ll {
            LineLang::Cjk => LangClass::Cjk,
            LineLang::NonCjk => LangClass::Latin,
            LineLang::Neutral => LangClass::Other,
        };

        // 统计该条目中出现的不同语言类别（非 Other/Neutral）
        let distinct_langs: std::collections::HashSet<LangClass> = line_langs.iter()
            .map(|&ll| to_lang_class(ll))
            .filter(|&cl| cl != LangClass::Other)
            .collect();
        // 只有多行且含至少两种不同语言的条目才可能是双语
        if distinct_langs.len() >= 2 {
            bilingual_candidate_count += 1;
        }

        // 找到语言切换点：前面都是一种语言，后面是另一种语言
        let mut split_point: Option<usize> = None;
        let mut first_lang = LineLang::Neutral;

        // 找第一个非 Neutral 语言
        for &ll in line_langs.iter() {
            if ll != LineLang::Neutral {
                first_lang = ll;
                break;
            }
        }
        if first_lang == LineLang::Neutral {
            continue;
        }

        // 找切换点：第一个与 first_lang 不同的非 Neutral 行
        for (i, &ll) in line_langs.iter().enumerate() {
            if ll != LineLang::Neutral && ll != first_lang {
                split_point = Some(i);
                break;
            }
        }

        if let Some(sp) = split_point {
            // 验证：切换点之后的所有非 Neutral 行应该都是同一种语言
            let b_lang = line_langs[sp];
            let after_valid = line_langs[sp..].iter().all(|&ll| {
                ll == LineLang::Neutral || ll == b_lang
            });
            if after_valid && b_lang != LineLang::Neutral {
                matched_count += 1;
                lang_a_class = to_lang_class(first_lang);
                lang_b_class = to_lang_class(b_lang);
            }
        }
    }

    // 阈值基于"可能为双语的条目数"而非所有条目数
    // 原因：导出的 ASS 双语文件中，没有翻译的条目会以 Secondary 样式单独导出为纯英文条目，
    // 这些单语言条目（即使多行）不应拉低双语检测的匹配率
    let threshold_base = if bilingual_candidate_count > 0 { bilingual_candidate_count } else { total };
    let threshold = (threshold_base * 3) / 5; // 60% 的候选条目匹配才算双语

    if matched_count >= threshold && matched_count >= 3 {
        BilingualDetectResult {
            is_bilingual: true,
            split_mode: SplitMode::EvenFirst, // 拆分时不再用此字段，改用语言分块
            lang_a: lang_class_name(lang_a_class),
            lang_b: lang_class_name(lang_b_class),
            matched_count,
            total_count: total,
        }
    } else {
        BilingualDetectResult {
            is_bilingual: false,
            split_mode: SplitMode::EvenFirst,
            lang_a: "unknown".to_string(),
            lang_b: "unknown".to_string(),
            matched_count,
            total_count: total,
        }
    }
}

/// 从多个语言类别中取主导语言
#[allow(dead_code)]
fn dominant_lang(langs: &[LangClass]) -> LangClass {
    let mut counts: std::collections::HashMap<LangClass, usize> = std::collections::HashMap::new();
    for l in langs {
        *counts.entry(*l).or_insert(0) += 1;
    }
    counts.into_iter().max_by_key(|(_, v)| *v).unwrap().0
}

/// 语言类别转可读名称
fn lang_class_name(c: LangClass) -> String {
    match c {
        LangClass::Cjk => "cjk".to_string(),
        LangClass::Hiragana => "hiragana".to_string(),
        LangClass::Katakana => "katakana".to_string(),
        LangClass::Hangul => "hangul".to_string(),
        LangClass::Latin => "latin".to_string(),
        LangClass::Cyrillic => "cyrillic".to_string(),
        LangClass::Arabic => "arabic".to_string(),
        LangClass::Other => "other".to_string(),
    }
}

/// 行语言分类：CJK组、非CJK组、Neutral（无字母内容的空行/纯标签行）
#[derive(Clone, Copy, PartialEq)]
enum LineLang {
    Cjk,
    NonCjk,
    Neutral,
}

/// 判断一行的语言分类
fn classify_line(line: &str) -> LineLang {
    let stripped = strip_ass_tags(line);
    let mut has_cjk = false;
    let mut has_non_cjk = false;
    for c in stripped.chars() {
        if !c.is_alphabetic() {
            continue;
        }
        match classify_char(c) {
            LangClass::Cjk | LangClass::Hiragana | LangClass::Katakana | LangClass::Hangul => has_cjk = true,
            LangClass::Latin | LangClass::Cyrillic | LangClass::Arabic => has_non_cjk = true,
            _ => {}
        }
    }
    if has_cjk {
        LineLang::Cjk
    } else if has_non_cjk {
        LineLang::NonCjk
    } else {
        LineLang::Neutral
    }
}

/// 拆分双语字幕：按语言分块，前半部分（语言A）填入 text，后半部分（语言B）填入 translated
/// 算法：按 \N 分行，用三态分类（CJK/NonCjk/Neutral）找到语言切换的行边界。
/// Neutral 行（纯标签/空行）不参与切换判断，避免空行干扰。
/// 使用 CJK 存在性而非字符计数，避免中文行中短英文单词（如 Duh.）导致误判。
pub fn split_bilingual(file: &mut SubtitleFile, _split_mode: SplitMode) {
    for entry in &mut file.entries {
        if is_ass_drawing(&entry.text) {
            continue;
        }
        // \N → \n 统一换行
        let normalized = entry.text.replace("\\N", "\n").replace("\\n", "\n");

        // 优先用 {\rPrimary} / {\rSecondary} 标签拆分（本工具导出的 ASS 格式）
        if let Some((a, b)) = split_by_style_tags(&normalized) {
            let clean_a = clean_split_text(&a);
            let clean_b = clean_split_text(&b);
            // 两方都必须有实际文字内容（不能只是空白）
            // 用非空白判断，避免 ♪♪、[Zapping] 等音效/符号内容被误判为无文字
            let a_has_text = clean_a.chars().any(|c| !c.is_whitespace());
            let b_has_text = clean_b.chars().any(|c| !c.is_whitespace());
            if a_has_text && b_has_text {
                // 根据实际语言判断哪个是原文哪个是译文
                let a_has_cjk = has_cjk_chars(&clean_a);
                let b_has_cjk = has_cjk_chars(&clean_b);
                if a_has_cjk && !b_has_cjk {
                    // a = CJK（译文），b = 非CJK（原文）
                    entry.text = clean_b;
                    entry.translated = clean_a;
                } else if b_has_cjk && !a_has_cjk {
                    // b = CJK（译文），a = 非CJK（原文）
                    entry.text = clean_a;
                    entry.translated = clean_b;
                } else {
                    // 两者语言相同，按 Primary=译文、Secondary=原文 处理
                    entry.text = clean_b;
                    entry.translated = clean_a;
                }
                continue;
            }
            // Primary 为空（翻译失败）：Secondary 是原文，译文留空
            // 仅当 Secondary 不含 CJK 时才判定为未翻译（纯原文）；
            // 若 Secondary 含 CJK（如旧版 bug 导出的混合内容），继续尝试其他拆分方式
            if b_has_text && !a_has_text && !has_cjk_chars(&clean_b) {
                entry.text = clean_b;
                entry.translated = String::new();
                continue;
            }
            // Secondary 为空（不正常但防御性处理）
            if a_has_text && !b_has_text && !has_cjk_chars(&clean_a) {
                entry.text = clean_a;
                entry.translated = String::new();
                continue;
            }
        }

        let lines: Vec<&str> = normalized.lines().collect();
        if lines.len() < 2 {
            continue;
        }

        // 三态分类每行
        let line_langs: Vec<LineLang> = lines.iter().map(|l| classify_line(l)).collect();

        // 找第一个 CJK 行和第一个非 CJK 行（跳过 Neutral）
        let first_cjk = line_langs.iter().position(|&x| x == LineLang::Cjk);
        let first_non_cjk = line_langs.iter().position(|&x| x == LineLang::NonCjk);

        if first_cjk.is_none() || first_non_cjk.is_none() {
            continue; // 只有一种语言，无需拆分
        }

        let first_cjk = first_cjk.unwrap();
        let first_non_cjk = first_non_cjk.unwrap();

        // 确定切换方向：CJK 在前还是非 CJK 在前
        let (split_line, cjk_first) = if first_cjk < first_non_cjk {
            // CJK 在前：找最后一个 CJK 行，切换行 = 其后第一个非 Neutral 行
            let last_cjk = line_langs.iter().rposition(|&x| x == LineLang::Cjk).unwrap();
            // 找切换行：last_cjk 之后第一个 NonCjk 行
            let mut switch = None;
            for (i, &ll) in line_langs.iter().enumerate().skip(last_cjk + 1) {
                if ll == LineLang::NonCjk {
                    switch = Some(i);
                    break;
                }
                // Neutral 行可以跳过，但如果遇到 Cjk 则模式不纯
                if ll == LineLang::Cjk {
                    break;
                }
            }
            match switch {
                Some(sl) => (sl, true),
                None => continue,
            }
        } else {
            // 非 CJK 在前：找最后一个非 CJK 行，切换行 = 其后第一个 CJK 行
            let last_non_cjk = line_langs.iter().rposition(|&x| x == LineLang::NonCjk).unwrap();
            let mut switch = None;
            for (i, &ll) in line_langs.iter().enumerate().skip(last_non_cjk + 1) {
                if ll == LineLang::Cjk {
                    switch = Some(i);
                    break;
                }
                if ll == LineLang::NonCjk {
                    break;
                }
            }
            match switch {
                Some(sl) => (sl, false),
                None => {
                    // 标准模式失败（如 E-C-E-E：英文名+中文翻译+英文原文）
                    // 回退策略：找最后一个 CJK 行，其后第一个 NonCjk 行作为切换点
                    // translated = 直到最后一个 CJK 行，text = 其后部分
                    if let Some(last_cjk) = line_langs.iter().rposition(|&x| x == LineLang::Cjk) {
                        // 找 last_cjk 之后的第一个 NonCjk 行
                        let mut fallback_switch = None;
                        for (i, &ll) in line_langs.iter().enumerate().skip(last_cjk + 1) {
                            if ll == LineLang::NonCjk {
                                fallback_switch = Some(i);
                                break;
                            }
                        }
                        match fallback_switch {
                            Some(sl) => (sl, true), // cjk_first=true，因为 translated 在前
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                }
            }
        };

        // 在原文中找到切换行的起始位置
        let mut byte_offset = 0usize;
        for line in lines.iter().take(split_line) {
            byte_offset += line.len() + 1; // +1 for \n
        }
        // 往前找标签起始位置（如 {\rSecondary}）
        let orig_split = find_tag_start_before(&normalized, byte_offset);
        let a = &normalized[..orig_split];
        let b = &normalized[orig_split..];
        let clean_a = clean_split_text(a);
        let clean_b = clean_split_text(b);
        if !clean_a.is_empty() && !clean_b.is_empty() {
            // 始终把非 CJK（如英文）放 text（原文），CJK（如中文）放 translated（译文）
            // 与 en→zh 翻译方向一致，避免导出预览"译文在上"显示的是英文
            if cjk_first {
                // clean_a = CJK（中文），clean_b = 非CJK（英文）
                entry.text = clean_b;
                entry.translated = clean_a;
            } else {
                // clean_a = 非CJK（英文），clean_b = CJK（中文）
                entry.text = clean_a;
                entry.translated = clean_b;
            }
        }
    }
}

/// 检查字符串是否包含 CJK 字符
fn has_cjk_chars(s: &str) -> bool {
    s.chars().any(|c| {
        let code = c as u32;
        (0x4E00..=0x9FFF).contains(&code)
    })
}

/// 用 {\rPrimary} / {\rSecondary} 标签拆分双语字幕
/// 返回 (primary_content, secondary_content)
fn split_by_style_tags(normalized: &str) -> Option<(String, String)> {
    // 查找 {\rPrimary} 和 {\rSecondary} 标签
    let has_primary = normalized.contains("{\\rPrimary}");
    let has_secondary = normalized.contains("{\\rSecondary}");
    if !has_primary || !has_secondary {
        return None;
    }

    // 用标签作为分隔符，提取各标签后的内容
    // 策略：把 normalized 按 {\r...} 标签分段，收集每段内容和对应的标签
    let stripped = normalized.replace("\\N", "\n");
    let mut sections: Vec<(&str, String)> = Vec::new(); // (tag_type, content)
    let mut remaining = stripped.as_str();

    while !remaining.is_empty() {
        // 找下一个 {\r 标签
        let primary_pos = remaining.find("{\\rPrimary}");
        let secondary_pos = remaining.find("{\\rSecondary}");

        let (pos, tag_len, tag_type): (usize, usize, &str) = match (primary_pos, secondary_pos) {
            (Some(p), Some(s)) if p < s => (p, 11, "primary"),
            (Some(p), None) => (p, 11, "primary"),
            (Some(_), Some(s)) => (s, 13, "secondary"),
            (None, Some(s)) => (s, 13, "secondary"),
            (None, None) => break,
        };

        // 标签前的内容追加到上一个 section
        if pos > 0 {
            if let Some(last) = sections.last_mut() {
                last.1.push_str(&remaining[..pos]);
            }
        }

        // 跳过标签
        remaining = &remaining[pos + tag_len..];

        // 添加新 section
        sections.push((tag_type, String::new()));
    }

    // 循环结束后，把剩余内容追加到最后一个 section
    // （最后一个标签后的内容 otherwise 会丢失）
    if !remaining.is_empty() {
        if let Some(last) = sections.last_mut() {
            last.1.push_str(remaining);
        }
    }

    // 合并各 section 的内容
    let mut primary_content = String::new();
    let mut secondary_content = String::new();
    for (tag, content) in &sections {
        if *tag == "primary" {
            primary_content.push_str(content);
        } else {
            secondary_content.push_str(content);
        }
    }

    if primary_content.is_empty() && secondary_content.is_empty() {
        return None;
    }
    Some((primary_content, secondary_content))
}

/// 清理拆分后的字幕文本：剥离 ASS/HTML 标签，去除首尾多余换行和空白
fn clean_split_text(s: &str) -> String {
    let stripped = strip_ass_tags(s);
    let trimmed = stripped.trim_matches(|c: char| c == '\n' || c == '\r' || c.is_whitespace());
    trimmed.to_string()
}

// === 双语检测模块 END ===

/// 从 orig_pos 往回扫描，找到最近的标签开始符 { 或 < 的位置
/// 切分点选在标签开始处，这样标签及其内容完整归到后半部分
fn find_tag_start_before(text: &str, orig_pos: usize) -> usize {
    let bytes = text.as_bytes();
    // 先检查 orig_pos 自身是否就是标签起始
    if orig_pos < bytes.len() && (bytes[orig_pos] == b'{' || bytes[orig_pos] == b'<') {
        return orig_pos;
    }
    let mut pos = orig_pos;
    while pos > 0 {
        let b = bytes[pos - 1];
        if b == b'<' || b == b'{' {
            return pos - 1;
        }
        pos -= 1;
    }
    orig_pos
}

/// SRT 时间码解析：00:00:01,234 -> 1234 ms
fn parse_srt_timecode(s: &str) -> Result<i64, AppError> {
    let s = s.trim();
    // 格式：HH:MM:SS,mmm
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return Err(AppError::SubtitleParseFailed {
            path: "srt timecode".to_string(),
        });
    }
    let hours: i64 = parts[0].parse().map_err(|_| AppError::SubtitleParseFailed {
        path: "srt timecode hours".to_string(),
    })?;
    let minutes: i64 = parts[1].parse().map_err(|_| AppError::SubtitleParseFailed {
        path: "srt timecode minutes".to_string(),
    })?;
    let sec_parts: Vec<&str> = parts[2].split(',').collect();
    if sec_parts.len() != 2 {
        // 兼容用 . 代替 , 的情况
        let sec_parts: Vec<&str> = parts[2].split('.').collect();
        if sec_parts.len() != 2 {
            return Err(AppError::SubtitleParseFailed {
                path: "srt timecode seconds".to_string(),
            });
        }
        let seconds: i64 = sec_parts[0].parse().map_err(|_| AppError::SubtitleParseFailed {
            path: "srt timecode seconds".to_string(),
        })?;
        let millis: i64 = sec_parts[1].parse().map_err(|_| AppError::SubtitleParseFailed {
            path: "srt timecode millis".to_string(),
        })?;
        return Ok(hours * 3600000 + minutes * 60000 + seconds * 1000 + millis);
    }
    let seconds: i64 = sec_parts[0].parse().map_err(|_| AppError::SubtitleParseFailed {
        path: "srt timecode seconds".to_string(),
    })?;
    let millis: i64 = sec_parts[1].parse().map_err(|_| AppError::SubtitleParseFailed {
        path: "srt timecode millis".to_string(),
    })?;
    Ok(hours * 3600000 + minutes * 60000 + seconds * 1000 + millis)
}

/// 解析 SRT 字幕
pub fn parse_srt(content: &str) -> Result<SubtitleFile, AppError> {
    let mut entries = Vec::new();
    // 统一换行符，处理 Windows \r\n 和 Unix \n 混用情况
    let content = content.replace("\r\n", "\n").replace("\r", "\n");
    // SRT 以空行分隔条目
    let blocks: Vec<&str> = content.split("\n\n").collect();

    for (idx, block) in blocks.iter().enumerate() {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let lines: Vec<&str> = block.lines().collect();
        if lines.len() < 2 {
            continue;
        }

        // 第一行：序号（可忽略，用 idx 代替）
        // 第二行：时间码 00:00:01,234 --> 00:00:03,456
        // 第三行起：字幕文本

        // 找到包含 --> 的行
        let timecode_line_idx = lines.iter().position(|l| l.contains("-->"));
        let timecode_line_idx = match timecode_line_idx {
            Some(i) => i,
            None => continue,
        };

        let timecode_line = lines[timecode_line_idx];
        let time_parts: Vec<&str> = timecode_line.split("-->").collect();
        if time_parts.len() != 2 {
            continue;
        }

        let start_ms = parse_srt_timecode(time_parts[0].trim())?;
        let end_ms = parse_srt_timecode(time_parts[1].trim())?;

        // 字幕文本（时间码行之后的所有行）
        let text_lines: Vec<&str> = lines[(timecode_line_idx + 1)..].to_vec();
        let text = text_lines.join("\n");

        entries.push(SubtitleEntry {
            index: idx,
            start_ms,
            end_ms,
            text,
            translated: String::new(),
            style: None,
            failed: false,
            from_cache: false,
        });
    }

    Ok(SubtitleFile {
        format: SubtitleFormat::Srt,
        entries,
        raw_header: None,
        source_path: None,
        file_hash: String::new(),
    })
}

/// 渲染 SRT 字幕为文本
pub fn render_srt(file: &SubtitleFile) -> String {
    let mut output = String::new();
    for (i, entry) in file.entries.iter().enumerate() {
        output.push_str(&format!("{}\n", i + 1));
        output.push_str(&format!(
            "{} --> {}\n",
            format_srt_timecode(entry.start_ms),
            format_srt_timecode(entry.end_ms)
        ));
        // 优先输出译文，无译文输出原文
        let text = if entry.translated.is_empty() {
            &entry.text
        } else {
            &entry.translated
        };
        output.push_str(text);
        output.push_str("\n\n");
    }
    output
}

/// 格式化 SRT 时间码：1234 ms -> 00:00:01,234
fn format_srt_timecode(ms: i64) -> String {
    let total_seconds = ms / 1000;
    let millis = ms % 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        hours, minutes, seconds, millis
    )
}

// === SECTION 2 END ===

/// VTT 时间码解析：00:00:01.234 或 00:01.234 -> 1234 ms
fn parse_vtt_timecode(s: &str) -> Result<i64, AppError> {
    let s = s.trim();
    let parts: Vec<&str> = s.split(':').collect();
    let (hours, minutes, seconds_str) = match parts.len() {
        3 => {
            let h: i64 = parts[0].parse().map_err(|_| AppError::SubtitleParseFailed {
                path: "vtt timecode hours".to_string(),
            })?;
            let m: i64 = parts[1].parse().map_err(|_| AppError::SubtitleParseFailed {
                path: "vtt timecode minutes".to_string(),
            })?;
            (h, m, parts[2])
        }
        2 => {
            let m: i64 = parts[0].parse().map_err(|_| AppError::SubtitleParseFailed {
                path: "vtt timecode minutes".to_string(),
            })?;
            (0, m, parts[1])
        }
        _ => {
            return Err(AppError::SubtitleParseFailed {
                path: "vtt timecode format".to_string(),
            })
        }
    };

    let sec_parts: Vec<&str> = seconds_str.split('.').collect();
    if sec_parts.len() != 2 {
        return Err(AppError::SubtitleParseFailed {
            path: "vtt timecode seconds".to_string(),
        });
    }
    let seconds: i64 = sec_parts[0].parse().map_err(|_| AppError::SubtitleParseFailed {
        path: "vtt timecode seconds".to_string(),
    })?;
    let millis: i64 = sec_parts[1].parse().map_err(|_| AppError::SubtitleParseFailed {
        path: "vtt timecode millis".to_string(),
    })?;
    Ok(hours * 3600000 + minutes * 60000 + seconds * 1000 + millis)
}

/// 解析 WebVTT 字幕
pub fn parse_vtt(content: &str) -> Result<SubtitleFile, AppError> {
    let mut entries = Vec::new();
    let mut header_end = 0;
    // 统一换行符
    let content = content.replace("\r\n", "\n").replace("\r", "\n");

    // 跳过 WEBVTT 头部
    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            header_end = i + 1;
            break;
        }
    }

    let body: String = content.lines().skip(header_end).collect::<Vec<_>>().join("\n");
    let blocks: Vec<&str> = body.split("\n\n").collect();

    for (idx, block) in blocks.iter().enumerate() {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let lines: Vec<&str> = block.lines().collect();
        if lines.is_empty() {
            continue;
        }

        // 找到包含 --> 的行
        let timecode_line_idx = lines.iter().position(|l| l.contains("-->"));
        let timecode_line_idx = match timecode_line_idx {
            Some(i) => i,
            None => continue,
        };

        let timecode_line = lines[timecode_line_idx];
        // VTT 时间码可能带位置信息：00:00:01.234 --> 00:00:03.456 align:start position:0%
        let time_parts: Vec<&str> = timecode_line.split("-->").collect();
        if time_parts.len() != 2 {
            continue;
        }

        let start_ms = parse_vtt_timecode(time_parts[0].trim())?;
        // end 部分可能带空格分隔的设置项，取第一个
        let end_str = time_parts[1].split_whitespace().next().unwrap_or("");
        let end_ms = parse_vtt_timecode(end_str)?;

        let text_lines: Vec<&str> = lines[(timecode_line_idx + 1)..].to_vec();
        let text = text_lines.join("\n");

        entries.push(SubtitleEntry {
            index: idx,
            start_ms,
            end_ms,
            text,
            translated: String::new(),
            style: None,
            failed: false,
            from_cache: false,
        });
    }

    Ok(SubtitleFile {
        format: SubtitleFormat::Vtt,
        entries,
        raw_header: None,
        source_path: None,
        file_hash: String::new(),
    })
}

/// 渲染 WebVTT 字幕为文本
pub fn render_vtt(file: &SubtitleFile) -> String {
    let mut output = String::from("WEBVTT\n\n");
    for (i, entry) in file.entries.iter().enumerate() {
        output.push_str(&format!("{}\n", i + 1));
        output.push_str(&format!(
            "{} --> {}\n",
            format_vtt_timecode(entry.start_ms),
            format_vtt_timecode(entry.end_ms)
        ));
        let text = if entry.translated.is_empty() {
            &entry.text
        } else {
            &entry.translated
        };
        output.push_str(text);
        output.push_str("\n\n");
    }
    output
}

/// 格式化 VTT 时间码：1234 ms -> 00:00:01.234
fn format_vtt_timecode(ms: i64) -> String {
    let total_seconds = ms / 1000;
    let millis = ms % 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        hours, minutes, seconds, millis
    )
}

// === SECTION 3 END ===

/// 解析 ASS 字幕（使用 ass-core）
/// 一期策略：保留原始 ass 文本，按 Dialogue 行提取条目
/// 样式标记保留在 text 字段中，翻译时由占位符保护算法处理
pub fn parse_ass(content: &str) -> Result<SubtitleFile, AppError> {
    // 使用 ass-core 解析以验证格式合法性
    // 若 ass-core 0.1.1 API 不稳定，回退到自写行解析
    let script = match ass_core::Script::parse(content) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("ass-core 解析失败，回退到自写解析: {}", e);
            return parse_ass_fallback(content);
        }
    };

    // 提取 [Script Info] + [V4+ Styles] 作为 raw_header
    let mut header = String::new();
    let mut in_events = false;
    let mut entries = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if trimmed.eq_ignore_ascii_case("[events]") {
                in_events = true;
                header.push_str(line);
                header.push('\n');
                continue;
            } else if in_events {
                in_events = false;
            }
        }

        if !in_events {
            header.push_str(line);
            header.push('\n');
        } else {
            // Events 区域
            let lower = trimmed.to_lowercase();
            if lower.starts_with("dialogue:") {
                let dialogue_content = &trimmed[9..];
                let fields: Vec<&str> = dialogue_content.split(',').collect();
                // ASS Dialogue 格式：Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
                // Text 字段可能含逗号，所以取第 9 个字段之后的所有内容
                if fields.len() < 10 {
                    continue;
                }
                let start_ms = parse_ass_timecode(fields[1].trim());
                let end_ms = parse_ass_timecode(fields[2].trim());
                let style = fields[3].trim().to_string();
                // Text 是第 10 个字段起（index 9），合并剩余部分
                let text = fields[9..].join(",");

                // 跳过 ass 非字幕条目（绘图指令/LOGO/水印/特效等）
                if is_non_subtitle_ass_entry(&text, &style) {
                    continue;
                }

                entries.push(SubtitleEntry {
                    index: entries.len(),
                    start_ms,
                    end_ms,
                    text,
                    translated: String::new(),
                    style: Some(style),
                    failed: false,
                    from_cache: false,
                });
            }
        }
    }

    // 使用 script 变量避免未使用警告（ass-core 解析成功说明格式合法）
    let _ = script;

    Ok(SubtitleFile {
        format: SubtitleFormat::Ass,
        entries,
        raw_header: Some(header),
        source_path: None,
        file_hash: String::new(),
    })
}

/// ASS 时间码解析：0:00:01.23 -> 1234 ms
fn parse_ass_timecode(s: &str) -> i64 {
    let s = s.trim();
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return 0;
    }
    let hours: i64 = parts[0].parse().unwrap_or(0);
    let minutes: i64 = parts[1].parse().unwrap_or(0);
    let sec_parts: Vec<&str> = parts[2].split('.').collect();
    if sec_parts.len() != 2 {
        return hours * 3600000 + minutes * 60000;
    }
    let seconds: i64 = sec_parts[0].parse().unwrap_or(0);
    let centis: i64 = sec_parts[1].parse().unwrap_or(0);
    // ASS 时间码精度为厘秒（1/100 秒）
    hours * 3600000 + minutes * 60000 + seconds * 1000 + centis * 10
}

/// 格式化 ASS 时间码：1234 ms -> 0:00:01.23
fn format_ass_timecode(ms: i64) -> String {
    let total_seconds = ms / 1000;
    let centis = (ms % 1000) / 10;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{}:{:02}:{:02}.{:02}", hours, minutes, seconds, centis)
}

/// ASS 解析回退（ass-core 失败时使用）
fn parse_ass_fallback(content: &str) -> Result<SubtitleFile, AppError> {
    let mut header = String::new();
    let mut in_events = false;
    let mut entries = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if trimmed.eq_ignore_ascii_case("[events]") {
                in_events = true;
            } else if in_events {
                in_events = false;
            }
        }

        if !in_events {
            header.push_str(line);
            header.push('\n');
        } else {
            let lower = trimmed.to_lowercase();
            if lower.starts_with("dialogue:") {
                let dialogue_content = &trimmed[9..];
                let fields: Vec<&str> = dialogue_content.split(',').collect();
                if fields.len() < 10 {
                    continue;
                }
                let start_ms = parse_ass_timecode(fields[1].trim());
                let end_ms = parse_ass_timecode(fields[2].trim());
                let style = fields[3].trim().to_string();
                let text = fields[9..].join(",");

                // 跳过 ass 非字幕条目（绘图指令/LOGO/水印/特效等）
                if is_non_subtitle_ass_entry(&text, &style) {
                    continue;
                }

                entries.push(SubtitleEntry {
                    index: entries.len(),
                    start_ms,
                    end_ms,
                    text,
                    translated: String::new(),
                    style: Some(style),
                    failed: false,
                    from_cache: false,
                });
            }
        }
    }

    Ok(SubtitleFile {
        format: SubtitleFormat::Ass,
        entries,
        raw_header: Some(header),
        source_path: None,
        file_hash: String::new(),
    })
}

/// 渲染 ASS 字幕为文本（保留原始 header + 样式）
pub fn render_ass(file: &SubtitleFile) -> String {
    let mut output = String::new();

    // 输出 header（[Script Info] + [V4+ Styles]）
    if let Some(header) = &file.raw_header {
        output.push_str(header);
        if !output.ends_with('\n') {
            output.push('\n');
        }
    } else {
        // 没有 header（如 SRT 转 ASS），生成最小 ASS header
        output.push_str("[Script Info]\n");
        output.push_str("ScriptType: v4.00+\n");
        output.push_str("PlayResX: 1920\n");
        output.push_str("PlayResY: 1080\n");
        output.push('\n');
        output.push_str("[V4+ Styles]\n");
        output.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n");
        output.push_str("Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,30,1\n");
        output.push('\n');
    }

    // 输出 [Events] 区域
    output.push_str("[Events]\n");
    output.push_str("Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");

    for entry in &file.entries {
        let text = if entry.translated.is_empty() {
            &entry.text
        } else {
            &entry.translated
        };
        // ASS 文本中换行符必须用 \N 标记，否则换行会被 parse 当成新行
        let text = text.replace('\n', "\\N").replace('\r', "");
        let style = entry.style.as_deref().unwrap_or("Default");
        output.push_str(&format!(
            "Dialogue: 0,{},{},{},,0,0,0,,{}\n",
            format_ass_timecode(entry.start_ms),
            format_ass_timecode(entry.end_ms),
            style,
            text
        ));
    }

    output
}

// === SECTION 4 END ===

/// 编码探测与解码
/// 顺序：BOM-UTF-8 → UTF-8 → chardetng 自动检测 → fallback Latin-1
pub fn decode_bytes(bytes: &[u8]) -> Result<(String, String), AppError> {
    // 1. BOM 检测
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let decoded = String::from_utf8_lossy(&bytes[3..]).to_string();
        return Ok((decoded, "utf-8-bom".to_string()));
    }

    // 2. 尝试严格 UTF-8
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Ok((s.to_string(), "utf-8".to_string()));
    }

    // 3. chardetng 自动检测
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let (encoding, confident) = detector.guess_assess(None, true);

    let decoded = encoding.decode(bytes).0.to_string();
    let encoding_name = encoding.name().to_string();

    if !confident {
        tracing::warn!("编码探测置信度低: {}", encoding_name);
    }

    Ok((decoded, encoding_name))
}

/// 按文件扩展名判断字幕格式
pub fn detect_format(path: &str) -> Result<SubtitleFormat, AppError> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "srt" => Ok(SubtitleFormat::Srt),
        "vtt" => Ok(SubtitleFormat::Vtt),
        "ass" => Ok(SubtitleFormat::Ass),
        "ssa" => Ok(SubtitleFormat::Ssa),
        _ => Err(AppError::SubtitleFormatUnsupported {
            codec: ext,
        }),
    }
}

/// 计算字幕内容 hash（sha256，hex 编码）
/// 基于所有条目的 index + start_ms + end_ms + text 拼接计算，
/// 确保不同字幕（即使含相同句子）hash 不同，用于翻译缓存隔离。
/// 不含 translated 字段（翻译前后 hash 不变），不含 source_path（文件可移动）。
pub fn compute_subtitle_hash(entries: &[SubtitleEntry]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for e in entries {
        hasher.update(e.index.to_le_bytes());
        hasher.update(e.start_ms.to_le_bytes());
        hasher.update(e.end_ms.to_le_bytes());
        hasher.update(e.text.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// 统一解析入口：按格式解析字幕文件
pub fn parse_subtitle(content: &str, format: &SubtitleFormat) -> Result<SubtitleFile, AppError> {
    let mut file = match format {
        SubtitleFormat::Srt => parse_srt(content)?,
        SubtitleFormat::Vtt => parse_vtt(content)?,
        SubtitleFormat::Ass | SubtitleFormat::Ssa => parse_ass(content)?,
    };
    file.file_hash = compute_subtitle_hash(&file.entries);
    Ok(file)
}

/// 统一渲染入口：按格式渲染字幕文件
pub fn render_subtitle(file: &SubtitleFile) -> String {
    match file.format {
        SubtitleFormat::Srt => render_srt(file),
        SubtitleFormat::Vtt => render_vtt(file),
        SubtitleFormat::Ass | SubtitleFormat::Ssa => render_ass(file),
    }
}

/// 从文件路径加载字幕（含编码探测）
pub fn load_subtitle_file(path: &str) -> Result<SubtitleFile, AppError> {
    use std::path::Path;

    if !Path::new(path).exists() {
        return Err(AppError::FileNotFound {
            path: path.to_string(),
        });
    }

    let bytes = std::fs::read(path).map_err(AppError::Io)?;
    let (content, encoding) = decode_bytes(&bytes)?;

    let format = detect_format(path)?;
    let mut file = parse_subtitle(&content, &format)?;
    file.source_path = Some(path.to_string());

    if encoding != "utf-8" && encoding != "utf-8-bom" {
        tracing::info!("字幕文件 {} 编码: {}", path, encoding);
    }

    Ok(file)
}

/// 保存字幕到文件（统一 UTF-8 无 BOM）
pub fn save_subtitle_file(file: &SubtitleFile, path: &str) -> Result<(), AppError> {
    let content = render_subtitle(file);
    std::fs::write(path, content).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => AppError::PermissionDenied {
            path: path.to_string(),
        },
        _ => AppError::StorageWriteFailed {
            path: path.to_string(),
        },
    })?;
    tracing::info!("字幕已保存: {}", path);
    Ok(())
}

use std::path::Path;

// === SECTION 5 END ===

// === 导出弹层相关（export-dialog-plan.md §2/§4） ===

/// 导出模式
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportMode {
    Monolingual,
    Bilingual,
}

/// 导出选项（前端 ExportDialog 组装后通过 IPC 传入）
#[derive(Debug, Clone, Deserialize)]
pub struct ExportOptions {
    pub format: SubtitleFormat,
    pub mode: ExportMode,
    /// 单语模式：输出哪种语言（"source" | "translated"）
    pub monolingual_lang: Option<String>,
    /// 双语模式：true=译文在上，false=原文在上
    pub bilingual_translated_first: Option<bool>,
    /// ASS 双语样式（仅 format=ass 且 mode=bilingual 时生效）
    pub ass_style: Option<AssBilingualStyle>,
    /// 视频实际宽度（像素），用于 ASS PlayResX，缺省 1280
    pub video_width: Option<u32>,
    /// 视频实际高度（像素），用于 ASS PlayResY，缺省 720
    pub video_height: Option<u32>,
}

/// ASS 双语样式配置
#[derive(Debug, Clone, Deserialize)]
pub struct AssBilingualStyle {
    pub primary_font_size: u32,
    pub secondary_font_size: u32,
    /// ASS BGR 格式 &HBBGGRR&
    pub primary_color: String,
    pub secondary_color: String,
    pub primary_bold: bool,
    pub primary_italic: bool,
    pub primary_underline: bool,
    pub secondary_bold: bool,
    pub secondary_italic: bool,
    pub secondary_underline: bool,
    /// 描边宽度
    pub outline: u32,
    /// 描边颜色，ASS BGR 格式 &HBBGGRR&
    pub outline_color: String,
    /// 阴影深度
    pub shadow: u32,
    /// 阴影颜色，ASS BGR 格式 &HBBGGRR&
    pub shadow_color: String,
}

impl Default for AssBilingualStyle {
    fn default() -> Self {
        Self {
            primary_font_size: 48,
            secondary_font_size: 30,
            primary_color: "&HFFFFFF&".into(),
            secondary_color: "&HCCCCCC&".into(),
            primary_bold: false,
            primary_italic: false,
            primary_underline: false,
            secondary_bold: false,
            secondary_italic: false,
            secondary_underline: false,
            outline: 2,
            outline_color: "&H000000&".into(),
            shadow: 1,
            shadow_color: "&H000000&".into(),
        }
    }
}

/// 导出入口：按选项渲染并返回字幕文本
pub fn export_subtitle(file: &SubtitleFile, options: &ExportOptions) -> String {
    match options.format {
        SubtitleFormat::Srt => render_srt_with_options(file, options),
        SubtitleFormat::Vtt => render_vtt_with_options(file, options),
        SubtitleFormat::Ass | SubtitleFormat::Ssa => render_ass_with_options(file, options),
    }
}

/// 导出入口：按选项渲染并写入文件
/// SRT 加 UTF-8 BOM（兼容部分老播放器识别中文 SRT）；ASS/VTT 不加 BOM
pub fn export_subtitle_file(
    file: &SubtitleFile,
    path: &str,
    options: &ExportOptions,
) -> Result<(), AppError> {
    let content = export_subtitle(file, options);
    let bytes: Vec<u8> = if matches!(options.format, SubtitleFormat::Srt) {
        let mut v = vec![0xEF, 0xBB, 0xBF];
        v.extend_from_slice(content.as_bytes());
        v
    } else {
        content.into_bytes()
    };
    std::fs::write(path, bytes).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => AppError::PermissionDenied {
            path: path.to_string(),
        },
        _ => AppError::StorageWriteFailed {
            path: path.to_string(),
        },
    })?;
    tracing::info!("字幕已导出: {} ({:?})", path, options.format);
    Ok(())
}

/// 按导出模式拼装单条字幕文本（SRT/VTT 共用）
fn build_entry_text(entry: &SubtitleEntry, options: &ExportOptions) -> String {
    match options.mode {
        ExportMode::Monolingual => {
            let lang = options.monolingual_lang.as_deref().unwrap_or("translated");
            if lang == "source" {
                entry.text.clone()
            } else {
                entry.translated.clone()
            }
        }
        ExportMode::Bilingual => {
            let first = options.bilingual_translated_first.unwrap_or(true);
            // 合并内部换行为空格，避免多行译文/原文导致重新导入时语言检测混乱
            let collapse = |s: &str| -> String {
                cleanup_cjk_spaces(
                    &strip_inline_ass_and_html_tags(s)
                        .replace('\n', " ")
                        .replace("\\N", " ")
                        .trim()
                        .to_string(),
                )
                .to_string()
            };
            let (top, bottom) = if first {
                (collapse(&entry.translated), collapse(&entry.text))
            } else {
                (collapse(&entry.text), collapse(&entry.translated))
            };
            // 翻译失败的条目只输出一行原文
            // 失败判定：entry.failed 标记、译文为空、译文与原文相同、
            // 译文和原文都无 CJK（split_bilingual 无法区分语言→无法拆分）、
            // 音效标记类型不一致（AI 错位翻译，如原文非音效但译文是音效）
            // 这样重新导入时 split_bilingual 无法拆分（单行），translated 保持空 → 正确显示未翻译
            let both_no_cjk = !has_cjk_chars(&top) && !has_cjk_chars(&bottom);
            let sound_mismatch = looks_like_sound_effect(&top) != looks_like_sound_effect(&bottom);
            if entry.failed || top.is_empty() || top == bottom || both_no_cjk || sound_mismatch {
                bottom.clone()
            } else if bottom.is_empty() {
                top.clone()
            } else {
                format!("{}\n{}", top, bottom)
            }
        }
    }
}

/// SRT 渲染（带导出选项）
fn render_srt_with_options(file: &SubtitleFile, options: &ExportOptions) -> String {
    let mut output = String::new();
    for (i, entry) in file.entries.iter().enumerate() {
        output.push_str(&format!("{}\n", i + 1));
        output.push_str(&format!(
            "{} --> {}\n",
            format_srt_timecode(entry.start_ms),
            format_srt_timecode(entry.end_ms)
        ));
        output.push_str(&build_entry_text(entry, options));
        output.push_str("\n\n");
    }
    output
}

/// VTT 渲染（带导出选项）
fn render_vtt_with_options(file: &SubtitleFile, options: &ExportOptions) -> String {
    let mut output = String::from("WEBVTT\n\n");
    for (i, entry) in file.entries.iter().enumerate() {
        output.push_str(&format!("{}\n", i + 1));
        output.push_str(&format!(
            "{} --> {}\n",
            format_vtt_timecode(entry.start_ms),
            format_vtt_timecode(entry.end_ms)
        ));
        output.push_str(&build_entry_text(entry, options));
        output.push_str("\n\n");
    }
    output
}

// === SECTION 6 END ===

/// ASS 渲染（带导出选项）—— 不复用 raw_header，生成新头部注入用户样式
fn render_ass_with_options(file: &SubtitleFile, options: &ExportOptions) -> String {
    let style = options.ass_style.clone().unwrap_or_default();
    let mut s = String::new();

    // [Script Info]
    // PlayResX/PlayResY 跟随视频实际分辨率，避免播放器按 1280 排版导致提前换行
    let play_res_x = options.video_width.unwrap_or(1280);
    let play_res_y = options.video_height.unwrap_or(720);
    s.push_str("[Script Info]\n");
    s.push_str("Title: AI-SubTrans Export\n");
    s.push_str("ScriptType: v4.00+\n");
    s.push_str(&format!("PlayResX: {}\n", play_res_x));
    s.push_str(&format!("PlayResY: {}\n", play_res_y));
    s.push_str("WrapStyle: 0\n\n");

    // [V4+ Styles]
    // 单语：定义 Default 样式
    // 双语：定义 Primary（第一行）+ Secondary（第二行）+ Default（兜底）
    s.push_str("[V4+ Styles]\n");
    s.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, ");
    s.push_str("OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ");
    s.push_str("ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, ");
    s.push_str("Alignment, MarginL, MarginR, MarginV, Encoding\n");

    match options.mode {
        ExportMode::Monolingual => {
            s.push_str(&format_style_line(
                "Default", 48, "&HFFFFFF&", false, false, false, 2, 1,
                "&H000000&", "&H000000&",
            ));
        }
        ExportMode::Bilingual => {
            s.push_str(&format_style_line(
                "Primary",
                style.primary_font_size,
                &style.primary_color,
                style.primary_bold,
                style.primary_italic,
                style.primary_underline,
                style.outline,
                style.shadow,
                &style.outline_color,
                &style.shadow_color,
            ));
            s.push_str(&format_style_line(
                "Secondary",
                style.secondary_font_size,
                &style.secondary_color,
                style.secondary_bold,
                style.secondary_italic,
                style.secondary_underline,
                style.outline,
                style.shadow,
                &style.outline_color,
                &style.shadow_color,
            ));
            // Default 兜底样式（部分播放器/工具要求 Default 存在），用 Primary 参数
            s.push_str(&format_style_line(
                "Default",
                style.primary_font_size,
                &style.primary_color,
                style.primary_bold,
                style.primary_italic,
                style.primary_underline,
                style.outline,
                style.shadow,
                &style.outline_color,
                &style.shadow_color,
            ));
        }
    }
    s.push('\n');

    // [Events]
    s.push_str("[Events]\n");
    s.push_str("Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");

    for entry in &file.entries {
        match options.mode {
            ExportMode::Monolingual => {
                let text = if options.monolingual_lang.as_deref().unwrap_or("translated") == "source" {
                    &entry.text
                } else {
                    &entry.translated
                };
                let text = normalize_ass_newline(&cleanup_cjk_spaces(&strip_inline_ass_and_html_tags(text)));
                s.push_str(&format!(
                    "Dialogue: 0,{},{},Default,,0,0,0,,{}\n",
                    format_ass_timecode(entry.start_ms),
                    format_ass_timecode(entry.end_ms),
                    text
                ));
            }
            ExportMode::Bilingual => {
                // 单条 Dialogue + \N 换行，第一行套 Primary，第二行套 Secondary
                let (first, second) = if options.bilingual_translated_first.unwrap_or(true) {
                    (&entry.translated, &entry.text)
                } else {
                    (&entry.text, &entry.translated)
                };
                // 剥离标签后，把内部换行合并为空格（避免 \N 与语言块分隔 \N 混淆，
                // 导致重新导入时 split_bilingual 无法正确识别语言切换点）
                let first = cleanup_cjk_spaces(&strip_inline_ass_and_html_tags(first).replace('\n', " ").replace("\\N", " "));
                let second = cleanup_cjk_spaces(&strip_inline_ass_and_html_tags(second).replace('\n', " ").replace("\\N", " "));
                let first_trim = first.trim();
                let second_trim = second.trim();
                // 翻译失败的条目：输出空的 Primary + Secondary 原文
                // 失败判定：entry.failed 标记、译文为空、译文与原文相同、
                // 译文和原文都无 CJK（split_bilingual 无法区分语言→无法拆分）、
                // 音效标记类型不一致（AI 错位翻译）
                // 重新导入时 split_by_style_tags 识别 Primary 为空 → translated 留空 → 正确显示未翻译
                let both_no_cjk = !has_cjk_chars(&first) && !has_cjk_chars(&second);
                let sound_mismatch = looks_like_sound_effect(first_trim) != looks_like_sound_effect(second_trim);
                let is_failed = entry.failed || first_trim.is_empty() || first_trim == second_trim || both_no_cjk || sound_mismatch;
                let text = if first_trim.is_empty() && second_trim.is_empty() {
                    String::new()
                } else if is_failed {
                    // 翻译失败：输出空的 Primary + Secondary 原文
                    format!("{{\\rPrimary}}\\N{{\\rSecondary}}{}", second)
                } else {
                    // 双语：\N 放在 override block 外部，避免重新加载时语言检测失败
                    format!(
                        "{{\\rPrimary}}{}\\N{{\\rSecondary}}{}",
                        first,
                        second
                    )
                };
                let style_name = if is_failed {
                    "Secondary"
                } else {
                    "Primary"
                };
                s.push_str(&format!(
                    "Dialogue: 0,{},{},{},,0,0,0,,{}\n",
                    format_ass_timecode(entry.start_ms),
                    format_ass_timecode(entry.end_ms),
                    style_name,
                    text
                ));
            }
        }
    }
    s
}

/// ASS Style 行字段顺序（23 个）：
/// Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour,
/// Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle,
/// Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
///
/// 必须写齐 3 个颜色占位（SecondaryColour/OutlineColour/BackColour），否则从 Bold 起字段错位。
/// Bold/Italic/Underline 用 -1（真）/ 0（假），符合 ASS 标准（部分播放器只认 -1）。
/// outline_color → OutlineColour（描边颜色），shadow_color → BackColour（阴影颜色）
fn format_style_line(
    name: &str,
    size: u32,
    color: &str,
    bold: bool,
    italic: bool,
    underline: bool,
    outline: u32,
    shadow: u32,
    outline_color: &str,
    shadow_color: &str,
) -> String {
    fn b(v: bool) -> i32 {
        if v { -1 } else { 0 }
    }
    format!(
        "Style: {},{},{},{},&H000000&,{},{},{},{},{},0,100,100,0,0,1,{},{},2,10,10,40,1\n",
        name,
        "Arial",
        size,
        color,
        outline_color,
        shadow_color,
        b(bold),
        b(italic),
        b(underline),
        outline,
        shadow
    )
}

/// 导出 ASS 时剥离条目内已有的 ASS/HTML 内联样式标记，避免覆盖用户在导出弹层里配置的统一样式。
/// 例如：源字幕里的 `{\r60字体 美剧 中文}`、`<font size="24">` 等会导致播放器最终效果与预览严重不一致。
fn strip_inline_ass_and_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_brace = false;
    let mut in_html = false;
    for c in s.chars() {
        if c == '{' {
            in_brace = true;
        } else if c == '}' {
            in_brace = false;
        } else if c == '<' {
            in_html = true;
        } else if c == '>' {
            in_html = false;
        } else if !in_brace && !in_html {
            out.push(c);
        }
    }
    out
}

/// 把硬换行（\r\n / \n / \r）统一转为 ASS 的 \N。
/// 注意：导出链路会先调用 strip_inline_ass_and_html_tags 剥离源条目内联样式标记，
/// 这里仅做换行标准化。
fn normalize_ass_newline(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\\N")
}

// === SECTION 7 END ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_srt_basic() {
        let content = "1\n00:00:01,000 --> 00:00:03,000\nHello World\n\n2\n00:00:04,000 --> 00:00:06,000\nThis is a test\n";
        let file = parse_srt(content).unwrap();
        assert_eq!(file.format, SubtitleFormat::Srt);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].start_ms, 1000);
        assert_eq!(file.entries[0].end_ms, 3000);
        assert_eq!(file.entries[0].text, "Hello World");
        assert_eq!(file.entries[1].start_ms, 4000);
        assert_eq!(file.entries[1].text, "This is a test");
    }

    #[test]
    fn test_parse_srt_multiline() {
        let content = "1\n00:00:01,000 --> 00:00:03,000\nLine 1\nLine 2\n";
        let file = parse_srt(content).unwrap();
        assert_eq!(file.entries[0].text, "Line 1\nLine 2");
    }

    #[test]
    fn test_render_srt_roundtrip() {
        let content = "1\n00:00:01,000 --> 00:00:03,000\nHello World\n\n";
        let file = parse_srt(content).unwrap();
        let rendered = render_srt(&file);
        let reparsed = parse_srt(&rendered).unwrap();
        assert_eq!(file.entries.len(), reparsed.entries.len());
        assert_eq!(file.entries[0].text, reparsed.entries[0].text);
        assert_eq!(file.entries[0].start_ms, reparsed.entries[0].start_ms);
    }

    #[test]
    fn test_parse_srt_timecode_dot() {
        // 兼容用 . 代替 , 的情况
        let content = "1\n00:00:01.000 --> 00:00:03.000\nTest\n";
        let file = parse_srt(content).unwrap();
        assert_eq!(file.entries[0].start_ms, 1000);
    }

    #[test]
    fn test_parse_vtt_basic() {
        let content = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:03.000\nHello World\n\n2\n00:00:04.000 --> 00:00:06.000\nTest\n";
        let file = parse_vtt(content).unwrap();
        assert_eq!(file.format, SubtitleFormat::Vtt);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].start_ms, 1000);
        assert_eq!(file.entries[0].text, "Hello World");
    }

    #[test]
    fn test_parse_vtt_with_settings() {
        let content = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:03.000 align:start position:0%\nHello\n";
        let file = parse_vtt(content).unwrap();
        assert_eq!(file.entries[0].start_ms, 1000);
        assert_eq!(file.entries[0].end_ms, 3000);
    }

    #[test]
    fn test_parse_vtt_short_timecode() {
        // VTT 支持省略小时：00:01.234
        let content = "WEBVTT\n\n1\n00:01.000 --> 00:03.000\nHello\n";
        let file = parse_vtt(content).unwrap();
        assert_eq!(file.entries[0].start_ms, 1000);
        assert_eq!(file.entries[0].end_ms, 3000);
    }

    #[test]
    fn test_parse_ass_basic() {
        let content = "[Script Info]\nTitle: Test\nScriptType: v4.00+\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize\nStyle: Default,Arial,20\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,Hello World\nDialogue: 0,0:00:04.00,0:00:06.00,Default,,0,0,0,,This is a test\n";
        let file = parse_ass(content).unwrap();
        assert_eq!(file.format, SubtitleFormat::Ass);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].start_ms, 1000);
        assert_eq!(file.entries[0].end_ms, 3000);
        assert_eq!(file.entries[0].text, "Hello World");
        assert_eq!(file.entries[0].style.as_deref(), Some("Default"));
        assert!(file.raw_header.is_some());
    }

    #[test]
    fn test_parse_ass_with_style_tags() {
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\an8}{\\b1}Bold top text\n";
        let file = parse_ass(content).unwrap();
        assert_eq!(file.entries[0].text, "{\\an8}{\\b1}Bold top text");
    }

    #[test]
    fn test_render_ass_roundtrip() {
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,Hello World\n";
        let file = parse_ass(content).unwrap();
        let rendered = render_ass(&file);
        let reparsed = parse_ass(&rendered).unwrap();
        assert_eq!(file.entries.len(), reparsed.entries.len());
        assert_eq!(file.entries[0].text, reparsed.entries[0].text);
        assert_eq!(file.entries[0].start_ms, reparsed.entries[0].start_ms);
    }

    #[test]
    fn test_ass_timecode_parse() {
        assert_eq!(parse_ass_timecode("0:00:01.00"), 1000);
        assert_eq!(parse_ass_timecode("0:00:01.50"), 1500);
        assert_eq!(parse_ass_timecode("1:02:03.45"), 3723450);
        assert_eq!(parse_ass_timecode("0:00:00.00"), 0);
    }

    #[test]
    fn test_ass_timecode_format() {
        assert_eq!(format_ass_timecode(1000), "0:00:01.00");
        assert_eq!(format_ass_timecode(1500), "0:00:01.50");
        assert_eq!(format_ass_timecode(0), "0:00:00.00");
    }

    #[test]
    fn test_decode_utf8() {
        let bytes = "Hello World".as_bytes();
        let (decoded, encoding) = decode_bytes(bytes).unwrap();
        assert_eq!(decoded, "Hello World");
        assert_eq!(encoding, "utf-8");
    }

    #[test]
    fn test_decode_utf8_bom() {
        let bytes = [0xEF, 0xBB, 0xBF, b'H', b'i'];
        let (decoded, encoding) = decode_bytes(&bytes).unwrap();
        assert_eq!(decoded, "Hi");
        assert_eq!(encoding, "utf-8-bom");
    }

    #[test]
    fn test_decode_gbk() {
        // "你好世界，这是一个测试" 的 GBK 编码（长文本提高 chardetng 准确率）
        let bytes = [
            0xC4, 0xE3, 0xBA, 0xC3, 0xCA, 0xC0, 0xBD, 0xE7, 0xA3, 0xAC, 0xD5, 0xE2, 0xCA, 0xC7,
            0xD2, 0xBB, 0xB8, 0xF6, 0xB2, 0xE2, 0xCA, 0xD4,
        ];
        let (decoded, _encoding) = decode_bytes(&bytes).unwrap();
        assert_eq!(decoded, "你好世界，这是一个测试");
    }

    #[test]
    fn test_detect_format() {
        assert_eq!(detect_format("test.srt").unwrap(), SubtitleFormat::Srt);
        assert_eq!(detect_format("test.vtt").unwrap(), SubtitleFormat::Vtt);
        assert_eq!(detect_format("test.ass").unwrap(), SubtitleFormat::Ass);
        assert_eq!(detect_format("test.ssa").unwrap(), SubtitleFormat::Ssa);
        assert!(detect_format("test.txt").is_err());
    }

    #[test]
    fn test_subtitle_with_translation() {
        let mut file = parse_srt("1\n00:00:01,000 --> 00:00:03,000\nHello\n").unwrap();
        file.entries[0].translated = "你好".to_string();
        let rendered = render_srt(&file);
        assert!(rendered.contains("你好"));
        assert!(!rendered.contains("Hello"));
    }

    #[test]
    fn test_parse_ass_skip_drawing() {
        // 含 \p1 矢量绘图指令的条目应被跳过
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.05,0:00:20.00,Default,,0,0,0,,{\\pos(960,50)\\p1}m 9.0 9.0 l 10.0 9.0\nDialogue: 0,0:00:04.09,0:00:05.71,Default,,0,0,0,,{\\r中文}杰瑞{\\r英文}\\NJerry\n";
        let file = parse_ass(content).unwrap();
        // 绘图条目被跳过，只剩 1 条字幕
        assert_eq!(file.entries.len(), 1);
        assert_eq!(file.entries[0].text, "{\\r中文}杰瑞{\\r英文}\\NJerry");
    }

    #[test]
    fn test_parse_ass_skip_logo_watermark() {
        // LOGO/水印/特效样式条目应被跳过
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.05,0:00:20.00,金流光,,0,0,0,,{\\an8\\pos(960,50)}微博@敛光存帧\n\
Dialogue: 0,0:10:00.05,0:10:20.00,顶中上浮LOGO_1080,,0,0,0,,{\\an8\\pos(960,50)}微博@敛光存帧\n\
Dialogue: 0,0:00:04.09,0:00:05.71,Default,,0,0,0,,{\\r中文}杰瑞{\\r英文}\\NJerry\n";
        let file = parse_ass(content).unwrap();
        // LOGO/水印条目被跳过，只剩 1 条字幕
        assert_eq!(file.entries.len(), 1, "应跳过 LOGO/水印，实际条目数: {}", file.entries.len());
        assert_eq!(file.entries[0].text, "{\\r中文}杰瑞{\\r英文}\\NJerry");
    }

    #[test]
    fn test_detect_bilingual_ass_with_n_separator() {
        // ass 双语字幕：\N 分隔中英文，带 ass 样式标记
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:04.09,0:00:05.71,Default,,0,0,0,,{\\r60字体 美剧 中文}杰瑞，这也太恶心了吧。{\\r60字体 美剧 英文}\\NJerry, this is disgusting.\n\
Dialogue: 0,0:00:05.80,0:00:07.22,Default,,0,0,0,,{\\r60字体 美剧 中文}怎么这么快就脏成这样了？{\\r60字体 美剧 英文}\\NHow did it get so dirty already?\n\
Dialogue: 0,0:00:07.30,0:00:09.51,Default,,0,0,0,,{\\r60字体 美剧 中文}因为你那破泳池机器人根本没用。{\\r60字体 美剧 英文}\\NBecause your crappy pool bot doesn't work.\n";
        let file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 3);
        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语：matched={}, total={}", result.matched_count, result.total_count);
        assert_eq!(result.lang_a, "cjk");
        assert_eq!(result.lang_b, "latin");
    }

    #[test]
    fn test_split_bilingual_ass_with_n_separator() {
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:04.09,0:00:05.71,Default,,0,0,0,,{\\r中文}杰瑞，这也太恶心了吧。{\\r英文}\\NJerry, this is disgusting.\n";
        let mut file = parse_ass(content).unwrap();
        split_bilingual(&mut file, SplitMode::EvenFirst);
        // 拆分后 text 应含英文（原文），translated 应含中文（译文）
        assert!(file.entries[0].text.contains("Jerry"), "text 应含英文：{}", file.entries[0].text);
        assert!(file.entries[0].translated.contains("杰瑞"), "translated 应含中文：{}", file.entries[0].translated);
    }

    #[test]
    fn test_detect_bilingual_ass_export_format() {
        // 模拟本软件导出的 ASS 双语字幕格式（修复后）：
        // {\rPrimary}译文\N{\rSecondary}原文  —— \N 在 override block 外部
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:04.09,0:00:05.71,Primary,,0,0,0,,{\\rPrimary}杰瑞，这也太恶心了吧。\\N{\\rSecondary}Jerry, this is disgusting.\n\
Dialogue: 0,0:00:05.80,0:00:07.22,Primary,,0,0,0,,{\\rPrimary}怎么这么快就脏成这样了？\\N{\\rSecondary}How did it get so dirty already?\n\
Dialogue: 0,0:00:07.30,0:00:09.51,Primary,,0,0,0,,{\\rPrimary}因为你那破泳池机器人根本没用。\\N{\\rSecondary}Because your crappy pool bot doesn't work.\n";
        let file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 3);
        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语：matched={}, total={}", result.matched_count, result.total_count);
    }

    #[test]
    fn test_detect_bilingual_ass_export_format_latin_first() {
        // 译文在上（Latin），原文在下（CJK）
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:04.09,0:00:05.71,Primary,,0,0,0,,{\\rPrimary}Jerry, this is disgusting.\\N{\\rSecondary}杰瑞，这也太恶心了吧。\n\
Dialogue: 0,0:00:05.80,0:00:07.22,Primary,,0,0,0,,{\\rPrimary}How did it get so dirty already?\\N{\\rSecondary}怎么这么快就脏成这样了？\n\
Dialogue: 0,0:00:07.30,0:00:09.51,Primary,,0,0,0,,{\\rPrimary}Because your crappy pool bot doesn't work.\\N{\\rSecondary}因为你那破泳池机器人根本没用。\n";
        let file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 3);
        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语（Latin在上）：matched={}, total={}", result.matched_count, result.total_count);
    }

    #[test]
    fn test_detect_bilingual_ass_with_html_font_tags() {
        // 真实案例：字幕组用 HTML <font> 标签区分双语
        // <font size="24">中文</font><font size="18" color="#cccccc">English</font>
        // strip_ass_tags 必须同时剥离 HTML 标签，否则 font/size/color 等拉丁字母会污染语言检测
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:02.96,0:00:05.04,Default,,0,0,0,,<font size=\"24\">杰瑞\\N啊，我好紧张！</font><font size=\"18\" color=\"#cccccc\">Jerry:\\NUhh, I'm so nervous!</font>\n\
Dialogue: 0,0:00:05.13,0:00:06.79,Default,,0,0,0,,<font size=\"24\">好久没人了\\N甚至取笑我</font><font size=\"18\" color=\"#cccccc\">It's been so long, no one\\Neven makes fun of me</font>\n\
Dialogue: 0,0:00:06.92,0:00:08.38,Default,,0,0,0,,<font size=\"24\">因为失业\\N不再。</font><font size=\"18\" color=\"#cccccc\">for being unemployed\\Nanymore.</font>\n\
Dialogue: 0,0:00:08.46,0:00:10.84,Default,,0,0,0,,<font size=\"24\">噢，我们可以取笑你\\N如果你愿意，亲爱的。</font><font size=\"18\" color=\"#cccccc\">Aw. We can make fun of you\\Nif you want, sweetie.</font>\n\
Dialogue: 0,0:00:10.96,0:00:12.93,Default,,0,0,0,,<font size=\"24\">你也可以\\N拿一个。</font><font size=\"18\" color=\"#cccccc\">You could also\\Ntake one of these.</font>\n";
        let file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 5);
        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语（HTML font标签）：matched={}, total={}", result.matched_count, result.total_count);
    }

    #[test]
    fn test_split_bilingual_ass_with_html_font_tags() {
        let content = "[Script Info]\nTitle: Test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:02.96,0:00:05.04,Default,,0,0,0,,<font size=\"24\">杰瑞\\N啊，我好紧张！</font><font size=\"18\" color=\"#cccccc\">Jerry:\\NUhh, I'm so nervous!</font>\n\
Dialogue: 0,0:00:05.13,0:00:06.79,Default,,0,0,0,,<font size=\"24\">好久没人了\\N甚至取笑我</font><font size=\"18\" color=\"#cccccc\">It's been so long, no one\\Neven makes fun of me</font>\n\
Dialogue: 0,0:00:06.92,0:00:08.38,Default,,0,0,0,,<font size=\"24\">因为失业\\N不再。</font><font size=\"18\" color=\"#cccccc\">for being unemployed\\Nanymore.</font>\n";
        let mut file = parse_ass(content).unwrap();
        split_bilingual(&mut file, SplitMode::EvenFirst);
        // 拆分后 text 应含英文（原文），translated 应含中文（译文）
        assert!(file.entries[0].text.contains("Jerry"), "text 应含英文：{}", file.entries[0].text);
        assert!(file.entries[0].translated.contains("杰瑞"), "translated 应含中文：{}", file.entries[0].translated);
    }

    #[test]
    fn test_export_ass_strips_inline_tags_to_match_preview() {
        // 输入条目内含 ASS/HTML 内联样式标记；导出应剥离这些内联标记，
        // 只保留导出模板注入的 Primary/Secondary 样式，避免播放器效果与预览差异过大。
        let file = SubtitleFile {
            format: SubtitleFormat::Ass,
            entries: vec![SubtitleEntry {
                index: 0,
                start_ms: 0,
                end_ms: 2000,
                text: "{\\r60字体 美剧 中文}<font size=\"24\">噢，我们可以取笑你</font>".to_string(),
                translated: "{\\r60字体 美剧 英文}<font size=\"18\">Aw. We can make fun of you</font>".to_string(),
                style: Some("Default".to_string()),
                failed: false,
                from_cache: false,
            }],
            raw_header: None,
            source_path: None,
            file_hash: String::new(),
        };
        let opts = ExportOptions {
            format: SubtitleFormat::Ass,
            mode: ExportMode::Bilingual,
            monolingual_lang: None,
            bilingual_translated_first: Some(true),
            ass_style: None,
            video_width: Some(1280),
            video_height: Some(720),
        };
        let out = render_ass_with_options(&file, &opts);
        // 事件文本里应只含导出模板样式，不再含源内联样式标签
        assert!(out.contains("{\\rPrimary}Aw. We can make fun of you\\N{\\rSecondary}噢，我们可以取笑你"));
        assert!(!out.contains("{\\r60字体 美剧 中文}"));
        assert!(!out.contains("{\\r60字体 美剧 英文}"));
        assert!(!out.contains("<font"));
    }

    #[test]
    fn test_detect_and_split_already_exported_broken_file() {
        // 真实案例：本软件之前（有 bug 时）导出的 ASS 双语文件
        // 因为检测失败，translated 为空，全部内容堆在 text 里，导出后格式错乱
        // 修复后重新加载此文件应能检测出双语并正确拆分
        let content = "[Script Info]\nTitle: AI-SubTrans Export\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Primary,Arial,24,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Secondary,Arial,18,&HCCCCCC&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Default,Arial,24,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:02.96,0:00:05.04,Primary,,0,0,0,,{\\rPrimary}\\N{\\rSecondary}<font size=\"24\"></font><font size=\"24\">杰瑞\\N啊，我好紧张！</font><font size=\"18\" color=\"#cccccc\">Jerry:\\NUhh, I'm so nervous!</font>\n\
Dialogue: 0,0:00:05.13,0:00:06.79,Primary,,0,0,0,,{\\rPrimary}\\N{\\rSecondary}<font size=\"24\"></font><font size=\"24\">好久没人了\\N甚至取笑我</font><font size=\"18\" color=\"#cccccc\">It's been so long, no one\\Neven makes fun of me</font>\n\
Dialogue: 0,0:00:06.92,0:00:08.38,Primary,,0,0,0,,{\\rPrimary}\\N{\\rSecondary}<font size=\"24\"></font><font size=\"24\">因为失业\\N不再。</font><font size=\"18\" color=\"#cccccc\">for being unemployed\\Nanymore.</font>\n\
Dialogue: 0,0:00:08.46,0:00:10.84,Primary,,0,0,0,,{\\rPrimary}\\N{\\rSecondary}<font size=\"24\"></font><font size=\"24\">噢，我们可以取笑你\\N如果你愿意，亲爱的。</font><font size=\"18\" color=\"#cccccc\">Aw. We can make fun of you\\Nif you want, sweetie.</font>\n\
Dialogue: 0,0:00:10.96,0:00:12.93,Primary,,0,0,0,,{\\rPrimary}\\N{\\rSecondary}<font size=\"24\"></font><font size=\"24\">你也可以\\N拿一个。</font><font size=\"18\" color=\"#cccccc\">You could also\\Ntake one of these.</font>\n";
        let mut file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 5, "应解析出5条字幕");

        // 1. 检测双语
        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语：matched={}, total={}", result.matched_count, result.total_count);

        // 2. 拆分
        split_bilingual(&mut file, SplitMode::EvenFirst);

        // 3. 拆分后 text 应含英文（原文），translated 应含中文（译文）
        // 注意：此测试数据是旧版 bug 导出格式，<font> 标签混合了中英文，
        // 拆分后 translated 可能含部分英文（如 Jerry:），text 含英文剩余部分
        assert!(file.entries[0].translated.contains("杰瑞"), "translated 应含中文：{}", file.entries[0].translated);
        assert!(file.entries[0].text.contains("nervous"), "text 应含英文：{}", file.entries[0].text);
    }

    #[test]
    fn test_detect_bilingual_real_export_multiline() {
        // 真实案例：本软件导出的 ASS 双语字幕，中文和英文各自含 \N 多行换行
        // 格式：{\rPrimary}中文1\N中文2\N{\rSecondary}英文1\N英文2
        // 重新导入后无法识别为双语
        let content = "[Script Info]\nTitle: AI-SubTrans Export\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Primary,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Secondary,Arial,30,&HCCCCCC&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Default,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:02.96,0:00:05.04,Primary,,0,0,0,,{\\rPrimary}杰瑞\\N啊，我好紧张！\\N{\\rSecondary}Jerry:\\NUhh, I'm so nervous!\n\
Dialogue: 0,0:00:05.13,0:00:06.79,Primary,,0,0,0,,{\\rPrimary}好久没人了\\N甚至取笑我\\N{\\rSecondary}It's been so long, no one\\Neven makes fun of me\n\
Dialogue: 0,0:00:06.92,0:00:08.38,Primary,,0,0,0,,{\\rPrimary}因为失业\\N不再。\\N{\\rSecondary}for being unemployed\\Nanymore.\n\
Dialogue: 0,0:00:08.46,0:00:10.84,Primary,,0,0,0,,{\\rPrimary}噢，我们可以取笑你\\N如果你愿意，亲爱的。\\N{\\rSecondary}Aw. We can make fun of you\\Nif you want, sweetie.\n\
Dialogue: 0,0:00:10.96,0:00:12.93,Primary,,0,0,0,,{\\rPrimary}你也可以\\N拿一个。\\N{\\rSecondary}You could also\\Ntake one of these.\n";
        let file = parse_ass(content).unwrap();
        eprintln!("entries count: {}", file.entries.len());
        for (i, e) in file.entries.iter().enumerate() {
            eprintln!("  entry[{}] text={:?}", i, e.text);
        }
        let result = detect_bilingual(&file);
        eprintln!("detect result: is_bilingual={}, matched={}, total={}", result.is_bilingual, result.matched_count, result.total_count);
        assert!(result.is_bilingual, "应检测出双语：matched={}, total={}", result.matched_count, result.total_count);
    }

    #[test]
    fn test_detect_bilingual_real_export_with_secondary_only_entries() {
        // 真实案例：本软件导出的 ASS 双语字幕，混合了：
        // 1. Primary 样式的双语条目（中英文用 \N 分隔）
        // 2. Secondary 样式的纯英文条目（无 \N 或有 \N 但只有英文）
        // 纯英文条目不应影响双语检测
        // 真实文件：181 条 Primary 双语 + 379 条 Secondary 纯英文 = 560 条
        // 旧逻辑：threshold = 560*3/5 = 336，matched 最多 181 < 336 → 检测失败
        // 新逻辑：threshold 基于 multiline_count，181 条双语全部是多行，matched=181 >= threshold
        let content = "[Script Info]\nTitle: AI-SubTrans Export\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Primary,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Secondary,Arial,30,&HCCCCCC&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Default,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:02.96,0:00:05.04,Primary,,0,0,0,,{\\rPrimary}杰瑞\\N啊，我好紧张！\\N{\\rSecondary}Jerry:\\NUhh, I'm so nervous!\n\
Dialogue: 0,0:00:05.13,0:00:06.79,Primary,,0,0,0,,{\\rPrimary}好久没人了\\N甚至取笑我\\N{\\rSecondary}It's been so long, no one\\Neven makes fun of me\n\
Dialogue: 0,0:00:06.92,0:00:08.38,Primary,,0,0,0,,{\\rPrimary}因为失业\\N不再。\\N{\\rSecondary}for being unemployed\\Nanymore.\n\
Dialogue: 0,0:00:08.46,0:00:10.84,Primary,,0,0,0,,{\\rPrimary}噢，我们可以取笑你\\N如果你愿意，亲爱的。\\N{\\rSecondary}Aw. We can make fun of you\\Nif you want, sweetie.\n\
Dialogue: 0,0:00:10.96,0:00:12.93,Primary,,0,0,0,,{\\rPrimary}你也可以\\N拿一个。\\N{\\rSecondary}You could also\\Ntake one of these.\n\
Dialogue: 0,0:03:47.85,0:03:49.18,Secondary,,0,0,0,,{\\rSecondary}What was <i>that </i>like?\n\
Dialogue: 0,0:04:25.05,0:04:26.97,Secondary,,0,0,0,,{\\rSecondary}<i>Drink.</i>\n\
Dialogue: 0,0:05:57.31,0:05:58.85,Secondary,,0,0,0,,{\\rSecondary}<i>in cruelty-free Mup technology,</i>\n\
Dialogue: 0,0:06:01.36,0:06:03.94,Secondary,,0,0,0,,{\\rSecondary}<i>with our drag-and-drop display.</i>\n\
Dialogue: 0,0:21:36.62,0:21:38.88,Secondary,,0,0,0,,{\\rSecondary}So it's all hands\\Non deck this week.\n";
        let file = parse_ass(content).unwrap();
        eprintln!("entries count: {}", file.entries.len());
        let result = detect_bilingual(&file);
        eprintln!("detect result: is_bilingual={}, matched={}, total={}", result.is_bilingual, result.matched_count, result.total_count);
        // 5 条双语多行 + 5 条纯英文（3 条单行 + 2 条多行）= 10 条
        // multiline_count = 5(双语) + 2(纯英文多行) = 7
        // threshold = 7*3/5 = 4, matched = 5 >= 4 → 检测成功
        assert!(result.is_bilingual, "应检测出双语：matched={}, total={}", result.matched_count, result.total_count);
    }

    #[test]
    fn test_split_bilingual_cjk_line_with_english_name() {
        // 真实案例：本软件导出的 ASS 双语字幕，中文行中混有英文人名 "Sum Sum"
        // 旧算法（逐字符扫描）会在 "S" 处误判为语言切换点，导致中文行被截断
        // 新算法（行级别检测）应正确在 {\rSecondary} 处切分
        let content = "[Script Info]\nTitle: AI-SubTrans Export\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Primary,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Secondary,Arial,30,&HCCCCCC&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Default,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:01:14.82,0:01:17.24,Primary,,0,0,0,,{\\rPrimary}你吃的所有东西\\N设计用于扩展。\\N{\\rSecondary}Everything you ate\\Nis designed to expand.\n\
Dialogue: 0,0:01:17.32,0:01:19.53,Primary,,0,0,0,,{\\rPrimary}[打嗝]\\N说得好，Sum Sum。\\N{\\rSecondary}[ Belches ]\\NGood point, Sum-Sum.\n\
Dialogue: 0,0:01:19.62,0:01:22.20,Primary,,0,0,0,,{\\rPrimary}我们去找爷爷。\\N新学期。\\N{\\rSecondary}L-Let's get grandpa.\\Nnew tum tum.\n";
        let mut file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 3);

        // 检测双语
        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语：matched={}, total={}", result.matched_count, result.total_count);

        // 拆分
        split_bilingual(&mut file, SplitMode::EvenFirst);

        // 第2条是关键测试：中文行含 "Sum Sum" 英文人名
        // 拆分后 text 应含英文（原文），translated 应含中文（译文）
        assert!(file.entries[1].text.contains("Good point"), "text 应含英文 'Good point'：{}", file.entries[1].text);
        assert!(!file.entries[1].text.contains("说得好"), "text 不应含中文：{}", file.entries[1].text);
        assert!(file.entries[1].translated.contains("说得好"), "translated 应含中文'说得好'：{}", file.entries[1].translated);
        assert!(file.entries[1].translated.contains("Sum Sum"), "translated 应含英文人名 'Sum Sum'：{}", file.entries[1].translated);
        assert!(!file.entries[1].translated.contains("Good point"), "translated 不应含英文翻译：{}", file.entries[1].translated);
        // 不应有残留 ASS 标签
        assert!(!file.entries[1].text.contains("{\\r"), "text 不应含 ASS 样式标签：{}", file.entries[1].text);
        assert!(!file.entries[1].translated.contains("{\\r"), "translated 不应含 ASS 样式标签：{}", file.entries[1].translated);
    }

    #[test]
    fn test_split_bilingual_primary_at_pos_zero() {
        // 真实案例：本软件导出的 ASS 双语字幕，{\rPrimary} 在字符串位置 0
        // 旧版 find_tag_start_before 不检查 orig_pos 自身，越过 \n 找到位置 0 的 {
        // 导致 a="" 切分失败。修复后应正确在 {\rSecondary} 处切分。
        let content = "[Script Info]\nTitle: AI-SubTrans Export\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Primary,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Secondary,Arial,30,&HCCCCCC&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Default,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:02.79,0:00:04.63,Primary,,0,0,0,,{\\rPrimary}[放声大哭]\\N{\\rSecondary}[ Gulping loudly ]\n\
Dialogue: 0,0:00:04.71,0:00:05.67,Primary,,0,0,0,,{\\rPrimary}啊！\\N{\\rSecondary}Ahh!\n\
Dialogue: 0,0:00:05.75,0:00:06.96,Primary,,0,0,0,,{\\rPrimary}什么？\\N没有什么。\\N{\\rSecondary}What?\\NNothing.\n\
Dialogue: 0,0:00:07.00,0:00:08.34,Primary,,0,0,0,,{\\rPrimary}只是等待\\N让你完成。\\N{\\rSecondary}Just waiting\\Nfor you to finish.\n";
        let mut file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 4);

        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语：matched={}, total={}", result.matched_count, result.total_count);

        split_bilingual(&mut file, SplitMode::EvenFirst);

        // 所有条目都应成功拆分
        for (i, e) in file.entries.iter().enumerate() {
            assert!(!e.text.is_empty(), "entry[{}] text 不应为空", i);
            assert!(!e.translated.is_empty(), "entry[{}] translated 不应为空", i);
            assert!(!e.text.contains("{\\r"), "entry[{}] text 不应含 ASS 标签：{}", i, e.text);
            assert!(!e.translated.contains("{\\r"), "entry[{}] translated 不应含 ASS 标签：{}", i, e.translated);
            assert!(!e.translated.contains("\\rSecondary"), "entry[{}] translated 不应含残留 \\rSecondary：{}", i, e.translated);
        }

        // 验证具体内容：text 应含英文（原文），translated 应含中文（译文）
        assert!(file.entries[0].text.contains("Gulping"), "entry[0] text：{}", file.entries[0].text);
        assert!(file.entries[0].translated.contains("放声大哭"), "entry[0] translated：{}", file.entries[0].translated);

        assert!(file.entries[1].text.contains("Ahh"), "entry[1] text：{}", file.entries[1].text);
        assert!(file.entries[1].translated.contains("啊"), "entry[1] translated：{}", file.entries[1].translated);

        assert!(file.entries[2].text.contains("What"), "entry[2] text：{}", file.entries[2].text);
        assert!(file.entries[2].translated.contains("什么"), "entry[2] translated：{}", file.entries[2].translated);
    }

    #[test]
    fn test_split_bilingual_tag_start_off_by_one() {
        // 真实案例 #508：{\rPrimary}W-你为什么会猜到？\N{\rSecondary}W-Why would you guess that?
        // {\rSecondary} 的 { 在 byte_offset-3 处（前面有 \n），
        // 旧版 find_tag_start_before 返回 pos 而非 pos-1，导致 { 留在 a 中，
        // b 以 \rSecondary} 开头，strip_ass_tags 无法剥离（没有配对的 {）
        let content = "[Script Info]\nTitle: AI-SubTrans Export\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Primary,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Secondary,Arial,30,&HCCCCCC&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Default,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:19:29.75,0:19:31.04,Primary,,0,0,0,,{\\rPrimary}你只是吸\\N你自己的鸡巴在里面？\\N{\\rSecondary}Did you just suck\\Nyour own dick in there?\n\
Dialogue: 0,0:19:31.12,0:19:32.29,Primary,,0,0,0,,{\\rPrimary}哇！\\N不，我没有！\\N{\\rSecondary}Whoa!\\NNo, I didn't!\n\
Dialogue: 0,0:19:32.38,0:19:33.71,Primary,,0,0,0,,{\\rPrimary}W-你为什么会猜到？\\N{\\rSecondary}W-Why would you guess that?\n\
Dialogue: 0,0:19:33.75,0:19:35.46,Primary,,0,0,0,,{\\rPrimary}你为什么要那么做？！\\N我没有！\\N{\\rSecondary}Why would you do that?!\\NI didn't!\n";
        let mut file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 4);

        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语");

        split_bilingual(&mut file, SplitMode::EvenFirst);

        // #508 是 entry[2]：W-你为什么会猜到？\NW-Why would you guess that?
        let e = &file.entries[2];
        assert!(!e.text.is_empty(), "text 不应为空：{}", e.text);
        assert!(!e.translated.is_empty(), "translated 不应为空：{}", e.translated);
        assert!(e.text.contains("Why would"), "text 应含英文：{}", e.text);
        assert!(e.translated.contains("你为什么"), "translated 应含中文：{}", e.translated);
        // 关键验证：不应有残留的 \rSecondary 或 rSecondary
        assert!(!e.translated.contains("rSecondary"), "translated 不应含残留 rSecondary：{}", e.translated);
        assert!(!e.text.contains("rPrimary}"), "text 不应含残留 rPrimary}}：{}", e.text);
        assert!(!e.translated.contains("{\\r"), "translated 不应含 ASS 标签：{}", e.translated);
    }

    #[test]
    fn test_split_bilingual_cjk_line_with_short_english() {
        // 真实案例 #89：{\rPrimary}夏季：Duh.\N看起来我谋杀了一个人。\N{\rSecondary}Summer: Duh.\NIt looks like I murdered a guy.
        // 旧算法用 detect_line_lang 按字符计数：夏季(Duh.) = 2 CJK vs 3 Latin → 误判为 Latin
        // 新算法用 CJK 存在性：含"夏季"→ CJK 行，不因短英文单词误判
        let content = "[Script Info]\nTitle: AI-SubTrans Export\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Primary,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Secondary,Arial,30,&HCCCCCC&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\nStyle: Default,Arial,48,&HFFFFFF&,&H000000&,&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:03:37.46,0:03:39.05,Primary,,0,0,0,,{\\rPrimary}先别放松。\\N{\\rSecondary}Don't relax yet.\n\
Dialogue: 0,0:03:39.13,0:03:40.76,Primary,,0,0,0,,{\\rPrimary}你找错地方了\\N随身携带那东西。\\N{\\rSecondary}You're in the wrong place\\Nto be toting that thing around.\n\
Dialogue: 0,0:03:40.84,0:03:42.22,Primary,,0,0,0,,{\\rPrimary}夏季：Duh.\\N看起来我谋杀了一个人。\\N{\\rSecondary}Summer: Duh.\\NIt looks like I murdered a guy.\n\
Dialogue: 0,0:03:42.30,0:03:43.76,Primary,,0,0,0,,{\\rPrimary}里克的头：哦，那很好。\\N{\\rSecondary}Rick's Head: Oh, that's fine.\n";
        let mut file = parse_ass(content).unwrap();
        assert_eq!(file.entries.len(), 4);

        let result = detect_bilingual(&file);
        assert!(result.is_bilingual, "应检测出双语");

        split_bilingual(&mut file, SplitMode::EvenFirst);

        // #89 是 entry[2]：夏季：Duh.\N看起来我谋杀了一个人。\NSummer: Duh.\NIt looks like I murdered a guy.
        let e = &file.entries[2];
        assert!(!e.text.is_empty(), "text 不应为空：{}", e.text);
        assert!(!e.translated.is_empty(), "translated 不应为空：{}", e.translated);
        // 拆分后 text 应含英文（原文），translated 应含中文（译文）
        assert!(e.text.contains("Summer"), "text 应含 'Summer'：{}", e.text);
        assert!(e.text.contains("murdered"), "text 应含 'murdered'：{}", e.text);
        assert!(e.translated.contains("夏季"), "translated 应含中文'夏季'：{}", e.translated);
        assert!(e.translated.contains("Duh"), "translated 应含 'Duh'：{}", e.translated);
        assert!(e.translated.contains("谋杀"), "translated 应含'谋杀'：{}", e.translated);
        assert!(!e.text.contains("夏季"), "text 不应含中文：{}", e.text);
        assert!(!e.translated.contains("murdered"), "translated 不应含英文翻译：{}", e.translated);
        assert!(!e.translated.contains("rSecondary"), "translated 不应含残留标签：{}", e.translated);
    }

    /// 导出→重新导入的一致性测试：
    /// 构造含各类失败条目（音效错位、♪♪、译文=原文、英文原样、failed=false 但音效错位）的字幕，
    /// 分别导出为 SRT/ASS/VTT，再解析+拆分，验证三种格式识别出的"未翻译"条目数一致。
    /// 特别覆盖：entry.failed=false 但译文/原文音效类型不一致的情况
    /// （Path 1 旧 bug：failed 字段缺少 sound_mismatch，导致导出双行→重新导入后各格式未翻译数不一致）
    #[test]
    fn test_roundtrip_failed_entries_consistent_across_formats() {
        // 构造测试条目：混合正常译文和各类失败情形
        let make = |i: usize, text: &str, translated: &str, failed: bool| SubtitleEntry {
            index: i,
            start_ms: (i as i64) * 2000,
            end_ms: (i as i64) * 2000 + 1800,
            text: text.to_string(),
            translated: translated.to_string(),
            style: None,
            failed,
            from_cache: false,
        };
        let entries = vec![
            make(0, "Hello world", "你好世界", false),            // 正常
            make(1, "How are you?", "你好吗？", false),           // 正常
            make(2, "♪♪", "♪♪", true),                            // 音乐符号（失败）
            make(3, "See you all in a week!", "[吞咽声]", true),   // 音效错位（failed=true）
            make(4, "[ Zapping ]", "[ Zapping ]", true),          // 音效原样（失败）
            make(5, "UGG! Glugg UGG!", "UGG! Glugg UGG!", true),  // 译文=原文（失败）
            make(6, "Guest house is here.", "客房在这里。", false), // 正常
            // 关键：failed=false 但译文是音效、原文不是音效
            // 旧 bug：build_entry_text 不检查 sound_mismatch → 输出双行 → SRT/VTT 重新导入后能拆分
            //         但 ASS 也能拆分 → 各格式未翻译数不一致
            // 修复后：build_entry_text 检查 sound_mismatch → 输出单行 → 三种格式一致
            make(7, "you need every week.", "[碰撞声持续]", false),
        ];
        // 期望的"失败/未翻译"数：index 2,3,4,5,7 共 5 条
        let expected_untranslated = 5;

        let mk_opts = |fmt: SubtitleFormat| ExportOptions {
            format: fmt,
            mode: ExportMode::Bilingual,
            monolingual_lang: None,
            bilingual_translated_first: Some(true),
            ass_style: None,
            video_width: Some(1280),
            video_height: Some(720),
        };

        // 判定"未翻译"：与前端 isUntranslated 逻辑一致（含音效错位检查）
        let is_untranslated = |e: &SubtitleEntry| -> bool {
            e.translated.trim().is_empty()
                || e.translated.trim() == e.text.trim()
                || (!has_cjk_chars(&e.translated) && !has_cjk_chars(&e.text))
                || looks_like_sound_effect(&e.text) != looks_like_sound_effect(&e.translated)
        };

        let cases: [(SubtitleFormat, fn(&str) -> Result<SubtitleFile, AppError>); 3] = [
            (SubtitleFormat::Srt, parse_srt),
            (SubtitleFormat::Vtt, parse_vtt),
            (SubtitleFormat::Ass, parse_ass),
        ];
        for (fmt, parse) in cases {
            let file = SubtitleFile {
                format: fmt.clone(),
                entries: entries.clone(),
                raw_header: None,
                source_path: None,
                file_hash: String::new(),
            };
            let exported = export_subtitle(&file, &mk_opts(fmt.clone()));
            let mut reparsed = parse(&exported).unwrap();
            assert_eq!(
                reparsed.entries.len(),
                entries.len(),
                "格式 {:?} 重新解析条目数不一致",
                fmt
            );
            // 拆分双语
            split_bilingual(&mut reparsed, SplitMode::EvenFirst);
            let untranslated = reparsed.entries.iter().filter(|e| is_untranslated(e)).count();
            assert_eq!(
                untranslated, expected_untranslated,
                "格式 {:?} 未翻译数应为 {}，实际 {}（条目: {:?}）",
                fmt,
                expected_untranslated,
                untranslated,
                reparsed.entries.iter().map(|e| (e.text.clone(), e.translated.clone())).collect::<Vec<_>>()
            );
        }
    }
}
