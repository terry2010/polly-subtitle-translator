// 认证模块（P0）
// 联调文档 01-P0-认证模块.md
// 覆盖：access_token 内存容器 / refresh_token SQLite 存储 / Token 存储实现规范
// 后续功能点（FP-P0-2~6）在此基础上追加：refresh / WebSocket 登录 / 401 处理 / 启动初始化 / IPC

use crate::db::Database;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

// === SECTION 0: 认证错误类型 ===

/// 认证模块错误（不直接映射到 AppError，由调用方决定如何处理）
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// refresh_token 已失效（401），需清除凭据并跳转登录页
    #[error("refresh_token expired or revoked")]
    RefreshTokenExpired,

    /// refresh 请求失败（非 401 的 HTTP 错误或响应解析失败）
    #[error("refresh failed: {0}")]
    RefreshFailed(String),

    /// 网络错误（DNS/连接超时/服务端不可达）
    #[error("network error: {0}")]
    NetworkError(String),

    /// WebSocket 连接失败
    #[error("websocket connection failed: {0}")]
    WebSocketConnectFailed(String),

    /// WebSocket 消息解析失败
    #[error("websocket message parse failed: {0}")]
    WebSocketMessageParseFailed(String),

    /// 登录超时（auth_timeout）
    #[error("login timeout")]
    LoginTimeout,

    /// 登录失败（auth_error，含错误码和消息）
    #[error("login failed: {error} - {message}")]
    LoginFailed { error: String, message: String },

    /// 数据库错误
    #[error("database error: {0}")]
    Database(#[from] AppError),

    /// HTTP 请求构建错误
    #[error("http request build error: {0}")]
    HttpRequestBuilder(#[from] reqwest::Error),
}

// === SECTION 0 END ===

// === SECTION 1: 常量与数据结构 ===

/// refresh_token 在 SQLite credentials 表中的 entry_name
pub const REFRESH_TOKEN_ENTRY: &str = "zimufan:refresh_token";

/// access_token 默认有效期（秒），仅在服务端未返回 expires_in 时用作兜底
const DEFAULT_ACCESS_TOKEN_TTL: u64 = 3600;

/// Token 对（用于序列化/IPC 传递，不含过期时间）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<i64>, // Unix timestamp（秒），仅用于 IPC 透传，内部判断用 Instant
}

/// 用户信息（GET /auth/me 与 WebSocket auth_success.user 共用结构）
/// 联调文档 01-P0 第 192-203 行字段说明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub email: String,
    pub token_balance: i64,
    pub bonus_balance: i64,
    pub traffic_pack_balance: i64,
    pub status: String,             // active / banned / pending
    pub vip_level: String,          // "free" / "vip1" ... "vip6"
    pub vip_expires_at: Option<String>,
    pub bonus_expires_at: Option<String>,
    pub traffic_pack_expires_at: Option<String>,
    pub max_concurrent_jobs: i32,
    pub available_models: Vec<ModelOption>,
}

/// 翻译档位选项（联调文档 01-P0 第 205-212 行）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOption {
    pub model: String,       // polly-fast / polly-standard / polly-fine
    pub name: String,        // 中文展示名
    pub description: String, // 描述
    pub enabled: bool,       // 当前用户是否可用
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_url: Option<String>, // 价格页面链接（可选）
}

// === SECTION 1 END ===

// === SECTION 2: access_token 内存容器 ===

// access_token 内存容器（token + 过期时间）
// 使用 std::time::Instant（单调时钟，不受系统时间调整影响）
static ACCESS_TOKEN: RwLock<Option<(String, Instant)>> = RwLock::const_new(None);

// 并发 refresh 防护锁（FP-P0-2/4 使用，此处先声明）
static REFRESH_LOCK: Mutex<()> = Mutex::const_new(());

/// 获取当前未过期的 access_token
/// 返回 None 表示无 token 或已过期，调用方需触发 refresh
pub async fn get_access_token() -> Option<String> {
    let guard = ACCESS_TOKEN.read().await;
    guard
        .as_ref()
        .filter(|(_, exp)| *exp > Instant::now())
        .map(|(t, _)| t.clone())
}

/// 获取 access_token 的过期时间（单调时钟），供后台刷新循环计算 sleep 时长
pub async fn get_access_token_expiry() -> Option<Instant> {
    let guard = ACCESS_TOKEN.read().await;
    guard.as_ref().map(|(_, exp)| *exp)
}

/// 存储 access_token 到内存
/// expires_in 为服务端返回的有效期（秒）；存真实过期时间，不提前减 300 秒
/// 提前刷新由 start_token_refresh_loop 负责（唯一一层提前量）
pub async fn set_access_token(token: String, expires_in: u64) {
    let ttl = if expires_in == 0 {
        DEFAULT_ACCESS_TOKEN_TTL
    } else {
        expires_in
    };
    let exp = Instant::now() + Duration::from_secs(ttl);
    let mut guard = ACCESS_TOKEN.write().await;
    *guard = Some((token, exp));
}

/// 清除内存中的 access_token（不清除 refresh_token）
pub async fn clear_access_token() {
    let mut guard = ACCESS_TOKEN.write().await;
    *guard = None;
}

/// 获取并发 refresh 防护锁的引用（供 FP-P0-2/4 使用）
pub(crate) fn refresh_lock() -> &'static Mutex<()> {
    &REFRESH_LOCK
}

// === SECTION 2 END ===

// === SECTION 3: refresh_token SQLite 存储 ===

/// 从 SQLite credentials 表读取 refresh_token
/// 返回 Ok(None) 表示无存储的 refresh_token（需登录）
pub fn load_refresh_token(db: &Database) -> Result<Option<String>, AppError> {
    db.get_credential(REFRESH_TOKEN_ENTRY)
}

/// 保存 refresh_token 到 SQLite credentials 表（UPSERT）
/// 滑动过期：每次 refresh 成功后用新值替换旧值
pub fn save_refresh_token(db: &Database, token: &str) -> Result<(), AppError> {
    db.set_credential(REFRESH_TOKEN_ENTRY, token)?;
    tracing::info!("refresh_token 已保存到 credentials 表");
    Ok(())
}

/// 清除 SQLite credentials 表中的 refresh_token
/// 用于：refresh 失败（401）/ 用户登出
pub fn clear_refresh_token(db: &Database) -> Result<(), AppError> {
    db.delete_credential(REFRESH_TOKEN_ENTRY)?;
    tracing::info!("refresh_token 已从 credentials 表清除");
    Ok(())
}

// === SECTION 3 END ===

// === SECTION 4: API base URL + Token 刷新（FP-P0-2）===

/// config 表中存储 API base URL 的 key
const API_BASE_URL_KEY: &str = "zimufan_api_base_url";

/// 默认 API base URL（生产环境）
const DEFAULT_API_BASE_URL: &str = "https://api.zimufan.com";

/// 默认站点 URL（生产环境，浏览器登录页）
const DEFAULT_SITE_URL: &str = "https://zimufan.com";

/// 从 config 表读取 API base URL，无配置时用默认值
pub fn get_api_base_url(db: &Database) -> String {
    db.get_config(API_BASE_URL_KEY)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_API_BASE_URL.to_string())
}

/// 获取站点 URL（浏览器登录页用）
/// 开发环境：API 是 localhost:8081 → 站点是 localhost:8080
/// 生产环境：API 是 api.zimufan.com → 站点是 zimufan.com
fn get_site_url(api_base_url: &str) -> String {
    if api_base_url.contains("localhost:8081") {
        "http://localhost:8080".to_string()
    } else if api_base_url == "https://api.zimufan.com" {
        DEFAULT_SITE_URL.to_string()
    } else {
        // 其他情况：假设站点和 API 同域
        api_base_url.to_string()
    }
}

/// 设置 API base URL（供设置页切换环境用）
pub fn set_api_base_url(db: &Database, url: &str) -> Result<(), AppError> {
    db.set_config(API_BASE_URL_KEY, url)
}

/// POST /auth/refresh 响应体（联调文档 01-P0 第 356-363 行）
#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    #[serde(default)]
    refresh_expires_in: u64,
}

/// Token 刷新结果
#[derive(Debug)]
pub struct RefreshResult {
    pub access_token: String,
    pub expires_in: u64,
}

/// 刷新 access_token（POST /auth/refresh）
///
/// 联调文档 01-P0 SECTION 5（Token 刷新）：
/// - 用 refresh_token 换新的 access_token + 新的 refresh_token（滑动过期）
/// - 新 refresh_token 替换 SQLite 中的旧值（旧 refresh_token 已作废）
/// - 并发防护：REFRESH_LOCK 保证同一时间只 refresh 一次
///
/// # 参数
/// - `db`: 数据库（读写 refresh_token）
/// - `client`: reqwest 客户端（复用代理配置）
///
/// # 返回
/// - `Ok(Some(RefreshResult))`: 刷新成功，access_token 已存内存，refresh_token 已更新 SQLite
/// - `Ok(None)`: 无 refresh_token（未登录），不执行刷新
/// - `Err(AuthError::RefreshFailed)`: refresh 失败（401 或网络错误），refresh_token 已清除
pub async fn refresh_access_token(
    db: &Database,
    client: &reqwest::Client,
) -> Result<Option<RefreshResult>, AuthError> {
    // 1. 读取 refresh_token
    let refresh_token = match load_refresh_token(db)? {
        Some(t) => t,
        None => return Ok(None),
    };

    // 2. 获取 REFRESH_LOCK（并发防护：多个 401 只 refresh 一次）
    let _lock = refresh_lock().lock().await;

    // 拿到锁后再次检查：是否已有其他请求完成了 refresh
    // 如果 access_token 已存在且未过期，说明其他请求刚 refresh 过，直接返回
    if let Some(token) = get_access_token().await {
        tracing::debug!("refresh_lock 获取后发现 access_token 已有效，跳过刷新");
        return Ok(Some(RefreshResult {
            access_token: token,
            expires_in: 0, // 调用方不需要 expires_in，因为 token 已存在
        }));
    }

    // 3. 调用 POST /auth/refresh
    let base_url = get_api_base_url(db);
    let url = format!("{}/auth/refresh", base_url.trim_end_matches('/'));

    tracing::info!("发起 token 刷新请求: POST {}", url);

    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().as_u16() == 200 => {
            let body: RefreshResponse = r.json().await.map_err(|e| {
                tracing::error!("解析 refresh 响应失败: {}", e);
                AuthError::RefreshFailed("响应解析失败".to_string())
            })?;

            // 4. 存新 access_token 到内存
            set_access_token(body.access_token.clone(), body.expires_in).await;

            // 5. 存新 refresh_token 到 SQLite（滑动过期：旧 token 已作废）
            save_refresh_token(db, &body.refresh_token)?;

            tracing::info!(
                "token 刷新成功，expires_in={}s, refresh_expires_in={}s",
                body.expires_in,
                body.refresh_expires_in
            );

            Ok(Some(RefreshResult {
                access_token: body.access_token,
                expires_in: body.expires_in,
            }))
        }
        Ok(r) if r.status().as_u16() == 401 => {
            // refresh_token 已失效（过期或被撤销）
            tracing::warn!("refresh_token 已失效（401），清除凭据");
            clear_access_token().await;
            clear_refresh_token(db)?;
            Err(AuthError::RefreshTokenExpired)
        }
        Ok(r) => {
            // 其他状态码（5xx 等）
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            tracing::error!("refresh 请求失败: status={}, body={}", status, body);
            Err(AuthError::RefreshFailed(format!("HTTP {}", status)))
        }
        Err(e) => {
            // 网络错误（DNS/连接超时/服务端不可达）
            tracing::error!("refresh 请求网络错误: {}", e);
            Err(AuthError::NetworkError(e.to_string()))
        }
    }
}

// === SECTION 4 END ===

// === SECTION 5: WebSocket 登录流程（FP-P0-3）===

use tokio_tungstenite::tungstenite::Message as WsMessage;

/// WebSocket 推送消息类型（联调文档 01-P0 第 155-243 行）
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum WsAuthMessage {
    #[serde(rename = "session_created")]
    SessionCreated { session_id: String },
    #[serde(rename = "auth_success")]
    AuthSuccess {
        access_token: String,
        refresh_token: String,
        #[serde(default)]
        expires_in: u64,
        user: UserInfo,
    },
    #[serde(rename = "auth_error")]
    AuthError { error: String, message: String },
    #[serde(rename = "auth_timeout")]
    AuthTimeout { message: String },
}

/// 登录结果（auth_success 时返回）
#[derive(Debug, Clone, Serialize)]
pub struct LoginSuccess {
    pub user: UserInfo,
    pub expires_in: u64,
}

/// 登录页 URL 构造（浏览器打开）
/// 联调文档 01-P0 第 109-111 行
fn build_login_page_url(base_url: &str, session_id: &str) -> String {
    format!(
        "{}/auth/client-login?session_id={}",
        base_url.trim_end_matches('/'),
        session_id
    )
}

/// WebSocket URL 构造
/// 联调文档 01-P0 第 113-115 行
/// base_url 的 http(s) → ws(s) 转换
fn build_websocket_url(base_url: &str, session_id: &str) -> String {
    let ws_base = if base_url.starts_with("https://") {
        base_url.replacen("https://", "wss://", 1)
    } else if base_url.starts_with("http://") {
        base_url.replacen("http://", "ws://", 1)
    } else {
        base_url.to_string()
    };
    format!(
        "{}/auth/wait?session_id={}",
        ws_base.trim_end_matches('/'),
        session_id
    )
}

/// 启动 WebSocket 登录流程
///
/// 联调文档 01-P0 SECTION 3（登录流程）：
/// 1. 生成 session_id (UUID v4)
/// 2. 打开浏览器登录页
/// 3. 连 WebSocket wss://auth/wait?session_id=X
/// 4. 等待服务端推送 auth_success / auth_error / auth_timeout
/// 5. auth_success → access_token 存内存 + refresh_token 存 SQLite
/// 6. auth_error → 返回错误（session_id 仍有效，可重试）
/// 7. auth_timeout → 返回 LoginTimeout（session_id 作废，需重新生成）
pub async fn login_via_websocket(
    db: &Database,
    base_url: &str,
) -> Result<LoginSuccess, AuthError> {
    login_via_websocket_with_browser(db, base_url, open_browser_default).await
}

/// 默认的浏览器打开实现（调用 open::that）
fn open_browser_default(url: &str) {
    if let Err(e) = open::that(url) {
        tracing::warn!("打开浏览器失败: {}（用户可手动访问 {}）", e, url);
    }
}

/// WebSocket 登录流程（可注入浏览器打开行为，供测试使用）
/// open_browser: 打开浏览器的回调函数，测试中传空闭包避免真的打开浏览器
async fn login_via_websocket_with_browser(
    db: &Database,
    base_url: &str,
    open_browser: impl FnOnce(&str),
) -> Result<LoginSuccess, AuthError> {
    // 1. 连 WebSocket（不带 session_id，由服务器生成并返回）
    let ws_url = build_websocket_url(base_url, "");
    let ws_url = if ws_url.ends_with("?session_id=") {
        ws_url.trim_end_matches("?session_id=").to_string()
    } else {
        ws_url
    };
    tracing::info!("WebSocket URL: {}", ws_url);

    // WebSocket 连接加 5 秒超时，网络不通时快速失败
    let connect_result = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&ws_url),
    )
    .await;
    let (ws_stream, _response) = match connect_result {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            tracing::error!("WebSocket 连接失败: {}", e);
            return Err(AuthError::WebSocketConnectFailed(e.to_string()));
        }
        Err(_) => {
            tracing::error!("WebSocket 连接超时（5秒）");
            return Err(AuthError::WebSocketConnectFailed(
                "连接服务器超时，请检查网络后重试".to_string(),
            ));
        }
    };

    tracing::info!("WebSocket 已连接，等待 session_created...");

    use futures_util::StreamExt;
    let mut ws_stream = ws_stream;

    // 2. 读第一条消息，必须是 session_created
    let session_id = loop {
        let msg_result = ws_stream.next().await.ok_or_else(|| {
            AuthError::WebSocketConnectFailed("WebSocket 在 session_created 前关闭".to_string())
        })?;
        let msg = msg_result.map_err(|e| {
            tracing::error!("WebSocket 读取错误: {}", e);
            AuthError::WebSocketConnectFailed(e.to_string())
        })?;
        let text = match msg {
            WsMessage::Text(t) => t.to_string(),
            WsMessage::Close(_) => {
                return Err(AuthError::WebSocketConnectFailed(
                    "WebSocket 被服务端关闭".to_string(),
                ));
            }
            _ => continue,
        };
        tracing::debug!("收到 WebSocket 消息: {}", text);
        let parsed: WsAuthMessage = serde_json::from_str(&text).map_err(|e| {
            tracing::error!("WebSocket 消息解析失败: {} - 原文: {}", e, text);
            AuthError::WebSocketMessageParseFailed(e.to_string())
        })?;
        match parsed {
            WsAuthMessage::SessionCreated { session_id } => {
                tracing::info!("收到 session_id={}", session_id);
                break session_id;
            }
            WsAuthMessage::AuthError { error, message } => {
                return Err(AuthError::LoginFailed { error, message });
            }
            _ => {
                tracing::warn!("期望 session_created，但收到其他消息，忽略");
            }
        }
    };

    // 3. 拿到 session_id 后，打开浏览器登录页
    let site_url = get_site_url(base_url);
    let login_url = build_login_page_url(&site_url, &session_id);
    tracing::info!("浏览器登录页 URL: {}", login_url);
    open_browser(&login_url);

    // 4. 继续读 WebSocket 消息，等待 auth_success / auth_error / auth_timeout
    while let Some(msg_result) = ws_stream.next().await {
        let msg = msg_result.map_err(|e| {
            tracing::error!("WebSocket 读取错误: {}", e);
            AuthError::WebSocketConnectFailed(e.to_string())
        })?;

        let text = match msg {
            WsMessage::Text(t) => t.to_string(),
            WsMessage::Close(_) => {
                tracing::info!("WebSocket 已关闭");
                return Err(AuthError::WebSocketConnectFailed(
                    "WebSocket 被服务端关闭".to_string(),
                ));
            }
            _ => continue,
        };

        tracing::debug!("收到 WebSocket 消息: {}", text);

        let parsed: WsAuthMessage = serde_json::from_str(&text).map_err(|e| {
            tracing::error!("WebSocket 消息解析失败: {} - 原文: {}", e, text);
            AuthError::WebSocketMessageParseFailed(e.to_string())
        })?;

        match parsed {
            WsAuthMessage::SessionCreated { .. } => {
                // 忽略重复的 session_created
            }
            WsAuthMessage::AuthSuccess {
                access_token,
                refresh_token,
                expires_in,
                user,
            } => {
                tracing::info!(
                    "登录成功！user_id={}, email={}, vip_level={}",
                    user.id,
                    user.email,
                    user.vip_level
                );

                set_access_token(access_token, expires_in).await;
                save_refresh_token(db, &refresh_token)?;

                let _ = ws_stream.close(None).await;

                return Ok(LoginSuccess { user, expires_in });
            }
            WsAuthMessage::AuthError { error, message } => {
                tracing::warn!("登录失败: {} - {}", error, message);
                return Err(AuthError::LoginFailed { error, message });
            }
            WsAuthMessage::AuthTimeout { message } => {
                tracing::warn!("登录超时: {}", message);
                return Err(AuthError::LoginTimeout);
            }
        }
    }

    tracing::warn!("WebSocket 流结束，未收到任何认证消息");
    Err(AuthError::WebSocketConnectFailed(
        "WebSocket 连接在收到认证消息前被关闭".to_string(),
    ))
}

// === SECTION 5 END ===

// === SECTION 6: 401 处理 + 后台 token 刷新循环（FP-P0-4）===

/// 带认证的 HTTP 请求封装（401 自动 refresh + 重试 1 次）
///
/// 联调文档 01-P0 SECTION 7（401 处理）：
/// - 请求带 Bearer access_token
/// - 收到 401 → 自动 refresh → 重试 1 次（最多 1 次防无限循环）
/// - refresh 失败（401）→ 清除凭据，返回 AuthError::RefreshTokenExpired
///
/// # 参数
/// - `db`: 数据库
/// - `client`: reqwest 客户端
/// - `method`: HTTP 方法
/// - `url`: 完整 URL
/// - `body`: 可选请求体（已序列化的 JSON 字符串）
///
/// # 返回
/// - `Ok(reqwest::Response)`: 请求成功（含非 401 的错误响应）
/// - `Err(AuthError::RefreshTokenExpired)`: refresh 失败，需跳转登录页
/// - `Err(AuthError::NetworkError)`: 网络错误
pub async fn authenticated_request(
    db: &Database,
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    body: Option<&str>,
) -> Result<reqwest::Response, AuthError> {
    // 第一次尝试
    let resp = send_with_token(client, method.clone(), url, body).await?;

    // 401 → refresh + 重试 1 次
    if resp.status().as_u16() == 401 {
        tracing::warn!("收到 401，尝试 refresh token 后重试");
        // 消费掉 401 响应体（释放连接）
        drop(resp);

        // 清除已失效的 access_token，确保 refresh 能真正执行
        // （refresh_access_token 的锁后双检查会跳过仍有效的 token）
        clear_access_token().await;

        // refresh token
        match refresh_access_token(db, client).await? {
            Some(_) => {
                // refresh 成功，重试 1 次
                tracing::info!("refresh 成功，重试请求");
                let retry_resp =
                    send_with_token(client, method, url, body).await?;
                Ok(retry_resp)
            }
            None => {
                // 无 refresh_token（未登录）
                tracing::warn!("无 refresh_token，无法 refresh");
                Err(AuthError::RefreshTokenExpired)
            }
        }
    } else {
        Ok(resp)
    }
}

/// 用当前 access_token 发送请求（内部辅助函数）
async fn send_with_token(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    body: Option<&str>,
) -> Result<reqwest::Response, AuthError> {
    let token = get_access_token()
        .await
        .ok_or(AuthError::RefreshTokenExpired)?;

    let mut req = client.request(method, url).bearer_auth(&token);
    if let Some(b) = body {
        req = req.header("Content-Type", "application/json").body(b.to_string());
    }

    req.send()
        .await
        .map_err(|e| {
            tracing::error!("HTTP 请求失败: {} - {}", url, e);
            AuthError::NetworkError(e.to_string())
        })
}

/// 带认证的 multipart 请求封装（401 自动 refresh + 重试 1 次）
/// 与 authenticated_request 相同的 401 处理逻辑，但接受一个 form 构建闭包
/// （reqwest::multipart::Form 未实现 Clone，重试时需重新构建）
pub async fn authenticated_multipart_request<F>(
    db: &Database,
    client: &reqwest::Client,
    url: &str,
    build_form: F,
) -> Result<reqwest::Response, AuthError>
where
    F: Fn() -> reqwest::multipart::Form,
{
    // 第一次尝试
    let resp = send_multipart_with_token(client, url, build_form()).await?;

    // 401 → refresh + 重试 1 次
    if resp.status().as_u16() == 401 {
        tracing::warn!("收到 401，尝试 refresh token 后重试");
        drop(resp);
        clear_access_token().await;

        match refresh_access_token(db, client).await? {
            Some(_) => {
                tracing::info!("refresh 成功，重试 multipart 请求");
                let retry_resp = send_multipart_with_token(client, url, build_form()).await?;
                Ok(retry_resp)
            }
            None => {
                tracing::warn!("无 refresh_token，无法 refresh");
                Err(AuthError::RefreshTokenExpired)
            }
        }
    } else {
        Ok(resp)
    }
}

/// 用当前 access_token 发送 multipart 请求（内部辅助函数）
async fn send_multipart_with_token(
    client: &reqwest::Client,
    url: &str,
    form: reqwest::multipart::Form,
) -> Result<reqwest::Response, AuthError> {
    let token = get_access_token()
        .await
        .ok_or(AuthError::RefreshTokenExpired)?;

    let req = client
        .request(reqwest::Method::POST, url)
        .bearer_auth(&token)
        .multipart(form);

    req.send()
        .await
        .map_err(|e| {
            tracing::error!("HTTP multipart 请求失败: {} - {}", url, e);
            AuthError::NetworkError(e.to_string())
        })
}

/// 后台 token 刷新循环
///
/// 联调文档 01-P0 SECTION 8（后台刷新循环）：
/// - 过期前 5 分钟自动刷新
/// - 并发 401 只 refresh 一次（由 REFRESH_LOCK 保证）
/// - refresh 失败 → 停止循环，等待用户重新登录
///
/// 这个函数会一直运行，直到 access_token 被清除（登出/refresh 失败）
/// 应该用 tokio::spawn 在后台运行
pub async fn start_token_refresh_loop(
    db: std::sync::Arc<Database>,
    client: reqwest::Client,
) {
    start_token_refresh_loop_with_config(db, client, RefreshLoopConfig::default()).await
}

/// 刷新循环配置（测试可注入短间隔）
struct RefreshLoopConfig {
    refresh_ahead: Duration,   // 过期前多久刷新
    no_token_check: Duration,  // 无 token 时检查间隔
    retry_on_error: Duration,  // 网络错误重试间隔
}

impl Default for RefreshLoopConfig {
    fn default() -> Self {
        Self {
            refresh_ahead: Duration::from_secs(300),    // 5 分钟
            no_token_check: Duration::from_secs(60),    // 60 秒
            retry_on_error: Duration::from_secs(30),    // 30 秒
        }
    }
}

async fn start_token_refresh_loop_with_config(
    db: std::sync::Arc<Database>,
    client: reqwest::Client,
    config: RefreshLoopConfig,
) {
    tracing::info!("启动 token 刷新后台循环");

    loop {
        let expiry = get_access_token_expiry().await;

        let sleep_duration = match expiry {
            Some(exp) => {
                let now = std::time::Instant::now();
                if exp <= now {
                    Duration::from_secs(0)
                } else {
                    let remaining = exp - now;
                    if remaining > config.refresh_ahead {
                        remaining - config.refresh_ahead
                    } else {
                        Duration::from_secs(0)
                    }
                }
            }
            None => config.no_token_check,
        };

        tokio::time::sleep(sleep_duration).await;

        if get_access_token().await.is_none() && load_refresh_token(&db).ok().flatten().is_none() {
            tracing::info!("token 刷新循环检测到无 token，退出循环");
            break;
        }

        match refresh_access_token(&db, &client).await {
            Ok(Some(_)) => {
                tracing::info!("后台 token 刷新成功");
            }
            Ok(None) => {
                tracing::debug!("无 refresh_token，等待用户登录");
            }
            Err(AuthError::RefreshTokenExpired) => {
                tracing::warn!("refresh_token 已失效，停止刷新循环");
                break;
            }
            Err(AuthError::NetworkError(e)) => {
                tracing::warn!("刷新网络错误: {}，重试中", e);
                tokio::time::sleep(config.retry_on_error).await;
            }
            Err(e) => {
                tracing::error!("刷新失败: {:?}，重试中", e);
                tokio::time::sleep(config.retry_on_error).await;
            }
        }
    }

    tracing::info!("token 刷新后台循环已停止");
}

// === SECTION 6 END ===

// === SECTION 7: 应用启动 auth 初始化 + GET /auth/me（FP-P0-5）===

/// GET /auth/me 响应体（联调文档 01-P0 第 425-440 行）
/// 与 UserInfo 结构一致，直接复用 UserInfo 反序列化

/// 应用启动时的 auth 初始化结果
#[derive(Debug, Clone, Serialize)]
pub struct AuthInitResult {
    /// 用户信息（成功获取时）
    pub user: Option<UserInfo>,
    /// 是否为离线模式（网络错误时降级）
    pub offline: bool,
    /// 错误信息（如有）
    pub error: Option<String>,
}

/// GET /auth/me 调用
///
/// 联调文档 01-P0 SECTION 9：
/// - 需要 Bearer access_token
/// - 返回当前用户信息（id/email/token_balance/status/vip_level/max_concurrent_jobs/available_models）
///
/// # 参数
/// - `db`: 数据库（获取 access_token + API base URL）
/// - `client`: reqwest 客户端
pub async fn fetch_user_info(
    db: &Database,
    client: &reqwest::Client,
) -> Result<UserInfo, AuthError> {
    let base_url = get_api_base_url(db);
    let url = format!("{}/auth/me", base_url.trim_end_matches('/'));

    tracing::info!("获取用户信息: GET {}", url);

    let resp = authenticated_request(
        db,
        client,
        reqwest::Method::GET,
        &url,
        None,
    )
    .await?;

    let status = resp.status();
    if status.as_u16() == 200 {
        let user: UserInfo = resp.json().await.map_err(|e| {
            tracing::error!("解析 /auth/me 响应失败: {}", e);
            AuthError::RefreshFailed(format!("/auth/me 响应解析失败: {}", e))
        })?;
        tracing::info!(
            "用户信息获取成功: id={}, email={}, vip_level={}",
            user.id,
            user.email,
            user.vip_level
        );
        Ok(user)
    } else {
        let body = resp.text().await.unwrap_or_default();
        tracing::error!("/auth/me 失败: status={}, body={}", status, body);
        Err(AuthError::RefreshFailed(format!(
            "/auth/me 返回 HTTP {}",
            status
        )))
    }
}

/// 应用启动 auth 初始化
///
/// 联调文档 01-P0 SECTION 9（启动流程）：
/// 1. 尝试 refresh token（如有 refresh_token）
/// 2. 调 GET /auth/me 获取用户信息
/// 3. 网络错误 → 降级离线模式（不阻塞进主界面）
/// 4. 指数退避重试 3 次（2s → 4s → 8s）
///
/// # 返回
/// - `AuthInitResult { user: Some(...), offline: false }`: 初始化成功
/// - `AuthInitResult { user: None, offline: true, error: Some(...) }`: 网络错误，离线模式
/// - `AuthInitResult { user: None, offline: false, error: Some(...) }`: refresh 失败（需登录）
pub async fn init_auth_on_startup(
    db: &Database,
    client: &reqwest::Client,
) -> AuthInitResult {
    // 检查是否有 refresh_token
    let has_refresh = load_refresh_token(db).ok().flatten().is_some();

    if !has_refresh {
        tracing::info!("无 refresh_token，跳过启动初始化");
        return AuthInitResult {
            user: None,
            offline: false,
            error: Some("未登录".to_string()),
        };
    }

    // 指数退避重试 3 次（2s → 4s → 8s）
    let backoff = [2u64, 4, 8];

    for attempt in 0..=backoff.len() {
        // 先 refresh
        match refresh_access_token(db, client).await {
            Ok(Some(_)) => {
                // refresh 成功，调 /auth/me
                match fetch_user_info(db, client).await {
                    Ok(user) => {
                        return AuthInitResult {
                            user: Some(user),
                            offline: false,
                            error: None,
                        };
                    }
                    Err(AuthError::RefreshTokenExpired) => {
                        // refresh_token 在 /auth/me 时失效（不太可能但处理）
                        return AuthInitResult {
                            user: None,
                            offline: false,
                            error: Some("登录已过期，请重新登录".to_string()),
                        };
                    }
                    Err(AuthError::NetworkError(e)) => {
                        // 网络错误，尝试重试
                        if attempt < backoff.len() {
                            tracing::warn!(
                                "/auth/me 网络错误: {}，{}s 后重试",
                                e,
                                backoff[attempt]
                            );
                            tokio::time::sleep(Duration::from_secs(backoff[attempt])).await;
                            continue;
                        }
                        // 重试耗尽，降级离线模式
                        tracing::warn!("重试耗尽，降级离线模式");
                        return AuthInitResult {
                            user: None,
                            offline: true,
                            error: Some(format!("网络错误: {}", e)),
                        };
                    }
                    Err(e) => {
                        // 其他错误（如响应解析失败）
                        if attempt < backoff.len() {
                            tracing::warn!(
                                "/auth/me 错误: {:?}，{}s 后重试",
                                e,
                                backoff[attempt]
                            );
                            tokio::time::sleep(Duration::from_secs(backoff[attempt])).await;
                            continue;
                        }
                        return AuthInitResult {
                            user: None,
                            offline: false,
                            error: Some(format!("{}", e)),
                        };
                    }
                }
            }
            Ok(None) => {
                // 无 refresh_token（不该到这里，前面已检查）
                return AuthInitResult {
                    user: None,
                    offline: false,
                    error: Some("未登录".to_string()),
                };
            }
            Err(AuthError::RefreshTokenExpired) => {
                // refresh_token 已失效
                return AuthInitResult {
                    user: None,
                    offline: false,
                    error: Some("登录已过期，请重新登录".to_string()),
                };
            }
            Err(AuthError::NetworkError(e)) => {
                // 网络错误，尝试重试
                if attempt < backoff.len() {
                    tracing::warn!(
                        "refresh 网络错误: {}，{}s 后重试",
                        e,
                        backoff[attempt]
                    );
                    tokio::time::sleep(Duration::from_secs(backoff[attempt])).await;
                    continue;
                }
                // 重试耗尽，降级离线模式
                tracing::warn!("重试耗尽，降级离线模式");
                return AuthInitResult {
                    user: None,
                    offline: true,
                    error: Some(format!("网络错误: {}", e)),
                };
            }
            Err(e) => {
                // 其他 refresh 错误
                if attempt < backoff.len() {
                    tracing::warn!(
                        "refresh 错误: {:?}，{}s 后重试",
                        e,
                        backoff[attempt]
                    );
                    tokio::time::sleep(Duration::from_secs(backoff[attempt])).await;
                    continue;
                }
                return AuthInitResult {
                    user: None,
                    offline: false,
                    error: Some(format!("{}", e)),
                };
            }
        }
    }

    // 不该到这里
    AuthInitResult {
        user: None,
        offline: false,
        error: Some("初始化失败".to_string()),
    }
}

/// 手动刷新 token 并获取用户信息（IPC auth_refresh 的核心逻辑）
///
/// 行为：
/// - refresh 成功 → 调 /auth/me → 成功返回 Some(user)，失败降级返回 None
/// - refresh 无 token → 返回 None
/// - refresh 失败 → 返回错误
pub async fn refresh_and_fetch_user_info(
    db: &Database,
    client: &reqwest::Client,
) -> Result<Option<UserInfo>, AuthError> {
    match refresh_access_token(db, client).await {
        Ok(Some(_)) => {
            match fetch_user_info(db, client).await {
                Ok(user) => Ok(Some(user)),
                Err(_) => Ok(None),
            }
        }
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

// === SECTION 7 END ===

// === SECTION 9: 官方服务翻译请求（FP-P1-2）===

/// 翻译提交响应（POST /v2/translate 返回的 3 种情况）
pub enum TranslateSubmitResponse {
    /// 200 + text/event-stream：SSE 流式响应，需要解析事件流
    /// job_id 从 response header X-Job-Id 提取，用于取消任务
    SseStream { response: reqwest::Response, job_id: Option<String> },
    /// 200 + application/json：非流式直接结果（断点续传时任务已完成）
    CompletedJson(TranslateResult),
    /// 202：异步处理，需轮询 GET /v2/translate/{job_id}
    Accepted { job_id: String },
    /// 错误响应（402/403/413/429/500/503 等）
    Error {
        status: u16,
        error_code: String,
        message: String,
    },
}

impl std::fmt::Debug for TranslateSubmitResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslateSubmitResponse::SseStream { job_id, .. } => write!(f, "SseStream(<response>, job_id={:?})", job_id),
            TranslateSubmitResponse::CompletedJson(r) => write!(f, "CompletedJson({:?})", r),
            TranslateSubmitResponse::Accepted { job_id } => write!(f, "Accepted({})", job_id),
            TranslateSubmitResponse::Error { status, error_code, message } => {
                write!(f, "Error({}: {} - {})", status, error_code, message)
            }
        }
    }
}

/// 官方翻译错误
#[derive(Debug)]
pub enum TranslateError {
    /// 网络错误
    NetworkError(String),
    /// access_token 无效（refresh 也失败）
    AuthError(AuthError),
    /// 服务端返回的错误（402/403/413/429/500/503）
    ServerError { status: u16, error_code: String, message: String },
    /// 响应解析失败
    ParseError(String),
    /// VTT 等不支持的格式
    UnsupportedFormat(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslateError::NetworkError(e) => write!(f, "网络错误: {}", e),
            TranslateError::AuthError(e) => write!(f, "认证错误: {:?}", e),
            TranslateError::ServerError { status, error_code, message } => {
                write!(f, "服务端错误 {}: {} - {}", status, error_code, message)
            }
            TranslateError::ParseError(e) => write!(f, "解析失败: {}", e),
            TranslateError::UnsupportedFormat(e) => write!(f, "不支持的格式: {}", e),
        }
    }
}

impl From<AuthError> for TranslateError {
    fn from(e: AuthError) -> Self {
        TranslateError::AuthError(e)
    }
}

/// 生成 idempotency_key（UUID v4）
pub fn generate_idempotency_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 提交翻译请求（POST /v2/translate/file，multipart 文件上传）
/// 联调文档 02-P1 SECTION 1：multipart/form-data 上传字幕文件
/// 返回 TranslateSubmitResponse，调用方根据类型处理
pub async fn submit_translate(
    db: &Database,
    client: &reqwest::Client,
    subtitle_content: &str,
    file_format: &str,
    model: &str,
    idempotency_key: &str,
) -> Result<TranslateSubmitResponse, TranslateError> {
    let base_url = get_api_base_url(db);
    let url = format!("{}/v2/translate/file", base_url);

    // 文件名根据格式确定扩展名（服务端通过扩展名 + 内容检测格式）
    let filename = match file_format.to_lowercase().as_str() {
        "ass" | "ssa" => "subtitle.ass",
        _ => "subtitle.srt",
    };

    // 构建 multipart form 的闭包（401 重试时需重新构建，因 Form 未实现 Clone）
    let subtitle_bytes = subtitle_content.as_bytes().to_vec();
    let idempotency_key_owned = idempotency_key.to_string();
    let model_owned = model.to_string();
    let build_form = move || {
        let part = reqwest::multipart::Part::bytes(subtitle_bytes.clone())
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .unwrap();
        reqwest::multipart::Form::new()
            .part("file", part)
            .text("idempotency_key", idempotency_key_owned.clone())
            .text("source_lang", "en".to_string())
            .text("target_lang", "zh".to_string())
            .text("model", model_owned.clone())
    };

    tracing::info!(
        "提交翻译请求: POST {} (multipart), file={}, model={}, idempotency_key={}",
        url, filename, model, idempotency_key
    );

    // 使用 authenticated_multipart_request 发送（自动处理 401 refresh）
    let resp = authenticated_multipart_request(db, client, &url, build_form).await?;

    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    tracing::info!(
        "翻译响应: status={}, content_type={}",
        status, content_type
    );

    match status {
        200 => {
            if content_type.contains("text/event-stream") {
                // SSE 流式响应，从 X-Job-Id header 提取 job_id（用于取消任务）
                let job_id = resp
                    .headers()
                    .get("X-Job-Id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                if job_id.is_some() {
                    tracing::info!("SSE 流式响应: job_id={}", job_id.as_ref().unwrap());
                } else {
                    tracing::warn!("SSE 流式响应: 缺少 X-Job-Id header，无法取消任务");
                }
                Ok(TranslateSubmitResponse::SseStream { response: resp, job_id })
            } else if content_type.contains("application/json") {
                // 非流式直接结果（断点续传时任务已完成）
                let result: TranslateResult = resp
                    .json()
                    .await
                    .map_err(|e| TranslateError::ParseError(format!("解析 JSON 结果失败: {}", e)))?;
                Ok(TranslateSubmitResponse::CompletedJson(result))
            } else {
                Err(TranslateError::ParseError(format!(
                    "未知 content_type: {}",
                    content_type
                )))
            }
        }
        202 => {
            // 异步处理，解析 job_id
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| TranslateError::ParseError(format!("解析 202 响应失败: {}", e)))?;
            let job_id = body
                .get("job_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| TranslateError::ParseError("202 响应缺少 job_id".to_string()))?
                .to_string();
            tracing::info!("翻译任务已提交（异步）: job_id={}", job_id);
            Ok(TranslateSubmitResponse::Accepted { job_id })
        }
        402 | 403 | 413 | 429 | 500 | 503 => {
            // 错误响应
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| TranslateError::ParseError(format!("解析错误响应失败: {}", e)))?;
            let error_code = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let message = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            tracing::warn!(
                "翻译请求错误: status={}, error={}, message={}",
                status, error_code, message
            );
            Ok(TranslateSubmitResponse::Error {
                status,
                error_code,
                message,
            })
        }
        _ => {
            // 其他未知状态码
            let body = resp.text().await.unwrap_or_default();
            Err(TranslateError::ParseError(format!(
                "未知状态码 {}: {}",
                status, body
            )))
        }
    }
}

/// POST /v2/translate/:job_id/start 的响应
#[derive(Debug, serde::Deserialize)]
pub struct StartJobResult {
    pub job_id: String,
    pub status: String,
    pub frozen_points: i64,
}

/// 启动翻译任务（POST /v2/translate/:job_id/start）
/// 服务端冻结点数并将任务状态改为 running
pub async fn start_translate_job(
    db: &Database,
    client: &reqwest::Client,
    job_id: &str,
) -> Result<StartJobResult, TranslateError> {
    let base_url = get_api_base_url(db);
    let url = format!("{}/v2/translate/{}/start", base_url, job_id);

    tracing::info!("启动翻译任务: POST {}", url);

    let resp = authenticated_request(db, client, reqwest::Method::POST, &url, None)
        .await
        .map_err(TranslateError::from)?;

    let status = resp.status().as_u16();
    tracing::info!("启动翻译响应: status={}", status);

    if status == 200 {
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TranslateError::ParseError(format!("解析启动响应失败: {}", e)))?;
        // 兼容 {code:0, data:{...}} 和扁平 {...} 两种格式
        let data = body.get("data").unwrap_or(&body);
        let result: StartJobResult = serde_json::from_value(data.clone())
            .map_err(|e| TranslateError::ParseError(format!("解析启动响应字段失败: {}", e)))?;
        Ok(result)
    } else {
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TranslateError::ParseError(format!("解析错误响应失败: {}", e)))?;
        let error_code = body
            .get("error")
            .and_then(|v| v.as_str())
            .or_else(|| body.get("code").and_then(|v| v.as_str()))
            .unwrap_or("unknown")
            .to_string();
        let message = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Err(TranslateError::ServerError {
            status,
            error_code,
            message,
        })
    }
}

/// 连接 SSE 流（GET /v2/translate/stream/:job_id?token=xxx）
/// 返回 reqwest::Response 供 handle_sse_stream 处理
pub async fn connect_sse_stream(
    db: &Database,
    client: &reqwest::Client,
    job_id: &str,
) -> Result<reqwest::Response, TranslateError> {
    let base_url = get_api_base_url(db);
    // SSE 流用 query param 传 token（不支持 header）
    let access_token = get_access_token().await
        .ok_or_else(|| TranslateError::AuthError(AuthError::RefreshTokenExpired))?;
    let url = format!(
        "{}/v2/translate/stream/{}?token={}",
        base_url, job_id, access_token
    );

    tracing::info!("连接 SSE 流: GET {}", url.replace(&access_token, "***"));

    let resp = client.get(&url).send().await
        .map_err(|e| TranslateError::NetworkError(format!("连接 SSE 失败: {}", e)))?;

    let status = resp.status().as_u16();
    if status != 200 {
        let body = resp.text().await.unwrap_or_default();
        return Err(TranslateError::ParseError(format!(
            "SSE 连接失败: status={}, body={}",
            status, body
        )));
    }

    tracing::info!("SSE 流已连接");
    Ok(resp)
}

/// 取消翻译任务（DELETE /v2/translate/{job_id}）
/// 服务端停止翻译并按已完成部分结算，退还剩余 token
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CancelTranslateResult {
    pub job_id: String,
    pub status: String,
    pub completed_lines: u32,
    pub total_lines: u32,
    pub tokens_used: i64,
    pub cost: i64,
    pub refunded: i64,
    pub token_balance: Option<i64>,
}

pub async fn cancel_translate(
    db: &Database,
    client: &reqwest::Client,
    job_id: &str,
) -> Result<CancelTranslateResult, TranslateError> {
    let base_url = get_api_base_url(db);
    let url = format!("{}/v2/translate/{}", base_url, job_id);

    tracing::info!("取消翻译任务: DELETE {}, job_id={}", url, job_id);

    let resp = authenticated_request(
        db,
        client,
        reqwest::Method::DELETE,
        &url,
        None,
    )
    .await?;

    let status = resp.status().as_u16();
    tracing::info!("取消翻译响应: status={}", status);

    if status == 200 || status == 202 {
        let result: CancelTranslateResult = resp
            .json()
            .await
            .map_err(|e| TranslateError::ParseError(format!("解析取消响应失败: {}", e)))?;
        tracing::info!(
            "取消翻译成功: job_id={}, completed={}/{}, refunded={}",
            result.job_id, result.completed_lines, result.total_lines, result.refunded
        );
        Ok(result)
    } else if status == 404 {
        // 任务不存在（可能已完成或已过期），视为取消成功
        tracing::info!("取消翻译: 任务不存在 (404)，视为已取消");
        Ok(CancelTranslateResult {
            job_id: job_id.to_string(),
            status: "cancelled".to_string(),
            completed_lines: 0,
            total_lines: 0,
            tokens_used: 0,
            cost: 0,
            refunded: 0,
            token_balance: None,
        })
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(TranslateError::ServerError {
            status,
            error_code: "cancel_failed".to_string(),
            message: format!("取消翻译失败 ({}): {}", status, body),
        })
    }
}

/// 单条翻译响应（POST /v2/translate/entry 返回的 JSON）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TranslateEntryResult {
    pub translated_text: String,
    #[serde(alias = "points_used")]
    pub tokens_used: i64,
    pub cost: i64,
    #[serde(default)]
    pub token_balance: Option<i64>,
    #[serde(default)]
    pub bonus_balance: Option<i64>,
}

/// 提交单条翻译请求（POST /v2/translate/entry，multipart 文件上传）
/// 与 submit_translate 类似，但只翻译指定 entry_id 的条目，返回 JSON（非 SSE）
///
/// # 参数
/// - `subtitle_content`: 完整字幕文件内容（服务端用作上下文）
/// - `file_format`: 字幕格式（"srt" / "ass"），决定文件名扩展名
/// - `entry_id`: 要翻译的条目在文件中的序号（0-based）
/// - `original_text`: 实际要翻译的原文（可能被用户编辑过，与文件中该位置内容不同）
/// - `job_id`: 可选，上次全文翻译的 job_id（服务端复用 TM/KNP 上下文）
/// - `model`: 翻译档位
/// - `idempotency_key`: 幂等键
pub async fn submit_translate_one(
    db: &Database,
    client: &reqwest::Client,
    subtitle_content: &str,
    file_format: &str,
    entry_id: usize,
    original_text: &str,
    job_id: Option<&str>,
    model: &str,
    idempotency_key: &str,
) -> Result<TranslateEntryResult, TranslateError> {
    let base_url = get_api_base_url(db);
    let url = format!("{}/v2/translate/entry", base_url);

    let filename = match file_format.to_lowercase().as_str() {
        "ass" | "ssa" => "subtitle.ass",
        _ => "subtitle.srt",
    };

    // 构建 multipart form 的闭包（401 重试时需重新构建）
    let subtitle_bytes = subtitle_content.as_bytes().to_vec();
    let entry_id_str = entry_id.to_string();
    let original_text_owned = original_text.to_string();
    let job_id_owned = job_id.map(|s| s.to_string());
    let idempotency_key_owned = idempotency_key.to_string();
    let model_owned = model.to_string();
    let build_form = move || {
        let part = reqwest::multipart::Part::bytes(subtitle_bytes.clone())
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .unwrap();
        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("entry_id", entry_id_str.clone())
            .text("original_text", original_text_owned.clone())
            .text("idempotency_key", idempotency_key_owned.clone())
            .text("source_lang", "en".to_string())
            .text("target_lang", "zh".to_string())
            .text("model", model_owned.clone());
        if let Some(ref jid) = job_id_owned {
            form = form.text("job_id", jid.clone());
        }
        form
    };

    tracing::info!(
        "提交单条翻译: POST {} (multipart), entry_id={}, job_id={:?}, model={}",
        url, entry_id, job_id, model
    );

    let resp = authenticated_multipart_request(db, client, &url, build_form).await?;

    let status = resp.status().as_u16();
    tracing::info!("单条翻译响应: status={}", status);

    match status {
        200 => {
            let result: TranslateEntryResult = resp
                .json()
                .await
                .map_err(|e| TranslateError::ParseError(format!("解析单条翻译结果失败: {}", e)))?;
            tracing::info!(
                "单条翻译完成: tokens_used={}, token_balance={:?}",
                result.tokens_used, result.token_balance
            );
            Ok(result)
        }
        402 | 403 | 413 | 429 | 500 | 503 => {
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| TranslateError::ParseError(format!("解析错误响应失败: {}", e)))?;
            let error_code = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let message = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("翻译失败")
                .to_string();
            Err(TranslateError::ServerError {
                status,
                error_code,
                message,
            })
        }
        _ => {
            let body = resp.text().await.unwrap_or_default();
            Err(TranslateError::ServerError {
                status,
                error_code: "unknown".to_string(),
                message: format!("单条翻译失败 ({}): {}", status, body),
            })
        }
    }
}

// === SECTION 9 END ===

// === SECTION 10: SSE 流式解析（FP-P1-3）===

pub use crate::subtitle::{ServerSubtitleEntry, TranslateResult};

/// SSE 事件类型
#[derive(Debug, Clone)]
pub enum SseEvent {
    /// 进度更新 {"phase":"...","step":"...","current":N,"total":M}
    Progress {
        phase: String,
        step: String,
        current: u32,
        total: u32,
    },
    /// 增量回传 {"type":"partial","subtitles":[...已翻译的字幕...],"current":N,"total":M}
    Partial {
        subtitles: Vec<crate::subtitle::ServerSubtitleEntry>,
        current: u32,
        total: u32,
    },
    /// 翻译结果 {"type":"result","subtitles":[...],"tokens_used":N,...}
    Result(TranslateResult),
    /// 错误 {"phase":"...","step":"...","error":"...","message":"..."}
    Error {
        phase: String,
        step: String,
        error_code: String,
        message: String,
    },
    /// 心跳 {"type":"heartbeat"}
    Heartbeat,
    /// 流结束
    Done,
}

/// 解析单条 SSE 事件（从 `data: <payload>` 行）
/// 联调文档 02-P1 第 216-240 行
pub fn parse_sse_event(raw: &str) -> Option<SseEvent> {
    // SSE 事件可能有多行（event: xxx\ndata: {...}），提取 data: 行
    // 也可能只有单行 data: {...}
    let data = raw.lines().find_map(|line| {
        line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:"))
    })?;
    let data = data.trim();

    if data == "[DONE]" {
        return Some(SseEvent::Done);
    }

    let payload: serde_json::Value = serde_json::from_str(data).ok()?;

    // heartbeat
    if payload.get("type").and_then(|v| v.as_str()) == Some("heartbeat") {
        return Some(SseEvent::Heartbeat);
    }

    // result
    if payload.get("type").and_then(|v| v.as_str()) == Some("result") {
        let result: TranslateResult = serde_json::from_value(payload).ok()?;
        return Some(SseEvent::Result(result));
    }

    // partial（增量回传已翻译字幕）
    if payload.get("type").and_then(|v| v.as_str()) == Some("partial") {
        let subtitles: Vec<crate::subtitle::ServerSubtitleEntry> =
            payload.get("subtitles").and_then(|v| v.as_array())
                .and_then(|arr| serde_json::from_value(serde_json::Value::Array(arr.clone())).ok())?;
        let current = payload.get("current").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let total = payload.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        return Some(SseEvent::Partial { subtitles, current, total });
    }

    // error
    if payload.get("error").is_some() {
        let phase = payload.get("phase").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let step = payload.get("step").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let error_code = payload.get("error").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let message = payload.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
        return Some(SseEvent::Error { phase, step, error_code, message });
    }

    // progress
    if payload.get("phase").is_some() {
        let phase = payload.get("phase").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let step = payload.get("step").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let current = payload.get("current").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let total = payload.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        return Some(SseEvent::Progress { phase, step, current, total });
    }

    None
}

/// SSE 事件处理器 trait（由 IPC 层实现，用于回调前端）
pub trait SseEventHandler: Send + Sync {
    /// 进度更新
    fn on_progress(&self, phase: &str, step: &str, current: u32, total: u32);
    /// 增量回传已翻译字幕（累计已翻译的全部，不是追加）
    fn on_partial(&self, subtitles: &[crate::subtitle::ServerSubtitleEntry], current: u32, total: u32);
    /// 翻译结果（全量译文）
    fn on_result(&self, result: TranslateResult);
    /// 错误
    fn on_error(&self, phase: &str, step: &str, error_code: &str, message: &str);
    /// 流结束
    fn on_done(&self);
}

/// 从 reqwest::Response 读取 SSE 流并解析事件
/// 联调文档 02-P1 第 343-350 行：
/// - 心跳 30s 一次，收到后重置看门狗
/// - 60s 无任何事件（含心跳）则判定连接异常
/// 返回 result 事件中的 TranslateResult（如果有）
pub async fn handle_sse_stream(
    response: reqwest::Response,
    handler: &dyn SseEventHandler,
) -> Result<Option<TranslateResult>, TranslateError> {
    handle_sse_stream_with_timeouts(
        response,
        handler,
        std::time::Duration::from_secs(60),
        std::time::Duration::from_secs(30),
    )
    .await
}

/// 可配置超时的 SSE 流处理（用于测试）
/// watchdog_timeout: 无事件超时阈值（生产 60s）
/// chunk_timeout: 单次读取超时（生产 30s，必须 < watchdog_timeout）
pub async fn handle_sse_stream_with_timeouts(
    response: reqwest::Response,
    handler: &dyn SseEventHandler,
    watchdog_timeout: std::time::Duration,
    chunk_timeout: std::time::Duration,
) -> Result<Option<TranslateResult>, TranslateError> {
    use futures_util::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut last_event_time = std::time::Instant::now();
    let mut final_result: Option<TranslateResult> = None;

    loop {
        // 看门狗超时检查
        if last_event_time.elapsed() > watchdog_timeout {
            tracing::warn!("SSE 看门狗超时（{:?} 无事件），断开连接", watchdog_timeout);
            return Err(TranslateError::NetworkError("SSE 看门狗超时".to_string()));
        }

        // 读取下一个 chunk（timeout < 看门狗，确保看门狗先触发）
        match tokio::time::timeout(
            chunk_timeout,
            stream.next(),
        )
        .await
        {
            Ok(Some(chunk_result)) => {
                let chunk = chunk_result.map_err(|e| {
                    TranslateError::NetworkError(format!("SSE 流读取失败: {}", e))
                })?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);

                // 按 \n\n 分割事件
                while let Some(pos) = buffer.find("\n\n") {
                    let raw_event = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    let trimmed = raw_event.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    tracing::debug!("SSE 原始事件: {:?}", trimmed);

                    match parse_sse_event(trimmed) {
                        Some(event) => {
                        last_event_time = std::time::Instant::now();
                        match event {
                            SseEvent::Progress { phase, step, current, total } => {
                                handler.on_progress(&phase, &step, current, total);
                            }
                            SseEvent::Partial { subtitles, current, total } => {
                                handler.on_partial(&subtitles, current, total);
                            }
                            SseEvent::Result(result) => {
                                handler.on_result(result.clone());
                                final_result = Some(result);
                            }
                            SseEvent::Error { phase, step, error_code, message } => {
                                handler.on_error(&phase, &step, &error_code, &message);
                                // error 事件后流会关闭，但还没收到 Done
                            }
                            SseEvent::Heartbeat => {
                                tracing::debug!("SSE 心跳");
                            }
                            SseEvent::Done => {
                                handler.on_done();
                                tracing::info!("SSE 流结束");
                                return Ok(final_result);
                            }
                        }
                        }
                        None => {
                            tracing::warn!("SSE 事件解析失败: {:?}", trimmed);
                        }
                    }
                }
            }
            Ok(None) => {
                // 流结束
                tracing::info!("SSE 流关闭（EOF）");
                return Ok(final_result);
            }
            Err(_) => {
                // per-chunk timeout —— 检查看门狗是否超时
                if last_event_time.elapsed() > watchdog_timeout {
                    tracing::warn!("SSE 看门狗超时（{:?} 无事件），断开连接", watchdog_timeout);
                    return Err(TranslateError::NetworkError("SSE 看门狗超时".to_string()));
                }
                // 看门狗未超时，继续读取（可能是正常的长间隔）
                tracing::debug!("SSE {:?} 无数据，继续等待", chunk_timeout);
            }
        }
    }
}

// === SECTION 10 END ===

// === SECTION 8: 单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use std::sync::Mutex as StdMutex;

    /// access_token 测试串行化锁（全局静态 ACCESS_TOKEN 不支持并发测试）
    /// 用 into_inner 恢复毒化的 Mutex，避免一个测试 panic 后连锁失败
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 创建临时内存数据库用于测试
    fn test_db() -> Database {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS credentials (
                entry_name TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        Database::from_connection(conn)
    }

    #[tokio::test]
    async fn test_access_token_set_get_clear() {
        let _guard = test_lock();
        // 清理可能残留的 token（测试间隔离）
        clear_access_token().await;
        assert!(get_access_token().await.is_none(), "初始状态应为 None");

        // set 后应能读到
        set_access_token("test-token-1".to_string(), 3600).await;
        let token = get_access_token().await;
        assert_eq!(token.as_deref(), Some("test-token-1"));
        assert!(get_access_token_expiry().await.is_some());

        // clear 后应为 None
        clear_access_token().await;
        assert!(get_access_token().await.is_none());
        assert!(get_access_token_expiry().await.is_none());
    }

    #[tokio::test]
    async fn test_access_token_expiry() {
        let _guard = test_lock();
        clear_access_token().await;
        // 设置 1 秒过期的 token
        set_access_token("short-lived".to_string(), 1).await;
        assert_eq!(get_access_token().await.as_deref(), Some("short-lived"));

        // 等待过期
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(
            get_access_token().await.is_none(),
            "过期后应返回 None"
        );
    }

    #[tokio::test]
    async fn test_access_token_zero_ttl_uses_default() {
        let _guard = test_lock();
        clear_access_token().await;
        // expires_in=0 应使用默认 TTL（3600），不应立即过期
        set_access_token("default-ttl".to_string(), 0).await;
        assert_eq!(get_access_token().await.as_deref(), Some("default-ttl"));
        clear_access_token().await;
    }

    #[test]
    fn test_refresh_token_storage_roundtrip() {
        let db = test_db();

        // 初始无 refresh_token
        assert!(load_refresh_token(&db).unwrap().is_none());

        // 保存后应能读到
        save_refresh_token(&db, "rt-abc-123").unwrap();
        assert_eq!(
            load_refresh_token(&db).unwrap().as_deref(),
            Some("rt-abc-123")
        );

        // 覆盖保存（滑动过期场景）
        save_refresh_token(&db, "rt-new-456").unwrap();
        assert_eq!(
            load_refresh_token(&db).unwrap().as_deref(),
            Some("rt-new-456")
        );

        // 清除后应为 None
        clear_refresh_token(&db).unwrap();
        assert!(load_refresh_token(&db).unwrap().is_none());
    }

    #[test]
    fn test_user_info_serde() {
        let user = UserInfo {
            id: 1,
            email: "test@example.com".to_string(),
            token_balance: 1000000,
            bonus_balance: 3300,
            traffic_pack_balance: 0,
            status: "active".to_string(),
            vip_level: "vip2".to_string(),
            vip_expires_at: None,
            bonus_expires_at: Some("2026-07-25T00:00:00Z".to_string()),
            traffic_pack_expires_at: None,
            max_concurrent_jobs: 3,
            available_models: vec![
                ModelOption {
                    model: "polly-standard".to_string(),
                    name: "标准翻译".to_string(),
                    description: "标准翻译，性价比之选".to_string(),
                    enabled: true,
                    price_url: None,
                },
                ModelOption {
                    model: "polly-fine".to_string(),
                    name: "高质量精译".to_string(),
                    description: "高质量精译，适合专业场景".to_string(),
                    enabled: false,
                    price_url: None,
                },
            ],
        };
        let json = serde_json::to_string(&user).unwrap();
        let parsed: UserInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.vip_level, "vip2");
        assert_eq!(parsed.available_models.len(), 2);
        assert!(parsed.available_models[0].enabled);
        assert!(!parsed.available_models[1].enabled);
    }

    #[test]
    fn test_auth_tokens_serde() {
        let tokens = AuthTokens {
            access_token: "at-xxx".to_string(),
            refresh_token: "rt-yyy".to_string(),
            expires_at: Some(1700000000),
        };
        let json = serde_json::to_string(&tokens).unwrap();
        let parsed: AuthTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.access_token, "at-xxx");
        assert_eq!(parsed.refresh_token, "rt-yyy");
        assert_eq!(parsed.expires_at, Some(1700000000));
    }

    // === FP-P0-2 测试：Token 刷新 ===

    /// 启动一个简单的 mock HTTP 服务器，返回固定的 JSON 响应
    /// 返回 (addr, handle)，handle 是 JoinHandle，drop 时服务器停止
    async fn start_mock_server(
        status: u16,
        body: String,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        let body = body.clone();
                        tokio::spawn(async move {
                            use tokio::io::AsyncReadExt;
                            use tokio::io::AsyncWriteExt;
                            // 读取请求（丢弃）
                            let mut buf = [0u8; 1024];
                            let _ = stream.read(&mut buf).await;
                            // 写响应
                            let resp = format!(
                                "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                                status,
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn test_refresh_no_refresh_token_returns_none() {
        let db = test_db();
        let client = reqwest::Client::new();
        let result = refresh_access_token(&db, &client).await;
        assert!(result.is_ok(), "无 refresh_token 应返回 Ok(None)");
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_refresh_success_updates_tokens() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        // 存入旧 refresh_token
        save_refresh_token(&db, "rt-old").unwrap();

        // 启动 mock 服务器返回成功响应
        let mock_body = r#"{"access_token":"at-new","refresh_token":"rt-new","expires_in":3600,"refresh_expires_in":2592000}"#.to_string();
        let (addr, _handle) = start_mock_server(200, mock_body).await;

        // 设置 API base URL 指向 mock 服务器
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = refresh_access_token(&db, &client).await;

        assert!(result.is_ok(), "refresh 应成功");
        let refresh_result = result.unwrap().unwrap();
        assert_eq!(refresh_result.access_token, "at-new");
        assert_eq!(refresh_result.expires_in, 3600);

        // 验证 access_token 已存内存
        assert_eq!(get_access_token().await.as_deref(), Some("at-new"));

        // 验证 refresh_token 已更新（滑动过期）
        assert_eq!(load_refresh_token(&db).unwrap().as_deref(), Some("rt-new"));

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_refresh_401_clears_credentials() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        // 存入 refresh_token（不预设 access_token，确保 refresh 请求会实际执行）
        save_refresh_token(&db, "rt-will-expire").unwrap();

        // 启动 mock 服务器返回 401
        let mock_body = r#"{"error":"invalid_refresh_token","message":"refresh_token 无效"}"#.to_string();
        let (addr, _handle) = start_mock_server(401, mock_body).await;

        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = refresh_access_token(&db, &client).await;

        // 应返回 RefreshTokenExpired
        assert!(matches!(result, Err(AuthError::RefreshTokenExpired)));

        // access_token 和 refresh_token 都应被清除
        assert!(get_access_token().await.is_none());
        assert!(load_refresh_token(&db).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_refresh_network_error_preserves_refresh_token() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-preserve").unwrap();

        // 指向一个不可达的地址（端口 1 通常是保留端口）
        db.set_config("zimufan_api_base_url", "http://127.0.0.1:1").unwrap();

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .unwrap();
        let result = refresh_access_token(&db, &client).await;

        // 应返回网络错误
        assert!(matches!(result, Err(AuthError::NetworkError(_))));

        // 网络错误时 refresh_token 应保留（不因网络错误清除）
        assert_eq!(
            load_refresh_token(&db).unwrap().as_deref(),
            Some("rt-preserve")
        );
    }

    #[test]
    fn test_api_base_url_default_and_override() {
        let db = test_db();
        // 无配置时用默认值
        assert_eq!(get_api_base_url(&db), "https://api.zimufan.com");
        // 有配置时用配置值
        set_api_base_url(&db, "http://localhost:8080").unwrap();
        assert_eq!(get_api_base_url(&db), "http://localhost:8080");
    }

    // === FP-P0-3 测试：WebSocket 登录 ===

    #[test]
    fn test_build_login_page_url() {
        let url = build_login_page_url("https://api.zimufan.com", "sess-123");
        assert_eq!(url, "https://api.zimufan.com/auth/client-login?session_id=sess-123");

        let url = build_login_page_url("http://localhost:8080/", "abc");
        assert_eq!(url, "http://localhost:8080/auth/client-login?session_id=abc");
    }

    #[test]
    fn test_build_websocket_url() {
        // https → wss
        let url = build_websocket_url("https://api.zimufan.com", "sess-123");
        assert_eq!(url, "wss://api.zimufan.com/auth/wait?session_id=sess-123");

        // http → ws
        let url = build_websocket_url("http://localhost:8080", "sess-123");
        assert_eq!(url, "ws://localhost:8080/auth/wait?session_id=sess-123");

        // 尾部斜杠
        let url = build_websocket_url("https://api.zimufan.com/", "x");
        assert_eq!(url, "wss://api.zimufan.com/auth/wait?session_id=x");
    }

    #[test]
    fn test_ws_auth_message_serde() {
        // auth_success
        let success_json = r#"{"type":"auth_success","access_token":"at","refresh_token":"rt","expires_in":3600,"user":{"id":1,"email":"a@b.com","token_balance":100,"bonus_balance":0,"traffic_pack_balance":0,"status":"active","vip_level":"vip2","vip_expires_at":null,"bonus_expires_at":null,"traffic_pack_expires_at":null,"max_concurrent_jobs":3,"available_models":[]}}"#;
        let msg: WsAuthMessage = serde_json::from_str(success_json).unwrap();
        match msg {
            WsAuthMessage::AuthSuccess { access_token, expires_in, user, .. } => {
                assert_eq!(access_token, "at");
                assert_eq!(expires_in, 3600);
                assert_eq!(user.id, 1);
                assert_eq!(user.vip_level, "vip2");
            }
            _ => panic!("应为 AuthSuccess"),
        }

        // auth_error
        let error_json = r#"{"type":"auth_error","error":"invalid_credentials","message":"密码错误"}"#;
        let msg: WsAuthMessage = serde_json::from_str(error_json).unwrap();
        match msg {
            WsAuthMessage::AuthError { error, message } => {
                assert_eq!(error, "invalid_credentials");
                assert_eq!(message, "密码错误");
            }
            _ => panic!("应为 AuthError"),
        }

        // auth_timeout
        let timeout_json = r#"{"type":"auth_timeout","message":"登录超时"}"#;
        let msg: WsAuthMessage = serde_json::from_str(timeout_json).unwrap();
        match msg {
            WsAuthMessage::AuthTimeout { message } => {
                assert_eq!(message, "登录超时");
            }
            _ => panic!("应为 AuthTimeout"),
        }
    }

    /// 启动 mock WebSocket 服务器
    /// 连接后发送指定的消息列表，然后关闭
    async fn start_mock_ws_server(
        messages: Vec<String>,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        use tokio_tungstenite::tungstenite::Message as WsMessage;
        use futures_util::SinkExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let msgs = messages.clone();
                        tokio::spawn(async move {
                            let mut ws_stream = tokio_tungstenite::accept_async(stream)
                                .await
                                .unwrap();
                            for msg in msgs {
                                let _ = ws_stream.send(WsMessage::Text(msg.into())).await;
                            }
                            let _ = ws_stream.close(None).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn test_login_auth_success() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        let session_created = r#"{"type":"session_created","session_id":"test-session-001"}"#.to_string();
        let success_msg = r#"{"type":"auth_success","access_token":"at-login","refresh_token":"rt-login","expires_in":3600,"user":{"id":42,"email":"test@x.com","token_balance":50000,"bonus_balance":0,"traffic_pack_balance":0,"status":"active","vip_level":"vip1","vip_expires_at":null,"bonus_expires_at":null,"traffic_pack_expires_at":null,"max_concurrent_jobs":2,"available_models":[{"model":"polly-standard","name":"标准","description":"快速","enabled":true}]}}"#.to_string();
        let (addr, _handle) = start_mock_ws_server(vec![session_created, success_msg]).await;

        // 用 ws:// 协议指向 mock 服务器
        let base_url = format!("http://{}", addr);
        let result = login_via_websocket_with_browser(&db, &base_url, |_| {}).await;

        assert!(result.is_ok(), "登录应成功: {:?}", result.err());
        let login = result.unwrap();
        assert_eq!(login.user.id, 42);
        assert_eq!(login.user.email, "test@x.com");
        assert_eq!(login.user.vip_level, "vip1");
        assert_eq!(login.expires_in, 3600);

        // 验证 token 已存储
        assert_eq!(get_access_token().await.as_deref(), Some("at-login"));
        assert_eq!(load_refresh_token(&db).unwrap().as_deref(), Some("rt-login"));

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_login_auth_error() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        let error_msg = r#"{"type":"auth_error","error":"invalid_credentials","message":"邮箱或密码错误"}"#.to_string();
        let (addr, _handle) = start_mock_ws_server(vec![error_msg]).await;

        let base_url = format!("http://{}", addr);
        let result = login_via_websocket_with_browser(&db, &base_url, |_| {}).await;

        assert!(matches!(
            result,
            Err(AuthError::LoginFailed { error, message })
            if error == "invalid_credentials" && message == "邮箱或密码错误"
        ));

        // 登录失败不应存储任何 token
        assert!(get_access_token().await.is_none());
        assert!(load_refresh_token(&db).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_login_auth_timeout() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        let session_created = r#"{"type":"session_created","session_id":"test-session-timeout"}"#.to_string();
        let timeout_msg = r#"{"type":"auth_timeout","message":"登录超时，请重试"}"#.to_string();
        let (addr, _handle) = start_mock_ws_server(vec![session_created, timeout_msg]).await;

        let base_url = format!("http://{}", addr);
        let result = login_via_websocket_with_browser(&db, &base_url, |_| {}).await;

        assert!(matches!(result, Err(AuthError::LoginTimeout)));

        // 超时不应存储任何 token
        assert!(get_access_token().await.is_none());
        assert!(load_refresh_token(&db).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_login_websocket_connection_failed() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        // 指向不可达地址
        let base_url = "http://127.0.0.1:1";
        let result = login_via_websocket_with_browser(&db, base_url, |_| {}).await;

        assert!(matches!(
            result,
            Err(AuthError::WebSocketConnectFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_login_websocket_message_parse_failed() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        // 发送畸形 JSON（不是合法的 WsAuthMessage）
        let bad_msg = r#"{"type":"unknown_type","foo":"bar"}"#.to_string();
        let (addr, _handle) = start_mock_ws_server(vec![bad_msg]).await;

        let base_url = format!("http://{}", addr);
        let result = login_via_websocket_with_browser(&db, &base_url, |_| {}).await;

        assert!(
            matches!(result, Err(AuthError::WebSocketMessageParseFailed(_))),
            "畸形 JSON 应返回 WebSocketMessageParseFailed，实际: {:?}",
            result
        );

        // 解析失败不应存储任何 token
        assert!(get_access_token().await.is_none());
        assert!(load_refresh_token(&db).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_login_websocket_closed_without_message() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        // mock 服务器立即关闭连接，不发送任何消息
        let (addr, _handle) = start_mock_ws_server(vec![]).await;

        let base_url = format!("http://{}", addr);
        let result = login_via_websocket_with_browser(&db, &base_url, |_| {}).await;

        // WS 流结束未收到认证消息，应返回 WebSocketConnectFailed
        assert!(
            matches!(result, Err(AuthError::WebSocketConnectFailed(_))),
            "WS 流提前结束应返回 WebSocketConnectFailed，实际: {:?}",
            result
        );

        assert!(get_access_token().await.is_none());
        assert!(load_refresh_token(&db).unwrap().is_none());
    }

    // === FP-P0-4 测试：401 处理 + 后台刷新 ===

    #[tokio::test]
    async fn test_authenticated_request_no_token_returns_expired() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        let client = reqwest::Client::new();

        let result = authenticated_request(
            &db,
            &client,
            reqwest::Method::GET,
            "http://127.0.0.1:1/test",
            None,
        )
        .await;

        // 无 access_token 应返回 RefreshTokenExpired
        assert!(
            matches!(result, Err(AuthError::RefreshTokenExpired)),
            "无 token 应返回 RefreshTokenExpired，实际: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_authenticated_request_success_no_retry() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        // 预设有效 access_token
        set_access_token("at-valid".to_string(), 3600).await;

        // mock 服务器返回 200
        let mock_body = r#"{"ok":true}"#.to_string();
        let (addr, _handle, _) =
            start_mock_server_with_capture(200, "OK", mock_body).await;
        let url = format!("http://{}/test", addr);

        let client = reqwest::Client::new();
        let result = authenticated_request(
            &db,
            &client,
            reqwest::Method::GET,
            &url,
            None,
        )
        .await;

        assert!(result.is_ok(), "200 请求应成功: {:?}", result.err());
        let resp = result.unwrap();
        assert_eq!(resp.status().as_u16(), 200);

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_authenticated_request_401_refresh_and_retry() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        // 存 refresh_token（供 refresh 使用）
        save_refresh_token(&db, "rt-for-401").unwrap();

        // 第一个 mock 服务器：refresh 接口，返回新 token
        let refresh_body = r#"{"access_token":"at-new","refresh_token":"rt-new","expires_in":3600}"#.to_string();
        let (refresh_addr, _refresh_handle, _) =
            start_mock_server_with_capture(200, "OK", refresh_body).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", refresh_addr)).unwrap();

        // 业务接口返回 401（重试后仍 401，验证最多重试 1 次）
        let (auth_fail_addr, _fail_handle, _) =
            start_mock_server_with_capture(401, "Unauthorized", r#"{"error":"unauthorized"}"#.to_string()).await;
        let fail_url = format!("http://{}/api/test", auth_fail_addr);

        // 预设旧 access_token（会被 401 拒绝）
        set_access_token("at-old".to_string(), 3600).await;

        let client = reqwest::Client::new();
        // 第一次请求命中 fail_url（401）→ refresh（命中 refresh_addr）→ 重试命中 fail_url（还是 401）
        // 这里验证 401 重试逻辑：重试后仍 401 则返回响应
        let result = authenticated_request(
            &db,
            &client,
            reqwest::Method::GET,
            &fail_url,
            None,
        )
        .await;

        // 重试后仍 401，应返回 401 响应（不无限重试）
        assert!(result.is_ok(), "重试后应返回响应: {:?}", result.err());
        let resp = result.unwrap();
        assert_eq!(resp.status().as_u16(), 401);

        // 验证 refresh 已执行（access_token 已更新）
        assert_eq!(get_access_token().await.as_deref(), Some("at-new"));
        assert_eq!(load_refresh_token(&db).unwrap().as_deref(), Some("rt-new"));

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_authenticated_request_401_refresh_success_retry_success() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-401-retry").unwrap();

        // refresh 服务器
        let refresh_body = r#"{"access_token":"at-refreshed","refresh_token":"rt-refreshed","expires_in":3600}"#.to_string();
        let (refresh_addr, _, _) =
            start_mock_server_with_capture(200, "OK", refresh_body).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", refresh_addr)).unwrap();

        // 业务服务器：第一次 401，第二次 200
        // 用一个状态机 mock 服务器
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let biz_addr = listener.local_addr().unwrap();
        let _biz_handle = tokio::spawn(async move {
            let mut request_count = 0u32;
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        request_count += 1;
                        let count = request_count;
                        tokio::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buf = vec![0u8; 4096];
                            let _ = stream.read(&mut buf).await;
                            let (status, body) = if count == 1 {
                                (401, r#"{"error":"unauthorized"}"#.to_string())
                            } else {
                                (200, r#"{"data":"ok"}"#.to_string())
                            };
                            let resp = format!(
                                "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                                status,
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let biz_url = format!("http://{}/api/test", biz_addr);
        set_access_token("at-old".to_string(), 3600).await;

        let client = reqwest::Client::new();
        let result = authenticated_request(
            &db,
            &client,
            reqwest::Method::GET,
            &biz_url,
            None,
        )
        .await;

        // 第一次 401 → refresh → 重试 200
        assert!(result.is_ok(), "重试应成功: {:?}", result.err());
        let resp = result.unwrap();
        assert_eq!(resp.status().as_u16(), 200);

        // 验证 token 已更新
        assert_eq!(get_access_token().await.as_deref(), Some("at-refreshed"));

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_authenticated_request_401_refresh_also_401() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-also-expired").unwrap();

        let (refresh_addr, _, _) = start_mock_server_with_capture(
            401,
            "Unauthorized",
            r#"{"error":"invalid_refresh_token"}"#.to_string(),
        )
        .await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", refresh_addr))
            .unwrap();

        let (biz_addr, _, _) = start_mock_server_with_capture(
            401,
            "Unauthorized",
            r#"{"error":"unauthorized"}"#.to_string(),
        )
        .await;
        let biz_url = format!("http://{}/api/test", biz_addr);

        set_access_token("at-old".to_string(), 3600).await;

        let client = reqwest::Client::new();
        let result = authenticated_request(&db, &client, reqwest::Method::GET, &biz_url, None).await;

        assert!(
            matches!(result, Err(AuthError::RefreshTokenExpired)),
            "refresh 也 401 应返回 RefreshTokenExpired，实际: {:?}",
            result
        );

        assert!(get_access_token().await.is_none());
        assert!(load_refresh_token(&db).unwrap().is_none());
    }

    // === FP-P0-5 测试：启动初始化 + GET /auth/me ===

    #[tokio::test]
    async fn test_init_auth_no_refresh_token() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        let client = reqwest::Client::new();
        let result = init_auth_on_startup(&db, &client).await;

        // 无 refresh_token → 返回未登录
        assert!(result.user.is_none());
        assert!(!result.offline);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_init_auth_refresh_expired() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-init-expired").unwrap();

        // refresh 接口返回 401
        let (refresh_addr, _, _) = start_mock_server_with_capture(
            401,
            "Unauthorized",
            r#"{"error":"invalid"}"#.to_string(),
        )
        .await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", refresh_addr))
            .unwrap();

        let client = reqwest::Client::new();
        let result = init_auth_on_startup(&db, &client).await;

        // refresh 401 → 返回需重新登录
        assert!(result.user.is_none());
        assert!(!result.offline);
        assert!(result.error.unwrap().contains("重新登录"));
    }

    #[tokio::test]
    async fn test_init_auth_success() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-init-success").unwrap();

        // refresh + /auth/me 共用一个 mock 服务器
        // refresh: POST /auth/refresh → 200 + 新 token
        // /auth/me: GET /auth/me → 200 + user info
        // 用状态机 mock 服务器区分 POST 和 GET
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        tokio::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buf = vec![0u8; 4096];
                            let n = stream.read(&mut buf).await.unwrap_or(0);
                            let request = String::from_utf8_lossy(&buf[..n]).to_string();

                            // 区分 POST /auth/refresh 和 GET /auth/me
                            let (status, body) = if request.contains("POST") && request.contains("/auth/refresh") {
                                (200, r#"{"access_token":"at-init","refresh_token":"rt-init-new","expires_in":3600}"#.to_string())
                            } else if request.contains("GET") && request.contains("/auth/me") {
                                (200, r#"{"id":1,"email":"init@test.com","token_balance":1000,"bonus_balance":0,"traffic_pack_balance":0,"status":"active","vip_level":"vip1","vip_expires_at":null,"bonus_expires_at":null,"traffic_pack_expires_at":null,"max_concurrent_jobs":2,"available_models":[]}"#.to_string())
                            } else {
                                (404, r#"{"error":"not found"}"#.to_string())
                            };

                            let resp = format!(
                                "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                                status,
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        db.set_config("zimufan_api_base_url", &format!("http://{}", addr))
            .unwrap();

        let client = reqwest::Client::new();
        let result = init_auth_on_startup(&db, &client).await;

        assert!(result.user.is_some(), "应获取到用户信息: {:?}", result.error);
        let user = result.user.unwrap();
        assert_eq!(user.id, 1);
        assert_eq!(user.email, "init@test.com");
        assert_eq!(user.vip_level, "vip1");
        assert!(!result.offline);
        assert!(result.error.is_none());

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_init_auth_network_error_offline_mode() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-init-offline").unwrap();

        // 指向不可达地址
        db.set_config("zimufan_api_base_url", "http://127.0.0.1:1")
            .unwrap();

        // 用短超时 client 加速测试
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();

        // 用 mock 时间加速——实际上指数退避会等 2+4+8=14 秒
        // 这里我们接受等待，验证离线降级
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            init_auth_on_startup(&db, &client),
        )
        .await;

        assert!(result.is_ok(), "应在超时前完成");
        let init_result = result.unwrap();
        assert!(init_result.user.is_none());
        assert!(init_result.offline, "网络错误应降级离线模式");
        assert!(init_result.error.is_some());
    }

    #[tokio::test]
    async fn test_fetch_user_info_success() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        // 预设有效 access_token
        set_access_token("at-me-test".to_string(), 3600).await;

        // mock /auth/me 返回 200
        let user_json = r#"{"id":99,"email":"me@test.com","token_balance":50000,"bonus_balance":3300,"traffic_pack_balance":0,"status":"active","vip_level":"vip3","vip_expires_at":null,"bonus_expires_at":"2026-07-25T00:00:00Z","traffic_pack_expires_at":null,"max_concurrent_jobs":5,"available_models":[{"model":"polly-fine","name":"高质量精译","description":"高质量精译，适合专业场景","enabled":true,"price_url":"https://zimufan.com/pricing/fine"}]}"#.to_string();
        let (addr, _, _) = start_mock_server_with_capture(200, "OK", user_json).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr))
            .unwrap();

        let client = reqwest::Client::new();
        let result = fetch_user_info(&db, &client).await;

        assert!(result.is_ok(), "获取用户信息应成功: {:?}", result.err());
        let user = result.unwrap();
        assert_eq!(user.id, 99);
        assert_eq!(user.email, "me@test.com");
        assert_eq!(user.vip_level, "vip3");
        assert_eq!(user.max_concurrent_jobs, 5);
        assert_eq!(user.available_models.len(), 1);
        assert!(user.available_models[0].enabled);

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_fetch_user_info_no_token() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        let client = reqwest::Client::new();
        let result = fetch_user_info(&db, &client).await;

        // 无 access_token → authenticated_request 返回 RefreshTokenExpired
        assert!(matches!(result, Err(AuthError::RefreshTokenExpired)));
    }

    #[tokio::test]
    async fn test_refresh_and_fetch_user_info_no_refresh_token() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        let client = reqwest::Client::new();
        let result = refresh_and_fetch_user_info(&db, &client).await;

        // 无 refresh_token → Ok(None)
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_refresh_and_fetch_user_info_refresh_expired() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-rf-expired").unwrap();

        // refresh 接口返回 401
        let (refresh_addr, _, _) = start_mock_server_with_capture(
            401,
            "Unauthorized",
            r#"{"error":"invalid"}"#.to_string(),
        )
        .await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", refresh_addr))
            .unwrap();

        let client = reqwest::Client::new();
        let result = refresh_and_fetch_user_info(&db, &client).await;

        // refresh 401 → Err(RefreshTokenExpired)
        assert!(matches!(result, Err(AuthError::RefreshTokenExpired)));
    }

    #[tokio::test]
    async fn test_refresh_and_fetch_user_info_success() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-rf-success").unwrap();

        // 状态机 mock：POST /auth/refresh → 200 + token；GET /auth/me → 200 + user
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        tokio::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buf = vec![0u8; 4096];
                            let n = stream.read(&mut buf).await.unwrap_or(0);
                            let request = String::from_utf8_lossy(&buf[..n]).to_string();

                            let (status, body) = if request.contains("POST") && request.contains("/auth/refresh") {
                                (200, r#"{"access_token":"at-rf","refresh_token":"rt-rf-new","expires_in":3600}"#.to_string())
                            } else if request.contains("GET") && request.contains("/auth/me") {
                                (200, r#"{"id":42,"email":"rf@test.com","token_balance":100,"bonus_balance":0,"traffic_pack_balance":0,"status":"active","vip_level":"vip2","vip_expires_at":null,"bonus_expires_at":null,"traffic_pack_expires_at":null,"max_concurrent_jobs":3,"available_models":[]}"#.to_string())
                            } else {
                                (404, r#"{"error":"not found"}"#.to_string())
                            };

                            let resp = format!(
                                "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                                status,
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        db.set_config("zimufan_api_base_url", &format!("http://{}", addr))
            .unwrap();

        let client = reqwest::Client::new();
        let result = refresh_and_fetch_user_info(&db, &client).await;

        assert!(result.is_ok(), "应成功: {:?}", result.err());
        let user = result.unwrap().unwrap();
        assert_eq!(user.id, 42);
        assert_eq!(user.email, "rf@test.com");

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_refresh_and_fetch_user_info_refresh_ok_but_me_fails() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();

        save_refresh_token(&db, "rt-rf-degrade").unwrap();

        // 状态机 mock：POST /auth/refresh → 200 + token；GET /auth/me → 500（失败）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        tokio::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buf = vec![0u8; 4096];
                            let n = stream.read(&mut buf).await.unwrap_or(0);
                            let request = String::from_utf8_lossy(&buf[..n]).to_string();

                            let (status, body) = if request.contains("POST") && request.contains("/auth/refresh") {
                                (200, r#"{"access_token":"at-rf-degrade","refresh_token":"rt-rf-degrade-new","expires_in":3600}"#.to_string())
                            } else {
                                // /auth/me 返回 500
                                (500, r#"{"error":"server_error"}"#.to_string())
                            };

                            let resp = format!(
                                "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                                status,
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        db.set_config("zimufan_api_base_url", &format!("http://{}", addr))
            .unwrap();

        let client = reqwest::Client::new();
        let result = refresh_and_fetch_user_info(&db, &client).await;

        // refresh 成功但 /auth/me 500 → 降级返回 Ok(None)
        assert!(result.is_ok(), "应降级返回 Ok(None): {:?}", result.err());
        assert!(result.unwrap().is_none());

        clear_access_token().await;
    }

    // === FP-P0-4 测试：start_token_refresh_loop ===

    #[tokio::test]
    async fn test_refresh_loop_exits_when_no_tokens() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = std::sync::Arc::new(test_db());

        let client = reqwest::Client::new();
        let config = RefreshLoopConfig {
            refresh_ahead: Duration::from_millis(100),
            no_token_check: Duration::from_millis(50),
            retry_on_error: Duration::from_millis(50),
        };

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            start_token_refresh_loop_with_config(db, client, config),
        )
        .await;

        assert!(result.is_ok(), "无 token 时循环应退出，不应超时");
    }

    #[tokio::test]
    async fn test_refresh_loop_stops_on_refresh_token_expired() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = std::sync::Arc::new(test_db());

        save_refresh_token(&db, "rt-loop-expired").unwrap();

        let (refresh_addr, _, _) = start_mock_server_with_capture(
            401,
            "Unauthorized",
            r#"{"error":"invalid"}"#.to_string(),
        )
        .await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", refresh_addr))
            .unwrap();

        // 预设已过期的 access_token（触发立即刷新）
        set_access_token("at-expired".to_string(), 1).await;
        tokio::time::sleep(Duration::from_millis(1100)).await;

        let client = reqwest::Client::new();
        let config = RefreshLoopConfig {
            refresh_ahead: Duration::from_millis(100),
            no_token_check: Duration::from_millis(50),
            retry_on_error: Duration::from_millis(50),
        };

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            start_token_refresh_loop_with_config(db, client, config),
        )
        .await;

        assert!(result.is_ok(), "refresh 401 后循环应停止，不应超时");
    }

    #[tokio::test]
    async fn test_refresh_loop_refreshes_expired_token() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = std::sync::Arc::new(test_db());

        save_refresh_token(&db, "rt-loop-success").unwrap();

        let refresh_body = r#"{"access_token":"at-loop-new","refresh_token":"rt-loop-new","expires_in":3600}"#.to_string();
        let (refresh_addr, _, _) =
            start_mock_server_with_capture(200, "OK", refresh_body).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", refresh_addr))
            .unwrap();

        // 预设已过期的 access_token
        set_access_token("at-loop-old".to_string(), 1).await;
        tokio::time::sleep(Duration::from_millis(1100)).await;

        let client = reqwest::Client::new();
        let config = RefreshLoopConfig {
            refresh_ahead: Duration::from_millis(100),
            no_token_check: Duration::from_millis(50),
            retry_on_error: Duration::from_millis(50),
        };

        let db_clone = db.clone();
        let handle = tokio::spawn(async move {
            start_token_refresh_loop_with_config(db_clone, client, config).await;
        });

        // 等待刷新发生
        for _ in 0..50 {
            if get_access_token().await.as_deref() == Some("at-loop-new") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        assert_eq!(
            get_access_token().await.as_deref(),
            Some("at-loop-new"),
            "循环应刷新 token"
        );
        assert_eq!(
            load_refresh_token(&db).unwrap().as_deref(),
            Some("rt-loop-new"),
            "refresh_token 应已滑动更新"
        );

        // 清理：清除 token 让循环退出
        clear_access_token().await;
        clear_refresh_token(&db).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    /// 改进版 mock 服务器：捕获请求体供断言，状态行正确
    async fn start_mock_server_with_capture(
        status: u16,
        status_text: &str,
        body: String,
    ) -> (
        std::net::SocketAddr,
        tokio::task::JoinHandle<()>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let captured: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        let status_text = status_text.to_string();
        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        let body = body.clone();
                        let status_text = status_text.clone();
                        let captured = captured_clone.clone();
                        tokio::spawn(async move {
                            use tokio::io::AsyncReadExt;
                            use tokio::io::AsyncWriteExt;
                            // 读取请求（捕获请求体）
                            let mut buf = vec![0u8; 4096];
                            let n = stream.read(&mut buf).await.unwrap_or(0);
                            let request_data = String::from_utf8_lossy(&buf[..n]).to_string();
                            // 提取 body（\r\n\r\n 之后的部分）
                            if let Some(idx) = request_data.find("\r\n\r\n") {
                                let req_body = request_data[idx + 4..].to_string();
                                captured.lock().unwrap().push(req_body);
                            }
                            // 写响应（状态行正确）
                            let resp = format!(
                                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                                status,
                                status_text,
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });
        (addr, handle, captured)
    }

    #[tokio::test]
    async fn test_refresh_request_body_format() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        save_refresh_token(&db, "rt-format-check").unwrap();

        let mock_body = r#"{"access_token":"at-x","refresh_token":"rt-x","expires_in":3600,"refresh_expires_in":2592000}"#.to_string();
        let (addr, _handle, captured) =
            start_mock_server_with_capture(200, "OK", mock_body).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let _ = refresh_access_token(&db, &client).await.unwrap();

        // 断言请求体包含 refresh_token 字段和正确的值
        let bodies = captured.lock().unwrap();
        assert_eq!(bodies.len(), 1, "应只发送 1 次请求");
        let body = &bodies[0];
        assert!(
            body.contains("\"refresh_token\""),
            "请求体应包含 refresh_token 字段: {}",
            body
        );
        assert!(
            body.contains("rt-format-check"),
            "请求体应包含正确的 refresh_token 值: {}",
            body
        );

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_refresh_lock_double_check_early_return() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        save_refresh_token(&db, "rt-should-not-be-used").unwrap();

        // 预设有效的 access_token（模拟"其他请求已完成 refresh"）
        set_access_token("already-refreshed".to_string(), 3600).await;

        // 启动 mock 服务器——如果早返回不生效，会命中此服务器
        let mock_body = r#"{"access_token":"should-not-appear","refresh_token":"should-not-appear","expires_in":3600}"#.to_string();
        let (addr, _handle, captured) =
            start_mock_server_with_capture(200, "OK", mock_body).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = refresh_access_token(&db, &client).await;

        // 应返回 Ok(Some)，且 access_token 是预设的值（不是 mock 返回的）
        assert!(result.is_ok());
        let refresh_result = result.unwrap().unwrap();
        assert_eq!(refresh_result.access_token, "already-refreshed");
        assert_eq!(refresh_result.expires_in, 0);

        // 验证 mock 服务器未被命中
        let bodies = captured.lock().unwrap();
        assert_eq!(bodies.len(), 0, "锁后双检查应早返回，不应发送 HTTP 请求");

        // refresh_token 不应被替换
        assert_eq!(
            load_refresh_token(&db).unwrap().as_deref(),
            Some("rt-should-not-be-used")
        );

        clear_access_token().await;
    }

    #[tokio::test]
    async fn test_refresh_lock_serializes_concurrent_refresh() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        save_refresh_token(&db, "rt-concurrent").unwrap();

        // 启动 mock 服务器，记录命中次数
        let mock_body = r#"{"access_token":"at-concurrent","refresh_token":"rt-concurrent-new","expires_in":3600}"#.to_string();
        let (addr, _handle, captured) =
            start_mock_server_with_capture(200, "OK", mock_body).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();

        // 并发发起 3 个 refresh 请求，共享同一个 &Database
        // Database 是 Send + Sync（内部 Mutex<Connection>），可安全共享引用
        let r1 = refresh_access_token(&db, &client);
        let r2 = refresh_access_token(&db, &client);
        let r3 = refresh_access_token(&db, &client);
        let (r1, r2, r3) = tokio::join!(r1, r2, r3);

        // 所有请求都应成功
        assert!(r1.is_ok() && r2.is_ok() && r3.is_ok());

        // mock 服务器应只被命中 1 次（REFRESH_LOCK 串行化 + 锁后双检查）
        let bodies = captured.lock().unwrap();
        assert_eq!(
            bodies.len(),
            1,
            "并发 refresh 应只触发 1 次真实 HTTP 请求，实际 {} 次",
            bodies.len()
        );

        clear_access_token().await;
    }

    // === FP-P1-2 测试：submit_translate ===

    #[test]
    fn test_generate_idempotency_key_is_uuid_v4() {
        let key = generate_idempotency_key();
        // UUID v4 格式：8-4-4-4-12
        assert_eq!(key.len(), 36);
        let parts: Vec<&str> = key.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
        // version 4
        assert!(parts[2].starts_with('4'));
    }

    #[test]
    fn test_generate_idempotency_key_unique() {
        let key1 = generate_idempotency_key();
        let key2 = generate_idempotency_key();
        assert_ne!(key1, key2);
    }

    #[tokio::test]
    async fn test_submit_translate_402_insufficient_balance() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        // mock 402 响应
        let error_body = r#"{"error":"insufficient_balance","message":"Token 余额不足"}"#;
        let (addr, _, _) = start_mock_server_with_capture(402, "Payment Required", error_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate(&db, &client, "test content", "srt", "polly-standard", "key-1").await;

        assert!(result.is_ok(), "submit_translate 应返回 Ok，错误在 response 中: {:?}", result.err());
        match result.unwrap() {
            TranslateSubmitResponse::Error { status, error_code, message } => {
                assert_eq!(status, 402);
                assert_eq!(error_code, "insufficient_balance");
                assert_eq!(message, "Token 余额不足");
            }
            other => panic!("期望 Error，实际: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_submit_translate_202_accepted() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        let accept_body = r#"{"job_id":"job-12345"}"#;
        let (addr, _, _) = start_mock_server_with_capture(202, "Accepted", accept_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate(&db, &client, "test content", "srt", "polly-standard", "key-2").await;

        match result.unwrap() {
            TranslateSubmitResponse::Accepted { job_id } => {
                assert_eq!(job_id, "job-12345");
            }
            other => panic!("期望 Accepted，实际: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_submit_translate_200_json_completed() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        // 200 + application/json（断点续传时任务已完成的场景）
        let result_body = r#"{"subtitles":[{"index":1,"start_ms":1000,"end_ms":3000,"translated_text":"你好","original_text":"Hello"}],"tokens_used":100,"cost":50,"token_balance":9000}"#;
        let (addr, _, _) = start_mock_server_with_capture(200, "application/json", result_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate(&db, &client, "test content", "srt", "polly-standard", "key-3").await;

        match result.unwrap() {
            TranslateSubmitResponse::CompletedJson(result) => {
                assert_eq!(result.subtitles.len(), 1);
                assert_eq!(result.subtitles[0].translated_text, "你好");
                assert_eq!(result.tokens_used, 100);
                assert_eq!(result.token_balance, Some(9000));
            }
            other => panic!("期望 CompletedJson，实际: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_submit_translate_429_rate_limited() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        let error_body = r#"{"error":"rate_limited","message":"请求过于频繁"}"#;
        let (addr, _, _) = start_mock_server_with_capture(429, "Too Many Requests", error_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate(&db, &client, "test content", "srt", "polly-standard", "key-4").await;

        match result.unwrap() {
            TranslateSubmitResponse::Error { status, error_code, .. } => {
                assert_eq!(status, 429);
                assert_eq!(error_code, "rate_limited");
            }
            other => panic!("期望 Error，实际: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_submit_translate_500_server_error() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        let error_body = r#"{"error":"internal_error","message":"服务端内部错误"}"#;
        let (addr, _, _) = start_mock_server_with_capture(500, "Internal Server Error", error_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate(&db, &client, "test content", "srt", "polly-standard", "key-5").await;

        match result.unwrap() {
            TranslateSubmitResponse::Error { status, error_code, .. } => {
                assert_eq!(status, 500);
                assert_eq!(error_code, "internal_error");
            }
            other => panic!("期望 Error，实际: {:?}", other),
        }
    }

    // === submit_translate_one 测试（单条翻译）===

    #[tokio::test]
    async fn test_submit_translate_one_200_success() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        let result_body = r#"{"translated_text":"你好世界","tokens_used":10,"cost":5,"token_balance":9990,"bonus_balance":100}"#;
        let (addr, _, _) = start_mock_server_with_capture(200, "application/json", result_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate_one(
            &db, &client, "test content", "srt", 5, "Hello World", None, "polly-standard", "entry-key-1",
        ).await;

        assert!(result.is_ok(), "submit_translate_one 应返回 Ok: {:?}", result.err());
        let resp = result.unwrap();
        assert_eq!(resp.translated_text, "你好世界");
        assert_eq!(resp.tokens_used, 10);
        assert_eq!(resp.cost, 5);
        assert_eq!(resp.token_balance, Some(9990));
        assert_eq!(resp.bonus_balance, Some(100));
    }

    #[tokio::test]
    async fn test_submit_translate_one_200_with_job_id() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        let result_body = r#"{"translated_text":"你好","tokens_used":5,"cost":2}"#;
        let (addr, _, captures) = start_mock_server_with_capture(200, "application/json", result_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate_one(
            &db, &client, "test content", "srt", 3, "Hello", Some("job-abc"), "polly-standard", "entry-key-2",
        ).await;

        assert!(result.is_ok(), "submit_translate_one 应返回 Ok: {:?}", result.err());
        assert_eq!(result.unwrap().translated_text, "你好");

        // 验证 multipart 请求体中包含 job_id 字段
        let captures = captures.lock().unwrap();
        let request_body = captures.join("\n");
        assert!(request_body.contains("job_id"), "multipart 请求应包含 job_id 字段");
        assert!(request_body.contains("job-abc"), "job_id 值应为 job-abc");
        assert!(request_body.contains("entry_id"), "multipart 请求应包含 entry_id 字段");
        assert!(request_body.contains("original_text"), "multipart 请求应包含 original_text 字段");
    }

    #[tokio::test]
    async fn test_submit_translate_one_402_insufficient_balance() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        let error_body = r#"{"error":"insufficient_balance","message":"Token 余额不足"}"#;
        let (addr, _, _) = start_mock_server_with_capture(402, "Payment Required", error_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate_one(
            &db, &client, "test content", "srt", 0, "Hello", None, "polly-standard", "entry-key-3",
        ).await;

        match result {
            Err(TranslateError::ServerError { status, error_code, message }) => {
                assert_eq!(status, 402);
                assert_eq!(error_code, "insufficient_balance");
                assert_eq!(message, "Token 余额不足");
            }
            other => panic!("期望 ServerError(402)，实际: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_submit_translate_one_429_rate_limited() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        let error_body = r#"{"error":"rate_limited","message":"请求过于频繁"}"#;
        let (addr, _, _) = start_mock_server_with_capture(429, "Too Many Requests", error_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate_one(
            &db, &client, "test content", "srt", 0, "Hello", None, "polly-standard", "entry-key-4",
        ).await;

        match result {
            Err(TranslateError::ServerError { status, error_code, .. }) => {
                assert_eq!(status, 429);
                assert_eq!(error_code, "rate_limited");
            }
            other => panic!("期望 ServerError(429)，实际: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_submit_translate_one_500_server_error() {
        let _guard = test_lock();
        clear_access_token().await;
        let db = test_db();
        set_access_token("at-test".to_string(), 3600).await;

        let error_body = r#"{"error":"internal_error","message":"服务端内部错误"}"#;
        let (addr, _, _) = start_mock_server_with_capture(500, "Internal Server Error", error_body.to_string()).await;
        db.set_config("zimufan_api_base_url", &format!("http://{}", addr)).unwrap();

        let client = reqwest::Client::new();
        let result = submit_translate_one(
            &db, &client, "test content", "srt", 0, "Hello", None, "polly-standard", "entry-key-5",
        ).await;

        match result {
            Err(TranslateError::ServerError { status, error_code, .. }) => {
                assert_eq!(status, 500);
                assert_eq!(error_code, "internal_error");
            }
            other => panic!("期望 ServerError(500)，实际: {:?}", other),
        }
    }

    // === FP-P1-3 测试：SSE 解析 ===

    #[test]
    fn test_parse_sse_done() {
        let event = parse_sse_event("data: [DONE]");
        assert!(matches!(event, Some(SseEvent::Done)));
    }

    #[test]
    fn test_parse_sse_heartbeat() {
        let event = parse_sse_event("data: {\"type\":\"heartbeat\"}");
        assert!(matches!(event, Some(SseEvent::Heartbeat)));
    }

    #[test]
    fn test_parse_sse_progress() {
        let event = parse_sse_event("data: {\"phase\":\"translate\",\"step\":\"translate\",\"current\":50,\"total\":100}");
        match event {
            Some(SseEvent::Progress { phase, step, current, total }) => {
                assert_eq!(phase, "translate");
                assert_eq!(step, "translate");
                assert_eq!(current, 50);
                assert_eq!(total, 100);
            }
            other => panic!("期望 Progress，实际: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_result() {
        let event = parse_sse_event("data: {\"type\":\"result\",\"subtitles\":[{\"index\":1,\"start_ms\":1000,\"end_ms\":3000,\"translated_text\":\"你好\",\"original_text\":\"Hello\"}],\"tokens_used\":100,\"cost\":50,\"token_balance\":9000,\"bonus_balance\":3000}");
        match event {
            Some(SseEvent::Result(result)) => {
                assert_eq!(result.subtitles.len(), 1);
                assert_eq!(result.subtitles[0].translated_text, "你好");
                assert_eq!(result.tokens_used, 100);
                assert_eq!(result.token_balance, Some(9000));
                assert_eq!(result.bonus_balance, Some(3000));
            }
            other => panic!("期望 Result，实际: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_error() {
        let event = parse_sse_event("data: {\"phase\":\"translate\",\"step\":\"translate\",\"error\":\"translation_failed\",\"message\":\"翻译失败\"}");
        match event {
            Some(SseEvent::Error { phase, step, error_code, message }) => {
                assert_eq!(phase, "translate");
                assert_eq!(step, "translate");
                assert_eq!(error_code, "translation_failed");
                assert_eq!(message, "翻译失败");
            }
            other => panic!("期望 Error，实际: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_invalid_data() {
        assert!(parse_sse_event("not data: something").is_none());
        assert!(parse_sse_event("data: invalid json").is_none());
        assert!(parse_sse_event("data: ").is_none()); // [DONE] trimmed = [DONE], but empty is None
    }

    #[test]
    fn test_parse_sse_result_without_balance() {
        let event = parse_sse_event("data: {\"type\":\"result\",\"subtitles\":[],\"tokens_used\":0,\"cost\":0}");
        match event {
            Some(SseEvent::Result(result)) => {
                assert!(result.token_balance.is_none());
                assert!(result.bonus_balance.is_none());
            }
            other => panic!("期望 Result，实际: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_error_content_filtered() {
        let event = parse_sse_event("data: {\"phase\":\"translate\",\"step\":\"translate\",\"error\":\"content_filtered\",\"message\":\"内容被过滤\"}");
        match event {
            Some(SseEvent::Error { error_code, .. }) => {
                assert_eq!(error_code, "content_filtered");
            }
            other => panic!("期望 Error，实际: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_error_llm_call_failed() {
        let event = parse_sse_event("data: {\"phase\":\"translate\",\"step\":\"translate\",\"error\":\"llm_call_failed\",\"message\":\"AI 服务不可用\"}");
        match event {
            Some(SseEvent::Error { error_code, .. }) => {
                assert_eq!(error_code, "llm_call_failed");
            }
            other => panic!("期望 Error，实际: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_progress_polish_step() {
        // 精品档润色阶段
        let event = parse_sse_event("data: {\"phase\":\"translate\",\"step\":\"polish\",\"current\":75,\"total\":100}");
        match event {
            Some(SseEvent::Progress { step, current, .. }) => {
                assert_eq!(step, "polish");
                assert_eq!(current, 75);
            }
            other => panic!("期望 Progress，实际: {:?}", other),
        }
    }

    // === handle_sse_stream 测试 ===

    /// mock SSE 服务器：按顺序发送 SSE 事件，然后关闭连接
    async fn start_mock_sse_server(events: Vec<String>) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            use tokio::io::AsyncWriteExt;
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        let events = events.clone();
                        tokio::spawn(async move {
                            // 读取请求
                            let mut buf = vec![0u8; 4096];
                            let _ = stream.read(&mut buf).await;
                            // 发送 SSE 响应头
                            let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
                            let _ = stream.write_all(header.as_bytes()).await;
                            let _ = stream.flush().await;
                            // 逐个发送事件
                            for event in &events {
                                let _ = stream.write_all(event.as_bytes()).await;
                                let _ = stream.flush().await;
                            }
                        });
                    }
                    Err(_) => break,
                }
            }
        });
        addr
    }

    /// mock SSE 事件处理器：记录所有回调
    struct MockSseHandler {
        progress_calls: std::sync::Arc<std::sync::Mutex<Vec<(String, String, u32, u32)>>>,
        result_calls: std::sync::Arc<std::sync::Mutex<Vec<TranslateResult>>>,
        error_calls: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>,
        done_called: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl MockSseHandler {
        fn new() -> Self {
            Self {
                progress_calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                result_calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                error_calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                done_called: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        }
    }

    impl SseEventHandler for MockSseHandler {
        fn on_progress(&self, phase: &str, step: &str, current: u32, total: u32) {
            self.progress_calls.lock().unwrap().push((
                phase.to_string(), step.to_string(), current, total,
            ));
        }
        fn on_partial(&self, _subtitles: &[crate::subtitle::ServerSubtitleEntry], _current: u32, _total: u32) {
            // 测试中暂不验证 partial
        }
        fn on_result(&self, result: TranslateResult) {
            self.result_calls.lock().unwrap().push(result);
        }
        fn on_error(&self, phase: &str, step: &str, error_code: &str, message: &str) {
            self.error_calls.lock().unwrap().push((
                phase.to_string(), step.to_string(), error_code.to_string(), message.to_string(),
            ));
        }
        fn on_done(&self) {
            self.done_called.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn test_handle_sse_stream_progress_and_done() {
        let events = vec![
            "data: {\"phase\":\"translate\",\"step\":\"translate\",\"current\":50,\"total\":100}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ];
        let addr = start_mock_sse_server(events).await;

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        let result = handle_sse_stream(resp, &handler).await.unwrap();

        // 应收到 1 个 progress 事件
        let progress = handler.progress_calls.lock().unwrap();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].0, "translate");
        assert_eq!(progress[0].2, 50);
        assert_eq!(progress[0].3, 100);

        // done 应被调用
        assert!(handler.done_called.load(std::sync::atomic::Ordering::SeqCst));

        // 返回 None（无 result 事件）
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_handle_sse_stream_result_event() {
        let events = vec![
            "data: {\"type\":\"result\",\"subtitles\":[{\"index\":1,\"start_ms\":1000,\"end_ms\":3000,\"translated_text\":\"你好\",\"original_text\":\"Hello\"}],\"tokens_used\":100,\"cost\":50,\"token_balance\":9000}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ];
        let addr = start_mock_sse_server(events).await;

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        let result = handle_sse_stream(resp, &handler).await.unwrap();

        // 应收到 1 个 result 事件
        let results = handler.result_calls.lock().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subtitles.len(), 1);
        assert_eq!(results[0].tokens_used, 100);

        // 返回 Some(result)
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.tokens_used, 100);
        assert_eq!(r.token_balance, Some(9000));
    }

    #[tokio::test]
    async fn test_handle_sse_stream_error_event() {
        let events = vec![
            "data: {\"phase\":\"translate\",\"step\":\"translate\",\"error\":\"translation_failed\",\"message\":\"翻译失败\"}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ];
        let addr = start_mock_sse_server(events).await;

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        let _ = handle_sse_stream(resp, &handler).await.unwrap();

        // 应收到 1 个 error 事件
        let errors = handler.error_calls.lock().unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].2, "translation_failed");
        assert_eq!(errors[0].3, "翻译失败");
    }

    #[tokio::test]
    async fn test_handle_sse_stream_heartbeat_resets_watchdog() {
        // 验证心跳重置看门狗：使用短超时（watchdog=150ms, chunk=50ms）
        // 服务器在 100ms 时发送心跳（此时看门狗未超时但接近），心跳后重置看门狗
        // 然后在 200ms 时发送 Done（如果没有心跳重置，100ms 时就会看门狗超时）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            use tokio::io::AsyncWriteExt;
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 4096];
                            let _ = stream.read(&mut buf).await;
                            let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
                            let _ = stream.write_all(header.as_bytes()).await;
                            let _ = stream.flush().await;
                            // 100ms 后发送心跳（watchdog=150ms，此时未超时但接近）
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            let _ = stream.write_all(b"data: {\"type\":\"heartbeat\"}\n\n").await;
                            let _ = stream.flush().await;
                            // 再 100ms 后发送 Done（总共 200ms，若无心跳重置则 150ms 时已超时）
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            let _ = stream.write_all(b"data: [DONE]\n\n").await;
                            let _ = stream.flush().await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        // watchdog=150ms, chunk=50ms —— 若心跳未重置看门狗，150ms 时会超时
        let result = handle_sse_stream_with_timeouts(
            resp, &handler,
            std::time::Duration::from_millis(150),
            std::time::Duration::from_millis(50),
        ).await.unwrap();

        // 应正常完成（心跳在 100ms 时重置了看门狗，Done 在 200ms 时到达）
        assert!(result.is_none());
        assert!(handler.done_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_handle_sse_stream_watchdog_timeout() {
        // 验证看门狗超时：服务器接受连接但不发送任何数据
        // 使用短超时（watchdog=100ms, chunk=30ms），100ms 后应返回看门狗超时错误
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            use tokio::io::AsyncWriteExt;
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 4096];
                            let _ = stream.read(&mut buf).await;
                            // 发送 SSE 头但不发送任何事件，保持连接
                            let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
                            let _ = stream.write_all(header.as_bytes()).await;
                            let _ = stream.flush().await;
                            // 不发送任何数据，等待看门狗超时
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        // watchdog=100ms, chunk=30ms —— 100ms 后应返回看门狗超时
        let result = handle_sse_stream_with_timeouts(
            resp, &handler,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(30),
        ).await;

        // 应返回看门狗超时错误
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            TranslateError::NetworkError(msg) => {
                assert!(msg.contains("看门狗超时"), "期望看门狗超时错误，实际: {}", msg);
            }
            other => panic!("期望 NetworkError(看门狗超时)，实际: {:?}", other),
        }
        // done 不应被调用
        assert!(!handler.done_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_handle_sse_stream_eof_without_done() {
        // 流结束但无 [DONE] 事件
        let events = vec![
            "data: {\"phase\":\"translate\",\"step\":\"translate\",\"current\":100,\"total\":100}\n\n".to_string(),
        ];
        let addr = start_mock_sse_server(events).await;

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        let result = handle_sse_stream(resp, &handler).await.unwrap();

        // 应正常完成（EOF）
        assert!(result.is_none());
        // done 不应被调用
        assert!(!handler.done_called.load(std::sync::atomic::Ordering::SeqCst));
        // 但 progress 应被调用
        assert_eq!(handler.progress_calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_handle_sse_stream_multiple_events() {
        let events = vec![
            "data: {\"phase\":\"translate\",\"step\":\"translate\",\"current\":25,\"total\":100}\n\n".to_string(),
            "data: {\"phase\":\"translate\",\"step\":\"translate\",\"current\":50,\"total\":100}\n\n".to_string(),
            "data: {\"phase\":\"translate\",\"step\":\"translate\",\"current\":75,\"total\":100}\n\n".to_string(),
            "data: {\"type\":\"result\",\"subtitles\":[],\"tokens_used\":200,\"cost\":100}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ];
        let addr = start_mock_sse_server(events).await;

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        let result = handle_sse_stream(resp, &handler).await.unwrap();

        // 3 个 progress 事件
        assert_eq!(handler.progress_calls.lock().unwrap().len(), 3);
        // 1 个 result 事件
        assert_eq!(handler.result_calls.lock().unwrap().len(), 1);
        // done 被调用
        assert!(handler.done_called.load(std::sync::atomic::Ordering::SeqCst));
        // 返回 result
        assert!(result.is_some());
        assert_eq!(result.unwrap().tokens_used, 200);
    }

    #[tokio::test]
    async fn test_handle_sse_stream_empty_response() {
        // 空响应（立即关闭连接）
        let events: Vec<String> = vec![];
        let addr = start_mock_sse_server(events).await;

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        let result = handle_sse_stream(resp, &handler).await.unwrap();

        // 应正常完成（EOF），无事件
        assert!(result.is_none());
        assert_eq!(handler.progress_calls.lock().unwrap().len(), 0);
        assert!(!handler.done_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_handle_sse_stream_split_events_across_chunks() {
        // 事件跨 chunk 边界（模拟网络分包）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            use tokio::io::AsyncWriteExt;
            loop {
                match listener.accept().await {
                    Ok((mut stream, _)) => {
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 4096];
                            let _ = stream.read(&mut buf).await;
                            let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
                            let _ = stream.write_all(header.as_bytes()).await;
                            let _ = stream.flush().await;
                            // 分包发送：先发 "data: {\"phase" 后发完
                            let part1 = "data: {\"phase\":\"translate\",\"step\":\"tr";
                            let part2 = "anslate\",\"current\":50,\"total\":100}\n\ndata: [DONE]\n\n";
                            let _ = stream.write_all(part1.as_bytes()).await;
                            let _ = stream.flush().await;
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            let _ = stream.write_all(part2.as_bytes()).await;
                            let _ = stream.flush().await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let client = reqwest::Client::new();
        let resp = client.get(format!("http://{}/", addr)).send().await.unwrap();
        let handler = MockSseHandler::new();
        let result = handle_sse_stream(resp, &handler).await.unwrap();

        // 应正确解析跨 chunk 的事件
        assert_eq!(handler.progress_calls.lock().unwrap().len(), 1);
        assert!(handler.done_called.load(std::sync::atomic::Ordering::SeqCst));
        assert!(result.is_none());
    }
}

// === SECTION 4 END ===
