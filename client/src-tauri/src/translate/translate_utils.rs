// 翻译工具函数模块
// 从 translate.rs 拆分出来的纯工具函数：检测、格式处理、占位符保护

use crate::translate::PlaceholderStrategy;

/// 检查字符串是否包含 CJK 字符（中日韩统一表意文字）
pub(crate) fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        let code = c as u32;
        (0x4E00..=0x9FFF).contains(&code)
    })
}

/// 判断文本是否为音效/环境声标记，如 [clattering continues] / [碰撞声持续] / [soft music]
/// 规则：整段 trimmed 文本被一对 [] 包裹，或主要内容是方括号内的一个短语。
pub(crate) fn looks_like_sound_effect(s: &str) -> bool {
    // 先去掉 ASS 定位/样式标签（如 {\an8}、{\b1} 等），与 build_entry_text 的 strip_inline_ass_and_html_tags 一致
    // 否则含 {\an8} 前缀的音效标记（如 {\an8}[phone buzzing]）会被误判为非音效标记，
    // 导致翻译时 is_untranslated 与导出往返后 is_untranslated 不一致
    let stripped = strip_ass_tags(s);
    let s = stripped.trim();
    if s.is_empty() {
        return false;
    }
    // 1. 整段被 [] 包裹
    if s.starts_with('[') && s.ends_with(']') {
        return true;
    }
    // 2. 去掉常见音效前缀（如 [Jeremy] / [Kaleb]）后仍被 [] 包裹
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
pub(crate) fn strip_ass_tags(s: &str) -> String {
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

/// 去掉所有格式标签：ASS 覆盖标签 {...} 和 HTML/占位符标签 <...>
/// 用于比较译文与原文是否相同时去除标签干扰
pub(crate) fn strip_format_tags(s: &str) -> String {
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

/// 判断是否为纯音乐符号/特殊符号（如 ♪♪、♬♬ 等，无文字内容）
pub(crate) fn is_music_or_symbol_only(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    s.chars().all(|c| {
        c.is_whitespace()
            || "♪♬♫♩♭♮♯".contains(c)
            || matches!(c, '[' | ']' | '(' | ')' | '.' | '-' | '_' | '*')
    })
}

/// 判断文本是否包含音乐符号（♪♬♫♩ 等）
/// 含音乐符号的译文是歌词/拟声词，无法翻译，保持原样是正确行为
pub(crate) fn has_music_symbols(s: &str) -> bool {
    s.chars().any(|c| "♪♬♫♩♭♮♯".contains(c))
}

/// 检查文本是否包含至少 min_len 个连续英文字母组成的单词
/// 用于区分英语内容和非英语内容（如拼写字母 "G-O-R..."、祖鲁语歌词等）
pub(crate) fn has_english_word(s: &str, min_len: usize) -> bool {
    let mut max_run = 0usize;
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            max_run += 1;
        } else {
            if max_run >= min_len {
                return true;
            }
            max_run = 0;
        }
    }
    max_run >= min_len
}

/// 统计字符串中的英文单词数（≥min_len 字符的连续字母序列）
/// 用于检测部分翻译：译文中残留的英文单词
#[allow(dead_code)]
pub(crate) fn count_english_words(s: &str, min_len: usize) -> usize {
    let mut count = 0usize;
    let mut run = 0usize;
    for c in s.chars() {
        if c.is_ascii_alphabetic() || c == '\'' || c == '-' {
            // 字母、撇号、连字符都算单词的一部分（如 don't, w-we）
            if c.is_ascii_alphabetic() {
                run += 1;
            }
        } else {
            if run >= min_len {
                count += 1;
            }
            run = 0;
        }
    }
    if run >= min_len {
        count += 1;
    }
    count
}

/// 提取字符串中的英文 token（≥min_len 字母字符的连续序列，含撇号和连字符）
/// 返回 token 字符串列表，用于细粒度分析残留英文
pub(crate) fn extract_english_tokens(s: &str, min_len: usize) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut letter_count = 0usize;
    for c in s.chars() {
        if c.is_ascii_alphabetic() || c == '\'' || c == '-' {
            current.push(c);
            if c.is_ascii_alphabetic() {
                letter_count += 1;
            }
        } else {
            if letter_count >= min_len {
                tokens.push(current.clone());
            }
            current.clear();
            letter_count = 0;
        }
    }
    if letter_count >= min_len {
        tokens.push(current);
    }
    tokens
}

/// 去掉 <name=...>...</name> 标签，保留标签内的中文翻译
/// 兼容三种格式：<name=En>Zh</name>、<name="En">Zh</name>、<name>Zh</name>（无英文名）
pub(crate) fn strip_name_tags_inline(s: &str) -> String {
    // 与 strip_name_tags 的 Pass 1 使用相同正则，统一处理所有 <name> 标签格式
    // 包括 <name=En>Zh</name>、<name>Zh</name>（无 =）、<name="En">Zh</name>
    static NAME_TAG_INLINE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = NAME_TAG_INLINE_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)<name(?:=([^>]*))?\s*>(.*?)</name\s*>"#).unwrap()
    });
    let pass1 = re.replace_all(s, "$2").to_string();

    // 兼容畸形闭标签 </name=...> 或孤立 </name>
    static ORPHAN_CLOSE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re2 = ORPHAN_CLOSE_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)</name[^>]*>"#).unwrap()
    });
    re2.replace_all(&pass1, "").to_string()
}


/// 检测部分翻译：译文含 CJK 但同时残留英文单词
/// Qwen3-8B 常见模式：翻译后半部分但保留前半部分英文原文
/// 检测策略：
/// 1. ≥2 个英文 token → 部分翻译
/// 2. 1 个英文 token 时：
///    - 含连字符（结巴模式如 Y-Y-You, a-and）→ 部分翻译
///    - 全小写（普通词汇如 guys, anymore）→ 部分翻译
///    - 全大写且 ≤5 字符（缩写如 MPS, MUP, CEO）→ 跳过
///    - 首字母大写（可能是专有名词如 Snowball, Skyzone）→ 跳过
///    - 全大写且 >5 字符（可能是感叹如 WAAAAAAAAAAAR）→ 跳过
pub(crate) fn is_partial_translation(orig: &str, trans: &str) -> bool {
    // 原文必须是英文内容
    if !has_english_word(orig, 3) {
        return false;
    }
    // 译文必须含 CJK（说明模型确实翻译了一部分）
    if !has_cjk(trans) {
        return false;
    }
    // 去掉 name 标签（保留中文）、ASS 标签、占位符标签
    let cleaned = strip_name_tags_inline(trans);
    let cleaned = strip_format_tags(&cleaned);
    // 检测混合行中的未翻译音效标记（如 "[ Crash ] 泰特斯: 哦，该死！"）
    // 原文含 [...] 音效标记，译文也含 [...] 但内容仍为英文（无 CJK）
    // 这种情况说明 AI 翻译了对白但跳过了音效标记
    if has_untranslated_sound_effect_in_mixed_line(orig, trans) {
        return true;
    }
    // 去掉音效标记 [...] 中的内容
    let cleaned = regex::Regex::new(r"\[[^\]]*\]")
        .unwrap()
        .replace_all(&cleaned, "")
        .to_string();
    // 提取残留英文 token（≥3 字母字符）
    let tokens = extract_english_tokens(&cleaned, 3);
    if tokens.is_empty() {
        return false;
    }
    // ≥2 个英文 token → 部分翻译
    if tokens.len() >= 2 {
        return true;
    }
    // 1 个英文 token：根据特征判断
    let token = &tokens[0];
    // 含连字符 → 结巴模式，部分翻译
    if token.contains('-') {
        return true;
    }
    // 全小写 → 普通词汇未翻译
    if token.chars().filter(|c| c.is_ascii_alphabetic()).all(|c| c.is_ascii_lowercase()) {
        return true;
    }
    // 全大写 → 缩写或感叹，跳过
    // 首字母大写 → 可能是专有名词，跳过
    false
}

/// 检测混合行（音效标记 + 对白）中的音效标记问题
/// 检测以下问题：
/// 1. 未翻译：译文 [...] 内容与原文相同（仍为英文）
/// 2. 损坏：译文 [...] 内容为纯英文但与原文不同（如 [SSEARCH] 代替 [ Sighs ]）
/// 3. 空括号：译文有 [] 但原文括号内有内容
/// 4. 丢失：原文有 [...] 但译文完全没有方括号
fn has_untranslated_sound_effect_in_mixed_line(orig: &str, trans: &str) -> bool {
    // 原文不能是纯音效标记（纯音效标记由 is_partial_sound_effect 处理）
    if looks_like_sound_effect(orig) {
        return false;
    }
    // 原文必须含 [...] 音效标记
    let re_bracket = regex::Regex::new(r"\[([^\]]*)\]").unwrap();
    let orig_brackets: Vec<String> = re_bracket
        .captures_iter(orig)
        .map(|c| c[1].trim().to_string())
        .collect();
    if orig_brackets.is_empty() {
        return false;
    }
    // 原文括号内必须有英文单词（≥3 字母字符）或 CJK（音效标记）
    let has_content_in_brackets = orig_brackets
        .iter()
        .any(|b| !b.is_empty() && (extract_english_tokens(b, 3).iter().any(|t| !t.is_empty()) || has_cjk(b)));
    if !has_content_in_brackets {
        return false;
    }
    // 译文清理：去掉 name 标签和格式标签
    let trans_cleaned = strip_name_tags_inline(trans);
    let trans_cleaned = strip_format_tags(&trans_cleaned);
    let trans_brackets: Vec<String> = re_bracket
        .captures_iter(&trans_cleaned)
        .map(|c| c[1].trim().to_string())
        .collect();

    // 问题 4：原文有 [...] 但译文完全没有方括号（音效被丢弃）
    // 译文必须含 CJK（说明模型确实翻译了对白部分，只是丢了音效）
    if trans_brackets.is_empty() {
        if has_cjk(&trans_cleaned) {
            return true;
        }
        return false;
    }

    // 检查译文的每个括号内容
    for tb in &trans_brackets {
        // 问题 3：空括号（原文括号有内容，译文括号为空）
        if tb.is_empty() {
            return true;
        }
        // 跳过含 CJK 的括号（已翻译）
        if has_cjk(tb) {
            continue;
        }
        // 译文的括号内容与原文某个括号内容相同（问题 1：未翻译）
        if orig_brackets.iter().any(|ob| ob.eq_ignore_ascii_case(tb)) {
            // 但跳过全大写缩写（如 [CEO], [MPS]）
            let tokens = extract_english_tokens(tb, 3);
            let has_lowercase = tokens.iter().any(|t| {
                t.chars().filter(|c| c.is_ascii_alphabetic()).any(|c| c.is_ascii_lowercase())
            });
            if has_lowercase {
                return true;
            }
            continue;
        }
        // 译文的括号内容与原文任何括号都不匹配（问题 2：损坏）
        // 且括号内容含英文单词（不是纯符号）
        let tokens = extract_english_tokens(tb, 3);
        if !tokens.is_empty() {
            return true;
        }
    }
    false
}

/// 检测音效标记的半翻译：原文是 [xxx] 音效，译文也是 [...] 格式但含未翻译的英文单词
/// 例如 [ All grunting ] → [ 所有人发出 grunt 声 ]，"grunt" 未翻译
pub(crate) fn is_partial_sound_effect(orig: &str, trans: &str) -> bool {
    // 原文必须是音效标记
    if !looks_like_sound_effect(orig) {
        return false;
    }
    // 译文也必须是 [...] 格式
    let trans_trimmed = trans.trim();
    if !(trans_trimmed.starts_with('[') && trans_trimmed.ends_with(']')) {
        return false;
    }
    // 提取括号内的内容
    let trans_inner = &trans_trimmed[1..trans_trimmed.len()-1];
    // 译文必须含 CJK（说明模型确实翻译了一部分）
    if !has_cjk(trans_inner) {
        return false;
    }
    // 剥离 <name=...> 和 </name> 标签后再提取英文 token，
    // 否则标签中的 "name" 会被误认为未翻译的英文单词（如 [<name=Sweet Marie>甜玛丽</name>尖叫]）
    let re_name_tag = regex::Regex::new(r#"(?i)</?name(?:=[^>]*)?\s*>|<name=[A-Za-z\s]*"#).unwrap();
    let cleaned = re_name_tag.replace_all(trans_inner, "");
    // 提取译文括号内的英文 token（≥3 字母字符）
    let tokens = extract_english_tokens(&cleaned, 3);
    if tokens.is_empty() {
        return false;
    }
    // 有英文 token 且含小写单词 → 半翻译
    // 全大写（如 CEO, MPS）→ 缩写，跳过
    for token in &tokens {
        if token.chars().filter(|c| c.is_ascii_alphabetic()).all(|c| c.is_ascii_lowercase()) {
            return true;
        }
        // 含连字符 → 结巴模式
        if token.contains('-') {
            return true;
        }
    }
    false
}

/// 判断字符是否为 CJK 字符（包括中日韩统一表意文字和 CJK 标点符号）
fn is_cjk_char(c: char) -> bool {
    let code = c as u32;
    // CJK 统一表意文字
    (0x4E00..=0x9FFF).contains(&code)
    // CJK 符号和标点（。、等）
    || (0x3000..=0x303F).contains(&code)
    // 全角形式（！？，等）
    || (0xFF00..=0xFFEF).contains(&code)
}

/// 清理 CJK 字符之间的异常空格
/// GLM-5.2 等模型会将英文单词边界保留为空格，导致中文之间出现多余空格
/// 如 "我 知道 我带了" → "我知道我带了"
/// "。 我" → "。我"
/// 保留 CJK 与非 CJK（拉丁字母、标签等）之间的空格
pub fn cleanup_cjk_spaces(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        result.push(chars[i]);
        if is_cjk_char(chars[i]) {
            // 收集后续空格
            let mut j = i + 1;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            // 如果空格后是 CJK 字符，跳过空格
            if j < chars.len() && is_cjk_char(chars[j]) {
                i = j; // 直接跳到下一个 CJK 字符
            } else {
                // 保留空格
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    result
}

/// 统一音效标记的括号格式：将中文全角括号【】转为半角方括号 []
/// 部分翻译引擎（如小牛翻译）会把 [ Error dings ] 翻译成 【错误叮当声】，
/// 导致音效标记格式不一致，影响后续的音效检测和导出逻辑。
/// 只替换包裹音效内容的 【...】 → [...]，不影响其他用途的【】
pub fn normalize_sound_effect_brackets(s: &str) -> String {
    // 简单替换：【 → [，】 → ]
    // 音效标记的【】只出现在译文中，且整段被【】包裹，直接替换即可
    s.replace('【', "[").replace('】', "]")
}

/// 检测多行原文中非音效行是否在译文中丢失
/// 例如原文 "from our mothers!\n[ Mup crying ]" → 译文 "[Mup 哭泣]"
/// 原文有非音效行 "from our mothers!"，但译文只有音效标记，说明非音效行被丢失
pub(crate) fn has_lost_non_sound_lines(orig: &str, trans: &str) -> bool {
    // 原文必须是多行
    if !orig.contains('\n') {
        return false;
    }
    // 译文必须是音效标记（只含音效标记的内容）
    if !looks_like_sound_effect(trans) {
        return false;
    }
    // 检查原文是否有至少一行非音效行
    let has_non_sound = orig.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !looks_like_sound_effect(trimmed) && !is_music_or_symbol_only(trimmed)
    });
    has_non_sound
}

/// 如果内容被代码块包裹，返回代码块内部内容；否则原样返回
pub(crate) fn strip_markdown_code_fence(s: &str) -> String {
    let s = s.trim();
    if !s.starts_with("```") {
        return s.to_string();
    }
    // 去掉第一行（```json 或 ```）
    let after_first_line = match s.find('\n') {
        Some(idx) => &s[idx + 1..],
        None => return s.to_string(),
    };
    // 去掉最后的 ``` 行
    let result = after_first_line.trim_end();
    if let Some(stripped) = result.strip_suffix("```") {
        stripped.trim().to_string()
    } else {
        result.to_string()
    }
}

/// 将字段内的 `|` 双写转义，确保拼接后可无歧义还原
pub fn escape_field(s: &str) -> String {
    s.replace('|', "||")
}

/// 拼接无歧义的缓存 provider_name：seg1|seg2|seg3，每段先转义
/// 用于 translate_subtitle / get_cached_translations 构造缓存 key 的 provider 字段
/// 保证不同输入产生不同字符串（无碰撞），从而缓存 key 自然隔离
pub fn build_cache_provider_name(segments: &[&str]) -> String {
    segments.iter().map(|s| escape_field(s)).collect::<Vec<_>>().join("|")
}


/// 占位符基字符（Unicode 私用区 U+E000）
const PLACEHOLDER_BASE: u32 = 0xE000;

/// 支持常见字幕 HTML 标签及其闭合形式，不限制标签长度。
/// 排除普通文本中的 `<` / `>` 符号（如数学表达式 `a < b`）。
pub(crate) fn is_html_subtitle_tag(tag: &str) -> bool {
    // 必须以 < 开头、> 结尾
    if !tag.starts_with('<') || !tag.ends_with('>') {
        return false;
    }
    // 提取标签名：跳过 < 和可选的 /
    let inner = &tag[1..tag.len() - 1]; // 去掉 < >
    let name_part = inner.strip_prefix('/').unwrap_or(inner);
    // 标签名到第一个空格或属性为止
    let tag_name = name_part.split_whitespace().next().unwrap_or(name_part);
    if tag_name.is_empty() {
        return false;
    }
    // 标签名必须全为字母（排除 <3、<.5 等非标签）
    if !tag_name.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    // 已知 HTML 字幕标签白名单
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "b" | "i" | "u" | "s" | "font" | "span" | "div" | "p" | "br"
        | "strong" | "em" | "mark" | "strike" | "sub" | "sup" | "small"
        | "big" | "tt" | "code" | "pre" | "blockquote" | "ruby" | "rt" | "rp"
    )
}

/// 占位符保护器
#[derive(Clone)]
pub struct PlaceholderProtector {
    /// 占位符映射表：占位符字符串 -> 原始文本
    placeholders: Vec<(String, String)>,
    /// 占位符策略
    strategy: PlaceholderStrategy,
}

#[allow(dead_code)]
impl PlaceholderProtector {
    pub fn new() -> Self {
        Self::with_strategy(PlaceholderStrategy::PrivateUse)
    }

    /// 使用指定策略创建保护器
    pub fn with_strategy(strategy: PlaceholderStrategy) -> Self {
        Self {
            placeholders: Vec::new(),
            strategy,
        }
    }

    /// 保护文本中的需保护片段，返回含占位符的文本
    pub fn protect(&mut self, text: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut remaining = text;

        while !remaining.is_empty() {
            // 检测 ass 样式标记 {\...}
            if remaining.starts_with('{') {
                if let Some(end) = remaining.find('}') {
                    let tag = &remaining[..=end];
                    let placeholder = self.add_placeholder(tag);
                    result.push_str(&placeholder);
                    remaining = &remaining[end + 1..];
                    continue;
                }
            }

            // 检测 HTML 标签 <...>
            if remaining.starts_with('<') {
                if let Some(end) = remaining.find('>') {
                    let tag = &remaining[..=end];
                    // 保护常见 HTML 字幕标签（含闭合标签），不保护普通 < > 符号
                    // 不再限制标签长度，支持 <span>/<div> 等任意标签
                    if is_html_subtitle_tag(tag) {
                        // DirectHtml 策略：HTML 标签不保护，直接发送给引擎
                        // 引擎原生支持 HTML 标签处理（DeepL/Google/Bing）
                        if self.strategy == PlaceholderStrategy::DirectHtml {
                            result.push_str(tag);
                            remaining = &remaining[end + 1..];
                            continue;
                        }
                        let placeholder = self.add_placeholder(tag);
                        result.push_str(&placeholder);
                        remaining = &remaining[end + 1..];
                        continue;
                    }
                }
            }

            // 检测连续换行符（\N 或 \n 在 ass 中是强制换行）
            if remaining.starts_with("\\N") || remaining.starts_with("\\n") {
                let tag = &remaining[..2];
                let placeholder = self.add_placeholder(tag);
                result.push_str(&placeholder);
                remaining = &remaining[2..];
                continue;
            }

            // 检测真正的换行符（SRT 中的 \n 0x0A）
            // 替换成占位符而非保留原样，避免 9b 模型把多行条目拆成多条翻译导致错位
            if remaining.starts_with('\n') {
                let placeholder = self.add_placeholder("\n");
                result.push_str(&placeholder);
                remaining = &remaining[1..];
                continue;
            }

            // 普通字符直接输出
            let ch = remaining.chars().next().unwrap();
            result.push(ch);
            remaining = &remaining[ch.len_utf8()..];
        }

        result
    }

    /// 回填占位符，将翻译后的文本中的占位符替换回原始内容
    pub fn restore(&self, text: &str) -> String {
        // 修复模型输出中格式标签的空格变异："< x0/>" → "<x0/>", "< /x0>" → "</x0>"
        let mut result = text.to_string();
        if matches!(self.strategy, PlaceholderStrategy::XmlTags) {
            let re = regex::Regex::new(r"<\s+(/?x\d+/?\s*>)").unwrap();
            result = re.replace_all(&result, "<$1").to_string();
        }
        // 按占位符长度降序替换，避免短占位符是长占位符的前缀导致误替换
        let mut sorted = self.placeholders.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for (placeholder, original) in &sorted {
            result = result.replace(placeholder, original);
        }
        result
    }

    /// 添加占位符映射，返回占位符字符串
    fn add_placeholder(&mut self, original: &str) -> String {
        let index = self.placeholders.len();
        let placeholder = match self.strategy {
            PlaceholderStrategy::PrivateUse => {
                if index >= 256 {
                    tracing::warn!("占位符超过 256 个上限，直接保留原文");
                    return original.to_string();
                }
                char::from_u32(PLACEHOLDER_BASE + index as u32)
                    .unwrap_or('\u{E0FF}')
                    .to_string()
            }
            PlaceholderStrategy::XmlTags => {
                // <xN></xN> 形式：每个标签用唯一编号（全局 index）
                // restore 按精确字符串匹配，不需要开/闭标签编号配对
                if original.starts_with("</") {
                    format!("</x{}>", index)
                } else if original.starts_with('<') {
                    format!("<x{}>", index)
                } else {
                    // 换行符等非标签内容
                    format!("<x{}/>", index)
                }
            }
            PlaceholderStrategy::DirectHtml => {
                // HTML 标签不保护（在 protect 中已处理），这里只处理 ASS 标签和换行符
                // 用私用区字符保护 ASS 标签和换行符
                if index >= 256 {
                    return original.to_string();
                }
                char::from_u32(PLACEHOLDER_BASE + index as u32)
                    .unwrap_or('\u{E0FF}')
                    .to_string()
            }
            PlaceholderStrategy::CurlyBraces => {
                // {N}{/N} 形式：每个标签用唯一编号
                if original.starts_with("</") {
                    format!("{{/{}}}", index)
                } else if original.starts_with('<') {
                    format!("{{{}}}", index)
                } else {
                    format!("{{n{}}}", index)
                }
            }
            PlaceholderStrategy::SquareBrackets => {
                // [N][/N] 形式：每个标签用唯一编号
                if original.starts_with("</") {
                    format!("[/{}]", index)
                } else if original.starts_with('<') {
                    format!("[{}]", index)
                } else {
                    format!("[n{}]", index)
                }
            }
        };
        self.placeholders.push((placeholder.clone(), original.to_string()));
        placeholder
    }

    /// 获取占位符数量
    pub fn placeholder_count(&self) -> usize {
        self.placeholders.len()
    }

    /// 回填占位符并恢复丢失的 ASS 前缀标签
    /// 比 restore 多两步：
    /// 1. 9b 模型可能丢弃占位符字符（U+E000~U+E0FF），导致 ASS 标签（如 {\an8}）在译文中丢失。
    ///    此方法检测丢失的前缀 ASS 标签并加回译文开头。
    /// 2. 9b 模型可能多输出未注册的占位符字符（如重复、错位），
    ///    restore() 只能替换已注册的占位符，无法清除多余字符。
    ///    此方法清除所有残留的占位符字符，避免污染译文导致 failed 误判。
    pub fn restore_with_ass_recovery(&self, text: &str, original_text: &str) -> String {
        let restored = self.restore(text);
        let recovered = recover_lost_ass_prefix_tags(&restored, original_text);
        strip_remaining_placeholders(&recovered, self.strategy)
    }

    /// 获取当前策略
    pub fn strategy(&self) -> PlaceholderStrategy {
        self.strategy
    }
}


/// 清除译文中残留的占位符
/// 9b 模型有时会多输出未注册的占位符（如重复、错位），
/// 这些占位符无法被 restore() 替换回原始内容，会污染译文。
pub(crate) fn strip_remaining_placeholders(s: &str, strategy: PlaceholderStrategy) -> String {
    match strategy {
        // 私用区字符和 DirectHtml（ASS 标签用私用区）：清除 U+E000~U+E0FF
        PlaceholderStrategy::PrivateUse | PlaceholderStrategy::DirectHtml => {
            s.chars().filter(|&ch| !('\u{E000}'..='\u{E0FF}').contains(&ch)).collect()
        }
        // XML 标签：清除残留的 <xN>、</xN>、<xN/>
        PlaceholderStrategy::XmlTags => {
            let re = regex::Regex::new(r"</?x\d+/??>").unwrap();
            re.replace_all(s, "").to_string()
        }
        // 花括号：清除残留的 {N}、{/N}、{nN}
        PlaceholderStrategy::CurlyBraces => {
            let re = regex::Regex::new(r"\{/?\d+\}|\{n\d+\}").unwrap();
            re.replace_all(s, "").to_string()
        }
        // 方括号：清除残留的 [N]、[/N]、[nN]
        PlaceholderStrategy::SquareBrackets => {
            let re = regex::Regex::new(r"\[/?\d+\]|\[n\d+\]").unwrap();
            re.replace_all(s, "").to_string()
        }
    }
}

/// 清理译文中泄漏的 JSON 格式语法
/// 9b 模型有时返回 JSON 数组格式（如 [{"n": 1, "t": "..."}]），但 JSON 解析因未转义引号而失败，
/// 回退到行对齐时 JSON 语法会被当作译文文本，导致译文中出现：
/// - 完整 JSON 包装：[{"n": 1, "t": "让我们看看发生了什么事。"}]
/// - JSON 语法残留：译文末尾出现 '},\n  { 等字符
/// 此函数提取 JSON 中的实际文本，剥离 JSON 语法残留。
pub(crate) fn clean_json_leak(s: &str) -> String {
    let trimmed = s.trim();

    // 1. 完整 JSON 数组包装：[{"n": N, "t": "..."}]
    // 提取 "t" 字段的值
    if trimmed.starts_with("[{\"n\":") || trimmed.starts_with("[{ \"n\":") {
        let re = regex::Regex::new(
            r#""t"\s*:\s*"((?:[^"\\]|\\.)*)""#
        ).unwrap();
        if let Some(cap) = re.captures(trimmed) {
            if let Some(t) = cap.get(1) {
                let text = t.as_str()
                    .replace("\\\"", "\"")
                    .replace("\\\\", "\\")
                    .replace("\\n", "\n")
                    .replace("\\t", "\t");
                return text.trim().to_string();
            }
        }
    }

    // 2. 译文末尾有 JSON 数组语法残留（如 '},\n  { 或 "},）
    // JSON 数组元素之间是 "},\n  {" 的模式，检测并截断
    // 匹配 JSON 对象结束符 }, 后跟空白和 {（下一个 JSON 对象的开始）
    let re_json_sep = regex::Regex::new(r"\},\s*\{.*$").unwrap();
    if let Some(m) = re_json_sep.find(trimmed) {
        let before = trimmed[..m.start()].trim_end();
        // 确保截断后仍有实质内容（含 CJK 字符，避免误截正常文本）
        if !before.is_empty() && has_cjk(before) {
            return before.to_string();
        }
    }

    s.to_string()
}

/// 恢复丢失的 ASS 前缀标签
/// 如果原文以 ASS 标签（如 {\an8}）开头但译文丢失了该标签，把丢失的前缀标签加回译文开头
pub(crate) fn recover_lost_ass_prefix_tags(restored: &str, original_text: &str) -> String {
    let orig_prefix_tags = extract_prefix_ass_tags(original_text);
    if orig_prefix_tags.is_empty() {
        return restored.to_string();
    }

    // 译文中已包含的 ASS 标签（不限于前缀，因为 9b 可能改变了标签位置）
    let trans_prefix_tags = extract_prefix_ass_tags(restored);

    // 找出丢失的前缀标签：原文有但译文完全没有的
    let lost_tags: Vec<&String> = orig_prefix_tags
        .iter()
        .filter(|tag| !trans_prefix_tags.contains(tag) && !restored.contains(tag.as_str()))
        .collect();

    if lost_tags.is_empty() {
        return restored.to_string();
    }

    // 把丢失的标签加到译文开头
    let mut result = String::with_capacity(restored.len() + 16);
    for tag in &lost_tags {
        result.push_str(tag);
    }
    result.push_str(restored);
    result
}

/// 提取文本开头的连续 ASS 标签（{\...} 格式）
pub(crate) fn extract_prefix_ass_tags(s: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut remaining = s;
    while remaining.starts_with('{') {
        if let Some(end) = remaining.find('}') {
            tags.push(remaining[..=end].to_string());
            remaining = &remaining[end + 1..];
        } else {
            break;
        }
    }
    tags
}

/// 翻译分段：将长文本按句号/换行切分，确保单段不超过 max_length（按字节计）。
/// 保留原始分隔符（. / \n / ？ / ！ / 。），避免补回错误的分隔符。
pub fn split_text(text: &str, max_length: usize) -> Vec<String> {
    if text.len() <= max_length {
        return vec![text.to_string()];
    }

    // 按句子切分，保留分隔符。分隔符视为句子结尾的一部分。
    // 句子边界字符：. ! ? 。 ！ ？ \n
    fn is_sentence_boundary(c: char) -> bool {
        matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '\n')
    }

    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if is_sentence_boundary(c) {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }

    // 贪心合并句子到段，每段不超过 max_length 字节
    let mut segments = Vec::new();
    let mut buf = String::new();
    for sentence in &sentences {
        if !buf.is_empty() && buf.len() + sentence.len() > max_length {
            segments.push(buf.trim().to_string());
            buf.clear();
        }
        if sentence.len() > max_length {
            // 单句超限：按字符硬切
            if !buf.is_empty() {
                segments.push(buf.trim().to_string());
                buf.clear();
            }
            let chars: Vec<char> = sentence.chars().collect();
            let mut chunk = String::new();
            for c in &chars {
                let next_len = chunk.len() + c.len_utf8();
                if next_len > max_length && !chunk.is_empty() {
                    segments.push(chunk.clone());
                    chunk.clear();
                }
                chunk.push(*c);
            }
            if !chunk.is_empty() {
                buf = chunk;
            }
        } else {
            buf.push_str(sentence);
        }
    }
    if !buf.is_empty() {
        segments.push(buf.trim().to_string());
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleanup_cjk_spaces_basic() {
        // CJK 之间的空格应被移除
        assert_eq!(cleanup_cjk_spaces("我 知道"), "我知道");
        assert_eq!(cleanup_cjk_spaces("我 知道 我带了"), "我知道我带了");
        // 多个连续空格
        assert_eq!(cleanup_cjk_spaces("我  知道"), "我知道");
        // CJK 标点与 CJK 之间
        assert_eq!(cleanup_cjk_spaces("。 我"), "。我");
        assert_eq!(cleanup_cjk_spaces("， 莫蒂"), "，莫蒂");
    }

    #[test]
    fn test_cleanup_cjk_spaces_preserve() {
        // CJK 与拉丁字母之间的空格应保留
        assert_eq!(cleanup_cjk_spaces("操纵 DNA"), "操纵 DNA");
        assert_eq!(cleanup_cjk_spaces("Hyena 3（鬣狗3号）"), "Hyena 3（鬣狗3号）");
        // 标签与 CJK 之间的空格应保留
        assert_eq!(cleanup_cjk_spaces("<x0/> 世界"), "<x0/> 世界");
        // 无空格的文本不变
        assert_eq!(cleanup_cjk_spaces("你好世界"), "你好世界");
        // 纯英文不变
        assert_eq!(cleanup_cjk_spaces("Hello world"), "Hello world");
    }

    #[test]
    fn test_normalize_sound_effect_brackets() {
        // 全角【】→ 半角[]
        assert_eq!(normalize_sound_effect_brackets("【错误叮当声】"), "[错误叮当声]");
        assert_eq!(normalize_sound_effect_brackets("【电噼啪声】哦!"), "[电噼啪声]哦!");
        // 混合场景：音效标记 + 正文
        assert_eq!(normalize_sound_effect_brackets("【错误的事情】啊!啊!"), "[错误的事情]啊!啊!");
        // 无【】的文本不变
        assert_eq!(normalize_sound_effect_brackets("[错误叮当声]"), "[错误叮当声]");
        assert_eq!(normalize_sound_effect_brackets("你好世界"), "你好世界");
        // 多个【】
        assert_eq!(normalize_sound_effect_brackets("【音效1】【音效2】"), "[音效1][音效2]");
    }

    #[test]
    fn test_cleanup_cjk_spaces_mixed() {
        // 混合场景
        assert_eq!(cleanup_cjk_spaces("我只喝了几杯 酒而已，莫蒂！"), "我只喝了几杯酒而已，莫蒂！");
        assert_eq!(cleanup_cjk_spaces("这破玩意儿 把我锁在外面了！"), "这破玩意儿把我锁在外面了！");
        // CJK 空格 CJK 空格 CJK
        assert_eq!(cleanup_cjk_spaces("我 知 道"), "知道".chars().fold("我".to_string(), |acc, c| acc + &c.to_string()));
    }

    #[test]
    fn test_is_partial_sound_effect() {
        // 半翻译：[ All grunting ] → [ 所有人发出 grunt 声 ]
        assert!(is_partial_sound_effect("[ All grunting ]", "[ 所有人发出 grunt 声 ]"));
        // 半翻译：[ All laughing ] → [所有人 laughing]
        assert!(is_partial_sound_effect("[ All laughing ]", "[所有人 laughing]"));
        // 正确翻译：[ All laughing ] → [所有人笑]
        assert!(!is_partial_sound_effect("[ All laughing ]", "[所有人笑]"));
        // 正确翻译：[ Water splashes ] → [ 水花溅起 ]
        assert!(!is_partial_sound_effect("[ Water splashes ]", "[ 水花溅起 ]"));
        // 正确翻译：[ Groans ] → [ 呻吟 ]
        assert!(!is_partial_sound_effect("[ Groans ]", "[ 呻吟 ]"));
        // 非音效标记
        assert!(!is_partial_sound_effect("Hello world", "你好世界"));
        // 译文不含 CJK（未翻译）
        assert!(!is_partial_sound_effect("[ All grunting ]", "[ All grunting ]"));
        // 译文不含英文（完全翻译）
        assert!(!is_partial_sound_effect("[ All grunting ]", "[ 所有人咕哝 ]"));
        // 含全大写缩写（不应标记）
        assert!(!is_partial_sound_effect("[ CEO meeting ]", "[ CEO会议 ]"));
        // 含 <name> 标签的正确翻译不应误判为半翻译
        // [<name=Sweet Marie>甜玛丽</name>尖叫] 中 "name" 来自标签，不是未翻译的英文
        assert!(!is_partial_sound_effect("[ Sweet Marie screams ]", "[<name=Sweet Marie>甜玛丽</name>尖叫]"));
        // <name>EnglishName</name>ChineseName 格式也不应误判
        assert!(!is_partial_sound_effect("[ Sweet Marie screams ]", "[<name>Sweet Marie</name>甜玛丽尖叫]"));
    }

    #[test]
    fn test_is_partial_translation_untranslated_sound_effect() {
        // 混合行：音效标记未翻译，对白已翻译
        // "[ Crash ] Titus: Oh, shit!" → "[ Crash ] 泰特斯: 哦，该死！"
        assert!(is_partial_translation(
            "[ Crash ] Titus: Oh, shit!",
            "[ Crash ] 泰特斯: 哦，该死！"
        ));
        // "[ Tires screech ] God damn it!" → "[ Tires screech ] 该死！"
        assert!(is_partial_translation(
            "[ Tires screech ] God damn it!",
            "[ Tires screech ] 该死！"
        ));
        // 正确翻译：音效标记已翻译
        assert!(!is_partial_translation(
            "[ Crash ] Titus: Oh, shit!",
            "[碰撞声] 泰特斯: 哦，该死！"
        ));
        // 纯音效标记行（由 is_partial_sound_effect 处理，不在此检测）
        assert!(!is_partial_translation(
            "[ All grunting ]",
            "[ 所有人发出 grunt 声 ]"
        ));
        // 全大写缩写不应标记
        assert!(!is_partial_translation(
            "[ CEO ] The boss is here.",
            "[ CEO ] 老板来了。"
        ));
        // 无音效标记的行不受影响
        assert!(!is_partial_translation(
            "Hello world this is a test",
            "你好世界这是一个测试"
        ));
        // 译文中括号内容已翻译（含 CJK）不应标记
        assert!(!is_partial_translation(
            "[ Mancors shrieking ] Aww, man!",
            "[曼科斯尖叫声] 哦，老天！"
        ));
        // 损坏的音效标记：[SSEARCH] 代替 [ Sighs ]
        assert!(is_partial_translation(
            "[ Sighs ] No.",
            "[SSEARCH]否。"
        ));
        // 空括号：原文有 [ Sighs ]，译文有 []
        assert!(is_partial_translation(
            "Ahh. [ Sighs ]",
            "啊。[]"
        ));
        // 丢失的音效标记：原文有 [...] 但译文无方括号
        assert!(is_partial_translation(
            "Evil boy. Wicked boy. [ Cellphone ringing ]",
            "邪恶的男孩。坏小子。"
        ));
        assert!(is_partial_translation(
            "[ Intercom static ] Reese is my friend!",
            "里斯是我的好朋友！"
        ));
        // 译文无 CJK 时不检测丢失（可能整行未翻译，由其他逻辑处理）
        assert!(!is_partial_translation(
            "[ Cellphone ringing ] Hello world",
            "[ Cellphone ringing ] Hello world"
        ));
        // 正确翻译的音效不应标记
        assert!(!is_partial_translation(
            "[ Cellphone ringing ] Hello world",
            "[手机铃声] 你好世界"
        ));
    }
}

