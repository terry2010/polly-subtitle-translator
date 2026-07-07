import { useEffect, lazy, Suspense } from "react";
import { HashRouter, Routes, Route, useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { listen, emit } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import MainView from "./views/MainView";
// 路由级懒加载：SettingsView/BatchView 不在首屏加载，减小首屏 JS 体积
const SettingsView = lazy(() => import("./views/SettingsView"));
const BatchView = lazy(() => import("./views/BatchView"));
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
import { log, error } from "./lib/logger";
import { Toaster } from "sonner";

export default function App() {
  const { i18n } = useTranslation();
  const theme = useThemeStore((s) => s.theme);
  const lang = useThemeStore((s) => s.language);
  const openVideo = useVideoStore((s) => s.openVideo);
  const loadSubtitle = useSubtitleStore((s) => s.loadSubtitle);

  // 根据 theme 设置 dark class；system 模式下跟随系统 prefers-color-scheme
  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      const isDark =
        theme === "dark" ||
        (theme === "system" &&
          window.matchMedia("(prefers-color-scheme: dark)").matches);
      root.classList.toggle("dark", isDark);
    };
    apply();
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
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
        // V2: quick 模式走批量翻译队列
        // 初始化 batchStore（如果尚未初始化），然后提交文件
        try {
          const { useBatchStore } = await import("./stores/batchStore");
          const batchStore = useBatchStore.getState();
          await batchStore.init();
          await batchStore.loadConfig();
          await batchStore.submitFiles([filePath]);
          // 跳转到批量翻译页面
          window.location.hash = "#/batch";
        } catch (e) {
          error("批量翻译提交失败，回退到旧流程:", e);
          // 回退：旧的静默流程
          await openVideo(filePath);
        }
      } else if (mode === "watch") {
        // V2: 文件夹右键菜单 → 添加到批量翻译监视
        try {
          const { useBatchStore } = await import("./stores/batchStore");
          const batchStore = useBatchStore.getState();
          await batchStore.init();
          await batchStore.loadConfig();
          // 启动文件夹监视
          await batchStore.startWatch([filePath], true);
          // 跳转到批量翻译页面
          window.location.hash = "#/batch";
        } catch (e) {
          error("文件夹监视启动失败:", e);
        }
      } else {
        // 无模式，根据文件扩展名判断
        const ext = filePath.split(".").pop()?.toLowerCase();
        if (ext && ["srt", "ass", "ssa", "vtt", "sub"].includes(ext)) {
          await loadSubtitle(filePath);
        } else {
          await openVideo(filePath);
        }
      }
    });

    return () => { void unlisten.then((fn) => fn()); };
  }, [openVideo, loadSubtitle]);

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

  // 禁用 WebView 全局默认右键菜单（capture 阶段 preventDefault，
  // 不影响各组件 onContextMenu 回调执行其自定义菜单）
  // 例外：input/textarea/contenteditable 中允许系统右键菜单（剪切/复制/粘贴）
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) {
        return; // 不阻止，让系统菜单显示
      }
      e.preventDefault();
    };
    window.addEventListener("contextmenu", handler, true);
    return () => { window.removeEventListener("contextmenu", handler, true); };
  }, []);

  // 前端初始加载完成，通知后端可以显示/置顶窗口
  useEffect(() => {
    emit("app://ready", {});
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
      // 注意：provider/serviceId/model 由 MainView 从 db 加载，这里不重复设置
      // 避免用错误的 default_api_provider 覆盖正确的 translate_provider
    };
    void initLangs();
  }, []);

  // 监听文件拖放事件（用 Rust 端 on_window_event 转发，比前端 onDragDropEvent 更可靠）
  useEffect(() => {
    const VIDEO_EXTS = ["mkv", "mp4", "avi", "mov", "wmv", "flv", "ts", "m2ts"];
    const SUB_EXTS = ["srt", "ass", "ssa", "vtt", "sub"];

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
      log("[App] app://file-drop received:", event.payload);
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

  // 开发模式下右键单击 toast 复制内容到剪贴板
  // sonner 的 toast 渲染在固定容器 [data-sonner-toaster] 中，
  // 用全局捕获 contextmenu 事件，命中 toast 元素时复制其文本
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (!useDevModeStore.getState().devMode) return;
      const target = e.target as HTMLElement;
      // 向上查找最近的 toast 元素（sonner 的 toast 有 [data-sonner-toast] 属性）
      const toastEl = target.closest("[data-sonner-toast]") as HTMLElement | null;
      if (!toastEl) return;
      e.preventDefault();
      const text = toastEl.innerText || toastEl.textContent || "";
      if (!text) return;
      // 优先用 navigator.clipboard API，失败时回退到 execCommand('copy')
      // Tauri WebView2 中 navigator.clipboard 可能因安全策略不可用
      const copyToClipboard = (str: string): Promise<boolean> => {
        if (navigator.clipboard && navigator.clipboard.writeText) {
          return navigator.clipboard.writeText(str).then(() => true).catch(() => false);
        }
        return Promise.resolve(false);
      };
      const fallbackCopy = (str: string): boolean => {
        try {
          const textarea = document.createElement("textarea");
          textarea.value = str;
          textarea.style.position = "fixed";
          textarea.style.opacity = "0";
          textarea.style.left = "-9999px";
          document.body.appendChild(textarea);
          textarea.focus();
          textarea.select();
          const ok = document.execCommand("copy");
          document.body.removeChild(textarea);
          return ok;
        } catch {
          return false;
        }
      };
      copyToClipboard(text).then((ok) => {
        if (ok) {
          toast.success("已复制 toast 内容", { duration: 1500 });
        } else if (fallbackCopy(text)) {
          toast.success("已复制 toast 内容", { duration: 1500 });
        } else {
          toast.error("复制失败", { duration: 1500 });
        }
      });
    };
    document.addEventListener("contextmenu", handler, true);
    return () => document.removeEventListener("contextmenu", handler, true);
  }, []);

  return (
    <>
      <HashRouter>
        <Suspense fallback={null}>
          <RouteWatcher />
          <Routes>
            <Route path="/" element={<MainView />} />
            <Route path="/settings" element={<SettingsView />} />
            <Route path="/batch" element={<BatchView />} />
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

/// 监听路由变化，切回首页（"/"）时关闭所有 toast
function RouteWatcher() {
  const location = useLocation();
  useEffect(() => {
    if (location.pathname === "/") {
      toast.dismiss();
    }
  }, [location.pathname]); // eslint-disable-line react-hooks/exhaustive-deps
  return null;
}
