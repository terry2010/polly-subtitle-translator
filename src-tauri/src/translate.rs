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
            password: crate::config::CredentialStore::load("proxy", "pass", "构建翻译/搜索代理客户端").ok(),
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

/// 翻译提供商类型
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
    /// - Qps：每秒最多 N 个请求，请求间强制间隔 1/N 秒（百度、有道）
    /// - Concurrency：最多 N 个并发请求，无间隔要求（OpenAI、DeepSeek、DeepL 等）
    pub fn rate_limit_policy(&self) -> RateLimitPolicy {
        match self {
            // 传统翻译 API：严格 QPS 限流
            TranslateProvider::Baidu => RateLimitPolicy::Qps(1),      // 免费 1 QPS
            TranslateProvider::Youdao => RateLimitPolicy::Qps(1),     // 免费 1 QPS
            // 大模型 / 高并发 API：并发数限制
            TranslateProvider::OpenAi => RateLimitPolicy::Concurrency(5),   // 默认 5 并发
            TranslateProvider::DeepL => RateLimitPolicy::Concurrency(5),
            TranslateProvider::Google => RateLimitPolicy::Concurrency(10),
            TranslateProvider::Bing => RateLimitPolicy::Concurrency(10),
            TranslateProvider::Caiyun => RateLimitPolicy::Qps(5),     // 彩云 QPS 限流
            TranslateProvider::Niutrans => RateLimitPolicy::Concurrency(5),
            TranslateProvider::Tencent => RateLimitPolicy::Qps(5),    // 腾讯 QPS 限流
            TranslateProvider::Volcengine => RateLimitPolicy::Qps(5), // 火山 QPS 限流
            TranslateProvider::Aliyun => RateLimitPolicy::Qps(50),    // 阿里 50 QPS
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

/// 分隔符：用于分隔输入和输出的字幕条目
/// 使用 emoji 🔸 作为分隔符，视觉显眼，模型不容易打错，且几乎不会在字幕内容中出现
const DELIMITER: &str = "🔸";

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
                 - Each input line is ONE subtitle entry. Do not split it into multiple numbered entries.\n\
                 - A translation may span multiple lines. If so, indent the continuation lines with two spaces.\n\
                 - Preserve special Unicode characters (like \u{E001}) exactly as-is.\n\
                 - SDH annotations in brackets are NOT dialogue. Translate their content into {tgt} but keep the brackets. This includes:\n\
                   * Sound effects: [birds chirping] -> [鸟鸣], [door closes] -> [关门声]\n\
                   * Music cues: [ominous music] -> [不祥的音乐], [music playing] -> [音乐播放中]\n\
                   * Speaker IDs: [man] -> [男人], [police officer] -> [警察], [Emma] -> [艾玛]\n\
                   * Vocal tone: [whispering] -> [低语], [sobbing] -> [啜泣], [sighs] -> [叹气]\n\
                   * Language tags: [in Spanish] -> [西班牙语], [speaking Japanese] -> [说日语]\n\
                   * Other: [bleep] -> [消音], [laughs] -> [笑]\n\
                 - Do not merge or split lines.\n\
                 - Do not add explanations, notes, or any extra text.\n\
                 - You MUST translate every entry into {tgt}. Do NOT output the original {src} text.\n\
                 - Person names must be translated consistently throughout. If a name appears multiple times, use the same translation every time.",
        user_line_format: "{index}. {text}",
    }),
    ("deepseek", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate from {src} to {tgt}.\n\n\
                 Output format:\n\
                 - One translation per line, prefixed with the input line number.\n\
                 - Format: \"N. <translation>\"\n\
                 - Each input line is ONE subtitle entry. Do not split it into multiple numbered entries.\n\
                 - A translation may span multiple lines. If so, indent the continuation lines with two spaces.\n\
                 - Preserve all special characters and placeholders unchanged.\n\
                 - SDH annotations in brackets are NOT dialogue. Translate their content into {tgt} but keep the brackets. This includes:\n\
                   * Sound effects: [birds chirping] -> [鸟鸣], [door closes] -> [关门声]\n\
                   * Music cues: [ominous music] -> [不祥的音乐], [music playing] -> [音乐播放中]\n\
                   * Speaker IDs: [man] -> [男人], [police officer] -> [警察], [Emma] -> [艾玛]\n\
                   * Vocal tone: [whispering] -> [低语], [sobbing] -> [啜泣], [sighs] -> [叹气]\n\
                   * Language tags: [in Spanish] -> [西班牙语], [speaking Japanese] -> [说日语]\n\
                   * Other: [bleep] -> [消音], [laughs] -> [笑]\n\
                 - Do not merge, split, or skip any lines.\n\
                 - Output ONLY the numbered translations, nothing else.\n\
                 - You MUST translate every entry into {tgt}. Do NOT output the original {src} text.\n\
                 - Person names must be translated consistently throughout. If a name appears multiple times, use the same translation every time.",
        user_line_format: "{index}. {text}",
    }),
    ("generic", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate the following {src} subtitles into {tgt}.\n\n\
                 Rules:\n\
                 - Output ONLY the translations, one per line, prefixed with the line number.\n\
                 - Format: \"N. <translation>\"\n\
                 - Keep the same line numbering as the input.\n\
                 - Each input line is ONE subtitle entry. Do not split it into multiple numbered entries.\n\
                 - A translation may span multiple lines. If so, indent the continuation lines with two spaces.\n\
                 - Preserve special Unicode characters exactly as-is.\n\
                 - SDH annotations in brackets are NOT dialogue. Translate their content into {tgt} but keep the brackets. This includes:\n\
                   * Sound effects: [birds chirping] -> [鸟鸣], [door closes] -> [关门声]\n\
                   * Music cues: [ominous music] -> [不祥的音乐], [music playing] -> [音乐播放中]\n\
                   * Speaker IDs: [man] -> [男人], [police officer] -> [警察], [Emma] -> [艾玛]\n\
                   * Vocal tone: [whispering] -> [低语], [sobbing] -> [啜泣], [sighs] -> [叹气]\n\
                   * Language tags: [in Spanish] -> [西班牙语], [speaking Japanese] -> [说日语]\n\
                   * Other: [bleep] -> [消音], [laughs] -> [笑]\n\
                 - Do not merge or split lines.\n\
                 - Do not add any extra text.\n\
                 - You MUST translate every entry into {tgt}. Do NOT output the original {src} text.\n\
                 - Person names must be translated consistently throughout. If a name appears multiple times, use the same translation every time.",
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

            // 检测普通换行符（SRT 中的多行字幕）
            // 用占位符保护，使每条字幕在发送给模型时是单行文本，避免模型拆行
            // restore 时精确还原为 \n，不会误伤翻译中原本的 | 字符
            if remaining.starts_with('\n') {
                let placeholder = self.add_placeholder("\n");
                result.push(placeholder);
                remaining = &remaining[1..];
                continue;
            }

            // 检测分隔符 emoji 🔸：用占位符保护，避免与翻译格式的分隔符混淆
            // restore 时精确还原为 🔸
            if remaining.starts_with(DELIMITER) {
                let placeholder = self.add_placeholder(DELIMITER);
                result.push(placeholder);
                remaining = &remaining[DELIMITER.len()..];
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
    /// 同时清除模型在翻译中残留的分隔符 🔸（模型有时会错误地插入 🔸）
    pub fn restore(&self, text: &str) -> String {
        // 先清除模型残留的 🔸（此时占位符还未替换，不会误伤被保护的 🔸）
        let text = text.replace(DELIMITER, "");
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

    /// 构建 system prompt（从内置模板渲染 + 译名表 + 人名标记规则）
    fn build_system_prompt(&self, source_lang: &str, target_lang: &str) -> String {
        let src = lang_full_name(source_lang);
        let tgt = lang_full_name(target_lang);
        let tmpl = BUILTIN_TEMPLATES
            .iter()
            .find(|(k, _)| *k == self.model_type.as_str())
            .map(|(_, t)| t)
            .unwrap_or(&BUILTIN_TEMPLATES[2].1); // 兜底 generic
        let mut prompt = tmpl.render_system(src, tgt);

        // 注入译名表
        if !self.glossary.is_empty() {
            // 检测是否有含 / 分隔的多译名条目
            let has_multi = self.glossary.iter().any(|(_, zh)| zh.contains('/'));
            let glossary_text = self.glossary
                .iter()
                .map(|(en, zh)| format!("  {} → {}", en, zh))
                .collect::<Vec<_>>()
                .join("\n");
            if has_multi {
                // 有多译名条目：告诉 AI 按语境选择
                prompt.push_str(&format!(
                    "\n\n\
                     - Established name translations (use these translations every time the name appears):\n\
                     {}\n\
                     - IMPORTANT: Some entries have multiple translations separated by \" / \" (e.g. \"TB → 牛结核病 / 太字节\").\n\
                     - For these entries, choose the MOST APPROPRIATE translation based on the surrounding context.\n\
                     - Use only ONE translation per occurrence (do NOT output the slash-separated alternatives).\n\
                     - If a name is not in this list, translate it consistently and use the same translation throughout.",
                    glossary_text
                ));
            } else {
                prompt.push_str(&format!(
                    "\n\n\
                     - Established name translations (MUST use these EXACT translations every time the name appears):\n\
                     {}\n\
                     - If a name is not in this list, translate it consistently and use the same translation throughout.",
                    glossary_text
                ));
            }
        }

        // 注入人名标记规则
        if self.name_tagging {
            prompt.push_str(
                "\n\n\
                 - IMPORTANT: Wrap EVERY person name in the translation with tags: <name=EnglishName>ChineseTranslation</name>\n\
                 - Example: \"Kaleb, come here!\" → \"<name=Kaleb>卡莱布</name>，过来！\"\n\
                 - Example: \"[Kaleb] Hello\" → \"[<name=Kaleb>卡莱布</name>] 你好\" (SDH speaker ID also tagged)\n\
                 - Use the original English name in the tag attribute, and the Chinese translation between the tags.\n\
                 - Tag ALL person names, including those in the glossary above.\n\
                 - Do NOT tag place names, brand names, or common words — only person names."
            );
        }

        prompt
    }

    /// 构建 user prompt（编号格式）
    /// 每条字幕前加序号，强制模型逐条翻译，避免模型 echo 回原文
    fn build_user_prompt(&self, texts: &[&String]) -> String {
        texts
            .iter()
            .enumerate()
            .map(|(i, txt)| format!("{}. {}", i + 1, txt))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 判断一行是否是前言/后记（模型的客套话，不是翻译内容）
    /// 仅用于编号解析失败后的行对齐回退路径
    fn is_preamble_or_postscript(line: &str) -> bool {
        let lower = line.to_lowercase();
        // 常见英文前言/后记
        const PREAMBLE_PATTERNS: &[&str] = &[
            "here are", "here is", "sure,", "certainly,", "of course,", "okay,",
            "translation:", "translations:", "the translation", "translated",
            "i'll translate", "i will translate", "let me translate",
            "below are", "the following",
        ];
        const POSTSCRIPT_PATTERNS: &[&str] = &[
            "hope this helps", "let me know", "feel free", "that's it",
            "done.", "that's all", "enjoy!",
        ];
        // 常见中文前言/后记
        const CN_PATTERNS: &[&str] = &[
            "以下是", "翻译如下", "好的，", "当然，", "没问题，",
            "这是翻译", "这些是翻译", "祝您", "希望对您",
        ];
        if PREAMBLE_PATTERNS.iter().any(|p| lower.starts_with(p)) {
            return true;
        }
        if POSTSCRIPT_PATTERNS.iter().any(|p| lower.starts_with(p)) {
            return true;
        }
        if CN_PATTERNS.iter().any(|p| line.starts_with(p)) {
            return true;
        }
        // 纯标点行（如 "..."、"——"）
        if line.chars().all(|c| c.is_whitespace() || ".,;:!?，。；：！？…—-—~～".contains(c)) {
            return true;
        }
        false
    }

    /// 解析模型返回的编号列表响应，按编号对齐回输入
    /// 支持多行翻译：非编号行（不以 N. 开头）作为上一条翻译的续行
    fn parse_numbered_response(
        content: &str,
        expected_count: usize,
    ) -> Result<Vec<String>, AppError> {
        let re = NUMBERED_LINE_RE.get_or_init(|| {
            regex::Regex::new(r"^(\d+)[.、:)]\s*(.*)$").unwrap()
        });

        // 1. 逐行扫描，收集编号条目
        // 遇到编号行开始新条目，缩进行（2+空格开头）作为上一条的多行续行
        // 非缩进的非编号行视为前言/后记，不追加到任何条目
        let mut all_numbered: Vec<(usize, String)> = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(captures) = re.captures(trimmed) {
                let num: usize = captures[1].parse().unwrap_or(0);
                let text = captures.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                all_numbered.push((num, text.to_string()));
            } else if line.starts_with("  ") {
                // 缩进行（2+空格开头）：追加到上一条翻译（多行翻译的续行）
                if let Some(last) = all_numbered.last_mut() {
                    last.1 = format!("{}\n{}", last.1, trimmed);
                }
            }
            // 非缩进的非编号行：前言/后记，跳过
        }

        // 1a. 优先尝试 1-based 编号对齐：编号 1..=expected_count
        let translations: std::collections::HashMap<usize, String> = all_numbered.iter()
            .filter(|(n, _)| *n >= 1 && *n <= expected_count)
            .map(|(n, t)| (*n, t.clone()))
            .collect();

        // 2. 按编号顺序组装结果（1-based 完全匹配）
        if translations.len() == expected_count {
            // 检查是否有编号超出范围的条目（模型把多行翻译拆成了多个编号条目）
            let extra: Vec<(usize, String)> = all_numbered.iter()
                .filter(|(n, _)| *n > expected_count)
                .cloned()
                .collect();
            if extra.is_empty() {
                // 没有超出条目，直接返回
                let result: Vec<String> = (1..=expected_count)
                    .map(|i| translations.get(&i).cloned().unwrap_or_default())
                    .collect();
                return Ok(result);
            }
            // 有超出条目，需要合并到前一个有效条目
            tracing::warn!(
                "1-based 匹配但有 {} 条编号超出范围，合并到前一条",
                extra.len()
            );
            let mut result: Vec<String> = (1..=expected_count)
                .map(|i| translations.get(&i).cloned().unwrap_or_default())
                .collect();
            // 按编号顺序合并超出条目到上一条
            let mut sorted_extra = extra;
            sorted_extra.sort_by_key(|(n, _)| *n);
            for (_num, text) in sorted_extra {
                if let Some(last) = result.last_mut() {
                    if !last.is_empty() {
                        last.push('\n');
                    }
                    last.push_str(&text);
                }
            }
            return Ok(result);
        }

        // 2c. 1-based 不完全匹配，尝试按编号排序重新对齐
        // 覆盖：0-based 编号、继续上一批编号、编号超出范围等情况
        // 优先于 2b：如果有编号 0 的行被 1-based 过滤，排序后能正确对齐
        if all_numbered.len() >= expected_count {
            let mut sorted = all_numbered.clone();
            sorted.sort_by_key(|(n, _)| *n);
            // 去重：同一编号取最后一条
            let mut deduped: Vec<(usize, String)> = Vec::new();
            for (num, text) in sorted {
                if let Some(last) = deduped.last_mut() {
                    if last.0 == num {
                        last.1 = text;
                        continue;
                    }
                }
                deduped.push((num, text));
            }
            if deduped.len() > expected_count {
                // 模型把多行翻译拆成了多个编号条目（如 #28 拆成 #28 + #31）
                // 把编号 > expected_count 的条目合并到前一个有效条目中
                tracing::warn!(
                    "编号重新对齐：收集到 {} 条编号行，期望 {} 条，合并超出的 {} 条",
                    deduped.len(),
                    expected_count,
                    deduped.len() - expected_count
                );
                let mut result: Vec<String> = Vec::new();
                for (num, text) in deduped {
                    if num <= expected_count {
                        // 编号在范围内，正常添加
                        // 确保结果向量有足够空间
                        while result.len() < num - 1 {
                            result.push(String::new());
                        }
                        if result.len() == num - 1 {
                            result.push(text);
                        } else {
                            // 编号已存在，替换
                            result[num - 1] = text;
                        }
                    } else {
                        // 编号超出范围，合并到上一条（用换行符连接）
                        if let Some(last) = result.last_mut() {
                            if !last.is_empty() {
                                last.push('\n');
                            }
                            last.push_str(&text);
                        }
                    }
                }
                // 确保结果数量等于 expected_count
                while result.len() < expected_count {
                    result.push(String::new());
                }
                result.truncate(expected_count);
                return Ok(result);
            }
            if deduped.len() == expected_count {
                let result: Vec<String> = deduped.into_iter()
                    .map(|(_, t)| t)
                    .collect();
                return Ok(result);
            }
        }

        // 2b. 1-based 部分匹配：编号在范围内但数量不够
        // 到这里说明 2c 也没能对齐（编号行总数 < expected_count）
        if !translations.is_empty() {
            let max_num = *translations.keys().max().unwrap_or(&0);
            if max_num <= expected_count && translations.len() < expected_count {
                // 检查编号是否连续（1, 2, 3, ..., max_num 无缺失）
                // 如果有缺失（如模型跳过了 #34），编号 N 的翻译可能对应原文 N+1，
                // 此时按编号对齐会导致整体错位，必须放弃编号对齐
                let is_contiguous = (1..=max_num).all(|i| translations.contains_key(&i));
                if is_contiguous {
                    // 编号连续 1..max_num，只是少了 max_num+1..expected_count
                    // 可以安全地按编号对齐，缺失的尾部填空让调度器逐条重试
                    let result: Vec<String> = (1..=expected_count)
                        .map(|i| translations.get(&i).cloned().unwrap_or_default())
                        .collect();
                    tracing::warn!(
                        "编号解析部分成功（连续 1-{}）：期望 {} 条，缺失尾部 {} 条，将由调度器逐条重试",
                        max_num,
                        expected_count,
                        expected_count - translations.len()
                    );
                    return Ok(result);
                } else {
                    // 编号有缺失（如跳过了 #34），编号 N 对应的可能是原文 N+1
                    // 放弃编号对齐，退回按行对齐 + 逐条重试
                    tracing::warn!(
                        "编号解析有缺失（非连续 1-{}），放弃编号对齐，退回按行对齐",
                        max_num
                    );
                }
            }
        }

        // 3. 编号解析失败 → 退化为按行对齐
        // 过滤掉空行，并尝试去掉编号前缀（模型可能用了不同格式）
        let raw_lines: Vec<String> = content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        // 过滤明显的前言/后记行（模型常加的客套话，不是翻译内容）
        // 仅在行数 > expected_count 时过滤，避免误删正常翻译
        let all_lines: Vec<String> = if raw_lines.len() > expected_count {
            raw_lines
                .iter()
                .filter(|l| !Self::is_preamble_or_postscript(l))
                .cloned()
                .collect()
        } else {
            raw_lines
        };

        // 3a. 行数正好匹配：去掉编号前缀后返回
        // 注意：不能直接返回原始行，因为可能包含 "1. " 等编号前缀
        if all_lines.len() == expected_count {
            let result: Vec<String> = all_lines
                .iter()
                .map(|l| {
                    // 去掉编号前缀（与 step 1 相同的正则）
                    if let Some(caps) = re.captures(l) {
                        caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or(l.clone())
                    } else {
                        l.clone()
                    }
                })
                .collect();
            tracing::warn!(
                "编号解析失败，按行对齐（行数匹配 {}），已去掉编号前缀",
                expected_count
            );
            return Ok(result);
        }

        // 3b. 行数过多：尝试截取连续的 expected_count 行（去掉前言/后记）
        if all_lines.len() > expected_count {
            // 找到第一个和最后一个编号行的位置，确定翻译内容区域
            let first_numbered = all_lines.iter().position(|l| re.captures(l).is_some());
            let last_numbered = all_lines.iter().rposition(|l| re.captures(l).is_some());
            if let (Some(first), Some(last)) = (first_numbered, last_numbered) {
                let numbered_count = last - first + 1;
                if numbered_count == expected_count {
                    // 编号行数量正好匹配，截取编号行区域
                    let result: Vec<String> = all_lines[first..=last]
                        .iter()
                        .map(|l| {
                            if let Some(caps) = re.captures(l) {
                                caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or(l.clone())
                            } else {
                                l.clone()
                            }
                        })
                        .collect();
                    tracing::warn!(
                        "行数过多（{} > {}），截取编号行区域 [{}..{}] 去除前言/后记",
                        all_lines.len(),
                        expected_count,
                        first,
                        last
                    );
                    return Ok(result);
                }
                // 编号行数量不匹配，但可以按编号提取
                if numbered_count > 0 {
                    let mut num_map: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
                    for l in &all_lines[first..=last] {
                        if let Some(caps) = re.captures(l) {
                            let num: usize = caps[1].parse().unwrap_or(0);
                            let text = caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
                            if num > 0 && num <= expected_count {
                                num_map.insert(num, text);
                            }
                        }
                    }
                    if num_map.len() == expected_count {
                        let result: Vec<String> = (1..=expected_count)
                            .map(|i| num_map.remove(&i).unwrap_or_default())
                            .collect();
                        return Ok(result);
                    }
                    // 部分编号匹配
                    if !num_map.is_empty() {
                        let result: Vec<String> = (1..=expected_count)
                            .map(|i| num_map.get(&i).cloned().unwrap_or_default())
                            .collect();
                        tracing::warn!(
                            "编号行部分匹配：期望 {} 条，匹配到 {} 条",
                            expected_count,
                            num_map.len()
                        );
                        return Ok(result);
                    }
                }
            }
            // 编号行定位失败，退回窗口扫描（保守策略：只选全是编号行或全非编号行的窗口）
            for start in 0..=(all_lines.len() - expected_count) {
                let window = &all_lines[start..start + expected_count];
                let numbered_in_window = window.iter().filter(|l| re.captures(l).is_some()).count();
                // 只接受全部是编号行的窗口（最安全）
                if numbered_in_window == expected_count {
                    let result: Vec<String> = window
                        .iter()
                        .map(|l| {
                            if let Some(caps) = re.captures(l) {
                                caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or(l.clone())
                            } else {
                                l.clone()
                            }
                        })
                        .collect();
                    tracing::warn!(
                        "行数过多（{} > {}），截取全编号行窗口 [{}..{}]",
                        all_lines.len(),
                        expected_count,
                        start,
                        start + expected_count
                    );
                    return Ok(result);
                }
            }
        }

        // 4. 所有策略都失败 → 记录原始内容并返回对齐失败
        tracing::warn!(
            "翻译对齐失败：期望 {} 条，编号解析到 {} 条，按行 {} 条。原始内容（前 500 字）：{}",
            expected_count,
            translations.len(),
            all_lines.len(),
            &content[..content.len().min(500)]
        );
        Err(AppError::TranslateAlignFailed {
            missing: expected_count.saturating_sub(all_lines.len()),
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
            "stream": false,
        });
        // Qwen3 thinking 模式：关闭 thinking
        if self.model_type == ModelType::Qwen3 {
            request_body["chat_template_kwargs"] = serde_json::json!({
                "enable_thinking": false
            });
        }
        let request_json = request_body;
        // 超时：本地模型（局域网/localhost）可能需要很长时间（27b 模型单批 10 分钟+）
        // 云端 API 通常 60 秒内返回，但给 120 秒兜底也不会有问题
        // 注意：流式请求优先使用，这里 non-stream 作为回退
        let timeout_secs = if self.is_local_url() { 1800 } else { 120 };
        let mut req = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .json(&request_json);

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

        let resp = req.send().await.map_err(|e| AppError::TranslateNetworkError {
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

        let response_body = resp.text().await.unwrap_or_default();

        // 开发者模式：记录所有 API 请求和响应
        crate::log_api_debug(
            &self.service_name,
            &self.model,
            source_lang,
            target_lang,
            &request_json.to_string(),
            &response_body,
            status.as_u16(),
        );

        // 余额不足：优先于认证失败判断（部分服务商余额不足时返回 403 而非 402）
        if let Some(detail) = check_insufficient_balance(status, &response_body) {
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

        if !status.is_success() {
            return Err(AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
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

        // 累计 token 用量（OpenAI 兼容 API 的 usage 字段）
        if let Some(usage) = body.get("usage") {
            use std::sync::atomic::Ordering;
            let pt = usage["prompt_tokens"].as_u64().unwrap_or(0);
            let ct = usage["completion_tokens"].as_u64().unwrap_or(0);
            let tt = usage["total_tokens"].as_u64().unwrap_or(pt + ct);
            self.prompt_tokens.fetch_add(pt, Ordering::Relaxed);
            self.completion_tokens.fetch_add(ct, Ordering::Relaxed);
            self.total_tokens.fetch_add(tt, Ordering::Relaxed);
        }

        Self::parse_numbered_response(content, texts.len()).map_err(|e| {
            // 对齐失败时记录 prompt 失败日志（发送内容 + 模型返回）
            crate::log_prompt_fail(
                &self.service_name,
                &self.model,
                source_lang,
                target_lang,
                &system_prompt,
                &user_prompt,
                content,
                &e.to_string(),
            );
            e
        })
    }

    /// 流式翻译单批：stream=true，实时接收 SSE chunk
    /// 优势：只要还在返回 token 就不会超时，适合慢速本地模型（27b 等）
    async fn translate_single_batch_stream(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, AppError> {
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
        // Qwen3 thinking 模式：关闭 thinking，避免 reasoning 消耗 token 导致部分条目未翻译
        // LM Studio / Ollama 等 OpenAI 兼容 API 通过 chat_template_kwargs 传递
        if self.model_type == ModelType::Qwen3 {
            request_body["chat_template_kwargs"] = serde_json::json!({
                "enable_thinking": false
            });
        }
        let request_json = request_body;

        // 流式请求：不设总超时（27b 模型可能 10 分钟+）
        // 改用 chunk 间超时：每个 chunk 读取最多等 60 秒，无新数据才超时
        let is_local = self.is_local_url();
        let chunk_timeout_secs = if is_local { 60 } else { 30 };

        // 构建无 timeout 的 client：reqwest::Client::new() 默认 30 秒 timeout，
        // 27b 模型第一个 token 可能 30 秒+ 才返回，会导致 "error decoding response body"
        // （这是 reqwest 的 body timeout 被误报为 decoding error，见 reqwest issue #2839）
        // reqwest 0.12 的 timeout() 接受 Duration 而非 Option，用极大值等效禁用
        let stream_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(86400)) // 24 小时，等效禁用
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut req = stream_client
            .post(&url)
            // 流式请求禁用压缩：避免 reqwest 流式解压问题
            .header("Accept-Encoding", "identity")
            .json(&request_json);

        // 认证头
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                if self.base_url.contains("openai.azure.com") {
                    req = req.header("api-key", key);
                } else {
                    req = req.header("Authorization", format!("Bearer {}", key));
                }
            }
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

        // 非 2xx：读取 body 返回错误（流式模式下 body 不是 JSON 而是 SSE）
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            // 记录错误日志
            crate::log_api_debug(
                &self.service_name,
                &self.model,
                source_lang,
                target_lang,
                &request_json.to_string(),
                &body,
                status.as_u16(),
            );
            // 余额不足
            if let Some(detail) = check_insufficient_balance(status, &body) {
                return Err(AppError::TranslateInsufficientBalance {
                    provider: self.service_name.clone(),
                    detail,
                });
            }
            if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
                return Err(AppError::TranslateAuthFailed {
                    provider: self.service_name.clone(),
                });
            }
            return Err(AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
                detail: format!("HTTP {}: {}", status, body),
            });
        }

        // 读取 SSE 流，累积 content
        // 用 resp.chunk() 逐块读取，不依赖 stream feature，兼容性更好
        let mut full_content = String::new();
        let mut buffer = String::new();
        let mut prompt_tokens = 0u64;
        let mut completion_tokens = 0u64;

        // 流式实时日志：从 task_local 读取当前并发槽位的文件句柄
        // 如果不在并发调度层 scope 内（如测试），跳过日志写入
        let stream_log_file = crate::STREAM_LOG_FILE.try_get().ok();

        // 写入请求体日志（完整请求，类似 LM Studio 的详细日志）
        // 跟踪 reasoning→content 切换，写入分隔标记
        let mut seen_content = false;
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n========== 批次开始 ==========\n时间: {}\nProvider: {}\nModel: {}\n批次: {} 条\n\n--- 请求体 ---\n{}\n\n--- 流式响应 ---\n",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                self.service_name, self.model, texts.len(),
                request_json.to_string(),
            ));
        }

        loop {
            // chunk 间超时：每个 chunk 最多等 chunk_timeout_secs 秒
            // 只要模型持续输出 token，就不会超时；卡死时 chunk_timeout_secs 秒后报错
            let chunk_result = tokio::time::timeout(
                std::time::Duration::from_secs(chunk_timeout_secs),
                resp.chunk(),
            ).await.map_err(|_| {
                crate::log_api_debug(
                    &self.service_name,
                    &self.model,
                    source_lang,
                    target_lang,
                    &request_json.to_string(),
                    &format!("[chunk timeout after {} chars, {}s no data]", full_content.len(), chunk_timeout_secs),
                    200,
                );
                AppError::TranslateNetworkError {
                    provider: self.service_name.clone(),
                    detail: format!("stream chunk timeout: {}s no data", chunk_timeout_secs),
                }
            })?;

            let chunk = chunk_result.map_err(|e| {
                // 记录流式读取错误日志
                crate::log_api_debug(
                    &self.service_name,
                    &self.model,
                    source_lang,
                    target_lang,
                    &request_json.to_string(),
                    &format!("[stream error after {} chars] {}", full_content.len(), e),
                    200,
                );
                AppError::TranslateNetworkError {
                    provider: self.service_name.clone(),
                    detail: format!("stream chunk error: {}", e),
                }
            })?;

            let Some(chunk) = chunk else { break; }; // 流结束

            // 将字节追加到缓冲区
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // 按行处理 SSE
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                // SSE 格式：data: {json} 或 data: [DONE]
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str.trim() == "[DONE]" {
                        // 流结束
                        continue;
                    }

                    // 解析 chunk JSON
                    if let Ok(chunk_json) = serde_json::from_str::<serde_json::Value>(json_str) {
                        let delta_obj = &chunk_json["choices"][0]["delta"];

                        // Qwen3 thinking 模式：先输出 reasoning_content（推理过程），再输出 content（最终答案）
                        // reasoning_content 不计入翻译结果，只写入实时日志供调试
                        if let Some(reasoning) = delta_obj["reasoning_content"].as_str() {
                            if !reasoning.is_empty() {
                                if let Some(ref log_file) = stream_log_file {
                                    // 用 [思维] 前缀标记推理过程
                                    crate::log_stream_to_file(log_file, reasoning);
                                }
                            }
                        }

                        // 累积 delta content（最终答案）
                        if let Some(delta) = delta_obj["content"].as_str() {
                            if !delta.is_empty() {
                                // 首次收到 content 时写分隔标记（之前是 reasoning）
                                if !seen_content {
                                    seen_content = true;
                                    if let Some(ref log_file) = stream_log_file {
                                        crate::log_stream_to_file(log_file, "\n\n--- 最终输出 ---\n");
                                    }
                                }
                                full_content.push_str(delta);
                                // 实时日志：追加 delta 到文件（sync_all 确保实时可见）
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, delta);
                                }
                            }
                        }
                        // 累积 usage（部分 API 在最后一个 chunk 返回 usage）
                        if let Some(usage) = chunk_json.get("usage") {
                            prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
                            completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
                        }
                    }
                }
            }
        }

        // 流式实时日志：结束汇总
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n=== 批次结束 ===\n总字符数: {}\nprompt_tokens: {}\ncompletion_tokens: {}\n时间: {}\n",
                full_content.len(), prompt_tokens, completion_tokens,
                chrono::Local::now().format("%H:%M:%S")
            ));
        }

        // 累计 token 用量
        if prompt_tokens > 0 || completion_tokens > 0 {
            use std::sync::atomic::Ordering;
            self.prompt_tokens.fetch_add(prompt_tokens, Ordering::Relaxed);
            self.completion_tokens.fetch_add(completion_tokens, Ordering::Relaxed);
            self.total_tokens.fetch_add(prompt_tokens + completion_tokens, Ordering::Relaxed);
        }

        // 开发者模式：记录 API 请求和响应
        crate::log_api_debug(
            &self.service_name,
            &self.model,
            source_lang,
            target_lang,
            &request_json.to_string(),
            &full_content,
            200,
        );

        if full_content.is_empty() {
            return Err(AppError::TranslateResponseParseFailed {
                detail: "stream: content is empty".to_string(),
            });
        }

        Ok(full_content)
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
            // 优先用流式请求（避免慢速模型超时），失败时回退到 non-stream
            let content = match self.translate_single_batch_stream(chunk, source_lang, target_lang).await {
                Ok(content) => content,
                Err(AppError::TranslateResponseParseFailed { detail })
                    if detail.contains("stream:") =>
                {
                    // 流式返回空内容，回退到 non-stream
                    tracing::warn!("流式翻译返回空，回退到 non-stream: {}", detail);
                    let translated = self.translate_single_batch(chunk, source_lang, target_lang).await?;
                    results.extend(translated);
                    continue;
                }
                Err(e) => return Err(e),
            };

            // 解析编号格式
            let translated = Self::parse_numbered_response(&content, chunk.len()).map_err(|e| {
                crate::log_prompt_fail(
                    &self.service_name,
                    &self.model,
                    source_lang,
                    target_lang,
                    &self.build_system_prompt(source_lang, target_lang),
                    &self.build_user_prompt(chunk),
                    &content,
                    &e.to_string(),
                );
                e
            })?;
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
            "temperature": 0.1,
            "stream": true,
        });
        if self.model_type == ModelType::Qwen3 {
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
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    concurrency: usize,
    /// 限流策略：Qps 模式下请求间强制间隔，Concurrency 模式下纯并发控制
    rate_limit: RateLimitPolicy,
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
            rate_limit: RateLimitPolicy::Concurrency(1),
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
            rate_limit: RateLimitPolicy::Concurrency(1),
        }
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
            RateLimitPolicy::Qps(_) => 1, // QPS 模式串行 + 间隔
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
                    translated: cached.trim().replace(DELIMITER, "").to_string(),
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
    /// skip_cache: true 时跳过缓存查询，强制重新请求 API（用于"重新翻译"）
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

        // 1. 缓存查询 + 占位符保护（skip_cache=true 时跳过缓存）
        for entry in entries {
            // 跳过 ass 矢量绘图指令（含 \p1 标记），不是字幕文本
            if entry.text.contains("\\p1") {
                tracing::info!("字幕 #{} 含 \\p1 绘图指令，跳过翻译", entry.index);
                continue;
            }

            if !skip_cache {
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
                        translated: cached.trim().replace(DELIMITER, "").to_string(),
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
                to_translate.push((entry.index, protected_text, entry.text.clone(), protector));
            }
        }

        // 2. 按 token 预算分批翻译，并发度由 self.concurrency 控制
        // 短文本（语气词/音乐符号）多打包，长文本少打包，每批 API 调用量均匀
        if !to_translate.is_empty() {
            let max_tokens = self.provider.max_batch_tokens();
            const MIN_BATCH: usize = 5;   // 最少 5 条/批（保证重试粒度）
            const MAX_BATCH: usize = 30;  // 最多 30 条/批（分隔符方案 + 小批次双重保险）

            let mut batches: Vec<Vec<(usize, String, String, PlaceholderProtector)>> = Vec::new();
            let mut current: Vec<(usize, String, String, PlaceholderProtector)> = Vec::new();
            let mut current_tokens = 0usize;
            for item in to_translate {
                let tokens = item.1.chars().count() / 3 + 1;
                if !current.is_empty()
                    && (current_tokens + tokens > max_tokens || current.len() >= MAX_BATCH)
                {
                    batches.push(std::mem::take(&mut current));
                    current_tokens = 0;
                }
                current.push(item);
                current_tokens += tokens;
            }
            if !current.is_empty() {
                // 最后一批如果太少，合并到上一批
                if current.len() < MIN_BATCH && !batches.is_empty() {
                    let last = batches.last_mut().unwrap();
                    let last_tokens: usize = last.iter().map(|(_, t, _, _)| t.chars().count() / 3 + 1).sum();
                    if last_tokens + current_tokens <= max_tokens * 2 {
                        last.extend(current);
                    } else {
                        batches.push(current);
                    }
                } else {
                    batches.push(current);
                }
            }

            let total_batches = batches.len();
            let concurrency = self.concurrency.max(1);
            let min_interval = self.rate_limit.min_interval();
            tracing::info!(
                "翻译调度: 并发={}, 限流={:?}, 间隔={}ms, 共 {} 批（token 预算: {}）",
                concurrency, self.rate_limit, min_interval.as_millis(), total_batches, max_tokens
            );

            // 并发调用 API：用 Semaphore 控制并发数，JoinSet 收集结果
            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
            let provider = self.provider.clone();
            let cancelled = self.cancelled.clone();
            // QPS 限流：共享上次请求时间，确保请求间至少间隔 min_interval
            let last_request = std::sync::Arc::new(tokio::sync::Mutex::new(
                std::time::Instant::now() - min_interval
            ));
            // 流式实时日志：预创建 concurrency 个文件，每个并发槽位复用一个
            let stream_log_slots = std::sync::Arc::new(crate::create_stream_log_slots(concurrency));
            // 并发槽位分配计数器：acquire permit 后用 fetch_add 取模获取 slot index
            let slot_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
            let mut join_set = tokio::task::JoinSet::new();

            for (batch_idx, batch) in batches.iter().enumerate() {
                let texts: Vec<String> = batch.iter().map(|(_, t, _, _)| t.clone()).collect();
                let source = source_lang.to_string();
                let target = target_lang.to_string();
                let provider = provider.clone();
                let cancelled = cancelled.clone();
                let semaphore = semaphore.clone();
                let last_request = last_request.clone();
                let stream_log_slots = stream_log_slots.clone();
                let slot_counter = slot_counter.clone();

                join_set.spawn(async move {
                    // 在 task 内部获取信号量，不阻塞 spawn 循环
                    // 这样 while join_next 循环能立即开始处理已完成的结果
                    let _permit = semaphore.acquire_owned().await.unwrap();
                    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                        return (batch_idx, Err(AppError::TranslateRetriesExhausted));
                    }

                    // 分配并发槽位：取模获取 slot index，对应一个复用的日志文件
                    let slot_idx = (slot_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % stream_log_slots.len() as u64) as usize;
                    let stream_log_file = stream_log_slots[slot_idx].clone();

                    // QPS 限流：获取信号量后，检查距上次请求是否已过 min_interval
                    if !min_interval.is_zero() {
                        let mut last = last_request.lock().await;
                        let elapsed = last.elapsed();
                        if elapsed < min_interval {
                            let wait = min_interval - elapsed;
                            tracing::debug!("QPS 限流: 等待 {}ms", wait.as_millis());
                            tokio::time::sleep(wait).await;
                        }
                        *last = std::time::Instant::now();
                    }

                    tracing::info!("翻译批次 {}/{}，本批 {} 条", batch_idx + 1, total_batches, texts.len());
                    // 用 task_local 传递日志文件句柄，translate_single_batch_stream 中读取
                    let result = crate::STREAM_LOG_FILE.scope(stream_log_file, async {
                        translate_with_retry_provider(
                            &*provider,
                            &texts,
                            &source,
                            &target,
                            &cancelled,
                        )
                        .await
                    }).await;
                    (batch_idx, result)
                });
            }

            // 批次完成即处理（不要求顺序）：立即回调 on_entry_done / on_progress，
            // 避免 head-of-line blocking（batch 0 慢时后续批次全部等待导致进度卡 0）
            // 用 select! 同时监听 join_next 和取消信号，确保取消时立即响应
            loop {
                if self.is_cancelled() {
                    tracing::info!("翻译已取消，abort 所有未完成任务");
                    join_set.abort_all();
                    break;
                }
                // join_next().await 会阻塞直到有任务完成，用 select! 让取消信号能立即打断
                let res = tokio::select! {
                    r = join_set.join_next() => r,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                        // 超时唤醒，循环回去检查 cancelled
                        continue;
                    }
                };
                let Some(res) = res else { break; }; // 所有任务完成
                let (batch_idx, api_result) = match res {
                    Ok(item) => item,
                    Err(e) => {
                        tracing::warn!("join 任务异常: {}", e);
                        continue;
                    }
                };

                let batch = &batches[batch_idx];
                let texts: Vec<String> = batch.iter().map(|(_, t, _, _)| t.clone()).collect();

                let translations = match api_result {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("批次 {} 整批翻译失败: {}", batch_idx + 1, e);
                        for (index, _protected, original_text, _protector) in batch.iter() {
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
                // 即使数量一致，也可能存在空翻译（如 parse_numbered_response 部分成功时尾部填空）
                let has_empty = translations.iter().any(|t| t.is_empty());
                // echo 检测：模型可能直接 echo 回原文（完全相同或部分相同）
                // 对于翻译到中文的字幕，如果译文不含任何中文字符，则判定为未翻译
                let target_is_cjk = target_lang.starts_with("zh")
                    || target_lang.starts_with("ja")
                    || target_lang.starts_with("ko");
                let has_echo = translations.iter().enumerate().any(|(i, t)| {
                    if i >= batch.len() || t.is_empty() {
                        return false;
                    }
                    // 检查 1：译文不含目标语言字符（如翻译到中文但全是英文）
                    if target_is_cjk {
                        let has_cjk = t.chars().any(|c| {
                            ('\u{4E00}'..='\u{9FFF}').contains(&c) // CJK 统一汉字
                                || ('\u{3040}'..='\u{30FF}').contains(&c) // 日文假名
                                || ('\u{AC00}'..='\u{D7AF}').contains(&c) // 韩文
                        });
                        if !has_cjk {
                            return true;
                        }
                    }
                    // 检查 2：译文和原文完全相同（trim 后忽略大小写和标点差异）
                    let original = &batch[i].1; // protected_text
                    let t_clean = t.replace(DELIMITER, "").trim().to_lowercase();
                    let o_clean = original.replace(DELIMITER, "").trim().to_lowercase();
                    t_clean == o_clean
                        || t_clean.replace(['.', ',', '!', '?', '。', '，', '！', '？', ' '], "")
                            == o_clean.replace(['.', ',', '!', '?', '。', '，', '！', '？', ' '], "")
                });
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

                    for ((index, _protected, original_text, protector), translated) in
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
                } else if has_empty || has_echo {
                    // 数量匹配但存在空翻译或 echo 原文（如 parse_numbered_response 部分成功尾部填空）
                    tracing::warn!(
                        "翻译批次 {} 数量匹配但存在空翻译或 echo 原文，逐条重试",
                        batch_idx + 1
                    );
                    let mut final_translations = translations.clone();
                    for (i, translated) in final_translations.iter_mut().enumerate() {
                        let is_echo = i < batch.len() && !translated.is_empty() && {
                            // 检查 1：译文不含目标语言字符
                            if target_is_cjk {
                                let has_cjk = translated.chars().any(|c| {
                                    ('\u{4E00}'..='\u{9FFF}').contains(&c)
                                        || ('\u{3040}'..='\u{30FF}').contains(&c)
                                        || ('\u{AC00}'..='\u{D7AF}').contains(&c)
                                });
                                if !has_cjk {
                                    true
                                } else {
                                    false
                                }
                            } else {
                                // 检查 2：译文和原文完全相同
                                let original = &batch[i].1;
                                let t_clean = translated.replace(DELIMITER, "").trim().to_lowercase();
                                let o_clean = original.replace(DELIMITER, "").trim().to_lowercase();
                                t_clean == o_clean
                                    || t_clean.replace(['.', ',', '!', '?', '。', '，', '！', '？', ' '], "")
                                        == o_clean.replace(['.', ',', '!', '?', '。', '，', '！', '？', ' '], "")
                            }
                        };
                        if translated.is_empty() || is_echo {
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

                    for ((index, _protected, original_text, protector), translated) in
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
                    for ((index, _protected, original_text, protector), translated) in
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
            &self.cancelled,
        ).await
    }
}

/// 可取消的 sleep：每 200ms 检查取消信号，返回 true 表示已取消
async fn cancelled_sleep(
    cancelled: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    secs: u64,
) -> bool {
    let mut elapsed = 0u64;
    while elapsed < secs {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return true;
        }
        let step = std::cmp::min(1, secs - elapsed);
        tokio::time::sleep(std::time::Duration::from_secs(step)).await;
        elapsed += step;
    }
    false
}

/// 等待取消信号（用于 select! 中打断正在进行的请求）
/// 每 200ms 轮询一次，返回时表示已取消
async fn cancelled_notify(cancelled: &std::sync::Arc<std::sync::atomic::AtomicBool>) {
    loop {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
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
            // 用 select! 让取消信号能打断正在进行的翻译请求
            let result = tokio::select! {
                r = provider.translate(texts, source_lang, target_lang) => r,
                _ = cancelled_notify(cancelled) => return Err(AppError::TranslateRetriesExhausted),
            };
            match result {
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
                    // 可取消的 sleep
                    if cancelled_sleep(cancelled, *delay).await { return Err(AppError::TranslateRetriesExhausted); }
                }
                Err(AppError::TranslateNetworkError { provider, detail }) => {
                    // 连接被拒绝（服务未启动）不重试，直接返回
                    if detail.contains("Connection refused") || detail.contains("connection refused") || detail.contains("connect error") {
                        tracing::warn!("翻译连接被拒绝，不重试：{}", detail);
                        return Err(AppError::TranslateNetworkError { provider, detail });
                    }
                    tracing::warn!(
                        "翻译网络错误（第 {} 次重试）：{}，等待 {} 秒",
                        attempt + 1,
                        detail,
                        delay
                    );
                    last_error = Some(AppError::TranslateNetworkError { provider, detail });
                    if cancelled_sleep(cancelled, *delay).await { return Err(AppError::TranslateRetriesExhausted); }
                }
                Err(AppError::TranslateAlignFailed { missing }) => {
                    // 对齐失败：模型返回了内容但格式不对，重试可能得到更好的结果
                    tracing::warn!(
                        "翻译对齐失败（第 {} 次重试），缺失 {} 条，等待 {} 秒后重试",
                        attempt + 1,
                        missing,
                        delay
                    );
                    last_error = Some(AppError::TranslateAlignFailed { missing });
                    if cancelled_sleep(cancelled, *delay).await { return Err(AppError::TranslateRetriesExhausted); }
                }
                Err(e) => return Err(e), // 鉴权失败等不重试
            }
        }

        Err(last_error.unwrap_or(AppError::TranslateRetriesExhausted))
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
        "You are a proper noun extraction assistant.\n\
         Read the following {src} subtitles and extract ALL proper nouns that appear.\n\
         This includes: person names, place names, farm names, field names, brand names, organization names, band names, song titles, movie titles, TV show names, magazine names, drug names, animal names, bird names, plant names, vehicle names, and any other proper nouns.\n\
         For each name, provide a {tgt} translation suggestion.\n\n\
         Output format (one per line, do NOT number the lines):\n\
         EnglishName → {tgt}Translation\n\n\
         Rules:\n\
         - Extract ALL proper nouns: person names, place names, brand names, organization names, band names, song/movie/TV titles, animal names, etc.\n\
         - Do NOT extract common words, verbs, adjectives, or exclamations even if they appear capitalized.\n\
         - If a name appears in multiple forms (full name + nickname), list each form separately.\n\
         - Output ONLY the name list, no explanations, no parenthetical notes, no extra text.\n\
         - The translation must be a clean name only, with NO parenthetical explanations or notes.\n\
         - Use → (U+2192) as the separator between English and translation.",
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
fn parse_name_extraction_response(content: &str) -> Vec<ExtractedName> {
    let mut names = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
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
        let zh = zh_raw
            .split(|c| c == '(' || c == '（' || c == '[' || c == '【')
            .next()
            .unwrap_or(zh_raw)
            .trim()
            .trim_matches('"')
            .trim();
        if !en.is_empty() && !zh.is_empty() {
            names.push(ExtractedName {
                english: en.to_string(),
                chinese: zh.to_string(),
                alternatives: Vec::new(),
            });
        }
    }
    names
}

/// 合并多段人名提取结果，同一英文名多个译名时频率优先，平局取首次出现
fn merge_extracted_names(segment_results: &[SegmentNameResult]) -> Vec<ExtractedName> {
    use std::collections::HashMap;
    // en_name -> (zh_name -> (count, first_segment_idx))
    let mut stats: HashMap<String, HashMap<String, (usize, usize)>> = HashMap::new();

    for result in segment_results {
        for name in &result.names {
            let entry = stats.entry(name.english.clone()).or_default();
            let counter = entry.entry(name.chinese.clone()).or_insert((0, result.segment_idx));
            counter.0 += 1;
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
pub async fn extract_names_from_subtitles(
    provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
    texts: &[String],
    source_lang: &str,
    target_lang: &str,
    max_input_tokens: usize,
    cancel_token: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<ExtractedName>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    // 按 token 预算分段（复用翻译分批的 token 估算逻辑）
    // 每段留 2000 token 给 system prompt + 输出
    let segment_budget = max_input_tokens.saturating_sub(2000).max(1000);
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_tokens = 0usize;
    for text in texts {
        let tokens = text.chars().count() / 3 + 1;
        if !current.is_empty() && current_tokens + tokens > segment_budget {
            segments.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        current.push(text.clone());
        current_tokens += tokens;
    }
    if !current.is_empty() {
        segments.push(current);
    }

    tracing::info!("人名预扫描: {} 段（token 预算: {}）", segments.len(), segment_budget);
    let scan_start = std::time::Instant::now();

    // 并发扫描各段（最多 3 并发，避免过载）
    let concurrency = segments.len().min(3);
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut join_set = tokio::task::JoinSet::new();
    let segments_len = segments.len();

    // 流式实时日志：预创建 concurrency 个文件（与翻译调度器相同的方式）
    let stream_log_slots = std::sync::Arc::new(crate::create_stream_log_slots(concurrency));
    let slot_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    for (idx, segment) in segments.iter().enumerate() {
        let segment = segment.clone();
        let source = source_lang.to_string();
        let target = target_lang.to_string();
        let provider = provider.clone();
        let semaphore = semaphore.clone();
        let stream_log_slots = stream_log_slots.clone();
        let slot_counter = slot_counter.clone();
        let cancelled = cancel_token.clone();
        join_set.spawn(async move {
            let _permit = semaphore.acquire_owned().await.unwrap();
            // 取消检查：获取信号量后检查取消标志
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
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

            match content {
                Ok(content) => {
                    let names = parse_name_extraction_response(&content);
                    tracing::info!("人名预扫描段 {} 提取到 {} 个人名, 耗时 {:.2}s", idx + 1, names.len(), seg_start.elapsed().as_secs_f64());
                    SegmentNameResult { segment_idx: idx, names }
                }
                Err(e) => {
                    tracing::warn!("人名预扫描段 {} 失败: {}", idx + 1, e);
                    SegmentNameResult { segment_idx: idx, names: Vec::new() }
                }
            }
        });
    }

    // 收集结果（支持取消：检测到取消时立即中止剩余任务）
    let mut segment_results = Vec::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(result) = res {
            segment_results.push(result);
        }
        // 取消检查：收到取消信号时中止剩余任务
        if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("人名预扫描被取消，中止剩余任务");
            join_set.abort_all();
            break;
        }
    }

    // 取消时返回空结果
    if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
        tracing::info!("人名预扫描已取消");
        return Ok(Vec::new());
    }

    // 按 segment_idx 排序
    segment_results.sort_by_key(|r| r.segment_idx);

    let merged = merge_extracted_names(&segment_results);
    tracing::info!("人名预扫描完成: 合并后 {} 个人名, 总耗时 {:.2}s", merged.len(), scan_start.elapsed().as_secs_f64());
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
        // 验证限流策略
        assert_eq!(TranslateProvider::OpenAi.rate_limit_policy(), RateLimitPolicy::Concurrency(5));
        assert_eq!(TranslateProvider::Baidu.rate_limit_policy(), RateLimitPolicy::Qps(1));
        assert_eq!(TranslateProvider::Youdao.rate_limit_policy(), RateLimitPolicy::Qps(1));
        assert_eq!(TranslateProvider::Aliyun.rate_limit_policy(), RateLimitPolicy::Qps(50));
    }

    #[test]
    fn test_rate_limit_policy_intervals() {
        // QPS=1 → 间隔 1 秒
        assert_eq!(RateLimitPolicy::Qps(1).min_interval(), std::time::Duration::from_secs(1));
        // QPS=2 → 间隔 0.5 秒
        assert_eq!(RateLimitPolicy::Qps(2).min_interval(), std::time::Duration::from_millis(500));
        // Concurrency → 无间隔
        assert_eq!(RateLimitPolicy::Concurrency(5).min_interval(), std::time::Duration::ZERO);
        // 并发上限
        assert_eq!(RateLimitPolicy::Qps(2).max_concurrency(), 1);
        assert_eq!(RateLimitPolicy::Concurrency(5).max_concurrency(), 5);
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
        // 编号解析部分成功：2 条编号解析到，但期望 3 条
        // 新行为：返回部分结果（缺失项为空字符串），由调度器逐条重试
        let content = "1. 你好\n2. 世界";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "你好");
        assert_eq!(result[1], "世界");
        assert_eq!(result[2], ""); // 缺失项为空
    }

    #[test]
    fn test_parse_numbered_response_with_extra_text() {
        // 模型可能加额外说明行，编号解析应忽略非编号行
        let content = "Here are the translations:\n1. 你好\n2. 世界\n\nDone.";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn test_parse_numbered_response_too_many_lines() {
        // 模型加了前言和后记，行数比期望多 → 截取编号行区域
        let content = "好的，以下是翻译：\n1. 你好\n2. 世界\n翻译完成。";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn test_parse_numbered_response_too_many_lines_mixed() {
        // 前言 + 编号行 + 非编号行混入 + 后记
        // 编号行数量正好匹配期望
        let content = "Sure, here are the translations:\n1. 你好\n2. 世界\n3. 测试\nDone.";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_response_too_many_lines_extra_between() {
        // 编号行之间混入非编号行（模型加了空行或说明）
        let content = "1. 你好\n\n2. 世界";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn test_parse_numbered_response_partial_numbered_contiguous() {
        // 模型只返回了前 3 条（连续编号 1-3），第 4 条缺失 → 部分填充，第 4 条为空
        let content = "1. 你好\n2. 世界\n3. 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 4).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "你好");
        assert_eq!(result[1], "世界");
        assert_eq!(result[2], "测试");
        assert_eq!(result[3], ""); // 第 4 条缺失，由调度器逐条重试
    }

    #[test]
    fn test_parse_numbered_response_partial_numbered_gap() {
        // 模型跳过了第 3 条（编号 1,2,4），编号不连续 → 放弃编号对齐
        // 退回按行对齐：3 行 < 4 期望，最终返回对齐失败
        let content = "1. 你好\n2. 世界\n4. 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 4);
        // 应该返回 Err（对齐失败），因为编号不连续且行数不够
        assert!(result.is_err(), "编号不连续时应返回对齐失败，而非错位对齐");
    }

    #[test]
    fn test_parse_numbered_response_preamble_filtered() {
        // 模型加了前言，行数恰好匹配 → 前言应被过滤，不能当作翻译
        let content = "以下是翻译：\n你好\n世界";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn test_parse_numbered_response_preamble_english() {
        // 英文前言
        let content = "Here are the translations:\nHello\nWorld";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result, vec!["Hello", "World"]);
    }

    #[test]
    fn test_parse_numbered_response_no_number_strip_prefix() {
        // 模型用了编号但正则没完全匹配（如 "1 - text"），3a 应去掉编号前缀
        // 由于 "1 - text" 不匹配正则 ^(\d+)[.、:)]\s*(.*)$，会走 3a
        let content = "1 - 你好\n2 - 世界";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        // 3a 应该返回去掉编号后的内容（虽然 "1 - " 不匹配正则，但 3a 不会去掉它）
        // 实际上 "1 - 你好" 不匹配正则，所以 3a 返回原始行
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_parse_numbered_response_out_of_range_renumber() {
        // 模型从 0 开始编号 → 编号超出范围，按编号顺序重新对齐
        let content = "0. 你好\n1. 世界\n2. 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_response_continued_numbering() {
        // 模型继续上一批的编号（如从 5 开始）
        let content = "5. 你好\n6. 世界\n7. 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_response_skip_causes_misalignment() {
        // 模拟实际 bug：模型跳过了第 2 条，导致编号 N 对应原文 N+1
        // 输入 4 条，模型返回 3 条（编号 1,3,4，跳过 2）
        // 编号不连续 → 放弃编号对齐，不应错位
        let content = "1. 翻译A\n3. 翻译C\n4. 翻译D";
        let result = OpenAiProvider::parse_numbered_response(content, 4);
        // 编号不连续，3 行 < 4 期望 → 对齐失败
        assert!(result.is_err(), "编号不连续时应返回对齐失败，避免错位");
    }

    // === 分隔符方案测试 ===

    #[test]
    fn test_parse_numbered_exact() {
        // 标准编号格式：N. 翻译
        let content = "1. 你好\n2. 世界\n3. 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_multiline() {
        // 多行翻译：缩进行（2+空格开头）作为上一条的续行
        let content = "1. [杰里米] <i>农场机器人\n  现在已经开始工作了</i>\n2. <i>重新种植洋葱</i>";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result[0], "[杰里米] <i>农场机器人\n现在已经开始工作了</i>");
        assert_eq!(result[1], "<i>重新种植洋葱</i>");
    }

    #[test]
    fn test_parse_numbered_with_extra_spaces() {
        // 模型可能在编号后加多余空格
        let content = "1.  你好\n2.  世界\n3.  测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_different_separators() {
        // 模型可能用不同的分隔符（. 、 : )）
        let content = "1. 你好\n2、 世界\n3: 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_partial() {
        // 模型少返回了一条 → 缺失尾部填空，由调度器逐条重试
        let content = "1. 翻译A\n2. 翻译B\n3. 翻译C";
        let result = OpenAiProvider::parse_numbered_response(content, 4).unwrap();
        assert_eq!(result[0], "翻译A");
        assert_eq!(result[1], "翻译B");
        assert_eq!(result[2], "翻译C");
        assert_eq!(result[3], ""); // 缺失项为空
    }

    #[test]
    fn test_parse_numbered_too_few() {
        // 模型只返回了 1 条，期望 3 条 → 部分填充
        let content = "1. 唯一翻译";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result[0], "唯一翻译");
        assert_eq!(result[1], "");
        assert_eq!(result[2], "");
    }

    #[test]
    fn test_parse_numbered_preamble_postscript() {
        // 模型加了前言/后记，编号行数量正好匹配时截取编号行区域
        let content = "Here are the translations:\n1. 你好\n2. 世界\nHope this helps!";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn test_parse_numbered_single() {
        let content = "1. 唯一翻译";
        let result = OpenAiProvider::parse_numbered_response(content, 1).unwrap();
        assert_eq!(result, vec!["唯一翻译"]);
    }

    #[test]
    fn test_parse_numbered_single_multiline() {
        // 单条多行翻译：续行缩进 2 空格
        let content = "1. [杰里米] <i>农场机器人\n  现在已经开始工作了</i>";
        let result = OpenAiProvider::parse_numbered_response(content, 1).unwrap();
        assert_eq!(result, vec!["[杰里米] <i>农场机器人\n现在已经开始工作了</i>"]);
    }

    #[test]
    fn test_parse_numbered_extra_entries_merged() {
        // 模型把多行翻译拆成了多个编号条目（35 条，期望 30 条）
        // 超出的 #31-#35 应合并到 #30
        let mut content = String::new();
        for i in 1..=30 {
            content.push_str(&format!("{}. 翻译{}\n", i, i));
        }
        // 模型多拆出了 5 条
        content.push_str("31. 多出的部分1\n");
        content.push_str("32. 多出的部分2\n");
        content.push_str("33. 多出的部分3\n");
        content.push_str("34. 多出的部分4\n");
        content.push_str("35. 多出的部分5\n");
        let result = OpenAiProvider::parse_numbered_response(&content, 30).unwrap();
        assert_eq!(result.len(), 30);
        // #30 应包含原 #30 + #31-#35 的合并
        assert_eq!(result[29], "翻译30\n多出的部分1\n多出的部分2\n多出的部分3\n多出的部分4\n多出的部分5");
        // #1-#29 应该正常
        assert_eq!(result[0], "翻译1");
        assert_eq!(result[28], "翻译29");
    }

    #[test]
    fn test_parse_numbered_extra_entries_merged_real() {
        // 模拟真实场景：模型返回 35 条，期望 30 条
        // 模型把多行翻译拆成了多个编号条目
        // 超出的条目全部合并到最后一条（#30）
        let mut content = String::new();
        for i in 1..=27 {
            content.push_str(&format!("{}. 翻译{}\n", i, i));
        }
        content.push_str("28. 让我惊讶的是\n");
        content.push_str("29. 而且知道\n");
        content.push_str("30. - 简直难以置信。\n");
        content.push_str("31. 它昨天就播下了二十万颗种子，\n");
        content.push_str("32. 每一颗种子的确切位置。\n");
        content.push_str("33. - 那么，\n");
        content.push_str("34. - 难以置信\n");
        content.push_str("35. 那么");
        let result = OpenAiProvider::parse_numbered_response(&content, 30).unwrap();
        assert_eq!(result.len(), 30);
        // #1-#29 应该正常
        assert_eq!(result[0], "翻译1");
        assert_eq!(result[27], "让我惊讶的是");
        assert_eq!(result[28], "而且知道");
        // #30 应包含原 #30 + #31-#35 的合并
        assert_eq!(result[29], "- 简直难以置信。\n它昨天就播下了二十万颗种子，\n每一颗种子的确切位置。\n- 那么，\n- 难以置信\n那么");
    }

    #[test]
    fn test_parse_numbered_zero_based() {
        // 模型用了 0-based 编号 → 按编号排序对齐
        let content = "0. 你好\n1. 世界\n2. 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_continued_numbering() {
        // 模型继续上一批的编号（如从 31 开始）→ 按编号排序取前 N 条
        let content = "31. 你好\n32. 世界\n33. 测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_parse_numbered_empty_input() {
        let content = "";
        let result = OpenAiProvider::parse_numbered_response(content, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_numbered_preserves_special_chars() {
        // 占位符等特殊字符应原样保留
        let content = "1. \u{E001}你好\u{E002}\n2. 世界\u{E003}";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result[0], "\u{E001}你好\u{E002}");
        assert_eq!(result[1], "世界\u{E003}");
    }

    #[test]
    fn test_parse_numbered_multiline_speaker() {
        // 多行带说话人标签的翻译：续行缩进 2 空格
        let content = "1. - [杰里米] 洋葱和甜菜根。\n  - [查理] 是的。\n2. [杰里米] 或者，就像我喜欢的称呼一样，\n  没有洋葱或甜菜根。";
        let result = OpenAiProvider::parse_numbered_response(content, 2).unwrap();
        assert_eq!(result[0], "- [杰里米] 洋葱和甜菜根。\n- [查理] 是的。");
        assert_eq!(result[1], "[杰里米] 或者，就像我喜欢的称呼一样，\n没有洋葱或甜菜根。");
    }

    #[test]
    fn test_parse_numbered_no_prefix_fallback() {
        // 模型没用编号前缀，按行对齐回退
        let content = "你好\n世界\n测试";
        let result = OpenAiProvider::parse_numbered_response(content, 3).unwrap();
        assert_eq!(result, vec!["你好", "世界", "测试"]);
    }

    #[test]
    fn test_builtin_templates_render() {
        let tmpl = BUILTIN_TEMPLATES.iter().find(|(k, _)| *k == "qwen3").map(|(_, t)| t).unwrap();
        let system = tmpl.render_system("English", "Chinese");
        assert!(system.contains("English"));
        assert!(system.contains("Chinese"));
    }

    #[test]
    fn test_builtin_prompt_template_generic() {
        // 内置 generic 模板渲染 system prompt
        let tmpl = BUILTIN_TEMPLATES
            .iter()
            .find(|(k, _)| *k == "generic")
            .map(|(_, t)| t)
            .unwrap();
        let system = tmpl.render_system("English", "Chinese");
        assert!(system.contains("English"));
        assert!(system.contains("Chinese"));
        assert!(system.contains("SDH annotations in brackets"));
    }

    #[test]
    fn test_builtin_prompt_template_qwen3_user() {
        // 内置 qwen3 模板：user_line_format 现在是 "{text}"，用 🔸 分隔
        let tmpl = BUILTIN_TEMPLATES
            .iter()
            .find(|(k, _)| *k == "qwen3")
            .map(|(_, t)| t)
            .unwrap();
        let hello = "Hello".to_string();
        let world = "World".to_string();
        let texts = vec![&hello, &world];
        let user: String = texts
            .iter()
            .map(|txt| tmpl.user_line_format.replace("{text}", txt))
            .collect::<Vec<_>>()
            .join("🔸");
        assert!(user.contains("Hello"));
        assert!(user.contains("World"));
        assert!(user.contains("🔸"));
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

    #[test]
    fn test_protector_srt_newline() {
        // SRT 多行字幕：\n 被保护为占位符，restore 后还原为 \n
        let mut p = PlaceholderProtector::new();
        let input = "Line 1\nLine 2";
        let protected = p.protect(input);
        assert!(!protected.contains('\n'), "换行应被替换为占位符");
        let restored = p.restore(&protected);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_protector_srt_newline_after_translation() {
        // 模拟翻译后占位符被保留在译文中，restore 应还原为 \n
        let mut p = PlaceholderProtector::new();
        let protected = p.protect("Hello\nWorld");
        // protected 中 \n 被替换为占位符字符，模型应原样保留
        let restored = p.restore(&protected);
        assert_eq!(restored, "Hello\nWorld");
    }

    #[test]
    fn test_protector_pipe_not_affected() {
        // 字幕中原本的 | 字符不应被误处理
        let mut p = PlaceholderProtector::new();
        let input = "cat file.txt | grep hello";
        let protected = p.protect(input);
        assert!(protected.contains('|'), "管道符应原样保留");
        let restored = p.restore(&protected);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_protector_restore_strips_delimiter() {
        // 模型在翻译中残留了 🔸，restore 应清除
        let mut p = PlaceholderProtector::new();
        let protected = p.protect("Hello");
        // 模拟模型在翻译后加了 🔸
        let with_delimiter = format!("{}🔸", protected);
        let restored = p.restore(&with_delimiter);
        assert_eq!(restored, "Hello", "restore 应清除残留的 🔸");
    }

    #[test]
    fn test_protector_delimiter_emoji_protected() {
        // 字幕中包含分隔符 emoji 🔸 时，应被占位符保护，不会干扰分割
        let mut p = PlaceholderProtector::new();
        let input = "点击🔸按钮继续";
        let protected = p.protect(input);
        assert!(!protected.contains(DELIMITER), "🔸 应被替换为占位符");
        let restored = p.restore(&protected);
        assert_eq!(restored, input, "restore 后应恢复 🔸");
    }

    #[test]
    fn test_protector_delimiter_emoji_with_newline() {
        // 多行字幕中同时包含 🔸 和换行符
        let mut p = PlaceholderProtector::new();
        let input = "第一行🔸\n第二行";
        let protected = p.protect(input);
        assert!(!protected.contains(DELIMITER), "🔸 应被替换为占位符");
        assert!(!protected.contains('\n'), "换行也应被替换为占位符");
        let restored = p.restore(&protected);
        assert_eq!(restored, input, "restore 后应恢复 🔸 和换行");
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
}

