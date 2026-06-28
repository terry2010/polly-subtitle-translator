// Tauri IPC 调用封装
// 统一处理 invoke 调用 + 错误解析

import { invoke } from "@tauri-apps/api/core";
import i18n from "./i18n";
import type {
  ProbeResult,
  SubtitleFile,
  TranslateResult,
  TranslateEntry,
  LanguageInfo,
  RecentFile,
  HistoryRecord,
  SubtitleSearchResult,
  LibmpvStatus,
  SubtitleEntry,
  IpcResponse,
  IpcError,
  BilingualDetectResult,
  SplitMode,
  ExportOptions,
  SubtitleStreamEdit,
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
    console.log(`[callIpc] ${cmd} 返回:`, result);
    return result as T;
  } catch (e: any) {
    // async 命令 reject：e 是序列化后的 IpcError 对象
    console.error(`[callIpc] ${cmd} 错误:`, e);
    throw e;
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
    throw e;
  }
}

/// 将 IpcError 转为可读消息（用 i18n 翻译错误码）
export function formatIpcError(error: IpcError): string {
  const key = `error.${error.code}`;
  const translated = i18n.t(key, error.args ?? {});
  // 如果 i18n 没有找到 key，t() 返回 key 本身；此时 fallback 到 code
  return translated === key ? error.code : translated;
}

// === FFmpeg 命令 ===
export const api = {
  probeVideo: (videoPath: string, ffmpegPath?: string) =>
    callIpc<ProbeResult>("probe_video", { videoPath, ffmpegPath: ffmpegPath ?? null }),

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
  translateSubtitle: (entries: SubtitleEntry[], sourceLang: string, targetLang: string, provider: string) =>
    callIpc<TranslateResult>("translate_subtitle", { entries, sourceLang, targetLang, provider }),

  getCachedTranslations: (entries: SubtitleEntry[], sourceLang: string, targetLang: string, provider: string) =>
    callIpc<TranslateEntry[]>("get_cached_translations", { entries, sourceLang, targetLang, provider }),

  cancelTranslate: () =>
    callIpc<void>("cancel_translate"),

  onTranslateProgress: async (callback: (progress: number, total: number, done: boolean) => void) => {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<{ progress: number; total: number; done: boolean }>("translate-progress", (event) => {
      callback(event.payload.progress, event.payload.total, event.payload.done);
    });
  },

  onTranslateEntryDone: async (callback: (entry: { index: number; original: string; translated: string; from_cache: boolean }) => void) => {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<{ index: number; original: string; translated: string; from_cache: boolean }>("translate-entry-done", (event) => {
      callback(event.payload);
    });
  },

  testTranslateConnection: (provider: string, appId?: string, secretKey?: string, region?: string) =>
    callIpc<void>("test_translate_connection", { provider, appId: appId ?? null, secretKey: secretKey ?? null, region: region ?? null }),

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

  // === 系统语言探测 ===
  getSystemLang: () =>
    callIpc<string>("get_system_lang"),

  // === DevTools 控制（开发者模式）===
  toggleDevtools: (open: boolean) =>
    callIpc<void>("toggle_devtools", { open }),

  // === 凭据命令 ===
  saveCredential: (provider: string, key: string, value: string) =>
    callIpc<void>("save_credential", { provider, key, value }),

  getCredential: (provider: string, key: string) =>
    callIpcNullable<string>("get_credential", { provider, key }),

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
  searchSubtitlesOnline: (query: string, language: string, apiKey: string) =>
    callIpc<SubtitleSearchResult[]>("search_subtitles_online", { query, language, apiKey }),

  downloadSubtitleOnline: (subtitleId: string, apiKey: string, outputPath: string) =>
    callIpc<void>("download_subtitle_online", { subtitleId, apiKey, outputPath }),

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

  playerResize: (x: number, y: number, w: number, h: number) =>
    invoke<void>("player_resize_cmd", { x, y, w, h }),

  playerShow: () =>
    invoke<void>("player_show_cmd"),

  playerHide: () =>
    invoke<void>("player_hide_cmd"),

  playerDestroy: () =>
    invoke<void>("player_destroy_cmd"),
};
