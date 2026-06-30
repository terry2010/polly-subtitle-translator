import { useCallback, useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useNavigate } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize, LogicalPosition } from "@tauri-apps/api/window";
import { Settings as SettingsIcon, Film, FileText, Loader2, Search, Download, Square, X, Upload } from "lucide-react";
import { VideoPlayer } from "../components/VideoPlayer";
import { Button } from "../components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "../components/ui/select";
import { Progress } from "../components/ui/progress";
import { useVideoStore } from "../stores/videoStore";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useTranslateStore } from "../stores/translateStore";
import { api, formatIpcError } from "../lib/api";
import { withPlayerHidden } from "../lib/utils";
import { SubtitlePreviewPanel } from "../components/SubtitlePreviewPanel";
import { SearchDialog } from "../components/SearchDialog";
import { HdrNotice } from "../components/HdrNotice";
import { SubtitleStreamEditorDialog } from "../components/SubtitleStreamEditorDialog";
import { FfmpegDownloadDialog } from "../components/FfmpegDownloadDialog";

// 跟踪窗口大小状态，避免组件卸载重载时丢失（如从设置页返回）
// null = 尚未初始化，true = 大窗口（有文件），false = 小窗口（空状态）
const windowSizeState = { initialized: null as boolean | null };

// 供 SettingsView 卸载时设置
export function setWindowSizeInitialized(v: boolean) {
  windowSizeState.initialized = v;
}

export default function MainView() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { probeResult, loading, error, openVideo, clearVideo, selectedSubtitleStream, selectSubtitleStream } = useVideoStore();
  const subtitleStore = useSubtitleStore();
  const translateStore = useTranslateStore();
  const [extracting, setExtracting] = useState(false);
  const [extractProgress, setExtractProgress] = useState(0);
  const [ffmpegDialogOpen, setFfmpegDialogOpen] = useState(false);
  const ffmpegDownloadedRef = useRef(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [streamEditorOpen, setStreamEditorOpen] = useState(false);
  const [extractedFiles, setExtractedFiles] = useState<{ name: string; path: string; status: string }[]>([]);
  // 提取失败的字幕流 index 集合（用于在列表中标记不可用的流）
  const [failedStreams, setFailedStreams] = useState<Set<number>>(new Set());
  // 导入的外部字幕列表
  const [importedSubtitles, setImportedSubtitles] = useState<{ name: string; path: string }[]>([]);
  // 字幕流提取缓存：streamIndex -> SubtitleFile
  const extractCacheRef = useRef<Map<number, any>>(new Map());
  // 自动提取已执行的 stream index（避免重复提取）
  const autoExtractedRef = useRef<number | null>(null);
  // 从纯字幕模式切到视频模式时，跳过一次自动提取，保留当前编辑的字幕
  const skipAutoExtractRef = useRef(false);
  // 提取取消标志：关闭视频时设为 true，正在进行的提取完成后丢弃结果
  const extractCancelledRef = useRef(false);
  // 各翻译引擎是否已配置凭据
  const [providerConfigured, setProviderConfigured] = useState<Record<string, boolean>>({});
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
        console.error("[handleOpenVideo] 检测 ffmpeg 状态失败:", e);
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
      filters: [{ name: "Subtitle", extensions: ["srt", "ass", "ssa", "vtt"] }],
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
      filters: [{ name: "Subtitle", extensions: ["srt", "ass", "ssa", "vtt"] }],
    });
    const selected = hasPlayer ? await withPlayerHidden(doOpen) : await doOpen();
    if (selected && Array.isArray(selected)) {
      for (const path of selected) {
        const name = path.split(/[\\/]/).pop() ?? path;
        setImportedSubtitles((prev) => [...prev, { name, path }]);
      }
      // 立刻加载第一个导入的字幕
      const firstPath = selected[0];
      await subtitleStore.loadSubtitle(firstPath);
      setSelectedImportedPath(firstPath);
      selectSubtitleStream(null);
    }
  }, [subtitleStore, selectSubtitleStream, withPlayerHidden]);

  // 关闭视频：清除所有视频相关状态（提取缓存、自动提取 ref、导入字幕列表）
  const handleCloseVideo = useCallback(() => {
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
      // 查询翻译缓存
      const { sourceLang, targetLang, provider } = useTranslateStore.getState();
      try {
        const cachedTr = await api.getCachedTranslations(cached.entries, sourceLang, targetLang, provider);
        if (cachedTr && cachedTr.length > 0) {
          const entries = cached.entries.map((e: any) => {
            const tr = cachedTr.find((c) => c.index === e.index);
            return tr ? { ...e, translated: tr.translated } : e;
          });
          subtitleStore.setFile({ ...cached, entries });
        }
      } catch (e) {
        console.warn("查询翻译缓存失败:", e);
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
        console.warn("提取完成但已被取消，丢弃结果");
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
        { name: `${baseName}.${lang}.${ext}`, path: outputPath, status: "已提取" },
      ]);

      // 提取完成后查询翻译缓存，自动填充已翻译的条目
      const subtitleState = useSubtitleStore.getState();
      if (subtitleState.file) {
        const { sourceLang, targetLang, provider } = useTranslateStore.getState();
        try {
          const cached = await api.getCachedTranslations(
            subtitleState.file.entries, sourceLang, targetLang, provider
          );
          if (cached && cached.length > 0) {
            const entries = subtitleState.file.entries.map((e) => {
              const tr = cached.find((c) => c.index === e.index);
              return tr ? { ...e, translated: tr.translated } : e;
            });
            subtitleState.setFile({ ...subtitleState.file, entries });
          }
        } catch (e) {
          console.warn("查询翻译缓存失败:", e);
        }
        // 缓存提取结果
        const finalState = useSubtitleStore.getState();
        if (finalState.file) {
          extractCacheRef.current.set(selectedSubtitleStream.index, finalState.file);
        }
      }
    } catch (e: any) {
      console.error("提取字幕失败:", e);
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
  }, [probeResult, selectedSubtitleStream, subtitleStore]);

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
    const providers = ["baidu", "bing", "google"];
    Promise.all(providers.map(async (p) => {
      try {
        const [appId, secretKeyring, secretConfig] = await Promise.all([
          api.getConfig(`translate_${p}_app_id`).catch(() => null),
          api.getCredential(p, "secret").catch(() => null),
          api.getConfig(`translate_${p}_secret`).catch(() => null),
        ]);
        // app_id 和 secret 都有值才算已配置
        const configured = !!(appId && (secretKeyring || secretConfig));
        return [p, configured] as [string, boolean];
      } catch {
        return [p, false] as [string, boolean];
      }
    })).then((results) => {
      setProviderConfigured(Object.fromEntries(results));
    });
  }, []);

  // === SECTION 1 END ===

  // 窗口大小自动调整：空状态用小窗口，打开文件后放大
  // 同时调整位置保持窗口中心点不变，避免放大后窗口跑出屏幕
  // 首次渲染跳过：Rust setup 已完成初始居中，避免二次定位导致抖动
  // 使用模块级变量，避免组件卸载重载（如从设置页返回）时丢失状态
  const hasFile = !!(probeResult || subtitleStore.file || loading || error);
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
      console.error("播放失败:", e);
    }
  }, [probeResult]);

  // libmpv 播放位置更新回调——用于字幕高亮联动
  const [currentPlayTime, setCurrentPlayTime] = useState(0);
  const handlePositionUpdate = useCallback((posSec: number, _durSec: number, _paused: boolean) => {
    setCurrentPlayTime(posSec);
  }, []);

  const handleTranslateAndMerge = useCallback(async () => {
    if (!subtitleStore.file) return;
    // 逐条翻译、逐条填充
    const result = await translateStore.startTranslate(
      subtitleStore.file.entries,
      (index, translated, failed) => {
        // 每条翻译完成后立即更新字幕预览区（含翻译失败标记）
        subtitleStore.updateEntry(index, { translated, failed });
      }
    );
    if (result && result.translations.length > 0) {
      // 确保所有结果都更新（包括可能遗漏的），同步 failed 标记
      const entries = subtitleStore.file.entries.map((e) => {
        const tr = result.translations.find((r) => r.index === e.index);
        if (!tr) return e;
        // 已有译文且非失败的保留，否则用结果覆盖（含 failed）
        if (e.translated && !e.failed) return e;
        return { ...e, translated: tr.translated, failed: tr.failed };
      });
      subtitleStore.setFile({ ...subtitleStore.file, entries });
    }
  }, [subtitleStore, translateStore]);

  const formatDuration = (s: number | null) => {
    if (!s) return "--";
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = Math.floor(s % 60);
    return `${h}:${m.toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}`;
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
  if (!probeResult && !loading && !error && !subtitleStore.file) {
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
            <div className="w-44" />
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

          {/* 播放预览区（仅视频模式） */}
          {probeResult && (
            <div className="flex-shrink-0 border-b">
              {/* 视频预览 + 内嵌字幕列表 横向排列 */}
              {/* items-start：不让字幕列表拉伸到与播放器等高；播放器自身视频区已限制 40vh，
                  播控条在视频区下方正常显示，不被 overflow-hidden 裁剪 */}
              <div className="flex gap-0 items-start">
                {/* 视频预览（libmpv 内嵌播放）。不要再套 max-h-[40vh] overflow-hidden，
                    否则会把 VideoPlayer 下方的播控条裁掉（视频区已自行限制 40vh） */}
                <div className="flex-1 min-w-0">
                  <VideoPlayer
                    probeResult={probeResult}
                    onPositionUpdate={handlePositionUpdate}
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
                        onClick={() => { selectSubtitleStream(stream); setSelectedImportedPath(null); }}
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
          {/* 视频信息卡 */}
          {probeResult && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="flex items-center gap-1 text-sm">
                  <Film className="h-4 w-4" />
                  {videoFileName}
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-1 text-xs text-muted-foreground">
                <div>{t("video.duration")}: {formatDuration(probeResult.format.duration)} · {formatSize(probeResult.format.size)}</div>
                {probeResult.video_stream && (
                  <div>{probeResult.video_stream.width}x{probeResult.video_stream.height} {probeResult.video_stream.codec_name}</div>
                )}
                <div>{t("video.audioStreams")}: {probeResult.audio_streams.map(s => s.language ?? "??").join(", ")}</div>
              </CardContent>
            </Card>
          )}

          {/* 快捷操作卡（视频模式）：关闭视频 / 系统设置 / 导入字幕 / 搜索 */}
          {probeResult && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">{t("video.quickOps")}</CardTitle>
              </CardHeader>
              <CardContent className="space-y-1.5">
                <div className="flex gap-1">
                  <Button size="sm" variant="destructive" className="h-7 flex-1 px-1 text-xs" onClick={() => { clearVideo(); subtitleStore.setFile(null); handleCloseVideo(); }}>
                    <X className="h-3.5 w-3.5 mr-0.5" />
                    {t("video.closeVideo")}
                  </Button>
                  <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={() => navigate("/settings")}>
                    <SettingsIcon className="h-3.5 w-3.5 mr-0.5" />
                    {t("menu.systemSettings")}
                  </Button>
                </div>
                <div className="flex gap-1">
                  <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={handleImportSubtitle}>
                    <Upload className="h-3.5 w-3.5 mr-0.5" />
                    {t("menu.importSubtitle")}
                  </Button>
                  <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={() => setSearchOpen(true)}>
                    <Search className="h-3.5 w-3.5 mr-0.5" />
                    {t("search.title")}
                  </Button>
                </div>
              </CardContent>
            </Card>
          )}

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

          {/* 快捷操作卡（纯字幕模式）：关闭字幕 / 系统设置 / 打开视频 / 字幕搜索 */}
          {subtitleStore.file && !probeResult && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">{t("video.quickOps")}</CardTitle>
              </CardHeader>
              <CardContent className="space-y-1.5">
                <div className="flex gap-1">
                  <Button size="sm" variant="destructive" className="h-7 flex-1 px-1 text-xs" onClick={() => { subtitleStore.setFile(null); }}>
                    <X className="h-3.5 w-3.5 mr-0.5" />
                    {t("subtitle.closeSubtitle")}
                  </Button>
                  <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={() => navigate("/settings")}>
                    <SettingsIcon className="h-3.5 w-3.5 mr-0.5" />
                    {t("menu.systemSettings")}
                  </Button>
                </div>
                <div className="flex gap-1">
                  <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={handleOpenVideo}>
                    <Film className="h-3.5 w-3.5 mr-0.5" />
                    {t("menu.openVideo")}
                  </Button>
                  <Button size="sm" variant="ghost" className="h-7 flex-1 px-1 text-xs" onClick={() => setSearchOpen(true)}>
                    <Search className="h-3.5 w-3.5 mr-0.5" />
                    {t("search.title")}
                  </Button>
                </div>
              </CardContent>
            </Card>
          )}

          {/* 字幕操作区 */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("video.subtitleOps")}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              {/* 翻译目标语言：label 和下拉框同一行 */}
              <div className="flex items-center gap-2">
                <label className="text-xs text-muted-foreground flex-shrink-0">{t("translate.targetLang")}</label>
                <Select
                  value={translateStore.targetLang}
                  onValueChange={translateStore.setTargetLang}
                >
                  <SelectTrigger className="h-8 text-xs flex-1">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="zh">中文</SelectItem>
                    <SelectItem value="en">English</SelectItem>
                    <SelectItem value="ja">日本語</SelectItem>
                    <SelectItem value="ko">한국어</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              {/* 翻译引擎 + API 下拉框 */}
              <div className="flex items-center gap-2">
                <label className="text-xs text-muted-foreground flex-shrink-0">{t("translate.engine")}</label>
                <Select
                  value={translateStore.provider}
                  onValueChange={translateStore.setProvider}
                >
                  <SelectTrigger className="h-8 text-xs flex-1">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="baidu">
                      <span className="flex items-center justify-between w-full">
                        <span>{t("settings.baidu")}</span>
                        {!providerConfigured["baidu"] && (
                          <span
                            className="text-amber-600 ml-2 text-xs cursor-pointer hover:underline"
                            onPointerDownCapture={(e) => { e.preventDefault(); e.stopPropagation(); navigate("/settings?provider=baidu"); }}
                          >
                            {t("common.notConfigured")}
                          </span>
                        )}
                      </span>
                    </SelectItem>
                    <SelectItem value="bing">
                      <span className="flex items-center justify-between w-full">
                        <span>Bing</span>
                        {!providerConfigured["bing"] && (
                          <span
                            className="text-amber-600 ml-2 text-xs cursor-pointer hover:underline"
                            onPointerDownCapture={(e) => { e.preventDefault(); e.stopPropagation(); navigate("/settings?provider=bing"); }}
                          >
                            {t("common.notConfigured")}
                          </span>
                        )}
                      </span>
                    </SelectItem>
                    <SelectItem value="google">
                      <span className="flex items-center justify-between w-full">
                        <span>Google</span>
                        {!providerConfigured["google"] && (
                          <span
                            className="text-amber-600 ml-2 text-xs cursor-pointer hover:underline"
                            onPointerDownCapture={(e) => { e.preventDefault(); e.stopPropagation(); navigate("/settings?provider=google"); }}
                          >
                            {t("common.notConfigured")}
                          </span>
                        )}
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
              ) : (
                <Button
                  size="sm"
                  className="w-full"
                  onClick={handleTranslateAndMerge}
                  disabled={!subtitleStore.file}
                >
                  {t("translate.translate")}
                </Button>
              )}

              {/* 翻译进度 */}
              {translateStore.translating && (
                <div className="space-y-1">
                  <div className="flex justify-between text-xs">
                    <span>{t("translate.progress")}</span>
                    <span>{translateStore.progress} / {translateStore.total}</span>
                  </div>
                  <Progress value={(translateStore.progress / translateStore.total) * 100} />
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
    </div>
  );
}

// === SECTION 3 END ===
