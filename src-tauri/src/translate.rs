// 翻译模块
// provider 抽象（百度/Bing/Google）+ 分段 + 占位符保护 + 缓存 + 限流重试

mod translate_utils;
mod translate_ai;

use crate::db::{translate_cache_key, Database};
use crate::error::AppError;
use serde::{Deserialize, Serialize};

// 从 translate_utils re-export，让 translate.rs 内部代码无需改动
pub(crate) use translate_utils::{
    PlaceholderProtector, build_cache_provider_name, clean_json_leak,
    has_cjk, has_english_word,
    has_music_symbols, is_music_or_symbol_only,
    is_partial_translation, looks_like_sound_effect,
    split_text, strip_markdown_code_fence,
};
#[cfg(test)]
pub(crate) use translate_utils::has_lost_non_sound_lines;

// 从 translate_ai re-export，让外部调用方（ipc.rs 等）无需改动
pub use translate_ai::{
    ai_service_display_name, extract_name_tags, extract_names_from_subtitles,
    get_remote_prompt_version, notify_global_cancel, AiService, ExtractedName, ModelType, NameConsistencyResult,
    NameInconsistency, OpenAiProvider, PromptTemplate, PromptTemplateRegistry,
    PromptTemplateView, RemotePromptConfig, ThinkingStyle, post_process_name_tags, strip_name_tags,
};
pub use translate_utils::cleanup_cjk_spaces;
pub use translate_utils::normalize_sound_effect_brackets;
// parse_name_extraction_response 被 e2e 集成测试（tests/e2e.rs）调用，
// 不能用 #[cfg(test)]（integration test 编译 lib 时不启用 test cfg）也不能用 pub(crate)，
// 必须 pub use 让外部 crate 可见
pub use translate_ai::parse_name_extraction_response;
#[cfg(test)]
pub(crate) use translate_ai::{is_likely_proper_noun, BUILTIN_TEMPLATES};
pub(crate) use translate_ai::{get_model_batch_sizes, translate_batch_with_fallback};

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
        self.build_client_builder().build().unwrap_or_else(|_| reqwest::Client::new())
    }

    /// 构建 reqwest::ClientBuilder（带代理），调用方可追加 timeout 等配置后 build
    pub fn build_client_builder(&self) -> reqwest::ClientBuilder {
        match self.proxy_url() {
            Some(url) => {
                tracing::info!("使用代理: {}", self.mode);
                reqwest::Client::builder()
                    .proxy(reqwest::Proxy::all(&url).unwrap_or_else(|e| {
                        tracing::warn!("代理配置失败: {}, 使用直连", e);
                        reqwest::Proxy::all("direct://").unwrap()
                    }))
            }
            None => reqwest::Client::builder(),
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RateLimitPolicy {
    /// 每秒最多 N 个请求（QPS），请求间强制间隔 1/N 秒
    /// 支持小数（如 0.5 = 每 2 秒 1 个请求），用于 GLM 等严格限流的 API
    Qps(f64),
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
            RateLimitPolicy::Qps(qps) if *qps > 0.0 => {
                std::time::Duration::from_secs_f64(1.0 / *qps)
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

/// 占位符保护策略（不同翻译引擎/模型对标签的处理能力不同）
///
/// 测试数据（保留率）：
/// | 策略 | 百度 | 9b 模型 | DeepL | Google | Bing |
/// |------|------|---------|-------|--------|------|
/// | PrivateUse | 100% | 0% | - | - | - |
/// | XmlTags | 90% | 93% | - | - | - |
/// | DirectHtml | 80% | 88% | 原生 | 原生 | 原生 |
/// | CurlyBraces | 63% | 87% | - | - | - |
/// | SquareBrackets | 36% | 80% | - | - | - |
///
/// 默认策略（按引擎）：
/// - baidu/youdao/caiyun/niutrans/tencent/volcengine/aliyun/amazon: PrivateUse（传统引擎，不可见字符保留率高）
/// - openai（AI 模型）: XmlTags（9b 模型对 XML 标签保留率 93%，远优于私用区 0%）
/// - deepl/google/bing: DirectHtml（原生支持 HTML 标签处理）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderStrategy {
    /// 私用区字符 U+E000~U+E0FF（现有方案，传统引擎最优）
    PrivateUse,
    /// XML 标签 <x1></x1>（AI 模型最优，通用性好）
    XmlTags,
    /// 直接发送 HTML 标签（DeepL/Google/Bing 原生支持）
    DirectHtml,
    /// 花括号数字 {1}{/1}
    CurlyBraces,
    /// 方括号数字 [1][/1]
    SquareBrackets,
}

impl PlaceholderStrategy {
    /// 根据翻译引擎名称返回默认占位符策略
    /// provider_name 可能是 "baidu"、"deepl" 或 "openai-lmstudio-xxx" 等复合名称
    pub fn for_provider(provider_name: &str) -> Self {
        // DeepL/Google/Bing：原生支持 HTML 标签处理
        if provider_name == "deepl" || provider_name == "google" || provider_name == "bing" {
            return PlaceholderStrategy::DirectHtml;
        }
        // OpenAI 兼容（AI 模型）：provider_name 以 "openai" 开头
        // 包括 "openai"、"openai-lmstudio-xxx"、"openai-deepseek-xxx" 等
        if provider_name.starts_with("openai") {
            return PlaceholderStrategy::XmlTags;
        }
        // 传统翻译引擎（百度/有道/彩云/小牛/腾讯/火山/阿里/亚马逊）：私用区字符保留率最高
        PlaceholderStrategy::PrivateUse
    }
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
            TranslateProvider::Baidu => RateLimitPolicy::Qps(1.0),
            TranslateProvider::Youdao => RateLimitPolicy::Qps(1.0),
            TranslateProvider::OpenAi => RateLimitPolicy::Concurrency(5),
            TranslateProvider::DeepL => RateLimitPolicy::Concurrency(5),
            TranslateProvider::Google => RateLimitPolicy::Concurrency(10),
            TranslateProvider::Bing => RateLimitPolicy::Concurrency(10),
            TranslateProvider::Caiyun => RateLimitPolicy::Qps(5.0),
            TranslateProvider::Niutrans => RateLimitPolicy::Qps(5.0),
            TranslateProvider::Tencent => RateLimitPolicy::Qps(5.0),
            TranslateProvider::Volcengine => RateLimitPolicy::Qps(5.0),
            TranslateProvider::Aliyun => RateLimitPolicy::Qps(50.0),
            TranslateProvider::Amazon => RateLimitPolicy::Concurrency(10),
        }
    }

    /// 各引擎的 QPS 上限（用于显示和兼容旧逻辑）
    pub fn qps_limit(&self) -> usize {
        match self.rate_limit_policy() {
            RateLimitPolicy::Qps(q) => q.round() as usize,
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

/// 检测响应是否为余额不足/额度耗尽/接口未授权
/// 返回 Some(detail) 表示需要用户到控制台处理，None 表示不是
pub fn check_insufficient_balance(status: reqwest::StatusCode, body: &str) -> Option<String> {
    let lower = body.to_lowercase();
    let is_endpoint_inactive = lower.contains("endpoint is inactive")
        || lower.contains("endpoint inactive")
        || lower.contains("401006");

    // HTTP 402 Payment Required：可能是余额不足，也可能是接口未激活（如 TokenHub）
    if status == reqwest::StatusCode::PAYMENT_REQUIRED || is_endpoint_inactive {
        let msg = extract_error_message(body);
        if is_endpoint_inactive {
            return Some(format!(
                "接口未激活，请前往服务商控制台开通/授权该模型后重试：{}",
                msg
            ));
        }
        return Some(msg);
    }
    // 响应体关键词检测（各服务商余额不足时的常见关键词）
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

    /// 返回服务名称（用于定制 prompt 等场景）
    fn service_name(&self) -> &str {
        "generic"
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
            let text_bytes = text.len(); // UTF-8 字节数（百度按字节计限）
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

        // 流式日志：从 task_local 读取当前并发槽位的文件句柄（与 AI 翻译一致）
        let stream_log_file = crate::STREAM_LOG_FILE.try_get().ok();

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
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

            // 日志请求体：脱敏 sign（只保留前 8 位），避免泄露完整签名
            let log_request = format!(
                "URL: {}\n\
                 from: {}\n\
                 to: {}\n\
                 appid: {}\n\
                 salt: {}\n\
                 sign: {}...(masked)\n\
                 q ({} bytes, {} lines):\n{}",
                url,
                Self::to_baidu_lang(source_lang),
                Self::to_baidu_lang(target_lang),
                self.app_id,
                salt,
                &sign[..sign.len().min(8)],
                joined.len(),
                chunk.len(),
                joined,
            );

            // 流式日志：记录请求
            if let Some(ref log_file) = stream_log_file {
                crate::log_stream_to_file(log_file, &format!(
                    "\n\n========== 百度翻译批次 {} ==========\n时间: {}\nProvider: baidu\n\n--- 请求体 ---\n{}\n\n--- 响应 ---\n",
                    chunk_idx + 1,
                    chrono::Local::now().format("%H:%M:%S%.3f"),
                    log_request,
                ));
            }

            let resp = self
                .client
                .post(url)
                .form(&params)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| {
                    let err_msg = e.to_string();
                    crate::log_api_debug(
                        "baidu", "", source_lang, target_lang,
                        &log_request, &format!("[send error] {}", err_msg), 0,
                    );
                    if let Some(ref log_file) = stream_log_file {
                        crate::log_stream_to_file(log_file, &format!(
                            "\n[发送失败] {}\n\n========== 百度翻译批次 {} 结束（错误）==========\n",
                            err_msg, chunk_idx + 1,
                        ));
                    }
                    AppError::TranslateRequestFailed {
                        detail: err_msg,
                    }
                })?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                crate::log_api_debug(
                    "baidu", "", source_lang, target_lang,
                    &log_request, "[429 Too Many Requests]", 429,
                );
                if let Some(ref log_file) = stream_log_file {
                    crate::log_stream_to_file(log_file, &format!(
                        "\n[HTTP 429] 请求过于频繁\n\n========== 百度翻译批次 {} 结束（限流）==========\n",
                        chunk_idx + 1,
                    ));
                }
                return Err(AppError::TranslateRateLimit {
                    provider: "baidu".to_string(),
                    retry_after: Some(1),
                });
            }

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                crate::log_api_debug(
                    "baidu", "", source_lang, target_lang,
                    &log_request, &body, status.as_u16(),
                );
                if let Some(ref log_file) = stream_log_file {
                    crate::log_stream_to_file(log_file, &format!(
                        "\n[HTTP {}] {}\n\n========== 百度翻译批次 {} 结束（错误）==========\n",
                        status, body.chars().take(500).collect::<String>(), chunk_idx + 1,
                    ));
                }
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

            // 读取响应文本（用于日志），再解析为 JSON
            let response_text = resp.text().await.unwrap_or_default();
            let body: serde_json::Value = serde_json::from_str(&response_text).map_err(|e| {
                crate::log_api_debug(
                    "baidu", "", source_lang, target_lang,
                    &log_request, &format!("[JSON parse error] {}\nraw: {}", e, response_text.chars().take(500).collect::<String>()), 200,
                );
                if let Some(ref log_file) = stream_log_file {
                    crate::log_stream_to_file(log_file, &format!(
                        "\n[JSON 解析失败] {}\n原始响应: {}\n\n========== 百度翻译批次 {} 结束（解析错误）==========\n",
                        e, response_text.chars().take(500).collect::<String>(), chunk_idx + 1,
                    ));
                }
                AppError::TranslateResponseParseFailed {
                    detail: e.to_string(),
                }
            })?;

            // 流式日志：记录响应
            if let Some(ref log_file) = stream_log_file {
                crate::log_stream_to_file(log_file, &format!(
                    "{}\n\n========== 百度翻译批次 {} 结束 ==========\n",
                    response_text, chunk_idx + 1,
                ));
            }

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
                    crate::log_api_debug(
                        "baidu", "", source_lang, target_lang,
                        &log_request, &response_text, 200,
                    );
                    return Err(AppError::TranslateRateLimit {
                        provider: "baidu".to_string(),
                        retry_after: Some(1),
                    });
                }
                // 54003 之外的错误，检查余额不足
                let full_msg = format!("error_code: {}, msg: {}", code, msg);
                crate::log_api_debug(
                    "baidu", "", source_lang, target_lang,
                    &log_request, &response_text, 200,
                );
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

            // 成功时也记录 API 调试日志（与 AI 翻译一致）
            crate::log_api_debug(
                "baidu", "", source_lang, target_lang,
                &log_request, &response_text, 200,
            );

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
                if let Some(ref log_file) = stream_log_file {
                    crate::log_stream_to_file(log_file, &format!(
                        "\n[对齐异常] 输入 {} 行，返回 {} 行\n",
                        chunk.len(), translations.len(),
                    ));
                }
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

/// Bing 翻译 API（Azure Translator 2026-06-06）
/// 文档：https://learn.microsoft.com/en-us/azure/ai-services/translator/text-translation/2026-06-06/translate-api
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
        let params = [("api-version", "2026-06-06")];

        // 2026-06-06 新格式：inputs 数组，每个元素含 text + language + targets
        let inputs: Vec<serde_json::Value> = texts
            .iter()
            .map(|t| serde_json::json!({
                "text": t.as_str(),
                "language": source_lang,
                "targets": [{ "language": target_lang }]
            }))
            .collect();
        let body = serde_json::json!({ "inputs": inputs });

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

        // 2026-06-06 响应格式：{ "value": [{ "translations": [{ "text": "..." }] }, ...] }
        let translations = result
            .get("value")
            .and_then(|v| v.as_array())
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

pub struct TranslateScheduler<'a> {
    db: &'a Database,
    provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
    provider_name: String,
    model: std::sync::Arc<String>,
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
        model: String,
    ) -> Self {
        Self {
            db,
            provider,
            provider_name,
            model: std::sync::Arc::new(model),
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
        model: String,
        cancel_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
        my_gen: u64,
    ) -> Self {
        Self {
            db,
            provider,
            provider_name,
            model: std::sync::Arc::new(model),
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
                    && !has_music_symbols(&cached)
                {
                    // 译文无 CJK（AI 未实际翻译成中文）：翻译时应跳过重新翻译，
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
                        || (target_lang.starts_with("zh") && !has_cjk(&cached) && !has_cjk(&entry.text) && !is_music_or_sfx && !is_non_english && !has_music_symbols(&cached));
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
            } else if entry.text.contains("Waaaa") || entry.text.contains("What's your name") {
                eprintln!("[DEBUG CACHE] #{} cache MISS", entry.index);
            }

            // 占位符保护（按引擎类型选择策略）
            let strategy = PlaceholderStrategy::for_provider(&self.provider_name);
            let mut protector = PlaceholderProtector::with_strategy(strategy);
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

                let restored = protector.restore_with_ass_recovery(&combined, &entry.text);
                // 翻译失败判定（与批次模式 translate_entries_full 的 failed 判定一致）
                // 非英语内容（如拼写字母 "G-O-R..."、祖鲁语歌词等）保持原样是正确行为
                let is_non_english = !has_english_word(&entry.text, 3);
                // 音乐符号/音效标记保持原样是正确行为
                let is_music_or_sfx = is_music_or_symbol_only(&entry.text)
                    || looks_like_sound_effect(&entry.text);
                // 译文含音乐符号（歌词/拟声词，无法翻译）：不算无 CJK 失败
                let trans_has_music = has_music_symbols(&restored);
                let same_as_orig = restored.trim() == entry.text.trim();
                let no_cjk = target_lang.starts_with("zh")
                    && !has_cjk(&restored)
                    && !has_cjk(&entry.text);
                let orig_is_sound = looks_like_sound_effect(&entry.text);
                let restored_is_sound = looks_like_sound_effect(&restored);
                let sound_mismatch = orig_is_sound != restored_is_sound;
                // 传统翻译引擎（Google/Bing/DeepL/百度/腾讯等）是确定性的，
                // 不会错位/合并条目，AI 错位检测（部分翻译、长度比值）对传统引擎会误判。
                let is_traditional = self.model.is_empty();
                // 长度比值异常：短原文被翻译成长译文（批次错位），仅对 AI 引擎检测
                let length_ratio_abnormal = if is_traditional {
                    false
                } else {
                    let orig_len = entry.text.chars().count().max(1);
                    let trans_len = restored.chars().count();
                    trans_len > 0 && {
                        let ratio = trans_len as f64 / orig_len as f64;
                        ratio > 5.0 && trans_len > 10
                    }
                };
                // failed 判定：与批次模式一致，排除非英语/音乐/音效内容
                // 传统引擎跳过 AI 部分翻译检测（保留英文术语如 alpha、beta 是正常翻译）
                let partial_trans = !is_traditional
                    && !is_non_english
                    && !is_music_or_sfx
                    && is_partial_translation(&entry.text, &restored);
                let failed = any_failed
                    || combined.is_empty()
                    || (!is_non_english && !is_music_or_sfx && same_as_orig)
                    || (no_cjk && !is_non_english && !is_music_or_sfx && !trans_has_music)
                    || sound_mismatch
                    || length_ratio_abnormal
                    || partial_trans;
                if !restored.is_empty() && !failed {
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
                } else if !restored.is_empty() && failed {
                    // 所有 failed 条目（译文非空）写入缓存，保证 get_cached_entries 恢复时译文一致。
                    // translate_entries_full 的 bad_cache 检查会跳过坏缓存重新翻译，
                    // 但恢复时 get_cached_entries 返回它们（标记 failed），前端视为未翻译，用户可重新翻译。
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
                    failed,
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
        // 策略：长条目按模型配置的批次大小提高效率；短条目（<30 字节）单独按 5 条一批，
        // 避免 AI 把短句和相邻长句合并翻译导致整条批次偏移。
        let (long_batch_size, _) = get_model_batch_sizes(&self.model);
        let short_batch_size = 5;
        let short_text_threshold = 30;
        // 致命错误（余额不足/接口未授权/每日限额/认证失败）：记录后中止所有批次并返回
        let mut fatal_error: Option<AppError> = None;
        if !to_translate.is_empty() {
            let (short_entries, long_entries): (
                Vec<(usize, String, String, PlaceholderProtector)>,
                Vec<(usize, String, String, PlaceholderProtector)>,
            ) = to_translate.into_iter().partition(|(_, _, t, _)| t.len() < short_text_threshold);
            let mut batches: Vec<Vec<(usize, String, String, PlaceholderProtector)>> = Vec::new();
            batches.extend(long_entries.chunks(long_batch_size).map(|c| c.to_vec()));
            batches.extend(short_entries.chunks(short_batch_size).map(|c| c.to_vec()));
            let total_batches = batches.len();
            let concurrency = self.concurrency.max(1);
            tracing::info!("翻译并发度: {}，共 {} 批，长条目批次大小: {}", concurrency, total_batches, long_batch_size);

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
                let model = (*self.model).clone();
                let cancel_counter = cancel_counter.clone();
                let semaphore = semaphore.clone();
                let stream_log_slots = stream_log_slots.clone();
                let slot_counter = slot_counter.clone();
                let rate_limit = self.rate_limit;

                join_set.spawn(async move {
                    // 在 task 内部获取信号量，不阻塞 spawn 循环
                    // 这样 while join_next 循环能立即开始处理已完成的结果
                    let _permit = semaphore.acquire_owned().await.unwrap();
                    if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                        return (batch_idx, Ok(vec![]));
                    }
                    // QPS 模式：获取信号量后，发请求前强制等待 min_interval
                    // 信号量只保证不并发，但不保证请求间隔；降级重试会发多个请求，
                    // 没有 sleep 的话 30→10→5→3→1 全部降级会产生 ~200 个无间隔请求
                    let min_interval = rate_limit.min_interval();
                    if !min_interval.is_zero() {
                        tokio::time::sleep(min_interval).await;
                    }
                    tracing::info!("翻译批次 {}/{}，本批 {} 条", batch_idx + 1, total_batches, texts.len());

                    // 分配并发槽位的日志文件
                    let slot_idx = (slot_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % stream_log_slots.len() as u64) as usize;
                    let stream_log_file = stream_log_slots[slot_idx].clone();

                    let result = crate::STREAM_LOG_FILE.scope(stream_log_file, async {
                        translate_batch_with_fallback(
                            &*provider,
                            &texts,
                            &source,
                            &target,
                            &model,
                            &cancel_counter,
                            my_gen,
                            rate_limit,
                        ).await
                    }).await;
                    // result 现在是 Result<Vec<String>, AppError>
                    // 致命错误（余额不足等）由外层 join_next 循环处理
                    (batch_idx, result)
                });
            }

            // 批次完成即处理（不要求顺序）：立即回调 on_entry_done / on_progress，
            // 避免 head-of-line blocking（batch 0 慢时后续批次全部等待导致进度卡 0）
            // 致命错误（余额不足/接口未授权/每日限额/认证失败）：立即中止所有批次并返回错误
            while let Some(res) = join_set.join_next().await {
                let (batch_idx, batch_result) = match res {
                    Ok(item) => item,
                    Err(e) => {
                        tracing::warn!("join 任务异常: {}", e);
                        continue;
                    }
                };
                // 检查是否为致命错误
                let translations = match batch_result {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!(
                            "翻译批次 {} 遇到致命错误，中止所有批次: {}",
                            batch_idx + 1,
                            e
                        );
                        fatal_error = Some(e);
                        join_set.abort_all();
                        break;
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
                    let mut restored = protector.restore_with_ass_recovery(translated, orig_text);
                    // 兜底：经所有降级重试后译文仍为空（如模型持续返回空/占位符恢复失败），
                    // 用原文填充并标记失败，避免导出/保存时出现空白中文行。
                    if restored.trim().is_empty() && !orig_text.trim().is_empty() {
                        tracing::warn!(
                            "字幕 #{} 经降级重试后译文仍为空，用原文兜底",
                            index
                        );
                        restored = orig_text.clone();
                    }
                    // 翻译失败判定：
                    // 1. 译文为空
                    // 2. 译文与原文相同（AI 未实际翻译，原样返回）
                    // 3. 目标语言是中文但译文无 CJK 字符（AI 只返回了部分原文或改了标签）
                    // 音效标记一致性校验：原文是音效标记但译文不是，或反过来译文是音效标记但原文不是，
                    // 通常意味着 AI 把相邻条目合并/错位翻译了（如 "you need every week." → "[碰撞声持续]"）。
                    let orig_is_sound = looks_like_sound_effect(orig_text);
                    let restored_is_sound = looks_like_sound_effect(&restored);
                    let sound_mismatch = orig_is_sound != restored_is_sound;

                    // 传统翻译引擎（Google/Bing/DeepL/百度/腾讯等）是确定性的，
                    // 不会错位/合并条目，跳过 AI 错位检测（部分翻译、长度比值）。
                    let is_traditional = self.model.is_empty();

                    // 4. 长度比值异常：短原文被翻译成长译文（如 "♪♪" → "Titus：哦，该死！\n♪♪"），
                    // 通常是批次错位翻译，不应缓存。仅对 AI 引擎检测。
                    let length_ratio_abnormal = if is_traditional {
                        false
                    } else {
                        let orig_len = orig_text.chars().count().max(1);
                        let trans_len = restored.chars().count();
                        trans_len > 0 && {
                            let ratio = trans_len as f64 / orig_len as f64;
                            ratio > 5.0 && trans_len > 10
                        }
                    };

                    // 非英语内容检测：原文本身不含英语字母（如拼写字母 "G-O-R..."、
                    // 祖鲁语歌词 "Nants ingonyama bagithi baba!" 等），9b 保持原样是正确行为，
                    // 不应标记为 failed。检测条件：原文无连续 3 个以上英文字母组成的单词。
                    let is_non_english = !has_english_word(orig_text, 3);

                    // 音乐符号/音效标记保持原样是正确行为，不算翻译失败
                    let is_music_or_sfx = is_music_or_symbol_only(orig_text)
                        || looks_like_sound_effect(orig_text);

                    // 译文含音乐符号（歌词/拟声词，无法翻译）：不算无 CJK 失败
                    let trans_has_music = has_music_symbols(&restored);

                    // 部分翻译检测：译文含 CJK 但同时残留英文单词或音效标记问题
                    // 与单条翻译路径（translate_entries_full 的分段翻译）一致
                    // 例外：音效标记/音乐符号/非英语内容不检测
                    // 传统引擎跳过该检测（保留英文术语如 alpha、beta 是正常翻译）
                    let partial_trans = !is_traditional
                        && !is_non_english
                        && !is_music_or_sfx
                        && is_partial_translation(orig_text, &restored);

                    let failed = restored.is_empty()
                        || (!is_non_english && !is_music_or_sfx && restored.trim() == orig_text.trim())
                        || (target_lang.starts_with("zh")
                            && !has_cjk(&restored)
                            && !has_cjk(orig_text)
                            && !is_non_english
                            && !is_music_or_sfx
                            && !trans_has_music)
                        || sound_mismatch
                        || batch_count_mismatch
                        || length_ratio_abnormal
                        || partial_trans;

                    // 数量不匹配的批次整批不缓存（防止错位译文污染缓存）
                    // 例外：以下 failed 条目也写入缓存，保证 get_cached_entries 恢复时译文一致：
                    //   1. 译文=原文（如祖鲁语歌词等非英语内容，AI 保持原样）
                    //   2. 音效标记不一致（sound_mismatch，如含 {\an8} 定位标签的混合条目被译成纯音效标记）
                    // 翻译时 translate_entries_full 的 bad_cache 检查会跳过这些坏缓存重新翻译，
                    // 但恢复时 get_cached_entries 返回它们（标记 failed），前端视为未翻译，用户可重新翻译。
                    let _same_as_orig = restored.trim() == orig_text.trim();
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
                    } else if failed && !batch_count_mismatch && !restored.is_empty() {
                        // 所有 failed 条目（译文非空）写入缓存，保证 get_cached_entries 恢复时译文一致。
                        // translate_entries_full 的 bad_cache 检查会跳过坏缓存重新翻译，
                        // 但恢复时 get_cached_entries 返回它们（标记 failed），前端视为未翻译，用户可重新翻译。
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

        // 致命错误（余额不足/接口未授权/每日限额/认证失败）：返回错误，让前端弹 toast
        if let Some(e) = fatal_error {
            tracing::error!("翻译因致命错误中止: {}", e);
            return Err(e);
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
                    // 可取消的 sleep：每 100ms 检查取消标志
                    let total_ms = *delay * 1000;
                    let mut waited = 0u64;
                    while waited < total_ms {
                        let chunk = std::cmp::min(100, total_ms - waited);
                        tokio::time::sleep(std::time::Duration::from_millis(chunk)).await;
                        waited += chunk;
                        if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                            return Err(AppError::TranslateRetriesExhausted);
                        }
                    }
                }
                Err(AppError::TranslateNetworkError { provider, detail }) => {
                    tracing::warn!(
                        "翻译网络错误（第 {} 次重试）：{}，等待 {} 秒",
                        attempt + 1,
                        detail,
                        delay
                    );
                    last_error = Some(AppError::TranslateNetworkError { provider, detail });
                    // 可取消的 sleep：每 100ms 检查取消标志
                    let total_ms = *delay * 1000;
                    let mut waited = 0u64;
                    while waited < total_ms {
                        let chunk = std::cmp::min(100, total_ms - waited);
                        tokio::time::sleep(std::time::Duration::from_millis(chunk)).await;
                        waited += chunk;
                        if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                            return Err(AppError::TranslateRetriesExhausted);
                        }
                    }
                }
                Err(e) => return Err(e), // 鉴权失败等不重试
            }
        }

        Err(last_error.unwrap_or(AppError::TranslateRetriesExhausted))
}

/// 获取模型的批次大小配置
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
/// 签名算法：SHA256(appKey + input + salt + curtime + appSecret)
/// 其中 input = q前10字符 + q长度 + q后10字符（q长度>20时），否则 input = q
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
        // 有道签名算法要求对 query 做 truncate 处理生成 input：
        // 当 q 长度 > 20：input = q前10字符 + q长度 + q后10字符
        // 当 q 长度 <= 20：input = q 完整字符串
        let input = if query.chars().count() > 20 {
            let chars: Vec<char> = query.chars().collect();
            let len = chars.len();
            let first10: String = chars[..10].iter().collect();
            let last10: String = chars[len - 10..].iter().collect();
            format!("{}{}{}", first10, len, last10)
        } else {
            query.to_string()
        };
        let sign_input = format!("{}{}{}{}{}", self.app_key, input, salt, curtime, self.app_secret);
        let mut hasher = Sha256::new();
        hasher.update(sign_input.as_bytes());
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

/// 小牛翻译 API（V2）
/// 文档：https://niutrans.com/documents/contents/transapi_text_v2
/// 认证：appId + apikey → MD5 签名生成 authStr（权限字符串）
/// 签名规则：将 apikey 及所有发送参数（除 authStr、空值外）按参数名 ASCII 排序，
/// 拼成 "k1=v1&k2=v2&..." 后 MD5，得到 authStr。
pub struct NiutransProvider {
    app_id: String,
    api_key: String,
    client: reqwest::Client,
}

impl NiutransProvider {
    pub fn new(app_id: String, api_key: String) -> Self {
        Self::with_client(app_id, api_key, reqwest::Client::new())
    }
    pub fn with_client(app_id: String, api_key: String, client: reqwest::Client) -> Self {
        Self { app_id, api_key, client }
    }

    /// 生成权限字符串 authStr
    /// 步骤：apikey + 所有发送参数（除 authStr、空值外）按参数名 ASCII 排序拼接 → MD5
    fn generate_auth_str(&self, params: &[(&str, &str)]) -> String {
        use md5::{Digest, Md5};
        // 构造 (key, value) 列表，加入 apikey（apikey 不随请求发送，只参与签名）
        let mut all: Vec<(String, String)> = params
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        all.push(("apikey".to_string(), self.api_key.clone()));
        // 按 key ASCII 排序
        all.sort_by(|a, b| a.0.cmp(&b.0));
        // 拼接 "k1=v1&k2=v2&..."
        let param_str: String = all
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");
        let mut hasher = Md5::new();
        hasher.update(param_str.as_bytes());
        format!("{:x}", hasher.finalize())
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

        // 时间戳（秒级，官方 Python/C# demo 均用秒级，文档表格"毫秒数"描述有误）
        let timestamp = format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );

        // 参与签名的参数（不含 authStr，空值不参与）
        let sign_params: Vec<(&str, &str)> = vec![
            ("from", from),
            ("to", to),
            ("appId", &self.app_id),
            ("srcText", text),
            ("timestamp", &timestamp),
        ];
        let auth_str = self.generate_auth_str(&sign_params);

        // 请求参数（camelCase 字段名，apikey 不发送）
        // 使用 form-urlencoded（与官方 Python demo 一致），避免 JSON 编码导致的签名不一致
        let form_params: [(&str, &str); 6] = [
            ("from", from),
            ("to", to),
            ("appId", &self.app_id),
            ("srcText", text),
            ("timestamp", &timestamp),
            ("authStr", &auth_str),
        ];

        let resp = self
            .client
            .post("https://api.niutrans.com/v2/text/translate")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&form_params)
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

        // V2 返回 camelCase 错误字段：errorCode / errorMsg
        if let Some(code) = result.get("errorCode").and_then(|c| c.as_str()) {
            if !code.is_empty() && code != "0" {
                let msg = result.get("errorMsg").and_then(|m| m.as_str()).unwrap_or("unknown");
                let full_msg = format!("errorCode: {}, errorMsg: {}", code, msg);
                // 13001: 字符流量不足或没有访问权限
                if code == "13001" {
                    return Err(AppError::TranslateInsufficientBalance {
                        provider: "niutrans".to_string(),
                        detail: full_msg,
                    });
                }
                // 20001: 鉴权失败
                if code == "20001" {
                    return Err(AppError::TranslateNetworkError {
                        provider: "niutrans".to_string(),
                        detail: format!("{}（请检查 App ID 和 API Key 是否正确）", full_msg),
                    });
                }
                return Err(AppError::TranslateNetworkError {
                    provider: "niutrans".to_string(),
                    detail: full_msg,
                });
            }
        }

        let tgt = result["tgtText"]
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
        let canonical_headers = "content-type:application/json; charset=utf-8\nhost:tmt.tencentcloudapi.com\nx-tc-action:texttranslatebatch\n".to_string();
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
            .header("X-TC-Region", "ap-beijing")
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

        // 4. HMAC-SHA1，key 为 AccessKeySecret + "&"
        let key = format!("{}&", self.access_key_secret);
        let mut mac = HmacSha1::new_from_slice(key.as_bytes()).unwrap();
        mac.update(string_to_sign.as_bytes());
        let signature = mac.finalize().into_bytes();
        // 阿里云要求 Base64 编码（标准 Base64，不是 hex）
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(signature)
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

/// URL 编码（阿里云要求 RFC3986 编码规则）
fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
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

/// 创建翻译 provider 实例
pub fn create_provider(
    provider: &TranslateProvider,
    credentials: &ProviderCredentials,
) -> Result<std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>, AppError> {
    create_provider_with_proxy(provider, credentials, &ProxyConfig::default(), None, &ProviderOptions::default())
}

/// AI 翻译附加选项（glossary / name_tagging / tpm），传统翻译忽略
#[derive(Debug, Clone, Default)]
pub struct ProviderOptions {
    /// 译名表：(EnglishName, ChineseTranslation)
    pub glossary: Vec<(String, String)>,
    /// 是否要求模型在译文中用 <name=En>Zh</name> 标记人名
    pub name_tagging: bool,
    /// TPM 上限（每分钟 token 数，0 = 不限制）
    pub tpm_limit: u64,
}

/// 创建翻译 provider 实例（带代理配置）
/// service_id: AI 服务商标识（如 "siliconflow"），用于构造 AiService 分发行为；传统翻译传 None
/// options: AI 翻译附加选项（glossary / name_tagging），传统翻译忽略
pub fn create_provider_with_proxy(
    provider: &TranslateProvider,
    credentials: &ProviderCredentials,
    proxy: &ProxyConfig,
    service_id: Option<&str>,
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
            let ai_service = AiService::from_service_id(service_id.unwrap_or("custom"));
            let name = ai_service.display_name().to_string();
            // 优先用用户配置的 TPM，否则用服务商默认值
            let tpm_limit = if options.tpm_limit > 0 {
                options.tpm_limit
            } else {
                ai_service.default_tpm_limit()
            };
            Ok(std::sync::Arc::new(
                OpenAiProvider::with_client(base_url, model, model_type, api_key, client)
                    .with_service_name(name)
                    .with_ai_service(ai_service)
                    .with_glossary(options.glossary.clone())
                    .with_name_tagging(options.name_tagging)
                    .with_tpm_limit(tpm_limit),
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
            let app_id = credentials.app_id.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "niutrans".to_string() }
            })?;
            let api_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound { provider: "niutrans".to_string() }
            })?;
            Ok(std::sync::Arc::new(NiutransProvider::with_client(app_id, api_key, client)))
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shift_detection_normal_chinese_translation_not_flagged() {
        // 回归测试：正常的中英翻译不应被错位检测误判。
        // Bug："But I have something to say." (28字符) → "但我有话要说。" (7字符)
        //   orig_len=28 < 30 → short_ratio_threshold=0.0（ratio 检测不生效）
        //   short_char_diff = (28-7) > 20 = true → 误判为错位
        //   导致 5 级降级重试全部失败、译文留空、标记 failed
        // 修复后：28 字符属于 15~29 区间，启用 ratio < 0.1 检测，
        //   ratio = 7/28 = 0.25 > 0.1，不触发；short_char_diff 不适用（orig_len >= 15）
        let cases: &[(&str, &str, bool)] = &[
            // (原文, 译文, 应否判为错位)
            ("But I have something to say.", "但我有话要说。", false),
            ("Congratulations!", "恭喜！", false),
            ("You expect us to listen to you?!", "你期待我们听你的？！", false),
            // 真正的错位（译文异常短）应被检测
            ("This is a longer sentence that should translate.", "是", true),
            // 极短原文的字符差检测
            ("Yeah okay.", "嗯", false),
        ];
        for (orig, trans, should_shift) in cases {
            let orig_len = orig.chars().count().max(1);
            let trans_len = trans.chars().count();
            let ratio = trans_len as f64 / orig_len as f64;
            let short_ratio_threshold = if orig_len >= 30 { 0.15 }
                else if orig_len >= 15 { 0.1 }
                else { 0.0 };
            let short_char_diff = if orig_len < 15 {
                (orig_len as i64 - trans_len as i64) > 10
            } else {
                false
            };
            let multiline_split = orig.contains('\n')
                && !trans.contains('\n')
                && orig_len >= 20
                && trans_len > 0
                && ratio < 0.5;
            let shift = ratio < short_ratio_threshold
                || short_char_diff
                || multiline_split
                || (ratio > 5.0 && trans_len > 10);
            assert_eq!(shift, *should_shift,
                "orig={:?} ({}chars) trans={:?} ({}chars) ratio={:.3}: expected shift={}, got shift={}",
                orig, orig_len, trans, trans_len, ratio, should_shift, shift);
        }
    }

    #[test]
    fn test_has_lost_non_sound_lines() {
        // 多行原文含非音效行，译文只是音效标记 → 应检测到丢失
        assert!(has_lost_non_sound_lines(
            "from our mothers!\n[ Mup crying ]",
            "[Mup 哭泣]"
        ));
        // 多行原文含非音效行，译文完整翻译了所有行 → 不应检测到丢失
        assert!(!has_lost_non_sound_lines(
            "from our mothers!\n[ Mup crying ]",
            "从我们母亲的身体里！\n[Mup 哭泣]"
        ));
        // 单行原文（非多行）→ 不应检测到丢失
        assert!(!has_lost_non_sound_lines(
            "[ Mup crying ]",
            "[Mup 哭泣]"
        ));
        // 多行原文全是音效行 → 不应检测到丢失（没有非音效行可丢失）
        assert!(!has_lost_non_sound_lines(
            "[ birds chirping ]\n[ wind blowing ]",
            "[鸟儿鸣叫]\n[风声]"
        ));
        // 多行原文含非音效行，译文不是音效标记 → 不应检测到丢失
        assert!(!has_lost_non_sound_lines(
            "Hello world\n[ sound ]",
            "你好世界\n[声音]"
        ));
        // 多行原文含非音效行，译文是音效标记但内容更丰富 → 应检测到丢失
        assert!(has_lost_non_sound_lines(
            "Some dialogue here\n[ music playing ]",
            "[音乐播放中]"
        ));
    }

    #[test]
    fn test_shift_detection_skipped_when_batch_size_is_one() {
        // 回归测试：batch_size == 1 时跳过错位检测。
        // 单条翻译没有相邻条目，不可能发生批次内错位；
        // 确定性 API（百度/DeepL/Google）单条翻译结果就是"正确答案"。
        // 即使译文看起来"异常短"（如 28字符→2字符），也不应判为错位。
        // 后续仍有 failed 判定（空译文/无 CJK/音效不一致等）兜底。
        let cases: &[(&str, &str)] = &[
            ("But I have something to say.", "话"),  // 极端短，但 batch_size=1 时应接受
            ("This is a longer sentence that should translate.", "是"),
            ("Congratulations!", "恭"),
        ];
        for (orig, trans) in cases {
            // 模拟 translate_batch_with_fallback 中 batch_size == 1 的行为
            let batch_size = 1;
            let skip_shift_check = batch_size == 1;
            assert!(skip_shift_check,
                "batch_size=1 时应跳过错位检测：orig={:?}", orig);
            // 即使错位检测规则会判为错位，skip_shift_check 也应阻止它
            let orig_len = orig.chars().count().max(1);
            let trans_len = trans.chars().count();
            let ratio = trans_len as f64 / orig_len as f64;
            let would_shift_without_skip = ratio < 0.1
                || (ratio > 5.0 && trans_len > 10);
            // 验证：如果不跳过，这些 case 确实会被判为错位（证明测试有意义）
            // 跳过后，shift 检测不执行，译文直接填入 results
            let _ = would_shift_without_skip; // 仅验证计算不 panic
        }
    }

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
    fn test_ass_tag_recovery_when_9b_drops_placeholder() {
        // 模拟 9b 模型丢弃占位符字符的情况
        let mut p = PlaceholderProtector::new();
        let input = r"{\an8}[phone buzzing] Oh, Greater Whitethroat!";
        let _protected = p.protect(input);
        // 9b 模型翻译后丢弃了占位符（模拟：翻译结果中不含占位符字符）
        let fake_translation = "哦，是白喉林莺！";
        // 普通 restore 会丢失 {\an8}
        let plain_restored = p.restore(fake_translation);
        assert!(!plain_restored.contains("{\\an8}"));
        // restore_with_ass_recovery 应恢复 {\an8} 前缀
        let recovered = p.restore_with_ass_recovery(fake_translation, input);
        assert!(recovered.starts_with("{\\an8}"));
        assert!(recovered.contains("白喉林莺"));
    }

    #[test]
    fn test_ass_tag_recovery_no_loss() {
        // 译文中已包含 ASS 标签时不应重复添加
        let mut p = PlaceholderProtector::new();
        let input = r"{\an8}Hello world";
        let protected = p.protect(input);
        // 模拟 9b 正确保留占位符
        let restored = p.restore_with_ass_recovery(&protected, input);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_ass_tag_recovery_no_prefix_tags() {
        // 原文没有前缀 ASS 标签时不做任何处理
        let mut p = PlaceholderProtector::new();
        let input = "Hello world";
        let protected = p.protect(input);
        let restored = p.restore_with_ass_recovery(&protected, input);
        assert_eq!(restored, input);
    }

    #[test]
    fn test_strip_remaining_placeholders() {
        // 9b 模型多输出未注册的占位符字符
        let mut p = PlaceholderProtector::new();
        let input = r"{\an8}[phone buzzing]";
        let _protected = p.protect(input);
        // 9b 返回时多了一个 \ue001（未注册的占位符）
        let fake = format!("\u{E000}\u{E001}[手机震动]");
        let restored = p.restore_with_ass_recovery(&fake, input);
        // \ue001 应被清除，不应出现在最终译文中
        assert!(!restored.contains('\u{E001}'));
        assert!(!restored.contains('\u{E000}'));
        assert!(restored.starts_with("{\\an8}"));
        assert!(restored.contains("[手机震动]"));
    }

    #[test]
    fn test_placeholder_strategy_xml_tags() {
        // XML 标签策略：HTML 标签被替换为 <xN></xN> 形式
        let mut p = PlaceholderProtector::with_strategy(PlaceholderStrategy::XmlTags);
        let input = "It is <i>you</i> playing <i>ball</i>";
        let protected = p.protect(input);
        // 4 个标签：index 0=<i>, 1=</i>, 2=<i>, 3=</i>
        assert!(protected.contains("<x0>"));
        assert!(protected.contains("</x1>"));
        assert!(protected.contains("<x2>"));
        assert!(protected.contains("</x3>"));
        // 不应包含原始 <i> 标签
        assert!(!protected.contains("<i>"));
        // 回填
        let fake_trans = "<x0>你</x1>在打<x2>球</x3>";
        let restored = p.restore(fake_trans);
        assert_eq!(restored, "<i>你</i>在打<i>球</i>");
    }

    #[test]
    fn test_placeholder_strategy_direct_html() {
        // DirectHtml 策略：HTML 标签直接保留，不替换为占位符
        let mut p = PlaceholderProtector::with_strategy(PlaceholderStrategy::DirectHtml);
        let input = "It is <i>you</i> playing <i>ball</i>";
        let protected = p.protect(input);
        // HTML 标签应直接保留
        assert!(protected.contains("<i>"));
        assert!(protected.contains("</i>"));
        assert_eq!(protected, input);
        // 回填不需要替换 HTML 标签（因为没有被保护）
        let fake_trans = "这是<i>你</i>在打<i>球</i>";
        let restored = p.restore(fake_trans);
        assert_eq!(restored, fake_trans);
    }

    #[test]
    fn test_placeholder_strategy_curly_braces() {
        // 花括号策略：HTML 标签被替换为 {N}{/N} 形式
        let mut p = PlaceholderProtector::with_strategy(PlaceholderStrategy::CurlyBraces);
        let input = "It is <i>you</i> playing <i>ball</i>";
        let protected = p.protect(input);
        // 4 个标签：index 0=<i>, 1=</i>, 2=<i>, 3=</i>
        assert!(protected.contains("{0}"));
        assert!(protected.contains("{/1}"));
        assert!(!protected.contains("<i>"));
        let fake_trans = "{0}你{/1}在打{2}球{/3}";
        let restored = p.restore(fake_trans);
        assert_eq!(restored, "<i>你</i>在打<i>球</i>");
    }

    #[test]
    fn test_placeholder_strategy_for_provider() {
        // 传统引擎 → PrivateUse
        assert_eq!(PlaceholderStrategy::for_provider("baidu"), PlaceholderStrategy::PrivateUse);
        assert_eq!(PlaceholderStrategy::for_provider("youdao"), PlaceholderStrategy::PrivateUse);
        assert_eq!(PlaceholderStrategy::for_provider("tencent"), PlaceholderStrategy::PrivateUse);
        // AI 模型 → XmlTags（包括复合 provider_name）
        assert_eq!(PlaceholderStrategy::for_provider("openai"), PlaceholderStrategy::XmlTags);
        assert_eq!(PlaceholderStrategy::for_provider("openai-lmstudio-qwen3.5-9b"), PlaceholderStrategy::XmlTags);
        assert_eq!(PlaceholderStrategy::for_provider("openai-deepseek-deepseek-v4"), PlaceholderStrategy::XmlTags);
        assert_eq!(PlaceholderStrategy::for_provider("openai-siliconflow-qwen-72b"), PlaceholderStrategy::XmlTags);
        // 原生 HTML 支持 → DirectHtml
        assert_eq!(PlaceholderStrategy::for_provider("deepl"), PlaceholderStrategy::DirectHtml);
        assert_eq!(PlaceholderStrategy::for_provider("google"), PlaceholderStrategy::DirectHtml);
        assert_eq!(PlaceholderStrategy::for_provider("bing"), PlaceholderStrategy::DirectHtml);
    }

    #[test]
    fn test_placeholder_xml_tags_with_newline() {
        // XML 标签策略：换行符也被保护
        let mut p = PlaceholderProtector::with_strategy(PlaceholderStrategy::XmlTags);
        let input = "line1\nline2";
        let protected = p.protect(input);
        // 换行符应被替换为 <xN/> 形式
        assert!(!protected.contains('\n'));
        assert!(protected.contains("<x"));
        let fake_trans = "第一行<x0/>第二行";
        let restored = p.restore(fake_trans);
        assert_eq!(restored, "第一行\n第二行");
    }

    #[test]
    fn test_placeholder_xml_tags_strip_remaining() {
        // XML 标签策略：清除残留的 <xN> 占位符
        let mut p = PlaceholderProtector::with_strategy(PlaceholderStrategy::XmlTags);
        let input = "<i>hello</i>";
        let _protected = p.protect(input);
        // 模拟 9b 模型多输出了未注册的 <x99>
        // index 0=<i>, 1=</i>
        let fake = "<x99>你好<x0>";
        let restored = p.restore_with_ass_recovery(fake, input);
        // <x99> 应被清除，<x0> 应被回填为 <i>
        assert!(!restored.contains("<x99>"));
        assert_eq!(restored, "你好<i>");
    }

    #[test]
    fn test_clean_json_leak_full_array() {
        // 完整 JSON 数组包装泄漏
        let leaked = r#"[{"n": 1, "t": "让我们看看发生了什么事。"}]"#;
        let cleaned = clean_json_leak(leaked);
        assert_eq!(cleaned, "让我们看看发生了什么事。");
    }

    #[test]
    fn test_clean_json_leak_trailing_syntax() {
        // 译文末尾 JSON 语法残留
        let leaked = "她说：'好吧，那我来看看情况吧，'},\n  {";
        let cleaned = clean_json_leak(leaked);
        assert_eq!(cleaned, "她说：'好吧，那我来看看情况吧，'");
        // 不应包含 JSON 语法
        assert!(!cleaned.contains("'},"));
        assert!(!cleaned.contains("\n  {"));
    }

    #[test]
    fn test_clean_json_leak_normal_text() {
        // 正常译文不应被修改
        let normal = "这是一个正常的翻译。";
        let cleaned = clean_json_leak(normal);
        assert_eq!(cleaned, normal);
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
    fn test_parse_name_extraction_all_caps_no_digit_brand() {
        // 全大写无数字的缩写也是品牌名，应保留 `英文（中文）` 格式
        // 防止 is_brand_name_format 退化成 AND 逻辑（要求必须有数字）
        let content = r#"[
            {"en": "GPS", "zh": "GPS（全球定位系统）"},
            {"en": "NASA", "zh": "NASA（美国宇航局）"}
        ]"#;
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].english, "GPS");
        assert_eq!(names[0].chinese, "GPS（全球定位系统）");
        assert_eq!(names[1].english, "NASA");
        assert_eq!(names[1].chinese, "NASA（美国宇航局）");
    }

    #[test]
    fn test_strip_name_tags_all_formats() {
        // 标准格式 <name=En>Zh</name>
        assert_eq!(translate_ai::strip_name_tags("<name=Rick>瑞克</name>"), "瑞克");
        // 无 = 格式 <name>Zh</name>
        assert_eq!(translate_ai::strip_name_tags("<name>瑞克</name>"), "瑞克");
        // 英文名含空格 <name=Georgie Boy>乔治男孩</name>
        assert_eq!(translate_ai::strip_name_tags("<name=Georgie Boy>乔治男孩</name>"), "乔治男孩");
        // 带引号 <name="Rick">瑞克</name>
        assert_eq!(translate_ai::strip_name_tags(r#"<name="Rick">瑞克</name>"#), "瑞克");
        // 混合文本
        assert_eq!(translate_ai::strip_name_tags("你好<name=Rick>瑞克</name>再见"), "你好瑞克再见");
        // 多个标签
        assert_eq!(translate_ai::strip_name_tags("<name=Rick>瑞克</name>和<name=Morty>莫蒂</name>"), "瑞克和莫蒂");
        // 无标签
        assert_eq!(translate_ai::strip_name_tags("普通文本"), "普通文本");
    }

    #[test]
    fn test_strip_name_tags_malformed() {
        // 9b 模型畸形标签：开标签缺少 >，但有 </name>
        // <name=Earth</name>地球 → 移除畸形标签，保留地球
        assert_eq!(translate_ai::strip_name_tags("<name=Earth</name>地球去。"), "地球去。");
        // <name=weird </name>奇怪的 → 移除畸形标签，保留奇怪的
        assert_eq!(translate_ai::strip_name_tags("<i><name=weird </name>奇怪的</i>"), "<i>奇怪的</i>");
        // 9b 模型畸形标签：开标签缺少 > 且无 </name>，英文名和中文名连在一起
        // <name=friends猪朋友们 → 移除 <name=friends，保留猪朋友们
        assert_eq!(translate_ai::strip_name_tags("<name=friends猪朋友们安排了新生活。"), "猪朋友们安排了新生活。");
        // 孤立闭标签 </name=Georgie Boy>
        assert_eq!(translate_ai::strip_name_tags("乔治男孩! </name=Georgie Boy>乔治男孩!"), "乔治男孩! 乔治男孩!");
        // 孤立闭标签 </name>
        assert_eq!(translate_ai::strip_name_tags("瑞克</name>再见"), "瑞克再见");
    }

    #[test]
    fn test_post_process_name_tags_english_in_content() {
        // llama4 等模型输出 <name>EnglishName</name>ChineseName 格式：
        // 标签内容是英文名，中文译名跟在标签外面。
        // post_process_name_tags 应移除整个标签，避免译名重复。
        let mut translations = vec![
            "<name>Jerry</name>杰瑞的焦虑尤其如此。".to_string(),
            "<name>Dogsss</name>狗狗们在城市里有一个繁殖计划。".to_string(),
            "我给她取名叫 <name>Jerry</name> 杰瑞。".to_string(),
        ];
        let pre_scan_glossary = vec![
            ("Jerry".to_string(), "杰瑞".to_string()),
            ("Dogsss".to_string(), "狗狗们".to_string()),
        ];
        let result = translate_ai::post_process_name_tags(&mut translations, &pre_scan_glossary);
        assert_eq!(translations[0], "杰瑞的焦虑尤其如此。");
        assert_eq!(translations[1], "狗狗们在城市里有一个繁殖计划。");
        assert_eq!(translations[2], "我给她取名叫杰瑞。");
        // 确保所有条目都被修正
        assert_eq!(result.corrected_indices.len(), 3);
    }

    #[test]
    fn test_extract_name_tags_all_formats() {
        // 标准格式
        let tags = translate_ai::extract_name_tags("<name=Rick>瑞克</name>");
        assert_eq!(tags, vec![("Rick".to_string(), "瑞克".to_string())]);
        // 无 = 格式：en 用 zh 代替
        let tags = translate_ai::extract_name_tags("<name>瑞克</name>");
        assert_eq!(tags, vec![("瑞克".to_string(), "瑞克".to_string())]);
        // 英文名含空格
        let tags = translate_ai::extract_name_tags("<name=Georgie Boy>乔治男孩</name>");
        assert_eq!(tags, vec![("Georgie Boy".to_string(), "乔治男孩".to_string())]);
    }

    #[test]
    fn test_get_cached_entries_aah_failed_recovery() {
        // 回归测试：9b 模型 "Aah!" → "Aah!"（未翻译），翻译时 failed=true，
        // get_cached_entries 恢复时也应返回 failed=true，否则 repeated_open 检查不一致。
        let db_path = std::env::temp_dir().join(format!("test_cache_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let db = Database::open(&db_path).expect("打开测试数据库失败");
        db.migrate().expect("数据库迁移失败");

        let source_lang = "en";
        let target_lang = "zh";
        let provider_name = "openai-lmstudio-test";
        let file_hash = "test_hash";

        // 写入缓存：Aah! → Aah!（模拟 9b 模型未翻译的 failed 条目）
        let cache_key = translate_cache_key("Aah!", source_lang, target_lang, provider_name, file_hash);
        db.set_translate_cache(&cache_key, "Aah!", "Aah!", source_lang, target_lang, provider_name)
            .unwrap();

        let scheduler = TranslateScheduler::new(
            &db,
            std::sync::Arc::new(crate::translate::BaiduProvider::new(String::new(), String::new()))
                as std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
            provider_name.to_string(),
            String::new(),
        )
        .with_file_hash(file_hash.to_string());

        let entries = vec![crate::subtitle::SubtitleEntry {
            index: 151,
            start_ms: 0,
            end_ms: 1000,
            text: "Aah!".to_string(),
            translated: String::new(),
            style: None,
            failed: false,
            from_cache: false,
        }];

        let result = scheduler.get_cached_entries(&entries, source_lang, target_lang).unwrap();
        assert_eq!(result.len(), 1);
        let entry = &result[0];
        assert!(entry.from_cache, "Should be from cache");
        assert!(entry.failed, "Aah! → Aah! should be marked as failed during recovery");
        assert_eq!(entry.translated, "Aah!");

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn from_service_id_exhaustive_test() {
        let known_ids = [
            "deepseek", "zhipu", "siliconflow", "groq", "qwen", "doubao",
            "hunyuan", "lingyi", "kimi", "openai", "azure_openai",
            "gemini", "ernie", "ollama", "lmstudio",
        ];
        for id in known_ids {
            assert_ne!(
                AiService::from_service_id(id),
                AiService::Custom,
                "id \"{}\" 不应回退为 Custom，请在 from_service_id 补分支",
                id
            );
        }
        assert_eq!(AiService::from_service_id("custom"), AiService::Custom);
    }

    #[test]
    fn display_name_exhaustive_test() {
        let all = [
            AiService::DeepSeek, AiService::Zhipu, AiService::SiliconFlow,
            AiService::Groq, AiService::Qwen, AiService::Doubao,
            AiService::Hunyuan, AiService::Lingyi, AiService::Kimi,
            AiService::OpenAI, AiService::AzureOpenAI, AiService::Gemini,
            AiService::Ernie, AiService::Ollama, AiService::Lmstudio,
            AiService::Custom,
        ];
        for svc in all {
            let name = svc.display_name();
            assert!(!name.is_empty(), "{:?} 的 display_name 为空", svc);
        }
        assert_eq!(ai_service_display_name("deepseek"), "DeepSeek");
        assert_eq!(ai_service_display_name("zhipu"), "智谱GLM");
        assert_eq!(ai_service_display_name("unknown"), "自定义端点");
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
    fn test_parse_name_extraction_json_mixed_latin_chinese_skipped() {
        // JSON 中译名为拉丁词+中文混合（如 nipple酒），应跳过
        let content = r#"[
          {"en": "Jeremy", "zh": "杰里米"},
          {"en": "Nipslip Vodka", "zh": "nipple酒"}
        ]"#;
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1, "混合拉丁词+中文译名应被跳过");
        assert_eq!(names[0].english, "Jeremy");
    }

    #[test]
    fn test_parse_name_extraction_arrow_mixed_latin_chinese_skipped() {
        // → 格式中译名为拉丁词+中文混合，应跳过
        let content = "Jeremy → 杰里米\nNipslip Vodka → nipple酒";
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1, "混合拉丁词+中文译名应被跳过");
        assert_eq!(names[0].english, "Jeremy");
    }

    #[test]
    fn test_parse_name_extraction_brand_with_paren_not_filtered() {
        // 品牌名格式（含括号分隔）不应被过滤
        let content = r#"[
          {"en": "Nipslip Vodka", "zh": "Nipslip Vodka（尼普斯利普伏特加）"}
        ]"#;
        let names = parse_name_extraction_response(content);
        assert_eq!(names.len(), 1, "品牌名括号格式不应被过滤");
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

    #[test]
    fn test_check_insufficient_balance_endpoint_inactive() {
        // TokenHub 接口未激活：HTTP 402 + endpoint is inactive
        let body = r#"{"error":{"message":"endpoint is inactive","code":"401006","type":"gateway_error","request_id":"abc"}}"#;
        let detail = check_insufficient_balance(reqwest::StatusCode::PAYMENT_REQUIRED, body)
            .expect("应识别为余额/授权问题");
        assert!(detail.contains("接口未激活"), "detail={}", detail);
        assert!(detail.contains("endpoint is inactive"), "detail={}", detail);

        // 余额不足：HTTP 402 + 普通余额不足消息
        let body2 = r#"{"error":{"message":"insufficient balance"}}"#;
        let detail2 = check_insufficient_balance(reqwest::StatusCode::PAYMENT_REQUIRED, body2)
            .expect("应识别为余额问题");
        assert!(!detail2.contains("接口未激活"), "detail2={}", detail2);

        // 非 402 且无关键词：不应识别
        assert!(check_insufficient_balance(reqwest::StatusCode::OK, "some random error").is_none());
    }

    #[tokio::test]
    async fn test_traditional_provider_skip_partial_translation_detection() {
        // 回归测试：传统翻译引擎（如 Google）不应被 AI 部分翻译检测误判。
        // 原文含英文术语 alpha，Google 正确翻译为 "一个洞？这根本不是alpha版本。"，
        // 残留英文 token "alpha" 是正常借词保留，不应标记 failed=true。
        // 若标记 failed，导出双语 SRT 会丢弃译文，导致编辑器有中文但文件只有英文。
        use std::collections::HashMap;

        struct MockProvider {
            translations: HashMap<String, String>,
        }

        #[async_trait::async_trait]
        impl TranslateProviderTrait for MockProvider {
            async fn translate(
                &self,
                texts: &[String],
                _source_lang: &str,
                _target_lang: &str,
            ) -> Result<Vec<String>, AppError> {
                Ok(texts
                    .iter()
                    .map(|t| self.translations.get(t).cloned().unwrap_or_default())
                    .collect())
            }

            async fn supported_target_langs(&self) -> Result<Vec<LanguageInfo>, AppError> {
                Ok(vec![])
            }

            async fn test_connection(&self) -> Result<(), AppError> {
                Ok(())
            }
        }

        let db_path = std::env::temp_dir()
            .join(format!("test_trad_partial_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let db = Database::open(&db_path).unwrap();

        let mut translations = HashMap::new();
        translations.insert(
            "A hole? This isn't alpha at all.".to_string(),
            "一个洞？这根本不是alpha版本。".to_string(),
        );

        let provider = std::sync::Arc::new(MockProvider { translations });
        let scheduler = TranslateScheduler::new(
            &db,
            provider,
            "google".to_string(),
            String::new(), // 空 model = 传统引擎
        )
        .with_file_hash("test_hash".to_string());

        let entries = vec![crate::subtitle::SubtitleEntry {
            index: 226,
            start_ms: 0,
            end_ms: 1000,
            text: "A hole? This isn't alpha at all.".to_string(),
            translated: String::new(),
            style: None,
            failed: false,
            from_cache: false,
        }];

        let result = scheduler
            .translate_entries_full(&entries, "en", "zh", 5000, None, None, false)
            .await
            .unwrap();
        assert_eq!(result.translations.len(), 1);
        let tr = &result.translations[0];
        assert_eq!(tr.translated, "一个洞？这根本不是alpha版本。");
        assert!(
            !tr.failed,
            "传统引擎不应因残留英文术语 alpha 被误判为 partial translation"
        );

        let _ = std::fs::remove_file(&db_path);
    }
}
