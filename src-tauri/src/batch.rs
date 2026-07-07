// 批量翻译队列核心模块
// 监视文件夹自动翻译 + 文件列表批量翻译
// 对应设计文档 batch-translate-design.md

use crate::db::{Database, HistoryRecord};
use crate::error::AppError;
use crate::ffmpeg;
use crate::ipc;
use crate::subtitle;
use crate::translate;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::sync::mpsc;

// === SECTION 1: 数据结构 ===

/// 任务状态枚举
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum BatchStatus {
    Queued,
    Probing,
    CheckingSubtitle,
    Extracting(f64),
    Parsing,
    Translating(f64),
    Exporting,
    Done,
    Skipped(String),
    Failed(String),
    Cancelled,
}

impl Default for BatchStatus {
    fn default() -> Self {
        BatchStatus::Queued
    }
}

/// 源文件类型
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PathType {
    Video,
    Subtitle,
}

impl Default for PathType {
    fn default() -> Self {
        PathType::Video
    }
}

/// 单个文件任务
#[derive(Clone, Debug, Serialize, Default)]
pub struct BatchTask {
    pub id: String,
    pub video_path: String,
    pub source_path_type: PathType,
    pub status: BatchStatus,
    pub subtitle_path: Option<String>,
    pub output_path: Option<String>,
    pub source_lang: String,
    pub target_lang: String,
    pub provider: String,
    pub total_entries: usize,
    pub done_entries: usize,
    pub cached_entries: usize,
    pub failed_entries: usize,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
}

/// 输出模式
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum OutputMode {
    Monolingual,
    Bilingual,
}

/// 工作时间设定
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum BatchSchedule {
    Always,
    TimeWindow {
        windows: Vec<(u32, u32)>,
        weekdays: Vec<u32>,
    },
}

impl Default for BatchSchedule {
    fn default() -> Self {
        BatchSchedule::Always
    }
}

impl BatchSchedule {
    /// 判断当前时间是否在工作时间内
    /// 支持跨午夜窗口：当 end <= start 时视为跨天（如 (22, 2) = 22-24 + 0-2）
    pub fn is_active_now(&self) -> bool {
        match self {
            BatchSchedule::Always => true,
            BatchSchedule::TimeWindow { windows, weekdays } => {
                use chrono::{Datelike, Local, Timelike};
                let now = Local::now();
                let weekday = now.weekday().num_days_from_sunday();
                let hour = now.hour();

                if !weekdays.is_empty() && !weekdays.contains(&weekday) {
                    return false;
                }
                if windows.is_empty() {
                    return true;
                }
                windows.iter().any(|(start, end)| {
                    if end > start {
                        hour >= *start && hour < *end
                    } else {
                        hour >= *start || hour < *end
                    }
                })
            }
        }
    }
}

/// 用户配置
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchConfig {
    /// 翻译时传给引擎的源语言（取 source_langs 第一个）
    #[serde(default = "default_source_lang")]
    pub source_lang: String,
    /// 源语言多选（仅作过滤，字幕语言不在列表中则跳过；空列表 = 不过滤）
    #[serde(default)]
    pub source_langs: Vec<String>,
    /// 目标语言
    pub target_lang: String,
    /// 不翻译的语言列表（检测到这些语言则跳过，外挂+内嵌+内容三处检测）
    #[serde(default)]
    pub skip_langs: Vec<String>,
    pub provider: String,
    pub model: Option<String>,
    pub model_type: Option<String>,
    pub service_id: Option<String>,
    pub file_concurrency: usize,
    pub entry_concurrency: usize,
    pub output_mode: OutputMode,
    /// 输出格式多选（选多种则一次生成多个不同格式字幕文件）
    #[serde(default = "default_output_formats")]
    pub output_formats: Vec<subtitle::SubtitleFormat>,
    /// @deprecated 已废弃，由 output_formats 取代（向后兼容）
    #[serde(default = "default_output_format")]
    pub output_format: subtitle::SubtitleFormat,
    /// 嵌入视频（将字幕合并到 mkv 文件中）
    #[serde(default)]
    pub embed_to_video: bool,
    #[serde(default = "default_output_suffix")]
    pub output_suffix: String,
    pub check_external: bool,
    pub check_embedded: bool,
    pub watch_paths: Vec<String>,
    pub watch_recursive: bool,
    pub scan_on_start: bool,
    pub schedule: BatchSchedule,
    pub min_file_size_mb: u64,
    pub min_duration_secs: f64,
    pub skip_cache: bool,
    pub debounce_secs: u64,
}

fn default_source_lang() -> String {
    "en".to_string()
}

fn default_output_suffix() -> String {
    ".zh".to_string()
}

fn default_output_format() -> subtitle::SubtitleFormat {
    subtitle::SubtitleFormat::Srt
}

fn default_output_formats() -> Vec<subtitle::SubtitleFormat> {
    vec![subtitle::SubtitleFormat::Srt]
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            source_lang: "en".to_string(),
            source_langs: vec!["en".to_string()],
            target_lang: "zh".to_string(),
            skip_langs: vec!["zh".to_string()],
            provider: "".to_string(),
            model: None,
            model_type: None,
            service_id: None,
            file_concurrency: 1,
            entry_concurrency: 3,
            output_mode: OutputMode::Bilingual,
            output_formats: vec![subtitle::SubtitleFormat::Srt],
            output_format: subtitle::SubtitleFormat::Srt,
            embed_to_video: false,
            output_suffix: ".zh".to_string(),
            check_external: true,
            check_embedded: true,
            watch_paths: Vec::new(),
            watch_recursive: true,
            scan_on_start: false,
            schedule: BatchSchedule::Always,
            min_file_size_mb: 1,
            min_duration_secs: 10.0,
            skip_cache: false,
            debounce_secs: 3,
        }
    }
}

// === SECTION 1 END ===

// === SECTION 2: 队列命令与全局队列 ===

/// 队列命令（通过 mpsc channel 发送给 worker）
#[derive(Debug)]
pub(crate) enum BatchCmd {
    AddTask(BatchTask),
    CancelTask(String),
    RetryTask(String),
    StartTask(String),
    DeleteTask(String),
    ReorderTasks(Vec<String>),
    ClearQueue,
    UpdateConfig(BatchConfig),
    Pause(Option<String>),
    Resume,
    /// 扫描已有文件并检查（外挂/内嵌字幕），不需要翻译的标记 Skipped
    ScanExisting,
    #[allow(dead_code)]
    Shutdown,
}

/// 全局批量翻译队列（通过 tauri::manage 注入为 State）
pub struct BatchQueue {
    pub(crate) tx: mpsc::Sender<BatchCmd>,
    pub(crate) tasks: Arc<Mutex<Vec<BatchTask>>>,
    pub(crate) config: Arc<Mutex<BatchConfig>>,
    pub(crate) watcher: Arc<Mutex<Option<FolderWatcher>>>,
    /// 扫描取消标志（true 表示请求取消当前扫描）
    pub(crate) scan_cancel: Arc<std::sync::atomic::AtomicBool>,
    #[allow(dead_code)]
    pub(crate) paused: Arc<Mutex<bool>>,
}

/// 当前时间戳（秒）
pub(crate) fn now_ts() -> i64 {
    chrono::Local::now().timestamp()
}

/// 将 task 的当前状态同步到全局 tasks 表（按 id 匹配更新）+ 持久化到 DB
fn sync_task_to_global(
    tasks: &Arc<Mutex<Vec<BatchTask>>>,
    task: &BatchTask,
    db: Option<&Database>,
) {
    {
        let mut guard = tasks.lock().unwrap();
        if let Some(t) = guard.iter_mut().find(|t| t.id == task.id) {
            *t = task.clone();
        }
    }
    // V2 持久化：同步写入 DB
    if let Some(db) = db {
        if let Err(e) = db.upsert_batch_task(task) {
            tracing::warn!("批量任务持久化失败 (id={}): {}", task.id, e);
        }
    }
}

/// 更新任务状态并同步到全局表 + emit 事件
fn update_status(
    app: &tauri::AppHandle,
    tasks: &Arc<Mutex<Vec<BatchTask>>>,
    task: &BatchTask,
    new_status: BatchStatus,
) {
    {
        let mut guard = tasks.lock().unwrap();
        if let Some(t) = guard.iter_mut().find(|t| t.id == task.id) {
            t.status = new_status.clone();
        }
    }
    let _ = app.emit(
        "batch-task-status",
        serde_json::json!({ "id": &task.id, "status": &new_status }),
    );
}

/// 通用事件发射
fn emit_batch_event<T: Serialize>(app: &tauri::AppHandle, event: &str, payload: &T) {
    let _ = app.emit(
        event,
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    );
}

/// 标记任务失败：更新状态 + emit 事件 + 清理临时文件 + 检查队列完成
fn fail_task(
    app: &tauri::AppHandle,
    tasks: &Arc<Mutex<Vec<BatchTask>>>,
    task: &BatchTask,
    error: &str,
    db: Option<&Database>,
) {
    let finished = now_ts();
    let mut updated_task = task.clone();
    updated_task.status = BatchStatus::Failed(error.to_string());
    updated_task.error = Some(error.to_string());
    updated_task.finished_at = Some(finished);
    {
        let mut guard = tasks.lock().unwrap();
        if let Some(t) = guard.iter_mut().find(|t| t.id == task.id) {
            *t = updated_task.clone();
        }
    }
    if let Some(db) = db {
        let _ = db.upsert_batch_task(&updated_task);
    }
    let _ = app.emit(
        "batch-file-error",
        serde_json::json!({ "id": &task.id, "error": error }),
    );
    if let Some(tmp) = &task.subtitle_path {
        if task.source_path_type == PathType::Video {
            let _ = std::fs::remove_file(tmp);
        }
    }
    check_queue_complete(tasks, app);
}

/// 标记任务跳过：更新状态 + emit 事件 + 检查队列完成
fn skip_task(
    app: &tauri::AppHandle,
    tasks: &Arc<Mutex<Vec<BatchTask>>>,
    task: &BatchTask,
    reason: String,
    db: Option<&Database>,
) {
    let finished = now_ts();
    let mut updated_task = task.clone();
    updated_task.status = BatchStatus::Skipped(reason.clone());
    updated_task.finished_at = Some(finished);
    {
        let mut guard = tasks.lock().unwrap();
        if let Some(t) = guard.iter_mut().find(|t| t.id == task.id) {
            *t = updated_task.clone();
        }
    }
    if let Some(db) = db {
        let _ = db.upsert_batch_task(&updated_task);
    }
    let _ = app.emit(
        "batch-file-skipped",
        serde_json::json!({ "id": &task.id, "reason": &reason }),
    );
    check_queue_complete(tasks, app);
}

/// 更新全局表中任务状态（用于 CancelTask 等不经过 process_task 的操作）
fn update_task_status(
    tasks: &Arc<Mutex<Vec<BatchTask>>>,
    task_id: &str,
    status: BatchStatus,
) {
    let mut guard = tasks.lock().unwrap();
    if let Some(t) = guard.iter_mut().find(|t| t.id == task_id) {
        t.status = status;
        t.finished_at = Some(now_ts());
    }
}

/// 将 Failed 任务重置为 Queued（用于 RetryTask）
fn reset_task_to_queued(tasks: &Arc<Mutex<Vec<BatchTask>>>, task_id: &str) {
    let mut guard = tasks.lock().unwrap();
    if let Some(t) = guard.iter_mut().find(|t| t.id == task_id) {
        t.status = BatchStatus::Queued;
        t.error = None;
        t.finished_at = None;
        t.started_at = None;
        t.done_entries = 0;
        t.failed_entries = 0;
    }
}

/// 检查队列是否全部处理完，若是则 emit batch-queue-complete + 系统通知
fn check_queue_complete(tasks: &Arc<Mutex<Vec<BatchTask>>>, app: &tauri::AppHandle) {
    let guard = tasks.lock().unwrap();
    let pending = guard.iter().filter(|t| {
        matches!(
            t.status,
            BatchStatus::Queued
                | BatchStatus::Probing
                | BatchStatus::CheckingSubtitle
                | BatchStatus::Extracting(_)
                | BatchStatus::Parsing
                | BatchStatus::Translating(_)
                | BatchStatus::Exporting
        )
    }).count();

    if pending == 0 {
        let total = guard.len();
        if total == 0 {
            return;
        }
        let done = guard.iter().filter(|t| t.status == BatchStatus::Done).count();
        let skipped = guard
            .iter()
            .filter(|t| matches!(t.status, BatchStatus::Skipped(_)))
            .count();
        let failed = guard
            .iter()
            .filter(|t| matches!(t.status, BatchStatus::Failed(_)))
            .count();

        let _ = app.emit(
            "batch-queue-complete",
            serde_json::json!({
                "total": total,
                "done": done,
                "skipped": skipped,
                "failed": failed,
            }),
        );
    }
}

// === SECTION 2 END ===

// === SECTION 3: 辅助函数 ===

/// 判断文件是否为视频文件（与 context_menu.rs VIDEO_EXTENSIONS 一致）
pub(crate) fn is_video_file(path: &str) -> bool {
    let exts = ["mkv", "mp4", "avi", "mov", "wmv", "flv", "ts", "m2ts"];
    let is_video_ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| exts.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false);

    if !is_video_ext {
        return false;
    }

    // .ts 扩展名可能是 TypeScript 源码，用 magic bytes 区分：
    // MPEG-TS 文件以 0x47 (sync byte) 开头；TypeScript 是 UTF-8 文本
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if ext == "ts" {
        // 读取前 1 字节，检查是否为 MPEG-TS sync byte 0x47
        match std::fs::File::open(path) {
            Ok(mut f) => {
                use std::io::Read;
                let mut buf = [0u8; 1];
                match f.read_exact(&mut buf) {
                    Ok(()) => return buf[0] == 0x47,
                    Err(_) => return false,
                }
            }
            Err(_) => return false,
        }
    }

    true
}

/// 判断文件是否为字幕文件
pub(crate) fn is_subtitle_file(path: &str) -> bool {
    let exts = ["srt", "ass", "ssa", "vtt", "sub"];
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| exts.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// 检测假视频文件（下载站常见的假 mkv，内容实为广告文本）
/// 返回 Some(reason) 表示是假文件，应跳过
fn detect_fake_video(
    probe: &ffmpeg::ProbeResult,
    _video_path: &str,
    config: &BatchConfig,
) -> Option<String> {
    // 1. 无视频流
    if probe.video_stream.is_none() {
        return Some(format!(
            "假文件：无视频流（可能不是真实视频），format={}",
            probe.format.format_name
        ));
    }

    // 2. 文件大小过小
    if let Some(size) = probe.format.size {
        let size_mb = size as f64 / (1024.0 * 1024.0);
        if size_mb < config.min_file_size_mb as f64 {
            return Some(format!(
                "假文件：文件大小仅 {:.2}MB（阈值 {}MB）",
                size_mb, config.min_file_size_mb
            ));
        }
    }

    // 3. 视频时长过短
    if let Some(duration) = probe.format.duration {
        if duration < config.min_duration_secs {
            return Some(format!(
                "假文件：时长仅 {:.1}秒（阈值 {}秒）",
                duration, config.min_duration_secs
            ));
        }
    }

    // 4. 非视频容器格式
    let suspicious_formats = ["tty", "ascii", "rawvideo", "srt", "ass"];
    if suspicious_formats.contains(&probe.format.format_name.as_str()) {
        return Some(format!(
            "假文件：容器格式 '{}' 不是视频",
            probe.format.format_name
        ));
    }

    None
}

/// 从 probe 结果中选择最佳字幕流
/// 规则：优先英文 SDH → 普通英文 → 任意非图形字幕流兜底
fn select_subtitle_stream(streams: &[ffmpeg::SubtitleStream]) -> Option<i32> {
    // 1. 英文 SDH
    let en_sdsh = streams.iter().find(|s| {
        !s.is_graphic
            && s.language.as_deref() == Some("eng")
            && s.title.as_deref().map(|t| {
                t.contains("SDH") || t.contains("HI") || t.contains("CC")
            }).unwrap_or(false)
    });
    if let Some(s) = en_sdsh {
        return Some(s.index);
    }

    // 2. 普通英文
    let en = streams.iter().find(|s| {
        !s.is_graphic && s.language.as_deref() == Some("eng")
    });
    if let Some(s) = en {
        return Some(s.index);
    }

    // 3. 兜底：第一个非图形字幕流
    streams.iter().find(|s| !s.is_graphic).map(|s| s.index)
}

/// 在视频同目录查找目标语言外挂字幕
/// 扫描所有以视频 stem 开头的字幕文件，通过文件名语言标记或内容检测判断语言
fn find_external_subtitle(video_path: &str, target_lang: &str) -> Option<String> {
    let path = std::path::Path::new(video_path);
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_str()?;
    let subtitle_exts = ["srt", "ass", "vtt", "ssa", "sub"];

    // 目标语言的各种别名（用于文件名匹配）
    let lang_aliases: Vec<&str> = match target_lang.to_lowercase().as_str() {
        "zh" => vec!["zh", "chs", "cht", "chinese", "zho", "chi", "cn", "gb", "big5", "zhs", "zht"],
        "en" => vec!["en", "eng", "english"],
        "ja" => vec!["ja", "jpn", "japanese", "jp"],
        "ko" => vec!["ko", "kor", "korean", "kr"],
        _ => vec![target_lang],
    };

    // 扫描同目录下所有以视频 stem 开头的字幕文件
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return None,
    };

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() { continue; }
        let file_name = match p.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        // 必须以视频 stem 开头
        if !file_name.starts_with(stem) { continue; }
        // 必须是字幕扩展名
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !subtitle_exts.contains(&ext.as_str()) { continue; }
        candidates.push(p);
    }

    // 第一轮：通过文件名语言标记匹配
    for candidate in &candidates {
        let file_name = candidate.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
        for alias in &lang_aliases {
            // 文件名中包含 .alias. 或 .alias- 标记
            if file_name.contains(&format!(".{}.", alias)) || file_name.contains(&format!(".{}-", alias)) {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }

    // 第二轮：通过内容检测语言
    for candidate in &candidates {
        let path_str = candidate.to_string_lossy().to_string();
        let content = match std::fs::read(candidate) {
            Ok(bytes) => match subtitle::decode_bytes(&bytes) {
                Ok((c, _)) => c,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if let Ok(format) = subtitle::detect_format(&path_str) {
            if let Ok(parsed) = subtitle::parse_subtitle(&content, &format) {
                // 检测是否为双语字幕
                let bilingual = subtitle::detect_bilingual(&parsed);
                if bilingual.is_bilingual {
                    let lang_a_code = lang_class_name_to_code(&bilingual.lang_a);
                    let lang_b_code = lang_class_name_to_code(&bilingual.lang_b);
                    if lang_a_code == target_lang || lang_b_code == target_lang {
                        return Some(path_str);
                    }
                    continue;
                }
                // 检测主导语言
                let mut lang_counts: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
                for entry in &parsed.entries {
                    let cls = subtitle::detect_line_lang(&entry.text);
                    if cls != subtitle::LangClass::Other {
                        let code = lang_class_to_code(cls);
                        if !code.is_empty() {
                            *lang_counts.entry(code).or_insert(0) += 1;
                        }
                    }
                }
                if let Some((&dominant, _)) = lang_counts.iter().max_by_key(|(_, v)| *v) {
                    if dominant == target_lang {
                        return Some(path_str);
                    }
                }
            }
        }
    }

    None
}

/// 在视频同目录查找任意外挂字幕文件（不区分语言）
fn find_any_external_subtitle(video_path: &str) -> Option<String> {
    let path = std::path::Path::new(video_path);
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_str()?;
    let subtitle_exts = ["srt", "ass", "vtt", "ssa", "sub"];

    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() { continue; }
        let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !file_name.starts_with(stem) { continue; }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if subtitle_exts.contains(&ext.as_str()) {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

/// 构建输出文件路径
/// 根据输出模式自动生成后缀：
/// - 单语：.zh.srt
/// - 双语：.bilingual.zh.srt
/// - 嵌入视频：.merged.mkv
fn build_output_path(video_path: &str, config: &BatchConfig, source_lang: &str, format: &subtitle::SubtitleFormat) -> String {
    let path = std::path::Path::new(video_path);
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    let candidate = if config.embed_to_video {
        // 嵌入视频模式：输出 .merged.mkv
        dir.join(format!("{}.merged.mkv", stem))
            .to_string_lossy()
            .to_string()
    } else {
        // 字幕文件模式：根据单语/双语自动选择后缀
        let ext = match format {
            subtitle::SubtitleFormat::Srt => "srt",
            subtitle::SubtitleFormat::Ass | subtitle::SubtitleFormat::Ssa => "ass",
            subtitle::SubtitleFormat::Vtt => "vtt",
        };
        let suffix = match config.output_mode {
            OutputMode::Monolingual => format!("{}.{}", config.target_lang, ext),
            OutputMode::Bilingual => format!("{}-{}.{}", config.target_lang, source_lang, ext),
        };
        dir.join(format!("{}.{}", stem, suffix))
            .to_string_lossy()
            .to_string()
    };

    // V2 覆盖保护：如果文件已存在，加 _1 _2 ... 后缀
    ensure_unique_path(&candidate)
}

/// 如果路径已存在，在文件名后缀前加 _1 _2 ... 直到不冲突
fn ensure_unique_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return path.to_string();
    }
    let dir = p.parent().unwrap_or(std::path::Path::new("."));
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    for i in 1..=999 {
        let new_name = if ext.is_empty() {
            format!("{}_{}", stem, i)
        } else {
            format!("{}_{}.{}", stem, i, ext)
        };
        let new_path = dir.join(new_name);
        if !new_path.exists() {
            return new_path.to_string_lossy().to_string();
        }
    }
    // 兜底：加时间戳
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if ext.is_empty() {
        dir.join(format!("{}_{}", stem, ts)).to_string_lossy().to_string()
    } else {
        dir.join(format!("{}_{}.{}", stem, ts, ext)).to_string_lossy().to_string()
    }
}

/// 批量翻译中间字幕文件路径
/// <app_data_dir>/ai-subtrans/batch_tmp/<task_id>.<ext>
fn get_batch_tmp_path(task_id: &str, format: &subtitle::SubtitleFormat) -> String {
    let app_data = dirs::data_dir().unwrap_or(std::path::PathBuf::from("."));
    let tmp_dir = app_data.join("ai-subtrans").join("batch_tmp");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let ext = match format {
        subtitle::SubtitleFormat::Srt => "srt",
        subtitle::SubtitleFormat::Ass | subtitle::SubtitleFormat::Ssa => "ass",
        subtitle::SubtitleFormat::Vtt => "vtt",
    };
    tmp_dir
        .join(format!("{}.{}", task_id, ext))
        .to_string_lossy()
        .to_string()
}

/// 判断译文是否含目标语言字符（简易判断：含 CJK 字符则视为含中文）
fn has_target_lang_chars(text: &str, target_lang: &str) -> bool {
    if target_lang == "zh" || target_lang == "ja" || target_lang == "ko" {
        // CJK 统一汉字范围 + 日文 + 韩文
        text.chars().any(|c| {
            ('\u{4E00}'..='\u{9FFF}').contains(&c) // CJK 统一汉字
                || ('\u{3040}'..='\u{30FF}').contains(&c) // 日文平假名+片假名
                || ('\u{AC00}'..='\u{D7AF}').contains(&c) // 韩文
        })
    } else {
        // 非CJK语言：简单判断译文非空且与原文不同
        !text.is_empty()
    }
}

/// 构建 provider 显示名（用于历史记录）
fn build_provider_name(cfg: &BatchConfig) -> String {
    if cfg.provider == "openai" {
        match (&cfg.service_id, &cfg.model) {
            (Some(sid), Some(m)) => {
                translate::build_cache_provider_name(&["openai", sid, m])
            }
            _ => "openai".to_string(),
        }
    } else {
        cfg.provider.clone()
    }
}

/// 获取 provider 的默认限流策略（用于 scheduler 构造）
fn get_rate_limit(provider: &str) -> translate::RateLimitPolicy {
    if let Some(prov) = translate::TranslateProvider::from_str(provider) {
        prov.rate_limit_policy()
    } else {
        translate::RateLimitPolicy::Concurrency(3)
    }
}

// === SECTION 3 END ===

// === SECTION 3.5: 文件就绪检查（三级检测策略）===

/// 视频容器格式
#[derive(Clone, Copy, Debug, PartialEq)]
enum VideoFormat {
    Mkv,    // MKV / WebM
    Mp4,    // MP4 / M4V / MOV
    Avi,
    Flv,
    Ts,     // MPEG-TS
    Wmv,    // ASF / WMV
    Unknown,
}

/// 文件就绪检查结果
enum FileReadiness {
    Ready,      // 文件就绪，可以入队
    NotReady,   // 重试耗尽，文件可能损坏或仍在写入
    FakeVideo,  // 判定为假文件
}

/// 文件尾检查结果
enum TailCheckResult {
    Complete,       // 文件尾有完整结束标记 → 文件完整
    Incomplete,     // 文件尾没有结束标记 → 文件还在下载
    NotApplicable,  // 该格式没有文件尾标记 → 用 fallback
}

/// 读取文件头 N 字节
async fn read_file_header(path: &str, len: usize) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read;
    let path = path.to_string();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)?;
        let mut buf = vec![0u8; len];
        let n = file.read(&mut buf)?;
        buf.truncate(n);
        Ok(buf)
    })
    .await?
}

/// 读取文件尾 N 字节
async fn read_file_tail(path: &str, len: usize) -> Result<Vec<u8>, std::io::Error> {
    use std::io::{Read, Seek, SeekFrom};
    let path = path.to_string();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)?;
        let file_size = file.metadata()?.len();
        let start = if file_size > len as u64 {
            file_size - len as u64
        } else {
            0
        };
        file.seek(SeekFrom::Start(start))?;
        let mut buf = vec![0u8; len.min(file_size as usize)];
        file.read_exact(&mut buf)?;
        Ok(buf)
    })
    .await?
}

/// 通过文件头 magic bytes 识别视频格式
fn identify_video_format(header: &[u8]) -> VideoFormat {
    if header.len() < 12 {
        return VideoFormat::Unknown;
    }
    // MKV / WebM: EBML header magic 1A 45 DF A3
    if header.len() >= 4 && &header[0..4] == &[0x1A, 0x45, 0xDF, 0xA3] {
        return VideoFormat::Mkv;
    }
    // MP4 / M4V / MOV: offset 4 处有 "ftyp"
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        return VideoFormat::Mp4;
    }
    // AVI: RIFF....AVI
    if header.len() >= 12 && &header[0..4] == b"RIFF" && &header[8..12] == b"AVI " {
        return VideoFormat::Avi;
    }
    // FLV: 46 4C 56 01
    if header.len() >= 4 && &header[0..4] == &[0x46, 0x4C, 0x56, 0x01] {
        return VideoFormat::Flv;
    }
    // MPEG-TS: sync byte 0x47，每 188 字节重复
    if header.len() >= 188 * 3 {
        if header[0] == 0x47 && header[188] == 0x47 && header[376] == 0x47 {
            return VideoFormat::Ts;
        }
    }
    // ASF / WMV: ASF header GUID
    if header.len() >= 16
        && &header[0..8] == &[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]
    {
        return VideoFormat::Wmv;
    }
    VideoFormat::Unknown
}

/// 在 buffer 中搜索子串，返回首次出现位置
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 判断是否为网络路径（用于自动增加等待时间）
fn is_network_path(path: &str) -> bool {
    // Windows UNC 路径: \\server\share
    if path.starts_with("\\\\") {
        return true;
    }
    #[cfg(unix)]
    {
        if path.starts_with("/mnt/") || path.starts_with("/Volumes/") {
            return true;
        }
    }
    false
}

// === SECTION 3.5a END ===

/// 检查文件尾是否有容器特有的完整性标记
/// 对 MP4/AVI/MOV 可立即判定，对 MKV/TS 返回 NotApplicable
fn check_tail_completeness(tail: &[u8], file_size: u64, format: VideoFormat) -> TailCheckResult {
    match format {
        // MP4 / MOV: 文件尾应有 moov box 或 mfra box
        VideoFormat::Mp4 => {
            if let Some(pos) = find_bytes(tail, b"moov") {
                if pos >= 4 {
                    let moov_start = pos - 4;
                    let size_bytes = &tail[moov_start..moov_start + 4];
                    let moov_size = u32::from_be_bytes([
                        size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3],
                    ]) as u64;
                    let moov_offset =
                        file_size.saturating_sub(tail.len() as u64) + moov_start as u64;
                    if moov_offset + moov_size <= file_size {
                        return TailCheckResult::Complete;
                    }
                }
                return TailCheckResult::Incomplete;
            }
            if find_bytes(tail, b"mfra").is_some() {
                return TailCheckResult::Complete;
            }
            TailCheckResult::Incomplete
        }
        // AVI: 文件尾应有 idx1
        VideoFormat::Avi => {
            if let Some(pos) = find_bytes(tail, b"idx1") {
                if pos + 8 <= tail.len() {
                    return TailCheckResult::Complete;
                }
            }
            TailCheckResult::Incomplete
        }
        // MKV: 文件尾可能有 Cues 元素（1C 53 BB 6B）
        VideoFormat::Mkv => {
            if find_bytes(tail, &[0x1C, 0x53, 0xBB, 0x6B]).is_some() {
                return TailCheckResult::Complete;
            }
            TailCheckResult::Incomplete
        }
        // TS / FLV / WMV: 纯流式容器，无文件尾标记
        VideoFormat::Ts | VideoFormat::Flv | VideoFormat::Wmv => {
            TailCheckResult::NotApplicable
        }
        VideoFormat::Unknown => TailCheckResult::NotApplicable,
    }
}

/// 检查文件尾最后一个数据包是否完整（针对 TS/FLV 等流式容器）
async fn check_tail_packet_integrity(path: &str, format: VideoFormat) -> bool {
    let tail = match read_file_tail(path, 4096).await {
        Ok(t) => t,
        Err(_) => return false,
    };
    match format {
        // MPEG-TS：每个包固定 188 字节，以 sync byte 0x47 开头
        VideoFormat::Ts => {
            let file_size = match std::fs::metadata(path) {
                Ok(m) => m.len(),
                Err(_) => return false,
            };
            let remainder = file_size % 188;
            if remainder == 0 {
                if tail.len() >= 188 {
                    return tail[tail.len() - 188] == 0x47;
                }
                return true;
            }
            false
        }
        // FLV：文件尾应有 4 字节 previous tag size（大端）
        VideoFormat::Flv => {
            if tail.len() >= 4 {
                let prev_tag_size = u32::from_be_bytes([
                    tail[tail.len() - 4],
                    tail[tail.len() - 3],
                    tail[tail.len() - 2],
                    tail[tail.len() - 1],
                ]);
                if prev_tag_size > 0 && prev_tag_size < 100_000_000 {
                    return true;
                }
            }
            false
        }
        // 其他格式：保守判定为 true（已通过文件头验证）
        _ => true,
    }
}

// === SECTION 3.5b END ===

/// 文件就绪检查：文件头 magic bytes + 文件尾完整性标记 + 大小稳定 fallback
///
/// 三级检测策略（从快到慢）：
/// 1. 文件头 magic bytes（读前 512 字节）→ 确认是真实视频格式，排除假文件
/// 2. 文件尾完整性标记（读后 4KB）→ 对 MP4/AVI/MOV 等格式可立即判定完整
/// 3. 大小稳定 fallback → 对 MKV/TS 等流式容器，等待大小不再变化
async fn is_file_ready(
    path: &str,
    debounce_secs: u64,
    _min_duration_secs: f64,
    _max_retries: u32,
) -> FileReadiness {
    // ── 第 1 级：文件头 magic bytes 识别 ──
    let header = match read_file_header(path, 512).await {
        Ok(h) => h,
        Err(_) => return FileReadiness::NotReady,
    };
    let format = identify_video_format(&header);
    match format {
        VideoFormat::Unknown => return FileReadiness::FakeVideo,
        _ => {}
    }

    // ── 第 2 级：文件尾完整性标记检查 ──
    let file_size = match tokio::fs::metadata(path).await {
        Ok(m) => m.len(),
        Err(_) => return FileReadiness::NotReady,
    };
    if file_size < 1024 {
        return FileReadiness::NotReady;
    }

    let tail = match read_file_tail(path, 4096).await {
        Ok(t) => t,
        Err(_) => return FileReadiness::NotReady,
    };

    match check_tail_completeness(&tail, file_size, format) {
        TailCheckResult::Complete => return FileReadiness::Ready,
        TailCheckResult::Incomplete => {
            return wait_for_tail_marker(path, format, debounce_secs).await;
        }
        TailCheckResult::NotApplicable => {}
    }

    // ── 第 3 级：大小稳定 + 数据包完整性验证（TS/FLV/WMV）──
    let stable_threshold: u32 = 5;
    let max_wait_secs: u64 = 300;
    let mut stable_count: u32 = 0;
    let mut last_size: u64 = 0;
    let start_time = std::time::Instant::now();

    loop {
        if start_time.elapsed().as_secs() > max_wait_secs {
            tracing::warn!("文件就绪检查超时（{}秒）: {}", max_wait_secs, path);
            return FileReadiness::NotReady;
        }
        let size_now = match tokio::fs::metadata(path).await {
            Ok(m) => m.len(),
            Err(_) => return FileReadiness::NotReady,
        };
        if size_now == last_size {
            stable_count += 1;
            if stable_count >= stable_threshold {
                if check_tail_packet_integrity(path, format).await {
                    return FileReadiness::Ready;
                }
                stable_count = 0;
            }
        } else {
            stable_count = 0;
        }
        last_size = size_now;

        let wait = if is_network_path(path) {
            debounce_secs * 2
        } else {
            debounce_secs
        };
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
    }
}

/// 持续轮询文件尾，等待容器结束标记出现（MP4 moov / AVI idx1 / MKV Cues）
async fn wait_for_tail_marker(
    path: &str,
    format: VideoFormat,
    debounce_secs: u64,
) -> FileReadiness {
    let max_wait_secs: u64 = 3600; // 1 小时超时（慢速下载场景）
    let start_time = std::time::Instant::now();
    let wait = if is_network_path(path) {
        debounce_secs * 2
    } else {
        debounce_secs
    };

    for _attempt in 0..720 {
        if start_time.elapsed().as_secs() > max_wait_secs {
            tracing::warn!("等待文件尾标记超时（{}秒）: {}", max_wait_secs, path);
            return FileReadiness::NotReady;
        }
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;

        let file_size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => return FileReadiness::NotReady,
        };
        if file_size < 1024 {
            continue;
        }
        let tail = match read_file_tail(path, 4096).await {
            Ok(t) => t,
            Err(_) => continue,
        };
        match check_tail_completeness(&tail, file_size, format) {
            TailCheckResult::Complete => return FileReadiness::Ready,
            _ => continue,
        }
    }
    FileReadiness::NotReady
}

// === SECTION 3.5c END ===

/// 计算距离下一个工作时间窗口的秒数
/// 简化实现：固定返回 60 秒（每分钟检查一次）
fn calculate_seconds_to_next_window(schedule: &BatchSchedule) -> u64 {
    match schedule {
        BatchSchedule::Always => 0,
        BatchSchedule::TimeWindow { .. } => 60,
    }
}

/// 阻塞直到进入工作时间段
/// 用于 BatchSchedule::TimeWindow 模式下，非工作时间的任务等待
async fn wait_for_active_time(config: Arc<Mutex<BatchConfig>>) {
    loop {
        let cfg = config.lock().unwrap().clone();
        if cfg.schedule.is_active_now() {
            return;
        }
        let sleep_secs = calculate_seconds_to_next_window(&cfg.schedule).min(300);
        tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
    }
}

// === SECTION 3.5d END ===

/// 文件夹监视器（第四阶段实现完整逻辑，当前为占位）
pub struct FolderWatcher {
    _watcher: notify::RecommendedWatcher,
}

impl FolderWatcher {
    /// 启动文件夹监视
    pub fn start(
        paths: Vec<String>,
        recursive: bool,
        debounce_secs: u64,
        on_ready: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<Self, AppError> {
        use notify::{RecursiveMode, Watcher};
        use std::sync::mpsc as std_mpsc;

        let (tx, rx) = std_mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(tx)
            .map_err(|e| AppError::BatchWatchError { detail: e.to_string() })?;

        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        for path in &paths {
            watcher
                .watch(std::path::Path::new(path), mode)
                .map_err(|e| AppError::BatchWatchError { detail: e.to_string() })?;
        }

        // 防抖线程：用 std::thread::spawn 避免阻塞 tokio
        let debounce = std::time::Duration::from_secs(debounce_secs);
        let on_ready_arc = std::sync::Arc::new(on_ready);
        std::thread::spawn(move || {
            use notify::EventKind;
            use std::collections::HashMap;
            use std::time::Instant;

            let mut pending: HashMap<String, Instant> = HashMap::new();

            loop {
                match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                    Ok(Ok(event)) => {
                        let is_relevant = matches!(
                            event.kind,
                            EventKind::Create(_) | EventKind::Modify(_)
                        );
                        if !is_relevant {
                            continue;
                        }
                        for path in &event.paths {
                            let path_str = path.to_string_lossy().to_string();
                            if !is_video_file(&path_str) {
                                continue;
                            }
                            pending.insert(path_str, Instant::now());
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("FolderWatcher 错误: {}", e);
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {
                        let now = Instant::now();
                        let ready: Vec<String> = pending
                            .iter()
                            .filter(|(_, t)| now.duration_since(**t) >= debounce)
                            .map(|(k, _)| k.clone())
                            .collect();

                        for path in ready {
                            pending.remove(&path);
                            // 文件就绪检查：三级检测策略
                            let on_ready_clone = on_ready_arc.clone();
                            let debounce_val = debounce_secs;
                            tauri::async_runtime::spawn(async move {
                                match is_file_ready(&path, debounce_val, 1.0, 3).await {
                                    FileReadiness::Ready => {
                                        on_ready_clone(path);
                                    }
                                    FileReadiness::NotReady => {
                                        tracing::warn!(
                                            "文件就绪检查失败（超时）: {}",
                                            path
                                        );
                                    }
                                    FileReadiness::FakeVideo => {
                                        tracing::info!(
                                            "文件就绪检查：判定为假文件: {}",
                                            path
                                        );
                                    }
                                }
                            });
                        }
                    }
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        Ok(FolderWatcher { _watcher: watcher })
    }
}

/// 启动时全量扫描目录
pub fn scan_directory(path: &str, recursive: bool) -> Vec<(String, PathType)> {
    let mut files = Vec::new();
    let walker = if recursive {
        walkdir::WalkDir::new(path)
    } else {
        walkdir::WalkDir::new(path).max_depth(1)
    };

    let mut it = walker.into_iter();
    while let Some(entry) = it.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        // 跳过 node_modules 等非视频目录
        if entry.file_type().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if name == "node_modules" || name == ".git" || name == "target" || name == "dist" {
                    it.skip_current_dir();
                    continue;
                }
            }
        }

        if entry.file_type().is_file() {
            let path = entry.path().to_string_lossy().to_string();
            if is_video_file(&path) {
                files.push((path, PathType::Video));
            } else if is_subtitle_file(&path) {
                files.push((path, PathType::Subtitle));
            }
        }
    }
    files
}

/// 检查单个文件是否需要翻译（外挂字幕 + 内嵌字幕流）
/// 将 LangClass 映射到语言代码
fn lang_class_to_code(cls: subtitle::LangClass) -> &'static str {
    use subtitle::LangClass::*;
    match cls {
        Cjk => "zh", // 中日韩汉字，默认归为中文
        Hiragana | Katakana => "ja",
        Hangul => "ko",
        Latin => "en", // 拉丁字母默认归为英文
        Cyrillic => "ru",
        Arabic => "ar",
        Other => "",
    }
}

/// 将 lang_class_name 返回的字符串映射到语言代码
fn lang_class_name_to_code(name: &str) -> &'static str {
    match name {
        "cjk" => "zh",
        "hiragana" | "katakana" => "ja",
        "hangul" => "ko",
        "latin" => "en",
        "cyrillic" => "ru",
        "arabic" => "ar",
        _ => "",
    }
}

/// 为字幕文件查找同目录下对应的视频文件路径
/// 例如字幕 `Rick and Morty...TURG.eng.srt` → 视频 `Rick and Morty...TURG.mkv`
fn find_video_for_subtitle(subtitle_path: &str) -> Option<String> {
    let path = std::path::Path::new(subtitle_path);
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_str()?; // e.g. "Rick and Morty...TURG.eng"

    // 遍历目录中的视频文件，找到 stem 以视频 stem 开头的
    // 视频 stem 应该是字幕 stem 去掉语言标记后的前缀
    let video_exts = ["mkv", "mp4", "avi", "mov", "wmv", "flv", "ts", "m2ts"];
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best_match: Option<(String, usize)> = None;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() { continue; }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !video_exts.contains(&ext.as_str()) { continue; }
        let vstem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // 字幕 stem 必须以视频 stem 开头（如 "video.eng" starts with "video"）
        if stem.starts_with(vstem) {
            // 选最长的视频 stem（最精确匹配）
            if best_match.as_ref().map_or(true, |(_, len)| vstem.len() > *len) {
                best_match = Some((p.to_string_lossy().to_string(), vstem.len()));
            }
        }
    }
    best_match.map(|(p, _)| p)
}

/// 检查字幕文件是否需要翻译
/// 通过读取字幕内容并检测主导语言来判断，而非依赖文件名
fn check_subtitle_needs_translate(subtitle_path: &str, cfg: &BatchConfig) -> bool {
    // 读取字幕文件内容（使用 decode_bytes 正确处理 BOM 和编码）
    let bytes = match std::fs::read(subtitle_path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("读取字幕文件失败: {} -> {}", subtitle_path, e);
            return true;
        }
    };
    let content = match subtitle::decode_bytes(&bytes) {
        Ok((c, _)) => c,
        Err(e) => {
            tracing::warn!("解码字幕文件失败: {} -> {}", subtitle_path, e);
            return true;
        }
    };

    // 解析字幕
    let format = match subtitle::detect_format(subtitle_path) {
        Ok(f) => f,
        Err(_) => return true, // 无法识别格式，默认需要翻译
    };

    let parsed = match subtitle::parse_subtitle(&content, &format) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("解析字幕失败: {} -> {}", subtitle_path, e);
            return true;
        }
    };

    // 先检测是否为双语字幕
    let bilingual_result = subtitle::detect_bilingual(&parsed);
    if bilingual_result.is_bilingual {
        // 将语言类别名映射到语言代码
        let lang_a_code = lang_class_name_to_code(&bilingual_result.lang_a);
        let lang_b_code = lang_class_name_to_code(&bilingual_result.lang_b);
        tracing::debug!(
            "字幕 {} 检测为双语字幕（{} [{}] + {} [{}]）",
            subtitle_path, bilingual_result.lang_a, lang_a_code, bilingual_result.lang_b, lang_b_code
        );
        // 检查双语字幕是否包含目标语言或 skip_langs 中的语言
        let langs = [lang_a_code, lang_b_code];
        if langs.contains(&cfg.target_lang.as_str()) {
            return false; // 包含目标语言，不需要翻译
        }
        if cfg.skip_langs.iter().any(|l| langs.contains(&l.as_str())) {
            return false; // 包含跳过语言，不需要翻译
        }
        // 双语字幕但不包含目标语言或跳过语言，仍需要翻译
    }

    // 统计所有条目的语言分布
    let mut lang_counts: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    for entry in &parsed.entries {
        let cls = subtitle::detect_line_lang(&entry.text);
        if cls != subtitle::LangClass::Other {
            let code = lang_class_to_code(cls);
            if !code.is_empty() {
                *lang_counts.entry(code).or_insert(0) += 1;
            }
        }
    }

    if lang_counts.is_empty() {
        return true; // 检测不到语言，默认需要翻译
    }

    // 找出主导语言
    let dominant_lang = lang_counts.iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| *k)
        .unwrap_or("");

    tracing::debug!(
        "字幕 {} 语言分布: {:?}, 主导语言: {}",
        subtitle_path, lang_counts, dominant_lang
    );

    // 如果主导语言是目标语言，不需要翻译
    if dominant_lang == cfg.target_lang.as_str() {
        return false;
    }

    // 如果主导语言在 skip_langs 中，不需要翻译
    if cfg.skip_langs.iter().any(|l| l.as_str() == dominant_lang) {
        return false;
    }

    true // 需要翻译
}

async fn check_file_needs_translate(
    video_path: &str,
    cfg: &BatchConfig,
) -> Result<Option<String>, String> {
    // 2a. 检查外挂字幕
    if cfg.check_external {
        if let Some(existing) = find_external_subtitle(video_path, &cfg.target_lang) {
            return Ok(Some(format!("已有外挂目标语言字幕: {}", existing)));
        }
        for lang in &cfg.skip_langs {
            if lang == &cfg.target_lang { continue; }
            if let Some(existing) = find_external_subtitle(video_path, lang) {
                return Ok(Some(format!("已有外挂 {} 字幕，跳过: {}", lang, existing)));
            }
        }
    }

    // 2b. 检查内嵌字幕流（需要 ffprobe）
    if cfg.check_embedded {
        let path = video_path.to_string();
        let ff_path = ffmpeg::find_ffmpeg(None).ok().map(|p| p.to_string_lossy().to_string());
        let probe_result = tokio::task::spawn_blocking(move || ffmpeg::probe_video(&path, ff_path.as_deref()))
            .await;

        match probe_result {
            Ok(Ok(probe)) => {
                if probe.subtitle_streams.iter().any(|s| {
                    s.language.as_deref() == Some(cfg.target_lang.as_str())
                }) {
                    return Ok(Some("已有内嵌目标语言字幕流".to_string()));
                }
                for lang in &cfg.skip_langs {
                    if lang == &cfg.target_lang { continue; }
                    if probe.subtitle_streams.iter().any(|s| {
                        s.language.as_deref() == Some(lang.as_str())
                    }) {
                        return Ok(Some(format!("已有内嵌 {} 字幕流，跳过", lang)));
                    }
                }
            }
            Ok(Err(e)) => {
                // ffprobe 失败（如 ffmpeg 未安装），降级为只检查外挂字幕，不阻止流程
                tracing::debug!("ffprobe 检查失败，跳过内嵌字幕检查: {}", e);
            }
            Err(e) => {
                tracing::debug!("ffprobe 任务失败，跳过内嵌字幕检查: {}", e);
            }
        }
    }

    Ok(None)
}

/// 应用启动时自动恢复文件夹监视
/// 在 lib.rs setup 中延迟 2 秒调用
pub async fn auto_start_watch(app: &tauri::AppHandle) -> Result<(), AppError> {
    let batch_queue = app.state::<BatchQueue>();
    let config = batch_queue.config.lock().unwrap().clone();

    if config.watch_paths.is_empty() {
        return Ok(());
    }

    tracing::info!(
        "批量翻译：自动恢复文件夹监视，目录: {:?}",
        config.watch_paths
    );

    let paths = config.watch_paths.clone();
    let recursive = config.watch_recursive;
    let debounce = config.debounce_secs;

    // 停止旧 watcher
    {
        let mut watcher = batch_queue.watcher.lock().unwrap();
        *watcher = None;
    }

    // 若 scan_on_start，发送 ScanExisting 命令（扫描+检查+入队）
    if config.scan_on_start {
        let _ = batch_queue.tx.try_send(BatchCmd::ScanExisting);
    }

    // 启动 notify watcher
    let tx = batch_queue.tx.clone();
    let watcher = FolderWatcher::start(paths, recursive, debounce, move |file_path: String| {
        let task = BatchTask {
            id: uuid::Uuid::new_v4().to_string(),
            video_path: file_path,
            source_path_type: PathType::Video,
            status: BatchStatus::Queued,
            created_at: now_ts(),
            ..Default::default()
        };
        let _ = tx.try_send(BatchCmd::AddTask(task));
    })?;

    *batch_queue.watcher.lock().unwrap() = Some(watcher);

    Ok(())
}

/// 启动时清理 batch_tmp/ 目录残留的临时文件
pub fn cleanup_batch_tmp() {
    let app_data = dirs::data_dir().unwrap_or(std::path::PathBuf::from("."));
    let tmp_dir = app_data.join("ai-subtrans").join("batch_tmp");
    if tmp_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&tmp_dir) {
            tracing::warn!("清理 batch_tmp 目录失败: {}", e);
        } else {
            tracing::info!("已清理 batch_tmp 目录: {:?}", tmp_dir);
        }
    }
}

// === SECTION 4 END ===

// === SECTION 5: BatchWorker + process_task ===

/// 启动 BatchWorker 后台 task
/// 在 lib.rs 的 setup 钩子中调用
pub(crate) fn spawn_batch_worker(
    app: tauri::AppHandle,
    rx: mpsc::Receiver<BatchCmd>,
    tasks: Arc<Mutex<Vec<BatchTask>>>,
    config: Arc<Mutex<BatchConfig>>,
    paused: Arc<Mutex<bool>>,
    scan_cancel: Arc<std::sync::atomic::AtomicBool>,
) {
    // 使用 tauri::async_runtime::spawn 而非 tokio::spawn，
    // 因为 setup 钩子不在 Tokio 运行时上下文中
    tauri::async_runtime::spawn(async move {
        let mut rx = rx;
        tracing::info!("批量翻译：BatchWorker 已启动，等待命令...");
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            config.lock().unwrap().file_concurrency.max(1),
        ));

        loop {
            match rx.recv().await {
                Some(BatchCmd::AddTask(task)) => {
                    // V2 持久化：新任务写入 DB
                    {
                        let db = app.state::<Database>();
                        if let Err(e) = db.upsert_batch_task(&task) {
                            tracing::warn!("批量任务持久化失败: {}", e);
                        }
                    }
                    tasks.lock().unwrap().push(task.clone());
                    emit_batch_event(&app, "batch-task-added", &task);

                    if *paused.lock().unwrap() {
                        tracing::info!("批量翻译：队列已暂停，任务 {} 保持排队", task.id);
                        continue;
                    }

                    let cfg = config.lock().unwrap().clone();
                    if !cfg.schedule.is_active_now() {
                        tracing::info!(
                            "批量翻译：当前不在工作时间段，任务 {} 保持排队",
                            task.id
                        );
                        // 启动定时唤醒器：到工作时间后重新调度所有 Queued 任务
                        let config_wake = config.clone();
                        let tasks_wake = tasks.clone();
                        let paused_wake = paused.clone();
                        let sem_wake = semaphore.clone();
                        let app_wake = app.clone();
                        tokio::spawn(async move {
                            wait_for_active_time(config_wake.clone()).await;
                            // 进入工作时间，重新调度所有 Queued 任务
                            if *paused_wake.lock().unwrap() {
                                return;
                            }
                            let queued: Vec<BatchTask> = tasks_wake.lock().unwrap().iter()
                                .filter(|t| t.status == BatchStatus::Queued)
                                .cloned().collect();
                            for task in queued {
                                let sem = sem_wake.clone();
                                let app_clone = app_wake.clone();
                                let config_clone = config_wake.clone();
                                let tasks_clone = tasks_wake.clone();
                                tokio::spawn(async move {
                                    let _permit = sem.acquire().await;
                                    process_task(app_clone, tasks_clone, task, config_clone).await;
                                });
                            }
                        });
                        continue;
                    }

                    let sem = semaphore.clone();
                    let app_clone = app.clone();
                    let config_clone = config.clone();
                    let tasks_clone = tasks.clone();
                    tokio::spawn(async move {
                        let _permit = sem.acquire().await;
                        process_task(app_clone, tasks_clone, task, config_clone).await;
                    });
                }
                Some(BatchCmd::CancelTask(task_id)) => {
                    update_task_status(&tasks, &task_id, BatchStatus::Cancelled);
                    // V2 持久化：更新 DB 中的任务状态
                    {
                        let db = app.state::<Database>();
                        if let Some(task) = tasks.lock().unwrap().iter().find(|t| t.id == task_id).cloned() {
                            let _ = db.upsert_batch_task(&task);
                        }
                    }
                    let _ = app.emit(
                        "batch-file-error",
                        serde_json::json!({ "id": &task_id, "error": "已取消" }),
                    );
                }
                Some(BatchCmd::RetryTask(task_id)) => {
                    reset_task_to_queued(&tasks, &task_id);
                    // V2 持久化：更新 DB 中的任务状态
                    {
                        let db = app.state::<Database>();
                        if let Some(task) = tasks.lock().unwrap().iter().find(|t| t.id == task_id).cloned() {
                            let _ = db.upsert_batch_task(&task);
                        }
                    }
                    // 重新调度
                    let cfg = config.lock().unwrap().clone();
                    if cfg.schedule.is_active_now() && !*paused.lock().unwrap() {
                        let task = tasks.lock().unwrap().iter()
                            .find(|t| t.id == task_id && t.status == BatchStatus::Queued)
                            .cloned();
                        if let Some(task) = task {
                            let sem = semaphore.clone();
                            let app_clone = app.clone();
                            let config_clone = config.clone();
                            let tasks_clone = tasks.clone();
                            tokio::spawn(async move {
                                let _permit = sem.acquire().await;
                                process_task(app_clone, tasks_clone, task, config_clone).await;
                            });
                        }
                    }
                }
                Some(BatchCmd::StartTask(task_id)) => {
                    // 确保任务为 Queued 状态
                    reset_task_to_queued(&tasks, &task_id);
                    {
                        let db = app.state::<Database>();
                        if let Some(task) = tasks.lock().unwrap().iter().find(|t| t.id == task_id).cloned() {
                            let _ = db.upsert_batch_task(&task);
                        }
                    }
                    // 强制调度该任务（忽略暂停状态和工作时间段）
                    let task = tasks.lock().unwrap().iter()
                        .find(|t| t.id == task_id && t.status == BatchStatus::Queued)
                        .cloned();
                    if let Some(task) = task {
                        let sem = semaphore.clone();
                        let app_clone = app.clone();
                        let config_clone = config.clone();
                        let tasks_clone = tasks.clone();
                        tokio::spawn(async move {
                            let _permit = sem.acquire().await;
                            process_task(app_clone, tasks_clone, task, config_clone).await;
                        });
                    }
                }
                Some(BatchCmd::DeleteTask(task_id)) => {
                    // 从内存列表移除
                    tasks.lock().unwrap().retain(|t| t.id != task_id);
                    // 从 DB 删除
                    {
                        let db = app.state::<Database>();
                        let _ = db.delete_batch_task(&task_id);
                    }
                    // 通知前端移除该任务
                    let _ = app.emit(
                        "batch-task-deleted",
                        serde_json::json!({ "id": &task_id }),
                    );
                }
                Some(BatchCmd::ReorderTasks(ordered_ids)) => {
                    // 按给定的 ID 顺序重新排列 tasks
                    let mut guard = tasks.lock().unwrap();
                    let mut new_order: Vec<BatchTask> = Vec::new();
                    for id in &ordered_ids {
                        if let Some(t) = guard.iter().find(|t| &t.id == id).cloned() {
                            new_order.push(t);
                        }
                    }
                    // 追加不在 ordered_ids 中的任务（保持原顺序）
                    for t in guard.iter() {
                        if !ordered_ids.contains(&t.id) {
                            new_order.push(t.clone());
                        }
                    }
                    *guard = new_order;
                }
                Some(BatchCmd::ClearQueue) => {
                    // 清除 Queued、Failed、Skipped、Cancelled 状态的任务（保留处理中和已完成的）
                    let removed_ids: Vec<String> = tasks.lock().unwrap().iter()
                        .filter(|t| matches!(t.status,
                            BatchStatus::Queued | BatchStatus::Failed(_) |
                            BatchStatus::Skipped(_) | BatchStatus::Cancelled))
                        .map(|t| t.id.clone())
                        .collect();
                    {
                        let db = app.state::<Database>();
                        for id in &removed_ids {
                            let _ = db.delete_batch_task(id);
                        }
                    }
                    tasks.lock().unwrap().retain(|t| {
                        !matches!(t.status,
                            BatchStatus::Queued | BatchStatus::Failed(_) |
                            BatchStatus::Skipped(_) | BatchStatus::Cancelled)
                    });
                }
                Some(BatchCmd::UpdateConfig(new_config)) => {
                    *config.lock().unwrap() = new_config;
                }
                Some(BatchCmd::Pause(reason)) => {
                    *paused.lock().unwrap() = true;
                    // reason 为 None 时表示调用方已自行 emit 事件（如凭据错误）
                    if let Some(r) = reason {
                        let _ = app.emit(
                            "batch-queue-paused",
                            serde_json::json!({ "reason": r }),
                        );
                    }
                }
                Some(BatchCmd::Resume) => {
                    *paused.lock().unwrap() = false;
                    let cfg = config.lock().unwrap().clone();
                    if cfg.schedule.is_active_now() {
                        let queued: Vec<BatchTask> = tasks.lock().unwrap().iter()
                            .filter(|t| t.status == BatchStatus::Queued)
                            .cloned().collect();
                        for task in queued {
                            let sem = semaphore.clone();
                            let app_clone = app.clone();
                            let config_clone = config.clone();
                            let tasks_clone = tasks.clone();
                            tokio::spawn(async move {
                                let _permit = sem.acquire().await;
                                process_task(app_clone, tasks_clone, task, config_clone).await;
                            });
                        }
                    }
                }
                Some(BatchCmd::ScanExisting) => {
                    let cfg = config.lock().unwrap().clone();
                    let watch_paths = cfg.watch_paths.clone();
                    let recursive = cfg.watch_recursive;

                    // 重置取消标志
                    scan_cancel.store(false, std::sync::atomic::Ordering::SeqCst);

                    // 扫描所有视频和字幕文件
                    let all_files: Vec<(String, PathType)> = watch_paths.iter()
                        .flat_map(|p| scan_directory(p, recursive))
                        .collect();

                    let total = all_files.len();
                    if total == 0 {
                        let _ = app.emit("batch-scan-progress", serde_json::json!({
                            "total": 0, "done": 0, "skipped": 0, "cancelled": false,
                        }));
                        let _ = app.emit("batch-scan-done", serde_json::json!({
                            "total": 0, "done": 0, "skipped": 0, "cancelled": false,
                        }));
                        continue;
                    }

                    tracing::info!("批量翻译：扫描已有文件，共 {} 个文件", total);

                    // 发送初始进度
                    let _ = app.emit("batch-scan-progress", serde_json::json!({
                        "total": total, "done": 0, "skipped": 0, "cancelled": false,
                    }));

                    let mut done = 0usize;
                    let mut skipped_count = 0usize;
                    let mut cancelled = false;

                    for (file_path, path_type) in all_files {
                        // 检查取消标志
                        if scan_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                            cancelled = true;
                            tracing::info!("批量翻译：扫描已被用户取消，已检查 {}/{}", done, total);
                            break;
                        }

                        // 跳过已有非终态任务
                        let exists = tasks.lock().unwrap().iter().any(|t| {
                            t.video_path == file_path && !matches!(t.status,
                                BatchStatus::Done | BatchStatus::Failed(_) |
                                BatchStatus::Cancelled | BatchStatus::Skipped(_))
                        });
                        if exists {
                            done += 1;
                            let _ = app.emit("batch-scan-progress", serde_json::json!({
                                "total": total, "done": done, "skipped": skipped_count, "cancelled": false,
                            }));
                            continue;
                        }

                        // 先检查是否需要翻译，不需要则直接跳过（不创建任务、不显示在列表）
                        let skip_reason = if path_type == PathType::Subtitle {
                            // 先检查同目录下是否已有目标语言或双语字幕（对应同一视频）
                            let mut sibling_skip: Option<String> = None;
                            if cfg.check_external {
                                if let Some(video_path) = find_video_for_subtitle(&file_path) {
                                    if let Some(existing) = find_external_subtitle(&video_path, &cfg.target_lang) {
                                        if existing != file_path {
                                            sibling_skip = Some(format!("已有外挂目标语言字幕: {}", existing));
                                        }
                                    }
                                    if sibling_skip.is_none() {
                                        for lang in &cfg.skip_langs {
                                            if lang == &cfg.target_lang { continue; }
                                            if let Some(existing) = find_external_subtitle(&video_path, lang) {
                                                if existing != file_path {
                                                    sibling_skip = Some(format!("已有外挂 {} 字幕，跳过: {}", lang, existing));
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(reason) = sibling_skip {
                                Some(reason)
                            } else if !check_subtitle_needs_translate(&file_path, &cfg) {
                                Some(format!("字幕已是目标语言 {}", cfg.target_lang))
                            } else {
                                None
                            }
                        } else {
                            match check_file_needs_translate(&file_path, &cfg).await {
                                Ok(Some(reason)) => Some(reason),
                                Ok(None) => None,
                                Err(e) => {
                                    tracing::warn!("批量翻译：检查文件 {} 失败: {}", file_path, e);
                                    Some(format!("检查失败: {}", e))
                                }
                            }
                        };

                        if let Some(reason) = skip_reason {
                            skipped_count += 1;
                            tracing::debug!("批量翻译：跳过文件 {}，原因: {}", file_path, reason);
                            done += 1;
                            let _ = app.emit("batch-scan-progress", serde_json::json!({
                                "total": total, "done": done, "skipped": skipped_count, "cancelled": false,
                            }));
                            continue;
                        }

                        // 创建任务并入队
                        let task = BatchTask {
                            id: uuid::Uuid::new_v4().to_string(),
                            video_path: file_path.clone(),
                            source_path_type: path_type.clone(),
                            status: BatchStatus::Queued,
                            source_lang: cfg.source_lang.clone(),
                            target_lang: cfg.target_lang.clone(),
                            provider: cfg.provider.clone(),
                            created_at: now_ts(),
                            ..Default::default()
                        };
                        {
                            let db = app.state::<Database>();
                            let _ = db.upsert_batch_task(&task);
                        }
                        tasks.lock().unwrap().push(task.clone());
                        emit_batch_event(&app, "batch-task-added", &task);

                        done += 1;
                        let _ = app.emit("batch-scan-progress", serde_json::json!({
                            "total": total, "done": done, "skipped": skipped_count, "cancelled": false,
                        }));
                    }

                    let _ = app.emit("batch-scan-done", serde_json::json!({
                        "total": total, "done": done, "skipped": skipped_count, "cancelled": cancelled,
                    }));

                    // 扫描完成后，调度所有 Queued 任务执行翻译
                    if !cancelled && !*paused.lock().unwrap() {
                        let cfg = config.lock().unwrap().clone();
                        if cfg.schedule.is_active_now() {
                            let queued: Vec<BatchTask> = tasks.lock().unwrap().iter()
                                .filter(|t| t.status == BatchStatus::Queued)
                                .cloned().collect();
                            tracing::info!("批量翻译：扫描完成，调度 {} 个 Queued 任务", queued.len());
                            for task in queued {
                                let sem = semaphore.clone();
                                let app_clone = app.clone();
                                let config_clone = config.clone();
                                let tasks_clone = tasks.clone();
                                tokio::spawn(async move {
                                    let _permit = sem.acquire().await;
                                    process_task(app_clone, tasks_clone, task, config_clone).await;
                                });
                            }
                        }
                    }
                }
                Some(BatchCmd::Shutdown) | None => break,
            }
        }
    });
}

/// process_task 处理单个文件的全流程
async fn process_task(
    app: tauri::AppHandle,
    tasks: Arc<Mutex<Vec<BatchTask>>>,
    mut task: BatchTask,
    config: Arc<Mutex<BatchConfig>>,
) {
    let cfg = config.lock().unwrap().clone();
    let db = app.state::<Database>();
    task.started_at = Some(now_ts());
    sync_task_to_global(&tasks, &task, Some(db.inner()));

    // ── 步骤 1：probe 视频（仅 Video 类型）──
    if task.source_path_type == PathType::Video {
        update_status(&app, &tasks, &task, BatchStatus::Probing);

        // 获取 ffmpeg 路径（从下载目录）
        let ffmpeg_path = ffmpeg::find_ffmpeg(None).ok().map(|p| p.to_string_lossy().to_string());

        let probe_result = tokio::task::spawn_blocking({
            let path = task.video_path.clone();
            let ff_path = ffmpeg_path.clone();
            move || ffmpeg::probe_video(&path, ff_path.as_deref())
        }).await;

        // ffprobe 失败时降级：跳过内嵌字幕检查，但仍检查外挂字幕
        let probe = match probe_result {
            Ok(Ok(p)) => Some(p),
            Ok(Err(e)) => {
                tracing::debug!("ffprobe 失败，降级为只检查外挂字幕: {}", e);
                None
            }
            Err(e) => {
                tracing::debug!("ffprobe 任务失败，降级为只检查外挂字幕: {}", e);
                None
            }
        };

        // ── 步骤 1.5：假文件检测（仅 probe 成功时）──
        if let Some(ref probe) = probe {
            if let Some(reason) = detect_fake_video(probe, &task.video_path, &cfg) {
                return skip_task(&app, &tasks, &task, reason, Some(db.inner()));
            }
        }

        // ── 步骤 2：检查目标语言字幕 + skip_langs 检测 ──
        update_status(&app, &tasks, &task, BatchStatus::CheckingSubtitle);

        // 2a. 检查外挂字幕：target_lang 或 skip_langs 中任一语言
        if cfg.check_external {
            // 检查目标语言外挂字幕
            if let Some(existing) = find_external_subtitle(&task.video_path, &cfg.target_lang) {
                return skip_task(&app, &tasks, &task,
                    format!("已有外挂目标语言字幕: {}", existing), Some(db.inner()));
            }
            // 检查 skip_langs 外挂字幕
            for lang in &cfg.skip_langs {
                if lang == &cfg.target_lang { continue; } // 已检查过
                if let Some(existing) = find_external_subtitle(&task.video_path, lang) {
                    return skip_task(&app, &tasks, &task,
                        format!("已有外挂 {} 字幕，跳过: {}", lang, existing), Some(db.inner()));
                }
            }
        }

        // 2b. 检查内嵌字幕流（仅 probe 成功时）
        if let Some(ref probe) = probe {
            if cfg.check_embedded {
                // 检查目标语言内嵌流
                if probe.subtitle_streams.iter().any(|s| {
                    s.language.as_deref() == Some(cfg.target_lang.as_str())
                }) {
                    return skip_task(&app, &tasks, &task,
                        "已有内嵌目标语言字幕流".to_string(), Some(db.inner()));
                }
                // 检查 skip_langs 内嵌流
                for lang in &cfg.skip_langs {
                    if lang == &cfg.target_lang { continue; }
                    if probe.subtitle_streams.iter().any(|s| {
                        s.language.as_deref() == Some(lang.as_str())
                    }) {
                        return skip_task(&app, &tasks, &task,
                            format!("已有内嵌 {} 字幕流，跳过", lang), Some(db.inner()));
                    }
                }
            }
        }

        // ── 步骤 3：选流 + 提取 ──
        // ffprobe 失败时无法提取内嵌字幕，检查是否有外挂字幕可用
        let probe: Option<ffmpeg::ProbeResult> = match probe {
            Some(p) => Some(p),
            None => {
                // 尝试查找外挂源语言字幕作为输入
                let source_sub = find_external_subtitle(&task.video_path, &cfg.source_lang)
                    .or_else(|| find_any_external_subtitle(&task.video_path));
                if let Some(sub_path) = source_sub {
                    // 有外挂字幕，跳过提取步骤，直接用外挂字幕翻译
                    tracing::info!("ffprobe 不可用，使用外挂字幕: {}", sub_path);
                    task.subtitle_path = Some(sub_path);
                    None
                } else {
                    return fail_task(&app, &tasks, &task,
                        "FFmpeg 不可用且无外挂字幕", Some(db.inner()));
                }
            }
        };

        if let Some(ref probe) = probe {
            let stream_index = select_subtitle_stream(&probe.subtitle_streams);
            if stream_index.is_none() {
                return fail_task(&app, &tasks, &task, "无可提取的字幕流", Some(db.inner()));
            }

        update_status(&app, &tasks, &task, BatchStatus::Extracting(0.0));

        let tmp_format = cfg.output_formats.first().unwrap_or(&cfg.output_format);
        let tmp_path = get_batch_tmp_path(&task.id, tmp_format);
        let app_clone = app.clone();
        let task_id = task.id.clone();
        let progress_cb = Box::new(move |pct: f64| {
            // pct 是 0-100 的百分比，转为 0-1 范围以与翻译进度统一
            let ratio = (pct / 100.0).min(1.0).max(0.0);
            let _ = app_clone.emit("batch-file-progress", serde_json::json!({
                "id": &task_id,
                "stage": "extracting",
                "progress": ratio,
            }));
        });

        let extract_result = tokio::task::spawn_blocking({
            let video_path = task.video_path.clone();
            let tmp_path = tmp_path.clone();
            let duration = probe.format.duration;
            let ff_path = ffmpeg_path.clone();
            move || ffmpeg::extract_subtitle_stream(
                &video_path, stream_index.unwrap(), &tmp_path,
                ff_path.as_deref(), duration, Some(&progress_cb),
            )
        }).await;

        match extract_result {
            Ok(Ok(())) => task.subtitle_path = Some(tmp_path),
            Ok(Err(e)) => return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner())),
            Err(e) => return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner())),
        }
        } // end if let Some(ref probe)
    } else {
        // Subtitle 类型：先检查同目录下是否已有目标语言或双语字幕
        if cfg.check_external {
            if let Some(video_path) = find_video_for_subtitle(&task.video_path) {
                if let Some(existing) = find_external_subtitle(&video_path, &cfg.target_lang) {
                    if existing != task.video_path {
                        return skip_task(&app, &tasks, &task,
                            format!("已有外挂目标语言字幕: {}", existing), Some(db.inner()));
                    }
                }
                for lang in &cfg.skip_langs {
                    if lang == &cfg.target_lang { continue; }
                    if let Some(existing) = find_external_subtitle(&video_path, lang) {
                        if existing != task.video_path {
                            return skip_task(&app, &tasks, &task,
                                format!("已有外挂 {} 字幕，跳过: {}", lang, existing), Some(db.inner()));
                        }
                    }
                }
            }
        }
        // 直接用原路径
        task.subtitle_path = Some(task.video_path.clone());
    }

    // ── 步骤 4：解析字幕 ──
    update_status(&app, &tasks, &task, BatchStatus::Parsing);

    let sub_path = task.subtitle_path.clone().unwrap();
    let mut subtitle_file = match tokio::task::spawn_blocking({
        let path = sub_path.clone();
        move || subtitle::load_subtitle_file(&path)
    }).await {
        Ok(Ok(f)) => f,
        Ok(Err(e)) => return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner())),
        Err(e) => return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner())),
    };

    // 双语检测：若已是双语且含目标语言 → 跳过
    let bilingual = subtitle::detect_bilingual(&subtitle_file);
    if bilingual.is_bilingual {
        let has_target = subtitle_file.entries.iter().any(|e|
            has_target_lang_chars(&e.translated, &cfg.target_lang));
        if has_target {
            return skip_task(&app, &tasks, &task, "字幕已含目标语言译文".to_string(), Some(db.inner()));
        }
    }

    // skip_langs 内容检测：字幕内容含 skip_langs 中任一语言的字符 → 跳过
    for lang in &cfg.skip_langs {
        if lang == &cfg.target_lang { continue; } // 已在双语检测中处理
        let has_skip_lang = subtitle_file.entries.iter().any(|e|
            has_target_lang_chars(&e.translated, lang) || has_target_lang_chars(&e.text, lang));
        if has_skip_lang {
            return skip_task(&app, &tasks, &task,
                format!("字幕内容含 {} 字符，跳过", lang), Some(db.inner()));
        }
    }

    // source_langs 优先级匹配：按用户排序的优先级依次检测字幕内容，
    // 找到第一个匹配的语言作为翻译源语言；都不匹配则跳过
    let effective_source_lang: String = if !cfg.source_langs.is_empty() {
        let mut matched_lang: Option<String> = None;
        for sl in &cfg.source_langs {
            let has_lang = subtitle_file.entries.iter().any(|e|
                has_target_lang_chars(&e.text, sl));
            if has_lang {
                matched_lang = Some(sl.clone());
                break;
            }
        }
        match matched_lang {
            Some(lang) => lang,
            None => {
                return skip_task(&app, &tasks, &task,
                    format!("字幕语言不在源语言优先级列表 {:?} 中，跳过", cfg.source_langs), Some(db.inner()));
            }
        }
    } else {
        cfg.source_lang.clone()
    };

    task.total_entries = subtitle_file.entries.len();
    sync_task_to_global(&tasks, &task, Some(db.inner()));

    // ── 步骤 5：翻译 ──
    update_status(&app, &tasks, &task, BatchStatus::Translating(0.0));

    let resolved = match ipc::resolve_provider(
        db.inner(), &cfg.provider, cfg.model.as_deref(),
        cfg.model_type.as_deref(), cfg.service_id.as_deref(),
        Vec::new(), false,
    ) {
        Ok(r) => r,
        Err(e) => {
            // 凭据/配置错误：暂停整个队列，防止后续任务全部失败
            let _ = app.emit("batch-queue-paused", serde_json::json!({
                "reason": e.to_string()
            }));
            // 发送 Pause 命令到 worker，真正暂停队列调度（reason=None 避免重复 emit）
            if let Some(bq) = app.try_state::<BatchQueue>() {
                let _ = bq.tx.try_send(BatchCmd::Pause(None));
            }
            // 标记当前任务为 Failed，但不触发 check_queue_complete（队列已暂停，不应 emit "完成"）
            let err_msg = e.to_string();
            let mut failed_task = task.clone();
            failed_task.status = BatchStatus::Failed(err_msg.clone());
            failed_task.error = Some(err_msg.clone());
            failed_task.finished_at = Some(now_ts());
            {
                let mut guard = tasks.lock().unwrap();
                if let Some(t) = guard.iter_mut().find(|t| t.id == task.id) {
                    *t = failed_task.clone();
                }
            }
            let _ = db.upsert_batch_task(&failed_task);
            let _ = app.emit(
                "batch-file-error",
                serde_json::json!({ "id": &task.id, "error": &err_msg }),
            );
            if let Some(tmp) = &task.subtitle_path {
                if task.source_path_type == PathType::Video {
                    let _ = std::fs::remove_file(tmp);
                }
            }
            return;
        }
    };

    let provider_name = build_provider_name(&cfg);
    let file_hash = subtitle::compute_subtitle_hash(&subtitle_file.entries);

    let scheduler = translate::TranslateScheduler::new(db.inner(), resolved.instance, provider_name)
        .with_file_hash(file_hash)
        .with_concurrency_and_rate_limit(cfg.entry_concurrency, get_rate_limit(&cfg.provider));

    let app_clone = app.clone();
    let task_id = task.id.clone();
    let total = task.total_entries;
    let progress_cb = Box::new(move |done: usize, _total: usize| {
        let pct = if total > 0 { done as f64 / total as f64 } else { 0.0 };
        let _ = app_clone.emit("batch-file-progress", serde_json::json!({
            "id": &task_id,
            "stage": "translating",
            "progress": pct,
            "done": done,
            "total": total,
        }));
    });

    // 翻译时使用按优先级匹配到的源语言
    let result = scheduler
        .translate_entries_full(
            &subtitle_file.entries,
            &effective_source_lang, &cfg.target_lang,
            5000, Some(progress_cb), None, cfg.skip_cache,
        )
        .await;

    match result {
        Ok(translate_result) => {
            for (i, entry) in subtitle_file.entries.iter_mut().enumerate() {
                if let Some(tr) = translate_result.translations.get(i) {
                    entry.translated = tr.translated.clone();
                    entry.failed = tr.failed;
                    entry.from_cache = tr.from_cache;
                }
            }
            task.done_entries = translate_result.translations.iter().filter(|t| !t.failed).count();
            task.cached_entries = translate_result.cached_count;
            task.failed_entries = translate_result.translations.iter().filter(|t| t.failed).count();
        }
        Err(e) => return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner())),
    }

    // ── 步骤 6：输出（支持多格式）──
    update_status(&app, &tasks, &task, BatchStatus::Exporting);

    // 确定要输出的格式列表：output_formats 优先，为空则用 output_format
    let formats: Vec<subtitle::SubtitleFormat> = if cfg.output_formats.is_empty() {
        vec![cfg.output_format.clone()]
    } else {
        cfg.output_formats.clone()
    };

    let mut output_paths: Vec<String> = Vec::new();

    if cfg.embed_to_video {
        // 嵌入视频模式：只用第一个格式生成字幕，再合并到 mkv
        let format = formats.first().unwrap();
        let output_path = build_output_path(&task.video_path, &cfg, &effective_source_lang, format);
        let merge_sub_path = get_batch_tmp_path(&format!("{}_merge", task.id), format);
        let merge_opts = subtitle::ExportOptions {
            format: format.clone(),
            mode: match cfg.output_mode {
                OutputMode::Monolingual => subtitle::ExportMode::Monolingual,
                OutputMode::Bilingual => subtitle::ExportMode::Bilingual,
            },
            monolingual_lang: None,
            bilingual_translated_first: Some(true),
            ass_style: None,
            video_width: None,
            video_height: None,
        };
        let exported = subtitle::export_subtitle(&subtitle_file, &merge_opts);
        if let Err(e) = tokio::fs::write(&merge_sub_path, &exported).await {
            return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner()));
        }

        let merge_result = tokio::task::spawn_blocking({
            let video = task.video_path.clone();
            let sub = merge_sub_path.clone();
            let out = output_path.clone();
            let lang = cfg.target_lang.clone();
            let ff_path = ffmpeg::find_ffmpeg(None).ok().map(|p| p.to_string_lossy().to_string());
            move || ffmpeg::merge_subtitle_to_video(&video, &sub, Some(&out), Some(&lang), ff_path.as_deref(), None)
        }).await;
        match merge_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner())),
            Err(e) => return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner())),
        }
        let _ = tokio::fs::remove_file(&merge_sub_path).await;
        output_paths.push(output_path);
    } else {
        // 字幕文件模式：遍历所有选中的格式，每种格式生成一个文件
        for format in &formats {
            let output_path = build_output_path(&task.video_path, &cfg, &effective_source_lang, format);
            let export_opts = subtitle::ExportOptions {
                format: format.clone(),
                mode: match cfg.output_mode {
                    OutputMode::Monolingual => subtitle::ExportMode::Monolingual,
                    OutputMode::Bilingual => subtitle::ExportMode::Bilingual,
                },
                monolingual_lang: Some("translated".to_string()),
                bilingual_translated_first: Some(true),
                ass_style: None,
                video_width: None,
                video_height: None,
            };
            let exported = subtitle::export_subtitle(&subtitle_file, &export_opts);
            // 原子写入：先写 .tmp 再 rename
            let tmp_output = format!("{}.tmp", output_path);
            match tokio::fs::write(&tmp_output, exported).await {
                Ok(()) => {
                    if let Err(e) = tokio::fs::rename(&tmp_output, &output_path).await {
                        let _ = tokio::fs::remove_file(&tmp_output).await;
                        return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner()));
                    }
                }
                Err(e) => return fail_task(&app, &tasks, &task, &e.to_string(), Some(db.inner())),
            }
            output_paths.push(output_path);
        }
    }

    // 多格式输出路径用 \n 分隔存储
    task.output_path = Some(output_paths.join("\n"));
    task.status = BatchStatus::Done;
    task.finished_at = Some(now_ts());
    sync_task_to_global(&tasks, &task, Some(db.inner()));
    emit_batch_event(&app, "batch-file-done", &task);

    // 清理临时文件
    if let Some(tmp) = &task.subtitle_path {
        if task.source_path_type == PathType::Video {
            let _ = tokio::fs::remove_file(tmp).await;
        }
    }

    // 写历史记录
    let _ = db.add_history(&HistoryRecord {
        video_path: Some(task.video_path.clone()),
        subtitle_path: task.output_path.clone(),
        source_lang: Some(cfg.source_lang.clone()),
        target_lang: Some(cfg.target_lang.clone()),
        provider: Some(cfg.provider.clone()),
        action: "batch_translate".to_string(),
        status: "success".to_string(),
        detail: Some(format!(
            "total: {}, cached: {}, failed: {}",
            task.total_entries, task.cached_entries, task.failed_entries
        )),
    });

    check_queue_complete(&tasks, &app);
}

// === SECTION 5 END ===

// === SECTION 6: 单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_video_file() {
        assert!(is_video_file("test.mkv"));
        assert!(is_video_file("test.MKV"));
        assert!(is_video_file("test.mp4"));
        assert!(is_video_file("test.avi"));
        assert!(is_video_file("test.mov"));
        assert!(is_video_file("test.wmv"));
        assert!(is_video_file("test.flv"));
        // .ts 文件需要 magic bytes 验证（0x47 sync byte），不存在的文件返回 false
        assert!(!is_video_file("test.ts"));
        assert!(is_video_file("test.m2ts"));
        assert!(!is_video_file("test.srt"));
        assert!(!is_video_file("test.ass"));
        assert!(!is_video_file("test.txt"));
        assert!(!is_video_file("test"));
    }

    #[test]
    fn test_is_video_file_ts_magic_bytes() {
        // 创建一个临时文件，写入 MPEG-TS sync byte 0x47
        let tmp = std::env::temp_dir().join("test_ts_magic.ts");
        std::fs::write(&tmp, [0x47u8, 0x40, 0x00, 0x10]).unwrap();
        assert!(is_video_file(tmp.to_str().unwrap()), "MPEG-TS 文件应被识别为视频");
        std::fs::remove_file(&tmp).ok();

        // TypeScript 文件（文本内容）不应被识别为视频
        let tmp_ts = std::env::temp_dir().join("test_ts_text.ts");
        std::fs::write(&tmp_ts, b"export const x = 1;").unwrap();
        assert!(!is_video_file(tmp_ts.to_str().unwrap()), "TypeScript 文件不应被识别为视频");
        std::fs::remove_file(&tmp_ts).ok();
    }

    #[test]
    fn test_is_subtitle_file() {
        assert!(is_subtitle_file("test.srt"));
        assert!(is_subtitle_file("test.SRT"));
        assert!(is_subtitle_file("test.ass"));
        assert!(is_subtitle_file("test.ssa"));
        assert!(is_subtitle_file("test.vtt"));
        assert!(is_subtitle_file("test.sub"));
        assert!(!is_subtitle_file("test.mkv"));
        assert!(!is_subtitle_file("test.txt"));
    }

    #[test]
    fn test_schedule_always_active() {
        let s = BatchSchedule::Always;
        assert!(s.is_active_now());
    }

    #[test]
    fn test_schedule_time_window_cross_day() {
        // 跨午夜窗口：22:00-02:00
        let s = BatchSchedule::TimeWindow {
            windows: vec![(22, 2)],
            weekdays: vec![],
        };
        // 这个测试只验证逻辑不 panic，具体时间判断依赖当前系统时间
        let _ = s.is_active_now();
    }

    #[test]
    fn test_schedule_time_window_normal() {
        // 正常窗口：09:00-17:00
        let s = BatchSchedule::TimeWindow {
            windows: vec![(9, 17)],
            weekdays: vec![],
        };
        let _ = s.is_active_now();
    }

    #[test]
    fn test_build_output_path_monolingual() {
        let cfg = BatchConfig {
            output_mode: OutputMode::Monolingual,
            output_format: subtitle::SubtitleFormat::Srt,
            output_suffix: ".zh".to_string(),
            ..Default::default()
        };
        // 使用不存在的路径，验证基本文件名生成
        let out = build_output_path("Z:\\NonExistent\\test.mkv", &cfg, "en", &subtitle::SubtitleFormat::Srt);
        assert!(out.contains("test.zh.srt"), "output was: {}", out);
    }

    #[test]
    fn test_ensure_unique_path_nonexistent() {
        // 不存在的路径应原样返回
        let path = "Z:\\NonExistent\\test.zh.srt";
        assert_eq!(ensure_unique_path(path), path);
    }

    #[test]
    fn test_build_output_path_merge() {
        let cfg = BatchConfig {
            embed_to_video: true,
            ..Default::default()
        };
        let out = build_output_path("Z:\\NonExistent\\test.mkv", &cfg, "en", &subtitle::SubtitleFormat::Srt);
        assert!(out.contains("test.merged.mkv"), "output was: {}", out);
    }

    #[test]
    fn test_build_output_path_ass_format() {
        let cfg = BatchConfig {
            output_mode: OutputMode::Monolingual,
            output_format: subtitle::SubtitleFormat::Ass,
            output_suffix: ".zh".to_string(),
            ..Default::default()
        };
        let out = build_output_path("Z:\\NonExistent\\test.mkv", &cfg, "en", &subtitle::SubtitleFormat::Ass);
        assert!(out.contains("test.zh.ass"), "output was: {}", out);
    }

    #[test]
    fn test_build_output_path_bilingual() {
        let cfg = BatchConfig {
            output_mode: OutputMode::Bilingual,
            output_format: subtitle::SubtitleFormat::Srt,
            target_lang: "zh".to_string(),
            ..Default::default()
        };
        // 中英双语：test.zh-en.srt
        let out = build_output_path("Z:\\NonExistent\\test.mkv", &cfg, "en", &subtitle::SubtitleFormat::Srt);
        assert!(out.contains("test.zh-en.srt"), "output was: {}", out);

        // 中日双语：test.zh-ja.srt
        let out_ja = build_output_path("Z:\\NonExistent\\test.mkv", &cfg, "ja", &subtitle::SubtitleFormat::Srt);
        assert!(out_ja.contains("test.zh-ja.srt"), "output was: {}", out_ja);
    }

    #[test]
    fn test_has_target_lang_chars_chinese() {
        assert!(has_target_lang_chars("你好世界", "zh"));
        assert!(has_target_lang_chars("Hello 你好", "zh"));
        assert!(!has_target_lang_chars("Hello World", "zh"));
    }

    #[test]
    fn test_has_target_lang_chars_english() {
        // 非CJK语言：非空即视为含目标语言
        assert!(has_target_lang_chars("Hello World", "en"));
        assert!(!has_target_lang_chars("", "en"));
    }

    #[test]
    fn test_select_subtitle_stream_english() {
        let streams = vec![
            ffmpeg::SubtitleStream {
                index: 0,
                codec_name: "subrip".to_string(),
                codec_long_name: "SubRip subtitle".to_string(),
                duration: None,
                language: Some("eng".to_string()),
                title: None,
                disposition_default: true,
                disposition_forced: false,
                disposition_hearing_impaired: false,
                is_graphic: false,
            },
            ffmpeg::SubtitleStream {
                index: 1,
                codec_name: "subrip".to_string(),
                codec_long_name: "SubRip subtitle".to_string(),
                duration: None,
                language: Some("chi".to_string()),
                title: None,
                disposition_default: false,
                disposition_forced: false,
                disposition_hearing_impaired: false,
                is_graphic: false,
            },
        ];
        let selected = select_subtitle_stream(&streams);
        assert_eq!(selected, Some(0)); // 应选英文流
    }

    #[test]
    fn test_select_subtitle_stream_skip_graphic() {
        let streams = vec![
            ffmpeg::SubtitleStream {
                index: 0,
                codec_name: "hdmv_pgs_subtitle".to_string(),
                codec_long_name: "HDMV PGS".to_string(),
                duration: None,
                language: Some("eng".to_string()),
                title: None,
                disposition_default: true,
                disposition_forced: false,
                disposition_hearing_impaired: false,
                is_graphic: true, // 图形字幕，应跳过
            },
            ffmpeg::SubtitleStream {
                index: 1,
                codec_name: "subrip".to_string(),
                codec_long_name: "SubRip".to_string(),
                duration: None,
                language: Some("eng".to_string()),
                title: None,
                disposition_default: false,
                disposition_forced: false,
                disposition_hearing_impaired: false,
                is_graphic: false,
            },
        ];
        let selected = select_subtitle_stream(&streams);
        assert_eq!(selected, Some(1)); // 应选非图形字幕流
    }

    #[test]
    fn test_batch_config_default() {
        let cfg = BatchConfig::default();
        assert_eq!(cfg.provider, "");
        assert_eq!(cfg.file_concurrency, 1);
        assert_eq!(cfg.entry_concurrency, 3);
        assert_eq!(cfg.output_mode, OutputMode::Bilingual);
        assert_eq!(cfg.min_file_size_mb, 1);
        assert_eq!(cfg.min_duration_secs, 10.0);
    }

    #[test]
    fn test_batch_task_default() {
        let task = BatchTask::default();
        assert_eq!(task.status, BatchStatus::Queued);
        assert_eq!(task.source_path_type, PathType::Video);
        assert_eq!(task.total_entries, 0);
    }
}

// === SECTION 6 END ===




