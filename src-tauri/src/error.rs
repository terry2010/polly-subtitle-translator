// 统一错误类型，对应 docs/error-codes.md
// IPC 错误返回协议：{ ok: false, error: { code, i18nKey, args?, message, severity } }

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 错误严重级别，决定前端 UI 反馈强度
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// 用户可恢复，前端用 toast 提示
    Recoverable,
    /// 需重启应用，前端用模态对话框提示重启
    Restart,
    /// 需重装或重配环境，前端用模态对话框引导重装/重配
    Reinstall,
}

/// IPC 错误返回体（前端接收的 error 字段）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: String,
    pub i18n_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    pub message: String,
    pub severity: Severity,
}

impl IpcError {
    pub fn new(code: &str, i18n_key: &str, message: &str, severity: Severity) -> Self {
        Self {
            code: code.to_string(),
            i18n_key: i18n_key.to_string(),
            args: None,
            message: message.to_string(),
            severity,
        }
    }

    pub fn with_args(mut self, args: serde_json::Value) -> Self {
        self.args = Some(args);
        self
    }
}

/// 后端统一错误枚举，可转换为 IpcError
#[derive(Debug, Error)]
pub enum AppError {
    #[error("FFmpeg not found: {path}")]
    FfmpegNotFound { path: String },

    #[error("FFmpeg execution failed: {detail}")]
    FfmpegExecutionFailed { detail: String },

    #[error("Graphic subtitle ({codec}) cannot be extracted as text")]
    FfmpegGraphicSubtitle { codec: String },

    #[error("Video probe failed: {video_path}")]
    FfmpegProbeFailed { video_path: String },

    #[error("Subtitle extraction failed: {detail}")]
    FfmpegExtractFailed { detail: String },

    #[error("Subtitle merge failed: {detail}")]
    FfmpegMergeFailed { detail: String },

    #[error("Translation rate limited by {provider}")]
    TranslateRateLimit { provider: String, retry_after: Option<u64> },

    #[error("Translation network error: {detail}")]
    TranslateNetworkError { provider: String, detail: String },

    #[error("Translation auth failed for {provider}")]
    TranslateAuthFailed { provider: String },

    #[error("No translation API configured")]
    TranslateNotConfigured,

    #[error("Translation alignment failed, {missing} entries missing")]
    TranslateAlignFailed { missing: usize },

    #[error("Subtitle #{index} too long, truncated")]
    TranslateSingleTooLong { index: usize },

    #[error("Subtitle #{index} placeholder broken")]
    TranslatePlaceholderBroken { index: usize },

    #[error("Player component not downloaded")]
    PlayerLibmpvNotDownloaded,

    #[error("Player component download failed: {detail}")]
    PlayerLibmpvDownloadFailed { detail: String },

    #[error("Player component checksum mismatch")]
    PlayerLibmpvChecksumMismatch,

    #[error("Video load failed: {video_path}")]
    PlayerLoadFailed { video_path: String },

    #[error("Search quota exhausted for {provider}")]
    SearchQuotaExhausted { provider: String },

    #[error("Search auth failed for {provider}")]
    SearchAuthFailed { provider: String },

    #[error("Search network error for {provider}")]
    SearchNetworkError { provider: String },

    #[error("Search provider not configured")]
    SearchNotConfigured,

    #[error("Search download failed from {provider}")]
    SearchDownloadFailed { provider: String },

    #[error("Subtitle parse failed: {path}")]
    SubtitleParseFailed { path: String },

    #[error("Subtitle encoding uncertain: {path}")]
    SubtitleEncodingLow { path: String },

    #[error("Subtitle save failed: {path}")]
    SubtitleSaveFailed { path: String },

    #[error("Subtitle export failed: {path}")]
    SubtitleExportFailed { path: String },

    #[error("Unsupported subtitle format: {codec}")]
    SubtitleFormatUnsupported { codec: String },

    #[error("Database corrupted: {path}")]
    StorageSqliteCorrupted { path: String },

    #[error("Database migration failed: {path}")]
    StorageSqliteMigrationFailed { path: String },

    #[error("Keychain unavailable")]
    StorageKeyringUnavailable,

    #[error("Credential not found for {provider}")]
    StorageCredentialNotFound { provider: String },

    #[error("Write failed: {path}")]
    StorageWriteFailed { path: String },

    #[error("Context menu register failed: {detail}")]
    SystemContextMenuRegisterFailed { detail: String },

    #[error("Context menu unregister failed: {detail}")]
    SystemContextMenuUnregisterFailed { detail: String },

    #[error("Single instance forward failed")]
    SystemSingleInstanceForwardFailed,

    #[error("WebView2 Runtime missing")]
    SystemWebview2Missing,

    #[error("File not found: {path}")]
    FileNotFound { path: String },

    #[error("File too large: {size} bytes")]
    FileTooLarge { size: u64, limit: u64 },

    #[error("Task cancelled")]
    TaskCancelled,

    #[error("Permission denied: {path}")]
    PermissionDenied { path: String },

    #[error("Unknown error: {detail}")]
    Unknown { detail: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),

    #[error(transparent)]
    Rusqlite(#[from] rusqlite::Error),
}

// === SECTION 1 END ===

impl AppError {
    /// 将 AppError 转换为前端可消费的 IpcError
    pub fn to_ipc_error(&self) -> IpcError {
        use AppError::*;
        match self {
            FfmpegNotFound { path } => IpcError::new(
                "ffmpeg.notFound",
                "error.ffmpeg.notFound",
                &format!("FFmpeg not found at {}", path),
                Severity::Reinstall,
            )
            .with_args(serde_json::json!({ "path": path })),

            FfmpegExecutionFailed { detail } => IpcError::new(
                "ffmpeg.executionFailed",
                "error.ffmpeg.executionFailed",
                &format!("FFmpeg execution failed: {}", detail),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": detail })),

            FfmpegGraphicSubtitle { codec } => IpcError::new(
                "ffmpeg.graphicSubtitle",
                "error.ffmpeg.graphicSubtitle",
                &format!("Graphic subtitle ({}) cannot be extracted as text", codec),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "codec": codec })),

            FfmpegProbeFailed { video_path } => IpcError::new(
                "ffmpeg.probeFailed",
                "error.ffmpeg.probeFailed",
                &format!("Video probe failed for {}", video_path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "videoPath": video_path })),

            FfmpegExtractFailed { detail } => IpcError::new(
                "ffmpeg.extractFailed",
                "error.ffmpeg.extractFailed",
                &format!("Subtitle extraction failed: {}", detail),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": detail })),

            FfmpegMergeFailed { detail } => IpcError::new(
                "ffmpeg.mergeFailed",
                "error.ffmpeg.mergeFailed",
                &format!("Subtitle merge failed: {}", detail),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": detail })),

            TranslateRateLimit { provider, retry_after } => IpcError::new(
                "translate.rateLimit",
                "error.translate.rateLimit",
                &format!("Translation rate limited by {}", provider),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "provider": provider, "retryAfter": retry_after })),

            TranslateNetworkError { provider, detail } => IpcError::new(
                "translate.networkError",
                "error.translate.networkError",
                &format!("Translation network error: {}", detail),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "provider": provider, "detail": detail })),

            TranslateAuthFailed { provider } => IpcError::new(
                "translate.authFailed",
                "error.translate.authFailed",
                &format!("{} translation auth failed", provider),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "provider": provider })),

            TranslateNotConfigured => IpcError::new(
                "translate.notConfigured",
                "error.translate.notConfigured",
                "No translation API configured",
                Severity::Recoverable,
            ),

            TranslateAlignFailed { missing } => IpcError::new(
                "translate.alignFailed",
                "error.translate.alignFailed",
                &format!("Translation alignment failed, {} entries missing", missing),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "missing": missing })),

            TranslateSingleTooLong { index } => IpcError::new(
                "translate.singleTooLong",
                "error.translate.singleTooLong",
                &format!("Subtitle #{} too long, truncated", index),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "index": index })),

            TranslatePlaceholderBroken { index } => IpcError::new(
                "translate.placeholderBroken",
                "error.translate.placeholderBroken",
                &format!("Subtitle #{} placeholder broken", index),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "index": index })),

            PlayerLibmpvNotDownloaded => IpcError::new(
                "player.libmpvNotDownloaded",
                "error.player.libmpvNotDownloaded",
                "Player component not downloaded",
                Severity::Recoverable,
            ),

            PlayerLibmpvDownloadFailed { detail } => IpcError::new(
                "player.libmpvDownloadFailed",
                "error.player.libmpvDownloadFailed",
                &format!("Player component download failed: {}", detail),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": detail })),

            PlayerLibmpvChecksumMismatch => IpcError::new(
                "player.libmpvChecksumMismatch",
                "error.player.libmpvChecksumMismatch",
                "Player component checksum mismatch",
                Severity::Reinstall,
            ),

            PlayerLoadFailed { video_path } => IpcError::new(
                "player.loadFailed",
                "error.player.loadFailed",
                &format!("Video load failed: {}", video_path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "videoPath": video_path })),

            SearchQuotaExhausted { provider } => IpcError::new(
                "search.quotaExhausted",
                "error.search.quotaExhausted",
                &format!("Search quota exhausted for {}", provider),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "provider": provider })),

            SearchAuthFailed { provider } => IpcError::new(
                "search.authFailed",
                "error.search.authFailed",
                &format!("Search auth failed for {}", provider),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "provider": provider })),

            SearchNetworkError { provider } => IpcError::new(
                "search.networkError",
                "error.search.networkError",
                &format!("Search network error for {}", provider),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "provider": provider })),

            SearchNotConfigured => IpcError::new(
                "search.notConfigured",
                "error.search.notConfigured",
                "Search provider not configured",
                Severity::Recoverable,
            ),

            SearchDownloadFailed { provider } => IpcError::new(
                "search.downloadFailed",
                "error.search.downloadFailed",
                &format!("Subtitle download failed from {}", provider),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "provider": provider })),

            SubtitleParseFailed { path } => IpcError::new(
                "subtitle.parseFailed",
                "error.subtitle.parseFailed",
                &format!("Subtitle parse failed for {}", path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "path": path })),

            SubtitleEncodingLow { path } => IpcError::new(
                "subtitle.encodingDetectedLow",
                "error.subtitle.encodingDetectedLow",
                &format!("Subtitle encoding uncertain for {}", path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "path": path })),

            SubtitleSaveFailed { path } => IpcError::new(
                "subtitle.saveFailed",
                "error.subtitle.saveFailed",
                &format!("Subtitle save failed to {}", path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "path": path })),

            SubtitleExportFailed { path } => IpcError::new(
                "subtitle.exportFailed",
                "error.subtitle.exportFailed",
                &format!("Subtitle export failed to {}", path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "path": path })),

            SubtitleFormatUnsupported { codec } => IpcError::new(
                "subtitle.formatUnsupported",
                "error.subtitle.formatUnsupported",
                &format!("Unsupported subtitle format: {}", codec),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "codec": codec })),

            StorageSqliteCorrupted { path } => IpcError::new(
                "storage.sqliteCorrupted",
                "error.storage.sqliteCorrupted",
                &format!("Database corrupted at {}", path),
                Severity::Restart,
            )
            .with_args(serde_json::json!({ "path": path })),

            StorageSqliteMigrationFailed { path } => IpcError::new(
                "storage.sqliteMigrationFailed",
                "error.storage.sqliteMigrationFailed",
                &format!("Database migration failed for {}", path),
                Severity::Restart,
            )
            .with_args(serde_json::json!({ "path": path })),

            StorageKeyringUnavailable => IpcError::new(
                "storage.keyringUnavailable",
                "error.storage.keyringUnavailable",
                "System keychain unavailable",
                Severity::Recoverable,
            ),

            StorageCredentialNotFound { provider } => IpcError::new(
                "storage.credentialNotFound",
                "error.storage.credentialNotFound",
                &format!("Credential not found for {}", provider),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "provider": provider })),

            StorageWriteFailed { path } => IpcError::new(
                "storage.writeFailed",
                "error.storage.writeFailed",
                &format!("Write failed to {}", path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "path": path })),

            SystemContextMenuRegisterFailed { detail } => IpcError::new(
                "system.contextMenuRegisterFailed",
                "error.system.contextMenuRegisterFailed",
                &format!("Context menu register failed: {}", detail),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": detail })),

            SystemContextMenuUnregisterFailed { detail } => IpcError::new(
                "system.contextMenuUnregisterFailed",
                "error.system.contextMenuUnregisterFailed",
                &format!("Context menu unregister failed: {}", detail),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": detail })),

            SystemSingleInstanceForwardFailed => IpcError::new(
                "system.singleInstanceForwardFailed",
                "error.system.singleInstanceForwardFailed",
                "Single instance forward failed",
                Severity::Recoverable,
            ),

            SystemWebview2Missing => IpcError::new(
                "system.webview2Missing",
                "error.system.webview2Missing",
                "WebView2 Runtime missing",
                Severity::Reinstall,
            ),

            FileNotFound { path } => IpcError::new(
                "common.fileNotFound",
                "error.common.fileNotFound",
                &format!("File not found: {}", path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "path": path })),

            FileTooLarge { size, limit } => IpcError::new(
                "common.fileTooLarge",
                "error.common.fileTooLarge",
                &format!("File too large ({} bytes, limit {})", size, limit),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "size": size, "limit": limit })),

            TaskCancelled => IpcError::new(
                "common.taskCancelled",
                "error.common.taskCancelled",
                "Task cancelled",
                Severity::Recoverable,
            ),

            PermissionDenied { path } => IpcError::new(
                "common.permissionDenied",
                "error.common.permissionDenied",
                &format!("Permission denied for {}", path),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "path": path })),

            Unknown { detail } => IpcError::new(
                "common.unknown",
                "error.common.unknown",
                &format!("Unknown error: {}", detail),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": detail })),

            Io(e) => IpcError::new(
                "common.unknown",
                "error.common.unknown",
                &format!("IO error: {}", e),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": e.to_string() })),

            SerdeJson(e) => IpcError::new(
                "common.unknown",
                "error.common.unknown",
                &format!("JSON error: {}", e),
                Severity::Recoverable,
            )
            .with_args(serde_json::json!({ "detail": e.to_string() })),

            Rusqlite(e) => IpcError::new(
                "storage.sqliteCorrupted",
                "error.storage.sqliteCorrupted",
                &format!("Database error: {}", e),
                Severity::Restart,
            ),
        }
    }
}

/// Tauri 命令返回类型：成功返回 { ok: true, value: T }，失败返回 { ok: false, error: {...} }
#[derive(Debug, Clone, Serialize)]
pub struct IpcResult<T: Serialize> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

impl<T: Serialize> From<Result<T, AppError>> for IpcResult<T> {
    fn from(result: Result<T, AppError>) -> Self {
        match result {
            Ok(v) => IpcResult { ok: true, value: Some(v), error: None },
            Err(e) => IpcResult { ok: false, value: None, error: Some(e.to_ipc_error()) },
        }
    }
}

/// 便捷包装：将 Result<T, AppError> 转为 IpcResult<T>
pub fn ipc_result<T: Serialize>(result: Result<T, AppError>) -> IpcResult<T> {
    result.into()
}

// === SECTION 2 END ===
