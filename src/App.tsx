import { useEffect } from "react";
import { HashRouter, Routes, Route } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open, save } from "@tauri-apps/plugin-dialog";
import MainView from "./views/MainView";
import SettingsView from "./views/SettingsView";
import { useThemeStore } from "./stores/themeStore";
import { useVideoStore } from "./stores/videoStore";
import { useSubtitleStore } from "./stores/subtitleStore";
import { useTranslateStore } from "./stores/translateStore";
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
              return tr ? { ...e, translated: tr.translated } : e;
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

  // 监听文件拖放事件
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

    const unlisten = getCurrentWebviewWindow().onDragDropEvent((event) => {
      if (event.payload.type === "drop") {
        const paths = event.payload.paths;
        if (paths && paths.length > 0) {
          handleFile(paths[0]);
        }
      }
    });

    return () => { void unlisten.then((fn) => fn()); };
  }, [openVideo, loadSubtitle]);

  return (
    <>
      <HashRouter>
        <Routes>
          <Route path="/" element={<MainView />} />
          <Route path="/settings" element={<SettingsView />} />
        </Routes>
      </HashRouter>
      <Toaster position="top-right" richColors closeButton />
    </>
  );
}
