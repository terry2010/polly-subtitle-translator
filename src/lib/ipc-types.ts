// Tauri IPC 类型定义（与后端 Rust 结构体对应）

// === FFmpeg 相关 ===
export interface HdrInfo {
  is_hdr: boolean;
  is_dolby_vision: boolean;
  hdr_format: string;
  details: string;
}

export interface VideoStream {
  index: number;
  codec_name: string;
  codec_long_name: string;
  profile: string | null;
  width: number;
  height: number;
  pix_fmt: string;
  r_frame_rate: string;
  avg_frame_rate: string;
  duration: number | null;
  bit_rate: number | null;
  bits_per_raw_sample: string | null;
  color_space: string | null;
  color_transfer: string | null;
  color_primaries: string | null;
  hdr_info: HdrInfo | null;
}

export interface AudioStream {
  index: number;
  codec_name: string;
  codec_long_name: string;
  sample_rate: number;
  channels: number;
  channel_layout: string | null;
  duration: number | null;
  bit_rate: number | null;
  language: string | null;
  title: string | null;
  disposition_default: boolean;
}

export interface SubtitleStream {
  index: number;
  codec_name: string;
  codec_long_name: string;
  duration: number | null;
  language: string | null;
  title: string | null;
  disposition_default: boolean;
  disposition_forced: boolean;
  disposition_hearing_impaired: boolean;
  is_graphic: boolean;
}

export interface VideoFormat {
  format_name: string;
  format_long_name: string;
  duration: number | null;
  size: number | null;
  bit_rate: number | null;
}

export interface ProbeResult {
  video_path: string;
  format: VideoFormat;
  video_stream: VideoStream | null;
  audio_streams: AudioStream[];
  subtitle_streams: SubtitleStream[];
}

// === 字幕相关 ===
export type SubtitleFormat = "srt" | "vtt" | "ass" | "ssa";

// === 字幕流编辑 ===
export interface SubtitleStreamEdit {
  original_index: number;   // 原始视频中的绝对流索引
  title: string | null;     // 新标题（null=保留原标题）
  language: string | null;  // 新语言代码（null=保留原语言）
}

export interface SubtitleEntry {
  index: number;
  start_ms: number;
  end_ms: number;
  text: string;
  translated: string;
  style: string | null;
  _deleted?: boolean;
  /** 翻译是否失败（仅内存状态，不写入字幕文件） */
  failed?: boolean;
  /** 译文是否来自缓存（仅内存状态，用于统计显示） */
  from_cache?: boolean;
}

export interface SubtitleFile {
  format: SubtitleFormat;
  entries: SubtitleEntry[];
  raw_header: string | null;
  source_path: string | null;
  /** 字幕内容 hash（sha256），用于翻译缓存隔离 */
  file_hash?: string;
}

// === 双语字幕检测 ===
export type SplitMode = "even_first" | "odd_first";

export interface BilingualDetectResult {
  is_bilingual: boolean;
  split_mode: SplitMode;
  lang_a: string;
  lang_b: string;
  matched_count: number;
  total_count: number;
}

// === 翻译相关 ===
export type TranslateProviderType = "baidu" | "bing" | "google";

export interface LanguageInfo {
  code: string;
  name: string;
  native_name: string;
}

export interface TranslateEntry {
  index: number;
  original: string;
  translated: string;
  from_cache: boolean;
  failed: boolean;
}

/** token 用量统计（仅 AI 翻译有值，传统翻译为 undefined） */
export interface TokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

export interface TranslateResult {
  translations: TranslateEntry[];
  provider: string;
  cached_count: number;
  /** token 用量（仅 AI 翻译有值，传统翻译为 undefined） */
  token_usage?: TokenUsage;
}

export interface TestConnectionResult {
  original: string | null;
  translated: string | null;
}

// === 搜索相关 ===
export interface SubtitleSearchResult {
  file_name: string;
  language: string;
  download_count: number;
  rating: number;
  release_info: string;
  subtitle_id: string;
}

// === 数据库相关 ===
export interface RecentFile {
  id: number;
  file_path: string;
  file_type: string;
  opened_at: string;
}

export interface HistoryRecord {
  video_path: string | null;
  subtitle_path: string | null;
  source_lang: string | null;
  target_lang: string | null;
  provider: string | null;
  action: string;
  status: string;
  detail: string | null;
}

// === 播放器相关 ===
export interface LibmpvStatus {
  downloaded: boolean;
  path: string | null;
  version: string | null;
}

// === 已安装播放器（右键菜单"用播放器打开"用） ===
export interface InstalledPlayer {
  name: string;
  exe_path: string;
  is_default: boolean;
}

// === 播放器图标（前端用 convertFileSrc 加载） ===
export interface PlayerIcon {
  exe_path: string;
  icon_path: string;
}

// === Prompt 失败日志 ===
export interface PromptFailLogEntry {
  name: string;
  size: number;
  modified: number;
}

// === IPC 错误 ===
export type Severity = "recoverable" | "restart" | "reinstall";

export interface IpcError {
  code: string;
  args?: Record<string, unknown>;
  severity: Severity;
}

export interface IpcResultOk<T> {
  ok: true;
  value?: T;
}

export interface IpcResultErr {
  ok: false;
  error?: IpcError;
}

export type IpcResponse<T> = IpcResultOk<T> | IpcResultErr;

// === 导出弹层（export-dialog-plan.md §2） ===

export interface ExportOptions {
  format: "srt" | "ass" | "vtt";
  mode: "monolingual" | "bilingual";
  /** 单语模式：输出哪种语言 */
  monolingual_lang?: "source" | "translated";
  /** 双语模式：true=译文在上，false=原文在上 */
  bilingual_translated_first?: boolean;
  /** ASS 双语样式（仅 format=ass 且 mode=bilingual 时生效） */
  ass_style?: AssBilingualStyle;
  /** 视频实际宽度（像素），用于 ASS PlayResX，缺省 1280 */
  video_width?: number;
  /** 视频实际高度（像素），用于 ASS PlayResY，缺省 720 */
  video_height?: number;
}

export interface AssBilingualStyle {
  /** 第一行（上）字号，默认 24 */
  primary_font_size: number;
  /** 第二行（下）字号，默认 18 */
  secondary_font_size: number;
  /** 第一行颜色，ASS BGR 格式 &HBBGGRR&，默认 &HFFFFFF&（白色） */
  primary_color: string;
  /** 第二行颜色，默认 &HCCCCCC&（浅灰） */
  secondary_color: string;
  /** 第一行特效 */
  primary_bold: boolean;
  primary_italic: boolean;
  primary_underline: boolean;
  /** 第二行特效 */
  secondary_bold: boolean;
  secondary_italic: boolean;
  secondary_underline: boolean;
  /** 描边宽度，默认 2 */
  outline: number;
  /** 描边颜色，ASS BGR 格式 &HBBGGRR&，默认 &H000000&（黑色） */
  outline_color: string;
  /** 阴影深度，默认 1 */
  shadow: number;
  /** 阴影颜色，ASS BGR 格式 &HBBGGRR&，默认 &H000000&（黑色） */
  shadow_color: string;
}

// === 批量翻译类型 ===

export type BatchStatus =
  | "Queued"
  | "Probing"
  | "CheckingSubtitle"
  | { Extracting: number }
  | "Parsing"
  | { Translating: number }
  | "Exporting"
  | "Done"
  | { Skipped: string }
  | { Failed: string }
  | "Cancelled";

export type PathType = "Video" | "Subtitle";

export interface BatchTask {
  id: string;
  video_path: string;
  source_path_type: PathType;
  status: BatchStatus;
  subtitle_path: string | null;
  output_path: string | null;
  source_lang: string;
  target_lang: string;
  provider: string;
  total_entries: number;
  done_entries: number;
  cached_entries: number;
  failed_entries: number;
  created_at: number;
  started_at: number | null;
  finished_at: number | null;
  error: string | null;
}

export type OutputMode = "Monolingual" | "Bilingual";

export type BatchSchedule =
  | "Always"
  | { TimeWindow: { windows: [number, number][]; weekdays: number[] } };

export interface BatchConfig {
  source_lang: string;
  /** 源语言多选（仅作过滤，字幕语言不在列表中则跳过；空列表 = 不过滤） */
  source_langs?: string[];
  target_lang: string;
  /** 不翻译的语言列表（检测到这些语言则跳过，外挂+内嵌+内容三处检测） */
  skip_langs?: string[];
  provider: string;
  model?: string | null;
  model_type?: string | null;
  service_id?: string | null;
  file_concurrency: number;
  entry_concurrency: number;
  output_mode: OutputMode;
  /** 输出格式多选（选多种则一次生成多个不同格式字幕文件） */
  output_formats?: Array<"srt" | "vtt" | "ass" | "ssa">;
  /** @deprecated 已废弃，由 output_formats 取代 */
  output_format: "srt" | "vtt" | "ass" | "ssa";
  /** 嵌入视频（将字幕合并到 mkv 文件中） */
  embed_to_video?: boolean;
  /** @deprecated 已废弃，根据输出模式自动选择后缀 */
  output_suffix?: string;
  check_external: boolean;
  check_embedded: boolean;
  watch_paths: string[];
  watch_recursive: boolean;
  scan_on_start: boolean;
  schedule: BatchSchedule;
  min_file_size_mb: number;
  min_duration_secs: number;
  skip_cache: boolean;
  debounce_secs: number;
}

/** TS 端默认值常量，与 Rust 端 `impl Default for AssBilingualStyle` 对齐 */
export const DEFAULT_ASS_STYLE: AssBilingualStyle = {
  primary_font_size: 48,
  secondary_font_size: 30,
  primary_color: "&HFFFFFF&",
  secondary_color: "&HCCCCCC&",
  primary_bold: false,
  primary_italic: false,
  primary_underline: false,
  secondary_bold: false,
  secondary_italic: false,
  secondary_underline: false,
  outline: 2,
  outline_color: "&H000000&",
  shadow: 1,
  shadow_color: "&H000000&",
};
