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

    #[error("Translation auth failed for {provider}")]
    TranslateAuthFailed { provider: String },

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

    #[error("Search network error for {provider}")]
    SearchNetworkError { provider: String },

    #[error("Search provider not configured")]
    SearchNotConfigured,

    #[error("Search download failed from {provider}")]
    SearchDownloadFailed { provider: String },

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

            TranslateAuthFailed { provider } => IpcError::new("translate.authFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

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

            SearchNetworkError { provider } => IpcError::new("search.networkError", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

            SearchNotConfigured => IpcError::new("search.notConfigured", Severity::Recoverable),

            SearchDownloadFailed { provider } => IpcError::new("search.downloadFailed", Severity::Recoverable)
                .with_args(serde_json::json!({ "provider": provider })),

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
