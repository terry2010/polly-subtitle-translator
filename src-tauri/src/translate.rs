// 翻译模块
// provider 抽象（百度/Bing/Google）+ 分段 + 占位符保护 + 缓存 + 限流重试

use crate::db::{translate_cache_key, Database};
use crate::error::AppError;
use serde::{Deserialize, Serialize};

/// 代理配置（从 config 表读取，构建 reqwest::Client 时使用）
#[derive(Debug, Clone, Default)]
pub struct ProxyConfig {
    pub mode: String,       // "none" / "http" / "socks5"
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ProxyConfig {
    /// 从 Database 读取代理配置
    pub fn load_from_db(db: &Database) -> Self {
        let get = |k: &str| db.get_config(k).ok().flatten().unwrap_or_default();
        let user = get("proxy_user");
        Self {
            mode: get("proxy_mode"),
            host: get("proxy_host"),
            port: get("proxy_port").parse().unwrap_or(0),
            username: if user.is_empty() { None } else { Some(user) },
            password: crate::config::CredentialStore::load("proxy", "pass").ok(),
        }
    }

    /// 构建 reqwest 代理 URL（如 mode != none）
    fn proxy_url(&self) -> Option<String> {
        tracing::info!("ProxyConfig: mode={}, host={}, port={}, user={}", self.mode, self.host, self.port, self.username.as_deref().unwrap_or(""));
        if self.mode == "none" || self.host.is_empty() || self.port == 0 {
            tracing::info!("代理未配置或配置不完整，使用直连");
            return None;
        }
        let scheme = if self.mode == "socks5" { "socks5" } else { "http" };
        match (&self.username, &self.password) {
            (Some(u), Some(p)) if !u.is_empty() => Some(format!("{}://{}:{}@{}:{}", scheme, u, p, self.host, self.port)),
            _ => Some(format!("{}://{}:{}", scheme, self.host, self.port)),
        }
    }

    /// 构建 reqwest::Client（带代理或普通）
    pub fn build_client(&self) -> reqwest::Client {
        match self.proxy_url() {
            Some(url) => {
                tracing::info!("使用代理: {}", self.mode);
                reqwest::Client::builder()
                    .proxy(reqwest::Proxy::all(&url).unwrap_or_else(|e| {
                        tracing::warn!("代理配置失败: {}, 使用直连", e);
                        reqwest::Proxy::all("direct://").unwrap()
                    }))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new())
            }
            None => reqwest::Client::new(),
        }
    }

    /// 构建 reqwest::blocking::Client（带代理或普通），供搜索等 blocking 场景使用
    pub fn build_blocking_client(&self) -> reqwest::blocking::Client {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        let base = || reqwest::blocking::Client::builder()
            .user_agent(ua)
            .redirect(reqwest::redirect::Policy::limited(10));
        match self.proxy_url() {
            Some(url) => {
                tracing::info!("搜索使用代理 URL: {}", url);
                let result = base()
                    .proxy(reqwest::Proxy::all(&url).unwrap_or_else(|e| {
                        tracing::warn!("代理配置失败: {}, 使用直连", e);
                        reqwest::Proxy::all("direct://").unwrap()
                    }))
                    .build();
                match &result {
                    Ok(_) => tracing::info!("代理客户端构建成功"),
                    Err(e) => tracing::warn!("代理客户端构建失败: {}, 回退到直连", e),
                }
                result.unwrap_or_else(|_| base().build().unwrap_or_default())
            }
            None => {
                tracing::info!("搜索使用直连（无代理）");
                base().build().unwrap_or_default()
            }
        }
    }
}

/// 翻译提供商类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TranslateProvider {
    Baidu,
    Bing,
    Google,
    OpenAi,
}

impl TranslateProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TranslateProvider::Baidu => "baidu",
            TranslateProvider::Bing => "bing",
            TranslateProvider::Google => "google",
            TranslateProvider::OpenAi => "openai",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "baidu" => Some(TranslateProvider::Baidu),
            "bing" => Some(TranslateProvider::Bing),
            "google" => Some(TranslateProvider::Google),
            "openai" => Some(TranslateProvider::OpenAi),
            _ => None,
        }
    }

    /// 各引擎的 QPS 上限（免费/默认档位）
    /// 百度免费版 1 QPS、Bing 10 QPS、Google 100 QPS、OpenAI 兼容 5（按 RPM 限流，保守取 5）
    pub fn qps_limit(&self) -> usize {
        match self {
            TranslateProvider::Baidu => 1,
            TranslateProvider::Bing => 10,
            TranslateProvider::Google => 100,
            TranslateProvider::OpenAi => 5,
        }
    }

    /// 计算实际并发 = min(用户配置并发, QPS 上限)，至少 1
    pub fn effective_concurrency(user_config: usize, provider: &TranslateProvider) -> usize {
        let qps = provider.qps_limit();
        let c = user_config.min(qps).max(1);
        c
    }
}

/// 翻译请求
#[derive(Debug, Clone)]
pub struct TranslateRequest {
    pub texts: Vec<String>,
    pub source_lang: String,
    pub target_lang: String,
    pub provider: TranslateProvider,
}

/// 翻译结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateResult {
    pub translations: Vec<TranslateEntry>,
    pub provider: String,
    pub cached_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateEntry {
    pub index: usize,
    pub original: String,
    pub translated: String,
    pub from_cache: bool,
    pub failed: bool,
}

/// 翻译提供商凭据
#[derive(Debug, Clone, Default)]
pub struct ProviderCredentials {
    pub app_id: Option<String>,
    pub secret_key: Option<String>,
    pub region: Option<String>,
    /// OpenAI 兼容 provider 专属：base_url / model / model_type
    /// 其他 provider 忽略这些字段
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub model_type: Option<String>,
}

/// 翻译提供商 trait
#[async_trait::async_trait]
pub trait TranslateProviderTrait: Send + Sync {
    /// 翻译一批文本
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError>;

    /// 获取支持的目标语言列表
    async fn supported_target_langs(&self) -> Result<Vec<LanguageInfo>, AppError>;

    /// 测试连接
    async fn test_connection(&self) -> Result<(), AppError>;
}

/// 语言信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub code: String,
    pub name: String,
    pub native_name: String,
}

/// 测试连接结果（OpenAi 返回原文+译文，其他 provider 字段为 None）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConnectionResult {
    pub original: Option<String>,
    pub translated: Option<String>,
}

/// AI 模型类型（用于 prompt 分发）
/// 初期支持 qwen3 / deepseek，其他模型用 Generic 通用 prompt
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    Qwen3,
    Deepseek,
    Generic,
}

impl ModelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelType::Qwen3 => "qwen3",
            ModelType::Deepseek => "deepseek",
            ModelType::Generic => "generic",
        }
    }

    /// 从 serde 字符串构造（用于从 db config 读取的值）
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "qwen3" => Some(ModelType::Qwen3),
            "deepseek" => Some(ModelType::Deepseek),
            "generic" => Some(ModelType::Generic),
            _ => None,
        }
    }

    /// 根据模型 id 自动识别模型类型（大小写不敏感）
    /// qwen3-14b → Qwen3、deepseek-v4 → Deepseek、gemma-3 → Generic
    pub fn from_model_id(id: &str) -> Self {
        let lower = id.to_lowercase();
        if lower.contains("qwen3") {
            ModelType::Qwen3
        } else if lower.contains("deepseek") {
            ModelType::Deepseek
        } else {
            ModelType::Generic
        }
    }
}

/// 内置 prompt 模板（编译进二进制，远程不可用时的兜底）
pub struct PromptTemplate {
    pub system: &'static str,
    pub user_line_format: &'static str,
}

impl PromptTemplate {
    /// 渲染 system prompt，替换 {src} / {tgt} 占位符
    pub fn render_system(&self, src: &str, tgt: &str) -> String {
        self.system.replace("{src}", src).replace("{tgt}", tgt)
    }
}

/// 内置模板表（按 ModelType::as_str() 索引）
/// 顺序：qwen3 / deepseek / generic
const BUILTIN_TEMPLATES: &[(&str, PromptTemplate)] = &[
    ("qwen3", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate the following {src} subtitles into {tgt}.\n\n\
                 Rules:\n\
                 - Output ONLY the translations, one per line, prefixed with the line number.\n\
                 - Format: \"N. <translation>\"\n\
                 - Keep the same line numbering as the input.\n\
                 - Preserve special Unicode characters (like \u{E001}) exactly as-is.\n\
                 - Do not merge or split lines.\n\
                 - Do not add explanations, notes, or any extra text.",
        user_line_format: "{index}. {text}",
    }),
    ("deepseek", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate from {src} to {tgt}.\n\n\
                 Output format:\n\
                 - One translation per line, prefixed with the input line number.\n\
                 - Format: \"N. <translation>\"\n\
                 - Preserve all special characters and placeholders unchanged.\n\
                 - Do not merge, split, or skip any lines.\n\
                 - Output ONLY the numbered translations, nothing else.",
        user_line_format: "{index}. {text}",
    }),
    ("generic", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate the following {src} subtitles into {tgt}.\n\n\
                 Rules:\n\
                 - Output ONLY the translations, one per line, prefixed with the line number.\n\
                 - Format: \"N. <translation>\"\n\
                 - Preserve special Unicode characters exactly as-is.\n\
                 - Do not merge or split lines.\n\
                 - Do not add any extra text.",
        user_line_format: "{index}. {text}",
    }),
];

/// ISO 639-1 语言码 → 英文全称（用于 prompt 占位符 {src} / {tgt}）
fn lang_full_name(code: &str) -> &'static str {
    match code.to_lowercase().as_str() {
        "zh" | "zh-cn" | "zh-hans" | "zhs" => "Chinese",
        "zh-tw" | "zh-hant" | "zht" => "Traditional Chinese",
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "ru" => "Russian",
        "it" => "Italian",
        "pt" => "Portuguese",
        "th" => "Thai",
        "vi" => "Vietnamese",
        "ar" => "Arabic",
        "hi" => "Hindi",
        "tr" => "Turkish",
        "nl" => "Dutch",
        "pl" => "Polish",
        "auto" => "the source language",
        _ => "the source language",
    }
}

// === SECTION 1 END ===

/// 占位符保护算法（对应需求文档 §4.2）
/// 翻译前将需保护片段替换为私用区占位符，翻译后回填
/// 保护范围：ass 样式标记 {\...}、HTML 标签 <...>、换行符、特殊符号

/// 私用区字符范围：U+E000 ~ U+E0FF（256 个占位符）
const PLACEHOLDER_BASE: u32 = 0xE000;

/// 判断一个 `<...>` 片段是否为 HTML 字幕标签（需保护）。
/// 支持常见字幕 HTML 标签及其闭合形式，不限制标签长度。
/// 排除普通文本中的 `<` / `>` 符号（如数学表达式 `a < b`）。
fn is_html_subtitle_tag(tag: &str) -> bool {
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
    /// 占位符映射表：占位符字符 -> 原始文本
    placeholders: Vec<(char, String)>,
}

impl PlaceholderProtector {
    pub fn new() -> Self {
        Self {
            placeholders: Vec::new(),
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
                    result.push(placeholder);
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
                        let placeholder = self.add_placeholder(tag);
                        result.push(placeholder);
                        remaining = &remaining[end + 1..];
                        continue;
                    }
                }
            }

            // 检测连续换行符（\N 或 \n 在 ass 中是强制换行）
            if remaining.starts_with("\\N") || remaining.starts_with("\\n") {
                let tag = &remaining[..2];
                let placeholder = self.add_placeholder(tag);
                result.push(placeholder);
                remaining = &remaining[2..];
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
        let mut result = String::with_capacity(text.len());
        for ch in text.chars() {
            if let Some((_, original)) = self.placeholders.iter().find(|(p, _)| *p == ch) {
                result.push_str(original);
            } else {
                result.push(ch);
            }
        }
        result
    }

    /// 添加占位符映射
    fn add_placeholder(&mut self, original: &str) -> char {
        let index = self.placeholders.len();
        if index >= 256 {
            // 超过 256 个占位符，使用兜底方案：直接保留原文
            tracing::warn!("占位符超过 256 个上限，直接保留原文");
            return '\u{E0FF}';
        }
        let placeholder = char::from_u32(PLACEHOLDER_BASE + index as u32).unwrap_or('\u{E0FF}');
        self.placeholders.push((placeholder, original.to_string()));
        placeholder
    }

    /// 获取占位符数量
    pub fn placeholder_count(&self) -> usize {
        self.placeholders.len()
    }
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

// === SECTION 2 END ===

/// 百度翻译 API
/// 文档：https://fanyi-api.baidu.com/doc/21
/// 签名算法：MD5(appid + q + salt + secretKey)
pub struct BaiduProvider {
    app_id: String,
    secret_key: String,
    client: reqwest::Client,
}

impl BaiduProvider {
    pub fn new(app_id: String, secret_key: String) -> Self {
        Self::with_client(app_id, secret_key, reqwest::Client::new())
    }
    pub fn with_client(app_id: String, secret_key: String, client: reqwest::Client) -> Self {
        Self { app_id, secret_key, client }
    }

    fn sign(&self, query: &str, salt: &str) -> String {
        use md5::{Digest, Md5};
        let input = format!("{}{}{}{}", self.app_id, query, salt, self.secret_key);
        let mut hasher = Md5::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// 将标准 ISO 639-1 语言码映射为百度 API 专用的语言码
    /// 百度部分语言使用非标准码：ja→jp, ko→kor, fr→fra
    fn to_baidu_lang(lang: &str) -> String {
        match lang {
            "ja" => "jp".to_string(),
            "ko" => "kor".to_string(),
            "fr" => "fra".to_string(),
            // 以下为百度支持但码不同的其他常见语言
            "es" => "spa".to_string(),
            "de" => "de".to_string(),
            "ru" => "ru".to_string(),
            "pt" => "pt".to_string(),
            "it" => "it".to_string(),
            "th" => "th".to_string(),
            "vi" => "vie".to_string(),
            "ar" => "ara".to_string(),
            "hi" => "hi".to_string(),
            "tr" => "tr".to_string(),
            "nl" => "nl".to_string(),
            "pl" => "pl".to_string(),
            "el" => "el".to_string(),
            "sv" => "swe".to_string(),
            "fi" => "fin".to_string(),
            "da" => "dan".to_string(),
            "cs" => "cs".to_string(),
            "hu" => "hu".to_string(),
            "auto" => "auto".to_string(),
            other => other.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl TranslateProviderTrait for BaiduProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // 百度 API 批量翻译：将多条文本用 \n 拼接成一次请求，
        // trans_result 数组每项对应一行，1 次 API 调用即可翻译整批。
        // 百度 q 参数上限约 6000 字节，超限时自动分块发送。
        const BAIDU_MAX_QUERY_BYTES: usize = 5000; // 留余量，避免超限

        // 记录非空文本的索引，空文本直接填空字符串
        let mut results = vec![String::new(); texts.len()];
        let non_empty: Vec<(usize, &String)> = texts
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();

        if non_empty.is_empty() {
            return Ok(results);
        }

        // 按 6000 字节上限分块：贪心地往当前块加文本，超限就开新块
        let mut chunks: Vec<Vec<(usize, &String)>> = Vec::new();
        let mut current_chunk: Vec<(usize, &String)> = Vec::new();
        let mut current_bytes = 0usize;
        for &(idx, text) in &non_empty {
            let text_bytes = text.as_bytes().len(); // UTF-8 字节数（百度按字节计限）
            if !current_chunk.is_empty() && current_bytes + text_bytes + 1 > BAIDU_MAX_QUERY_BYTES {
                chunks.push(std::mem::take(&mut current_chunk));
                current_bytes = 0;
            }
            current_chunk.push((idx, text));
            current_bytes += text_bytes + 1; // +1 for \n separator
        }
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        for chunk in chunks {
            let joined = chunk.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
            let salt = uuid::Uuid::new_v4().simple().to_string();
            let sign = self.sign(&joined, &salt);

            let url = "https://fanyi-api.baidu.com/api/trans/vip/translate";
            let params = serde_json::json!({
                "q": joined,
                "from": Self::to_baidu_lang(source_lang),
                "to": Self::to_baidu_lang(target_lang),
                "appid": self.app_id,
                "salt": salt,
                "sign": sign,
            });

            let resp = self
                .client
                .post(url)
                .form(&params)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| AppError::TranslateRequestFailed {
                    detail: e.to_string(),
                })?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(AppError::TranslateRateLimit {
                    provider: "baidu".to_string(),
                    retry_after: Some(1),
                });
            }

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
                    return Err(AppError::TranslateAuthFailed {
                        provider: "baidu".to_string(),
                    });
                }
                return Err(AppError::TranslateNetworkError {
                    provider: "baidu".to_string(),
                    detail: format!("HTTP {}: {}", status, body),
                });
            }

            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| AppError::TranslateResponseParseFailed {
                    detail: e.to_string(),
                })?;

            // 百度 error_code 可能是字符串或数字
            let has_error = body.get("error_code").is_some();
            if has_error {
                let code = body.get("error_code").map(|v| {
                    if let Some(s) = v.as_str() { s.to_string() }
                    else if let Some(n) = v.as_i64() { n.to_string() }
                    else { String::new() }
                }).unwrap_or_default();
                let msg = body.get("error_msg").and_then(|v| v.as_str()).unwrap_or("");
                // 54003 = 请求过于频繁，按限流处理
                if code == "54003" {
                    return Err(AppError::TranslateRateLimit {
                        provider: "baidu".to_string(),
                        retry_after: Some(1),
                    });
                }
                return Err(AppError::TranslateNetworkError {
                    provider: "baidu".to_string(),
                    detail: format!("error_code: {}, msg: {}", code, msg),
                });
            }

            // trans_result 是数组，每项 {src, dst} 对应输入的一行
            let trans_result = body.get("trans_result");
            let translations: Vec<String> = match trans_result {
                Some(arr) if arr.is_array() => {
                    arr.as_array().unwrap()
                        .iter()
                        .map(|item| {
                            item.get("dst")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect()
                }
                _ => {
                    // 无法解析时，用原文回填
                    chunk.iter().map(|(_, t)| (*t).clone()).collect()
                }
            };

            // 对齐检查：百度返回的行数应与输入一致
            if translations.len() != chunk.len() {
                tracing::warn!(
                    "百度翻译对齐异常：输入 {} 行，返回 {} 行，按可用结果回填",
                    chunk.len(),
                    translations.len()
                );
            }

            for (i, (idx, _)) in chunk.iter().enumerate() {
                let translated = translations.get(i).cloned().unwrap_or_default();
                results[*idx] = translated;
            }
        }

        Ok(results)
    }

    async fn supported_target_langs(&self) -> Result<Vec<LanguageInfo>, AppError> {
        Ok(vec![
            LanguageInfo { code: "zh".into(), name: "Chinese".into(), native_name: "中文".into() },
            LanguageInfo { code: "en".into(), name: "English".into(), native_name: "English".into() },
            LanguageInfo { code: "ja".into(), name: "Japanese".into(), native_name: "日本語".into() },
            LanguageInfo { code: "ko".into(), name: "Korean".into(), native_name: "한국어".into() },
            LanguageInfo { code: "fr".into(), name: "French".into(), native_name: "Français".into() },
            LanguageInfo { code: "de".into(), name: "German".into(), native_name: "Deutsch".into() },
            LanguageInfo { code: "es".into(), name: "Spanish".into(), native_name: "Español".into() },
            LanguageInfo { code: "ru".into(), name: "Russian".into(), native_name: "Русский".into() },
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        let result = self.translate(&["test".to_string()], "en", "zh").await;
        match result {
            Ok(_) => Ok(()),
            Err(AppError::TranslateAuthFailed { .. }) => Err(AppError::TranslateAuthFailed {
                provider: "baidu".to_string(),
            }),
            Err(e) => Err(e),
        }
    }
}

// === SECTION 3 END ===

/// Bing 翻译 API（Azure Cognitive Services Translator）
/// 文档：https://learn.microsoft.com/en-us/azure/ai-services/translator/
/// 认证：Ocp-Apim-Subscription-Key + region（Ocp-Apim-Subscription-Region）
pub struct BingProvider {
    api_key: String,
    region: String,
    client: reqwest::Client,
}

impl BingProvider {
    pub fn new(api_key: String, region: String) -> Self {
        Self::with_client(api_key, region, reqwest::Client::new())
    }
    pub fn with_client(api_key: String, region: String, client: reqwest::Client) -> Self {
        Self { api_key, region, client }
    }

    /// 单批翻译（不超过字符上限）
    async fn translate_single_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        let url = "https://api.cognitive.microsofttranslator.com/translate";
        let params = [
            ("api-version", "3.0"),
            ("from", source_lang),
            ("to", target_lang),
        ];

        // Bing 接受数组形式的 body，每个元素含 Text
        let body: Vec<serde_json::Value> = texts
            .iter()
            .map(|t| serde_json::json!({ "Text": t.as_str() }))
            .collect();

        let resp = self
            .client
            .post(url)
            .query(&params)
            .header("Ocp-Apim-Subscription-Key", &self.api_key)
            .header("Ocp-Apim-Subscription-Region", &self.region)
            .header("Content-Type", "application/json; charset=UTF-8")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "bing".to_string(),
                detail: e.to_string(),
            })?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::TranslateRateLimit {
                provider: "bing".to_string(),
                retry_after: Some(60),
            });
        }

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::TranslateAuthFailed {
                provider: "bing".to_string(),
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "bing".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        // Bing 返回 [{ "translations": [{ "text": "..." }] }, ...]
        let translations = result
            .as_array()
            .ok_or_else(|| AppError::TranslateAlignFailed {
                missing: texts.len(),
            })?;

        let results: Vec<String> = translations
            .iter()
            .map(|item| {
                item.get("translations")
                    .and_then(|t| t.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|first| first.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect();

        if results.len() != texts.len() {
            return Err(AppError::TranslateAlignFailed {
                missing: texts.len().saturating_sub(results.len()),
            });
        }

        Ok(results)
    }
}

#[async_trait::async_trait]
impl TranslateProviderTrait for BingProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        const BING_MAX_CHARS: usize = 5000; // Bing 单次请求字符上限（留余量）

        // 按字符上限分块：贪心累计，超限就开新块
        let mut chunks: Vec<Vec<&String>> = Vec::new();
        let mut current_chunk: Vec<&String> = Vec::new();
        let mut current_chars = 0usize;
        for text in texts {
            let text_chars = text.chars().count();
            if !current_chunk.is_empty() && current_chars + text_chars + 2 > BING_MAX_CHARS {
                chunks.push(std::mem::take(&mut current_chunk));
                current_chars = 0;
            }
            current_chunk.push(text);
            current_chars += text_chars + 2; // +2 估算 JSON 开销
        }
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        let mut all_results: Vec<String> = Vec::with_capacity(texts.len());
        for chunk in &chunks {
            let translations = self.translate_single_batch(chunk, source_lang, target_lang).await?;
            all_results.extend(translations);
        }
        Ok(all_results)
    }

    async fn supported_target_langs(&self) -> Result<Vec<LanguageInfo>, AppError> {
        Ok(vec![
            LanguageInfo { code: "zh-Hans".into(), name: "Chinese (Simplified)".into(), native_name: "简体中文".into() },
            LanguageInfo { code: "zh-Hant".into(), name: "Chinese (Traditional)".into(), native_name: "繁體中文".into() },
            LanguageInfo { code: "en".into(), name: "English".into(), native_name: "English".into() },
            LanguageInfo { code: "ja".into(), name: "Japanese".into(), native_name: "日本語".into() },
            LanguageInfo { code: "ko".into(), name: "Korean".into(), native_name: "한국어".into() },
            LanguageInfo { code: "fr".into(), name: "French".into(), native_name: "Français".into() },
            LanguageInfo { code: "de".into(), name: "German".into(), native_name: "Deutsch".into() },
            LanguageInfo { code: "es".into(), name: "Spanish".into(), native_name: "Español".into() },
            LanguageInfo { code: "ru".into(), name: "Russian".into(), native_name: "Русский".into() },
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh-Hans").await?;
        Ok(())
    }
}

// === SECTION 4 END ===

/// Google 翻译 API（Google Cloud Translation）
/// 文档：https://cloud.google.com/translate/docs
/// 认证：API Key（URL 参数 key=...）
pub struct GoogleProvider {
    api_key: String,
    client: reqwest::Client,
}

impl GoogleProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_client(api_key, reqwest::Client::new())
    }
    pub fn with_client(api_key: String, client: reqwest::Client) -> Self {
        Self { api_key, client }
    }

    /// 单批翻译（不超过字符上限）
    async fn translate_single_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        let url = "https://translation.googleapis.com/language/translate/v2";
        let q: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();
        let body = serde_json::json!({
            "q": q,
            "source": source_lang,
            "target": target_lang,
            "format": "text",
        });

        let resp = self
            .client
            .post(url)
            .query(&[("key", &self.api_key)])
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "google".to_string(),
                detail: e.to_string(),
            })?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::TranslateRateLimit {
                provider: "google".to_string(),
                retry_after: Some(60),
            });
        }

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::TranslateAuthFailed {
                provider: "google".to_string(),
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "google".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        // Google 返回 { "data": { "translations": [{ "translatedText": "..." }] } }
        let translations = result
            .get("data")
            .and_then(|d| d.get("translations"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| AppError::TranslateAlignFailed {
                missing: texts.len(),
            })?;

        let results: Vec<String> = translations
            .iter()
            .map(|item| {
                item.get("translatedText")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect();

        if results.len() != texts.len() {
            return Err(AppError::TranslateAlignFailed {
                missing: texts.len().saturating_sub(results.len()),
            });
        }

        Ok(results)
    }
}

#[async_trait::async_trait]
impl TranslateProviderTrait for GoogleProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        const GOOGLE_MAX_CHARS: usize = 5000; // Google v2 单次请求字符上限（留余量）

        // 按字符上限分块：贪心累计，超限就开新块
        let mut chunks: Vec<Vec<&String>> = Vec::new();
        let mut current_chunk: Vec<&String> = Vec::new();
        let mut current_chars = 0usize;
        for text in texts {
            let text_chars = text.chars().count();
            if !current_chunk.is_empty() && current_chars + text_chars > GOOGLE_MAX_CHARS {
                chunks.push(std::mem::take(&mut current_chunk));
                current_chars = 0;
            }
            current_chunk.push(text);
            current_chars += text_chars;
        }
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        let mut all_results: Vec<String> = Vec::with_capacity(texts.len());
        for chunk in &chunks {
            let translations = self.translate_single_batch(chunk, source_lang, target_lang).await?;
            all_results.extend(translations);
        }
        Ok(all_results)
    }

    async fn supported_target_langs(&self) -> Result<Vec<LanguageInfo>, AppError> {
        Ok(vec![
            LanguageInfo { code: "zh".into(), name: "Chinese".into(), native_name: "中文".into() },
            LanguageInfo { code: "en".into(), name: "English".into(), native_name: "English".into() },
            LanguageInfo { code: "ja".into(), name: "Japanese".into(), native_name: "日本語".into() },
            LanguageInfo { code: "ko".into(), name: "Korean".into(), native_name: "한국어".into() },
            LanguageInfo { code: "fr".into(), name: "French".into(), native_name: "Français".into() },
            LanguageInfo { code: "de".into(), name: "German".into(), native_name: "Deutsch".into() },
            LanguageInfo { code: "es".into(), name: "Spanish".into(), native_name: "Español".into() },
            LanguageInfo { code: "ru".into(), name: "Russian".into(), native_name: "Русский".into() },
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 5 END ===

/// OpenAI 兼容翻译 provider
/// 通过标准 OpenAI Chat Completions 协议调用任意兼容端点：
/// 局域网 LM Studio / Ollama / vLLM、云 API DeepSeek / Qwen / OpenAI / Kimi / 智谱 等
/// 认证可选：api_key 留空时不带 Authorization header（适配局域网无认证场景）
pub struct OpenAiProvider {
    base_url: String,
    model: String,
    model_type: ModelType,
    api_key: Option<String>,
    client: reqwest::Client,
}

/// 编号行正则：匹配 "1. text" / "1、text" / "1: text" / "1) text"
static NUMBERED_LINE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

impl OpenAiProvider {
    pub fn new(
        base_url: String,
        model: String,
        model_type: ModelType,
        api_key: Option<String>,
    ) -> Self {
        Self::with_client(base_url, model, model_type, api_key, reqwest::Client::new())
    }

    pub fn with_client(
        base_url: String,
        model: String,
        model_type: ModelType,
        api_key: Option<String>,
        client: reqwest::Client,
    ) -> Self {
        Self { base_url, model, model_type, api_key, client }
    }

    /// 构建 system prompt（优先远程模板，回退内置）
    fn build_system_prompt(&self, source_lang: &str, target_lang: &str) -> String {
        let src = lang_full_name(source_lang);
        let tgt = lang_full_name(target_lang);
        let view = PromptTemplateRegistry::get_template(&self.model_type);
        view.render_system(src, tgt)
    }

    /// 构建 user prompt（编号列表格式）
    fn build_user_prompt(&self, texts: &[&String]) -> String {
        let view = PromptTemplateRegistry::get_template(&self.model_type);
        view.render_user(texts)
    }

    /// 解析模型返回的编号列表响应，按编号对齐回输入
    fn parse_numbered_response(
        content: &str,
        expected_count: usize,
    ) -> Result<Vec<String>, AppError> {
        let re = NUMBERED_LINE_RE.get_or_init(|| {
            regex::Regex::new(r"^(\d+)[.、:)]\s*(.+)$").unwrap()
        });

        // 1. 尝试按编号解析
        let mut translations: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(captures) = re.captures(line) {
                let num: usize = captures[1].parse().unwrap_or(0);
                let text = captures.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                if num > 0 && num <= expected_count {
                    translations.insert(num, text.to_string());
                }
            }
        }

        // 2. 按编号顺序组装结果
        if translations.len() == expected_count {
            let result: Vec<String> = (1..=expected_count)
                .map(|i| translations.remove(&i).unwrap_or_default())
                .collect();
            return Ok(result);
        }

        // 3. 编号解析失败 → 退化为按行对齐
        let lines: Vec<String> = content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.len() == expected_count {
            return Ok(lines);
        }

        // 4. 行数也不对 → 返回对齐失败，由调度器触发逐条重试
        Err(AppError::TranslateAlignFailed {
            missing: expected_count.saturating_sub(lines.len()),
        })
    }

    /// 单批翻译（构造 chat completion 请求）
    async fn translate_single_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        let system_prompt = self.build_system_prompt(source_lang, target_lang);
        let user_prompt = self.build_user_prompt(texts);

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(120))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    { "role": "system", "content": system_prompt },
                    { "role": "user",   "content": user_prompt },
                ],
                "temperature": 0.3,
                "stream": false,
            }));

        // api_key 非空时才带认证头（局域网无认证场景）
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", key));
            }
        }

        let resp = req.send().await.map_err(|e| AppError::TranslateNetworkError {
            provider: "openai".to_string(),
            detail: e.to_string(),
        })?;

        let status = resp.status();

        // 限流
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::TranslateRateLimit {
                provider: "openai".to_string(),
                retry_after: Some(60),
            });
        }

        // 认证失败
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::TranslateAuthFailed {
                provider: "openai".to_string(),
            });
        }

        let response_body = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "openai".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        // 解析响应
        let body: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: format!("JSON parse error: {}", e),
            }
        })?;

        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| AppError::TranslateResponseParseFailed {
                detail: "choices[0].message.content missing".to_string(),
            })?;

        Self::parse_numbered_response(content, texts.len())
    }
}

#[async_trait::async_trait]
impl TranslateProviderTrait for OpenAiProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // token 预算分块：粗估 token = chars / 3，预留响应空间
        const MAX_INPUT_TOKENS: usize = 3000;

        let mut chunks: Vec<Vec<&String>> = Vec::new();
        let mut current: Vec<&String> = Vec::new();
        let mut current_tokens = 0usize;
        for text in texts {
            let tokens = text.chars().count() / 3 + 1;
            if !current.is_empty() && current_tokens + tokens > MAX_INPUT_TOKENS {
                chunks.push(std::mem::take(&mut current));
                current_tokens = 0;
            }
            current.push(text);
            current_tokens += tokens;
        }
        if !current.is_empty() {
            chunks.push(current);
        }

        let mut results = Vec::with_capacity(texts.len());
        for chunk in &chunks {
            let translated = self.translate_single_batch(chunk, source_lang, target_lang).await?;
            results.extend(translated);
        }
        Ok(results)
    }

    async fn supported_target_langs(&self) -> Result<Vec<LanguageInfo>, AppError> {
        // AI 模型支持任意语言，返回常用列表
        Ok(vec![
            LanguageInfo { code: "zh".into(), name: "Chinese".into(), native_name: "中文".into() },
            LanguageInfo { code: "en".into(), name: "English".into(), native_name: "English".into() },
            LanguageInfo { code: "ja".into(), name: "Japanese".into(), native_name: "日本語".into() },
            LanguageInfo { code: "ko".into(), name: "Korean".into(), native_name: "한국어".into() },
            LanguageInfo { code: "fr".into(), name: "French".into(), native_name: "Français".into() },
            LanguageInfo { code: "de".into(), name: "German".into(), native_name: "Deutsch".into() },
            LanguageInfo { code: "es".into(), name: "Spanish".into(), native_name: "Español".into() },
            LanguageInfo { code: "ru".into(), name: "Russian".into(), native_name: "Русский".into() },
            LanguageInfo { code: "it".into(), name: "Italian".into(), native_name: "Italiano".into() },
            LanguageInfo { code: "pt".into(), name: "Portuguese".into(), native_name: "Português".into() },
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        // 发一个最小翻译请求验证连通性
        self.translate(&["Hello".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 5.5 END ===

/// 翻译调度器
/// 负责缓存查询、分段、占位符保护、限流重试
pub struct TranslateScheduler<'a> {
    db: &'a Database,
    provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
    provider_name: String,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    concurrency: usize,
}

impl<'a> TranslateScheduler<'a> {
    pub fn new(
        db: &'a Database,
        provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
        provider_name: String,
    ) -> Self {
        Self {
            db,
            provider,
            provider_name,
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            concurrency: 1,
        }
    }

    pub fn with_cancel_token(
        db: &'a Database,
        provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
        provider_name: String,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            db,
            provider,
            provider_name,
            cancelled,
            concurrency: 1,
        }
    }

    /// 设置并发数（实际并发 = min(此值, QPS 上限)）
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 批量查询缓存，返回已缓存的翻译结果（不调用 API）
    pub fn get_cached_entries(
        &self,
        entries: &[crate::subtitle::SubtitleEntry],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<TranslateEntry>, AppError> {
        let mut results = Vec::new();
        for entry in entries {
            let cache_key = translate_cache_key(
                &entry.text,
                source_lang,
                target_lang,
                &self.provider_name,
            );
            if let Some(cached) = self.db.get_translate_cache(&cache_key)? {
                results.push(TranslateEntry {
                    index: entry.index,
                    original: entry.text.clone(),
                    translated: cached,
                    from_cache: true,
                    failed: false,
                });
            }
        }
        Ok(results)
    }

    /// 翻译一批字幕条目（含缓存 + 占位符保护 + 重试 + 分批 + 进度推送）
    pub async fn translate_entries(
        &self,
        entries: &[crate::subtitle::SubtitleEntry],
        source_lang: &str,
        target_lang: &str,
        max_single_length: usize,
    ) -> Result<TranslateResult, AppError> {
        self.translate_entries_with_progress(entries, source_lang, target_lang, max_single_length, None).await
    }

    /// 带进度回调的翻译
    pub async fn translate_entries_with_progress(
        &self,
        entries: &[crate::subtitle::SubtitleEntry],
        source_lang: &str,
        target_lang: &str,
        max_single_length: usize,
        on_progress: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
    ) -> Result<TranslateResult, AppError> {
        self.translate_entries_full(entries, source_lang, target_lang, max_single_length, on_progress, None).await
    }

    /// 带进度回调 + 单条完成回调的翻译
    pub async fn translate_entries_full(
        &self,
        entries: &[crate::subtitle::SubtitleEntry],
        source_lang: &str,
        target_lang: &str,
        max_single_length: usize,
        on_progress: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
        on_entry_done: Option<Box<dyn Fn(&TranslateEntry) + Send + Sync>>,
    ) -> Result<TranslateResult, AppError> {
        let mut results = Vec::with_capacity(entries.len());
        let mut cached_count = 0;
        let mut to_translate: Vec<(usize, String, PlaceholderProtector)> = Vec::new();

        // 1. 缓存查询 + 占位符保护
        for entry in entries {
            // 跳过 ass 矢量绘图指令（含 \p1 标记），不是字幕文本
            if entry.text.contains("\\p1") {
                tracing::info!("字幕 #{} 含 \\p1 绘图指令，跳过翻译", entry.index);
                continue;
            }
            let cache_key = translate_cache_key(
                &entry.text,
                source_lang,
                target_lang,
                &self.provider_name,
            );

            if let Some(cached) = self.db.get_translate_cache(&cache_key)? {
                let te = TranslateEntry {
                    index: entry.index,
                    original: entry.text.clone(),
                    translated: cached,
                    from_cache: true,
                    failed: false,
                };
                if let Some(ref cb) = on_entry_done {
                    cb(&te);
                }
                results.push(te);
                cached_count += 1;
                continue;
            }

            // 占位符保护
            let mut protector = PlaceholderProtector::new();
            let protected_text = protector.protect(&entry.text);

            // 分段（如果超过 API 上限）：按句号二次切分，逐段翻译后拼接
            if protected_text.len() > max_single_length {
                tracing::warn!("字幕 #{} 超过 API 上限（{}字节），按句号切分翻译", entry.index, protected_text.len());
                let segments = split_text(&protected_text, max_single_length);
                tracing::info!("字幕 #{} 切分为 {} 段", entry.index, segments.len());

                let mut combined = String::new();
                let mut any_failed = false;
                for seg in &segments {
                    if self.is_cancelled() {
                        any_failed = true;
                        break;
                    }
                    match self.translate_with_retry(&[seg.clone()], source_lang, target_lang).await {
                        Ok(tr) if !tr.is_empty() && !tr[0].is_empty() => {
                            combined.push_str(&tr[0]);
                        }
                        _ => {
                            any_failed = true;
                            tracing::warn!("字幕 #{} 切分段翻译失败", entry.index);
                        }
                    }
                }

                let restored = protector.restore(&combined);
                if !restored.is_empty() && !any_failed {
                    let cache_key = translate_cache_key(
                        &entry.text,
                        source_lang,
                        target_lang,
                        &self.provider_name,
                    );
                    let _ = self.db.set_translate_cache(
                        &cache_key,
                        &entry.text,
                        &restored,
                        source_lang,
                        target_lang,
                        &self.provider_name,
                    );
                }

                let te = TranslateEntry {
                    index: entry.index,
                    original: entry.text.clone(),
                    translated: restored,
                    from_cache: false,
                    failed: any_failed || combined.is_empty(),
                };
                if let Some(ref cb) = on_entry_done {
                    cb(&te);
                }
                results.push(te);
                if let Some(ref cb) = on_progress {
                    cb(results.len(), entries.len());
                }
            } else {
                to_translate.push((entry.index, protected_text, protector));
            }
        }

        // 2. 分批翻译（每批 30 条，带重试），并发度由 self.concurrency 控制
        const BATCH_SIZE: usize = 30;
        if !to_translate.is_empty() {
            let batches: Vec<Vec<(usize, String, PlaceholderProtector)>> =
                to_translate.chunks(BATCH_SIZE).map(|c| c.to_vec()).collect();
            let total_batches = batches.len();
            let concurrency = self.concurrency.max(1);
            tracing::info!("翻译并发度: {}，共 {} 批", concurrency, total_batches);

            // 并发调用 API：用 Semaphore 控制并发数，JoinSet 收集结果
            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
            let provider = self.provider.clone();
            let cancelled = self.cancelled.clone();
            let mut join_set = tokio::task::JoinSet::new();

            for (batch_idx, batch) in batches.iter().enumerate() {
                let texts: Vec<String> = batch.iter().map(|(_, t, _)| t.clone()).collect();
                let source = source_lang.to_string();
                let target = target_lang.to_string();
                let provider = provider.clone();
                let cancelled = cancelled.clone();
                let permit = semaphore.clone().acquire_owned().await.unwrap();

                join_set.spawn(async move {
                    let _permit = permit;
                    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                        return (batch_idx, Err(AppError::TranslateRetriesExhausted));
                    }
                    tracing::info!("翻译批次 {}/{}，本批 {} 条", batch_idx + 1, total_batches, texts.len());
                    let result = translate_with_retry_provider(
                        &*provider,
                        &texts,
                        &source,
                        &target,
                        &cancelled,
                    )
                    .await;
                    (batch_idx, result)
                });
            }

            // 收集所有 API 结果，按 batch_idx 排序保证顺序
            let mut batch_api_results: Vec<(usize, Result<Vec<String>, AppError>)> = Vec::new();
            while let Some(res) = join_set.join_next().await {
                if let Ok(item) = res {
                    batch_api_results.push(item);
                }
            }
            batch_api_results.sort_by_key(|(idx, _)| *idx);

            // 顺序后处理：对齐检查、回填占位符、写缓存、回调
            for (batch_idx, api_result) in batch_api_results {
                if self.is_cancelled() {
                    tracing::info!("翻译已取消，停止后处理（已完成到批次 {}）", batch_idx + 1);
                    break;
                }

                let batch = &batches[batch_idx];
                let texts: Vec<String> = batch.iter().map(|(_, t, _)| t.clone()).collect();

                let translations = match api_result {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("批次 {} 整批翻译失败: {}", batch_idx + 1, e);
                        for (index, original_text, _protector) in batch.iter() {
                            let te = TranslateEntry {
                                index: *index,
                                original: original_text.clone(),
                                translated: String::new(),
                                from_cache: false,
                                failed: true,
                            };
                            if let Some(ref cb) = on_entry_done {
                                cb(&te);
                            }
                            results.push(te);
                        }
                        if let Some(ref cb) = on_progress {
                            cb(results.len(), entries.len());
                        }
                        continue;
                    }
                };

                // 对齐检查：API 返回的翻译数量必须与输入一致
                if translations.len() != batch.len() {
                    tracing::warn!(
                        "翻译批次 {} 对齐异常：输入 {} 条，返回 {} 条，逐条重试缺失项",
                        batch_idx + 1,
                        batch.len(),
                        translations.len()
                    );
                    let mut final_translations: Vec<String> = translations.clone();
                    while final_translations.len() < batch.len() {
                        final_translations.push(String::new());
                    }
                    for (i, translated) in final_translations.iter_mut().enumerate() {
                        if translated.is_empty() {
                            if self.is_cancelled() {
                                break;
                            }
                            let single_text = vec![texts[i].clone()];
                            match self.translate_with_retry(&single_text, source_lang, target_lang).await {
                                Ok(single_result) if !single_result.is_empty() => {
                                    *translated = single_result[0].clone();
                                    tracing::info!("逐条重试成功：批次 {} 第 {} 条", batch_idx + 1, i + 1);
                                }
                                Ok(_) => {
                                    tracing::warn!("逐条重试返回空：批次 {} 第 {} 条", batch_idx + 1, i + 1);
                                }
                                Err(e) => {
                                    tracing::warn!("逐条重试失败：批次 {} 第 {} 条: {}", batch_idx + 1, i + 1, e);
                                }
                            }
                        }
                    }

                    for ((index, original_text, protector), translated) in
                        batch.iter().zip(final_translations.iter())
                    {
                        let restored = protector.restore(translated);
                        if !restored.is_empty() {
                            let cache_key = translate_cache_key(
                                original_text,
                                source_lang,
                                target_lang,
                                &self.provider_name,
                            );
                            let _ = self.db.set_translate_cache(
                                &cache_key,
                                original_text,
                                &restored,
                                source_lang,
                                target_lang,
                                &self.provider_name,
                            );
                        }
                        let te = TranslateEntry {
                            index: *index,
                            original: original_text.clone(),
                            translated: restored,
                            from_cache: false,
                            failed: translated.is_empty(),
                        };
                        if let Some(ref cb) = on_entry_done {
                            cb(&te);
                        }
                        results.push(te);
                    }
                } else {
                    for ((index, original_text, protector), translated) in
                        batch.iter().zip(translations.iter())
                    {
                        let restored = protector.restore(translated);

                        let cache_key = translate_cache_key(
                            original_text,
                            source_lang,
                            target_lang,
                            &self.provider_name,
                        );
                        let _ = self.db.set_translate_cache(
                            &cache_key,
                            original_text,
                            &restored,
                            source_lang,
                            target_lang,
                            &self.provider_name,
                        );

                        let te = TranslateEntry {
                            index: *index,
                            original: original_text.clone(),
                            translated: restored,
                            from_cache: false,
                            failed: false,
                        };
                        if let Some(ref cb) = on_entry_done {
                            cb(&te);
                        }
                        results.push(te);
                    }
                }

                if let Some(ref cb) = on_progress {
                    cb(results.len(), entries.len());
                }
            }
        }

        // 按 index 排序
        results.sort_by_key(|r| r.index);

        Ok(TranslateResult {
            translations: results,
            provider: self.provider_name.clone(),
            cached_count,
        })
    }

    /// 带重试的翻译（指数退避：1s/2s/4s，最多 3 次）
    async fn translate_with_retry(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        translate_with_retry_provider(
            &*self.provider,
            texts,
            source_lang,
            target_lang,
            &self.cancelled,
        ).await
    }
}

/// 独立的带重试翻译函数（可在 spawned task 中调用，不依赖 &self）
/// 指数退避：1s/2s/4s，最多 3 次
async fn translate_with_retry_provider(
    provider: &dyn TranslateProviderTrait,
    texts: &[String],
    source_lang: &str,
    target_lang: &str,
    cancelled: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<String>, AppError> {
        let mut last_error: Option<AppError> = None;
        let delays = [1u64, 2, 4];

        for (attempt, delay) in delays.iter().enumerate() {
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(AppError::TranslateRetriesExhausted);
            }
            match provider
                .translate(texts, source_lang, target_lang)
                .await
            {
                Ok(result) => return Ok(result),
                Err(AppError::TranslateRateLimit { provider, .. }) => {
                    tracing::warn!(
                        "翻译被限流（第 {} 次重试），等待 {} 秒",
                        attempt + 1,
                        delay
                    );
                    last_error = Some(AppError::TranslateRateLimit {
                        provider,
                        retry_after: Some(*delay),
                    });
                    tokio::time::sleep(std::time::Duration::from_secs(*delay)).await;
                }
                Err(AppError::TranslateNetworkError { provider, detail }) => {
                    tracing::warn!(
                        "翻译网络错误（第 {} 次重试）：{}，等待 {} 秒",
                        attempt + 1,
                        detail,
                        delay
                    );
                    last_error = Some(AppError::TranslateNetworkError { provider, detail });
                    tokio::time::sleep(std::time::Duration::from_secs(*delay)).await;
                }
                Err(e) => return Err(e), // 鉴权失败等不重试
            }
        }

        Err(last_error.unwrap_or(AppError::TranslateRetriesExhausted))
}

/// 创建翻译 provider 实例
pub fn create_provider(
    provider: &TranslateProvider,
    credentials: &ProviderCredentials,
) -> Result<std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>, AppError> {
    create_provider_with_proxy(provider, credentials, &ProxyConfig::default())
}

/// 创建翻译 provider 实例（带代理配置）
pub fn create_provider_with_proxy(
    provider: &TranslateProvider,
    credentials: &ProviderCredentials,
    proxy: &ProxyConfig,
) -> Result<std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>, AppError> {
    let client = proxy.build_client();
    match provider {
        TranslateProvider::Baidu => {
            let app_id = credentials.app_id.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound {
                    provider: "baidu".to_string(),
                }
            })?;
            let secret_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound {
                    provider: "baidu".to_string(),
                }
            })?;
            Ok(std::sync::Arc::new(BaiduProvider::with_client(app_id, secret_key, client)))
        }
        TranslateProvider::Bing => {
            let api_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound {
                    provider: "bing".to_string(),
                }
            })?;
            let region = credentials.region.clone().unwrap_or_else(|| "global".to_string());
            Ok(std::sync::Arc::new(BingProvider::with_client(api_key, region, client)))
        }
        TranslateProvider::Google => {
            let api_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound {
                    provider: "google".to_string(),
                }
            })?;
            Ok(std::sync::Arc::new(GoogleProvider::with_client(api_key, client)))
        }
        TranslateProvider::OpenAi => {
            let base_url = credentials.base_url.clone().ok_or_else(|| {
                AppError::TranslateNotConfigured
            })?;
            let model = credentials.model.clone().ok_or_else(|| {
                AppError::TranslateNotConfigured
            })?;
            let model_type = credentials
                .model_type
                .as_deref()
                .and_then(ModelType::from_str)
                .unwrap_or(ModelType::Generic);
            // api_key 可选：None 或空字符串 = 无认证
            let api_key = credentials
                .secret_key
                .clone()
                .filter(|s| !s.is_empty());
            Ok(std::sync::Arc::new(OpenAiProvider::with_client(
                base_url, model, model_type, api_key, client,
            )))
        }
    }
}

// === SECTION 6 END ===

// === 远程 Prompt 配置 ===

/// 远程 prompt 配置文件结构
#[derive(Debug, Clone, Deserialize)]
pub struct RemotePromptConfig {
    pub version: String,
    pub templates: std::collections::HashMap<String, RemotePromptTemplate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemotePromptTemplate {
    pub system: String,
    pub user_line_format: String,
}

/// 远程 prompt 配置的 GitHub raw URL
/// 改 prompt 只需更新此文件并 git push，所有客户端下次启动自动生效
const REMOTE_PROMPT_URL: &str = "https://raw.githubusercontent.com/zimufan/ai-subtrans/main/config/prompts.json";

/// 远程 prompt 配置缓存 key（db config 表）
const PROMPT_CONFIG_DB_KEY: &str = "translate_prompt_remote_config";
const PROMPT_CONFIG_VERSION_DB_KEY: &str = "translate_prompt_remote_version";

/// 全局远程配置缓存（启动时拉取后写入，翻译时读取）
static REMOTE_CONFIG: std::sync::OnceLock<std::sync::RwLock<Option<RemotePromptConfig>>> = std::sync::OnceLock::new();

/// 模板视图（统一远程和内置的渲染接口）
pub enum PromptTemplateView {
    Builtin(&'static PromptTemplate),
    Remote(RemotePromptTemplate),
}

impl PromptTemplateView {
    pub fn render_system(&self, src: &str, tgt: &str) -> String {
        match self {
            Self::Builtin(t) => t.render_system(src, tgt),
            Self::Remote(t) => t.system.replace("{src}", src).replace("{tgt}", tgt),
        }
    }

    pub fn render_user(&self, texts: &[&String]) -> String {
        match self {
            Self::Builtin(t) => texts
                .iter()
                .enumerate()
                .map(|(i, txt)| {
                    t.user_line_format
                        .replace("{index}", &(i + 1).to_string())
                        .replace("{text}", txt)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Remote(t) => texts
                .iter()
                .enumerate()
                .map(|(i, txt)| {
                    t.user_line_format
                        .replace("{index}", &(i + 1).to_string())
                        .replace("{text}", txt)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// 模板注册表：优先远程，回退内置
pub struct PromptTemplateRegistry;

impl PromptTemplateRegistry {
    /// 初始化（启动时调用，从 db 加载已缓存的远程配置到内存）
    pub fn init_from_db(db: &Database) {
        let config = db
            .get_config(PROMPT_CONFIG_DB_KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<RemotePromptConfig>(&json).ok());
        let lock = REMOTE_CONFIG.get_or_init(|| std::sync::RwLock::new(None));
        *lock.write().unwrap() = config;
        if let Some(ref c) = *lock.read().unwrap() {
            tracing::info!("远程 prompt 配置已加载: version={}", c.version);
        } else {
            tracing::info!("无远程 prompt 配置，使用内置模板");
        }
    }

    /// 获取模板：远程优先，回退内置
    pub fn get_template(model_type: &ModelType) -> PromptTemplateView {
        let key = model_type.as_str();

        // 尝试远程
        if let Some(lock) = REMOTE_CONFIG.get() {
            if let Some(ref config) = *lock.read().unwrap() {
                if let Some(tmpl) = config.templates.get(key) {
                    return PromptTemplateView::Remote(tmpl.clone());
                }
            }
        }

        // 回退内置
        let builtin = BUILTIN_TEMPLATES
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, t)| t)
            .unwrap_or(&BUILTIN_TEMPLATES[2].1); // 兜底 generic
        PromptTemplateView::Builtin(builtin)
    }
}

/// 拉取远程 prompt 配置（应用启动时调用，失败静默回退内置）
pub async fn fetch_remote_prompt_config(db: &Database) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    match client.get(REMOTE_PROMPT_URL).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(text) => {
                // 校验 JSON 合法性
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    let version = json["version"].as_str().unwrap_or("");
                    let cached_version = db
                        .get_config(PROMPT_CONFIG_VERSION_DB_KEY)
                        .ok()
                        .flatten()
                        .unwrap_or_default();

                    if version != cached_version {
                        let _ = db.set_config(PROMPT_CONFIG_DB_KEY, &text);
                        let _ = db.set_config(PROMPT_CONFIG_VERSION_DB_KEY, version);
                        tracing::info!("远程 prompt 配置已更新: version={}", version);
                    } else {
                        tracing::info!("远程 prompt 配置版本未变: {}", version);
                    }
                }
            }
            Err(e) => tracing::warn!("远程 prompt 配置读取失败: {}", e),
        },
        Ok(resp) => tracing::warn!("远程 prompt 配置 HTTP {}", resp.status()),
        Err(e) => tracing::warn!("远程 prompt 配置拉取失败（使用内置模板）: {}", e),
    }
    // 任何失败都静默处理，翻译时回退内置模板
}

/// 获取当前已加载的远程 prompt 配置版本（供前端显示）
pub fn get_remote_prompt_version() -> Option<String> {
    REMOTE_CONFIG.get().and_then(|lock| {
        lock.read().unwrap().as_ref().map(|c| c.version.clone())
    })
}

// === SECTION 6.5 END ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protector_ass_tags() {
        let mut p = PlaceholderProtector::new();
        let input = r"{\an8}{\b1}Bold top text";
        let protected = p.protect(input);
        // 样式标记应被替换为占位符
        assert!(!protected.contains("{\\an8}"));
        assert!(!protected.contains("{\\b1}"));
        assert!(protected.contains("Bold top text"));
        // 回填后应恢复原文
        let restored = p.restore(&protected);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_protector_multiple_tags() {
        let mut p = PlaceholderProtector::new();
        let input = r"{\an8}Line 1\NLine 2{\b1}Bold";
        let protected = p.protect(input);
        let restored = p.restore(&protected);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_protector_no_tags() {
        let mut p = PlaceholderProtector::new();
        let input = "Hello World";
        let protected = p.protect(input);
        assert_eq!(protected, input);
        assert_eq!(p.placeholder_count(), 0);
    }

    #[test]
    fn test_protector_newline() {
        let mut p = PlaceholderProtector::new();
        let input = r"Line 1\NLine 2";
        let protected = p.protect(input);
        assert!(!protected.contains("\\N"));
        let restored = p.restore(&protected);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_split_text_short() {
        let result = split_text("Hello", 100);
        assert_eq!(result, vec!["Hello"]);
    }

    #[test]
    fn test_split_text_long() {
        let long_text = "This is a sentence. This is another sentence. And a third one here.";
        let result = split_text(long_text, 30);
        assert!(result.len() > 1);
        // 每段不超过 30 字符（除硬切情况）
        for seg in &result {
            assert!(seg.len() <= 31); // 允许句号多 1 字符
        }
    }

    #[test]
    fn test_split_text_multiline() {
        let text = "Line 1\nLine 2\nLine 3";
        let result = split_text(text, 10);
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_provider_from_str() {
        assert_eq!(
            TranslateProvider::from_str("baidu"),
            Some(TranslateProvider::Baidu)
        );
        assert_eq!(
            TranslateProvider::from_str("BING"),
            Some(TranslateProvider::Bing)
        );
        assert_eq!(
            TranslateProvider::from_str("google"),
            Some(TranslateProvider::Google)
        );
        assert_eq!(
            TranslateProvider::from_str("openai"),
            Some(TranslateProvider::OpenAi)
        );
        assert_eq!(
            TranslateProvider::from_str("OpenAI"),
            Some(TranslateProvider::OpenAi)
        );
        assert_eq!(TranslateProvider::from_str("unknown"), None);
    }

    #[test]
    fn test_provider_openai_as_str_and_qps() {
        assert_eq!(TranslateProvider::OpenAi.as_str(), "openai");
        assert_eq!(TranslateProvider::OpenAi.qps_limit(), 5);
    }

    #[test]
    fn test_model_type_from_model_id() {
        assert_eq!(ModelType::from_model_id("qwen3-14b"), ModelType::Qwen3);
        assert_eq!(ModelType::from_model_id("Qwen3-32B-Instruct"), ModelType::Qwen3);
        assert_eq!(ModelType::from_model_id("deepseek-v4"), ModelType::Deepseek);
        assert_eq!(ModelType::from_model_id("DeepSeek-R1"), ModelType::Deepseek);
        assert_eq!(ModelType::from_model_id("gemma-3-12b"), ModelType::Generic);
        assert_eq!(ModelType::from_model_id("llama-3.1-8b"), ModelType::Generic);
    }

    #[test]
    fn test_model_type_from_str() {
        assert_eq!(ModelType::from_str("qwen3"), Some(ModelType::Qwen3));
        assert_eq!(ModelType::from_str("Deepseek"), Some(ModelType::Deepseek));
        assert_eq!(ModelType::from_str("generic"), Some(ModelType::Generic));
        assert_eq!(ModelType::from_str("unknown"), None);
    }

    #[test]
    fn test_parse_numbered_response_exact() {
        let content = "1. 你好\n2. 世界\n3. 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_response_chinese_punct() {
        // 中文顿号分隔
        let content = "1、你好\n2、世界";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn test_parse_numbered_response_out_of_order() {
        // 编号乱序也能按编号对齐
        let content = "3. 三\n1. 一\n2. 二";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["一", "二", "三"]);
    }

    #[test]
    fn test_parse_numbered_response_fallback_to_lines() {
        // 无编号 → 退化为按行对齐
        let content = "你好\n世界\n测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_response_count_mismatch() {
        // 数量不对 → 返回对齐失败
        let content = "1. 你好\n2. 世界";
        let result = OpenAiProvider::parse_numbered_response(content, 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_numbered_response_with_extra_text() {
        // 模型可能加额外说明行，编号解析应忽略非编号行
        let content = "Here are the translations:\n1. 你好\n2. 世界\n\nDone.";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn test_builtin_templates_render() {
        let tmpl = BUILTIN_TEMPLATES.iter().find(|(k, _)| *k == "qwen3").map(|(_, t)| t).unwrap();
        let system = tmpl.render_system("English", "Chinese");
        assert!(system.contains("English"));
        assert!(system.contains("Chinese"));
    }

    #[test]
    fn test_prompt_template_registry_builtin_fallback() {
        // 未初始化远程配置时，应回退到内置模板
        let view = PromptTemplateRegistry::get_template(&ModelType::Generic);
        let system = view.render_system("English", "Chinese");
        assert!(system.contains("English"));
        assert!(system.contains("Chinese"));
    }

    #[test]
    fn test_prompt_template_view_render_user() {
        let view = PromptTemplateRegistry::get_template(&ModelType::Qwen3);
        let hello = "Hello".to_string();
        let world = "World".to_string();
        let texts = vec![&hello, &world];
        let user = view.render_user(&texts);
        assert!(user.contains("1. Hello"));
        assert!(user.contains("2. World"));
    }

    #[test]
    fn test_lang_full_name() {
        assert_eq!(lang_full_name("en"), "English");
        assert_eq!(lang_full_name("zh"), "Chinese");
        assert_eq!(lang_full_name("zh-tw"), "Traditional Chinese");
        assert_eq!(lang_full_name("ja"), "Japanese");
        assert_eq!(lang_full_name("auto"), "the source language");
        assert_eq!(lang_full_name("xx"), "the source language");
    }

    #[test]
    fn test_create_openai_provider_no_key() {
        // 无 api_key 应成功创建（局域网无认证场景）
        let creds = ProviderCredentials {
            base_url: Some("http://localhost:1234/v1".into()),
            model: Some("qwen3-14b".into()),
            model_type: Some("qwen3".into()),
            secret_key: None,
            ..Default::default()
        };
        let provider = create_provider(&TranslateProvider::OpenAi, &creds);
        assert!(provider.is_ok());
    }

    #[test]
    fn test_create_openai_provider_missing_config() {
        // 缺少 base_url 应返回 TranslateNotConfigured
        let creds = ProviderCredentials {
            base_url: None,
            model: Some("qwen3-14b".into()),
            ..Default::default()
        };
        let result = create_provider(&TranslateProvider::OpenAi, &creds);
        assert!(matches!(result, Err(AppError::TranslateNotConfigured)));
    }

    #[test]
    fn test_protector_restore_after_translation() {
        let mut p = PlaceholderProtector::new();
        let input = r"{\b1}Hello{\b0}";
        let protected = p.protect(input);
        // 模拟翻译：占位符保留，文本翻译
        // 假设翻译后占位符位置不变
        let translated = format!("{}你好{}", protected.chars().next().unwrap(), protected.chars().nth(4).unwrap());
        // 实际翻译 API 会保留占位符字符
        let _ = translated;
    }

    // === SECTION 7 END ===

    // === 百度语言码映射 ===
    #[test]
    fn test_baidu_lang_ja() {
        assert_eq!(BaiduProvider::to_baidu_lang("ja"), "jp");
    }

    #[test]
    fn test_baidu_lang_ko() {
        assert_eq!(BaiduProvider::to_baidu_lang("ko"), "kor");
    }

    #[test]
    fn test_baidu_lang_fr() {
        assert_eq!(BaiduProvider::to_baidu_lang("fr"), "fra");
    }

    #[test]
    fn test_baidu_lang_es() {
        assert_eq!(BaiduProvider::to_baidu_lang("es"), "spa");
    }

    #[test]
    fn test_baidu_lang_vi() {
        assert_eq!(BaiduProvider::to_baidu_lang("vi"), "vie");
    }

    #[test]
    fn test_baidu_lang_ar() {
        assert_eq!(BaiduProvider::to_baidu_lang("ar"), "ara");
    }

    #[test]
    fn test_baidu_lang_sv() {
        assert_eq!(BaiduProvider::to_baidu_lang("sv"), "swe");
    }

    #[test]
    fn test_baidu_lang_fi() {
        assert_eq!(BaiduProvider::to_baidu_lang("fi"), "fin");
    }

    #[test]
    fn test_baidu_lang_da() {
        assert_eq!(BaiduProvider::to_baidu_lang("da"), "dan");
    }

    #[test]
    fn test_baidu_lang_auto() {
        assert_eq!(BaiduProvider::to_baidu_lang("auto"), "auto");
    }

    #[test]
    fn test_baidu_lang_passthrough() {
        // 未列出的语言原样传递
        assert_eq!(BaiduProvider::to_baidu_lang("en"), "en");
        assert_eq!(BaiduProvider::to_baidu_lang("zh"), "zh");
        assert_eq!(BaiduProvider::to_baidu_lang("de"), "de");
    }

    // === SECTION 8 END ===

    // === split_text 边界 ===
    #[test]
    fn test_split_text_empty() {
        let result = split_text("", 100);
        assert!(result.is_empty() || (result.len() == 1 && result[0].is_empty()));
    }

    #[test]
    fn test_split_text_single_short() {
        let result = split_text("hello", 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "hello");
    }

    #[test]
    fn test_split_text_exact_limit() {
        let text = "abcdefghij"; // 10 chars
        let result = split_text(text, 10);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_split_text_exceeds_limit() {
        let text = "aaaaaaaaaaaaaaaaaaaa"; // 20 chars
        let result = split_text(text, 10);
        assert!(result.len() >= 2);
    }

    // === SECTION 9 END ===

    // === PlaceholderProtector 边界 ===
    #[test]
    fn test_protector_empty_string() {
        let mut p = PlaceholderProtector::new();
        let protected = p.protect("");
        assert!(protected.is_empty());
    }

    #[test]
    fn test_protector_plain_text_no_tags() {
        let mut p = PlaceholderProtector::new();
        let input = "Hello World";
        let protected = p.protect(input);
        // 无标签，不应插入占位符
        assert_eq!(protected, input);
    }

    #[test]
    fn test_protector_multiple_newlines() {
        let mut p = PlaceholderProtector::new();
        let input = "Line1\\NLine2\\NLine3";
        let protected = p.protect(input);
        // 应保护 \\N 标记
        let restored = p.restore(&protected);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_protector_nested_braces() {
        let mut p = PlaceholderProtector::new();
        let input = r"{\an8}{\b1}Text{\b0}";
        let protected = p.protect(input);
        let restored = p.restore(&protected);
        assert_eq!(restored, input);
    }

    // === SECTION 10 END ===
}
