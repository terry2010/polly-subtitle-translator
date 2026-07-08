import { useCallback, useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useNavigate } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize, LogicalPosition } from "@tauri-apps/api/window";
import { Settings as SettingsIcon, Film, FileText, Loader2, Search, Download, Square, X, Upload, ChevronDown, Check, Plus } from "lucide-react";
import { VideoPlayer } from "../components/VideoPlayer";
import { Button } from "../components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "../components/ui/select";
import { Progress } from "../components/ui/progress";
import { useVideoStore } from "../stores/videoStore";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useTranslateStore } from "../stores/translateStore";
import { useDevModeStore } from "../stores/devModeStore";
import { api, formatIpcError } from "../lib/api";
import { warn, error as logError } from "../lib/logger";
import { withPlayerHidden } from "../lib/utils";
import { SERVICES, encodeAiSelectValue, decodeAiSelectValue } from "../lib/services";
import { SubtitlePreviewPanel } from "../components/SubtitlePreviewPanel";
import { SearchDialog } from "../components/SearchDialog";
import { HdrNotice } from "../components/HdrNotice";
import { GlossaryConfirmDialog } from "../components/GlossaryConfirmDialog";
import { SubtitleStreamEditorDialog } from "../components/SubtitleStreamEditorDialog";
import { FfmpegDownloadDialog } from "../components/FfmpegDownloadDialog";

// 跟踪窗口大小状态，避免组件卸载重载时丢失（如从设置页返回）
// null = 尚未初始化，true = 大窗口（有文件），false = 小窗口（空状态）
const windowSizeState = { initialized: null as boolean | null };

// 供 SettingsView 卸载时设置
export function setWindowSizeInitialized(v: boolean) {
  windowSizeState.initialized = v;
}

// === 可搜索语言选择器 ===
interface LangOption {
  code: string;
  name: string;       // 英文名
  nativeName: string; // 母语名
}

interface SearchableLangSelectProps {
  value: string;
  onChange: (code: string) => void;
  options: LangOption[];
  placeholder?: string;
}

function SearchableLangSelect({ value, onChange, options, placeholder }: SearchableLangSelectProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const ref = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // 点击外部关闭
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
        setQuery("");
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  // 打开时自动聚焦输入框
  useEffect(() => {
    if (open) {
      setTimeout(() => inputRef.current?.focus(), 0);
    } else {
      setQuery("");
    }
  }, [open]);

  const selected = options.find((o) => o.code === value);
  const q = query.trim().toLowerCase();
  const filtered = q
    ? options.filter((o) =>
        o.code.toLowerCase().includes(q) ||
        o.name.toLowerCase().includes(q) ||
        o.nativeName.toLowerCase().includes(q)
      )
    : options;

  return (
    <div className="relative flex-1" ref={ref}>
      <button
        type="button"
        className="flex h-8 w-full items-center justify-between rounded-md border border-input bg-transparent px-2 text-xs shadow-sm hover:bg-muted/50 focus:outline-none focus:ring-1"
        onClick={() => setOpen(!open)}
      >
        <span className="truncate">{selected ? selected.nativeName : (placeholder ?? "")}</span>
        <ChevronDown className="h-3.5 w-3.5 opacity-50 shrink-0" />
      </button>
      {open && (
        <div className="absolute top-full left-0 right-0 z-50 mt-1 rounded-md border border-border bg-popover shadow-md">
          <div className="p-1.5 border-b border-border">
            <input
              ref={inputRef}
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="搜索语言..."
              className="h-7 w-full rounded border border-input bg-transparent px-2 text-xs outline-none focus:ring-1"
              onKeyDown={(e) => {
                if (e.key === "Enter" && filtered.length > 0) {
                  onChange(filtered[0].code);
                  setOpen(false);
                }
                if (e.key === "Escape") {
                  setOpen(false);
                }
              }}
            />
          </div>
          <div className="max-h-48 overflow-auto p-1">
            {filtered.length === 0 ? (
              <div className="px-2 py-3 text-center text-xs text-muted-foreground">无匹配结果</div>
            ) : (
              filtered.map((o) => (
                <button
                  key={o.code}
                  type="button"
                  className="flex w-full items-center justify-between rounded px-2 py-1.5 text-xs hover:bg-accent"
                  onClick={() => {
                    onChange(o.code);
                    setOpen(false);
                  }}
                >
                  <span className="flex items-center gap-2">
                    {o.code === value && <Check className="h-3 w-3 text-primary" />}
                    {o.code !== value && <span className="w-3" />}
                    <span>{o.nativeName}</span>
                  </span>
                  <span className="text-muted-foreground text-[10px]">{o.name}</span>
                </button>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}

// 模块级状态：跨路由保持（MainView 卸载重挂载时不丢失）
// autoExtractedRef：自动提取已执行的 stream index（避免从设置返回时重复提取字幕）
const autoExtractedStreamIdx: { current: number | null } = { current: null };
// extractCacheRef：字幕流提取缓存（streamIndex -> SubtitleFile），避免从设置返回时重新提取
const extractCache: Map<number, any> = new Map();
// currentPlayTime：播放位置（秒），跨路由保持（避免从设置返回时字幕列表停在第一条）
const currentPlayTimeState: { value: number } = { value: 0 };

export default function MainView() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { probeResult, loading, error, openVideo, clearVideo, selectedSubtitleStream, selectSubtitleStream } = useVideoStore();
  const subtitleStore = useSubtitleStore();
  const translateStore = useTranslateStore();
  const namePrecisionEnabled = useDevModeStore((s) => s.namePrecisionEnabled);
  const devModeEnabled = useDevModeStore((s) => s.devMode);
  // 人名精译状态从 translateStore 读取（跨路由保持）
  const nameExtracting = useTranslateStore((s) => s.extractingNames);
  const glossaryDialogOpen = useTranslateStore((s) => s.glossaryDialogOpen);
  const glossaryDraft = useTranslateStore((s) => s.glossaryDraft);
  const [extracting, setExtracting] = useState(false);
  const [extractProgress, setExtractProgress] = useState(0);
  const [ffmpegDialogOpen, setFfmpegDialogOpen] = useState(false);
  const ffmpegDownloadedRef = useRef(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [streamEditorOpen, setStreamEditorOpen] = useState(false);
  const [extractedFiles, setExtractedFiles] = useState<{ name: string; path: string; status: string }[]>([]);
  // 提取失败的字幕流 index 集合（用于在列表中标记不可用的流）
  const [failedStreams, setFailedStreams] = useState<Set<number>>(new Set());
  // refs for promise-based dialog flow
  const glossaryConfirmedRef = useRef(false);
  const nameExtractCancelledRef = useRef(false);
  // 自动翻译模式：弹窗弹出后翻译已在后台进行，弹窗关闭时不中止翻译
  const glossaryAutoModeRef = useRef(false);
  // 自动模式下翻译是否已完成（用于弹窗按钮状态）
  const [glossaryTranslateDone, setGlossaryTranslateDone] = useState(false);
  // 预扫描提取完毕后是否自动翻译字幕（仅专有名词精译启用时可见）
  // 未设置过默认为 false，用户点击后持久化最后一次状态
  const [autoTranslateAfterExtract, setAutoTranslateAfterExtract] = useState(false);
  // ref 镜像，避免 useCallback 闭包捕获旧值（异步流程中用户可能修改复选框）
  const autoTranslateAfterExtractRef = useRef(false);
  // 用户是否已点击过翻译按钮（控制"提取后自动翻译"选项的显示时机）
  const [translateClicked, setTranslateClicked] = useState(false);
  // 是否已加载持久化配置
  const [autoTranslateLoaded, setAutoTranslateLoaded] = useState(false);
  // 导入的外部字幕列表
  const [importedSubtitles, setImportedSubtitles] = useState<{ name: string; path: string }[]>([]);
  // 字幕流提取缓存（模块级变量，避免从设置返回时丢失缓存导致重新提取）
  const extractCacheRef = { current: extractCache };
  // 自动提取已执行的 stream index（模块级变量，避免从设置返回时重复提取）
  const autoExtractedRef = autoExtractedStreamIdx;
  // 视频信息卡展开/收起状态
  const [cardExpanded, setCardExpanded] = useState(true);
  const [cardHovered, setCardHovered] = useState(false);
  // 视频信息遮罩（右键"视频信息"触发：遮住整个界面但高亮顶部卡片）
  const [videoInfoOverlay, setVideoInfoOverlay] = useState(false);
  const [overlayFading, setOverlayFading] = useState(false);
  const cardCollapseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 加载"提取后自动翻译"的持久化配置（未设置过则默认为 false）
  useEffect(() => {
    api.getConfig("auto_translate_after_extract")
      .then((val) => {
        if (val !== null) {
          const checked = val === "true";
          setAutoTranslateAfterExtract(checked);
          autoTranslateAfterExtractRef.current = checked;
        }
      })
      .catch(() => { /* 未配置时保持默认 false */ })
      .finally(() => setAutoTranslateLoaded(true));
  }, []);

  // 打开视频后默认展开，5秒后收起（鼠标 hover 时不收起）
  useEffect(() => {
    if (!probeResult) return;
    setCardExpanded(true);
    if (cardCollapseTimer.current) clearTimeout(cardCollapseTimer.current);
    cardCollapseTimer.current = setTimeout(() => {
      if (!cardHovered) setCardExpanded(false);
    }, 5000);
    return () => {
      if (cardCollapseTimer.current) clearTimeout(cardCollapseTimer.current);
    };
  }, [probeResult]); // eslint-disable-line react-hooks/exhaustive-deps

  // 鼠标进入卡片：展开 + 取消收起定时器
  const handleCardMouseEnter = useCallback(() => {
    setCardHovered(true);
    if (cardCollapseTimer.current) clearTimeout(cardCollapseTimer.current);
    setCardExpanded(true);
  }, []);

  // 鼠标离开卡片：如果不是因为视频信息遮罩，则收起
  const handleCardMouseLeave = useCallback(() => {
    setCardHovered(false);
    if (videoInfoOverlay) return; // 视频信息展示中，不收起
    if (cardCollapseTimer.current) clearTimeout(cardCollapseTimer.current);
    cardCollapseTimer.current = setTimeout(() => setCardExpanded(false), 300);
  }, [videoInfoOverlay]);

  // 右键"视频信息"：显示遮罩 + 展开卡片，1秒后遮罩淡出
  const handleShowVideoInfo = useCallback(() => {
    setVideoInfoOverlay(true);
    setOverlayFading(false);
    setCardExpanded(true);
    if (cardCollapseTimer.current) clearTimeout(cardCollapseTimer.current);
    // 1秒后遮罩开始淡出
    setTimeout(() => setOverlayFading(true), 1000);
    // 淡出动画完成后移除遮罩
    setTimeout(() => {
      setVideoInfoOverlay(false);
      setOverlayFading(false);
      // 遮罩消失后，如果鼠标不在卡片上，5秒后收起
      if (!cardHovered) {
        if (cardCollapseTimer.current) clearTimeout(cardCollapseTimer.current);
        cardCollapseTimer.current = setTimeout(() => setCardExpanded(false), 5000);
      }
    }, 1500); // 1秒 + 0.5秒淡出动画
  }, [cardHovered]);

  // 从纯字幕模式切到视频模式时，跳过一次自动提取，保留当前编辑的字幕
  const skipAutoExtractRef = useRef(false);
  // 提取取消标志：关闭视频时设为 true，正在进行的提取完成后丢弃结果
  const extractCancelledRef = useRef(false);
  // 各翻译引擎是否已配置凭据
  const [providerConfigured, setProviderConfigured] = useState<Record<string, boolean>>({});
  // OpenAi：已配置的 AI 服务模型列表（含 serviceId + serviceName + model + modelType）
  const [aiServiceModels, setAiServiceModels] = useState<{ serviceId: string; serviceName: string; model: string; modelType: string }[]>([]);
  // 当前选中的导入字幕路径（用于高亮）
  const [selectedImportedPath, setSelectedImportedPath] = useState<string | null>(null);

  // 原生文件对话框弹出前隐藏 libmpv 子窗口，避免悬浮窗口遮挡对话框；
  // 对话框关闭后恢复显示。withPlayerHidden 抽到 utils 层供所有组件复用。

  const handleOpenVideo = useCallback(async () => {
    // 先检测 ffmpeg 是否已安装（打开视频需要 ffprobe 探测）
    if (!ffmpegDownloadedRef.current) {
      try {
        const status = await api.getFfmpegStatus();
        if (!status.installed) {
          setFfmpegDialogOpen(true);
          return;
        }
        ffmpegDownloadedRef.current = true;
      } catch (e) {
        logError("[handleOpenVideo] 检测 ffmpeg 状态失败:", e);
      }
    }
    // 启动时播放器未初始化，无需 withPlayerHidden
    const hasPlayer = !!useVideoStore.getState().probeResult;
    const doOpen = () => open({
      multiple: false,
      filters: [{ name: "Video", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "ts", "m2ts"] }],
    });
    const selected = hasPlayer
      ? await withPlayerHidden(doOpen)
      : await doOpen();
    if (typeof selected === "string") {
      // 纯字幕模式切换到视频模式：跳过自动提取，保留当前编辑的字幕
      const cur = useSubtitleStore.getState().file;
      if (cur?.source_path) {
        skipAutoExtractRef.current = true;
      }
      await openVideo(selected);
      // 后台异步提取播放器图标（不阻塞主流程，已提取过的会跳过）
      api.extractPlayerIcons(selected).catch((e) => {
        warn("提取播放器图标失败:", e);
      });
      if (cur?.source_path) {
        const name = cur.source_path.split(/[\\/]/).pop() ?? cur.source_path;
        setImportedSubtitles((prev) =>
          prev.some((s) => s.path === cur.source_path) ? prev : [...prev, { name, path: cur.source_path! }]
        );
        setSelectedImportedPath(cur.source_path);
        selectSubtitleStream(null);
      }
    }
  }, [openVideo, withPlayerHidden, selectSubtitleStream]);

  const handleOpenSubtitle = useCallback(async () => {
    const hasPlayer = !!useVideoStore.getState().probeResult;
    const doOpen = () => open({
      multiple: false,
      filters: [{ name: "Subtitle", extensions: ["srt", "ass", "ssa", "vtt", "sub"] }],
    });
    const selected = hasPlayer ? await withPlayerHidden(doOpen) : await doOpen();
    if (typeof selected === "string") {
      await subtitleStore.loadSubtitle(selected);
    }
  }, [subtitleStore, withPlayerHidden]);

  // 导入外部字幕（添加到导入列表并立刻加载第一个）
  const handleImportSubtitle = useCallback(async () => {
    const hasPlayer = !!useVideoStore.getState().probeResult;
    const doOpen = () => open({
      multiple: true,
      filters: [{ name: "Subtitle", extensions: ["srt", "ass", "ssa", "vtt", "sub"] }],
    });
    const selected = hasPlayer ? await withPlayerHidden(doOpen) : await doOpen();
    if (selected && Array.isArray(selected)) {
      // 去重：跳过已导入列表中存在的路径
      setImportedSubtitles((prev) => {
        const existing = new Set(prev.map((s) => s.path));
        const added: { name: string; path: string }[] = [];
        for (const path of selected) {
          if (existing.has(path)) continue;
          const name = path.split(/[\\/]/).pop() ?? path;
          added.push({ name, path });
        }
        return added.length ? [...prev, ...added] : prev;
      });
      // 立刻加载第一个导入的字幕
      const firstPath = selected[0];
      await subtitleStore.loadSubtitle(firstPath);
      setSelectedImportedPath(firstPath);
      selectSubtitleStream(null);
    }
  }, [subtitleStore, selectSubtitleStream, withPlayerHidden]);

  // 关闭视频：清除所有视频相关状态（提取缓存、自动提取 ref、导入字幕列表）
  const handleCloseVideo = useCallback(() => {
    // 关闭所有 toast（如双语字幕拆分提示）
    toast.dismiss();
    // 取消正在进行的提取，杀死 ffmpeg 进程，避免堆积导致 IPC 线程池耗尽
    extractCancelledRef.current = true;
    api.cancelExtractSubtitle().catch(() => {});
    extractCacheRef.current.clear();
    autoExtractedRef.current = null;
    setImportedSubtitles([]);
    setSelectedImportedPath(null);
    setFailedStreams(new Set());
    // 重置提取状态，避免异步提取未完成时卡在"正在提取字幕中..."
    setExtracting(false);
  }, []);

  // 加载导入的字幕到编辑区
  const handleLoadImportedSubtitle = useCallback(async (path: string) => {
    await subtitleStore.loadSubtitle(path);
    setSelectedImportedPath(path);
    // 取消字幕流高亮
    selectSubtitleStream(null);
  }, [subtitleStore, selectSubtitleStream]);

  // 删除导入的字幕
  const handleRemoveImportedSubtitle = useCallback((path: string) => {
    setImportedSubtitles((prev) => prev.filter((s) => s.path !== path));
  }, []);

  const handleExtractSubtitle = useCallback(async () => {
    if (!probeResult || !selectedSubtitleStream) return;
    // 重置取消标志（新的提取开始）
    extractCancelledRef.current = false;
    // 检查缓存
    const cached = extractCacheRef.current.get(selectedSubtitleStream.index);
    if (cached) {
      subtitleStore.setFile(cached);
      // 查询翻译缓存（双语文件跳过，原因同 loadSubtitle）
      if (!subtitleStore.bilingualDetect?.is_bilingual) {
        const { sourceLang, targetLang, provider, serviceId, model } = useTranslateStore.getState();
        try {
          const cachedTr = await api.getCachedTranslations(
            cached.entries, sourceLang, targetLang, provider,
            provider === "openai" ? (serviceId || undefined) : undefined,
            provider === "openai" ? (model || undefined) : undefined,
          );
          if (cachedTr && cachedTr.length > 0) {
            const entries = cached.entries.map((e: any) => {
              const tr = cachedTr.find((c) => c.index === e.index);
              return tr ? { ...e, translated: tr.translated, from_cache: true } : e;
            });
            subtitleStore.setFile({ ...cached, entries });
          }
        } catch (e) {
          warn("查询翻译缓存失败:", e);
        }
      }
      return;
    }
    setExtracting(true);
    setExtractProgress(0);
    try {
      const baseName = probeResult.video_path.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
      const lang = selectedSubtitleStream.language ?? "sub";
      // 写到系统临时目录，避免 Vite 监听项目目录变化触发页面刷新
      const tempDir = await import("@tauri-apps/api/path").then(m => m.tempDir());
      // 原流为 ass/vtt 时保留原格式，避免样式信息丢失；其余默认 srt
      const codec = selectedSubtitleStream.codec_name?.toLowerCase() ?? "";
      const ext = codec === "ass" || codec === "ssa" ? "ass"
        : codec === "webvtt" || codec === "vtt" ? "vtt"
        : "srt";
      const outputPath = `${tempDir}${baseName}.${lang}.${ext}`;
      // 传入视频时长（秒）用于计算提取进度百分比
      const durationSec = probeResult.format?.duration ?? undefined;
      await api.extractSubtitle(probeResult.video_path, selectedSubtitleStream.index, outputPath, undefined, durationSec);
      // 异步提取期间用户可能已关闭视频或切换了字幕流，检查取消标志
      if (extractCancelledRef.current) {
        warn("提取完成但已被取消，丢弃结果");
        return;
      }
      await subtitleStore.loadSubtitle(outputPath);
      // 提取成功，清除该流的失败标记
      setFailedStreams((prev) => {
        const next = new Set(prev);
        next.delete(selectedSubtitleStream.index);
        return next;
      });
      setExtractedFiles((prev) => [
        ...prev,
        { name: `${baseName}.${lang}.${ext}`, path: outputPath, status: t("video.extracted", "已提取") },
      ]);

      // 提取完成后查询翻译缓存，自动填充已翻译的条目
      // 双语文件跳过缓存查询（原因同 loadSubtitle）
      const subtitleState = useSubtitleStore.getState();
      if (subtitleState.file && !subtitleState.bilingualDetect?.is_bilingual) {
        const { sourceLang, targetLang, provider, serviceId, model } = useTranslateStore.getState();
        try {
          const cached = await api.getCachedTranslations(
            subtitleState.file.entries, sourceLang, targetLang, provider,
            provider === "openai" ? (serviceId || undefined) : undefined,
            provider === "openai" ? (model || undefined) : undefined,
          );
          if (cached && cached.length > 0) {
            const entries = subtitleState.file.entries.map((e) => {
              const tr = cached.find((c) => c.index === e.index);
              return tr ? { ...e, translated: tr.translated, from_cache: true } : e;
            });
            subtitleState.setFile({ ...subtitleState.file, entries });
          }
        } catch (e) {
          warn("查询翻译缓存失败:", e);
        }
      }
      // 缓存提取结果
      {
        const finalState = useSubtitleStore.getState();
        if (finalState.file) {
          extractCacheRef.current.set(selectedSubtitleStream.index, finalState.file);
        }
      }
    } catch (e: any) {
      logError("提取字幕失败:", e);
      const msg = formatIpcError(e);
      setExtractedFiles((prev) => [
        ...prev,
        { name: t("ffmpeg.extractFailedShort"), path: "", status: msg },
      ]);
      // 标记该流提取失败
      if (selectedSubtitleStream) {
        setFailedStreams((prev) => new Set(prev).add(selectedSubtitleStream.index));
      }
      toast.error(msg);
    } finally {
      setExtracting(false);
    }
  }, [probeResult, selectedSubtitleStream, subtitleStore, t]);

  // 自动提取字幕：当 selectedSubtitleStream 首次被设置时自动提取
  const handleExtractRef = useRef(handleExtractSubtitle);
  handleExtractRef.current = handleExtractSubtitle;
  // 监听字幕提取进度事件
  useEffect(() => {
    const unlisten = listen<{ progress: number }>("extract_progress", (event) => {
      setExtractProgress(event.payload.progress);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    if (!probeResult || !selectedSubtitleStream) return;
    if (autoExtractedRef.current === selectedSubtitleStream.index) return;
    autoExtractedRef.current = selectedSubtitleStream.index;
    if (skipAutoExtractRef.current) {
      skipAutoExtractRef.current = false;
      return;
    }
    handleExtractRef.current();
  }, [probeResult, selectedSubtitleStream]);

  // Ctrl+S 全局快捷键：分发 "export-subtitle" 事件，由 SubtitlePreviewPanel 监听并打开 ExportDialog
  // WebView2 中 Ctrl+S 可能触发浏览器保存对话框，需 preventDefault 拦截
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s" && !e.shiftKey) {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("export-subtitle"));
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  // 查询各翻译引擎是否已配置凭据
  useEffect(() => {
    // 传统翻译：baidu/bing/google
    const traditionalProviders = ["baidu", "bing", "google"];
    // AI 服务：遍历所有 AI 服务定义，检查 per-service 配置
    const aiServices = SERVICES.filter((s) => s.category === "ai");

    const traditionalPromise = Promise.all(traditionalProviders.map(async (p) => {
      try {
        const [appId, secretKeyring, secretConfig] = await Promise.all([
          api.getConfig(`translate_${p}_app_id`).catch(() => null),
          api.getCredential(p, "secret", `启动检查配置状态(${p})`).catch(() => null),
          api.getConfig(`translate_${p}_secret`).catch(() => null),
        ]);
        const configured = !!(appId && (secretKeyring || secretConfig));
        return [p, configured] as [string, boolean];
      } catch {
        return [p, false] as [string, boolean];
      }
    }));

    const aiPromise = Promise.all(aiServices.map(async (s) => {
      try {
        const [baseUrl, selectedModels] = await Promise.all([
          api.getConfig(`translate_openai_${s.id}_base_url`).catch(() => null),
          api.getConfig(`translate_openai_${s.id}_selected_models`).catch(() => null),
        ]);
        if (!baseUrl || !selectedModels) return [s.id, false] as [string, boolean];
        if (s.requiresApiKey) {
          const apiKey = await api.getCredential(`openai_${s.id}`, "secret", `启动检查配置状态(${s.id})`).catch(() => null);
          if (!apiKey) return [s.id, false] as [string, boolean];
        }
        return [s.id, true] as [string, boolean];
      } catch {
        return [s.id, false] as [string, boolean];
      }
    }));

    // 同时加载保存的 provider 配置，避免与兜底逻辑竞态
    const savedConfigPromise = Promise.all([
      api.getConfig("translate_provider").catch(() => null),
      api.getConfig("translate_openai_service_id").catch(() => null),
      api.getConfig("translate_current_model").catch(() => null),
    ]);

    Promise.all([traditionalPromise, aiPromise, savedConfigPromise]).then(async ([tradResults, aiResults, [savedProvider, savedServiceId, savedModel]]) => {
      const configured = Object.fromEntries([...tradResults, ...aiResults]);
      setProviderConfigured(configured);

      // 1. 先用 db 中保存的值恢复 provider（修复旧版本可能把 serviceId 存为 provider 的 bug）
      const aiServiceIds = aiServices.map((s) => s.id);
      let effectiveProvider = translateStore.provider;
      let effectiveServiceId = translateStore.serviceId;
      let effectiveModel = translateStore.model;

      if (savedProvider) {
        // "openai" 是所有 AI 服务的 provider 值，不是 serviceId
        // 旧版本 bug：savedProvider 实际上是 serviceId（如 "deepseek"），此时 savedProvider !== "openai"
        const isServiceId = savedProvider !== "openai" && aiServiceIds.includes(savedProvider);
        if (isServiceId) {
          // 旧版本 bug：savedProvider 实际上是 serviceId
          effectiveProvider = "openai";
          effectiveServiceId = savedProvider;
          effectiveModel = savedModel || "";
          // 修正 db
          api.setConfig("translate_provider", "openai").catch(() => {});
          api.setConfig("translate_openai_service_id", savedProvider).catch(() => {});
        } else {
          effectiveProvider = savedProvider;
          if (savedProvider === "openai") {
            effectiveServiceId = savedServiceId || null;
            effectiveModel = savedModel || "";
          } else {
            effectiveServiceId = null;
            effectiveModel = "";
          }
        }
      }

      // 记录切换前的引擎信息，用于 toast 提示
      const oldProvider = effectiveProvider;
      const oldServiceId = effectiveServiceId;
      const oldModel = effectiveModel;

      // 2. 检查 effectiveProvider 是否已配置
      // 对于 AI 服务，必须同时检查 serviceId 已配置且当前 model 在该服务的已选模型列表中
      let isCurrentConfigured = false;
      let currentSelectedModels: string[] = [];
      if (effectiveProvider === "openai" && effectiveServiceId) {
        const savedModels = await api.getConfig(`translate_openai_${effectiveServiceId}_selected_models`).catch(() => null);
        currentSelectedModels = savedModels ? savedModels.split(",").filter(Boolean) : [];
        isCurrentConfigured = configured[effectiveServiceId] && currentSelectedModels.includes(effectiveModel || "");
      } else if (effectiveProvider !== "openai") {
        isCurrentConfigured = configured[effectiveProvider];
      }

      if (!isCurrentConfigured) {
        // 优先找传统翻译
        const firstTrad = traditionalProviders.find((p) => configured[p]);
        if (firstTrad) {
          effectiveProvider = firstTrad;
          effectiveServiceId = null;
          effectiveModel = "";
        } else {
          // 找第一个已配置的 AI 服务
          const firstAi = aiServices.find((s) => configured[s.id]);
          if (firstAi) {
            effectiveProvider = "openai";
            effectiveServiceId = firstAi.id;
            // 加载该服务的第一个模型
            const models = await api.getConfig(`translate_openai_${firstAi.id}_selected_models`).catch(() => null);
            if (models) {
              effectiveModel = models.split(",")[0] || "";
            }
            // 持久化
            api.setConfig("translate_provider", "openai").catch(() => {});
            api.setConfig("translate_openai_service_id", firstAi.id).catch(() => {});
            api.setConfig("translate_current_model", effectiveModel).catch(() => {});
          } else {
            // 没有任何引擎已配置：清空 provider，让 Select 显示 placeholder
            effectiveProvider = "";
            effectiveServiceId = null;
            effectiveModel = "";
            api.setConfig("translate_provider", "").catch(() => {});
            api.setConfig("translate_openai_service_id", "").catch(() => {});
            api.setConfig("translate_current_model", "").catch(() => {});
          }
        }
      } else if (effectiveProvider === "openai" && effectiveServiceId && !currentSelectedModels.includes(effectiveModel || "")) {
        // service 已配置，但当前 model 已不在该服务的已选模型列表中，切换到该服务的第一个模型
        effectiveModel = currentSelectedModels[0] || "";
        if (effectiveModel) {
          api.setConfig("translate_current_model", effectiveModel).catch(() => {});
        }
      } else if (effectiveProvider === "openai" && !effectiveModel && effectiveServiceId) {
        // provider 已配置但 model 为空，加载第一个模型
        const models = await api.getConfig(`translate_openai_${effectiveServiceId}_selected_models`).catch(() => null);
        if (models) {
          effectiveModel = models.split(",")[0] || "";
          api.setConfig("translate_current_model", effectiveModel).catch(() => {});
        }
      }

      // 3. 一次性更新 store，避免中间状态导致 Select 显示为空
      translateStore.setProvider(effectiveProvider);
      translateStore.setServiceId(effectiveServiceId);
      if (effectiveModel) translateStore.setModel(effectiveModel);

      // 4. 如果发生了引擎切换，仅记录日志（初始化时不弹 toast，避免从设置返回时打扰用户）
      const hasChanged = effectiveProvider !== oldProvider || effectiveServiceId !== oldServiceId || effectiveModel !== oldModel;
      if (hasChanged) {
        const getProviderName = (provider: string, serviceId: string | null, model: string) => {
          if (provider === "openai" && serviceId && model) {
            const service = SERVICES.find((s) => s.id === serviceId);
            return service ? `${service.name} - ${model}` : model;
          }
          const service = SERVICES.find((s) => s.id === provider);
          return service?.name || provider;
        };
        const oldName = getProviderName(oldProvider, oldServiceId, oldModel);
        const newName = getProviderName(effectiveProvider, effectiveServiceId, effectiveModel);
        console.info(`[MainView] 翻译引擎已切换: ${oldName} -> ${newName}`);
      }
    });
  }, []);

  // OpenAi：加载所有已配置 AI 服务的模型列表（用于引擎下拉）
  // 遍历所有 AI 服务，收集已配置的 serviceId + models
  useEffect(() => {
    const aiServices = SERVICES.filter((s) => s.category === "ai");
    Promise.all(aiServices.map(async (s) => {
      const [baseUrl, selectedModels, modelTypes] = await Promise.all([
        api.getConfig(`translate_openai_${s.id}_base_url`).catch(() => null),
        api.getConfig(`translate_openai_${s.id}_selected_models`).catch(() => null),
        api.getConfig(`translate_openai_${s.id}_selected_model_types`).catch(() => null),
      ]);
      if (!baseUrl || !selectedModels) return [];
      const ids = selectedModels.split(",").filter(Boolean);
      let typeMap: Record<string, string> = {};
      try { typeMap = JSON.parse(modelTypes || "{}"); } catch { /* ignore */ }
      return ids.map((id) => ({
        serviceId: s.id,
        serviceName: s.name,
        model: id,
        modelType: typeMap[id] || "generic",
      }));
    })).then((results) => {
      setAiServiceModels(results.flat());
    });
  }, []);

  // 兜底：如果 provider 是 openai 且 model 已设置，但 model 不在 aiServiceModels 列表中，
  // 自动将 model 加入列表，避免下拉框找不到匹配项显示为空
  useEffect(() => {
    if (translateStore.provider === "openai" && translateStore.model) {
      setAiServiceModels((prev) => {
        if (prev.find((m) => m.model === translateStore.model && m.serviceId === translateStore.serviceId)) return prev;
        return [...prev, {
          serviceId: translateStore.serviceId || "openai",
          serviceName: translateStore.serviceId || "openai",
          model: translateStore.model,
          modelType: translateStore.modelType || "generic",
        }];
      });
    }
  }, [translateStore.provider, translateStore.model, translateStore.serviceId]);

  // === SECTION 1 END ===

  // 窗口大小自动调整：空状态用小窗口，打开文件后放大
  // 同时调整位置保持窗口中心点不变，避免放大后窗口跑出屏幕
  // 首次渲染跳过：Rust setup 已完成初始居中，避免二次定位导致抖动
  // 使用模块级变量，避免组件卸载重载（如从设置页返回）时丢失状态
  const hasFile = !!(probeResult || subtitleStore.file || loading || error || subtitleStore.error);
  useEffect(() => {
    if (windowSizeState.initialized === null) {
      windowSizeState.initialized = hasFile;
      return;
    }
    if (windowSizeState.initialized === hasFile) return;
    windowSizeState.initialized = hasFile;

    const win = getCurrentWindow();
    const newW = hasFile ? 1280 : 520;
    const newH = hasFile ? 800 : 325;
    (async () => {
      try {
        const scaleFactor = await win.scaleFactor();
        // setSize 设置的是 inner size（客户区），所以用 innerSize 比较
        const inner = await win.innerSize();
        const curW = inner.width / scaleFactor;
        const curH = inner.height / scaleFactor;
        // 尺寸已匹配则跳过，避免亚像素舍入导致窗口闪烁
        if (Math.abs(curW - newW) < 1 && Math.abs(curH - newH) < 1) return;

        // 获取原窗口中心点（setSize 之前），用于保持窗口大致在原位置
        const pos = await win.outerPosition();
        const outer = await win.outerSize();
        let cx = pos.x + outer.width / 2;
        let cy = pos.y + outer.height / 2;

        // 目标窗口物理尺寸（inner → physical）
        let winPhysW = Math.round(newW * scaleFactor);
        let winPhysH = Math.round(newH * scaleFactor);
        let finalW = newW;
        let finalH = newH;

        // 用工作区（排除任务栏）约束窗口尺寸和位置
        try {
          const wa = await api.getWorkArea();
          // 如果目标窗口物理尺寸超过工作区，缩小窗口以适应
          if (winPhysW > wa.width) {
            winPhysW = wa.width;
            finalW = Math.floor(wa.width / scaleFactor);
          }
          if (winPhysH > wa.height) {
            winPhysH = wa.height;
            finalH = Math.floor(wa.height / scaleFactor);
          }
          // 约束中心点在工作区内
          cx = Math.min(Math.max(cx, wa.x + winPhysW / 2), wa.x + wa.width - winPhysW / 2);
          cy = Math.min(Math.max(cy, wa.y + winPhysH / 2), wa.y + wa.height - winPhysH / 2);
        } catch {
          // 获取工作区失败：至少保证中心点不导致窗口跑到负坐标
          cx = Math.max(cx, winPhysW / 2);
          cy = Math.max(cy, winPhysH / 2);
        }

        const newX = Math.round(cx - winPhysW / 2);
        const newY = Math.round(cy - winPhysH / 2);
        // 先 setPosition 再 setSize：先移动到目标位置（保持旧尺寸），再设置新尺寸
        await win.setPosition(new LogicalPosition(newX / scaleFactor, newY / scaleFactor));
        await win.setSize(new LogicalSize(finalW, finalH));
      } catch {
        win.setSize(new LogicalSize(newW, newH)).catch(() => {});
      }
    })();
  }, [hasFile]);

  const handlePlayVideo = useCallback(async () => {
    if (!probeResult) return;
    // 降级路径：使用系统默认播放器打开视频
    try {
      await api.openInSystemPlayer(probeResult.video_path);
    } catch (e) {
      logError("播放失败:", e);
    }
  }, [probeResult]);

  // libmpv 播放位置更新回调——用于字幕高亮联动
  // currentPlayTime 用模块级变量 + useState 双写，确保从设置返回时初始值不丢失
  const [currentPlayTime, setCurrentPlayTimeState] = useState(currentPlayTimeState.value);
  const handlePositionUpdate = useCallback((posSec: number, _durSec: number, _paused: boolean) => {
    currentPlayTimeState.value = posSec;
    setCurrentPlayTimeState(posSec);
  }, []);

  // ISO 639-2/B → ISO 639-1 映射（FFprobe 常见的三字母语言码 + 非标准码）
  const LANG_639_2_TO_1: Record<string, string> = {
    // 英语
    eng: "en", en: "en",
    // 中文
    chi: "zh", zho: "zh", zh: "zh", chs: "zh", cht: "zh",
    "zh-hans": "zh", "zh-hant": "zh", "zh-cn": "zh", "zh-tw": "zh",
    // 日语
    jpn: "ja", ja: "ja",
    // 韩语
    kor: "ko", ko: "ko",
    // 法语
    fra: "fr", fre: "fr", fr: "fr",
    // 德语
    deu: "de", ger: "de", de: "de",
    // 西班牙语
    spa: "es", es: "es",
    // 俄语
    rus: "ru", ru: "ru",
    // 葡萄牙语
    por: "pt", pt: "pt", ptb: "pt", "pt-br": "pt",
    // 意大利语
    ita: "it", it: "it",
    // 泰语
    tha: "th", th: "th",
    // 越南语
    vie: "vi", vi: "vi",
    // 印尼语
    ind: "id", id: "id",
    // 马来语
    may: "ms", msa: "ms", ms: "ms",
    // 阿拉伯语
    ara: "ar", ar: "ar",
    // 印地语
    hin: "hi", hi: "hi",
    // 土耳其语
    tur: "tr", tr: "tr",
    // 波兰语
    pol: "pl", pl: "pl",
    // 荷兰语
    nld: "nl", dut: "nl", nl: "nl",
    // 瑞典语
    swe: "sv", sv: "sv",
    // 芬兰语
    fin: "fi", fi: "fi",
    // 丹麦语
    dan: "da", da: "da",
    // 挪威语
    nor: "no", no: "no",
    // 捷克语
    ces: "cs", cz: "cs", cs: "cs",
    // 匈牙利语
    hun: "hu", hu: "hu",
    // 罗马尼亚语
    ron: "ro", rum: "ro", ro: "ro",
    // 希腊语
    ell: "el", gre: "el", el: "el",
    // 希伯来语
    heb: "he", he: "he",
    // 乌克兰语
    ukr: "uk", uk: "uk",
    // 波斯语
    fas: "fa", per: "fa", fa: "fa",
    // 缅甸语
    mya: "my", bur: "my", my: "my",
    // 高棉语（柬埔寨）
    khm: "km", km: "km",
    // 老挝语
    lao: "lo", lo: "lo",
    // 蒙古语
    mon: "mn", mn: "mn",
    // 世界语
    epo: "eo", eo: "eo",
    // 其他常见亚洲语言
    tgl: "tl", tl: "tl", // 他加禄语（菲律宾）
  };

  // 当选中的字幕流变化时，自动推断来源语言
  useEffect(() => {
    if (!selectedSubtitleStream) return;
    const rawLang = selectedSubtitleStream.language;
    if (rawLang && rawLang.trim()) {
      const normalized = LANG_639_2_TO_1[rawLang.trim().toLowerCase()];
      if (normalized) {
        translateStore.setSourceLang(normalized);
        return;
      }
    }
    // 无法识别语言 → auto
    translateStore.setSourceLang("auto");
  }, [selectedSubtitleStream]); // eslint-disable-line react-hooks/exhaustive-deps

  // 来源语言列表（含 auto）
  const SOURCE_LANG_OPTIONS: LangOption[] = [
    { code: "auto", name: "Auto Detect", nativeName: t("translate.autoDetect", "自动检测") },
    { code: "en", name: "English", nativeName: "English" },
    { code: "ja", name: "Japanese", nativeName: "日本語" },
    { code: "ko", name: "Korean", nativeName: "한국어" },
    { code: "zh", name: "Chinese", nativeName: "中文" },
    { code: "fr", name: "French", nativeName: "Français" },
    { code: "de", name: "German", nativeName: "Deutsch" },
    { code: "es", name: "Spanish", nativeName: "Español" },
    { code: "ru", name: "Russian", nativeName: "Русский" },
    { code: "pt", name: "Portuguese", nativeName: "Português" },
    { code: "it", name: "Italian", nativeName: "Italiano" },
    { code: "th", name: "Thai", nativeName: "ไทย" },
    { code: "vi", name: "Vietnamese", nativeName: "Tiếng Việt" },
    { code: "id", name: "Indonesian", nativeName: "Bahasa Indonesia" },
    { code: "ms", name: "Malay", nativeName: "Bahasa Melayu" },
    { code: "ar", name: "Arabic", nativeName: "العربية" },
    { code: "hi", name: "Hindi", nativeName: "हिन्दी" },
    { code: "tr", name: "Turkish", nativeName: "Türkçe" },
    { code: "pl", name: "Polish", nativeName: "Polski" },
    { code: "nl", name: "Dutch", nativeName: "Nederlands" },
    { code: "sv", name: "Swedish", nativeName: "Svenska" },
    { code: "fi", name: "Finnish", nativeName: "Suomi" },
    { code: "da", name: "Danish", nativeName: "Dansk" },
    { code: "no", name: "Norwegian", nativeName: "Norsk" },
    { code: "cs", name: "Czech", nativeName: "Čeština" },
    { code: "hu", name: "Hungarian", nativeName: "Magyar" },
    { code: "ro", name: "Romanian", nativeName: "Română" },
    { code: "el", name: "Greek", nativeName: "Ελληνικά" },
    { code: "he", name: "Hebrew", nativeName: "עברית" },
    { code: "uk", name: "Ukrainian", nativeName: "Українська" },
    { code: "fa", name: "Persian", nativeName: "فارسی" },
    { code: "my", name: "Burmese", nativeName: "မြန်မာ" },
    { code: "km", name: "Khmer", nativeName: "ខ្មែរ" },
    { code: "lo", name: "Lao", nativeName: "ລາວ" },
    { code: "mn", name: "Mongolian", nativeName: "Монгол" },
    { code: "tl", name: "Tagalog", nativeName: "Tagalog" },
  ];

  // 目标语言列表（不含 auto）
  const TARGET_LANG_OPTIONS: LangOption[] = SOURCE_LANG_OPTIONS.filter((o) => o.code !== "auto");

  // 各翻译引擎是否支持 auto 自动检测源语言
  const PROVIDER_SUPPORTS_AUTO: Record<string, boolean> = {
    baidu: true,
    bing: false,
    google: false,
  };

  const handleTranslateAndMerge = useCallback(async () => {
    if (!subtitleStore.file) return;
    setTranslateClicked(true);
    const { sourceLang, provider, serviceId } = translateStore;
    // 检查：翻译 API 是否已配置凭据
    // AI 服务：检查 serviceId 对应的配置；传统翻译：检查 provider 对应的配置
    const configKey = provider === "openai" ? (serviceId || "openai") : provider;
    if (!providerConfigured[configKey]) {
      const providerName = provider === "openai"
        ? (SERVICES.find((s) => s.id === serviceId)?.name || "AI")
        : provider === "bing" ? "Bing" : provider === "google" ? "Google" : t("settings.baidu");
      toast.error(
        t("translate.providerNotConfigured", "{{provider}} 翻译 API 尚未配置密钥，请先在设置中配置后再翻译", { provider: providerName }),
        {
          action: {
            label: t("translate.goConfig", "去配置"),
            onClick: () => navigate("/settings?provider=" + provider),
          },
          duration: 5000,
        }
      );
      return;
    }
    // 检查：如果翻译 API 不支持 auto 且源语言为 auto，提示用户
    if (sourceLang === "auto" && !PROVIDER_SUPPORTS_AUTO[provider]) {
      const providerName = provider === "bing" ? "Bing" : provider === "google" ? "Google" : provider;
      toast.error(
        t("translate.autoNotSupported", "{{provider}} 翻译不支持自动检测源语言，请在上方「来源语言」下拉框中手动选择字幕的语言", { provider: providerName })
      );
      return;
    }

    // 人名精译：AI 翻译且启用时，先预扫描提取人名
    let glossary: [string, string][] | undefined;
    let nameTagging = false;
    if (namePrecisionEnabled && provider === "openai") {
      nameTagging = true;
      nameExtractCancelledRef.current = false;
      translateStore.setExtractingNames(true);
      toast.info(t("translate.nameExtracting", "正在预扫描提取人名..."));
      try {
        const extracted = await translateStore.extractNames(subtitleStore.file.entries);
        // 检查是否被用户取消
        if (nameExtractCancelledRef.current) {
          translateStore.setExtractingNames(false);
          toast.info(t("translate.nameExtractCancelled", "人名精译已取消，翻译中止"));
          return;
        }
        if (extracted && extracted.length > 0) {
          // 先隐藏播放器子窗口（await 确保 IPC 执行完成），避免 Dialog 渲染时被视频遮盖
          api.devLog("[MainView] 翻译前调用 playerHide");
          await api.playerHide().catch(() => { /* 播放器未初始化，忽略 */ });

          if (autoTranslateAfterExtractRef.current) {
            // 勾选了"自动翻译"：弹出译名表供查看，但不等待确认，直接开始翻译
            // 播放器保持隐藏（弹窗显示期间不恢复），避免视频覆盖弹窗
            glossaryAutoModeRef.current = true;
            translateStore.setGlossaryDraft(extracted);
            translateStore.setGlossaryDialogOpen(true);
            glossary = extracted.map((g) => [g.english, g.chinese] as [string, string]);
            api.devLog("[MainView] 自动模式：弹窗已弹出，播放器保持隐藏，开始翻译");
          } else {
            // 未勾选：弹窗让用户确认/修改译名表，等待用户操作
            glossaryAutoModeRef.current = false;
            try {
              translateStore.setGlossaryDraft(extracted);
              translateStore.setGlossaryDialogOpen(true);
              // 等待用户确认（通过轮询 store 状态）
              const confirmed = await new Promise<boolean>((resolve) => {
                const checkInterval = setInterval(() => {
                  if (!useTranslateStore.getState().glossaryDialogOpen) {
                    clearInterval(checkInterval);
                    resolve(glossaryConfirmedRef.current);
                  }
                }, 200);
              });
              if (!confirmed) {
                translateStore.setExtractingNames(false);
                toast.info(t("translate.nameExtractCancelled", "人名精译已取消，翻译中止"));
                return;
              }
              glossary = useTranslateStore.getState().glossaryDraft.map((g) => [g.english, g.chinese] as [string, string]);
            } finally {
              // 弹窗关闭后恢复播放器（无论确认还是取消）
              api.devLog("[MainView] 弹窗关闭后调用 playerShow");
              api.playerShow().catch(() => { /* 播放器未初始化，忽略 */ });
            }
          }
        }
      } catch (e: any) {
        warn("人名预扫描失败，继续正常翻译:", e);
        toast.warning(t("translate.nameExtractFailed", "人名预扫描失败，将使用正常翻译"));
      }
      translateStore.setExtractingNames(false);
    }

    // 逐条翻译、逐条填充
    setGlossaryTranslateDone(false);
    const result = await translateStore.startTranslate(
      subtitleStore.file.entries,
      (index, translated, failed) => {
        // 每条翻译完成后立即更新字幕预览区（含翻译失败标记）
        subtitleStore.updateEntry(index, { translated, failed, from_cache: false });
      },
      undefined,
      glossary,
      nameTagging,
      subtitleStore.file?.file_hash || undefined,
    );
    if (result && result.translations.length > 0) {
      // 确保所有结果都更新（包括可能遗漏的），同步 failed 和 from_cache 标记
      const entries = subtitleStore.file.entries.map((e) => {
        const tr = result.translations.find((r) => r.index === e.index);
        if (!tr) return e;
        // 已有译文且非失败的保留，否则用结果覆盖（含 failed 和 from_cache）
        if (e.translated && !e.failed) return e;
        return { ...e, translated: tr.translated, failed: tr.failed, from_cache: tr.from_cache };
      });
      subtitleStore.setFile({ ...subtitleStore.file, entries });
    }
    // 翻译完成，更新弹窗按钮状态
    setGlossaryTranslateDone(true);
  }, [subtitleStore, translateStore, t, providerConfigured, navigate, namePrecisionEnabled]);

  const formatDuration = (s: number | null) => {
    if (!s) return "--";
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = Math.floor(s % 60);
    return `${h}:${m.toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}`;
  };

  const formatEta = (secs: number) => {
    if (secs <= 0) return "--";
    if (secs < 60) return `${Math.ceil(secs)}秒`;
    const m = Math.floor(secs / 60);
    const s = Math.ceil(secs % 60);
    return `${m}分${s}秒`;
  };

  const formatSize = (bytes: number | null) => {
    if (!bytes) return "--";
    if (bytes > 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
    if (bytes > 1e6) return `${(bytes / 1e6).toFixed(2)} MB`;
    return `${(bytes / 1e3).toFixed(0)} KB`;
  };

  const videoFileName = probeResult?.video_path.split(/[\\/]/).pop() ?? "";

  // === SECTION 2 END ===

  // 空状态：未打开任何文件
  if (!probeResult && !loading && !error && !subtitleStore.file && !subtitleStore.error) {
    return (
      <div className="flex h-screen flex-col">
        <div className="flex flex-1 flex-col items-center justify-center gap-6">
          <div className="flex gap-4">
            <Button size="lg" onClick={handleOpenVideo} className="h-20 w-44 flex-col gap-2">
              <Film className="h-6 w-6" />
              {t("menu.openVideo")}
            </Button>
            <Button size="lg" variant="secondary" onClick={handleOpenSubtitle} className="h-20 w-44 flex-col gap-2">
              <FileText className="h-6 w-6" />
              {t("menu.openSubtitle")}
            </Button>
          </div>
          <p className="text-sm text-muted-foreground text-center whitespace-nowrap">
            {t("app.dragHint")} · mkv mp4 avi mov / srt ass vtt
          </p>
          <div className="flex gap-4 w-[368px]">
            <div className="w-44 flex justify-start" />
            <div className="w-44 flex justify-end">
              <Button variant="ghost" size="sm" onClick={() => navigate("/settings")}>
                <SettingsIcon className="mr-1 h-4 w-4" />
                {t("menu.settings")}
              </Button>
            </div>
          </div>
        </div>
        <SearchDialog open={searchOpen} onOpenChange={setSearchOpen} />
        <FfmpegDownloadDialog
          open={ffmpegDialogOpen}
          onDownloaded={() => {
            setFfmpegDialogOpen(false);
            ffmpegDownloadedRef.current = true;
          }}
          onCancel={() => setFfmpegDialogOpen(false)}
        />
      </div>
    );
  }

  return (
    <div className="flex h-screen flex-col">
      {/* 视频信息遮罩：遮住整个界面，顶部卡片 z-[60] 浮在遮罩上方 */}
      {videoInfoOverlay && (
        <div
          className={`fixed inset-0 z-50 bg-black/40 transition-opacity duration-500 ${
            overlayFading ? "opacity-0" : "opacity-100"
          }`}
        />
      )}
      {/* 主体两栏布局 */}
      <main className="flex flex-1 overflow-hidden">
        {/* 左栏：播放预览 + 字幕对比预览 */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {loading && (
            <div className="flex items-center justify-center py-20">
              <Loader2 className="h-8 w-8 animate-spin" />
            </div>
          )}
          {error && (
            <div className="m-4 rounded-md border border-destructive/50 bg-destructive/10 p-4 text-sm text-destructive">
              {error}
            </div>
          )}
          {subtitleStore.error && !error && (
            <div className="m-4 rounded-md border border-destructive/50 bg-destructive/10 p-4 text-sm text-destructive">
              {subtitleStore.error}
            </div>
          )}

          {/* 播放预览区（仅视频模式） */}
          {probeResult && (
            <div className="flex-shrink-0 border-b">
              {/* 视频预览 + 内嵌字幕列表 横向排列 */}
              {/* items-start：不让字幕列表拉伸到与播放器等高；播放器自身视频区已限制 40vh，
                  播控条在视频区下方正常显示，不被 overflow-hidden 裁剪 */}
              <div className="flex gap-0 items-start p-4">
                {/* 视频预览（libmpv 内嵌播放）。不要再套 max-h-[40vh] overflow-hidden，
                    否则会把 VideoPlayer 下方的播控条裁掉（视频区已自行限制 40vh） */}
                <div className="flex-1 min-w-0">
                  <VideoPlayer
                    probeResult={probeResult}
                    onPositionUpdate={handlePositionUpdate}
                    onCloseVideo={() => { clearVideo(); subtitleStore.setFile(null); handleCloseVideo(); }}
                    onShowVideoInfo={handleShowVideoInfo}
                  />
                </div>
                {/* 内嵌字幕列表 */}
                <div className="w-56 border-l bg-muted/20 flex flex-col max-h-[40vh] overflow-hidden">
                  <div className="px-3 py-1.5 border-b text-xs font-medium flex-shrink-0 flex items-center justify-between">
                    <span>{t("video.subtitleStreams")} ({probeResult.subtitle_streams.length})</span>
                    <button
                      className="text-xs text-primary hover:underline"
                      onClick={() => setStreamEditorOpen(true)}
                    >
                      {t("subtitle.streamEditor")}
                    </button>
                  </div>
                  <div className="overflow-auto p-1.5 space-y-1 flex-1">
                    {probeResult.subtitle_streams.length === 0 && (
                      <p className="text-xs text-muted-foreground px-1 py-2">{t("video.noSubtitle")}</p>
                    )}
                    {probeResult.subtitle_streams.map((stream) => {
                      const failed = failedStreams.has(stream.index);
                      return (
                      <button
                        key={stream.index}
                        onClick={() => {
                          selectSubtitleStream(stream);
                          setSelectedImportedPath(null);
                          // 如果该流已在缓存中，加载到字幕编辑器
                          if (extractCacheRef.current.has(stream.index)) {
                            const cachedFile = extractCacheRef.current.get(stream.index);
                            subtitleStore.setFile(cachedFile);
                          }
                        }}
                        className={`w-full text-left rounded px-2 py-1.5 text-xs transition-colors flex items-center gap-2 ${
                          selectedSubtitleStream?.index === stream.index
                            ? "bg-primary text-primary-foreground"
                            : "hover:bg-accent"
                        } ${failed ? "opacity-50" : ""}`}
                        disabled={stream.is_graphic}
                      >
                        <span className={`w-2 h-2 rounded-full ${selectedSubtitleStream?.index === stream.index ? "bg-primary-foreground" : "bg-muted-foreground/40"}`} />
                        <span className="font-mono">#{stream.index}</span>
                        <span>{stream.language ?? "??"}</span>
                        {stream.disposition_forced && <span className="opacity-60">forced</span>}
                        {stream.disposition_hearing_impaired && <span className="opacity-60">SDH</span>}
                        {stream.is_graphic && <span className="opacity-60">(graphic)</span>}
                        {failed && <span className="opacity-60 text-destructive">✗</span>}
                      </button>
                      );
                    })}
                  </div>
                  {/* 导入的外部字幕列表 */}
                  {importedSubtitles.length > 0 && (
                    <div className="border-t flex-shrink-0">
                      <div className="px-3 py-1 border-b text-xs font-medium bg-muted/30">
                        {t("subtitle.imported")} ({importedSubtitles.length})
                      </div>
                      <div className="p-1 space-y-0.5 max-h-24 overflow-auto">
                        {importedSubtitles.map((sub) => (
                          <div key={sub.path} className={`flex items-center gap-1 rounded px-2 py-1 text-xs group transition-colors ${
                            selectedImportedPath === sub.path ? "bg-primary text-primary-foreground" : "hover:bg-accent"
                          }`}>
                            <FileText className="h-3 w-3 flex-shrink-0" />
                            <button
                              className="flex-1 text-left truncate"
                              onClick={() => handleLoadImportedSubtitle(sub.path)}
                            >
                              {sub.name}
                            </button>
                            <button
                              className={`flex-shrink-0 ${selectedImportedPath === sub.path ? "text-primary-foreground hover:text-primary-foreground/70" : "opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive"}`}
                              onClick={() => handleRemoveImportedSubtitle(sub.path)}
                            >
                              <X className="h-3 w-3" />
                            </button>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              </div>
              {/* HDR 提示 */}
              <HdrNotice />
              {/* 播控条由 VideoPlayer 组件自身渲染（见上方 VideoPlayer），
                  此处不再放置静态占位播控条，避免出现两条控制栏且占位条永远显示 0:00 */}
            </div>
          )}

          {/* 字幕对比预览区 */}
          <div className="flex-1 min-h-0 overflow-hidden">
            <SubtitlePreviewPanel extracting={extracting} extractProgress={extractProgress} currentPlayTime={currentPlayTime} />
          </div>
        </div>

        {/* 右栏：视频信息 + 字幕操作区 */}
        <div className="w-80 border-l overflow-auto p-3 space-y-3 flex-shrink-0">
          {/* 视频信息卡（可展开/收起，hover 展开，5秒后自动收起） */}
          {probeResult && (
            <Card
              className={`relative transition-all duration-300 overflow-hidden ${videoInfoOverlay ? "z-[60]" : ""}`}
              onMouseEnter={handleCardMouseEnter}
              onMouseLeave={handleCardMouseLeave}
            >
              <CardHeader className="pb-2">
                <CardTitle className="flex items-center gap-1 text-sm">
                  <Film className="h-4 w-4" />
                  {videoFileName}
                </CardTitle>
              </CardHeader>
              <div
                className="grid transition-all duration-300 ease-in-out"
                style={{
                  gridTemplateRows: cardExpanded ? "1fr" : "0fr",
                  opacity: cardExpanded ? 1 : 0,
                }}
              >
                <CardContent className="space-y-1 text-xs text-muted-foreground overflow-hidden">
                  <div>{t("video.duration")}: {formatDuration(probeResult.format.duration)} · {formatSize(probeResult.format.size)}</div>
                  {probeResult.video_stream && (
                    <div>{probeResult.video_stream.width}x{probeResult.video_stream.height} {probeResult.video_stream.codec_name}</div>
                  )}
                  <div>{t("video.format", "格式")}: {probeResult.format.format_name}</div>
                  <div>{t("video.audioStreams")}: {probeResult.audio_streams.map(s => s.language ?? "??").join(", ")}</div>
                  <div>{t("video.subtitleStreams")}: {probeResult.subtitle_streams.length}</div>
                </CardContent>
              </div>
            </Card>
          )}

          {/* 快捷操作卡：视频模式 / 纯字幕模式 / 错误状态 */}
          {probeResult || (subtitleStore.file && !probeResult) || error || subtitleStore.error ? (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">{t("video.quickOps")}</CardTitle>
              </CardHeader>
              <CardContent className="space-y-1.5">
                <div className="flex gap-1">
                  <Button size="sm" variant="destructive" className="h-7 flex-1 px-1 text-xs" onClick={() => { clearVideo(); subtitleStore.setFile(null); handleCloseVideo(); }}>
                    <X className="h-3.5 w-3.5 mr-0.5" />
                    {probeResult ? t("video.closeVideo") : subtitleStore.file ? t("subtitle.closeSubtitle", "关闭字幕") : t("common.close", "关闭")}
                  </Button>
                  <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={() => navigate("/settings")}>
                    <SettingsIcon className="h-3.5 w-3.5 mr-0.5" />
                    {t("menu.systemSettings")}
                  </Button>
                </div>
                <div className="flex gap-1">
                  {probeResult ? (
                    <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={handleImportSubtitle}>
                      <Upload className="h-3.5 w-3.5 mr-0.5" />
                      {t("menu.importSubtitle")}
                    </Button>
                  ) : (
                    <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={handleOpenVideo}>
                      <Film className="h-3.5 w-3.5 mr-0.5" />
                      {t("menu.openVideo")}
                    </Button>
                  )}
                  <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={() => setSearchOpen(true)}>
                    <Search className="h-3.5 w-3.5 mr-0.5" />
                    {t("search.title")}
                  </Button>
                </div>
              </CardContent>
            </Card>
          ) : null}

          {/* 字幕文件信息（纯字幕模式） */}
          {subtitleStore.file && !probeResult && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="flex items-center gap-1 text-sm">
                  <FileText className="h-4 w-4" />
                  {subtitleStore.file.source_path?.split(/[\\/]/).pop() ?? "subtitle"}
                </CardTitle>
              </CardHeader>
              <CardContent className="text-xs text-muted-foreground">
                <div>{t("subtitle.format")}: {subtitleStore.file.format}</div>
                <div>{t("subtitle.count")}: {subtitleStore.file.entries.length}</div>
              </CardContent>
            </Card>
          )}

          {/* 字幕操作区 */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("video.translateSubtitle", "翻译字幕")}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              {/* 来源语言：默认从字幕流语言推断，无法识别时为 auto */}
              <div className="flex items-center gap-2">
                <label className="w-12 text-xs text-muted-foreground flex-shrink-0">{t("translate.sourceLang", "来源语言")}</label>
                <SearchableLangSelect
                  value={translateStore.sourceLang}
                  onChange={translateStore.setSourceLang}
                  options={SOURCE_LANG_OPTIONS}
                />
              </div>

              {/* 翻译目标语言：label 和下拉框同一行 */}
              <div className="flex items-center gap-2">
                <label className="w-12 text-xs text-muted-foreground flex-shrink-0">{t("translate.targetLang")}</label>
                <SearchableLangSelect
                  value={translateStore.targetLang}
                  onChange={translateStore.setTargetLang}
                  options={TARGET_LANG_OPTIONS}
                />
              </div>

              {/* 翻译引擎 + AI 模型 下拉框 */}
              <div className="flex items-center gap-2">
                <label className="text-xs text-muted-foreground flex-shrink-0">{t("translate.engine")}</label>
                <Select
                  value={translateStore.provider === "openai" && translateStore.model
                    ? encodeAiSelectValue(translateStore.serviceId || "openai", translateStore.model)
                    : translateStore.provider === "openai" ? "" : (translateStore.provider || undefined)}
                  onValueChange={(val) => {
                    if (val === "__add_more__") {
                      navigate("/settings?tab=translate");
                      return;
                    }
                    const decoded = decodeAiSelectValue(val);
                    if (decoded) {
                      // AI 模型
                      const { serviceId, model } = decoded;
                      translateStore.setProvider("openai");
                      translateStore.setServiceId(serviceId);
                      translateStore.setModel(model);
                      const found = aiServiceModels.find((m) => m.serviceId === serviceId && m.model === model);
                      translateStore.setModelType(found?.modelType || "generic");
                      // 持久化
                      api.setConfig("translate_provider", "openai").catch(() => {});
                      api.setConfig("translate_openai_service_id", serviceId).catch(() => {});
                      api.setConfig("translate_current_model", model).catch(() => {});
                    } else {
                      // 传统翻译
                      translateStore.setProvider(val);
                      translateStore.setServiceId(null);
                      translateStore.setModel("");
                      // 持久化
                      api.setConfig("translate_provider", val).catch(() => {});
                      api.setConfig("translate_openai_service_id", "").catch(() => {});
                      api.setConfig("translate_current_model", "").catch(() => {});
                    }
                  }}
                >
                  <SelectTrigger className="h-8 text-xs flex-1 overflow-hidden">
                    <SelectValue placeholder={t("translate.noEngineAvailable", "无可用引擎")} className="truncate min-w-0 text-muted-foreground" />
                  </SelectTrigger>
                  <SelectContent>
                    {/* 传统引擎：只显示已配置或当前选中的 */}
                    {providerConfigured["baidu"] && (
                      <SelectItem value="baidu">{t("settings.baidu")}</SelectItem>
                    )}
                    {providerConfigured["bing"] && (
                      <SelectItem value="bing">Bing</SelectItem>
                    )}
                    {providerConfigured["google"] && (
                      <SelectItem value="google">Google</SelectItem>
                    )}
                    {/* AI 模型：遍历所有已配置的 AI 服务 */}
                    {aiServiceModels.map((m) => {
                      const value = encodeAiSelectValue(m.serviceId, m.model);
                      return (
                        <SelectItem key={value} value={value}>
                          <span className="block truncate" title={`AI模型 - ${m.serviceName} - ${m.model}`}>
                            AI模型 - {m.serviceName} - {m.model}
                          </span>
                        </SelectItem>
                      );
                    })}
                    {/* 添加更多引擎 */}
                    <SelectItem value="__add_more__">
                      <span className="flex items-center gap-1 text-primary">
                        <Plus className="h-3 w-3" />
                        {t("translate.addMoreEngines", "添加更多引擎")}
                      </span>
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              {/* 翻译按钮 / 停止按钮 */}
              {translateStore.translating ? (
                <Button
                  size="sm"
                  variant="destructive"
                  className="w-full"
                  onClick={() => translateStore.cancelTranslate()}
                >
                  <Square className="mr-1 h-4 w-4" />
                  {t("translate.stop")}
                </Button>
              ) : nameExtracting ? (
                <Button
                  size="sm"
                  variant="destructive"
                  className="w-full"
                  onClick={() => {
                    nameExtractCancelledRef.current = true;
                    translateStore.cancelTranslate();
                    translateStore.setExtractingNames(false);
                    glossaryConfirmedRef.current = false;
                    translateStore.setGlossaryDialogOpen(false);
                  }}
                >
                  <Square className="mr-1 h-4 w-4" />
                  {t("translate.nameExtracting", "正在预扫描提取人名...")}
                </Button>
              ) : (
                <Button
                  size="sm"
                  className="w-full"
                  onClick={handleTranslateAndMerge}
                  disabled={!subtitleStore.file || !translateStore.provider}
                  title={!translateStore.provider ? t("translate.noEngineAvailable", "未配置翻译引擎") : undefined}
                >
                  {t("translate.translate")}
                </Button>
              )}

              {/* 开发模式：翻译统计 */}
              {devModeEnabled && translateStore.lastTranslateTime > 0 && (
                <div className="text-[10px] text-muted-foreground space-y-0.5 py-1 border-t border-border/50 pt-2">
                  <div>
                    耗时: {(translateStore.lastTranslateTime / 1000).toFixed(2)}秒
                  </div>
                  <div>
                    字数: {translateStore.lastTranslateChars.toLocaleString()}
                  </div>
                  {translateStore.lastTranslateTokens && (
                    <div>
                      Token: {translateStore.lastTranslateTokens.toLocaleString()}
                    </div>
                  )}
                </div>
              )}

              {/* 专有名词精译：提取后自动翻译开关（仅 AI 引擎 + 精译启用 + 已点击翻译 + 未在翻译/提取中时显示） */}
              {autoTranslateLoaded && translateClicked && namePrecisionEnabled && translateStore.provider === "openai" && !translateStore.translating && !translateStore.extractingNames && !glossaryDialogOpen && (
                <label className="flex items-center gap-2 text-xs text-muted-foreground cursor-pointer select-none py-0.5">
                  <input
                    type="checkbox"
                    checked={autoTranslateAfterExtract}
                    onChange={(e) => {
                      const checked = e.target.checked;
                      setAutoTranslateAfterExtract(checked);
                      autoTranslateAfterExtractRef.current = checked;
                      api.setConfig("auto_translate_after_extract", String(checked)).catch(() => {});
                    }}
                    className="h-3.5 w-3.5 rounded border-gray-300 accent-primary flex-shrink-0"
                  />
                  <span>{t("translate.autoTranslateAfterExtract", "提取完毕后自动翻译字幕")}</span>
                </label>
              )}

              {/* 翻译进度 */}
              {translateStore.translating && (
                <div className="space-y-1">
                  <div className="flex justify-between text-xs">
                    <span>{t("translate.progress")}</span>
                    <span>{translateStore.progress} / {translateStore.total}</span>
                  </div>
                  <Progress value={(translateStore.progress / translateStore.total) * 100} />
                  <div className="flex justify-between text-[10px] text-muted-foreground">
                    <span>{translateStore.totalChars.toLocaleString()} 字</span>
                    <span>{translateStore.speed > 0 ? `${translateStore.speed.toFixed(0)} 字/秒` : "计算中..."}</span>
                    <span>{translateStore.eta > 0 ? `剩余 ${formatEta(translateStore.eta)}` : ""}</span>
                  </div>
                </div>
              )}

              {/* 人名预扫描进度 */}
              {nameExtracting && translateStore.extractNamesTotal > 0 && (
                <div className="space-y-1">
                  <div className="flex justify-between text-xs">
                    <span>{t("translate.nameExtracting", "正在预扫描提取人名...")}</span>
                    <span>{translateStore.extractNamesProgress} / {translateStore.extractNamesTotal}</span>
                  </div>
                  <Progress value={(translateStore.extractNamesProgress / translateStore.extractNamesTotal) * 100} />
                  <div className="flex justify-between text-[10px] text-muted-foreground">
                    <span>{translateStore.extractNamesSpeed > 0 ? `${translateStore.extractNamesSpeed.toFixed(1)} 段/秒` : "计算中..."}</span>
                    <span>{translateStore.extractNamesEta > 0 ? `剩余 ${formatEta(translateStore.extractNamesEta)}` : ""}</span>
                  </div>
                  {/* 提取后自动翻译 checkbox */}
                  {autoTranslateLoaded && (
                    <label className="flex items-center gap-2 text-xs text-muted-foreground cursor-pointer select-none py-0.5">
                      <input
                        type="checkbox"
                        checked={autoTranslateAfterExtract}
                        onChange={(e) => {
                          const checked = e.target.checked;
                          setAutoTranslateAfterExtract(checked);
                          autoTranslateAfterExtractRef.current = checked;
                          api.setConfig("auto_translate_after_extract", String(checked)).catch(() => {});
                        }}
                        className="h-3.5 w-3.5 rounded border-gray-300 accent-primary flex-shrink-0"
                      />
                      <span>{t("translate.autoTranslateAfterExtract", "提取完毕后自动翻译字幕")}</span>
                    </label>
                  )}
                </div>
              )}

              {/* 翻译错误 */}
              {translateStore.error && (
                <p className="text-xs text-destructive">{translateStore.error}</p>
              )}
            </CardContent>
          </Card>

          {/* 处理结果列表 */}
          {extractedFiles.length > 0 && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">{t("video.results")}</CardTitle>
              </CardHeader>
              <CardContent className="space-y-1 max-h-32 overflow-auto">
                {extractedFiles.map((f, i) => (
                  <div key={i} className="flex items-center justify-between text-xs">
                    <div className="flex items-center gap-1 min-w-0">
                      <FileText className="h-3 w-3 flex-shrink-0" />
                      <span className="truncate">{f.name}</span>
                    </div>
                    <span className="text-muted-foreground flex-shrink-0 ml-2">{f.status}</span>
                  </div>
                ))}
              </CardContent>
            </Card>
          )}
        </div>
      </main>

      {/* 状态栏 */}
      <footer className="flex items-center justify-between border-t px-4 py-1 text-xs text-muted-foreground">
        <span>{translateStore.translating ? t("translate.progress") : t("common.ready")}</span>
        <span>v1.0.0</span>
      </footer>

      {/* 搜索对话框 */}
      <SearchDialog
        open={searchOpen}
        onOpenChange={setSearchOpen}
        videoName={probeResult?.video_path?.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "")}
      />

      {/* 字幕流编辑弹层 */}
      {probeResult && (
        <SubtitleStreamEditorDialog
          open={streamEditorOpen}
          onOpenChange={setStreamEditorOpen}
          videoPath={probeResult.video_path}
          streams={probeResult.subtitle_streams}
          onSaved={() => { openVideo(probeResult.video_path); }}
        />
      )}

      {/* FFmpeg 下载弹窗 */}
      <FfmpegDownloadDialog
        open={ffmpegDialogOpen}
        onDownloaded={() => {
          setFfmpegDialogOpen(false);
          ffmpegDownloadedRef.current = true;
        }}
        onCancel={() => setFfmpegDialogOpen(false)}
      />

      {/* 人名精译：译名表确认弹窗 */}
      {glossaryDialogOpen && (
        <GlossaryConfirmDialog
          glossary={glossaryDraft}
          onGlossaryChange={(g) => translateStore.setGlossaryDraft(g)}
          autoTranslating={glossaryAutoModeRef.current}
          translateDone={glossaryAutoModeRef.current && glossaryTranslateDone}
          showAutoTranslateCheckbox={true}
          autoTranslateAfterExtract={autoTranslateAfterExtract}
          onAutoTranslateChange={(checked) => {
            setAutoTranslateAfterExtract(checked);
            autoTranslateAfterExtractRef.current = checked;
            api.setConfig("auto_translate_after_extract", String(checked)).catch(() => {});
          }}
          onConfirm={() => {
            glossaryConfirmedRef.current = true;
            translateStore.setGlossaryDialogOpen(false);
            // 自动模式下播放器一直隐藏，弹窗关闭时恢复
            if (glossaryAutoModeRef.current) {
              api.devLog("[MainView] 自动模式弹窗关闭，恢复播放器");
              api.playerShow().catch(() => { /* 播放器未初始化，忽略 */ });
            }
          }}
          onCancel={() => {
            // 自动模式下弹窗只是查看窗口，关闭不中止翻译
            if (glossaryAutoModeRef.current) {
              translateStore.setGlossaryDialogOpen(false);
              api.devLog("[MainView] 自动模式弹窗关闭，恢复播放器");
              api.playerShow().catch(() => { /* 播放器未初始化，忽略 */ });
            } else {
              glossaryConfirmedRef.current = false;
              translateStore.setGlossaryDialogOpen(false);
            }
          }}
        />
      )}
    </div>
  );
}

// === SECTION 3 END ===
