// 字幕解析与编辑模块
// srt/vtt 自写解析器，ass 使用 ass-rs（ass-core + ass-editor）
// 统一内部结构 SubtitleEntry，对应需求文档 §7 parse_subtitle 返回值

use crate::error::AppError;
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
}

/// 字幕文件（解析后的统一结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleFile {
    pub format: SubtitleFormat,
    pub entries: Vec<SubtitleEntry>,
    pub raw_header: Option<String>, // ass 的 [Script Info] + [V4+ Styles]，srt/vtt 为 None
    pub source_path: Option<String>,
}

// === SECTION 1 END ===

// === 双语字幕检测模块 ===

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

/// 检测一行文本的主导语言（忽略数字、标点、空白）
fn detect_line_lang(line: &str) -> LangClass {
    let mut counts: std::collections::HashMap<LangClass, usize> = std::collections::HashMap::new();
    for c in line.chars() {
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

    for entry in &file.entries {
        let lines: Vec<&str> = entry.text.lines().collect();
        if lines.len() < 2 {
            continue;
        }
        // 对每行检测语言
        let line_langs: Vec<LangClass> = lines.iter().map(|l| detect_line_lang(l)).collect();

        // 找到语言切换点：前面都是一种语言，后面是另一种语言
        // 策略：找到第一个"非Other"的语言作为语言A的起点，
        // 然后找到第一个与语言A不同的"非Other"语言作为切换点
        let mut split_point: Option<usize> = None;
        let mut first_lang = LangClass::Other;

        // 找第一个非Other语言
        for (i, &cl) in line_langs.iter().enumerate() {
            if cl != LangClass::Other {
                first_lang = cl;
                break;
            }
        }
        if first_lang == LangClass::Other {
            continue;
        }

        // 找切换点：第一个与 first_lang 不同且非Other 的语言
        for (i, &cl) in line_langs.iter().enumerate() {
            if cl != LangClass::Other && is_different_lang(cl, first_lang) {
                split_point = Some(i);
                break;
            }
        }

        if let Some(sp) = split_point {
            // 验证：切换点之后的所有非Other行应该都是同一种语言
            let b_lang = line_langs[sp];
            let after_valid = line_langs[sp..].iter().all(|&cl| {
                cl == LangClass::Other || !is_different_lang(cl, b_lang)
            });
            if after_valid && b_lang != LangClass::Other {
                matched_count += 1;
                lang_a_class = first_lang;
                lang_b_class = b_lang;
            }
        }
    }

    let threshold = (total * 3) / 5; // 60% 的条目匹配才算双语

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

/// 拆分双语字幕：按语言分块，前半部分（语言A）填入 text，后半部分（语言B）填入 translated
pub fn split_bilingual(file: &mut SubtitleFile, _split_mode: SplitMode) {
    for entry in &mut file.entries {
        let lines: Vec<String> = entry.text.lines().map(|l| l.to_string()).collect();
        if lines.len() < 2 {
            continue;
        }
        let line_langs: Vec<LangClass> = lines.iter().map(|l| detect_line_lang(l)).collect();

        // 找第一个非Other语言
        let mut first_lang = LangClass::Other;
        for &cl in &line_langs {
            if cl != LangClass::Other {
                first_lang = cl;
                break;
            }
        }
        if first_lang == LangClass::Other {
            continue;
        }

        // 找切换点：第一个与 first_lang 不同且非Other 的语言
        let mut split_point: Option<usize> = None;
        for (i, &cl) in line_langs.iter().enumerate() {
            if cl != LangClass::Other && is_different_lang(cl, first_lang) {
                split_point = Some(i);
                break;
            }
        }

        if let Some(sp) = split_point {
            let a_lines = lines[..sp].join("\n");
            let b_lines = lines[sp..].join("\n");
            if !a_lines.is_empty() && !b_lines.is_empty() {
                entry.text = a_lines;
                entry.translated = b_lines;
            }
        }
    }
}

// === 双语检测模块 END ===

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
        let text_lines: Vec<&str> = lines[(timecode_line_idx + 1)..].iter().copied().collect();
        let text = text_lines.join("\n");

        entries.push(SubtitleEntry {
            index: idx,
            start_ms,
            end_ms,
            text,
            translated: String::new(),
            style: None,
        });
    }

    Ok(SubtitleFile {
        format: SubtitleFormat::Srt,
        entries,
        raw_header: None,
        source_path: None,
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
        let end_str = time_parts[1].trim().split_whitespace().next().unwrap_or("");
        let end_ms = parse_vtt_timecode(end_str)?;

        let text_lines: Vec<&str> = lines[(timecode_line_idx + 1)..]
            .iter()
            .copied()
            .collect();
        let text = text_lines.join("\n");

        entries.push(SubtitleEntry {
            index: idx,
            start_ms,
            end_ms,
            text,
            translated: String::new(),
            style: None,
        });
    }

    Ok(SubtitleFile {
        format: SubtitleFormat::Vtt,
        entries,
        raw_header: None,
        source_path: None,
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

                entries.push(SubtitleEntry {
                    index: entries.len(),
                    start_ms,
                    end_ms,
                    text,
                    translated: String::new(),
                    style: Some(style),
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

                entries.push(SubtitleEntry {
                    index: entries.len(),
                    start_ms,
                    end_ms,
                    text,
                    translated: String::new(),
                    style: Some(style),
                });
            }
        }
    }

    Ok(SubtitleFile {
        format: SubtitleFormat::Ass,
        entries,
        raw_header: Some(header),
        source_path: None,
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

/// 统一解析入口：按格式解析字幕文件
pub fn parse_subtitle(content: &str, format: &SubtitleFormat) -> Result<SubtitleFile, AppError> {
    match format {
        SubtitleFormat::Srt => parse_srt(content),
        SubtitleFormat::Vtt => parse_vtt(content),
        SubtitleFormat::Ass | SubtitleFormat::Ssa => parse_ass(content),
    }
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

    let bytes = std::fs::read(path).map_err(|e| AppError::Io(e))?;
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
}
