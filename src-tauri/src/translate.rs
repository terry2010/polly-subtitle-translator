// 翻译模块
// provider 抽象（百度/Bing/Google）+ 分段 + 占位符保护 + 缓存 + 限流重试

use crate::db::{translate_cache_key, Database};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

/// 检查字符串是否包含 CJK 字符（中日韩统一表意文字）
fn has_cjk(s: &str) -> bool {
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

/// 剥离 markdown 代码块包裹（```json ... ``` 或 ``` ... ```）
/// 如果内容被代码块包裹，返回代码块内部内容；否则原样返回
fn strip_markdown_code_fence(s: &str) -> String {
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
    if result.ends_with("```") {
        result[..result.len() - 3].trim().to_string()
    } else {
        result.to_string()
    }
}

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
            password: db.get_credential("proxy:pass").ok().flatten(),
        }
    }

    /// 构建 reqwest 代理 URL（如 mode != none）
    fn proxy_url(&self) -> Option<String> {
        // 日志只记录非敏感信息，不记录用户名/密码，避免凭据泄露到日志文件
        tracing::info!("ProxyConfig: mode={}, host={}, port={}, has_auth={}", self.mode, self.host, self.port, self.username.is_some());
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
                // 日志只记录 scheme://host:port，不记录凭据
                tracing::info!("搜索使用代理: {}://{}:{}", self.mode, self.host, self.port);
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

/// 限流策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitPolicy {
    /// 每秒最多 N 个请求（QPS），请求间强制间隔 1/N 秒
    Qps(usize),
    /// 最多 N 个并发请求，无间隔要求
    Concurrency(usize),
}

impl RateLimitPolicy {
    /// 并发上限：Qps 模式下为 1（串行+间隔），Concurrency 模式下为 N
    pub fn max_concurrency(&self) -> usize {
        match self {
            RateLimitPolicy::Qps(_) => 1,
            RateLimitPolicy::Concurrency(n) => *n,
        }
    }

    /// 请求发出后的强制等待时间（Qps 模式下为 1/N 秒，Concurrency 模式下为 0）
    pub fn min_interval(&self) -> std::time::Duration {
        match self {
            RateLimitPolicy::Qps(qps) if *qps > 0 => {
                std::time::Duration::from_secs_f64(1.0 / *qps as f64)
            }
            _ => std::time::Duration::ZERO,
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
    DeepL,
    Youdao,
    Caiyun,
    Niutrans,
    Tencent,
    Volcengine,
    Aliyun,
    Amazon,
}

impl TranslateProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TranslateProvider::Baidu => "baidu",
            TranslateProvider::Bing => "bing",
            TranslateProvider::Google => "google",
            TranslateProvider::OpenAi => "openai",
            TranslateProvider::DeepL => "deepl",
            TranslateProvider::Youdao => "youdao",
            TranslateProvider::Caiyun => "caiyun",
            TranslateProvider::Niutrans => "niutrans",
            TranslateProvider::Tencent => "tencent",
            TranslateProvider::Volcengine => "volcengine",
            TranslateProvider::Aliyun => "aliyun",
            TranslateProvider::Amazon => "amazon",
        }
    }

    /// 限流策略：按各 API 官方政策
    pub fn rate_limit_policy(&self) -> RateLimitPolicy {
        match self {
            TranslateProvider::Baidu => RateLimitPolicy::Qps(1),
            TranslateProvider::Youdao => RateLimitPolicy::Qps(1),
            TranslateProvider::OpenAi => RateLimitPolicy::Concurrency(5),
            TranslateProvider::DeepL => RateLimitPolicy::Concurrency(5),
            TranslateProvider::Google => RateLimitPolicy::Concurrency(10),
            TranslateProvider::Bing => RateLimitPolicy::Concurrency(10),
            TranslateProvider::Caiyun => RateLimitPolicy::Qps(5),
            TranslateProvider::Niutrans => RateLimitPolicy::Concurrency(5),
            TranslateProvider::Tencent => RateLimitPolicy::Qps(5),
            TranslateProvider::Volcengine => RateLimitPolicy::Qps(5),
            TranslateProvider::Aliyun => RateLimitPolicy::Qps(50),
            TranslateProvider::Amazon => RateLimitPolicy::Concurrency(10),
        }
    }

    /// 各引擎的 QPS 上限（用于显示和兼容旧逻辑）
    pub fn qps_limit(&self) -> usize {
        match self.rate_limit_policy() {
            RateLimitPolicy::Qps(q) => q,
            RateLimitPolicy::Concurrency(n) => n,
        }
    }

    /// 计算实际并发 = min(用户配置并发, 策略并发上限)，至少 1
    pub fn effective_concurrency(user_config: usize, qps: usize) -> usize {
        user_config.min(qps).max(1)
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "baidu" => Some(TranslateProvider::Baidu),
            "bing" => Some(TranslateProvider::Bing),
            "google" => Some(TranslateProvider::Google),
            "openai" => Some(TranslateProvider::OpenAi),
            "deepl" => Some(TranslateProvider::DeepL),
            "youdao" => Some(TranslateProvider::Youdao),
            "caiyun" => Some(TranslateProvider::Caiyun),
            "niutrans" => Some(TranslateProvider::Niutrans),
            "tencent" => Some(TranslateProvider::Tencent),
            "volcengine" => Some(TranslateProvider::Volcengine),
            "aliyun" => Some(TranslateProvider::Aliyun),
            "amazon" => Some(TranslateProvider::Amazon),
            _ => None,
        }
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
    /// token 用量（仅 AI 翻译有值，传统翻译为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
}

/// token 用量统计（OpenAI 兼容 API 返回的 usage 字段）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
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

/// 检测响应是否为余额不足/额度耗尽
/// 返回 Some(detail) 表示余额不足，None 表示不是
pub fn check_insufficient_balance(status: reqwest::StatusCode, body: &str) -> Option<String> {
    // HTTP 402 Payment Required
    if status == reqwest::StatusCode::PAYMENT_REQUIRED {
        return Some(extract_error_message(body));
    }
    // 响应体关键词检测（各服务商余额不足时的常见关键词）
    let lower = body.to_lowercase();
    let keywords = [
        "insufficient balance",
        "insufficient_balance",
        "insufficient quota",
        "insufficient_quota",
        "insufficientquota",
        "insufficientbalance",
        "quota exhausted",
        "quota_exhausted",
        "quota exceeded",
        "quota_exceeded",
        "out of quota",
        "no credit",
        "no_credit",
        "out of credit",
        "out_of_credit",
        "credit exhausted",
        "credit_exhausted",
        "account suspended",
        "account_suspended",
        "payment required",
        "billing",
        "余额不足",
        "额度耗尽",
        "额度已用尽",
        "额度不足",
        "欠费",
        "账户已停用",
    ];
    for kw in &keywords {
        if lower.contains(kw) {
            return Some(extract_error_message(body));
        }
    }
    None
}

/// 从 JSON 响应体中提取 error.message 字段，失败则返回前 200 字符
fn extract_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| body.chars().take(200).collect())
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

    /// 获取累计的 token 用量（仅 AI 翻译有值，传统翻译默认返回 None）
    fn token_usage(&self) -> Option<TokenUsage> {
        None
    }

    /// 返回每批翻译的输入 token 预算（scheduler 据此分批）
    /// 传统翻译返回较小值（按条数分批即可），AI 翻译按模型类型返回
    fn max_batch_tokens(&self) -> usize {
        3000
    }

    /// 从文本中提取人名（仅 AI 翻译支持，传统翻译返回 NotSupported）
    /// system_prompt / user_prompt 由调用方构建
    async fn extract_names_raw(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
    ) -> Result<String, AppError> {
        Err(AppError::TranslateNotConfigured)
    }
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

    /// 按模型类型返回输入 token 预算（每批 API 请求的输入上限估算）
    /// 取模型上下文窗口的 ~10%，预留输出 + system prompt 空间
    pub fn max_input_tokens(&self) -> usize {
        match self {
            // Qwen3 系列：上下文 128K，取 12000
            ModelType::Qwen3 => 12000,
            // DeepSeek 系列：上下文 64K，取 8000
            ModelType::Deepseek => 8000,
            // Generic（含本地小模型）：保守取 3000，兼容 4K 上下文的小模型
            ModelType::Generic => 3000,
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
                 Output format (JSON):\n\
                 - Return a JSON array of objects: [{\"n\": 1, \"t\": \"<translation1>\"}, {\"n\": 2, \"t\": \"<translation2>\"}]\n\
                 - Each object contains the input line number \"n\" and the translation \"t\".\n\
                 - Preserve special Unicode characters (like \u{E001}) exactly as-is.\n\
                 - Each input line is an independent subtitle entry. Do NOT merge, split, or skip any lines, even if a sentence appears to span multiple lines.\n\
                 - The output array must contain exactly the same number of objects as the input lines.\n\
                 - Do not add explanations, notes, or any extra text outside the JSON.\n\n\
                 Example:\n\
                 Input:\n\
                 1. And it's 24 millimetres\n\
                 2. you need every week.\n\
                 3. [clattering continues]\n\
                 Output:\n\
                 [{\"n\": 1, \"t\": \"而每周需要二十四毫米。\"}, {\"n\": 2, \"t\": \"你每周都需要。\"}, {\"n\": 3, \"t\": \"[碰撞声持续]\"}]",
        user_line_format: "{index}. {text}",
    }),
    ("deepseek", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate from {src} to {tgt}.\n\n\
                 Output format (JSON):\n\
                 - Return a JSON array of objects: [{\"n\": 1, \"t\": \"<translation1>\"}, {\"n\": 2, \"t\": \"<translation2>\"}]\n\
                 - Each object contains the input line number \"n\" and the translation \"t\".\n\
                 - Preserve all special characters and placeholders unchanged.\n\
                 - Each input line is an independent subtitle entry. Do NOT merge, split, or skip any lines, even if a sentence appears to span multiple lines.\n\
                 - The output array must contain exactly the same number of objects as the input lines.\n\
                 - Do not add any extra text outside the JSON.\n\n\
                 Example:\n\
                 Input:\n\
                 1. And it's 24 millimetres\n\
                 2. you need every week.\n\
                 3. [clattering continues]\n\
                 Output:\n\
                 [{\"n\": 1, \"t\": \"而每周需要二十四毫米。\"}, {\"n\": 2, \"t\": \"你每周都需要。\"}, {\"n\": 3, \"t\": \"[碰撞声持续]\"}]",
        user_line_format: "{index}. {text}",
    }),
    ("generic", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate the following {src} subtitles into {tgt}.\n\n\
                 Output format (JSON):\n\
                 - Return a JSON array of objects: [{\"n\": 1, \"t\": \"<translation1>\"}, {\"n\": 2, \"t\": \"<translation2>\"}]\n\
                 - Each object contains the input line number \"n\" and the translation \"t\".\n\
                 - Preserve special Unicode characters exactly as-is.\n\
                 - Each input line is an independent subtitle entry. Do NOT merge, split, or skip any lines, even if a sentence appears to span multiple lines.\n\
                 - The output array must contain exactly the same number of objects as the input lines.\n\
                 - Do not add any extra text outside the JSON.\n\n\
                 Example:\n\
                 Input:\n\
                 1. And it's 24 millimetres\n\
                 2. you need every week.\n\
                 3. [clattering continues]\n\
                 Output:\n\
                 [{\"n\": 1, \"t\": \"而每周需要二十四毫米。\"}, {\"n\": 2, \"t\": \"你每周都需要。\"}, {\"n\": 3, \"t\": \"[碰撞声持续]\"}]",
        user_line_format: "{index}. {text}",
    }),
];

/// ISO 639-1 语言码 → 英文全称（用于 prompt 占位符 {src} / {tgt}）
pub fn lang_full_name(code: &str) -> &'static str {
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

            // 检测真正的换行符（SRT 中的 \n 0x0A）
            // 替换成占位符而非保留原样，避免 9b 模型把多行条目拆成多条翻译导致错位
            if remaining.starts_with('\n') {
                let placeholder = self.add_placeholder("\n");
                result.push(placeholder);
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
                if let Some(detail) = check_insufficient_balance(status, &body) {
                    return Err(AppError::TranslateInsufficientBalance {
                        provider: "baidu".to_string(),
                        detail,
                    });
                }
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
                // 54003 之外的错误，检查余额不足
                let full_msg = format!("error_code: {}, msg: {}", code, msg);
                if let Some(detail) = check_insufficient_balance(reqwest::StatusCode::OK, &full_msg) {
                    return Err(AppError::TranslateInsufficientBalance {
                        provider: "baidu".to_string(),
                        detail,
                    });
                }
                return Err(AppError::TranslateNetworkError {
                    provider: "baidu".to_string(),
                    detail: full_msg,
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

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "bing".to_string(),
                detail,
            });
        }

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

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "google".to_string(),
                detail,
            });
        }

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
    /// 服务商显示名（如 "DeepSeek" / "LM Studio"），用于错误消息
    service_name: String,
    client: reqwest::Client,
    /// 累计 token 用量（原子计数器，线程安全）
    prompt_tokens: std::sync::atomic::AtomicU64,
    completion_tokens: std::sync::atomic::AtomicU64,
    total_tokens: std::sync::atomic::AtomicU64,
    /// 译名表：(EnglishName, ChineseTranslation)，注入到 system prompt
    glossary: Vec<(String, String)>,
    /// 是否要求模型在译文中用 <name=EnglishName>中文</name> 标记人名
    name_tagging: bool,
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
        Self {
            base_url,
            model,
            model_type,
            api_key,
            service_name: "OpenAI".to_string(),
            client,
            prompt_tokens: std::sync::atomic::AtomicU64::new(0),
            completion_tokens: std::sync::atomic::AtomicU64::new(0),
            total_tokens: std::sync::atomic::AtomicU64::new(0),
            glossary: Vec::new(),
            name_tagging: false,
        }
    }

    /// 设置服务商显示名（用于错误消息中显示真实服务商名而非 "openai"）
    pub fn with_service_name(mut self, name: String) -> Self {
        self.service_name = name;
        self
    }

    /// 设置译名表（注入到 system prompt，保证跨 batch 人名一致）
    pub fn with_glossary(mut self, glossary: Vec<(String, String)>) -> Self {
        self.glossary = glossary;
        self
    }

    /// 启用/禁用人名标记模式（要求模型在译文中用 <name=En>Zh</name> 标记人名）
    pub fn with_name_tagging(mut self, enabled: bool) -> Self {
        self.name_tagging = enabled;
        self
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

        // 1. 尝试解析 JSON 数组格式：[{"n": 1, "t": "..."}, ...]
        // AI 可能用 ```json ... ``` 代码块包裹，先剥离 markdown 代码块
        let trimmed = content.trim();
        let json_content = strip_markdown_code_fence(trimmed);
        if json_content.starts_with('[') {
            // 1a. 标准 JSON 解析
            if let Ok(json) = serde_json::from_str::<Vec<serde_json::Value>>(&json_content) {
                let mut translations: std::collections::HashMap<usize, String> =
                    std::collections::HashMap::new();
                // 检测 AI 是否拆分了条目：如果返回的 n 超出 expected_count，
                // 说明 AI 把含多行的条目拆成了多个翻译（如 "Jerry:\nUhh" → n=1 "Jerry" + n=2 "Uhh"），
                // 导致后续所有翻译错位。此时整批对齐失败，触发降级重试（batch_size=1 时无法拆分）。
                let mut out_of_range = false;
                for item in json {
                    if let (Some(n), Some(t)) = (
                        item.get("n").and_then(|v| v.as_u64()).map(|n| n as usize),
                        item.get("t").and_then(|v| v.as_str()).map(|s| s.trim()),
                    ) {
                        if n > 0 && n <= expected_count {
                            translations.insert(n, t.to_string());
                        } else if n > expected_count {
                            out_of_range = true;
                        }
                    }
                }
                if out_of_range {
                    tracing::warn!(
                        "JSON 解析：AI 返回了超出范围的编号（n > {}），可能拆分了条目，返回对齐失败",
                        expected_count
                    );
                    return Err(AppError::TranslateAlignFailed {
                        missing: expected_count,
                    });
                }
                if translations.len() == expected_count {
                    let result: Vec<String> = (1..=expected_count)
                        .map(|i| translations.remove(&i).unwrap_or_default())
                        .collect();
                    return Ok(result);
                }
                if !translations.is_empty() {
                    // JSON 解析成功但数量不匹配，返回部分结果（空字符串占位）
                    // 调度器会对空字符串的条目进行降级重试
                    tracing::warn!(
                        "JSON 解析数量不匹配：期望 {}，实际 {}，返回部分结果",
                        expected_count,
                        translations.len()
                    );
                    let result: Vec<String> = (1..=expected_count)
                        .map(|i| translations.remove(&i).unwrap_or_default())
                        .collect();
                    return Ok(result);
                }
            } else {
                // 1b. 标准 JSON 解析失败，尝试用正则从 JSON 文本中提取 {"n": N, "t": "..."} 对
                // AI 有时会输出未转义的双引号导致 JSON 格式错误
                let re_json = regex::Regex::new(
                    r#""n"\s*:\s*(\d+)\s*,\s*"t"\s*:\s*"((?:[^"\\]|\\.)*)""#
                ).unwrap();
                let mut translations: std::collections::HashMap<usize, String> =
                    std::collections::HashMap::new();
                let mut out_of_range = false;
                for cap in re_json.captures_iter(&json_content) {
                    if let (Ok(n), Some(t)) = (cap[1].parse::<usize>(), cap.get(2)) {
                        if n > 0 && n <= expected_count {
                            // 反转义 JSON 字符串中的转义字符
                            let text = t.as_str()
                                .replace("\\\"", "\"")
                                .replace("\\\\", "\\")
                                .replace("\\n", "\n")
                                .replace("\\t", "\t");
                            translations.insert(n, text.trim().to_string());
                        } else if n > expected_count {
                            out_of_range = true;
                        }
                    }
                }
                if out_of_range {
                    tracing::warn!(
                        "JSON 正则提取：AI 返回了超出范围的编号（n > {}），可能拆分了条目，返回对齐失败",
                        expected_count
                    );
                    return Err(AppError::TranslateAlignFailed {
                        missing: expected_count,
                    });
                }
                if !translations.is_empty() {
                    tracing::warn!(
                        "JSON 正则提取：期望 {}，提取 {}，返回部分结果",
                        expected_count,
                        translations.len()
                    );
                    let result: Vec<String> = (1..=expected_count)
                        .map(|i| translations.remove(&i).unwrap_or_default())
                        .collect();
                    return Ok(result);
                }
            }
        }

        // 2. 尝试按编号解析（用剥离代码块后的内容，避免 ```json 行干扰）
        let parse_content = if json_content.starts_with('[') || json_content.starts_with("```") {
            &json_content
        } else {
            trimmed
        };
        let mut translations: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
        let mut out_of_range = false;
        for line in parse_content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(captures) = re.captures(line) {
                let num: usize = captures[1].parse().unwrap_or(0);
                let text = captures.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                if num > 0 && num <= expected_count {
                    translations.insert(num, text.to_string());
                } else if num > expected_count {
                    out_of_range = true;
                }
            }
        }
        if out_of_range {
            tracing::warn!(
                "编号解析：AI 返回了超出范围的编号（n > {}），可能拆分了条目，返回对齐失败",
                expected_count
            );
            return Err(AppError::TranslateAlignFailed {
                missing: expected_count,
            });
        }

        // 3. 按编号顺序组装结果
        if translations.len() == expected_count {
            let result: Vec<String> = (1..=expected_count)
                .map(|i| translations.remove(&i).unwrap_or_default())
                .collect();
            return Ok(result);
        }

        // 4. 编号解析失败 → 退化为按行对齐（去掉编号前缀）
        let lines: Vec<String> = parse_content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .map(|l| {
                // 去掉可能残留的编号前缀 "1. " / "1) " / "1、" 等，避免翻译结果含编号
                re.replace(&l, "${2}").to_string()
            })
            .collect();
        if lines.len() == expected_count {
            return Ok(lines);
        }

        // 5. 行数也不对 → 返回对齐失败，由调度器触发逐条重试
        tracing::warn!(
            "编号解析失败：期望 {} 条，编号匹配 {} 条，行对齐 {} 行。原始输出前 500 字符: {:?}",
            expected_count,
            translations.len(),
            lines.len(),
            content.chars().take(500).collect::<String>()
        );
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
        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user",   "content": user_prompt },
            ],
            "temperature": 0.3,
            "stream": true,
        });
        // Qwen3 系列关闭 thinking 模式，避免 reasoning 内容干扰 JSON 解析并节省时间
        // 但 "nothink" 版本本身已禁用 thinking，不支持 chat_template_kwargs 参数，
        // 强行添加会导致 LM Studio 流式响应返回空内容。
        // Qwen3 系列关闭 thinking 模式，避免 reasoning 内容干扰 JSON 解析并节省时间
        // 但 "nothink" 版本本身已禁用 thinking，不支持 chat_template_kwargs 参数，
        // 强行添加会导致 LM Studio 流式响应返回空内容。
        if self.model_type == ModelType::Qwen3 && !self.model.to_lowercase().contains("nothink") {
            request_body["chat_template_kwargs"] = serde_json::json!({
                "enable_thinking": false
            });
        }
        let timeout_secs = if self.is_local_url() { 1800 } else { 120 };
        let chunk_timeout_secs = if self.is_local_url() { 300 } else { 60 };
        let mut req = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .json(&request_body);

        // api_key 非空时才带认证头（局域网无认证场景）
        // Azure OpenAI 使用 api-key 头而非 Authorization: Bearer
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                if self.base_url.contains("openai.azure.com") {
                    req = req.header("api-key", key);
                } else {
                    req = req.header("Authorization", format!("Bearer {}", key));
                }
            }
        }

        // 流式日志：从 task_local 读取当前并发槽位的文件句柄
        let stream_log_file = crate::STREAM_LOG_FILE.try_get().ok();
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n========== 翻译批次 ==========\n时间: {}\nProvider: {}\nModel: {}\n\n--- 请求体 ---\n{}\n\n--- 流式响应 ---\n",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                self.service_name, self.model,
                request_body.to_string(),
            ));
        }

        let mut resp = req.send().await.map_err(|e| AppError::TranslateNetworkError {
            provider: self.service_name.clone(),
            detail: e.to_string(),
        })?;

        let status = resp.status();

        // 限流
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::TranslateRateLimit {
                provider: self.service_name.clone(),
                retry_after: Some(60),
            });
        }

        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            // 余额不足：优先于认证失败判断（部分服务商余额不足时返回 403 而非 402）
            if let Some(detail) = check_insufficient_balance(status, &error_body) {
                return Err(AppError::TranslateInsufficientBalance {
                    provider: self.service_name.clone(),
                    detail,
                });
            }
            // 认证失败（401/403）：排除了余额不足后才判定为认证失败
            if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
                return Err(AppError::TranslateAuthFailed {
                    provider: self.service_name.clone(),
                });
            }
            crate::log_api_debug(
                &self.service_name, &self.model, "auto", "auto",
                &request_body.to_string(), &error_body, status.as_u16(),
            );
            if let Some(ref log_file) = stream_log_file {
                crate::log_stream_to_file(log_file, &format!(
                    "\n[HTTP {}] {}\n\n========== 翻译批次结束（错误）==========\n",
                    status, error_body.chars().take(200).collect::<String>(),
                ));
            }
            return Err(AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
                detail: format!("HTTP {}: {}", status, error_body.chars().take(200).collect::<String>()),
            });
        }

        // 流式读取 SSE
        let mut buffer = String::new();
        let mut full_content = String::new();
        let mut seen_content = false;
        let mut prompt_tokens = 0u64;
        let mut completion_tokens = 0u64;

        loop {
            let chunk_result = tokio::time::timeout(
                std::time::Duration::from_secs(chunk_timeout_secs),
                resp.chunk(),
            ).await.map_err(|_| {
                crate::log_api_debug(
                    &self.service_name, &self.model, "auto", "auto",
                    &request_body.to_string(),
                    &format!("[chunk timeout after {} chars, {}s no data]", full_content.len(), chunk_timeout_secs),
                    200,
                );
                AppError::TranslateNetworkError {
                    provider: self.service_name.clone(),
                    detail: format!("stream chunk timeout: {}s no data", chunk_timeout_secs),
                }
            })?;

            let chunk = chunk_result.map_err(|e| AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
                detail: format!("stream chunk error: {}", e),
            })?;

            let Some(chunk) = chunk else { break; };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() { continue; }

                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str.trim() == "[DONE]" { continue; }

                    if let Ok(chunk_json) = serde_json::from_str::<serde_json::Value>(json_str) {
                        let delta_obj = &chunk_json["choices"][0]["delta"];

                        // Qwen3 thinking 模式：reasoning_content 实时记录到日志
                        if let Some(reasoning) = delta_obj["reasoning_content"].as_str() {
                            if !reasoning.is_empty() {
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, reasoning);
                                }
                            }
                        }

                        // 累积 content
                        if let Some(delta) = delta_obj["content"].as_str() {
                            if !delta.is_empty() {
                                if !seen_content {
                                    seen_content = true;
                                    if let Some(ref log_file) = stream_log_file {
                                        crate::log_stream_to_file(log_file, "\n\n--- 最终输出 ---\n");
                                    }
                                }
                                full_content.push_str(delta);
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, delta);
                                }
                            }
                        }

                        // 累积 usage
                        if let Some(usage) = chunk_json.get("usage") {
                            prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
                            completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
                        }
                    }
                }
            }
        }

        // LM Studio 流式响应可能不返回 usage，用字符数估算
        if prompt_tokens == 0 || completion_tokens == 0 {
            let estimated_prompt = (system_prompt.len() + user_prompt.len()) / 4;
            let estimated_completion = full_content.len() / 3;
            if prompt_tokens == 0 { prompt_tokens = estimated_prompt as u64; }
            if completion_tokens == 0 { completion_tokens = estimated_completion as u64; }
        }

        // 流式日志：结束汇总
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n=== 翻译批次结束 ===\n总字符数: {}\nprompt_tokens: {}\ncompletion_tokens: {}\n时间: {}\n",
                full_content.len(), prompt_tokens, completion_tokens,
                chrono::Local::now().format("%H:%M:%S"),
            ));
        }

        // 累计 token 用量
        if prompt_tokens > 0 || completion_tokens > 0 {
            use std::sync::atomic::Ordering;
            self.prompt_tokens.fetch_add(prompt_tokens, Ordering::Relaxed);
            self.completion_tokens.fetch_add(completion_tokens, Ordering::Relaxed);
            self.total_tokens.fetch_add(prompt_tokens + completion_tokens, Ordering::Relaxed);
        }

        // 开发者模式：记录 API 调试日志
        crate::log_api_debug(
            &self.service_name,
            &self.model,
            "auto",
            "auto",
            &request_body.to_string(),
            &full_content,
            200,
        );

        Self::parse_numbered_response(&full_content, texts.len())
    }

    /// 判断 base_url 是否为本地模型
    fn is_local_url(&self) -> bool {
        self.base_url.contains("localhost")
            || self.base_url.contains("127.0.0.1")
            || self.base_url.contains("192.168.")
            || self.base_url.contains("10.")
            || self.base_url.contains("172.16.")
            || self.base_url.contains("172.17.")
            || self.base_url.contains("172.18.")
            || self.base_url.contains("172.19.")
            || self.base_url.contains("172.20.")
            || self.base_url.contains("172.21.")
            || self.base_url.contains("172.22.")
            || self.base_url.contains("172.23.")
            || self.base_url.contains("172.24.")
            || self.base_url.contains("172.25.")
            || self.base_url.contains("172.26.")
            || self.base_url.contains("172.27.")
            || self.base_url.contains("172.28.")
            || self.base_url.contains("172.29.")
            || self.base_url.contains("172.30.")
            || self.base_url.contains("172.31.")
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
        // token 预算分块：按模型类型取预算，粗估 token = chars / 3
        let max_input_tokens = self.model_type.max_input_tokens();

        let mut chunks: Vec<Vec<&String>> = Vec::new();
        let mut current: Vec<&String> = Vec::new();
        let mut current_tokens = 0usize;
        for text in texts {
            let tokens = text.chars().count() / 3 + 1;
            if !current.is_empty() && current_tokens + tokens > max_input_tokens {
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

    fn token_usage(&self) -> Option<TokenUsage> {
        use std::sync::atomic::Ordering;
        let pt = self.prompt_tokens.load(Ordering::Relaxed);
        let ct = self.completion_tokens.load(Ordering::Relaxed);
        let tt = self.total_tokens.load(Ordering::Relaxed);
        if pt == 0 && ct == 0 && tt == 0 {
            None
        } else {
            Some(TokenUsage {
                prompt_tokens: pt,
                completion_tokens: ct,
                total_tokens: tt,
            })
        }
    }

    fn max_batch_tokens(&self) -> usize {
        self.model_type.max_input_tokens()
    }

    /// 人名提取：用自定义 system/user prompt 调用 chat completion，返回纯文本响应
    async fn extract_names_raw(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user",   "content": user_prompt },
            ],
            "temperature": 0,
            "stream": true,
            // stop 序列：JSON 格式下只需少量兜底
            "stop": [
                "\n\n", "\nNote:", "\nLet's", "\nHowever", "\nBut ", "\nAlso",
                "\n\nNote:", "\n\nLet's", "\n\nHowever", "\n\nBut ",
            ],
        });
        // response_format: json_object 并非所有 OpenAI 兼容 API 都支持
        // 已知支持：OpenAI、DeepSeek、通义千问
        // 已知不支持/不确定：LM Studio、Ollama、其他本地推理引擎
        // 对云端 API 加 response_format，对本地 URL 不加（避免 400 错误）
        if !self.is_local_url() {
            request_body["response_format"] = serde_json::json!({ "type": "json_object" });
        }
        if self.model_type == ModelType::Qwen3 && !self.model.to_lowercase().contains("nothink") {
            request_body["chat_template_kwargs"] = serde_json::json!({
                "enable_thinking": false
            });
        }
        let timeout_secs = if self.is_local_url() { 1800 } else { 120 };
        let chunk_timeout_secs = if self.is_local_url() { 300 } else { 60 };
        let mut req = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .json(&request_body);
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                if self.base_url.contains("openai.azure.com") {
                    req = req.header("api-key", key);
                } else {
                    req = req.header("Authorization", format!("Bearer {}", key));
                }
            }
        }

        // 流式实时日志：从 task_local 读取当前并发槽位的文件句柄
        let stream_log_file = crate::STREAM_LOG_FILE.try_get().ok();
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n========== 人名预扫描开始 ==========\n时间: {}\nProvider: {}\nModel: {}\n\n--- 请求体 ---\n{}\n\n--- 流式响应 ---\n",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                self.service_name, self.model,
                request_body.to_string(),
            ));
        }

        let mut resp = req.send().await.map_err(|e| AppError::TranslateNetworkError {
            provider: self.service_name.clone(),
            detail: e.to_string(),
        })?;
        let status = resp.status();

        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            crate::log_api_debug(
                &self.service_name,
                &self.model,
                "auto",
                "auto",
                &request_body.to_string(),
                &error_body,
                status.as_u16(),
            );
            if let Some(ref log_file) = stream_log_file {
                crate::log_stream_to_file(log_file, &format!(
                    "\n[HTTP {}] {}\n\n========== 人名预扫描结束（错误）==========\n",
                    status, error_body,
                ));
            }
            return Err(AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
                detail: format!("HTTP {}: {}", status, error_body.chars().take(200).collect::<String>()),
            });
        }

        // 流式读取 SSE
        let mut buffer = String::new();
        let mut full_content = String::new();
        let mut seen_content = false;
        let mut prompt_tokens = 0u64;
        let mut completion_tokens = 0u64;

        loop {
            let chunk_result = tokio::time::timeout(
                std::time::Duration::from_secs(chunk_timeout_secs),
                resp.chunk(),
            ).await.map_err(|_| {
                crate::log_api_debug(
                    &self.service_name,
                    &self.model,
                    "auto",
                    "auto",
                    &request_body.to_string(),
                    &format!("[chunk timeout after {} chars, {}s no data]", full_content.len(), chunk_timeout_secs),
                    200,
                );
                AppError::TranslateNetworkError {
                    provider: self.service_name.clone(),
                    detail: format!("stream chunk timeout: {}s no data", chunk_timeout_secs),
                }
            })?;

            let chunk = chunk_result.map_err(|e| AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
                detail: format!("stream chunk error: {}", e),
            })?;

            let Some(chunk) = chunk else { break; };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() { continue; }

                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str.trim() == "[DONE]" { continue; }

                    if let Ok(chunk_json) = serde_json::from_str::<serde_json::Value>(json_str) {
                        let delta_obj = &chunk_json["choices"][0]["delta"];

                        // Qwen3 thinking 模式：reasoning_content 实时记录到日志
                        if let Some(reasoning) = delta_obj["reasoning_content"].as_str() {
                            if !reasoning.is_empty() {
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, reasoning);
                                }
                            }
                        }

                        // 累积 content
                        if let Some(delta) = delta_obj["content"].as_str() {
                            if !delta.is_empty() {
                                if !seen_content {
                                    seen_content = true;
                                    if let Some(ref log_file) = stream_log_file {
                                        crate::log_stream_to_file(log_file, "\n\n--- 最终输出 ---\n");
                                    }
                                }
                                full_content.push_str(delta);
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, delta);
                                }
                            }
                        }

                        // 累积 usage
                        if let Some(usage) = chunk_json.get("usage") {
                            prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
                            completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
                        }
                    }
                }
            }
        }

        // LM Studio 流式响应可能不返回 usage 字段，用字符数估算
        // 估算必须在日志之前，否则日志显示的 token 数永远是 0
        if prompt_tokens == 0 || completion_tokens == 0 {
            // 估算：prompt token ≈ system+user 字符数 / 4，completion token ≈ 输出字符数 / 3
            let estimated_prompt = (system_prompt.len() + user_prompt.len()) / 4;
            let estimated_completion = full_content.len() / 3;
            if prompt_tokens == 0 { prompt_tokens = estimated_prompt as u64; }
            if completion_tokens == 0 { completion_tokens = estimated_completion as u64; }
        }

        // 流式日志：结束汇总
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n=== 人名预扫描结束 ===\n总字符数: {}\nprompt_tokens: {}\ncompletion_tokens: {}\n时间: {}\n",
                full_content.len(), prompt_tokens, completion_tokens,
                chrono::Local::now().format("%H:%M:%S"),
            ));
        }

        // 累计 token 用量
        if prompt_tokens > 0 || completion_tokens > 0 {
            use std::sync::atomic::Ordering;
            self.prompt_tokens.fetch_add(prompt_tokens, Ordering::Relaxed);
            self.completion_tokens.fetch_add(completion_tokens, Ordering::Relaxed);
            self.total_tokens.fetch_add(prompt_tokens + completion_tokens, Ordering::Relaxed);
        }

        // 开发者模式：记录 API 调试日志
        crate::log_api_debug(
            &self.service_name,
            &self.model,
            "auto",
            "auto",
            &request_body.to_string(),
            &full_content,
            200,
        );

        Ok(full_content)
    }
}

// === SECTION 5.5 END ===

/// 翻译调度器
/// 负责缓存查询、分段、占位符保护、限流重试
pub struct TranslateScheduler<'a> {
    db: &'a Database,
    provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
    provider_name: String,
    /// 代际计数器（全局共享）
    cancel_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// 本任务的代际号，counter != my_gen 表示被取消
    my_gen: u64,
    concurrency: usize,
    /// 限流策略：Qps 模式下请求间强制间隔，Concurrency 模式下纯并发控制
    rate_limit: RateLimitPolicy,
    /// 字幕内容 hash，用于缓存隔离（空字符串=兼容旧调用方）
    file_hash: String,
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
            cancel_counter: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            my_gen: 0,
            concurrency: 1,
            rate_limit: RateLimitPolicy::Concurrency(1),
            file_hash: String::new(),
        }
    }

    pub fn with_cancel_token(
        db: &'a Database,
        provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
        provider_name: String,
        cancel_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
        my_gen: u64,
    ) -> Self {
        Self {
            db,
            provider,
            provider_name,
            cancel_counter,
            my_gen,
            concurrency: 1,
            rate_limit: RateLimitPolicy::Concurrency(1),
            file_hash: String::new(),
        }
    }

    /// 设置字幕内容 hash（用于缓存隔离）
    pub fn with_file_hash(mut self, file_hash: String) -> Self {
        self.file_hash = file_hash;
        self
    }

    /// 设置并发数和限流策略
    /// Qps 模式：并发固定为 1，请求间强制间隔 1/N 秒
    /// Concurrency 模式：并发 = min(用户配置, 策略上限)
    pub fn with_concurrency_and_rate_limit(
        mut self,
        user_concurrency: usize,
        rate_limit: RateLimitPolicy,
    ) -> Self {
        self.rate_limit = rate_limit;
        self.concurrency = match rate_limit {
            RateLimitPolicy::Qps(_) => 1,
            RateLimitPolicy::Concurrency(max_n) => user_concurrency.min(max_n).max(1),
        };
        self
    }

    /// 兼容旧接口：仅设置并发数（不设限流策略）
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != self.my_gen
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
                &self.file_hash,
            );
            if let Some(cached) = self.db.get_translate_cache(&cache_key)? {
                // 缓存命中后做质量校验，忽略坏缓存重新翻译：
                // 1. 音效标记不一致（如英文短句被缓存成了中文音效标记）
                // 2. 译文=原文（AI 未实际翻译，原样返回）——但音乐符号/音效标记/非英语内容保持原样是正确行为
                // 3. 目标语言是中文但译文无 CJK（且原文也无 CJK）——但音乐符号/音效标记/非英语内容不需要 CJK
                // 注意：质量校验逻辑必须与 translate_entries_full 的 failed 判定 + 缓存写入逻辑对称，
                // 否则翻译时写入缓存的条目，恢复时会被错误忽略（缓存恢复译文不一致）。
                let is_music_or_sfx = is_music_or_symbol_only(&entry.text)
                    || looks_like_sound_effect(&entry.text);
                // 非英语内容（如拼写字母 "G-O-R..."、祖鲁语歌词等）保持原样是正确行为，
                // 与 translate_entries_full 的 is_non_english 判定一致
                let is_non_english = !has_english_word(&entry.text, 3);
                if looks_like_sound_effect(&entry.text) != looks_like_sound_effect(&cached) {
                    // 音效标记不一致（如含 {\an8} 定位标签的混合条目被译成纯音效标记）：
                    // 翻译时应跳过重新翻译，但恢复时（get_cached_entries）仍返回缓存，
                    // 保证译文一致。标记 failed=true，前端视为未翻译，用户可重新翻译。
                    results.push(TranslateEntry {
                        index: entry.index,
                        original: entry.text.clone(),
                        translated: cached,
                        from_cache: true,
                        failed: true,
                    });
                    continue;
                }
                if cached.trim() == entry.text.trim() && !is_music_or_sfx && !is_non_english {
                    // 英语内容译文=原文（AI 未实际翻译）：翻译时应跳过重新翻译，
                    // 但恢复时（get_cached_entries）仍返回缓存，保证译文一致。
                    // 标记 failed=true，与翻译时 failed 判定一致，前端会视为未翻译。
                    results.push(TranslateEntry {
                        index: entry.index,
                        original: entry.text.clone(),
                        translated: cached,
                        from_cache: true,
                        failed: true,
                    });
                    continue;
                }
                if target_lang.starts_with("zh")
                    && !has_cjk(&cached)
                    && !has_cjk(&entry.text)
                    && !is_music_or_sfx
                    && !is_non_english
                {
                    tracing::warn!(
                        "缓存译文无 CJK，忽略缓存重新翻译: index={}, text=[{}], cached=[{}]",
                        entry.index,
                        entry.text,
                        cached
                    );
                    continue;
                }
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
        self.translate_entries_full(entries, source_lang, target_lang, max_single_length, on_progress, None, false).await
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
        skip_cache: bool,
    ) -> Result<TranslateResult, AppError> {
        let mut results = Vec::with_capacity(entries.len());
        let mut cached_count = 0;
        let mut to_translate: Vec<(usize, String, String, PlaceholderProtector)> = Vec::new();
        // to_translate 元素：(index, original_text, protected_text, protector)
        // original_text 用于缓存 key，protected_text 用于发送给 API

        // 1. 缓存查询 + 占位符保护（skip_cache=true 时跳过缓存）
        for entry in entries {
            // 跳过 ass 矢量绘图指令（含 \p1 标记），不是字幕文本
            if entry.text.contains("\\p1") {
                tracing::info!("字幕 #{} 含 \\p1 绘图指令，跳过翻译", entry.index);
                continue;
            }

            // 跳过纯音乐符号/特殊符号（如 ♪♪、♬♬ 等），直接保持原样
            // 9b 模型会把 ♪♪ 翻译成 "哦。" 等无关内容，导致错位
            if is_music_or_symbol_only(&entry.text) {
                tracing::info!("字幕 #{} 为纯音乐符号，跳过翻译: {:?}", entry.index, entry.text);
                // 写入缓存，保证 get_cached_entries 能恢复（避免缓存恢复译文不一致）
                if !skip_cache {
                    let cache_key = translate_cache_key(
                        &entry.text,
                        source_lang,
                        target_lang,
                        &self.provider_name,
                        &self.file_hash,
                    );
                    let _ = self.db.set_translate_cache(
                        &cache_key,
                        &entry.text,
                        &entry.text,
                        source_lang,
                        target_lang,
                        &self.provider_name,
                    );
                }
                let te = TranslateEntry {
                    index: entry.index,
                    original: entry.text.clone(),
                    translated: entry.text.clone(),
                    from_cache: false,
                    failed: false,
                };
                if let Some(ref cb) = on_entry_done {
                    cb(&te);
                }
                results.push(te);
                continue;
            }

            if !skip_cache {
                let cache_key = translate_cache_key(
                    &entry.text,
                    source_lang,
                    target_lang,
                    &self.provider_name,
                    &self.file_hash,
                );

                if let Some(cached) = self.db.get_translate_cache(&cache_key)? {
                    // 缓存质量校验（与 get_cached_entries 一致）
                    // 音乐符号/音效标记/非英语内容保持原样是正确行为，不算坏缓存
                    let is_music_or_sfx = is_music_or_symbol_only(&entry.text)
                        || looks_like_sound_effect(&entry.text);
                    let is_non_english = !has_english_word(&entry.text, 3);
                    let bad_cache = (looks_like_sound_effect(&entry.text) != looks_like_sound_effect(&cached))
                        || (!is_music_or_sfx && !is_non_english && cached.trim() == entry.text.trim())
                        || (target_lang.starts_with("zh") && !has_cjk(&cached) && !has_cjk(&entry.text) && !is_music_or_sfx && !is_non_english);
                    if !bad_cache {
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
                    tracing::warn!(
                        "缓存质量校验失败，忽略缓存重新翻译: index={}, text=[{}]",
                        entry.index,
                        entry.text
                    );
                }
            } else {
                if entry.text.contains("Waaaa") || entry.text.contains("What's your name") {
                    eprintln!("[DEBUG CACHE] #{} cache MISS", entry.index);
                }
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
                // 翻译失败判定
                let same_as_orig = restored.trim() == entry.text.trim();
                let no_cjk = target_lang.starts_with("zh")
                    && !has_cjk(&restored)
                    && !has_cjk(&entry.text);
                let orig_is_sound = looks_like_sound_effect(&entry.text);
                let restored_is_sound = looks_like_sound_effect(&restored);
                let sound_mismatch = orig_is_sound != restored_is_sound;
                // 长度比值异常：短原文被翻译成长译文（批次错位）
                let orig_len = entry.text.chars().count().max(1);
                let trans_len = restored.chars().count();
                let length_ratio_abnormal = trans_len > 0 && {
                    let ratio = trans_len as f64 / orig_len as f64;
                    ratio > 5.0 && trans_len > 10
                };
                if !restored.is_empty() && !any_failed && !same_as_orig && !no_cjk && !sound_mismatch && !length_ratio_abnormal {
                    let cache_key = translate_cache_key(
                        &entry.text,
                        source_lang,
                        target_lang,
                        &self.provider_name,
                        &self.file_hash,
                    );
                    let _ = self.db.set_translate_cache(
                        &cache_key,
                        &entry.text,
                        &restored,
                        source_lang,
                        target_lang,
                        &self.provider_name,
                    );
                } else if !restored.is_empty() && (same_as_orig || sound_mismatch) {
                    // 译文=原文或音效标记不一致的 failed 条目写入缓存，保证恢复时译文一致
                    let cache_key = translate_cache_key(
                        &entry.text,
                        source_lang,
                        target_lang,
                        &self.provider_name,
                        &self.file_hash,
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
                    failed: any_failed || combined.is_empty() || same_as_orig || no_cjk || sound_mismatch || length_ratio_abnormal,
                };
                if let Some(ref cb) = on_entry_done {
                    cb(&te);
                }
                results.push(te);
                if let Some(ref cb) = on_progress {
                    cb(results.len(), entries.len());
                }
            } else {
                to_translate.push((entry.index, entry.text.clone(), protected_text, protector));
            }
        }

        // 2. 分批翻译（带重试），并发度由 self.concurrency 控制
        // 策略：长条目按 30 条一批提高效率；短条目（<30 字节）单独按 5 条一批，
        // 避免 AI 把短句和相邻长句合并翻译导致整条批次偏移。
        const LONG_BATCH_SIZE: usize = 30;
        const SHORT_BATCH_SIZE: usize = 5;
        const SHORT_TEXT_THRESHOLD: usize = 30;
        if !to_translate.is_empty() {
            let (mut short_entries, mut long_entries): (
                Vec<(usize, String, String, PlaceholderProtector)>,
                Vec<(usize, String, String, PlaceholderProtector)>,
            ) = to_translate.into_iter().partition(|(_, _, t, _)| t.len() < SHORT_TEXT_THRESHOLD);
            let mut batches: Vec<Vec<(usize, String, String, PlaceholderProtector)>> = Vec::new();
            batches.extend(long_entries.chunks(LONG_BATCH_SIZE).map(|c| c.to_vec()));
            batches.extend(short_entries.chunks(SHORT_BATCH_SIZE).map(|c| c.to_vec()));
            let total_batches = batches.len();
            let concurrency = self.concurrency.max(1);
            tracing::info!("翻译并发度: {}，共 {} 批", concurrency, total_batches);

            // 并发调用 API：用 Semaphore 控制并发数，JoinSet 收集结果
            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
            let provider = self.provider.clone();
            let cancel_counter = self.cancel_counter.clone();
            let my_gen = self.my_gen;
            let mut join_set = tokio::task::JoinSet::new();

            // 流式实时日志：预创建 concurrency 个文件
            let stream_log_slots = std::sync::Arc::new(crate::create_stream_log_slots(concurrency));
            let slot_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

            for (batch_idx, batch) in batches.iter().enumerate() {
                let texts: Vec<String> = batch.iter().map(|(_, _, t, _)| t.clone()).collect();
                let source = source_lang.to_string();
                let target = target_lang.to_string();
                let provider = provider.clone();
                let cancel_counter = cancel_counter.clone();
                let semaphore = semaphore.clone();
                let stream_log_slots = stream_log_slots.clone();
                let slot_counter = slot_counter.clone();

                join_set.spawn(async move {
                    // 在 task 内部获取信号量，不阻塞 spawn 循环
                    // 这样 while join_next 循环能立即开始处理已完成的结果
                    let _permit = semaphore.acquire_owned().await.unwrap();
                    if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                        return (batch_idx, vec![]);
                    }
                    tracing::info!("翻译批次 {}/{}，本批 {} 条，启用降级重试", batch_idx + 1, total_batches, texts.len());

                    // 分配并发槽位的日志文件
                    let slot_idx = (slot_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % stream_log_slots.len() as u64) as usize;
                    let stream_log_file = stream_log_slots[slot_idx].clone();

                    let result = crate::STREAM_LOG_FILE.scope(stream_log_file, async {
                        translate_batch_with_fallback(
                            &*provider,
                            &texts,
                            &source,
                            &target,
                            &cancel_counter,
                            my_gen,
                        ).await
                    }).await;
                    (batch_idx, result)
                });
            }

            // 批次完成即处理（不要求顺序）：立即回调 on_entry_done / on_progress，
            // 避免 head-of-line blocking（batch 0 慢时后续批次全部等待导致进度卡 0）
            while let Some(res) = join_set.join_next().await {
                let (batch_idx, translations) = match res {
                    Ok(item) => item,
                    Err(e) => {
                        tracing::warn!("join 任务异常: {}", e);
                        continue;
                    }
                };
                if self.is_cancelled() {
                    tracing::info!("翻译已取消，停止后处理（已完成到批次 {}）", batch_idx + 1);
                    break;
                }
                if translations.is_empty() {
                    // 任务被取消或异常，跳过该批次
                    continue;
                }

                let batch = &batches[batch_idx];

                // 检测批次返回数量是否匹配：数量不匹配时，JSON 解析虽然用 n 字段对齐，
                // 但 n 字段本身可能被 AI 写错（如重复、跳号），导致译文与原文错位。
                // 错位的译文可能通过 failed 检查（非空、有 CJK、非音效）被错误缓存，
                // 污染后续翻译和导出再导入的统计。因此数量不匹配时整批不缓存。
                let batch_count_mismatch = translations.len() != batch.len();
                if batch_count_mismatch {
                    tracing::warn!(
                        "翻译批次 {} 返回长度异常：期望 {}，实际 {}，整批标记为不缓存",
                        batch_idx + 1,
                        batch.len(),
                        translations.len()
                    );
                }
                // 确保返回长度与批次一致（降级重试已尽力保证，但做兜底）
                let mut final_translations: Vec<String> = translations;
                while final_translations.len() < batch.len() {
                    final_translations.push(String::new());
                }
                final_translations.truncate(batch.len());

                for ((index, orig_text, _protected_text, protector), translated) in
                    batch.iter().zip(final_translations.iter())
                {
                    let restored = protector.restore(translated);
                    // 翻译失败判定：
                    // 1. 译文为空
                    // 2. 译文与原文相同（AI 未实际翻译，原样返回）
                    // 3. 目标语言是中文但译文无 CJK 字符（AI 只返回了部分原文或改了标签）
                    // 音效标记一致性校验：原文是音效标记但译文不是，或反过来译文是音效标记但原文不是，
                    // 通常意味着 AI 把相邻条目合并/错位翻译了（如 "you need every week." → "[碰撞声持续]"）。
                    let orig_is_sound = looks_like_sound_effect(orig_text);
                    let restored_is_sound = looks_like_sound_effect(&restored);
                    let sound_mismatch = orig_is_sound != restored_is_sound;

                    // 4. 长度比值异常：短原文被翻译成长译文（如 "♪♪" → "Titus：哦，该死！\n♪♪"），
                    // 通常是批次错位翻译，不应缓存。
                    let orig_len = orig_text.chars().count().max(1);
                    let trans_len = restored.chars().count();
                    let length_ratio_abnormal = trans_len > 0 && {
                        let ratio = trans_len as f64 / orig_len as f64;
                        ratio > 5.0 && trans_len > 10
                    };

                    // 非英语内容检测：原文本身不含英语字母（如拼写字母 "G-O-R..."、
                    // 祖鲁语歌词 "Nants ingonyama bagithi baba!" 等），9b 保持原样是正确行为，
                    // 不应标记为 failed。检测条件：原文无连续 3 个以上英文字母组成的单词。
                    let is_non_english = !has_english_word(orig_text, 3);

                    // 音乐符号/音效标记保持原样是正确行为，不算翻译失败
                    let is_music_or_sfx = is_music_or_symbol_only(orig_text)
                        || looks_like_sound_effect(orig_text);

                    let failed = restored.is_empty()
                        || (!is_non_english && !is_music_or_sfx && restored.trim() == orig_text.trim())
                        || (target_lang.starts_with("zh")
                            && !has_cjk(&restored)
                            && !has_cjk(orig_text)
                            && !is_non_english
                            && !is_music_or_sfx)
                        || sound_mismatch
                        || batch_count_mismatch
                        || length_ratio_abnormal;

                    // 数量不匹配的批次整批不缓存（防止错位译文污染缓存）
                    // 例外：以下 failed 条目也写入缓存，保证 get_cached_entries 恢复时译文一致：
                    //   1. 译文=原文（如祖鲁语歌词等非英语内容，AI 保持原样）
                    //   2. 音效标记不一致（sound_mismatch，如含 {\an8} 定位标签的混合条目被译成纯音效标记）
                    // 翻译时 translate_entries_full 的 bad_cache 检查会跳过这些坏缓存重新翻译，
                    // 但恢复时 get_cached_entries 返回它们（标记 failed），前端视为未翻译，用户可重新翻译。
                    let same_as_orig = restored.trim() == orig_text.trim();
                    if !failed && !batch_count_mismatch {
                        // 缓存 key 用原始文本（与查询时一致），而非占位符保护后的文本
                        let cache_key = translate_cache_key(
                            orig_text,
                            source_lang,
                            target_lang,
                            &self.provider_name,
                            &self.file_hash,
                        );
                        let _ = self.db.set_translate_cache(
                            &cache_key,
                            orig_text,
                            &restored,
                            source_lang,
                            target_lang,
                            &self.provider_name,
                        );
                    } else if failed && !batch_count_mismatch && !restored.is_empty()
                        && (same_as_orig || sound_mismatch)
                    {
                        // 译文=原文或音效标记不一致的 failed 条目写入缓存，保证恢复时译文一致
                        let cache_key = translate_cache_key(
                            orig_text,
                            source_lang,
                            target_lang,
                            &self.provider_name,
                            &self.file_hash,
                        );
                        let _ = self.db.set_translate_cache(
                            &cache_key,
                            orig_text,
                            &restored,
                            source_lang,
                            target_lang,
                            &self.provider_name,
                        );
                    }

                    let te = TranslateEntry {
                        index: *index,
                        original: orig_text.clone(),
                        translated: restored,
                        from_cache: false,
                        failed,
                    };
                    if let Some(ref cb) = on_entry_done {
                        cb(&te);
                    }
                    results.push(te);
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
            token_usage: self.provider.token_usage(),
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
            &self.cancel_counter,
            self.my_gen,
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
    cancel_counter: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    my_gen: u64,
) -> Result<Vec<String>, AppError> {
        let mut last_error: Option<AppError> = None;
        let delays = [1u64, 2, 4];

        for (attempt, delay) in delays.iter().enumerate() {
            if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
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

/// 批次降级翻译：对齐失败时自动缩小批次重试（迭代实现，无递归）
/// 顺序：30 -> 10 -> 5 -> 3 -> 1
/// 每一级只重试仍然失败的条目，已成功的不重试
/// 返回 Vec<String>，失败的条目用空字符串占位（长度始终等于输入）
async fn translate_batch_with_fallback(
    provider: &dyn TranslateProviderTrait,
    texts: &[String],
    source_lang: &str,
    target_lang: &str,
    cancel_counter: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    my_gen: u64,
) -> Vec<String> {
    const BATCH_SIZES: [usize; 5] = [30, 10, 5, 3, 1];

    // 结果数组，初始全空；pending 记录尚未翻译成功的索引
    let mut results: Vec<String> = vec![String::new(); texts.len()];
    let mut pending: Vec<usize> = (0..texts.len()).collect();

    for &batch_size in &BATCH_SIZES {
        if pending.is_empty() {
            break;
        }
        if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
            break;
        }

        let mut still_pending: Vec<usize> = Vec::new();

        for chunk_indices in pending.chunks(batch_size) {
            if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                break;
            }
            let chunk_texts: Vec<String> =
                chunk_indices.iter().map(|&i| texts[i].clone()).collect();
            match translate_with_retry_provider(
                provider,
                &chunk_texts,
                source_lang,
                target_lang,
                cancel_counter,
                my_gen,
            )
            .await
            {
                Ok(translations) if translations.len() == chunk_indices.len() => {
                    // 完全对齐：填入结果，但检查空译文和错位译文
                    // 错位检测：9b 模型在批次翻译时可能拆分/合并条目，
                    // 导致翻译内容错位到相邻条目。检测到异常的条目不填入 results，
                    // 而是放入 still_pending 进入下一级降级重试（更小批次/单条）。
                    let mut shift_detected = false;
                    for (&idx, t) in chunk_indices.iter().zip(translations.iter()) {
                        if t.is_empty() {
                            continue;
                        }
                        let orig_text = &texts[idx];
                        let orig_len = orig_text.chars().count().max(1);
                        let trans_len = t.chars().count();
                        // 跳过音效标记和音乐符号（长度比值无意义）
                        let is_sound = looks_like_sound_effect(orig_text);
                        let is_music = is_music_or_symbol_only(orig_text);
                        if !is_sound && !is_music && trans_len > 0 {
                            let ratio = trans_len as f64 / orig_len as f64;
                            // 错位检测阈值根据原文长度动态调整：
                            // - 长原文（≥30字符）：ratio < 0.25 表示严重偏短（如两行只翻译了第一行）
                            //   0.25 能捕获 "I had the craziest night\n..." (55字符) → "这棵树的汁液绝对值得。" (11字符) ratio=0.20
                            // - 短原文（<30字符）：中英翻译压缩比可达 0.1（如 "Congratulations!" → "恭喜！"），
                            //   用固定字符数差值检测更可靠：译文比原文少 20 字符以上才判为错位
                            // - 长度比值 > 5.0 且 trans_len > 10：译文严重偏长（如把相邻条目合并了）
                            // - 多行原文（含\n）但译文不含换行：可能只翻译了第一行，降级重试
                            let short_ratio_threshold = if orig_len >= 30 { 0.25 } else { 0.0 };
                            let short_char_diff = if orig_len < 30 {
                                (orig_len as i64 - trans_len as i64) > 20
                            } else {
                                false
                            };
                            let multiline_split = orig_text.contains('\n')
                                && !t.contains('\n')
                                && orig_len >= 20
                                && trans_len > 0
                                && ratio < 0.5;
                            if ratio < short_ratio_threshold
                                || short_char_diff
                                || multiline_split
                                || (ratio > 5.0 && trans_len > 10)
                            {
                                tracing::warn!(
                                    "批次错位检测：idx={} 原文{}字符→译文{}字符 ratio={:.2}，标记为待重试",
                                    idx, orig_len, trans_len, ratio
                                );
                                shift_detected = true;
                                continue; // 不填入 results，进入 still_pending
                            }
                        }
                        results[idx] = t.clone();
                    }
                    if shift_detected && batch_size > 1 {
                        tracing::warn!(
                            "批次大小 {} 检测到错位，将异常条目降级到更小批次重试",
                            batch_size
                        );
                    }
                    for &idx in chunk_indices {
                        if results[idx].is_empty() {
                            still_pending.push(idx);
                        }
                    }
                }
                Ok(translations) => {
                    // 数量不匹配：把非空的填入，空的留到下一级
                    tracing::warn!(
                        "批次降级：大小 {} 返回数量不匹配（期望 {}，实际 {}），继续降级",
                        batch_size,
                        chunk_indices.len(),
                        translations.len()
                    );
                    for (&idx, t) in chunk_indices.iter().zip(translations.iter()) {
                        if !t.is_empty() {
                            results[idx] = t.clone();
                        }
                    }
                    for &idx in chunk_indices {
                        if results[idx].is_empty() {
                            still_pending.push(idx);
                        }
                    }
                }
                Err(e) => {
                    // 整批失败，全部留到下一级
                    tracing::warn!(
                        "批次降级：大小 {} 翻译失败（{}），继续降级",
                        batch_size,
                        e
                    );
                    still_pending.extend_from_slice(chunk_indices);
                }
            }
        }

        pending = still_pending;
    }

    results
}


// === 新增传统翻译 Provider ===

/// DeepL 翻译 API
/// 文档：https://developers.deepl.com/docs/api-reference/translate/openapi-spec-for-text-translate
/// 认证：Authorization: DeepL-Auth-Key xxx
/// Free 版用 https://api-free.deepl.com，Pro 版用 https://api.deepl.com
pub struct DeepLProvider {
    auth_key: String,
    client: reqwest::Client,
}

impl DeepLProvider {
    pub fn new(auth_key: String) -> Self {
        Self::with_client(auth_key, reqwest::Client::new())
    }
    pub fn with_client(auth_key: String, client: reqwest::Client) -> Self {
        Self { auth_key, client }
    }

    /// 根据 Auth Key 自动选择 Free / Pro 端点
    /// Free key 以 ":fx" 结尾
    fn api_url(&self) -> &str {
        if self.auth_key.ends_with(":fx") {
            "https://api-free.deepl.com/v2/translate"
        } else {
            "https://api.deepl.com/v2/translate"
        }
    }

    /// DeepL 使用大写语言码（EN, ZH, JA），且部分语言有特殊映射
    fn to_deepl_lang(lang: &str) -> String {
        match lang.to_uppercase().as_str() {
            "AUTO" => "".to_string(),
            "EN" => "EN".to_string(),
            "ZH" => "ZH".to_string(),
            "JA" => "JA".to_string(),
            "KO" => "KO".to_string(),
            "FR" => "FR".to_string(),
            "DE" => "DE".to_string(),
            "ES" => "ES".to_string(),
            "PT" => "PT".to_string(),
            "IT" => "IT".to_string(),
            "RU" => "RU".to_string(),
            "NL" => "NL".to_string(),
            "PL" => "PL".to_string(),
            other => other.to_string(),
        }
    }

    async fn translate_single_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        let url = self.api_url();
        let target = Self::to_deepl_lang(target_lang);
        if target.is_empty() {
            return Err(AppError::TranslateNetworkError {
                provider: "deepl".to_string(),
                detail: "target_lang is empty".to_string(),
            });
        }

        // DeepL 接受 text=xxx 多次传递（form-encoded）
        let mut form = vec![("target_lang".to_string(), target.clone())];
        let src = Self::to_deepl_lang(source_lang);
        if !src.is_empty() {
            form.push(("source_lang".to_string(), src));
        }
        for t in texts {
            form.push(("text".to_string(), t.as_str().to_string()));
        }

        let resp = self
            .client
            .post(url)
            .header("Authorization", format!("DeepL-Auth-Key {}", self.auth_key))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&form)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "deepl".to_string(),
                detail: e.to_string(),
            })?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::TranslateRateLimit {
                provider: "deepl".to_string(),
                retry_after: Some(60),
            });
        }

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "deepl".to_string(),
                detail,
            });
        }

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::TranslateAuthFailed {
                provider: "deepl".to_string(),
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "deepl".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        let translations = result["translations"]
            .as_array()
            .ok_or_else(|| AppError::TranslateAlignFailed {
                missing: texts.len(),
            })?;

        let results: Vec<String> = translations
            .iter()
            .map(|item| {
                item.get("text")
                    .and_then(|t| t.as_str())
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
impl TranslateProviderTrait for DeepLProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        const DEEPL_MAX_TEXTS: usize = 50; // DeepL 一次最多 50 条文本
        let mut results = vec![String::new(); texts.len()];
        let non_empty: Vec<(usize, &String)> = texts
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();

        if non_empty.is_empty() {
            return Ok(results);
        }

        for chunk in non_empty.chunks(DEEPL_MAX_TEXTS) {
            let refs: Vec<&String> = chunk.iter().map(|(_, t)| *t).collect();
            let translated = self
                .translate_single_batch(&refs, source_lang, target_lang)
                .await?;
            for (i, tr) in translated.into_iter().enumerate() {
                results[chunk[i].0] = tr;
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
            LanguageInfo { code: "pt".into(), name: "Portuguese".into(), native_name: "Português".into() },
            LanguageInfo { code: "it".into(), name: "Italian".into(), native_name: "Italiano".into() },
            LanguageInfo { code: "ru".into(), name: "Russian".into(), native_name: "Русский".into() },
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 7 END ===

/// 有道翻译 API
/// 文档：https://ai.youdao.com/DOCSIRMA/html/trans/api/wbfy/index.html
/// 签名算法：SHA256(appKey + q + salt + curtime + appSecret)
pub struct YoudaoProvider {
    app_key: String,
    app_secret: String,
    client: reqwest::Client,
}

impl YoudaoProvider {
    pub fn new(app_key: String, app_secret: String) -> Self {
        Self::with_client(app_key, app_secret, reqwest::Client::new())
    }
    pub fn with_client(app_key: String, app_secret: String, client: reqwest::Client) -> Self {
        Self { app_key, app_secret, client }
    }

    fn sign(&self, query: &str, salt: &str, curtime: &str) -> String {
        use sha2::{Digest, Sha256};
        let input = format!("{}{}{}{}{}", self.app_key, query, salt, curtime, self.app_secret);
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// 有道语言码映射
    fn to_youdao_lang(lang: &str) -> &str {
        match lang {
            "auto" => "auto",
            "zh" => "zh-CHS",
            "en" => "en",
            "ja" => "ja",
            "ko" => "ko",
            "fr" => "fr",
            "de" => "de",
            "es" => "es",
            "pt" => "pt",
            "it" => "it",
            "ru" => "ru",
            "vi" => "vi",
            "th" => "th",
            "ar" => "ar",
            other => other,
        }
    }

    async fn translate_single(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, AppError> {
        let salt = uuid::Uuid::new_v4().simple().to_string();
        let curtime = chrono::Utc::now().timestamp().to_string();
        let sign = self.sign(text, &salt, &curtime);

        let from = Self::to_youdao_lang(source_lang);
        let to = Self::to_youdao_lang(target_lang);

        let params = [
            ("q", text.to_string()),
            ("from", from.to_string()),
            ("to", to.to_string()),
            ("appKey", self.app_key.clone()),
            ("salt", salt.clone()),
            ("sign", sign),
            ("signType", "v3".to_string()),
            ("curtime", curtime),
        ];

        let resp = self
            .client
            .post("https://openapi.youdao.com/api")
            .form(&params)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "youdao".to_string(),
                detail: e.to_string(),
            })?;

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "youdao".to_string(),
                detail,
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "youdao".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        // 检查错误码
        if let Some(error_code) = result.get("errorCode").and_then(|c| c.as_str()) {
            if error_code != "0" {
                let full_msg = format!("errorCode: {}", error_code);
                if let Some(detail) = check_insufficient_balance(reqwest::StatusCode::OK, &full_msg) {
                    return Err(AppError::TranslateInsufficientBalance {
                        provider: "youdao".to_string(),
                        detail,
                    });
                }
                return Err(AppError::TranslateNetworkError {
                    provider: "youdao".to_string(),
                    detail: full_msg,
                });
            }
        }

        let translation = result["translation"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|t| t.as_str())
            .unwrap_or("");

        Ok(translation.to_string())
    }
}

#[async_trait::async_trait]
impl TranslateProviderTrait for YoudaoProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // 有道不支持批量，逐条翻译
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            if text.trim().is_empty() {
                results.push(String::new());
                continue;
            }
            let tr = self.translate_single(text, source_lang, target_lang).await?;
            results.push(tr);
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
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 8 END ===

/// 彩云小译 API
/// 文档：https://docs.caiyunapp.com/docs/tables/overall
/// 认证：token 方式，JSON body 请求
pub struct CaiyunProvider {
    token: String,
    client: reqwest::Client,
}

impl CaiyunProvider {
    pub fn new(token: String) -> Self {
        Self::with_client(token, reqwest::Client::new())
    }
    pub fn with_client(token: String, client: reqwest::Client) -> Self {
        Self { token, client }
    }

    /// 彩云语言对映射：source_lang→target_lang 格式
    fn trans_type(source: &str, target: &str) -> Result<String, AppError> {
        let pair = match (source, target) {
            ("auto", "zh") => "auto2zh",
            ("auto", "en") => "auto2en",
            ("zh", "en") => "zh2en",
            ("en", "zh") => "en2zh",
            ("zh", "ja") => "zh2ja",
            ("ja", "zh") => "ja2zh",
            ("zh", "ko") => "zh2ko",
            ("ko", "zh") => "ko2zh",
            ("zh", "fr") => "zh2fr",
            ("fr", "zh") => "fr2zh",
            ("zh", "de") => "zh2de",
            ("de", "zh") => "de2zh",
            ("zh", "es") => "zh2es",
            ("es", "zh") => "es2zh",
            ("zh", "it") => "zh2it",
            ("it", "zh") => "it2zh",
            ("zh", "ru") => "zh2ru",
            ("ru", "zh") => "ru2zh",
            ("en", "ja") => "en2ja",
            ("ja", "en") => "ja2en",
            ("en", "ko") => "en2ko",
            ("ko", "en") => "ko2en",
            _ => return Err(AppError::TranslateNetworkError {
                provider: "caiyun".to_string(),
                detail: format!("unsupported language pair: {}→{}", source, target),
            }),
        };
        Ok(pair.to_string())
    }

    async fn translate_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        let trans_type = Self::trans_type(source_lang, target_lang)?;
        let request_id = uuid::Uuid::new_v4().to_string();
        let text_list: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();

        let body = serde_json::json!({
            "source": text_list,
            "trans_type": trans_type,
            "request_id": request_id,
            "detect": true,
        });

        let url = "https://api.interpreter.caiyunai.com/v1/translator";
        let resp = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-Authorization", format!("token {}", self.token))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "caiyun".to_string(),
                detail: e.to_string(),
            })?;

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "caiyun".to_string(),
                detail,
            });
        }

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::TranslateAuthFailed {
                provider: "caiyun".to_string(),
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "caiyun".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        let target = result["target"]
            .as_array()
            .ok_or_else(|| AppError::TranslateAlignFailed {
                missing: texts.len(),
            })?;

        let results: Vec<String> = target
            .iter()
            .map(|t| t.as_str().unwrap_or("").to_string())
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
impl TranslateProviderTrait for CaiyunProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        const CAIYUN_MAX_TEXTS: usize = 20;
        let mut results = vec![String::new(); texts.len()];
        let non_empty: Vec<(usize, &String)> = texts
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();

        if non_empty.is_empty() {
            return Ok(results);
        }

        for chunk in non_empty.chunks(CAIYUN_MAX_TEXTS) {
            let refs: Vec<&String> = chunk.iter().map(|(_, t)| *t).collect();
            let translated = self
                .translate_batch(&refs, source_lang, target_lang)
                .await?;
            for (i, tr) in translated.into_iter().enumerate() {
                results[chunk[i].0] = tr;
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
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 9 END ===

/// 小牛翻译 API
/// 文档：https://niutrans.com/Document
/// 认证：apikey 参数
pub struct NiutransProvider {
    api_key: String,
    client: reqwest::Client,
}

impl NiutransProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_client(api_key, reqwest::Client::new())
    }
    pub fn with_client(api_key: String, client: reqwest::Client) -> Self {
        Self { api_key, client }
    }

    /// 小牛翻译语言码映射
    fn to_niutrans_lang(lang: &str) -> &str {
        match lang {
            "auto" => "auto",
            "zh" => "zh",
            "en" => "en",
            "ja" => "ja",
            "ko" => "ko",
            "fr" => "fr",
            "de" => "de",
            "es" => "es",
            "pt" => "pt",
            "it" => "it",
            "ru" => "ru",
            "vi" => "vi",
            "th" => "th",
            "ar" => "ar",
            other => other,
        }
    }

    async fn translate_single(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, AppError> {
        let from = Self::to_niutrans_lang(source_lang);
        let to = Self::to_niutrans_lang(target_lang);

        let body = serde_json::json!({
            "apikey": self.api_key,
            "src_text": text,
            "from": from,
            "to": to,
        });

        let resp = self
            .client
            .post("https://nmt-api.niutrans.com/NMT/translate")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "niutrans".to_string(),
                detail: e.to_string(),
            })?;

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "niutrans".to_string(),
                detail,
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "niutrans".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        // 检查错误码
        if let Some(code) = result.get("error_code").and_then(|c| c.as_i64()) {
            if code != 0 {
                let msg = result.get("error_msg").and_then(|m| m.as_str()).unwrap_or("unknown");
                let full_msg = format!("error_code: {}, error_msg: {}", code, msg);
                if let Some(detail) = check_insufficient_balance(reqwest::StatusCode::OK, &full_msg) {
                    return Err(AppError::TranslateInsufficientBalance {
                        provider: "niutrans".to_string(),
                        detail,
                    });
                }
                return Err(AppError::TranslateNetworkError {
                    provider: "niutrans".to_string(),
                    detail: full_msg,
                });
            }
        }

        let tgt = result["tgt_text"]
            .as_str()
            .unwrap_or("");

        Ok(tgt.to_string())
    }
}

#[async_trait::async_trait]
impl TranslateProviderTrait for NiutransProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // 小牛翻译不支持批量，逐条翻译
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            if text.trim().is_empty() {
                results.push(String::new());
                continue;
            }
            let tr = self.translate_single(text, source_lang, target_lang).await?;
            results.push(tr);
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
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 10 END ===

/// 腾讯翻译君 API（TMT）
/// 文档：https://cloud.tencent.com/document/product/555
/// 认证：HMAC-SHA256 签名（TC3-HMAC-SHA256）
pub struct TencentProvider {
    secret_id: String,
    secret_key: String,
    client: reqwest::Client,
}

impl TencentProvider {
    pub fn new(secret_id: String, secret_key: String) -> Self {
        Self::with_client(secret_id, secret_key, reqwest::Client::new())
    }
    pub fn with_client(secret_id: String, secret_key: String, client: reqwest::Client) -> Self {
        Self { secret_id, secret_key, client }
    }

    /// TC3-HMAC-SHA256 签名
    fn sign(&self, payload: &str, timestamp: i64, date: &str) -> String {
        use sha2::{Digest, Sha256};
        use hmac::{Hmac, Mac};

        type HmacSha256 = Hmac<Sha256>;

        // 1. 拼接规范请求串
        let canonical_uri = "/";
        let canonical_querystring = "";
        let canonical_headers = format!(
            "content-type:application/json; charset=utf-8\nhost:tmt.tencentcloudapi.com\nx-tc-action:texttranslate\n"
        );
        let signed_headers = "content-type;host;x-tc-action";
        let hashed_payload = hex::encode(Sha256::digest(payload.as_bytes()));
        let canonical_request = format!(
            "POST\n{}\n{}\n{}\n{}\n{}",
            canonical_uri, canonical_querystring, canonical_headers, signed_headers, hashed_payload
        );

        // 2. 拼接待签名字符串
        let algorithm = "TC3-HMAC-SHA256";
        let credential_scope = format!("{}/tmt/tc3_request", date);
        let hashed_canonical_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let string_to_sign = format!(
            "{}\n{}\n{}\n{}",
            algorithm, timestamp, credential_scope, hashed_canonical_request
        );

        // 3. 计算签名
        let secret_date = {
            let mut mac = HmacSha256::new_from_slice(format!("TC3{}", self.secret_key).as_bytes()).unwrap();
            mac.update(date.as_bytes());
            mac.finalize().into_bytes()
        };
        let secret_service = {
            let mut mac = HmacSha256::new_from_slice(&secret_date).unwrap();
            mac.update(b"tmt");
            mac.finalize().into_bytes()
        };
        let secret_signing = {
            let mut mac = HmacSha256::new_from_slice(&secret_service).unwrap();
            mac.update(b"tc3_request");
            mac.finalize().into_bytes()
        };
        let signature = {
            let mut mac = HmacSha256::new_from_slice(&secret_signing).unwrap();
            mac.update(string_to_sign.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };

        // 4. 拼接 Authorization
        format!(
            "{} Credential={}/{}, SignedHeaders={}, Signature={}",
            algorithm, self.secret_id, credential_scope, signed_headers, signature
        )
    }

    async fn translate_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // 腾讯 TMT TextTranslate 一次只翻译一条文本
        // 批量翻译用 TextTranslateBatch
        let text_list: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();
        let payload = serde_json::json!({
            "SourceTextList": text_list,
            "Source": source_lang,
            "Target": target_lang,
            "ProjectId": 0,
        }).to_string();

        let timestamp = chrono::Utc::now().timestamp();
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let authorization = self.sign(&payload, timestamp, &date);

        let resp = self
            .client
            .post("https://tmt.tencentcloudapi.com/")
            .header("Authorization", &authorization)
            .header("Content-Type", "application/json; charset=utf-8")
            .header("Host", "tmt.tencentcloudapi.com")
            .header("X-TC-Action", "TextTranslateBatch")
            .header("X-TC-Version", "2018-03-21")
            .header("X-TC-Timestamp", timestamp.to_string())
            .body(payload)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "tencent".to_string(),
                detail: e.to_string(),
            })?;

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "tencent".to_string(),
                detail,
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "tencent".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        // 检查错误
        if let Some(err) = result.get("Response").and_then(|r| r.get("Error")) {
            let code = err.get("Code").and_then(|c| c.as_str()).unwrap_or("unknown");
            let msg = err.get("Message").and_then(|m| m.as_str()).unwrap_or("");
            let full_msg = format!("{}: {}", code, msg);
            if let Some(detail) = check_insufficient_balance(reqwest::StatusCode::OK, &full_msg) {
                return Err(AppError::TranslateInsufficientBalance {
                    provider: "tencent".to_string(),
                    detail,
                });
            }
            return Err(AppError::TranslateNetworkError {
                provider: "tencent".to_string(),
                detail: full_msg,
            });
        }

        let target_list = result["Response"]["TargetTextList"]
            .as_array()
            .ok_or_else(|| AppError::TranslateAlignFailed {
                missing: texts.len(),
            })?;

        let results: Vec<String> = target_list
            .iter()
            .map(|t| t.as_str().unwrap_or("").to_string())
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
impl TranslateProviderTrait for TencentProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        const TENCENT_MAX_BATCH: usize = 25; // 腾讯批量翻译上限
        let mut results = vec![String::new(); texts.len()];
        let non_empty: Vec<(usize, &String)> = texts
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();

        if non_empty.is_empty() {
            return Ok(results);
        }

        for chunk in non_empty.chunks(TENCENT_MAX_BATCH) {
            let refs: Vec<&String> = chunk.iter().map(|(_, t)| *t).collect();
            let translated = self
                .translate_batch(&refs, source_lang, target_lang)
                .await?;
            for (i, tr) in translated.into_iter().enumerate() {
                results[chunk[i].0] = tr;
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
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 11 END ===

/// 火山翻译 API（火山引擎机器翻译）
/// 文档：https://www.volcengine.com/docs/4640
/// 认证：HMAC-SHA256 签名（火山引擎 OpenAPI 签名方式）
pub struct VolcengineProvider {
    access_key: String,
    secret_key: String,
    client: reqwest::Client,
}

impl VolcengineProvider {
    pub fn new(access_key: String, secret_key: String) -> Self {
        Self::with_client(access_key, secret_key, reqwest::Client::new())
    }
    pub fn with_client(access_key: String, secret_key: String, client: reqwest::Client) -> Self {
        Self { access_key, secret_key, client }
    }

    /// 火山引擎 V4 签名
    fn sign(&self, payload: &str, timestamp: &str, date: &str) -> (String, String) {
        use sha2::{Digest, Sha256};
        use hmac::{Hmac, Mac};

        type HmacSha256 = Hmac<Sha256>;

        let service = "translate";
        let region = "cn-north-1";
        let host = "open.volcengineapi.com";

        // 1. 规范请求
        let canonical_uri = "/";
        let canonical_query = "Action=TranslateText&Version=2020-06-01";
        let canonical_headers = format!(
            "content-type:application/json; charset=utf-8\nhost:{}\nx-date:{}\n",
            host, timestamp
        );
        let signed_headers = "content-type;host;x-date";
        let hashed_payload = hex::encode(Sha256::digest(payload.as_bytes()));
        let canonical_request = format!(
            "POST\n{}\n{}\n{}\n{}\n{}",
            canonical_uri, canonical_query, canonical_headers, signed_headers, hashed_payload
        );

        // 2. 待签名字符串
        let credential_scope = format!("{}/{}/{}/request", date, region, service);
        let hashed_canonical_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let string_to_sign = format!(
            "HMAC-SHA256\n{}\n{}\n{}",
            timestamp, credential_scope, hashed_canonical_request
        );

        // 3. 计算签名
        let k_date = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes()).unwrap();
            mac.update(date.as_bytes());
            mac.finalize().into_bytes()
        };
        let k_region = {
            let mut mac = HmacSha256::new_from_slice(&k_date).unwrap();
            mac.update(region.as_bytes());
            mac.finalize().into_bytes()
        };
        let k_service = {
            let mut mac = HmacSha256::new_from_slice(&k_region).unwrap();
            mac.update(service.as_bytes());
            mac.finalize().into_bytes()
        };
        let k_signing = {
            let mut mac = HmacSha256::new_from_slice(&k_service).unwrap();
            mac.update(b"request");
            mac.finalize().into_bytes()
        };
        let signature = {
            let mut mac = HmacSha256::new_from_slice(&k_signing).unwrap();
            mac.update(string_to_sign.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };

        // 4. Authorization
        let auth = format!(
            "HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key, credential_scope, signed_headers, signature
        );

        (auth, host.to_string())
    }

    async fn translate_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        let text_list: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();
        let payload = serde_json::json!({
            "TargetLanguage": target_lang,
            "TextList": text_list,
        }).to_string();
        if !source_lang.is_empty() && source_lang != "auto" {
            // 火山引擎支持 SourceLanguage 可选字段
        }

        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let date = chrono::Utc::now().format("%Y%m%d").to_string();
        let (authorization, host) = self.sign(&payload, &timestamp, &date);

        let resp = self
            .client
            .post("https://open.volcengineapi.com/?Action=TranslateText&Version=2020-06-01")
            .header("Authorization", &authorization)
            .header("Content-Type", "application/json; charset=utf-8")
            .header("Host", &host)
            .header("X-Date", &timestamp)
            .body(payload)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "volcengine".to_string(),
                detail: e.to_string(),
            })?;

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "volcengine".to_string(),
                detail,
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "volcengine".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        // 检查错误
        if let Some(err) = result.get("ResponseMetadata").and_then(|r| r.get("Error")) {
            let code = err.get("Code").and_then(|c| c.as_str()).unwrap_or("unknown");
            let msg = err.get("Message").and_then(|m| m.as_str()).unwrap_or("");
            let full_msg = format!("{}: {}", code, msg);
            if let Some(detail) = check_insufficient_balance(reqwest::StatusCode::OK, &full_msg) {
                return Err(AppError::TranslateInsufficientBalance {
                    provider: "volcengine".to_string(),
                    detail,
                });
            }
            return Err(AppError::TranslateNetworkError {
                provider: "volcengine".to_string(),
                detail: full_msg,
            });
        }

        let translation_list = result["TranslationList"]
            .as_array()
            .ok_or_else(|| AppError::TranslateAlignFailed {
                missing: texts.len(),
            })?;

        let results: Vec<String> = translation_list
            .iter()
            .map(|item| {
                item.get("Translation")
                    .and_then(|t| t.as_str())
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
impl TranslateProviderTrait for VolcengineProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        const VOLC_MAX_BATCH: usize = 20;
        let mut results = vec![String::new(); texts.len()];
        let non_empty: Vec<(usize, &String)> = texts
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();

        if non_empty.is_empty() {
            return Ok(results);
        }

        for chunk in non_empty.chunks(VOLC_MAX_BATCH) {
            let refs: Vec<&String> = chunk.iter().map(|(_, t)| *t).collect();
            let translated = self
                .translate_batch(&refs, source_lang, target_lang)
                .await?;
            for (i, tr) in translated.into_iter().enumerate() {
                results[chunk[i].0] = tr;
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
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 12 END ===

/// 阿里翻译 API（阿里云机器翻译）
/// 文档：https://www.aliyun.com/product/ai/alimt
/// 认证：HMAC-SHA1 签名（阿里云 OpenAPI 签名方式）
pub struct AliyunProvider {
    access_key_id: String,
    access_key_secret: String,
    client: reqwest::Client,
}

impl AliyunProvider {
    pub fn new(access_key_id: String, access_key_secret: String) -> Self {
        Self::with_client(access_key_id, access_key_secret, reqwest::Client::new())
    }
    pub fn with_client(access_key_id: String, access_key_secret: String, client: reqwest::Client) -> Self {
        Self { access_key_id, access_key_secret, client }
    }

    /// 阿里云 RPC API 签名（HMAC-SHA1）
    fn sign(&self, params: &[(String, String)]) -> String {
        use sha1::Sha1;
        use hmac::{Hmac, Mac};

        type HmacSha1 = Hmac<Sha1>;

        // 1. 排序参数
        let mut sorted = params.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));

        // 2. 拼接 canonicalized query string
        let canonicalized: String = sorted
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    url_encode(k),
                    url_encode(v)
                )
            })
            .collect::<Vec<_>>()
            .join("&");

        // 3. 构造待签名字符串
        let string_to_sign = format!(
            "GET&%2F&{}",
            url_encode(&canonicalized)
        );

        // 4. HMAC-SHA1
        let key = format!("{}&", self.access_key_secret);
        let mut mac = HmacSha1::new_from_slice(key.as_bytes()).unwrap();
        mac.update(string_to_sign.as_bytes());
        let signature = mac.finalize().into_bytes();
        base64_url::encode(&signature)
    }

    async fn translate_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // 阿里云 通用文本翻译 GetTranslateRequestBatch 或 逐条 TranslateGeneral
        // 使用 TranslateGeneral 逐条翻译
        let mut results = Vec::with_capacity(texts.len());

        for text in texts {
            if text.trim().is_empty() {
                results.push(String::new());
                continue;
            }

            let nonce = uuid::Uuid::new_v4().to_string();
            let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

            let mut params: Vec<(String, String)> = vec![
                ("Format".into(), "JSON".into()),
                ("Version".into(), "2018-10-12".into()),
                ("AccessKeyId".into(), self.access_key_id.clone()),
                ("SignatureMethod".into(), "HMAC-SHA1".into()),
                ("Timestamp".into(), timestamp.clone()),
                ("SignatureVersion".into(), "1.0".into()),
                ("SignatureNonce".into(), nonce.clone()),
                ("Action".into(), "TranslateGeneral".into()),
                ("Scene".into(), "general".into()),
                ("SourceLanguage".into(), source_lang.into()),
                ("TargetLanguage".into(), target_lang.into()),
                ("SourceText".into(), text.as_str().to_string()),
                ("FormatType".into(), "text".into()),
            ];

            // 计算签名
            let signature = self.sign(&params);
            params.push(("Signature".into(), signature));

            // 构建 query string
            let query: String = params
                .iter()
                .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
                .collect::<Vec<_>>()
                .join("&");

            let url = format!("https://mt.cn-hangzhou.aliyuncs.com/?{}", query);

            let resp = self
                .client
                .get(&url)
                .send()
                .await
                .map_err(|e| AppError::TranslateNetworkError {
                    provider: "aliyun".to_string(),
                    detail: e.to_string(),
                })?;

            let status = resp.status();
            let response_body = resp.text().await.unwrap_or_default();

            if let Some(detail) = check_insufficient_balance(status, &response_body) {
                return Err(AppError::TranslateInsufficientBalance {
                    provider: "aliyun".to_string(),
                    detail,
                });
            }

            if !status.is_success() {
                return Err(AppError::TranslateNetworkError {
                    provider: "aliyun".to_string(),
                    detail: format!("HTTP {}: {}", status, response_body),
                });
            }

            let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
                AppError::TranslateResponseParseFailed {
                    detail: e.to_string(),
                }
            })?;

            // 检查错误
            if let Some(code) = result.get("Code").and_then(|c| c.as_str()) {
                if code != "200" {
                    let msg = result.get("Message").and_then(|m| m.as_str()).unwrap_or("");
                    let full_msg = format!("Code: {}, Message: {}", code, msg);
                    if let Some(detail) = check_insufficient_balance(reqwest::StatusCode::OK, &full_msg) {
                        return Err(AppError::TranslateInsufficientBalance {
                            provider: "aliyun".to_string(),
                            detail,
                        });
                    }
                    return Err(AppError::TranslateNetworkError {
                        provider: "aliyun".to_string(),
                        detail: full_msg,
                    });
                }
            }

            let translated = result["Data"]["Translated"]
                .as_str()
                .unwrap_or("");

            results.push(translated.to_string());
        }

        Ok(results)
    }
}

/// URL 编码（阿里云要求特殊编码规则）
fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b'*' => result.push_str("%2A"),
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

/// 简单 base64 URL-safe 编码
mod base64_url {
    pub fn encode(data: &[u8]) -> String {
        use std::fmt::Write;
        let mut result = String::new();
        for byte in data {
            write!(result, "{:02x}", byte).unwrap();
        }
        result
    }
}

#[async_trait::async_trait]
impl TranslateProviderTrait for AliyunProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // 阿里云 TranslateGeneral 逐条翻译
        self.translate_batch(
            &texts.iter().collect::<Vec<_>>(),
            source_lang,
            target_lang,
        )
        .await
    }

    async fn supported_target_langs(&self) -> Result<Vec<LanguageInfo>, AppError> {
        Ok(vec![
            LanguageInfo { code: "zh".into(), name: "Chinese".into(), native_name: "中文".into() },
            LanguageInfo { code: "en".into(), name: "English".into(), native_name: "English".into() },
            LanguageInfo { code: "ja".into(), name: "Japanese".into(), native_name: "日本語".into() },
            LanguageInfo { code: "ko".into(), name: "Korean".into(), native_name: "한국어".into() },
            LanguageInfo { code: "fr".into(), name: "French".into(), native_name: "Français".into() },
            LanguageInfo { code: "de".into(), name: "German".into(), native_name: "Deutsch".into() },
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 13 END ===

/// Amazon Translate API
/// 文档：https://docs.aws.amazon.com/translate/latest/dg/api-reference.html
/// 认证：AWS Signature v4
pub struct AmazonProvider {
    access_key: String,
    secret_key: String,
    region: String,
    client: reqwest::Client,
}

impl AmazonProvider {
    pub fn new(access_key: String, secret_key: String, region: String) -> Self {
        Self::with_client(access_key, secret_key, region, reqwest::Client::new())
    }
    pub fn with_client(access_key: String, secret_key: String, region: String, client: reqwest::Client) -> Self {
        Self { access_key, secret_key, region, client }
    }

    /// AWS SigV4 签名
    fn sign(&self, payload: &str, timestamp: &str, date: &str, host: &str) -> String {
        use sha2::{Digest, Sha256};
        use hmac::{Hmac, Mac};

        type HmacSha256 = Hmac<Sha256>;

        let service = "translate";
        let canonical_uri = "/";
        let canonical_query = "";
        let canonical_headers = format!(
            "content-type:application/x-amz-json-1.1\nhost:{}\nx-amz-date:{}\n",
            host, timestamp
        );
        let signed_headers = "content-type;host;x-amz-date";
        let hashed_payload = hex::encode(Sha256::digest(payload.as_bytes()));
        let canonical_request = format!(
            "POST\n{}\n{}\n{}\n{}\n{}",
            canonical_uri, canonical_query, canonical_headers, signed_headers, hashed_payload
        );

        let credential_scope = format!("{}/{}/{}/aws4_request", date, self.region, service);
        let hashed_canonical_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            timestamp, credential_scope, hashed_canonical_request
        );

        let k_date = {
            let mut mac = HmacSha256::new_from_slice(format!("AWS4{}", self.secret_key).as_bytes()).unwrap();
            mac.update(date.as_bytes());
            mac.finalize().into_bytes()
        };
        let k_region = {
            let mut mac = HmacSha256::new_from_slice(&k_date).unwrap();
            mac.update(self.region.as_bytes());
            mac.finalize().into_bytes()
        };
        let k_service = {
            let mut mac = HmacSha256::new_from_slice(&k_region).unwrap();
            mac.update(service.as_bytes());
            mac.finalize().into_bytes()
        };
        let k_signing = {
            let mut mac = HmacSha256::new_from_slice(&k_service).unwrap();
            mac.update(b"aws4_request");
            mac.finalize().into_bytes()
        };
        let signature = {
            let mut mac = HmacSha256::new_from_slice(&k_signing).unwrap();
            mac.update(string_to_sign.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };

        format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key, credential_scope, signed_headers, signature
        )
    }

    async fn translate_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // Amazon Translate: TranslateText 一次最多 10 条
        let text_list: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();
        let payload = serde_json::json!({
            "TextList": text_list,
            "SourceLanguageCode": if source_lang == "auto" { "en" } else { source_lang },
            "TargetLanguageCode": target_lang,
        }).to_string();

        let host = format!("translate.{}.amazonaws.com", self.region);
        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let date = chrono::Utc::now().format("%Y%m%d").to_string();
        let authorization = self.sign(&payload, &timestamp, &date, &host);

        let url = format!("https://{}", host);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", &authorization)
            .header("Content-Type", "application/x-amz-json-1.1")
            .header("X-Amz-Date", &timestamp)
            .header("X-Amz-Target", "AWSShineFrontendService_20170701.TranslateText")
            .body(payload)
            .send()
            .await
            .map_err(|e| AppError::TranslateNetworkError {
                provider: "amazon".to_string(),
                detail: e.to_string(),
            })?;

        let status = resp.status();
        let response_body = resp.text().await.unwrap_or_default();

        if let Some(detail) = check_insufficient_balance(status, &response_body) {
            return Err(AppError::TranslateInsufficientBalance {
                provider: "amazon".to_string(),
                detail,
            });
        }

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::TranslateAuthFailed {
                provider: "amazon".to_string(),
            });
        }

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: "amazon".to_string(),
                detail: format!("HTTP {}: {}", status, response_body),
            });
        }

        let result: serde_json::Value = serde_json::from_str(&response_body).map_err(|e| {
            AppError::TranslateResponseParseFailed {
                detail: e.to_string(),
            }
        })?;

        let translations = result["TranslationsList"]
            .as_array()
            .ok_or_else(|| AppError::TranslateAlignFailed {
                missing: texts.len(),
            })?;

        let results: Vec<String> = translations
            .iter()
            .map(|item| {
                item.get("TranslatedText")
                    .and_then(|t| t.as_str())
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
impl TranslateProviderTrait for AmazonProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        const AMAZON_MAX_BATCH: usize = 10;
        let mut results = vec![String::new(); texts.len()];
        let non_empty: Vec<(usize, &String)> = texts
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.trim().is_empty())
            .collect();

        if non_empty.is_empty() {
            return Ok(results);
        }

        for chunk in non_empty.chunks(AMAZON_MAX_BATCH) {
            let refs: Vec<&String> = chunk.iter().map(|(_, t)| *t).collect();
            let translated = self
                .translate_batch(&refs, source_lang, target_lang)
                .await?;
            for (i, tr) in translated.into_iter().enumerate() {
                results[chunk[i].0] = tr;
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
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        self.translate(&["test".to_string()], "en", "zh").await?;
        Ok(())
    }
}

// === SECTION 14 END ===

/// AI 服务 ID → 显示名映射（用于错误消息中显示真实服务商名）
pub fn ai_service_display_name(service_id: &str) -> &'static str {
    match service_id {
        "deepseek" => "DeepSeek",
        "zhipu" => "智谱GLM",
        "siliconflow" => "硅基流动",
        "groq" => "Groq",
        "qwen" => "通义千问",
        "doubao" => "豆包",
        "hunyuan" => "混元",
        "lingyi" => "零一万物",
        "kimi" => "Kimi",
        "openai" => "OpenAI",
        "azure_openai" => "Azure OpenAI",
        "gemini" => "Gemini",
        "ernie" => "文心一言",
        "ollama" => "Ollama",
        "lmstudio" => "LM Studio",
        "custom" => "自定义端点",
        _ => "OpenAI",
    }
}

/// 创建翻译 provider 实例
pub fn create_provider(
    provider: &TranslateProvider,
    credentials: &ProviderCredentials,
) -> Result<std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>, AppError> {
    create_provider_with_proxy(provider, credentials, &ProxyConfig::default(), None, &ProviderOptions::default())
}

/// AI 翻译附加选项（glossary / name_tagging），传统翻译忽略
#[derive(Debug, Clone, Default)]
pub struct ProviderOptions {
    /// 译名表：(EnglishName, ChineseTranslation)
    pub glossary: Vec<(String, String)>,
    /// 是否要求模型在译文中用 <name=En>Zh</name> 标记人名
    pub name_tagging: bool,
}

/// 创建翻译 provider 实例（带代理配置）
/// service_name: AI 服务的显示名（如 "DeepSeek"），用于错误消息；传统翻译传 None
/// options: AI 翻译附加选项（glossary / name_tagging），传统翻译忽略
pub fn create_provider_with_proxy(
    provider: &TranslateProvider,
    credentials: &ProviderCredentials,
    proxy: &ProxyConfig,
    service_name: Option<&str>,
    options: &ProviderOptions,
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
            let name = service_name.unwrap_or("OpenAI").to_string();
            Ok(std::sync::Arc::new(
                OpenAiProvider::with_client(base_url, model, model_type, api_key, client)
                    .with_service_name(name)
                    .with_glossary(options.glossary.clone())
                    .with_name_tagging(options.name_tagging),
            ))
        }
        TranslateProvider::DeepL => {
            let auth_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "deepl".to_string() }
            })?;
            Ok(std::sync::Arc::new(DeepLProvider::with_client(auth_key, client)))
        }
        TranslateProvider::Youdao => {
            let app_key = credentials.app_id.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "youdao".to_string() }
            })?;
            let app_secret = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "youdao".to_string() }
            })?;
            Ok(std::sync::Arc::new(YoudaoProvider::with_client(app_key, app_secret, client)))
        }
        TranslateProvider::Caiyun => {
            let token = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "caiyun".to_string() }
            })?;
            Ok(std::sync::Arc::new(CaiyunProvider::with_client(token, client)))
        }
        TranslateProvider::Niutrans => {
            let api_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "niutrans".to_string() }
            })?;
            Ok(std::sync::Arc::new(NiutransProvider::with_client(api_key, client)))
        }
        TranslateProvider::Tencent => {
            let secret_id = credentials.app_id.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "tencent".to_string() }
            })?;
            let secret_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "tencent".to_string() }
            })?;
            Ok(std::sync::Arc::new(TencentProvider::with_client(secret_id, secret_key, client)))
        }
        TranslateProvider::Volcengine => {
            let access_key = credentials.app_id.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "volcengine".to_string() }
            })?;
            let secret_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "volcengine".to_string() }
            })?;
            Ok(std::sync::Arc::new(VolcengineProvider::with_client(access_key, secret_key, client)))
        }
        TranslateProvider::Aliyun => {
            let access_key_id = credentials.app_id.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "aliyun".to_string() }
            })?;
            let access_key_secret = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "aliyun".to_string() }
            })?;
            Ok(std::sync::Arc::new(AliyunProvider::with_client(access_key_id, access_key_secret, client)))
        }
        TranslateProvider::Amazon => {
            let access_key = credentials.app_id.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "amazon".to_string() }
            })?;
            let secret_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "amazon".to_string() }
            })?;
            let region = credentials.region.clone().unwrap_or_else(|| "us-east-1".to_string());
            Ok(std::sync::Arc::new(AmazonProvider::with_client(access_key, secret_key, region, client)))
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

// === SECTION: 人名预扫描提取 ===

/// 人名提取结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedName {
    pub english: String,
    pub chinese: String,
    /// 所有候选译名（按频率降序排列，chinese 是频率最高的）
    pub alternatives: Vec<String>,
}

/// 人名提取的 segment 结果（含来源 segment 索引，用于频率统计）
#[derive(Debug, Clone)]
struct SegmentNameResult {
    segment_idx: usize,
    names: Vec<ExtractedName>,
}

/// 构建人名提取的 system prompt
fn build_name_extraction_system_prompt(source_lang: &str, target_lang: &str) -> String {
    let src = lang_full_name(source_lang);
    let tgt = lang_full_name(target_lang);
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

/// 构建人名提取的 user prompt（编号格式字幕文本）
fn build_name_extraction_user_prompt(texts: &[String]) -> String {
    texts
        .iter()
        .enumerate()
        .map(|(i, txt)| format!("{}. {}", i + 1, txt))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 解析人名提取响应，按 `EnglishName → ChineseTranslation` 格式逐行解析
/// 清理译名中的括号注释（如 `米奇 (Sheepdog's name)` → `米奇`）
/// 判断字符是否为中文（CJK 统一表意文字）
fn is_chinese_char(c: char) -> bool {
    matches!(c,
        '\u{4e00}'..='\u{9fff}'   // CJK 统一表意文字
        | '\u{3400}'..='\u{4dbf}'  // CJK 扩展 A
        | '\u{f900}'..='\u{faff}'  // CJK 兼容表意文字
    )
}

/// 判断字符串是否纯 ASCII（无中文字符）
fn is_pure_ascii(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii())
}

/// 从括号内提取中文翻译
/// 模型有时输出 `EnglishName → EnglishName（中文解释）` 格式
/// 此时括号前的部分是英文原名，中文在括号内
/// 例如：`The FarmDroid（农场机器人）` → `农场机器人`
///       `Mounjaro Ramp（莫努佳罗斜坡，可能指某种设备或地形特征）` → `莫努佳罗斜坡`
fn extract_chinese_from_parenthetical(zh_raw: &str) -> Option<String> {
    // 找到第一对括号
    let start_idx = zh_raw.find(|c: char| c == '(' || c == '（' || c == '[' || c == '【')?;
    let open_char = zh_raw.chars().nth(start_idx)?;
    let close_char = match open_char {
        '(' => ')',
        '（' => '）',
        '[' => ']',
        '【' => '】',
        _ => return None,
    };
    let after_open = &zh_raw[start_idx + open_char.len_utf8()..];
    let end_idx = after_open.find(close_char)?;
    let content = &after_open[..end_idx];

    // 按逗号分割取第一段（通常是翻译，后面是解释说明）
    let first_phrase = content
        .split(|c: char| c == '，' || c == ',' || c == '、')
        .next()?
        .trim();

    // 如果第一段包含中文，直接返回
    if first_phrase.chars().any(is_chinese_char) {
        return Some(first_phrase.to_string());
    }

    // 第一段无中文，检查整个括号内容是否有中文
    // 提取所有中文片段（过滤掉纯英文/标点部分）
    let chinese_only: String = content
        .chars()
        .filter(|c| is_chinese_char(*c) || *c == '·' || *c == '/' || *c == ' ')
        .collect();
    let trimmed = chinese_only.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }

    None
}

/// 判断提取的英文名是否可能是专有名词
/// 过滤掉明显的短语、句子（9b 模型常把短语当专有名词输出）
fn is_likely_proper_noun(english: &str) -> bool {
    let trimmed = english.trim();
    if trimmed.is_empty() { return false; }

    // 按空格分词（连字符词算一个词）
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let word_count = words.len();

    // 4+ 词几乎肯定是短语/句子，不是专有名词
    if word_count > 3 {
        return false;
    }

    // 常见功能词/虚词 - 多词条目含这些词时判定为短语
    // 注意：冠词 the/a/an 作为首词在专有名词中常见（The Who, The FarmDroid），
    // 只在非首词位置才算短语标志
    let articles: &[&str] = &["the", "a", "an"];
    let function_words: &[&str] = &[
        "in", "on", "at", "to", "for", "of", "with", "by",
        "is", "are", "was", "were", "will", "be", "been", "being",
        "this", "that", "these", "those", "next", "last",
        "and", "or", "but", "not",
        "his", "her", "their", "our", "my", "your",
        "up", "down", "out", "off", "back", "here", "there",
        "when", "where", "what", "how", "why",
        "if", "then", "so", "because",
        "all", "some", "any", "more", "most",
        "two", "three", "four", "five", "six",
        "days", "weeks", "months", "years", "day", "week", "month", "year",
        "time", "later", "ago", "ready", "stuck",
    ];

    // 多词条目：检查是否含功能词
    if word_count > 1 {
        for (i, word) in words.iter().enumerate() {
            // 去掉首尾非字母数字字符后再比较
            let lower: String = word
                .to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            // 冠词只在非首词位置才算短语标志
            if i == 0 && articles.contains(&lower.as_str()) {
                continue;
            }
            if function_words.contains(&lower.as_str()) || articles.contains(&lower.as_str()) {
                return false;
            }
        }
    }

    // 常见非专有名词黑名单（9b 模型常把普通名词当专有名词输出）
    // 注意：鸟种名（skylark/yellowhammer/bunting 等）保留为专有名词，
    //       因为字幕翻译中需要统一译名。
    // 包含：农作物、颜色、月份/星期、度量单位、天气、
    //       农业术语、状态描述、季节、通用动物名（非特定物种名）等
    let common_nouns: &[&str] = &[
        // 农作物/植物
        "oats", "oat", "wheat", "wheats", "barley", "mustard", "onion", "onions",
        "beetroot", "beetroots", "sugar", "grass", "hay", "silage", "seed", "seeds",
        "crop", "crops", "grain", "maize", "corn", "rice", "soybean", "potato",
        // 通用动物名（非特定物种名/品种名）
        "cow", "cows", "cattle", "calf", "calves", "bull", "bulls", "sheep",
        "horse", "horses", "pig", "pigs", "chicken", "chickens", "dog", "dogs",
        "cat", "cats", "bird", "birds",
        // 颜色
        "red", "green", "blue", "white", "black", "yellow", "orange", "purple",
        "brown", "grey", "gray", "pink",
        // 月份/星期
        "january", "february", "march", "april", "may", "june", "july", "august",
        "september", "october", "november", "december",
        "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
        // 度量单位
        "inch", "inches", "millimetre", "millimetres", "millimeter", "millimeters",
        "centimetre", "centimetres", "meter", "meters", "kilometre", "kilometres",
        "mile", "miles", "acre", "acres", "hectare", "hectares", "ton", "tons",
        "tonne", "tonnes", "kilo", "kilos", "kilogram", "kilograms", "gram", "grams",
        "litre", "litres", "gallon", "gallons",
        // 天气/自然
        "rain", "rains", "snow", "wind", "sun", "sunshine", "drought", "flood",
        "storm", "weather", "moisture", "water", "fire",
        // 农业术语
        "harvest", "field", "fields", "farm", "barn", "shed", "tractor", "machine",
        "combine", "plough", "mower", "seeder", "chart", "callipers", "square",
        // 状态/描述
        "pass", "fail", "marginal", "inconclusive", "ready", "stuck", "clear",
        "good", "bad", "sad", "happy", "angry", "worried", "fine", "okay",
        // 通用物品
        "food", "dinner", "lunch", "breakfast", "drink", "tea", "coffee",
        // 季节
        "spring", "summer", "autumn", "fall", "winter",
        // 其他常见误提
        "herd", "operation", "treatment", "test", "ministry", "twins",
        "lumps", "lump", "pneumonia", "cancer", "biopsy", "medical",
        "fraction", "twenty",
        // 日期/年份/数字
        "dates", "date", "2019", "2020", "2021", "2022", "2023", "2024", "2025",
        "twelve", "twelve-twelve",
        // 广告/通用商业词
        "advertisement", "advertisements", "ad", "ads",
        // 通用描述词/状态
        "aggressive", "bovine", "cardboard", "cleaning", "clip", "decluttering",
        "desert", "disorganised", "early", "heads", "light", "necks", "office",
        "palatable", "peaky", "pisser", "posted", "prepped", "pub", "shop",
        "restrictions", "slaughtered", "speedy", "recovery", "stationary", "storage",
        "stressed", "sugars", "whispering", "shrivelled",
        // 通用动作/状态短语
        "big", "plan", "grain", "carting", "grains", "combine", "man",
        "reproducing", "calves", "telehandler", "reactor",
        // 身体部位/发型
        "middle", "parting", "side",
        // 通用短语组件
        "enough", "tall", "stage", "official", "status", "free", "tb",
        "silent", "end", "credits", "tweezer", "things",
        "plus", "one", "two", "inches", "below",
        "fat", "jabs",
        "ever", "shrinking",
    ];

    // 单词条目：检查是否在常见名词黑名单中
    if word_count == 1 {
        let lower: String = words[0]
            .to_lowercase()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        if common_nouns.contains(&lower.as_str()) {
            return false;
        }
    } else {
        // 多词条目：如果所有词都在黑名单中，也不是专有名词
        let all_common = words.iter().all(|w| {
            let lower: String = w
                .to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            common_nouns.contains(&lower.as_str()) || articles.contains(&lower.as_str())
        });
        if all_common {
            return false;
        }
        // 多词条目：如果首词是常见形容词/描述词，很可能是短语而非专有名词
        let first_lower: String = words[0]
            .to_lowercase()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        let adjective_starts: &[&str] = &[
            "big", "small", "large", "tiny", "early", "late", "old", "new",
            "young", "tall", "short", "long", "wide", "narrow", "high", "low",
            "fat", "thin", "slim", "heavy", "light", "dark", "bright",
            "happy", "sad", "angry", "worried", "stressed", "disorganised",
            "aggressive", "palatable", "peaky", "stationary", "shrivelled",
            "silent", "official", "speedy", "ever", "middle", "side",
            "tweezer", "plus", "storage", "reproducing", "grain", "carting",
            "combine", "fat", "cleaning", "decluttering", "whispering",
            "hairy", "paving", "slab",
        ];
        if adjective_starts.contains(&first_lower.as_str()) {
            return false;
        }
    }

    true
}

pub fn parse_name_extraction_response(content: &str) -> Vec<ExtractedName> {
    use std::collections::HashSet;
    let mut names = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // 1. 优先尝试 JSON 解析
    let trimmed = content.trim();
    let json_content = strip_markdown_code_fence(trimmed);
    if json_content.starts_with('[') || json_content.starts_with("{") {
        if let Ok(json) = serde_json::from_str::<Vec<serde_json::Value>>(&json_content) {
            for item in json {
                if let (Some(en), Some(zh)) = (
                    item.get("en").and_then(|v| v.as_str()),
                    item.get("zh").and_then(|v| v.as_str()),
                ) {
                    let en = en.trim();
                    let zh = zh.trim();
                    if !en.is_empty() && !zh.is_empty() && seen.insert(en.to_string()) {
                        if !is_likely_proper_noun(en) {
                            tracing::debug!("人名过滤：'{}' 不像专有名词，跳过", en);
                            continue;
                        }
                        if zh.eq_ignore_ascii_case(en) || is_pure_ascii(zh) {
                            tracing::debug!("人名过滤：'{}' 译名 '{}' 无中文，跳过", en, zh);
                            continue;
                        }
                        // 按 `/` 拆分候选译名
                        let zh_candidates: Vec<String> = zh
                            .split('/')
                            .map(|s| s.trim().trim_matches('"').trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if !zh_candidates.is_empty() {
                            names.push(ExtractedName {
                                english: en.to_string(),
                                chinese: zh_candidates[0].clone(),
                                alternatives: zh_candidates.into_iter().skip(1).collect(),
                            });
                        }
                    }
                }
            }
            if !names.is_empty() {
                tracing::info!("人名提取：JSON 解析成功，提取 {} 个人名", names.len());
                return names;
            }
        }
    }

    // 2. 正则提取 {"en": "...", "zh": "..."} 对（对任意内容都尝试）
    let re_json = regex::Regex::new(
        r#""en"\s*:\s*"([^"]+)"\s*,\s*"zh"\s*:\s*"([^"]+)""#
    ).unwrap();
    for cap in re_json.captures_iter(content) {
        let en = cap[1].trim();
        let zh = cap[2].trim();
        if !en.is_empty() && !zh.is_empty() && seen.insert(en.to_string()) {
            if !is_likely_proper_noun(en) { continue; }
            if zh.eq_ignore_ascii_case(en) || is_pure_ascii(zh) { continue; }
            let zh_candidates: Vec<String> = zh
                .split('/')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !zh_candidates.is_empty() {
                names.push(ExtractedName {
                    english: en.to_string(),
                    chinese: zh_candidates[0].clone(),
                    alternatives: zh_candidates.into_iter().skip(1).collect(),
                });
            }
        }
    }
    if !names.is_empty() {
        tracing::info!("人名提取：JSON 正则提取成功，提取 {} 个人名", names.len());
        return names;
    }

    // 3. 回退到旧的 → 格式解析（兼容模型不输出 JSON 的情况）
    tracing::warn!("人名提取：JSON 解析失败，回退到 → 格式解析");

    // 9b nothink 模型的输出模式不确定：
    // - 有时先输出干净名单，再逐行分析（思考过程在后面）
    // - 有时先逐行分析，再输出干净名单（思考过程在前面）
    // 策略：扫描所有行，分出多段连续的有效名单行，取最长的一段。
    // "有效名单行"判定：含 → 或 -> 分隔符，且不含思考关键词。
    let thinking_keywords: &[&str] = &[
        "let's", "note:", "however,", "maybe", "i think", "i'd say",
        "strictly", "borderline", "usually", "might be", "referring to",
        "descriptive", "exclude", "include", "check", "re-eval", "refining",
        "final check", "correction", "re-check", "items to", "rule says",
        "exclude.", "generic term", "wait,", "so:", "yes.", "no.", "sure.",
        "exclude?", "line ", "extract", "proper noun", "common noun",
        "going through", "line by line",
        "but ", "but,", "also,", "furthermore", "in this", "in the",
        "for example", "for instance", "this means", "this is", "these are",
        "those are", "i will", "i'll", "i see", "perhaps", "probably",
        "likely", "often", "sometimes", "could be", "would be", "should be",
        "is it", "are they", "does it", "do they",
        "is often", "is usually", "is a generic", "is a common",
        "is a proper", "is a specific", "is a type",
        "treated as", "considered", "strictly speaking",
    ];

    let is_thinking_line = |lower: &str| -> bool {
        thinking_keywords.iter().any(|kw| lower.contains(kw))
    };

    let all_lines: Vec<&str> = content.lines().collect();
    // 分段：连续的有效名单行（允许空行间隔）为一段
    let mut blocks: Vec<(usize, usize)> = Vec::new(); // (start, end_exclusive)
    let mut block_start: Option<usize> = None;
    let mut last_valid: Option<usize> = None;

    for (i, line) in all_lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let has_sep = trimmed.contains('→') || trimmed.contains("->");
        let lower = trimmed.to_lowercase();
        let is_thinking = is_thinking_line(&lower);

        if has_sep && !is_thinking {
            // 有效名单行
            if block_start.is_none() {
                block_start = Some(i);
            }
            last_valid = Some(i);
        } else {
            // 非名单行或思考行：结束当前段
            if let (Some(start), Some(end)) = (block_start, last_valid) {
                blocks.push((start, end + 1));
                block_start = None;
                last_valid = None;
            }
        }
    }
    // 收集最后一段
    if let (Some(start), Some(end)) = (block_start, last_valid) {
        blocks.push((start, end + 1));
    }

    // 取最长的一段
    let effective_lines: Vec<&str> = if blocks.is_empty() {
        all_lines.clone()
    } else {
        let best = blocks.iter().max_by_key(|(s, e)| e - s).unwrap();
        all_lines[best.0..best.1].to_vec()
    };

    for line in effective_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        // 过滤含思考关键词的行（双重检查）
        let lower = trimmed.to_lowercase();
        if is_thinking_line(&lower) {
            tracing::debug!("人名过滤：行含思考关键词，跳过: {}", trimmed.chars().take(80).collect::<String>());
            continue;
        }
        // 支持 → 和 -> 两种分隔符
        let parts: Vec<&str> = if let Some(idx) = trimmed.find('→') {
            vec![&trimmed[..idx], &trimmed[idx + '→'.len_utf8()..]]
        } else if let Some(idx) = trimmed.find("->") {
            vec![&trimmed[..idx], &trimmed[idx + 2..]]
        } else {
            // 没有分隔符，跳过
            continue;
        };
        let en = parts[0].trim().trim_matches('"').trim();
        // 清理译名中的括号注释：`米奇 (Sheepdog's name)` → `米奇`
        // 支持中英文括号
        let zh_raw = parts[1].trim().trim_matches('"').trim();
        let zh_before_paren = zh_raw
            .split(|c| c == '(' || c == '（' || c == '[' || c == '【')
            .next()
            .unwrap_or(zh_raw)
            .trim()
            .trim_matches('"')
            .trim();
        // 判断是否为品牌名格式：`EnglishName → EnglishName（中文翻译）`
        // 只有括号前部分和英文名完全相同（不区分大小写）时才视为品牌名格式。
        // 不用 is_pure_ascii 判断，因为 `Endgame → Endgame（终结者）` 这种动物名
        // 括号前也是纯 ASCII，但它不是品牌名，应该只取括号内中文。
        // 真正区分品牌名的特征是：英文原名本身在译文中保留（如 AgBot、CornHub），
        // 而人名/动物名/作品名应该直接翻译为中文。
        // 启发式判断：英文名全大写或含数字（GS4、TB）→ 品牌名；否则 → 直接翻译
        let is_brand_name = en.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ')
            && en.chars().any(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        let is_brand_format = zh_before_paren.eq_ignore_ascii_case(en) && is_brand_name;
        let zh_candidates: Vec<String> = if is_brand_format {
            if let Some(chinese) = extract_chinese_from_parenthetical(zh_raw) {
                // 品牌名格式：按 `/` 拆分括号内的中文候选，每个都包装成 `英文（中文）`
                chinese
                    .split('/')
                    .map(|s| s.trim().trim_matches('"').trim().to_string())
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("{}（{}）", en, s))
                    .collect()
            } else {
                vec![zh_before_paren.to_string()]
            }
        } else if zh_before_paren.eq_ignore_ascii_case(en) {
            // 非品牌名但括号前=英文名（如 Endgame → Endgame（终结者））：只取括号内中文
            if let Some(chinese) = extract_chinese_from_parenthetical(zh_raw) {
                chinese
                    .split('/')
                    .map(|s| s.trim().trim_matches('"').trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                vec![zh_before_paren.to_string()]
            }
        } else {
            zh_before_paren
                .split('/')
                .map(|s| s.trim().trim_matches('"').trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        if !en.is_empty() && !zh_candidates.is_empty() && seen.insert(en.to_string()) {
            // 过滤掉明显的短语/句子（9b 模型常把短语当专有名词输出）
            if !is_likely_proper_noun(en) {
                tracing::debug!("人名过滤：'{}' 不像专有名词，跳过", en);
                continue;
            }
            let chinese = zh_candidates[0].clone();
            // 如果译名和英文名完全相同（或译名是纯英文/纯ASCII），说明模型没翻译
            // 跳过这个条目，避免把英文当译名注入翻译批次
            if chinese.eq_ignore_ascii_case(en) || is_pure_ascii(&chinese) {
                tracing::debug!("人名过滤：'{}' 译名 '{}' 无中文，跳过", en, chinese);
                continue;
            }
            let alternatives = zh_candidates.into_iter().skip(1).collect();
            names.push(ExtractedName {
                english: en.to_string(),
                chinese,
                alternatives,
            });
        }
    }
    names
}

/// 合并多段人名提取结果，同一英文名多个译名时频率优先，平局取首次出现
fn merge_extracted_names(segment_results: &[SegmentNameResult]) -> Vec<ExtractedName> {
    use std::collections::{HashMap, HashSet};
    // en_name -> (zh_name -> (count, first_segment_idx))
    let mut stats: HashMap<String, HashMap<String, (usize, usize)>> = HashMap::new();

    for result in segment_results {
        for name in &result.names {
            let entry = stats.entry(name.english.clone()).or_default();
            // 把主译名和候选译名都纳入统计；同一 segment 内相同译名只计一次
            let all_zh: HashSet<&String> = std::iter::once(&name.chinese)
                .chain(name.alternatives.iter())
                .collect();
            for zh in all_zh {
                let counter = entry.entry(zh.clone()).or_insert((0, result.segment_idx));
                counter.0 += 1;
            }
        }
    }

    // 每个英文名：按频率降序排列所有候选译名，频率最高的作为 chinese，其余作为 alternatives
    let mut merged: Vec<ExtractedName> = stats
        .iter()
        .map(|(en, translations)| {
            let mut sorted: Vec<(&String, &(usize, usize))> = translations.iter().collect();
            // 按频率降序，平局取首次出现 segment 最小的
            sorted.sort_by(|a, b| {
                b.1.0.cmp(&a.1.0).then_with(|| a.1.1.cmp(&b.1.1))
            });
            let chinese = sorted.first().map(|(zh, _)| (*zh).clone()).unwrap_or_default();
            let alternatives: Vec<String> = sorted.iter().skip(1).map(|(zh, _)| (*zh).clone()).collect();
            ExtractedName {
                english: en.clone(),
                chinese,
                alternatives,
            }
        })
        .collect();

    // 按英文名排序，输出稳定
    merged.sort_by(|a, b| a.english.to_lowercase().cmp(&b.english.to_lowercase()));
    merged
}

/// 从字幕文本中提取人名（分段并发扫描 + 合并去重）
/// 返回统一的人名译名表
/// app_handle: 用于向前端发送进度事件（extract-names-progress）
pub async fn extract_names_from_subtitles(
    provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
    texts: &[String],
    source_lang: &str,
    target_lang: &str,
    max_input_tokens: usize,
    cancel_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
    my_gen: u64,
    app_handle: Option<tauri::AppHandle>,
    user_concurrency: usize,
) -> Result<Vec<ExtractedName>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    // 按 token 预算分段
    // 9b 模型在内容过多时容易"逐行分析"产生大量思考过程，
    // 限制每段最多 150 条字幕（约 2500-3000 token），减少 AI 的分析量。
    // 同时保留 token 预算上限作为第二道限制。
    const MAX_LINES_PER_SEGMENT: usize = 150;
    let segment_budget = max_input_tokens.saturating_sub(2000).max(1000).min(3500);
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_tokens = 0usize;
    for text in texts {
        let tokens = text.chars().count() / 3 + 1;
        let would_exceed_tokens = !current.is_empty() && current_tokens + tokens > segment_budget;
        let would_exceed_lines = current.len() >= MAX_LINES_PER_SEGMENT;
        if would_exceed_tokens || would_exceed_lines {
            segments.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        current.push(text.clone());
        current_tokens += tokens;
    }
    if !current.is_empty() {
        segments.push(current);
    }

    let total_segments = segments.len();
    tracing::info!("人名预扫描: {} 段（token 预算: {}）", total_segments, segment_budget);
    let scan_start = std::time::Instant::now();

    // 发送初始进度事件
    if let Some(ref handle) = app_handle {
        let _ = handle.emit("extract-names-progress", serde_json::json!({
            "progress": 0,
            "total": total_segments,
            "done": false
        }));
    }

    // 并发扫描各段，并发数受用户配置控制（本地模型如 LM Studio 可能只支持 1 并发）
    let concurrency = segments.len().min(user_concurrency.max(1));
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut join_set = tokio::task::JoinSet::new();
    let segments_len = segments.len();

    // 流式实时日志：预创建 concurrency 个文件（与翻译调度器相同的方式）
    let stream_log_slots = std::sync::Arc::new(crate::create_stream_log_slots(concurrency));
    let slot_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    // 进度计数器
    let completed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for (idx, segment) in segments.iter().enumerate() {
        let segment = segment.clone();
        let source = source_lang.to_string();
        let target = target_lang.to_string();
        let provider = provider.clone();
        let semaphore = semaphore.clone();
        let stream_log_slots = stream_log_slots.clone();
        let slot_counter = slot_counter.clone();
        let cancel_counter = cancel_counter.clone();
        let my_gen = my_gen;
        let completed_count = completed_count.clone();
        let app_handle = app_handle.clone();
        // 本地模型（如 LM Studio）的 KV cache 可能在连续请求间被污染，
        // 导致后续请求输出质量下降。每段请求间隔 500ms 启动，给引擎时间清理缓存。
        if idx > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        join_set.spawn(async move {
            let _permit = semaphore.acquire_owned().await.unwrap();
            // 取消检查：获取信号量后检查取消标志
            if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                tracing::info!("人名预扫描段 {} 已取消", idx + 1);
                return SegmentNameResult { segment_idx: idx, names: Vec::new() };
            }
            tracing::info!("人名预扫描段 {}/{}，{} 条字幕", idx + 1, segments_len, segment.len());
            let seg_start = std::time::Instant::now();

            // 分配并发槽位：取模获取 slot index，对应一个复用的日志文件
            let slot_idx = (slot_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % stream_log_slots.len() as u64) as usize;
            let stream_log_file = stream_log_slots[slot_idx].clone();

            // 用 provider 的 extract_names_raw 方法发送自定义 prompt 请求
            let system_prompt = build_name_extraction_system_prompt(&source, &target);
            let user_prompt = build_name_extraction_user_prompt(&segment);

            // 用 task_local 传递日志文件句柄，extract_names_raw 中读取
            let content = crate::STREAM_LOG_FILE.scope(stream_log_file, async {
                provider.extract_names_raw(&system_prompt, &user_prompt).await
            }).await;

            let result = match content {
                Ok(content) => {
                    let names = parse_name_extraction_response(&content);
                    tracing::info!("人名预扫描段 {} 提取到 {} 个人名, 耗时 {:.2}s", idx + 1, names.len(), seg_start.elapsed().as_secs_f64());
                    SegmentNameResult { segment_idx: idx, names }
                }
                Err(e) => {
                    tracing::warn!("人名预扫描段 {} 失败: {}", idx + 1, e);
                    SegmentNameResult { segment_idx: idx, names: Vec::new() }
                }
            };

            // 发送进度事件
            let completed = completed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if let Some(ref handle) = app_handle {
                let _ = handle.emit("extract-names-progress", serde_json::json!({
                    "progress": completed,
                    "total": segments_len,
                    "done": completed >= segments_len
                }));
            }

            result
        });
    }

    // 收集结果（支持取消：检测到取消时立即中止剩余任务）
    let mut segment_results = Vec::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(result) = res {
            segment_results.push(result);
        }
        // 取消检查：收到取消信号时中止剩余任务
        if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
            tracing::info!("人名预扫描被取消，中止剩余任务");
            join_set.abort_all();
            break;
        }
    }

    // 取消时返回空结果
    if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
        tracing::info!("人名预扫描已取消");
        // 发送完成事件
        if let Some(ref handle) = app_handle {
            let _ = handle.emit("extract-names-progress", serde_json::json!({
                "progress": total_segments,
                "total": total_segments,
                "done": true
            }));
        }
        return Ok(Vec::new());
    }

    // 按 segment_idx 排序
    segment_results.sort_by_key(|r| r.segment_idx);

    let merged = merge_extracted_names(&segment_results);
    tracing::info!("人名预扫描完成: 合并后 {} 个人名, 总耗时 {:.2}s", merged.len(), scan_start.elapsed().as_secs_f64());

    // 发送完成事件
    if let Some(ref handle) = app_handle {
        let _ = handle.emit("extract-names-progress", serde_json::json!({
            "progress": total_segments,
            "total": total_segments,
            "done": true
        }));
    }

    Ok(merged)
}

// === SECTION: 人名预扫描 END ===

// === SECTION: <name> 标签后处理 ===

/// <name=EnglishName>ChineseTranslation</name> 标签正则
static NAME_TAG_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

/// 从译文中提取所有 <name=En>Zh</name> 标签
/// 返回 (english_name, chinese_translation) 列表
pub fn extract_name_tags(text: &str) -> Vec<(String, String)> {
    let re = NAME_TAG_RE.get_or_init(|| {
        // 容错多种变体：<name=X>Y</name>、<Name=X>Y</Name>、<name="X">Y</name>
        regex::Regex::new(r#"(?i)<name[=\s"]*([^>"\s]+)["\s]*>(.*?)</name\s*>"#).unwrap()
    });
    re.captures_iter(text)
        .filter_map(|cap| {
            let en = cap.get(1)?.as_str().trim().to_string();
            let zh = cap.get(2)?.as_str().trim().to_string();
            if !en.is_empty() && !zh.is_empty() {
                Some((en, zh))
            } else {
                None
            }
        })
        .collect()
}

/// 剥离译文中所有 <name=...>...</name> 标签，只保留中文部分
pub fn strip_name_tags(text: &str) -> String {
    let re = NAME_TAG_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)<name[=\s"]*([^>"\s]+)["\s]*>(.*?)</name\s*>"#).unwrap()
    });
    re.replace_all(text, "$2").to_string()
}

/// 人名一致性后处理结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameConsistencyResult {
    /// 最终的译名表（合并预扫描 glossary + 标签提取的新名字）
    pub final_glossary: Vec<ExtractedName>,
    /// 发现的不一致人名（同一英文名有多个中文译名）
    pub inconsistencies: Vec<NameInconsistency>,
    /// 修正后的翻译条目（标签已剥离）
    pub corrected_indices: Vec<(usize, String)>,
}

/// 单个人名不一致记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameInconsistency {
    pub english: String,
    pub translations: Vec<String>,
    /// 选定的标准译名（频率优先，平局取首次出现）
    pub chosen: String,
}

/// 对翻译结果执行人名一致性后处理
/// 1. 从所有译文中提取 <name=En>Zh</name> 标签
/// 2. 合并预扫描 glossary 和标签提取的人名
/// 3. 检测不一致（同一英文名多个中文译名）
/// 4. 对不一致的译名执行全局替换
/// 5. 剥离所有 <name> 标签
pub fn post_process_name_tags(
    translations: &mut [String],
    pre_scan_glossary: &[(String, String)],
) -> NameConsistencyResult {
    use std::collections::HashMap;

    // 1. 从所有译文中提取标签
    // en_name -> (zh_name -> count)
    let mut tag_stats: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for tr in translations.iter() {
        for (en, zh) in extract_name_tags(tr) {
            *tag_stats.entry(en).or_default().entry(zh).or_default() += 1;
        }
    }

    // 2. 合并预扫描 glossary 和标签提取的人名
    // 预扫描 glossary 优先（用户已确认），标签提取的作为补充
    let mut final_map: HashMap<String, String> = HashMap::new();
    // 先放预扫描结果
    for (en, zh) in pre_scan_glossary {
        final_map.insert(en.clone(), zh.clone());
    }
    // 再放标签提取的（不覆盖预扫描）
    let mut inconsistencies: Vec<NameInconsistency> = Vec::new();
    for (en, zh_map) in &tag_stats {
        if final_map.contains_key(en) {
            // 预扫描已有此名字，检查标签中的译名是否一致
            let chosen = final_map[en].clone();
            let mut all_translations: Vec<String> = zh_map.keys().cloned().collect();
            if !all_translations.contains(&chosen) {
                all_translations.push(chosen.clone());
            }
            if all_translations.len() > 1 {
                inconsistencies.push(NameInconsistency {
                    english: en.clone(),
                    translations: all_translations,
                    chosen: chosen.clone(),
                });
            }
            // 用预扫描的译名替换标签中的不一致译名
        } else {
            // 预扫描没有此名字，从标签中选频率最高的
            let best = zh_map
                .iter()
                .max_by_key(|(_, count)| *count)
                .map(|(zh, _)| zh.clone())
                .unwrap_or_default();
            let mut all_translations: Vec<String> = zh_map.keys().cloned().collect();
            if all_translations.len() > 1 {
                inconsistencies.push(NameInconsistency {
                    english: en.clone(),
                    translations: all_translations.clone(),
                    chosen: best.clone(),
                });
            }
            final_map.insert(en.clone(), best);
        }
    }

    // 3. 全局替换：把所有标签中的不一致译名替换为标准译名，然后剥离标签
    let mut corrected_indices: Vec<(usize, String)> = Vec::new();
    for (i, tr) in translations.iter_mut().enumerate() {
        let original = tr.clone();
        // 先替换标签内的不一致译名
        let re = NAME_TAG_RE.get_or_init(|| {
            regex::Regex::new(r#"(?i)<name[=\s"]*([^>"\s]+)["\s]*>(.*?)</name\s*>"#).unwrap()
        });
        let replaced = re.replace_all(tr, |caps: &regex::Captures| {
            let en = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let zh = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            if let Some(standard) = final_map.get(en) {
                format!("<name={}>{}</name>", en, standard)
            } else {
                format!("<name={}>{}</name>", en, zh)
            }
        }).to_string();
        // 再剥离标签
        let stripped = strip_name_tags(&replaced);
        if stripped != original {
            *tr = stripped.clone();
            corrected_indices.push((i, stripped));
        }
    }

    // 4. 构建最终 glossary
    let mut final_glossary: Vec<ExtractedName> = final_map
        .iter()
        .map(|(en, zh)| ExtractedName {
            english: en.clone(),
            chinese: zh.clone(),
            alternatives: Vec::new(),
        })
        .collect();
    final_glossary.sort_by(|a, b| a.english.to_lowercase().cmp(&b.english.to_lowercase()));

    NameConsistencyResult {
        final_glossary,
        inconsistencies,
        corrected_indices,
    }
}

// === SECTION: <name> 标签后处理 END ===

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
    fn test_parse_numbered_response_json_split_detection() {
        // AI 拆分了含多行的条目，返回了超出范围的编号 n=6（期望 5）
        // 应检测为对齐失败，触发降级重试（而非 silently 过滤 n=6 后数量恰好匹配）
        let content = r#"```json
[{"n": 1, "t": "杰瑞："}, {"n": 2, "t": "呃，我太紧张了！"}, {"n": 3, "t": "因为不再失业了。"}, {"n": 4, "t": "这是个担忧虫。"}, {"n": 5, "t": "我们要吃早餐药吗？"}, {"n": 6, "t": "没人给谁当经销商。"}]
```"#;
        let result = OpenAiProvider::parse_numbered_response(content, 5);
        assert!(result.is_err(), "AI 返回超出范围的编号应触发对齐失败");
    }

    #[test]
    fn test_parse_numbered_response_json_normal_5() {
        // 正常的 5 条 JSON 响应应成功解析
        let content = r#"[{"n": 1, "t": "你好"}, {"n": 2, "t": "世界"}, {"n": 3, "t": "测试"}, {"n": 4, "t": "四"}, {"n": 5, "t": "五"}]"#;
        let result = OpenAiProvider::parse_numbered_response(content, 5).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试", "四", "五"]);
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

    // === SECTION 11: cache provider name tests ===

    #[test]
    fn test_build_cache_provider_name_injection() {
        // 不同输入产生不同输出
        let a = build_cache_provider_name(&["openai", "deepseek", "deepseek-chat"]);
        let b = build_cache_provider_name(&["openai", "zhipu", "glm-4-flash"]);
        assert_ne!(a, b);
    }

    #[test]
    fn test_build_cache_provider_name_no_collision_pipe_in_model() {
        // model 含 || 时不应与 serviceId 含 || 碰撞
        let a = build_cache_provider_name(&["openai", "x", "a||b"]);
        let b = build_cache_provider_name(&["openai", "x||a", "b"]);
        assert_ne!(a, b);
    }

    #[test]
    fn test_build_cache_provider_name_escape() {
        // 字段内的 | 被双写转义
        let name = build_cache_provider_name(&["openai", "deepseek", "model|with|pipe"]);
        // model|with|pipe → model||with||pipe
        assert_eq!(name, "openai|deepseek|model||with||pipe");
    }

    #[test]
    fn test_effective_concurrency_new_signature() {
        assert_eq!(TranslateProvider::effective_concurrency(10, 5), 5);
        assert_eq!(TranslateProvider::effective_concurrency(3, 10), 3);
        assert_eq!(TranslateProvider::effective_concurrency(0, 5), 1);
    }

    // === SECTION 11 END ===

    // === 人名提取解析测试 ===
    #[test]
    fn test_parse_name_extraction_english_with_chinese_paren() {
        // 模型输出 `EnglishName → EnglishName（中文翻译）` 格式
        // 非品牌名（如 The FarmDroid）：只取括号内中文
        // 品牌名（如 GS4、AgBot）：保留 `英文（中文）` 格式
        let content = "The FarmDroid → The FarmDroid（农场机器人）\nCornHub → CornHub（玉米枢纽/智能农机品牌）\nGS4 → GS4（农业系统4）\nJeremy → 杰里米";
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 4);
        assert_eq!(names[0].english, "The FarmDroid");
        assert_eq!(names[0].chinese, "农场机器人");
        // CornHub 不是全大写，不是品牌名格式，只取括号内中文
        assert_eq!(names[1].english, "CornHub");
        assert_eq!(names[1].chinese, "玉米枢纽");
        assert_eq!(names[1].alternatives, vec!["智能农机品牌"]);
        // GS4 全大写+数字，是品牌名格式，保留 `英文（中文）`
        assert_eq!(names[2].english, "GS4");
        assert_eq!(names[2].chinese, "GS4（农业系统4）");
        assert_eq!(names[3].english, "Jeremy");
        assert_eq!(names[3].chinese, "杰里米");
    }

    #[test]
    fn test_parse_name_extraction_paren_with_explanation() {
        // 括号内含翻译+解释说明，非品牌名只取逗号前的中文翻译
        let content = "Mounjaro ramp → Mounjaro Ramp（莫努佳罗斜坡，可能指某种设备或地形特征）";
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].english, "Mounjaro ramp");
        assert_eq!(names[0].chinese, "莫努佳罗斜坡");
    }

    #[test]
    fn test_parse_name_extraction_normal_chinese_translation() {
        // 正常中文翻译不受影响
        let content = "Jeremy → 杰里米\nKaleb → 卡莱布\nLisa → 丽莎";
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 3);
        assert_eq!(names[0].chinese, "杰里米");
        assert_eq!(names[1].chinese, "卡莱布");
        assert_eq!(names[2].chinese, "丽莎");
    }

    #[test]
    fn test_parse_name_extraction_chinese_with_paren_note() {
        // 中文翻译后带括号注释，应清理括号保留中文
        let content = "Charlie → 查理（牧羊犬名）";
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].chinese, "查理");
    }

    #[test]
    fn test_parse_name_extraction_pure_english_no_paren() {
        // 纯英文无括号（模型无法翻译），跳过该条目
        let content = "Harveest → Harveest";
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 0, "纯英文译名应被跳过");
    }

    #[test]
    fn test_parse_name_extraction_json_format() {
        // JSON 数组格式（新 prompt 要求的输出格式）
        let content = r#"[
          {"en": "Jeremy", "zh": "杰里米"},
          {"en": "Endgame", "zh": "终结者"},
          {"en": "Skylark", "zh": "云雀"},
          {"en": "GS4", "zh": "GS4（农业系统4）"},
          {"en": "Countryfile", "zh": "乡村档案"}
        ]"#;
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 5);
        assert_eq!(names[0].english, "Jeremy");
        assert_eq!(names[0].chinese, "杰里米");
        assert_eq!(names[1].english, "Endgame");
        assert_eq!(names[1].chinese, "终结者");
        assert_eq!(names[2].english, "Skylark");
        assert_eq!(names[2].chinese, "云雀");
        assert_eq!(names[3].english, "GS4");
        assert_eq!(names[3].chinese, "GS4（农业系统4）");
        assert_eq!(names[4].english, "Countryfile");
        assert_eq!(names[4].chinese, "乡村档案");
    }

    #[test]
    fn test_parse_name_extraction_json_with_code_fence() {
        // JSON 被 markdown 代码块包裹
        let content = "```json\n[\n  {\"en\": \"Jeremy\", \"zh\": \"杰里米\"}\n]\n```";
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].english, "Jeremy");
        assert_eq!(names[0].chinese, "杰里米");
    }

    #[test]
    fn test_parse_name_extraction_json_pure_english_skipped() {
        // JSON 中译名为纯英文，应跳过
        let content = r#"[
          {"en": "Jeremy", "zh": "杰里米"},
          {"en": "Mup", "zh": "Mup"}
        ]"#;
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].english, "Jeremy");
    }

    #[test]
    fn test_parse_name_extraction_json_slash_alternatives() {
        // JSON 中 zh 含 `/` 分隔的多个候选译名
        let content = r#"[
          {"en": "Zeppelin", "zh": "齐柏林飞艇/斑羚"}
        ]"#;
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].chinese, "齐柏林飞艇");
        assert_eq!(names[0].alternatives, vec!["斑羚"]);
    }

    #[test]
    fn test_parse_name_extraction_json_malformed_regex_fallback() {
        // JSON 格式错误（缺括号），正则提取兜底
        let content = r#"Here are the names:
        {"en": "Jeremy", "zh": "杰里米"},
        {"en": "Kaleb", "zh": "卡莱布"},
        "#;
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].english, "Jeremy");
        assert_eq!(names[0].chinese, "杰里米");
        assert_eq!(names[1].english, "Kaleb");
        assert_eq!(names[1].chinese, "卡莱布");
    }

    #[test]
    fn test_parse_name_extraction_slash_alternatives() {
        // `/` 分隔多个候选译名
        let content = "Zeppelin → 齐柏林飞艇 / 斑羚";
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].chinese, "齐柏林飞艇");
        assert_eq!(names[0].alternatives, vec!["斑羚"]);
    }

    #[test]
    fn test_parse_name_extraction_thinking_process_filtered() {
        // 9b 模型在"最终输出"区域混入思考过程，最后重新输出干净名单
        // 解析器应只取最后一段连续名单，过滤掉思考过程行
        let content = r#"Kaleb → 卡莱布
Hannah → 汉娜
Reactor → 反应牛（结核阳性牛）
Re-evaluating Line 4: "yellowhammeresque" -> Yellowhammer (bird species)
Refining the list for strict Proper Nouns:
1. Kaleb
2. Hannah
Let's compile the final list.

Kaleb → 卡莱布
Hannah → 汉娜
Yellowhammer → 黄鹀
Hitler → 希特勒"#;
        let names = parse_name_extraction_response(content);
        // 应只取最后 4 行干净名单，思考过程行被过滤
        assert_eq!(names.len(), 4);
        assert_eq!(names[0].english, "Kaleb");
        assert_eq!(names[0].chinese, "卡莱布");
        assert_eq!(names[1].english, "Hannah");
        assert_eq!(names[1].chinese, "汉娜");
        assert_eq!(names[2].english, "Yellowhammer");
        assert_eq!(names[3].english, "Hitler");
        // 确保思考过程的条目不在结果中
        assert!(names.iter().all(|n| n.english != "Reactor"));
        assert!(names.iter().all(|n| n.english != "Re-evaluating Line 4"));
    }

    #[test]
    fn test_is_likely_proper_noun_person_names() {
        // 人名应保留
        assert!(is_likely_proper_noun("Jeremy"));
        assert!(is_likely_proper_noun("Kaleb"));
        assert!(is_likely_proper_noun("Martin Brundle"));
        assert!(is_likely_proper_noun("Marcus Aurelius"));
        assert!(is_likely_proper_noun("Mariah Carey"));
    }

    #[test]
    fn test_is_likely_proper_noun_bird_animal_names() {
        // 鸟名/动物名应保留
        assert!(is_likely_proper_noun("Skylark"));
        assert!(is_likely_proper_noun("Corn Bunting"));
        assert!(is_likely_proper_noun("Yellowhammer"));
        assert!(is_likely_proper_noun("Greater Whitethroat"));
        assert!(is_likely_proper_noun("Turtle Dove"));
    }

    #[test]
    fn test_is_likely_proper_noun_brands_places() {
        // 品牌/地名应保留
        assert!(is_likely_proper_noun("AgBot"));
        assert!(is_likely_proper_noun("CornHub"));
        assert!(is_likely_proper_noun("Countryfile"));
        assert!(is_likely_proper_noun("Barn Ground"));
        assert!(is_likely_proper_noun("Diddly Squat"));
        assert!(is_likely_proper_noun("GS4 field"));
        // 含首词冠词 The 的专有名词应保留
        assert!(is_likely_proper_noun("The FarmDroid"));
        assert!(is_likely_proper_noun("The Who"));
    }

    #[test]
    fn test_is_likely_proper_noun_filters_long_phrases() {
        // 长短语应被过滤
        assert!(!is_likely_proper_noun("Big plan of getting more cattle this winter"));
        assert!(!is_likely_proper_noun("Fucked in terms of reproducing calves"));
        assert!(!is_likely_proper_noun("Dilwyn measuring lumps in the cow's necks"));
        assert!(!is_likely_proper_noun("Most valuable thing on the farm at the moment"));
        assert!(!is_likely_proper_noun("Endgame and two others are marginal"));
    }

    #[test]
    fn test_is_likely_proper_noun_filters_phrases_with_function_words() {
        // 含功能词的短语应被过滤
        assert!(!is_likely_proper_noun("Rammed home the point"));
        assert!(!is_likely_proper_noun("Cultivating the margin"));
        assert!(!is_likely_proper_noun("Grand scheme of things"));
        assert!(!is_likely_proper_noun("Oats next week"));
        assert!(!is_likely_proper_noun("Wheats will be ready"));
        assert!(!is_likely_proper_noun("Two weeks later"));
        assert!(!is_likely_proper_noun("Monday the 28th"));
        assert!(!is_likely_proper_noun("End of July"));
        assert!(!is_likely_proper_noun("Test in 60 days"));
        assert!(!is_likely_proper_noun("Right on the borderline"));
        assert!(!is_likely_proper_noun("Bring cows in or out"));
        assert!(!is_likely_proper_noun("Keep that head up"));
        assert!(!is_likely_proper_noun("Whole herd stuck here"));
        assert!(!is_likely_proper_noun("Six-monthly test for TB"));
    }

    #[test]
    fn test_parse_name_extraction_filters_phrases() {
        // 模型输出中混入短语，解析时应过滤掉
        let content = "Jeremy → 杰里米\n\
            Big plan of getting more cattle this winter → 今年冬天增加牛只的大计划\n\
            Kaleb → 卡莱布\n\
            Cultivating the margin → 培育边缘\n\
            Skylark → 云雀";
        let names = parse_name_extraction_response(content);
        // 应只保留 3 个专有名词，2 个短语被过滤
        assert_eq!(names.len(), 3);
        assert_eq!(names[0].english, "Jeremy");
        assert_eq!(names[1].english, "Kaleb");
        assert_eq!(names[2].english, "Skylark");
    }
}
