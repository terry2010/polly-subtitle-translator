import { useCallback, useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Settings as SettingsIcon, FolderOpen, Film, FileText, Loader2, Search, Merge, Download, ArrowLeft, Square, X, Upload } from "lucide-react";
import { VideoPlayer } from "../components/VideoPlayer";
import { Button } from "../components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "../components/ui/select";
import { Progress } from "../components/ui/progress";
import { useVideoStore } from "../stores/videoStore";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useTranslateStore } from "../stores/translateStore";
import { api } from "../lib/api";
import { SubtitlePreviewPanel } from "../components/SubtitlePreviewPanel";
import { SearchDialog } from "../components/SearchDialog";
import { HdrNotice } from "../components/HdrNotice";

export default function MainView() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { probeResult, loading, error, openVideo, clearVideo, selectedSubtitleStream, selectSubtitleStream } = useVideoStore();
  const subtitleStore = useSubtitleStore();
  const translateStore = useTranslateStore();
  const [extracting, setExtracting] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [merging, setMerging] = useState(false);
  const [autoMerge, setAutoMerge] = useState(false);
  const [extractedFiles, setExtractedFiles] = useState<{ name: string; path: string; status: string }[]>([]);
  // 导入的外部字幕列表
  const [importedSubtitles, setImportedSubtitles] = useState<{ name: string; path: string }[]>([]);
  // 字幕流提取缓存：streamIndex -> SubtitleFile
  const extractCacheRef = useRef<Map<number, any>>(new Map());
  // 各翻译引擎是否已配置凭据
  const [providerConfigured, setProviderConfigured] = useState<Record<string, boolean>>({});
  // 当前选中的导入字幕路径（用于高亮）
  const [selectedImportedPath, setSelectedImportedPath] = useState<string | null>(null);

  const handleOpenVideo = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Video", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "ts", "m2ts"] }],
    });
    if (typeof selected === "string") {
      await openVideo(selected);
    }
  }, [openVideo]);

  const handleOpenSubtitle = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Subtitle", extensions: ["srt", "ass", "ssa", "vtt"] }],
    });
    if (typeof selected === "string") {
      await subtitleStore.loadSubtitle(selected);
    }
  }, [subtitleStore]);

  // 导入外部字幕（添加到导入列表并立刻加载第一个）
  const handleImportSubtitle = useCallback(async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Subtitle", extensions: ["srt", "ass", "ssa", "vtt"] }],
    });
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
  }, [subtitleStore, selectSubtitleStream]);

  // 清除提取缓存
  const clearExtractCache = useCallback(() => {
    extractCacheRef.current.clear();
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
    try {
      const baseName = probeResult.video_path.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
      const lang = selectedSubtitleStream.language ?? "sub";
      // 写到系统临时目录，避免 Vite 监听项目目录变化触发页面刷新
      const tempDir = await import("@tauri-apps/api/path").then(m => m.tempDir());
      const outputPath = `${tempDir}${baseName}.${lang}.srt`;
      await api.extractSubtitle(probeResult.video_path, selectedSubtitleStream.index, outputPath);
      await subtitleStore.loadSubtitle(outputPath);
      setExtractedFiles((prev) => [
        ...prev,
        { name: `${baseName}.${lang}.srt`, path: outputPath, status: "已提取" },
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
      const msg = e?.message ?? e?.code ?? String(e);
      setExtractedFiles((prev) => [
        ...prev,
        { name: "提取失败", path: "", status: msg },
      ]);
    } finally {
      setExtracting(false);
    }
  }, [probeResult, selectedSubtitleStream, subtitleStore]);

  // 自动提取字幕：当 selectedSubtitleStream 首次被设置时自动提取
  const autoExtractedRef = useRef<number | null>(null);
  const handleExtractRef = useRef(handleExtractSubtitle);
  handleExtractRef.current = handleExtractSubtitle;
  useEffect(() => {
    if (!probeResult || !selectedSubtitleStream) return;
    if (autoExtractedRef.current === selectedSubtitleStream.index) return;
    autoExtractedRef.current = selectedSubtitleStream.index;
    handleExtractRef.current();
  }, [probeResult, selectedSubtitleStream]);

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

  const handleMergeSubtitle = useCallback(async () => {
    if (!probeResult || !subtitleStore.file) return;
    setMerging(true);
    try {
      const baseName = probeResult.video_path.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
      const tempDir = await import("@tauri-apps/api/path").then(m => m.tempDir());
      const tempSub = `${tempDir}${baseName}.zh.srt`;
      await subtitleStore.saveSubtitle(tempSub);
      const outputPath = `${tempDir}${baseName}.merged.mkv`;
      await api.mergeSubtitle(probeResult.video_path, tempSub, outputPath, "zh");
      setExtractedFiles((prev) => [
        ...prev,
        { name: `${baseName}.merged.mkv`, path: outputPath, status: "已合并" },
      ]);
    } catch (e: any) {
      console.error("合并字幕失败:", e);
      const msg = e?.message ?? e?.code ?? String(e);
      setExtractedFiles((prev) => [
        ...prev,
        { name: "合并失败", path: "", status: msg },
      ]);
    } finally {
      setMerging(false);
    }
  }, [probeResult, subtitleStore]);

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
      (index, translated) => {
        // 每条翻译完成后立即更新字幕预览区
        subtitleStore.updateEntry(index, { translated });
      }
    );
    if (result && result.translations.length > 0) {
      // 确保所有结果都更新（包括可能遗漏的）
      const entries = subtitleStore.file.entries.map((e) => {
        const tr = result.translations.find((r) => r.index === e.index);
        return tr && !e.translated ? { ...e, translated: tr.translated } : e;
      });
      subtitleStore.setFile({ ...subtitleStore.file, entries });

      // 自动合并
      if (autoMerge && probeResult) {
        try {
          const baseName = probeResult.video_path.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
          const tempDir = await import("@tauri-apps/api/path").then(m => m.tempDir());
          const tempSub = `${tempDir}${baseName}.zh.srt`;
          await subtitleStore.saveSubtitle(tempSub);
          const outputPath = `${tempDir}${baseName}.merged.mkv`;
          await api.mergeSubtitle(probeResult.video_path, tempSub, outputPath, "zh");
          setExtractedFiles((prev) => [
            ...prev,
            { name: `${baseName}.merged.mkv`, path: outputPath, status: "已翻译合并" },
          ]);
        } catch (e: any) {
          console.error("自动合并失败:", e);
        }
      }
    }
  }, [subtitleStore, translateStore, autoMerge, probeResult]);

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
        <header className="flex items-center justify-between border-b px-4 py-2">
          <h1 className="text-lg font-semibold">{t("app.title")}</h1>
          <Button variant="ghost" size="sm" onClick={() => navigate("/settings")}>
            <SettingsIcon className="mr-1 h-4 w-4" />
            {t("menu.settings")}
          </Button>
        </header>
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
          <p className="text-sm text-muted-foreground">
            {t("app.dragHint", "或将文件拖入此窗口")} · mkv mp4 avi mov / srt ass vtt
          </p>
        </div>
        <SearchDialog open={searchOpen} onOpenChange={setSearchOpen} />
      </div>
    );
  }

  return (
    <div className="flex h-screen flex-col">
      {/* 顶栏 */}
      <header className="flex items-center justify-between border-b px-4 py-2">
        <div className="flex items-center gap-3">
          {subtitleStore.file && !probeResult && (
            <Button variant="ghost" size="sm" onClick={() => { subtitleStore.setFile(null); }}>
              <ArrowLeft className="mr-1 h-4 w-4" />
              {t("common.back")}
            </Button>
          )}
          <h1 className="text-lg font-semibold truncate max-w-md">
            {videoFileName || (subtitleStore.file?.source_path?.split(/[\\/]/).pop() ?? t("app.title"))}
          </h1>
        </div>
        <div className="flex gap-1">
          <Button variant="ghost" size="sm" onClick={handleOpenVideo}>
            <FolderOpen className="mr-1 h-4 w-4" />
            {t("menu.openVideo")}
          </Button>
          <Button variant="ghost" size="sm" onClick={handleOpenSubtitle}>
            <FileText className="mr-1 h-4 w-4" />
            {t("menu.openSubtitle")}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setSearchOpen(true)}>
            <Search className="mr-1 h-4 w-4" />
            {t("search.title")}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => navigate("/settings")}>
            <SettingsIcon className="h-4 w-4" />
          </Button>
        </div>
      </header>

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
                  <div className="px-3 py-1.5 border-b text-xs font-medium flex-shrink-0">
                    {t("video.subtitleStreams", "内嵌字幕")} ({probeResult.subtitle_streams.length})
                  </div>
                  <div className="overflow-auto p-1.5 space-y-1 flex-1">
                    {probeResult.subtitle_streams.length === 0 && (
                      <p className="text-xs text-muted-foreground px-1 py-2">{t("video.noSubtitle", "无内嵌字幕")}</p>
                    )}
                    {probeResult.subtitle_streams.map((stream) => (
                      <button
                        key={stream.index}
                        onClick={() => { selectSubtitleStream(stream); setSelectedImportedPath(null); }}
                        className={`w-full text-left rounded px-2 py-1.5 text-xs transition-colors flex items-center gap-2 ${
                          selectedSubtitleStream?.index === stream.index
                            ? "bg-primary text-primary-foreground"
                            : "hover:bg-accent"
                        }`}
                        disabled={stream.is_graphic}
                      >
                        <span className={`w-2 h-2 rounded-full ${selectedSubtitleStream?.index === stream.index ? "bg-primary-foreground" : "bg-muted-foreground/40"}`} />
                        <span className="font-mono">#{stream.index}</span>
                        <span>{stream.language ?? "??"}</span>
                        {stream.disposition_forced && <span className="opacity-60">forced</span>}
                        {stream.disposition_hearing_impaired && <span className="opacity-60">SDH</span>}
                        {stream.is_graphic && <span className="opacity-60">(graphic)</span>}
                      </button>
                    ))}
                  </div>
                  {/* 导入的外部字幕列表 */}
                  {importedSubtitles.length > 0 && (
                    <div className="border-t flex-shrink-0">
                      <div className="px-3 py-1 border-b text-xs font-medium bg-muted/30">
                        {t("subtitle.imported", "导入字幕")} ({importedSubtitles.length})
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
                  {/* 操作按钮栏：两行 */}
                  <div className="border-t flex flex-col gap-0.5 p-1 flex-shrink-0">
                    <div className="flex gap-0.5">
                      <Button size="sm" variant="ghost" className="h-6 flex-1 px-1 text-xs" onClick={() => { clearVideo(); subtitleStore.setFile(null); clearExtractCache(); }}>
                        <X className="h-3 w-3 mr-0.5" />
                        {t("video.closeVideo", "关闭视频")}
                      </Button>
                      <Button size="sm" variant="ghost" className="h-6 flex-1 px-1 text-xs" onClick={() => navigate("/settings")}>
                        <SettingsIcon className="h-3 w-3 mr-0.5" />
                        {t("menu.systemSettings", "系统设置")}
                      </Button>
                    </div>
                    <div className="flex gap-0.5">
                      <Button size="sm" variant="ghost" className="h-6 flex-1 px-1 text-xs" onClick={handleImportSubtitle}>
                        <Upload className="h-3 w-3 mr-0.5" />
                        {t("menu.importSubtitle", "导入字幕")}
                      </Button>
                      <Button size="sm" variant="ghost" className="h-6 flex-1 px-1 text-xs" onClick={() => setSearchOpen(true)}>
                        <Search className="h-3 w-3 mr-0.5" />
                        {t("search.title", "搜索")}
                      </Button>
                    </div>
                  </div>
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
            <SubtitlePreviewPanel extracting={extracting} currentPlayTime={currentPlayTime} />
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
                <div>{t("video.duration", "时长")}: {formatDuration(probeResult.format.duration)} · {formatSize(probeResult.format.size)}</div>
                {probeResult.video_stream && (
                  <div>{probeResult.video_stream.width}x{probeResult.video_stream.height} {probeResult.video_stream.codec_name}</div>
                )}
                <div>{t("video.audioStreams", "音轨")}: {probeResult.audio_streams.map(s => s.language ?? "??").join(", ")}</div>
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
                <div>{t("subtitle.format", "格式")}: {subtitleStore.file.format}</div>
                <div>{t("subtitle.count", "条目数")}: {subtitleStore.file.entries.length}</div>
              </CardContent>
            </Card>
          )}

          {/* 字幕操作区 */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("video.subtitleOps", "字幕操作")}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              {/* 翻译目标语言：label 和下拉框同一行 */}
              <div className="flex items-center gap-2">
                <label className="text-xs text-muted-foreground flex-shrink-0">{t("translate.targetLang", "目标")}</label>
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
                <label className="text-xs text-muted-foreground flex-shrink-0">{t("translate.engine", "翻译引擎")}</label>
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
                        <span>百度</span>
                        {!providerConfigured["baidu"] && (
                          <span
                            className="text-amber-600 ml-2 text-xs cursor-pointer hover:underline"
                            onPointerDownCapture={(e) => { e.preventDefault(); e.stopPropagation(); navigate("/settings?provider=baidu"); }}
                          >
                            待配置
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
                            待配置
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
                            待配置
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
                  {t("translate.stop", "停止翻译")}
                </Button>
              ) : (
                <Button
                  size="sm"
                  className="w-full"
                  onClick={handleTranslateAndMerge}
                  disabled={!subtitleStore.file}
                >
                  {t("translate.translate", "翻译字幕")}
                </Button>
              )}

              {/* 自动合并 checkbox */}
              {probeResult && (
                <label className="flex items-center gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={autoMerge}
                    onChange={(e) => setAutoMerge(e.target.checked)}
                    className="rounded"
                  />
                  {t("video.autoMerge", "翻译完成后自动合并字幕到视频")}
                </label>
              )}

              {/* 手动合并按钮 */}
              {probeResult && subtitleStore.file && (
                <Button
                  size="sm"
                  variant="outline"
                  className="w-full"
                  onClick={handleMergeSubtitle}
                  disabled={merging}
                >
                  {merging ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : <Merge className="mr-1 h-4 w-4" />}
                  {t("video.merge", "合并字幕到视频")}
                </Button>
              )}

              {/* 翻译进度 */}
              {translateStore.translating && (
                <div className="space-y-1">
                  <div className="flex justify-between text-xs">
                    <span>{t("translate.progress", "翻译中")}</span>
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
                <CardTitle className="text-sm">{t("video.results", "处理结果")}</CardTitle>
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
        <span>{translateStore.translating ? t("translate.progress") : t("common.ready", "就绪")}</span>
        <span>v1.0.0</span>
      </footer>

      {/* 搜索对话框 */}
      <SearchDialog
        open={searchOpen}
        onOpenChange={setSearchOpen}
        videoName={probeResult?.video_path?.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "")}
      />
    </div>
  );
}

// === SECTION 3 END ===
