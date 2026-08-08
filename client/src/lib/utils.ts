import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import type { ExportOptions } from "./ipc-types";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatTime(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const millis = Math.floor(ms % 1000);
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
}

export function formatDuration(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}

/**
 * 模块级 UI 状态（不触发 re-render，用 ref 语义跨组件共享）。
 * 用于：原生 <select> 展开时暂停字幕自动滚动，避免滚动动画导致下拉菜单被浏览器收起。
 */
export const uiState = {
  /** 是否有下拉框（select）当前处于展开态 */
  selectOpen: false,
  /** 鼠标是否悬停在字幕编辑器区域内（SubtitlePreviewPanel 设置）。
   *  VideoPlayer 据此判断空格键是否触发播放/暂停：在编辑区内不响应，避免影响文本编辑。 */
  mouseInSubtitleEditor: false,
};

/**
 * 原生文件对话框（open/save）弹出前隐藏 libmpv 子窗口，避免悬浮窗口遮挡对话框；
 * 对话框关闭后恢复显示。playerHide/playerShow 在播放器未初始化时会返回 Err，
 * 这里 catch 掉即可，不影响无播放器场景。
 *
 * 抽到 utils 层供所有组件复用（MainView 的 open、SubtitleListPanel/PreviewPanel/SearchDialog
 * 的 save 都用同一份逻辑）。
 */
export async function withPlayerHidden<T>(fn: () => Promise<T>): Promise<T> {
  // 延迟导入 api，避免 utils.ts 与 api.ts 之间形成静态循环依赖
  const { api } = await import("./api");
  try { await api.playerHide(); } catch { /* 播放器未初始化，忽略 */ }
  try {
    return await fn();
  } finally {
    try { await api.playerShow(); } catch { /* 播放器未初始化，忽略 */ }
  }
}

// === 导出弹层工具函数（export-dialog-plan.md §3.5/§5.1.4） ===

/** API 语言代码 → 文件名短代码（ISO 639-1 → ISO 639-3/B 简写） */
const LANG_CODE_MAP: Record<string, string> = {
  "zh": "zhs", "zh-Hans": "zhs", "zh-Hant": "zht",
  "en": "eng", "ja": "jpn", "ko": "kor",
  "fr": "fre", "de": "ger", "es": "spa", "ru": "rus",
};

export function toFileLangCode(apiCode: string): string {
  return LANG_CODE_MAP[apiCode] ?? apiCode.toLowerCase();
}

/** API 语言代码 → 中文名（用于字幕流 title metadata） */
const LANG_NATIVE_NAME: Record<string, string> = {
  "zh": "中文", "zh-Hans": "中文简体", "zh-Hant": "中文繁体",
  "en": "English", "ja": "日本語", "ko": "한국어",
  "fr": "Français", "de": "Deutsch", "es": "Español", "ru": "Русский",
};

/** 生成合并到视频的字幕流 title（播放器字幕菜单显示用） */
export function buildSubtitleTitle(
  options: ExportOptions,
  sourceLang: string,
  targetLang: string,
): string {
  const nameOf = (code: string) => LANG_NATIVE_NAME[code] ?? code;
  if (options.mode === "monolingual") {
    return options.monolingual_lang === "source"
      ? nameOf(sourceLang)
      : nameOf(targetLang);
  }
  // 双语：第一语言-第二语言
  const first = options.bilingual_translated_first ? targetLang : sourceLang;
  const second = options.bilingual_translated_first ? sourceLang : targetLang;
  return `${nameOf(first)}${nameOf(second)}双语`;
}

/** 从文件路径提取去扩展名的文件名（不含目录） */
export function stripExt(filePath: string): string {
  const name = filePath.split(/[\\/]/).pop() ?? filePath;
  return name.replace(/\.[^.]+$/, "");
}

/** 从文件路径提取所在目录（含末尾分隔符），无目录返回空串 */
export function fileDir(filePath: string): string {
  const idx = Math.max(filePath.lastIndexOf("\\"), filePath.lastIndexOf("/"));
  return idx >= 0 ? filePath.slice(0, idx + 1) : "";
}

/** 生成导出默认文件名（纯文件名，不含目录） */
export function buildExportFileName(
  options: ExportOptions,
  sourceLang: string,
  targetLang: string,
  baseName?: string,
): string {
  const base = baseName ?? "subtitle";
  const ext = options.format;
  const langPart = options.mode === "monolingual"
    ? toFileLangCode(options.monolingual_lang === "source" ? sourceLang : targetLang)
    : options.bilingual_translated_first
      ? `${toFileLangCode(targetLang)}-${toFileLangCode(sourceLang)}`
      : `${toFileLangCode(sourceLang)}-${toFileLangCode(targetLang)}`;
  return `${base}.${langPart}.${ext}`;
}

/**
 * 生成导出默认完整路径（目录 + 文件名）。
 * 目录优先取视频所在目录，其次字幕所在目录，都没有则返回纯文件名（由 Tauri save 决定目录）。
 */
export function buildExportFilePath(
  videoPath: string | null,
  subtitlePath: string | null,
  options: ExportOptions,
  sourceLang: string,
  targetLang: string,
): string {
  const baseName = videoPath ? stripExt(videoPath)
    : subtitlePath ? stripExt(subtitlePath)
    : "subtitle";
  const fileName = buildExportFileName(options, sourceLang, targetLang, baseName);
  const dir = videoPath ? fileDir(videoPath)
    : subtitlePath ? fileDir(subtitlePath)
    : "";
  return dir ? `${dir}${fileName}` : fileName;
}

/** ASS 颜色 &HBBGGRR& → CSS #RRGGBB */
export function assColorToCss(assColor: string): string {
  const m = assColor.match(/&H([0-9A-Fa-f]{6})&/);
  if (!m) return "#ffffff";
  const bgr = m[1];
  const b = bgr.slice(0, 2), g = bgr.slice(2, 4), r = bgr.slice(4, 6);
  return `#${r}${g}${b}`;
}

/** CSS #RRGGBB → ASS 颜色 &HBBGGRR& */
export function hexToAssColor(hex: string): string {
  const r = hex.slice(1, 3), g = hex.slice(3, 5), b = hex.slice(5, 7);
  return `&H${b}${g}${r}&`;
}
