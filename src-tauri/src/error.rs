// 统一错误类型 — 错误码驱动方案
// IPC 错误返回协议：{ ok: false, error: { code, args?, severity } }
// 后端只返回错误码 + 结构化参数，前端用 i18n.t("error." + code, args) 翻译

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
/// 后端不生成人类可读文本，前端根据 code + args 自行翻译
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    pub severity: Severity,
}

impl IpcError {
    pub fn new(code: &str, severity: Severity) -> Self {
        Self {
            code: code.to_string(),
            args: None,
            severity,
        }
    }

    pub fn with_args(mut self, args: serde_json::Value) -> Self {
        self.args = Some(args);
        self
    }
}

/// 后端统一错误枚举，可转换为 IpcError
/// detail 字段仅用于技术性信息（如 OS 错误消息），不用于人类可读文本
#[derive(Debug, Error)]
pub enum AppError {
    // === FFmpeg ===
    #[error("FFmpeg not found: {path}")]
    FfmpegNotFound { path: String },

    #[error("FFmpeg execution failed: {detail}")]
    FfmpegExecutionFailed { detail: String },

    #[error("FFmpeg start failed: {detail}")]
    FfmpegStartFailed { detail: String },

    #[error("FFmpeg wait failed: {detail}")]
    FfmpegWaitFailed { detail: String },

    #[error("FFmpeg wait thread disconnected")]
    FfmpegWaitDisconnected,

    #[error("FFmpeg probe start failed: {detail}")]
    FfmpegProbeStartFailed { detail: String },

    #[error("FFmpeg probe parse failed: {detail}")]
    FfmpegProbeParseFailed { detail: String },

    #[error("FFmpeg probe task failed: {detail}")]
    FfmpegProbeTaskFailed { detail: String },

    #[error("FFmpeg extract task failed: {detail}")]
    FfmpegExtractTaskFailed { detail: String },

    #[error("Graphic subtitle ({codec}) cannot be extracted as text")]
    FfmpegGraphicSubtitle { codec: String },

    #[error("Video probe failed: {video_path}")]
    FfmpegProbeFailed { video_path: String },

    #[error("Subtitle extraction failed: {detail}")]
    FfmpegExtractFailed { detail: String },

    #[error("Subtitle extraction cancelled")]
    FfmpegExtractCancelled,

    #[error("Subtitle extraction timed out ({timeout}s)")]
    FfmpegExtractTimeout { timeout: u64 },

    #[error("Subtitle stream is empty or contains no text")]
    FfmpegExtractEmptyStream,

    #[error("Keep at least one subtitle stream")]
    FfmpegNoStreamsKept,

    #[error("Subtitle merge failed: {detail}")]
    FfmpegMergeFailed { detail: String },

    #[error("FFmpeg merge task failed: {detail}")]
    FfmpegMergeTaskFailed { detail: String },

    // === FFmpeg Download ===
    #[error("FFmpeg download: mkdir failed: {detail}")]
    FfmpegDownloadMkdirFailed { detail: String },

    #[error("FFmpeg download: HTTP client build failed: {detail}")]
    FfmpegDownloadHttpClientFailed { detail: String },

    #[error("FFmpeg download: proxy config failed: {detail}")]
    FfmpegDownloadProxyFailed { detail: String },

    #[error("FFmpeg download: request failed: {detail}")]
    FfmpegDownloadRequestFailed { detail: String },

    #[error("FFmpeg download: HTTP status failed: {status}")]
    FfmpegDownloadHttpStatusFailed { status: String },

    #[error("FFmpeg download: create file failed: {detail}")]
    FfmpegDownloadCreateFileFailed { detail: String },

    #[error("FFmpeg download: stream read failed: {detail}")]
    FfmpegDownloadStreamReadFailed { detail: String },

    #[error("FFmpeg download: write failed: {detail}")]
    FfmpegDownloadWriteFailed { detail: String },

    #[error("FFmpeg download: extract failed: {detail}")]
    FfmpegDownloadExtractFailed { detail: String },

    #[error("FFmpeg download: exe not found in archive")]
    FfmpegDownloadExeNotFound,

    #[error("FFmpeg download: copy exe failed: {detail}")]
    FfmpegDownloadCopyFailed { detail: String },

    #[error("FFmpeg download: delete failed: {detail}")]
    FfmpegDownloadDeleteFailed { detail: String },

    // === Translate ===
    #[error("Translation rate limited by {provider}")]
    TranslateRateLimit { provider: String, retry_after: Option<u64> },

    #[error("Translation network error: {detail}")]
    TranslateNetworkError { provider: String, detail: String },

    #[error("Translation timeout: {provider} did not respond within {timeout_secs}s")]
    TranslateTimeout { provider: String, timeout_secs: u64 },

    #[error("Translation auth failed for {provider}")]
    TranslateAuthFailed { provider: String },

    #[error("Translation model unavailable for {provider}: {model}")]
    TranslateModelUnavailable { provider: String, model: String },

    #[error("Translation insufficient balance for {provider}: {detail}")]
    TranslateInsufficientBalance { provider: String, detail: String },

    /// 每日 token 限额已用尽（TPD），不可重试，需等次日重置
    #[error("Translation daily token limit reached for {provider}: {detail}")]
    TranslateDailyLimitReached { provider: String, detail: String },

    #[error("Translation credentials not configured")]
    TranslateCredentialsNotConfigured,

    #[error("No translation API configured")]
    TranslateNotConfigured,

    #[error("Translation alignment failed, {missing} entries missing")]
    TranslateAlignFailed { missing: usize },

    #[error("Subtitle #{index} too long, truncated")]
    TranslateSingleTooLong { index: usize },

    #[error("Subtitle #{index} placeholder broken")]
    TranslatePlaceholderBroken { index: usize },

    #[error("Translation request failed: {detail}")]
    TranslateRequestFailed { detail: String },

    #[error("Translation response parse failed: {detail}")]
    TranslateResponseParseFailed { detail: String },

    #[error("Translation retries exhausted")]
    TranslateRetriesExhausted,

    #[error("Unknown translation engine: {provider}")]
    TranslateUnknownProvider { provider: String },

    // === Player ===
    #[error("Player component not downloaded")]
    PlayerLibmpvNotDownloaded,

    #[error("Player component download failed: {detail}")]
    PlayerLibmpvDownloadFailed { detail: String },

    #[error("Player component download: create directory failed: {detail}")]
    PlayerDownloadMkdirFailed { detail: String },

    #[error("Player component download: proxy config failed: {detail}")]
    PlayerDownloadProxyFailed { detail: String },

    #[error("Player component download: HTTP client build failed: {detail}")]
    PlayerDownloadHttpClientFailed { detail: String },

    #[error("Player component download: RSS request failed: {detail}")]
    PlayerDownloadRssRequestFailed { detail: String },

    #[error("Player component download: RSS status code abnormal: {status}")]
    PlayerDownloadRssStatusFailed { status: String },

    #[error("Player component download: RSS read failed: {detail}")]
    PlayerDownloadRssReadFailed { detail: String },

    #[error("Player component download: mpv-dev-x86_64 package not found in RSS")]
    PlayerDownloadPackageNotFound,

    #[error("Player component download: download request failed: {detail}")]
    PlayerDownloadRequestFailed { detail: String },

    #[error("Player component download: HTTP status code abnormal: {status}")]
    PlayerDownloadHttpStatusFailed { status: String },

    #[error("Player component download: create file failed: {detail}")]
    PlayerDownloadCreateFileFailed { detail: String },

    #[error("Player component download: read stream failed: {detail}")]
    PlayerDownloadStreamReadFailed { detail: String },

    #[error("Player component download: write file failed: {detail}")]
    PlayerDownloadWriteFailed { detail: String },

    #[error("Player component download: create extraction directory failed: {detail}")]
    PlayerDownloadExtractMkdirFailed { detail: String },

    #[error("Player component download: extract 7z failed: {detail}")]
    PlayerDownloadExtractFailed { detail: String },

    #[error("Player component download: libmpv-2.dll or mpv-2.dll not found after extraction")]
    PlayerDownloadDllNotFound,

    #[error("Player component download: copy dll failed: {detail}")]
    PlayerDownloadCopyDllFailed { detail: String },

    #[error("Player component download: delete directory failed: {detail}")]
    PlayerDownloadDeleteFailed { detail: String },

    #[error("Player component checksum mismatch")]
    PlayerLibmpvChecksumMismatch,

    #[error("Player load failed: {detail}")]
    PlayerLoadFailed { detail: String },

    #[error("Player: load libmpv.dll failed: {detail}")]
    PlayerLibmpvDllLoadFailed { detail: String },

    #[error("Player: symbol {name} not found: {detail}")]
    PlayerSymbolNotFound { name: String, detail: String },

    #[error("Player: set wid failed: {code}")]
    PlayerSetWidFailed { code: String },

    #[error("Player: mpv_initialize failed: {code}")]
    PlayerInitFailed { code: String },

    #[error("Player: load video failed: {path} ({code})")]
    PlayerLoadVideoFailed { path: String, code: String },

    #[error("Player: seek failed: {code}")]
    PlayerSeekFailed { code: String },

    #[error("Player: set option {name}={value} failed: {code}")]
    PlayerSetOptionFailed { name: String, value: String, code: String },

    #[error("Player: set property {name}={value} failed: {code}")]
    PlayerSetPropertyFailed { name: String, value: String, code: String },

    #[error("Player: create floating window failed: {detail}")]
    PlayerCreateWindowFailed { detail: String },

    // === Search ===
    #[error("Search quota exhausted for {provider}")]
    SearchQuotaExhausted { provider: String },

    #[error("Search auth failed for {provider}")]
    SearchAuthFailed { provider: String },

    #[error("Search network error for {provider}: {detail}")]
    SearchNetworkError { provider: String, detail: String },

    #[error("Search provider not configured")]
    SearchNotConfigured,

    #[error("Search download failed from {provider}")]
    SearchDownloadFailed { provider: String },

    #[error("Search captcha required from {provider}")]
    SearchCaptchaRequired {
        provider: String,
        captcha_image: String,
        session_cookie: String,
        original_url: String,
        verify_path: String,
    },

    // === Subtitle ===
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

    // === Storage ===
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

    // === System ===
    #[error("Context menu register failed: {detail}")]
    SystemContextMenuRegisterFailed { detail: String },

    #[error("Context menu unregister failed: {detail}")]
    SystemContextMenuUnregisterFailed { detail: String },

    #[error("Single instance forward failed")]
    SystemSingleInstanceForwardFailed,

    #[error("WebView2 Runtime missing")]
    SystemWebview2Missing,

    // === Common ===
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

    #[error("Get data directory failed: {detail}")]
    GetDataDirFailed { detail: String },

    #[error("Download task failed: {detail}")]
    DownloadTaskFailed { detail: String },

    // === Batch ===
    #[error("批量翻译队列错误: {detail}")]
    BatchQueueError { detail: String },

    #[error("文件夹监视错误: {detail}")]
    BatchWatchError { detail: String },

    #[error("批量翻译文件不存在: {path}")]
    BatchFileNotFound { path: String },

    #[error("批量翻译配置无效: {detail}")]
    BatchConfigInvalid { detail: String },

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
    /// 只返回错误码 + 结构化参数 + 严重级别，不生成人类可读文本
    pub fn to_ipc_error(&self) -> IpcError {
        use AppError::*;
        match self {
            // === FFmpeg ===
            FfmpegNotFound { path } => IpcError::new("ffmpeg.notFound", Severity::Reinstall)
                .with_args(serde_json::json!({ "path": path })),

            FfmpegExecutionFailed { detail } => IpcError::new("ffmpeg.executionFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegStartFailed { detail } => IpcError::new("ffmpeg.startFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegWaitFailed { detail } => IpcError::new("ffmpeg.waitFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegWaitDisconnected => IpcError::new("ffmpeg.waitDisconnected", Severity::Recoverable),

            FfmpegProbeStartFailed { detail } => IpcError::new("ffmpeg.probeStartFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegProbeParseFailed { detail } => IpcError::new("ffmpeg.probeParseFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegProbeTaskFailed { detail } => IpcError::new("ffmpeg.probeTaskFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegExtractTaskFailed { detail } => IpcError::new("ffmpeg.extractTaskFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegGraphicSubtitle { codec } => IpcError::new("ffmpeg.graphicSubtitle", Severity::Recoverable)
                .with_args(serde_json::json!({ "codec": codec })),

            FfmpegProbeFailed { video_path } => IpcError::new("ffmpeg.probeFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "videoPath": video_path })),

            FfmpegExtractFailed { detail } => IpcError::new("ffmpeg.extractFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegExtractCancelled => IpcError::new("ffmpeg.extractCancelled", Severity::Recoverable),

            FfmpegExtractTimeout { timeout } => IpcError::new("ffmpeg.extractTimeout", Severity::Recoverable)
                .with_args(serde_json::json!({ "timeout": timeout })),

            FfmpegExtractEmptyStream => IpcError::new("ffmpeg.extractEmptyStream", Severity::Recoverable),

            FfmpegNoStreamsKept => IpcError::new("ffmpeg.noStreamsKept", Severity::Recoverable),

            FfmpegMergeFailed { detail } => IpcError::new("ffmpeg.mergeFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            FfmpegMergeTaskFailed { detail } => IpcError::new("ffmpeg.mergeTaskFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            // === FFmpeg Download ===
            FfmpegDownloadMkdirFailed { detail } => IpcError::new("ffmpeg.downloadMkdirFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadHttpClientFailed { detail } => IpcError::new("ffmpeg.downloadHttpClientFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadProxyFailed { detail } => IpcError::new("ffmpeg.downloadProxyFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadRequestFailed { detail } => IpcError::new("ffmpeg.downloadRequestFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadHttpStatusFailed { status } => IpcError::new("ffmpeg.downloadHttpStatusFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "status": status })),
            FfmpegDownloadCreateFileFailed { detail } => IpcError::new("ffmpeg.downloadCreateFileFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadStreamReadFailed { detail } => IpcError::new("ffmpeg.downloadStreamReadFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadWriteFailed { detail } => IpcError::new("ffmpeg.downloadWriteFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadExtractFailed { detail } => IpcError::new("ffmpeg.downloadExtractFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadExeNotFound => IpcError::new("ffmpeg.downloadExeNotFound", Severity::Recoverable),
            FfmpegDownloadCopyFailed { detail } => IpcError::new("ffmpeg.downloadCopyFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),
            FfmpegDownloadDeleteFailed { detail } => IpcError::new("ffmpeg.downloadDeleteFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            // === Translate ===
            TranslateRateLimit { provider, retry_after } => IpcError::new("translate.rateLimit", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider, "retryAfter": retry_after })),

            TranslateNetworkError { provider, detail } => IpcError::new("translate.networkError", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider, "detail": detail })),

            TranslateTimeout { provider, timeout_secs } => IpcError::new("translate.timeout", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider, "timeoutSecs": timeout_secs })),

            TranslateAuthFailed { provider } => IpcError::new("translate.authFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

            TranslateModelUnavailable { provider, model } => IpcError::new("translate.modelUnavailable", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider, "model": model })),

            TranslateInsufficientBalance { provider, detail } => IpcError::new("translate.insufficientBalance", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider, "detail": detail })),

            TranslateDailyLimitReached { provider, detail } => IpcError::new("translate.dailyLimitReached", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider, "detail": detail })),

            TranslateCredentialsNotConfigured => IpcError::new("translate.credentialsNotConfigured", Severity::Recoverable),

            TranslateNotConfigured => IpcError::new("translate.notConfigured", Severity::Recoverable),

            TranslateAlignFailed { missing } => IpcError::new("translate.alignFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "missing": missing })),

            TranslateSingleTooLong { index } => IpcError::new("translate.singleTooLong", Severity::Recoverable)
                .with_args(serde_json::json!({ "index": index })),

            TranslatePlaceholderBroken { index } => IpcError::new("translate.placeholderBroken", Severity::Recoverable)
                .with_args(serde_json::json!({ "index": index })),

            TranslateRequestFailed { detail } => IpcError::new("translate.requestFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            TranslateResponseParseFailed { detail } => IpcError::new("translate.responseParseFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            TranslateRetriesExhausted => IpcError::new("translate.retriesExhausted", Severity::Recoverable),

            TranslateUnknownProvider { provider } => IpcError::new("translate.unknownProvider", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

            // === Player ===
            PlayerLibmpvNotDownloaded => IpcError::new("player.libmpvNotDownloaded", Severity::Recoverable),

            PlayerLibmpvDownloadFailed { detail } => IpcError::new("player.libmpvDownloadFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadMkdirFailed { detail } => IpcError::new("player.downloadMkdirFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadProxyFailed { detail } => IpcError::new("player.downloadProxyFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadHttpClientFailed { detail } => IpcError::new("player.downloadHttpClientFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadRssRequestFailed { detail } => IpcError::new("player.downloadRssRequestFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadRssStatusFailed { status } => IpcError::new("player.downloadRssStatusFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "status": status })),

            PlayerDownloadRssReadFailed { detail } => IpcError::new("player.downloadRssReadFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadPackageNotFound => IpcError::new("player.downloadPackageNotFound", Severity::Recoverable),

            PlayerDownloadRequestFailed { detail } => IpcError::new("player.downloadRequestFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadHttpStatusFailed { status } => IpcError::new("player.downloadHttpStatusFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "status": status })),

            PlayerDownloadCreateFileFailed { detail } => IpcError::new("player.downloadCreateFileFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadStreamReadFailed { detail } => IpcError::new("player.downloadStreamReadFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadWriteFailed { detail } => IpcError::new("player.downloadWriteFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadExtractMkdirFailed { detail } => IpcError::new("player.downloadExtractMkdirFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadExtractFailed { detail } => IpcError::new("player.downloadExtractFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadDllNotFound => IpcError::new("player.downloadDllNotFound", Severity::Recoverable),

            PlayerDownloadCopyDllFailed { detail } => IpcError::new("player.downloadCopyDllFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerDownloadDeleteFailed { detail } => IpcError::new("player.downloadDeleteFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerLibmpvChecksumMismatch => IpcError::new("player.libmpvChecksumMismatch", Severity::Reinstall),

            PlayerLoadFailed { detail } => IpcError::new("player.loadFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerLibmpvDllLoadFailed { detail } => IpcError::new("player.libmpvDllLoadFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            PlayerSymbolNotFound { name, detail } => IpcError::new("player.symbolNotFound", Severity::Recoverable)
                .with_args(serde_json::json!({ "name": name, "detail": detail })),

            PlayerSetWidFailed { code } => IpcError::new("player.setWidFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "code": code })),

            PlayerInitFailed { code } => IpcError::new("player.initFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "code": code })),

            PlayerLoadVideoFailed { path, code } => IpcError::new("player.loadVideoFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path, "code": code })),

            PlayerSeekFailed { code } => IpcError::new("player.seekFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "code": code })),

            PlayerSetOptionFailed { name, value, code } => IpcError::new("player.setOptionFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "name": name, "value": value, "code": code })),

            PlayerSetPropertyFailed { name, value, code } => IpcError::new("player.setPropertyFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "name": name, "value": value, "code": code })),

            PlayerCreateWindowFailed { detail } => IpcError::new("player.createWindowFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            // === Search ===
            SearchQuotaExhausted { provider } => IpcError::new("search.quotaExhausted", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

            SearchAuthFailed { provider } => IpcError::new("search.authFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

            SearchNetworkError { provider, detail } => IpcError::new("search.networkError", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider, "detail": detail })),

            SearchNotConfigured => IpcError::new("search.notConfigured", Severity::Recoverable),

            SearchDownloadFailed { provider } => IpcError::new("search.downloadFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

            SearchCaptchaRequired { provider, captcha_image, session_cookie, original_url, verify_path } => IpcError::new("search.captchaRequired", Severity::Recoverable)
                .with_args(serde_json::json!({
                    "provider": provider,
                    "captchaImage": captcha_image,
                    "sessionCookie": session_cookie,
                    "originalUrl": original_url,
                    "verifyPath": verify_path,
                })),

            // === Subtitle ===
            SubtitleParseFailed { path } => IpcError::new("subtitle.parseFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path })),

            SubtitleEncodingLow { path } => IpcError::new("subtitle.encodingDetectedLow", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path })),

            SubtitleSaveFailed { path } => IpcError::new("subtitle.saveFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path })),

            SubtitleExportFailed { path } => IpcError::new("subtitle.exportFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path })),

            SubtitleFormatUnsupported { codec } => IpcError::new("subtitle.formatUnsupported", Severity::Recoverable)
                .with_args(serde_json::json!({ "codec": codec })),

            // === Storage ===
            StorageSqliteCorrupted { path } => IpcError::new("storage.sqliteCorrupted", Severity::Restart)
                .with_args(serde_json::json!({ "path": path })),

            StorageSqliteMigrationFailed { path } => IpcError::new("storage.sqliteMigrationFailed", Severity::Restart)
                .with_args(serde_json::json!({ "path": path })),

            StorageKeyringUnavailable => IpcError::new("storage.keyringUnavailable", Severity::Recoverable),

            StorageCredentialNotFound { provider } => IpcError::new("storage.credentialNotFound", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

            StorageWriteFailed { path } => IpcError::new("storage.writeFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path })),

            // === System ===
            SystemContextMenuRegisterFailed { detail } => IpcError::new("system.contextMenuRegisterFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            SystemContextMenuUnregisterFailed { detail } => IpcError::new("system.contextMenuUnregisterFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            SystemSingleInstanceForwardFailed => IpcError::new("system.singleInstanceForwardFailed", Severity::Recoverable),

            SystemWebview2Missing => IpcError::new("system.webview2Missing", Severity::Reinstall),

            // === Common ===
            FileNotFound { path } => IpcError::new("common.fileNotFound", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path })),

            FileTooLarge { size, limit } => IpcError::new("common.fileTooLarge", Severity::Recoverable)
                .with_args(serde_json::json!({ "size": size, "limit": limit })),

            TaskCancelled => IpcError::new("common.taskCancelled", Severity::Recoverable),

            PermissionDenied { path } => IpcError::new("common.permissionDenied", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path })),

            Unknown { detail } => IpcError::new("common.unknown", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            GetDataDirFailed { detail } => IpcError::new("common.getDataDirFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            DownloadTaskFailed { detail } => IpcError::new("common.downloadTaskFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            // === Batch ===
            BatchQueueError { detail } => IpcError::new("batch.queueError", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            BatchWatchError { detail } => IpcError::new("batch.watchError", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            BatchFileNotFound { path } => IpcError::new("batch.fileNotFound", Severity::Recoverable)
                .with_args(serde_json::json!({ "path": path })),

            BatchConfigInvalid { detail } => IpcError::new("batch.configInvalid", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": detail })),

            Io(e) => IpcError::new("common.ioError", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": e.to_string() })),

            SerdeJson(e) => IpcError::new("common.jsonError", Severity::Recoverable)
                .with_args(serde_json::json!({ "detail": e.to_string() })),

            Rusqlite(e) => IpcError::new("storage.sqliteError", Severity::Restart)
                .with_args(serde_json::json!({ "detail": e.to_string() })),
        }
    }
}

// === SECTION 2 END ===

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

#[cfg(test)]
mod tests {
    use super::*;

    // === SECTION 3 END ===

    // 辅助：断言 IpcError 的 code 和 severity
    fn assert_ipc(err: AppError, expected_code: &str, expected_severity: Severity) {
        let ipc = err.to_ipc_error();
        assert_eq!(ipc.code, expected_code, "code mismatch for {:?}", err);
        assert_eq!(ipc.severity, expected_severity, "severity mismatch for {:?}", err);
    }

    // 辅助：断言 IpcError 的 code + severity + args 中某个 key 的值
    fn assert_ipc_with_arg(
        err: AppError,
        expected_code: &str,
        expected_severity: Severity,
        arg_key: &str,
        expected_arg: &serde_json::Value,
    ) {
        let ipc = err.to_ipc_error();
        assert_eq!(ipc.code, expected_code);
        assert_eq!(ipc.severity, expected_severity);
        let args = ipc.args.expect("args should be present");
        let actual = args.get(arg_key).expect(&format!("arg '{}' missing", arg_key));
        assert_eq!(actual, expected_arg);
    }

    // === SECTION 4 END ===

    // === FFmpeg ===
    #[test]
    fn test_err_ffmpeg_not_found() {
        assert_ipc(AppError::FfmpegNotFound { path: "/usr/bin/ffmpeg".into() }, "ffmpeg.notFound", Severity::Reinstall);
    }

    #[test]
    fn test_err_ffmpeg_execution_failed() {
        assert_ipc_with_arg(AppError::FfmpegExecutionFailed { detail: "exit code 1".into() }, "ffmpeg.executionFailed", Severity::Recoverable, "detail", &serde_json::json!("exit code 1"));
    }

    #[test]
    fn test_err_ffmpeg_graphic_subtitle() {
        assert_ipc_with_arg(AppError::FfmpegGraphicSubtitle { codec: "hdmv_pgs".into() }, "ffmpeg.graphicSubtitle", Severity::Recoverable, "codec", &serde_json::json!("hdmv_pgs"));
    }

    #[test]
    fn test_err_ffmpeg_extract_cancelled() {
        assert_ipc(AppError::FfmpegExtractCancelled, "ffmpeg.extractCancelled", Severity::Recoverable);
    }

    #[test]
    fn test_err_ffmpeg_extract_timeout() {
        assert_ipc_with_arg(AppError::FfmpegExtractTimeout { timeout: 30 }, "ffmpeg.extractTimeout", Severity::Recoverable, "timeout", &serde_json::json!(30));
    }

    #[test]
    fn test_err_ffmpeg_no_streams_kept() {
        assert_ipc(AppError::FfmpegNoStreamsKept, "ffmpeg.noStreamsKept", Severity::Recoverable);
    }

    // === SECTION 5 END ===

    // === Translate ===
    #[test]
    fn test_err_translate_rate_limit() {
        assert_ipc_with_arg(AppError::TranslateRateLimit { provider: "baidu".into(), retry_after: Some(5) }, "translate.rateLimit", Severity::Recoverable, "provider", &serde_json::json!("baidu"));
    }

    #[test]
    fn test_err_translate_auth_failed() {
        assert_ipc_with_arg(AppError::TranslateAuthFailed { provider: "google".into() }, "translate.authFailed", Severity::Recoverable, "provider", &serde_json::json!("google"));
    }

    #[test]
    fn test_err_translate_not_configured() {
        assert_ipc(AppError::TranslateNotConfigured, "translate.notConfigured", Severity::Recoverable);
    }

    #[test]
    fn test_err_translate_credentials_not_configured() {
        assert_ipc(AppError::TranslateCredentialsNotConfigured, "translate.credentialsNotConfigured", Severity::Recoverable);
    }

    #[test]
    fn test_err_translate_align_failed() {
        assert_ipc_with_arg(AppError::TranslateAlignFailed { missing: 3 }, "translate.alignFailed", Severity::Recoverable, "missing", &serde_json::json!(3));
    }

    #[test]
    fn test_err_translate_unknown_provider() {
        assert_ipc_with_arg(AppError::TranslateUnknownProvider { provider: "deepl".into() }, "translate.unknownProvider", Severity::Recoverable, "provider", &serde_json::json!("deepl"));
    }

    #[test]
    fn test_err_translate_retries_exhausted() {
        assert_ipc(AppError::TranslateRetriesExhausted, "translate.retriesExhausted", Severity::Recoverable);
    }

    // === SECTION 6 END ===

    // === Search ===
    #[test]
    fn test_err_search_quota_exhausted() {
        assert_ipc_with_arg(AppError::SearchQuotaExhausted { provider: "opensubtitles".into() }, "search.quotaExhausted", Severity::Recoverable, "provider", &serde_json::json!("opensubtitles"));
    }

    #[test]
    fn test_err_search_not_configured() {
        assert_ipc(AppError::SearchNotConfigured, "search.notConfigured", Severity::Recoverable);
    }

    #[test]
    fn test_err_search_captcha_required() {
        let err = AppError::SearchCaptchaRequired {
            provider: "zimuku".into(),
            captcha_image: "base64data".into(),
            session_cookie: "cookie123".into(),
            original_url: "https://zimuku.org/search".into(),
            verify_path: "/verify".into(),
        };
        let ipc = err.to_ipc_error();
        assert_eq!(ipc.code, "search.captchaRequired");
        assert_eq!(ipc.severity, Severity::Recoverable);
        let args = ipc.args.unwrap();
        assert_eq!(args["provider"], "zimuku");
        assert_eq!(args["captchaImage"], "base64data");
        assert_eq!(args["sessionCookie"], "cookie123");
    }

    // === SECTION 7 END ===

    // === Subtitle ===
    #[test]
    fn test_err_subtitle_parse_failed() {
        assert_ipc_with_arg(AppError::SubtitleParseFailed { path: "/test.srt".into() }, "subtitle.parseFailed", Severity::Recoverable, "path", &serde_json::json!("/test.srt"));
    }

    #[test]
    fn test_err_subtitle_format_unsupported() {
        assert_ipc_with_arg(AppError::SubtitleFormatUnsupported { codec: "microdvd".into() }, "subtitle.formatUnsupported", Severity::Recoverable, "codec", &serde_json::json!("microdvd"));
    }

    // === SECTION 8 END ===

    // === Storage ===
    #[test]
    fn test_err_storage_sqlite_corrupted() {
        assert_ipc_with_arg(AppError::StorageSqliteCorrupted { path: "/data.db".into() }, "storage.sqliteCorrupted", Severity::Restart, "path", &serde_json::json!("/data.db"));
    }

    #[test]
    fn test_err_storage_keyring_unavailable() {
        assert_ipc(AppError::StorageKeyringUnavailable, "storage.keyringUnavailable", Severity::Recoverable);
    }

    #[test]
    fn test_err_storage_credential_not_found() {
        assert_ipc_with_arg(AppError::StorageCredentialNotFound { provider: "baidu".into() }, "storage.credentialNotFound", Severity::Recoverable, "provider", &serde_json::json!("baidu"));
    }

    // === SECTION 9 END ===

    // === System ===
    #[test]
    fn test_err_system_webview2_missing() {
        assert_ipc(AppError::SystemWebview2Missing, "system.webview2Missing", Severity::Reinstall);
    }

    #[test]
    fn test_err_system_single_instance_forward() {
        assert_ipc(AppError::SystemSingleInstanceForwardFailed, "system.singleInstanceForwardFailed", Severity::Recoverable);
    }

    // === SECTION 10 END ===

    // === Common ===
    #[test]
    fn test_err_file_not_found() {
        assert_ipc_with_arg(AppError::FileNotFound { path: "/missing.mkv".into() }, "common.fileNotFound", Severity::Recoverable, "path", &serde_json::json!("/missing.mkv"));
    }

    #[test]
    fn test_err_file_too_large() {
        let err = AppError::FileTooLarge { size: 500_000_000, limit: 100_000_000 };
        let ipc = err.to_ipc_error();
        assert_eq!(ipc.code, "common.fileTooLarge");
        let args = ipc.args.unwrap();
        assert_eq!(args["size"], 500_000_000);
        assert_eq!(args["limit"], 100_000_000);
    }

    #[test]
    fn test_err_task_cancelled() {
        assert_ipc(AppError::TaskCancelled, "common.taskCancelled", Severity::Recoverable);
    }

    #[test]
    fn test_err_permission_denied() {
        assert_ipc_with_arg(AppError::PermissionDenied { path: "/secret".into() }, "common.permissionDenied", Severity::Recoverable, "path", &serde_json::json!("/secret"));
    }

    #[test]
    fn test_err_unknown() {
        assert_ipc_with_arg(AppError::Unknown { detail: "something".into() }, "common.unknown", Severity::Recoverable, "detail", &serde_json::json!("something"));
    }

    // === SECTION 11 END ===

    // === Player ===
    #[test]
    fn test_err_player_libmpv_not_downloaded() {
        assert_ipc(AppError::PlayerLibmpvNotDownloaded, "player.libmpvNotDownloaded", Severity::Recoverable);
    }

    #[test]
    fn test_err_player_libmpv_checksum_mismatch() {
        assert_ipc(AppError::PlayerLibmpvChecksumMismatch, "player.libmpvChecksumMismatch", Severity::Reinstall);
    }

    #[test]
    fn test_err_player_load_video_failed() {
        let err = AppError::PlayerLoadVideoFailed { path: "/video.mkv".into(), code: "-1".into() };
        let ipc = err.to_ipc_error();
        assert_eq!(ipc.code, "player.loadVideoFailed");
        let args = ipc.args.unwrap();
        assert_eq!(args["path"], "/video.mkv");
        assert_eq!(args["code"], "-1");
    }

    // === SECTION 12 END ===

    // === IpcError 构造 ===
    #[test]
    fn test_ipc_error_new() {
        let err = IpcError::new("test.code", Severity::Recoverable);
        assert_eq!(err.code, "test.code");
        assert_eq!(err.severity, Severity::Recoverable);
        assert!(err.args.is_none());
    }

    #[test]
    fn test_ipc_error_with_args() {
        let err = IpcError::new("test.code", Severity::Recoverable)
            .with_args(serde_json::json!({ "key": "value" }));
        assert!(err.args.is_some());
        assert_eq!(err.args.unwrap()["key"], "value");
    }

    // === SECTION 13 END ===

    // === IpcResult 转换 ===
    #[test]
    fn test_ipc_result_ok() {
        let result: Result<i32, AppError> = Ok(42);
        let ipc = IpcResult::from(result);
        assert!(ipc.ok);
        assert_eq!(ipc.value, Some(42));
        assert!(ipc.error.is_none());
    }

    #[test]
    fn test_ipc_result_err() {
        let result: Result<i32, AppError> = Err(AppError::TaskCancelled);
        let ipc = IpcResult::from(result);
        assert!(!ipc.ok);
        assert!(ipc.value.is_none());
        assert!(ipc.error.is_some());
        assert_eq!(ipc.error.unwrap().code, "common.taskCancelled");
    }

    #[test]
    fn test_ipc_result_wrapper() {
        let ok_result: Result<i32, AppError> = Ok(10);
        let ipc = ipc_result(ok_result);
        assert!(ipc.ok);
        assert_eq!(ipc.value, Some(10));
    }

    // === SECTION 14 END ===

    // === Severity 序列化 ===
    #[test]
    fn test_severity_serialize() {
        assert_eq!(serde_json::to_string(&Severity::Recoverable).unwrap(), "\"recoverable\"");
        assert_eq!(serde_json::to_string(&Severity::Restart).unwrap(), "\"restart\"");
        assert_eq!(serde_json::to_string(&Severity::Reinstall).unwrap(), "\"reinstall\"");
    }

    #[test]
    fn test_severity_deserialize() {
        let s: Severity = serde_json::from_str("\"restart\"").unwrap();
        assert_eq!(s, Severity::Restart);
    }

    #[test]
    fn test_severity_eq() {
        assert_eq!(Severity::Recoverable, Severity::Recoverable);
        assert_ne!(Severity::Recoverable, Severity::Restart);
    }

    // === SECTION 15 END ===

    // === Io / SerdeJson / Rusqlite 透明转发 ===
    #[test]
    fn test_err_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = AppError::Io(io_err);
        let ipc = err.to_ipc_error();
        assert_eq!(ipc.code, "common.ioError");
        assert_eq!(ipc.severity, Severity::Recoverable);
    }

    #[test]
    fn test_err_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
        let err = AppError::SerdeJson(json_err);
        let ipc = err.to_ipc_error();
        assert_eq!(ipc.code, "common.jsonError");
        assert_eq!(ipc.severity, Severity::Recoverable);
    }

    // === SECTION 16 END ===
}
