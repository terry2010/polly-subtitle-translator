// Tauri IPC 调用封装
// 统一处理 invoke 调用 + 错误解析

import { invoke } from "@tauri-apps/api/core";
import i18n from "./i18n";
import { log, error } from "./logger";
import type {
  ProbeResult,
  SubtitleFile,
  TranslateResult,
  TranslateEntry,
  TestConnectionResult,
  LanguageInfo,
  RecentFile,
  HistoryRecord,
  SubtitleSearchResult,
  LibmpvStatus,
  InstalledPlayer,
  PlayerIcon,
  SubtitleEntry,
  IpcResponse,
  IpcError,
  BilingualDetectResult,
  SplitMode,
  ExportOptions,
  SubtitleStreamEdit,
  PromptFailLogEntry,
  BatchTask,
  BatchConfig,
} from "./ipc-types";

/// 调用 IPC 命令并解析 IpcResult 包装
/// 同步命令返回 IpcResult 结构体 { ok, value, error }
/// async 命令返回 Result<T, IpcError>，invoke 对 Err 直接 reject
async function callIpc<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    const result = await invoke<IpcResponse<T>>(cmd, args);
    // 同步命令：IpcResult 结构体
    if (result && typeof result.ok === "boolean") {
      if (result.ok) {
        return result.value as T;
      } else {
        throw (result as any)?.error ?? { code: "common.unknown", severity: "recoverable" };
      }
    }
    // async 命令直接返回值（无包装）
    log(`[callIpc] ${cmd} 返回:`, result);
    return result as T;
  } catch (e: any) {
    // async 命令 reject：e 可能是 IpcError 对象，也可能是序列化后的 JSON 字符串
    const parsed = typeof e === "string" ? safeParseIpcError(e) : e;
    error(`[callIpc] ${cmd} 错误:`, parsed);
    throw parsed;
  }
}

/// 调用 IPC 命令，允许返回 null（用于 Optional 返回类型）
async function callIpcNullable<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  try {
    const result = await invoke<IpcResponse<T>>(cmd, args);
    if (result && typeof result.ok === "boolean") {
      if (result.ok) {
        return (result.value ?? null) as T | null;
      } else {
        throw (result as any)?.error ?? { code: "common.unknown", severity: "recoverable" };
      }
    }
    return (result ?? null) as T | null;
  } catch (e: any) {
    const parsed = typeof e === "string" ? safeParseIpcError(e) : e;
    throw parsed;
  }
}

/// 将序列化的 JSON 字符串解析为 IpcError 对象，解析失败则返回通用错误
function safeParseIpcError(s: string): IpcError {
  try {
    const obj = JSON.parse(s);
    if (obj && typeof obj.code === "string") return obj as IpcError;
  } catch { /* not JSON */ }
  return { code: "common.unknown", severity: "recoverable" };
}

/// 将 IpcError 转为可读消息（用 i18n 翻译错误码）
export function formatIpcError(error: IpcError): string {
  const key = `error.${error.code}`;
  const translated = i18n.t(key, error.args ?? {});
  if (translated !== key) return translated;
  // i18n 没有找到 key，fallback：显示 code + detail（如果有）
  const detail = (error.args as any)?.detail;
  return detail ? `${error.code}: ${detail}` : error.code;
}

/// 判断 IpcError 是否为超时错误
export function isTimeoutError(error: IpcError): boolean {
  return error.code === "translate.timeout";
}

/// 判断 IpcError 是否为每日限额错误
export function isDailyLimitError(error: IpcError): boolean {
  return error.code === "translate.dailyLimitReached";
}

/// 判断 IpcError 是否为余额不足/接口未授权错误
export function isInsufficientBalanceError(error: IpcError): boolean {
  return error.code === "translate.insufficientBalance";
}

// === FFmpeg 命令 ===
export const api = {
  probeVideo: (videoPath: string, ffmpegPath?: string) =>
    callIpc<ProbeResult>("probe_video", { videoPath, ffmpegPath: ffmpegPath ?? null }),

  isDirectory: (path: string) =>
    callIpc<boolean>("is_directory", { path }),

  extractSubtitle: (videoPath: string, streamIndex: number, outputPath: string, ffmpegPath?: string, durationSec?: number) =>
    callIpc<void>("extract_subtitle", { videoPath, streamIndex, outputPath, ffmpegPath: ffmpegPath ?? null, durationSec: durationSec ?? null }),

  cancelExtractSubtitle: () =>
    callIpc<void>("cancel_extract_subtitle", {}),

  // === FFmpeg 按需下载 ===
  getFfmpegStatus: () =>
    callIpc<{ installed: boolean; source: string | null; path: string | null }>("get_ffmpeg_status_cmd"),

  downloadFfmpeg: (proxy?: string) =>
    callIpc<void>("download_ffmpeg_cmd", { proxy: proxy ?? null }),

  deleteFfmpeg: () =>
    callIpc<void>("delete_ffmpeg_cmd"),

  mergeSubtitle: (videoPath: string, subtitlePath: string, outputPath: string | null, language?: string, title?: string, ffmpegPath?: string) =>
    callIpc<void>("merge_subtitle", { videoPath, subtitlePath, outputPath, language: language ?? null, title: title ?? null, ffmpegPath: ffmpegPath ?? null }),

  checkMergeSpace: (videoPath: string) =>
    callIpc<{ video_size: number; free_space: number; enough: boolean }>("check_merge_space", { videoPath }),

  // === 字幕命令 ===
  parseSubtitleFile: (filePath: string) =>
    callIpc<SubtitleFile>("parse_subtitle_file", { filePath }),

  saveSubtitleFile: (file: SubtitleFile, outputPath: string) =>
    callIpc<void>("save_subtitle_file_cmd", { file, outputPath }),

  // === 导出弹层命令（export-dialog-plan.md §4.6） ===
  exportSubtitle: (file: SubtitleFile, outputPath: string, options: ExportOptions) =>
    callIpc<void>("export_subtitle_cmd", { file, outputPath, options }),

  // === 字幕流编辑 ===
  editSubtitleStreams: (videoPath: string, streams: SubtitleStreamEdit[], outputPath: string | null, ffmpegPath?: string) =>
    callIpc<void>("edit_subtitle_streams_cmd", { videoPath, streams, outputPath, ffmpegPath: ffmpegPath ?? null }),

  detectBilingual: (file: SubtitleFile) =>
    callIpc<BilingualDetectResult>("detect_bilingual", { file }),

  splitBilingualSubtitle: (file: SubtitleFile, splitMode: SplitMode) =>
    callIpc<SubtitleFile>("split_bilingual_subtitle", { file, splitMode }),

  // === 翻译命令 ===
  translateSubtitle: (entries: SubtitleEntry[], sourceLang: string, targetLang: string, provider: string, model?: string, modelType?: string, serviceId?: string, skipCache?: boolean, glossary?: [string, string][], nameTagging?: boolean, fileHash?: string) =>
    callIpc<TranslateResult>("translate_subtitle", { entries, sourceLang, targetLang, provider, model: model ?? null, modelType: modelType ?? null, serviceId: serviceId ?? null, skipCache: skipCache ?? null, glossary: glossary ?? null, nameTagging: nameTagging ?? null, fileHash: fileHash ?? null }),

  // 人名预扫描提取（仅 AI 翻译支持）
  extractNames: (texts: string[], sourceLang: string, targetLang: string, provider: string, model?: string, modelType?: string, serviceId?: string) =>
    callIpc<{ english: string; chinese: string; alternatives: string[] }[]>("extract_names", { texts, sourceLang, targetLang, provider, model: model ?? null, modelType: modelType ?? null, serviceId: serviceId ?? null }),

  getCachedTranslations: (entries: SubtitleEntry[], sourceLang: string, targetLang: string, provider: string, serviceId?: string, model?: string, fileHash?: string) =>
    callIpc<TranslateEntry[]>("get_cached_translations", { entries, sourceLang, targetLang, provider, serviceId: serviceId ?? null, model: model ?? null, fileHash: fileHash ?? null }),

  // === 原文编辑缓存 ===
  getSourceEdits: (fileHash: string) =>
    callIpc<{ entry_index: number; corrected_text: string; pre_edit_text: string }[]>("get_source_edits", { fileHash }),

  saveSourceEdit: (entryIndex: number, correctedText: string, preEditText: string, originalFileHash: string) =>
    callIpc<void>("save_source_edit", { entryIndex, correctedText, preEditText, originalFileHash }),

  deleteSourceEdit: (entryIndex: number, originalFileHash: string) =>
    callIpc<number>("delete_source_edit", { entryIndex, originalFileHash }),

  replaceSourceEdits: (fileHash: string, edits: [number, string, string][]) =>
    callIpc<void>("replace_source_edits", { fileHash, edits }),

  cancelTranslate: () =>
    callIpc<void>("cancel_translate"),

  onTranslateProgress: async (callback: (progress: number, total: number, done: boolean) => void) => {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<{ progress: number; total: number; done: boolean }>("translate-progress", (event) => {
      callback(event.payload.progress, event.payload.total, event.payload.done);
    });
  },

  onExtractNamesProgress: async (callback: (progress: number, total: number, done: boolean) => void) => {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<{ progress: number; total: number; done: boolean }>("extract-names-progress", (event) => {
      callback(event.payload.progress, event.payload.total, event.payload.done);
    });
  },

  onTranslateEntryDone: async (callback: (entry: { index: number; original: string; translated: string; from_cache: boolean; failed: boolean; pre_edit_text: string | null }) => void) => {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<{ index: number; original: string; translated: string; from_cache: boolean; failed: boolean; pre_edit_text: string | null }>("translate-entry-done", (event) => {
      callback(event.payload);
    });
  },

  testTranslateConnection: (
    provider: string,
    appId?: string,
    secretKey?: string | null,
    region?: string,
    baseUrl?: string,
    model?: string,
    modelType?: string,
    serviceId?: string,
    useProxy?: boolean | null,
  ) =>
    callIpc<TestConnectionResult>("test_translate_connection", {
      provider,
      appId: appId ?? null,
      secretKey: secretKey ?? null,
      region: region ?? null,
      baseUrl: baseUrl ?? null,
      model: model ?? null,
      modelType: modelType ?? null,
      serviceId: serviceId ?? null,
      useProxyOverride: useProxy ?? null,
    }),

  listOpenaiModels: (baseUrl: string, apiKey?: string, serviceId?: string, useProxy?: boolean | null) =>
    callIpc<string[]>("list_openai_models", { baseUrl, apiKey: apiKey ?? null, serviceId: serviceId ?? null, useProxyOverride: useProxy ?? null }),

  getSupportedTargetLangs: (provider: string) =>
    callIpc<LanguageInfo[]>("get_supported_target_langs", { provider }),

  // === 配置命令 ===
  getConfig: (key: string) =>
    callIpcNullable<string>("get_config", { key }),

  setConfig: (key: string, value: string) =>
    callIpc<void>("set_config", { key, value }),

  getAllConfig: () =>
    callIpc<[string, string][]>("get_all_config"),

  clearTranslateCache: () =>
    callIpc<number>("clear_translate_cache"),

  // === 代理配置 ===
  setProxy: (mode: string, host: string, port: string, username?: string, password?: string) =>
    callIpc<void>("set_proxy", { mode, host, port, username: username ?? null, password: password ?? null }),

  getProxy: () =>
    callIpc<{ mode: string; host: string; port: string; username: string; hasPassword: boolean }>("get_proxy"),

  getTranslateUseProxy: (provider: string) =>
    callIpc<boolean | null>("get_translate_use_proxy", { provider }),

  setTranslateUseProxy: (provider: string, value: boolean | null) =>
    callIpc<void>("set_translate_use_proxy", { provider, value }),

  testProxy: (url: string) =>
    callIpc<{ success: boolean; status: number; elapsed_ms: number; url: string }>("test_proxy", { url }),

  // === 系统语言探测 ===
  getSystemLang: () =>
    callIpc<string>("get_system_lang"),

  // === 获取工作区（排除任务栏），物理像素 ===
  getWorkArea: () =>
    callIpc<{ x: number; y: number; width: number; height: number }>("get_work_area"),

  // === DevTools 控制（开发者模式）===
  toggleDevtools: (open: boolean) =>
    callIpc<void>("toggle_devtools", { open }),

  // === 凭据命令 ===
  saveCredential: (provider: string, key: string, value: string) =>
    callIpc<void>("save_credential", { provider, key, value }),

  getCredential: (provider: string, key: string, reason?: string) =>
    callIpcNullable<string>("get_credential", { provider, key, reason: reason ?? i18n.t("common.unspecified") }),

  deleteCredential: (provider: string, key: string) =>
    callIpc<void>("delete_credential", { provider, key }),

  // === 最近文件 ===
  getRecentFiles: (fileType?: string) =>
    callIpc<RecentFile[]>("get_recent_files", { fileType: fileType ?? null }),

  addRecentFile: (filePath: string, fileType: string) =>
    callIpc<void>("add_recent_file", { filePath, fileType }),

  // === 历史 ===
  getHistory: (limit?: number) =>
    callIpc<HistoryRecord[]>("get_history", { limit: limit ?? null }),

  addHistoryRecord: (record: HistoryRecord) =>
    callIpc<number>("add_history_record", { record }),

  // === 搜索 ===
  searchSubtitlesOnline: (query: string, language: string, apiKey: string, source?: string) =>
    callIpc<SubtitleSearchResult[]>("search_subtitles_online", { query, language, apiKey, source }),

  searchSubtitlesWithCaptcha: (query: string, source: string, captcha: string, sessionCookie: string, verifyPath: string) =>
    callIpc<SubtitleSearchResult[]>("search_subtitles_with_captcha", { query, source, captcha, sessionCookie, verifyPath }),

  downloadSubtitleOnline: (subtitleId: string, apiKey: string, outputPath: string) =>
    callIpc<void>("download_subtitle_online", { subtitleId, apiKey, outputPath }),

  simplifySearchKeyword: (filename: string) =>
    callIpc<string>("simplify_search_keyword", { filename }),

  // === 右键菜单 ===
  registerVideoMenu: (exePath: string) =>
    callIpc<void>("register_video_menu", { exePath }),

  unregisterVideoMenu: () =>
    callIpc<void>("unregister_video_menu"),

  registerSubtitleMenu: (exePath: string) =>
    callIpc<void>("register_subtitle_menu", { exePath }),

  unregisterSubtitleMenu: () =>
    callIpc<void>("unregister_subtitle_menu"),

  isVideoMenuRegistered: () =>
    callIpc<boolean>("is_video_menu_registered"),

  isSubtitleMenuRegistered: () =>
    callIpc<boolean>("is_subtitle_menu_registered"),

  // === 播放器 ===
  getLibmpvStatus: () =>
    callIpc<LibmpvStatus>("get_libmpv_status_cmd"),

  downloadLibmpv: (proxy?: string) =>
    callIpc<void>("download_libmpv_cmd", { proxy: proxy ?? null }),

  deleteLibmpv: () =>
    callIpc<void>("delete_libmpv_cmd"),

  openInSystemPlayer: (videoPath: string) =>
    callIpc<void>("open_in_system_player_cmd", { videoPath }),

  listInstalledPlayers: (videoPath: string) =>
    callIpc<InstalledPlayer[]>("list_installed_players_cmd", { videoPath }),

  openWithPlayer: (exePath: string, videoPath: string) =>
    callIpc<void>("open_with_player_cmd", { exePath, videoPath }),

  revealInExplorer: (filePath: string) =>
    callIpc<void>("reveal_in_explorer_cmd", { filePath }),

  openPath: (path: string) =>
    callIpc<void>("open_path_cmd", { path }),

  getCrashLogDir: () =>
    callIpc<string>("get_crash_log_dir_cmd"),

  clearCrashLogs: () =>
    callIpc<number>("clear_crash_logs_cmd"),

  getPromptFailDir: () =>
    callIpc<string>("get_prompt_fail_dir_cmd"),

  listPromptFailLogs: () =>
    callIpc<PromptFailLogEntry[]>("list_prompt_fail_logs_cmd"),

  readPromptFailLog: (name: string) =>
    callIpc<string>("read_prompt_fail_log_cmd", { name }),

  deletePromptFailLog: (name: string) =>
    callIpc<void>("delete_prompt_fail_log_cmd", { name }),

  clearPromptFailLogs: () =>
    callIpc<number>("clear_prompt_fail_logs_cmd"),

  setDevMode: (enabled: boolean) =>
    callIpc<void>("set_dev_mode_cmd", { enabled }),

  setLogApiEnabled: (enabled: boolean) =>
    callIpc<void>("set_log_api_enabled_cmd", { enabled }),

  getApiDebugDir: () =>
    callIpc<string>("get_api_debug_dir_cmd"),

  listApiDebugLogs: () =>
    callIpc<PromptFailLogEntry[]>("list_api_debug_logs_cmd"),

  clearApiDebugLogs: () =>
    callIpc<number>("clear_api_debug_logs_cmd"),

  extractPlayerIcons: (videoPath: string) =>
    callIpc<PlayerIcon[]>("extract_player_icons_cmd", { videoPath }),

  clearPlayerIconsCache: () =>
    callIpc<number>("clear_player_icons_cache_cmd"),

  // === libmpv 内嵌播放命令 ===
  // player_* 命令返回 Result<T, ()>，invoke 对 Err 直接 reject
  playerInit: (dllPath: string, x: number, y: number, w: number, h: number) =>
    invoke<void>("player_init", { dllPath, x, y, w, h }),

  playerLoad: (filePath: string) =>
    invoke<void>("player_load_cmd", { filePath }),

  playerPlay: () =>
    invoke<void>("player_play_cmd"),

  playerPause: () =>
    invoke<void>("player_pause_cmd"),

  playerSeek: (timeSec: number) =>
    invoke<void>("player_seek_cmd", { timeSec }),

  playerSetVolume: (volume: number) =>
    invoke<void>("player_set_volume_cmd", { volume }),

  playerSetSpeed: (speed: number) =>
    invoke<void>("player_set_speed_cmd", { speed }),

  playerSetAudioTrack: (audioId: number) =>
    invoke<void>("player_set_audio_track_cmd", { audioId }),

  playerGetPosition: () =>
    invoke<[number, number]>("player_get_position_cmd"),

  devLog: (msg: string) =>
    invoke<void>("dev_log_cmd", { msg }).catch(() => {}),

  setSpaceDisabled: (disabled: boolean) =>
    invoke<void>("set_space_disabled_cmd", { disabled }).catch(() => {}),

  playerResize: (x: number, y: number, w: number, h: number) =>
    invoke<void>("player_resize_cmd", { x, y, w, h }),

  playerShow: () =>
    invoke<void>("player_show_cmd"),

  playerHide: () =>
    invoke<void>("player_hide_cmd"),

  isCursorInWindow: () =>
    invoke<boolean>("is_cursor_in_window_cmd"),

  playerDestroy: () =>
    invoke<void>("player_destroy_cmd"),

  // === 自动更新 ===
  checkForUpdate: () =>
    callIpc<{ available: boolean; version: string; notes: string; pub_date: string }>("check_for_update"),

  downloadAndInstallUpdate: () =>
    callIpc<void>("download_and_install_update"),

  // === 批量翻译 ===
  getBatchStatus: () =>
    callIpc<BatchTask[]>("get_batch_status"),

  batchTranslateFiles: (paths: string[], config?: BatchConfig) =>
    callIpc<string[]>("batch_translate_files", { paths, config: config ?? null }),

  startFolderWatch: (paths: string[], recursive: boolean, config?: BatchConfig) =>
    callIpc<void>("start_folder_watch", { paths, recursive, config: config ?? null }),

  stopFolderWatch: () =>
    callIpc<void>("stop_folder_watch"),

  scanExistingFiles: (paths?: string[], recursive?: boolean) =>
    callIpc<void>("scan_existing_files", { paths: paths ?? null, recursive: recursive ?? null }),

  cancelScan: () =>
    callIpc<void>("cancel_scan"),

  addFilesToQueue: (files: string[]) =>
    callIpc<number>("add_files_to_queue", { files }),

  cancelBatchTask: (taskId: string) =>
    callIpc<void>("cancel_batch_task", { taskId }),

  deleteBatchTask: (taskId: string) =>
    callIpc<void>("delete_batch_task", { taskId }),

  startBatchTask: (taskId: string) =>
    callIpc<void>("start_batch_task", { taskId }),

  reorderBatchTasks: (taskIds: string[]) =>
    callIpc<void>("reorder_batch_tasks", { taskIds }),

  retryBatchTask: (taskId: string) =>
    callIpc<void>("retry_batch_task", { taskId }),

  clearBatchQueue: () =>
    callIpc<void>("clear_batch_queue"),

  pauseBatchQueue: () =>
    callIpc<void>("pause_batch_queue"),

  resumeBatchQueue: () =>
    callIpc<void>("resume_batch_queue"),

  saveBatchConfig: (config: BatchConfig) =>
    callIpc<void>("save_batch_config", { config }),

  getBatchConfig: () =>
    callIpc<BatchConfig>("get_batch_config"),

  // === 文件夹右键菜单 ===
  registerFolderMenu: (exePath: string) =>
    callIpc<void>("register_folder_menu", { exePath }),

  unregisterFolderMenu: () =>
    callIpc<void>("unregister_folder_menu"),

  isFolderMenuRegistered: () =>
    callIpc<boolean>("is_folder_menu_registered"),
};
