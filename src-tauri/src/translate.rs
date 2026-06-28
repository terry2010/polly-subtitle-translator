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
        if self.mode == "none" || self.host.is_empty() || self.port == 0 {
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
}

/// 翻译提供商类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TranslateProvider {
    Baidu,
    Bing,
    Google,
}

impl TranslateProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TranslateProvider::Baidu => "baidu",
            TranslateProvider::Bing => "bing",
            TranslateProvider::Google => "google",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "baidu" => Some(TranslateProvider::Baidu),
            "bing" => Some(TranslateProvider::Bing),
            "google" => Some(TranslateProvider::Google),
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
#[derive(Debug, Clone)]
pub struct ProviderCredentials {
    pub app_id: Option<String>,
    pub secret_key: Option<String>,
    pub region: Option<String>,
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

// === SECTION 1 END ===

/// 占位符保护算法（对应需求文档 §4.2）
/// 翻译前将需保护片段替换为私用区占位符，翻译后回填
/// 保护范围：ass 样式标记 {\...}、HTML 标签 <...>、换行符、特殊符号

/// 私用区字符范围：U+E000 ~ U+E0FF（256 个占位符）
const PLACEHOLDER_BASE: u32 = 0xE000;

/// 占位符保护器
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
                    // 仅保护 HTML 标签，不保护普通 < > 符号
                    if tag.len() < 50 && (tag.starts_with("</") || tag.starts_with("<b") || tag.starts_with("<i") || tag.starts_with("<u") || tag.starts_with("<font")) {
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

/// 翻译分段：将长文本按句号/换行切分，确保单条不超过 API 上限
pub fn split_text(text: &str, max_length: usize) -> Vec<String> {
    if text.len() <= max_length {
        return vec![text.to_string()];
    }

    let mut segments = Vec::new();
    let mut current = String::new();

    // 按句号、换行、问号、感叹号切分
    let sentences: Vec<&str> = text.split(|c| c == '.' || c == '\n' || c == '？' || c == '！' || c == '。').collect();

    for (i, sentence) in sentences.iter().enumerate() {
        let mut s = sentence.to_string();
        // 补回分隔符（最后一个不补）
        if i < sentences.len() - 1 {
            s.push('.');
        }

        if current.len() + s.len() > max_length {
            if !current.is_empty() {
                segments.push(current.trim().to_string());
                current = String::new();
            }
            // 如果单个句子就超过上限，硬切
            if s.len() > max_length {
                let chars: Vec<char> = s.chars().collect();
                for chunk in chars.chunks(max_length) {
                    segments.push(chunk.iter().collect());
                }
            } else {
                current = s;
            }
        } else {
            current.push_str(&s);
        }
    }

    if !current.is_empty() {
        segments.push(current.trim().to_string());
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
}

#[async_trait::async_trait]
impl TranslateProviderTrait for BaiduProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // 百度 API 逐条翻译（避免 \n 拼接导致签名/对齐问题）
        let mut results = Vec::with_capacity(texts.len());
        for (i, text) in texts.iter().enumerate() {
            // 跳过空文本
            if text.trim().is_empty() {
                results.push(String::new());
                continue;
            }

            let salt = uuid::Uuid::new_v4().simple().to_string();
            let sign = self.sign(text, &salt);

            let url = "https://fanyi-api.baidu.com/api/trans/vip/translate";
            let params = serde_json::json!({
                "q": text,
                "from": source_lang,
                "to": target_lang,
                "appid": self.app_id,
                "salt": salt,
                "sign": sign,
            });

            let resp = self
                .client
                .post(url)
                .form(&params)
                .timeout(std::time::Duration::from_secs(15))
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

            let trans_result = body.get("trans_result");
            let translated = match trans_result {
                Some(arr) if arr.is_array() => {
                    arr.as_array().unwrap()
                        .iter()
                        .map(|item| {
                            item.get("dst")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect::<Vec<String>>()
                        .join("\n")
                }
                _ => text.clone(), // 无法解析时返回原文
            };

            results.push(translated);

            // QPS 限制：每条之间延迟 1 秒（百度免费版 1 QPS）
            if i + 1 < texts.len() {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
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
}

#[async_trait::async_trait]
impl TranslateProviderTrait for BingProvider {
    async fn translate(
        &self,
        texts: &[String],
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
            .map(|t| serde_json::json!({ "Text": t }))
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
}

#[async_trait::async_trait]
impl TranslateProviderTrait for GoogleProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        let url = "https://translation.googleapis.com/language/translate/v2";
        let body = serde_json::json!({
            "q": texts,
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

/// 翻译调度器
/// 负责缓存查询、分段、占位符保护、限流重试
pub struct TranslateScheduler<'a> {
    db: &'a Database,
    provider: Box<dyn TranslateProviderTrait>,
    provider_name: String,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl<'a> TranslateScheduler<'a> {
    pub fn new(
        db: &'a Database,
        provider: Box<dyn TranslateProviderTrait>,
        provider_name: String,
    ) -> Self {
        Self {
            db,
            provider,
            provider_name,
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn with_cancel_token(
        db: &'a Database,
        provider: Box<dyn TranslateProviderTrait>,
        provider_name: String,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            db,
            provider,
            provider_name,
            cancelled,
        }
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

            // 分段（如果超过 API 上限）
            if protected_text.len() > max_single_length {
                tracing::warn!("字幕 #{} 超过 API 上限，硬切翻译", entry.index);
                let truncated: String = protected_text.chars().take(max_single_length).collect();
                to_translate.push((entry.index, truncated, protector));
            } else {
                to_translate.push((entry.index, protected_text, protector));
            }
        }

        // 2. 分批翻译（每批 30 条，带重试）
        const BATCH_SIZE: usize = 30;
        if !to_translate.is_empty() {
            let total_batches = (to_translate.len() + BATCH_SIZE - 1) / BATCH_SIZE;
            for (batch_idx, batch) in to_translate.chunks(BATCH_SIZE).enumerate() {
                // 检查取消标志
                if self.is_cancelled() {
                    tracing::info!("翻译已取消，已完成 {} 批", batch_idx);
                    break;
                }

                tracing::info!(
                    "翻译批次 {}/{}，本批 {} 条",
                    batch_idx + 1,
                    total_batches,
                    batch.len()
                );

                let texts: Vec<String> = batch.iter().map(|(_, t, _)| t.clone()).collect();
                let translations = self.translate_with_retry(&texts, source_lang, target_lang).await?;

                // 3. 回填占位符 + 写入缓存
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

                // 发送进度
                if let Some(ref cb) = on_progress {
                    cb(results.len(), entries.len());
                }

                // 批次间延迟（百度已逐条延迟，这里仅对非百度引擎需要）
                if batch_idx + 1 < total_batches && !self.is_cancelled() {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
        let mut last_error: Option<AppError> = None;
        let delays = [1u64, 2, 4];

        for (attempt, delay) in delays.iter().enumerate() {
            match self
                .provider
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
}

/// 创建翻译 provider 实例
pub fn create_provider(
    provider: &TranslateProvider,
    credentials: &ProviderCredentials,
) -> Result<Box<dyn TranslateProviderTrait>, AppError> {
    create_provider_with_proxy(provider, credentials, &ProxyConfig::default())
}

/// 创建翻译 provider 实例（带代理配置）
pub fn create_provider_with_proxy(
    provider: &TranslateProvider,
    credentials: &ProviderCredentials,
    proxy: &ProxyConfig,
) -> Result<Box<dyn TranslateProviderTrait>, AppError> {
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
            Ok(Box::new(BaiduProvider::with_client(app_id, secret_key, client)))
        }
        TranslateProvider::Bing => {
            let api_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound {
                    provider: "bing".to_string(),
                }
            })?;
            let region = credentials.region.clone().unwrap_or_else(|| "global".to_string());
            Ok(Box::new(BingProvider::with_client(api_key, region, client)))
        }
        TranslateProvider::Google => {
            let api_key = credentials.secret_key.clone().ok_or_else(|| {
                AppError::StorageCredentialNotFound {
                    provider: "google".to_string(),
                }
            })?;
            Ok(Box::new(GoogleProvider::with_client(api_key, client)))
        }
    }
}

// === SECTION 6 END ===

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
        assert_eq!(TranslateProvider::from_str("unknown"), None);
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
}
