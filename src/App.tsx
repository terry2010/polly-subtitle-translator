import { useEffect, lazy, Suspense } from "react";
import { HashRouter, Routes, Route } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import MainView from "./views/MainView";
// 路由级懒加载：SettingsView 不在首屏加载，减小首屏 JS 体积
const SettingsView = lazy(() => import("./views/SettingsView"));
import { useThemeStore } from "./stores/themeStore";
import { useVideoStore } from "./stores/videoStore";
import { useSubtitleStore } from "./stores/subtitleStore";
import { useTranslateStore } from "./stores/translateStore";
import { useDevModeStore } from "./stores/devModeStore";
import { useLibmpvStore } from "./stores/libmpvStore";
import { useFfmpegStore } from "./stores/ffmpegStore";
import { useUpdateStore } from "./stores/updateStore";
import { UpdateDialog } from "./components/UpdateDialog";
import { api } from "./lib/api";
import { Toaster } from "sonner";

export default function App() {
  const { i18n } = useTranslation();
  const theme = useThemeStore((s) => s.theme);
  const lang = useThemeStore((s) => s.language);
  const openVideo = useVideoStore((s) => s.openVideo);
  const loadSubtitle = useSubtitleStore((s) => s.loadSubtitle);
  const setFile = useSubtitleStore((s) => s.setFile);
  const startTranslate = useTranslateStore((s) => s.startTranslate);

  useEffect(() => {
    const root = document.documentElement;
    if (theme === "dark") root.classList.add("dark");
    else root.classList.remove("dark");
  }, [theme]);

  useEffect(() => {
    void i18n.changeLanguage(lang);
  }, [lang, i18n]);

  // 启动时检查开发者模式重启计数（开启后重启 3 次自动关闭）
  const initDevMode = useDevModeStore((s) => s.initOnStartup);
  useEffect(() => { void initDevMode(); }, [initDevMode]);

  // 全局监听 libmpv 下载进度事件（路由切换时组件卸载也不丢失）
  const onLibmpvProgress = useLibmpvStore((s) => s.onProgressEvent);
  const refreshLibmpvStatus = useLibmpvStore((s) => s.refreshStatus);
  useEffect(() => {
    const unlisten = listen<{
      stage: string;
      progress: number;
      message?: string;
      speed_mbps?: number;
      eta_secs?: number;
    }>("libmpv_download_progress", (event) => {
      onLibmpvProgress(event.payload);
    });
    // 启动时刷新 libmpv 安装状态
    void refreshLibmpvStatus();
    return () => { unlisten.then((fn) => fn()); };
  }, [onLibmpvProgress, refreshLibmpvStatus]);

  // 全局监听 ffmpeg 下载进度事件
  const onFfmpegProgress = useFfmpegStore((s) => s.onProgressEvent);
  const refreshFfmpegStatus = useFfmpegStore((s) => s.refreshStatus);
  useEffect(() => {
    const unlisten = listen<{
      stage: string;
      progress: number;
      message?: string;
      speed_mbps?: number;
      eta_secs?: number;
      code?: string;
      args?: Record<string, unknown>;
    }>("ffmpeg_download_progress", (event) => {
      onFfmpegProgress(event.payload);
    });
    void refreshFfmpegStatus();
    return () => { unlisten.then((fn) => fn()); };
  }, [onFfmpegProgress, refreshFfmpegStatus]);

  // 普通模式下拦截调试窗口快捷键（F12 / Ctrl+Shift+I / Ctrl+Shift+J / Ctrl+U）
  // 开发模式下放行
  const devMode = useDevModeStore((s) => s.devMode);
  useEffect(() => {
    if (devMode) return; // 开发模式：不拦截
    const handler = (e: KeyboardEvent) => {
      const isF12 = e.key === "F12";
      const isCtrlShiftI = e.ctrlKey && e.shiftKey && (e.key === "I" || e.key === "i");
      const isCtrlShiftJ = e.ctrlKey && e.shiftKey && (e.key === "J" || e.key === "j");
      const isCtrlU = e.ctrlKey && !e.shiftKey && (e.key === "U" || e.key === "u");
      if (isF12 || isCtrlShiftI || isCtrlShiftJ || isCtrlU) {
        e.preventDefault();
        e.stopPropagation();
      }
    };
    // capture 阶段拦截，确保在 WebView2 内置处理之前执行
    window.addEventListener("keydown", handler, true);
    return () => { window.removeEventListener("keydown", handler, true); };
  }, [devMode]);

  // 监听 CLI 参数事件（右键菜单静默模式）
  useEffect(() => {
    const unlisten = listen<{ mode: string; filePath: string }>("cli-args", async (event) => {
      const { mode, filePath } = event.payload;
      if (!filePath) return;

      if (mode === "edit") {
        // 字幕静默编辑模式
        await loadSubtitle(filePath);
      } else if (mode === "quick") {
        // 视频静默流程：自动提取→翻译→合并
        await openVideo(filePath);
        // 等待 probe 完成后自动处理
        setTimeout(async () => {
          await runQuickMode(filePath);
        }, 1000);
      } else {
        // 无模式，根据文件扩展名判断
        const ext = filePath.split(".").pop()?.toLowerCase();
        if (ext && ["srt", "ass", "ssa", "vtt"].includes(ext)) {
          await loadSubtitle(filePath);
        } else {
          await openVideo(filePath);
        }
      }
    });

    // 静默流程：自动提取第一条文本字幕→翻译→合并
    const runQuickMode = async (videoPath: string) => {
      try {
        const probe = await api.probeVideo(videoPath);
        // 找第一条英文文本字幕
        const subStream = probe.subtitle_streams.find(
          (s) => !s.is_graphic && (s.language === "en" || s.language === "eng")
        );
        if (!subStream) return;

        // 提取字幕到临时文件
        const tempPath = videoPath.replace(/\.[^.]+$/, ".temp.srt");
        await api.extractSubtitle(videoPath, subStream.index, tempPath);
        await loadSubtitle(tempPath);

        // 翻译
        const subtitleState = useSubtitleStore.getState();
        if (subtitleState.file) {
          const result = await startTranslate(subtitleState.file.entries);
          if (result) {
            const entries = subtitleState.file.entries.map((e) => {
              const tr = result.translations.find((r) => r.index === e.index);
              return tr ? { ...e, translated: tr.translated, failed: tr.failed } : e;
            });
            setFile({ ...subtitleState.file, entries });

            // 保存翻译后字幕
            const translatedPath = videoPath.replace(/\.[^.]+$/, ".zh.srt");
            await subtitleState.saveSubtitle(translatedPath);

            // 合并到视频
            const outputPath = videoPath.replace(/\.[^.]+$/, ".merged.mkv");
            await api.mergeSubtitle(videoPath, translatedPath, outputPath, "zh");
          }
        }
      } catch (e) {
        console.error("静默流程失败:", e);
      }
    };

    return () => { void unlisten.then((fn) => fn()); };
  }, [openVideo, loadSubtitle, setFile, startTranslate]);

  // 阻止浏览器默认拖放行为
  useEffect(() => {
    const preventDefault = (e: DragEvent) => { e.preventDefault(); };
    window.addEventListener("dragover", preventDefault);
    window.addEventListener("drop", preventDefault);
    return () => {
      window.removeEventListener("dragover", preventDefault);
      window.removeEventListener("drop", preventDefault);
    };
  }, []);

  // 启动时从 config 初始化翻译默认语言（含跟随系统语言）
  useEffect(() => {
    const initLangs = async () => {
      const followRaw = await api.getConfig("default_target_lang_follow_system");
      const follow = followRaw === null ? true : followRaw === "true";
      if (follow) {
        const sysLang = await api.getSystemLang().catch(() => "zh");
        await api.setConfig("default_target_lang", sysLang);
        useTranslateStore.getState().setTargetLang(sysLang);
      } else {
        const saved = await api.getConfig("default_target_lang");
        if (saved) useTranslateStore.getState().setTargetLang(saved);
      }
      const savedSrc = await api.getConfig("default_source_lang");
      if (savedSrc) useTranslateStore.getState().setSourceLang(savedSrc);
      const savedProvider = await api.getConfig("default_api_provider");
      if (savedProvider) useTranslateStore.getState().setProvider(savedProvider);
    };
    void initLangs();
  }, []);

  // 监听文件拖放事件（用 Rust 端 on_window_event 转发，比前端 onDragDropEvent 更可靠）
  useEffect(() => {
    const VIDEO_EXTS = ["mkv", "mp4", "avi", "mov", "wmv", "flv", "ts", "m2ts"];
    const SUB_EXTS = ["srt", "ass", "ssa", "vtt"];

    const handleFile = (filePath: string) => {
      const ext = filePath.split(".").pop()?.toLowerCase();
      if (!ext) return;
      if (SUB_EXTS.includes(ext)) {
        loadSubtitle(filePath);
      } else if (VIDEO_EXTS.includes(ext)) {
        openVideo(filePath);
      }
    };

    const unlisten = listen<string[]>("app://file-drop", (event) => {
      console.log("[App] app://file-drop received:", event.payload);
      const paths = event.payload;
      if (paths && paths.length > 0) {
        handleFile(paths[0]);
      }
    });

    return () => { void unlisten.then((fn) => fn()); };
  }, [openVideo, loadSubtitle]);

  // 启动时自动检查更新（延迟 5 秒，不阻塞启动）
  const checkOnStartup = useUpdateStore((s) => s.checkOnStartup);
  useEffect(() => {
    const timer = setTimeout(() => { void checkOnStartup(); }, 5000);
    return () => clearTimeout(timer);
  }, [checkOnStartup]);

  // 更新弹窗
  const updateDialogOpen = useUpdateStore((s) => s.dialogOpen);
  const updateInfo = useUpdateStore((s) => s.updateInfo);
  const closeUpdateDialog = useUpdateStore((s) => s.closeDialog);

  return (
    <>
      <HashRouter>
        <Suspense fallback={null}>
          <Routes>
            <Route path="/" element={<MainView />} />
            <Route path="/settings" element={<SettingsView />} />
          </Routes>
        </Suspense>
      </HashRouter>
      <Toaster position="top-right" richColors closeButton />
      {updateInfo && (
        <UpdateDialog
          open={updateDialogOpen}
          version={updateInfo.version}
          notes={updateInfo.notes}
          onClose={closeUpdateDialog}
        />
      )}
    </>
  );
}
