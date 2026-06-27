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

export interface SubtitleEntry {
  index: number;
  start_ms: number;
  end_ms: number;
  text: string;
  translated: string;
  style: string | null;
  _deleted?: boolean;
}

export interface SubtitleFile {
  format: SubtitleFormat;
  entries: SubtitleEntry[];
  raw_header: string | null;
  source_path: string | null;
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

export interface TranslateResult {
  translations: TranslateEntry[];
  provider: string;
  cached_count: number;
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

// === IPC 错误 ===
export type Severity = "recoverable" | "restart" | "reinstall";

export interface IpcError {
  code: string;
  i18n_key: string;
  args?: Record<string, unknown>;
  message: string;
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
