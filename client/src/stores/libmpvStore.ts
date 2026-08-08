// libmpv 下载状态 store
// 将下载状态从 VideoPlayer 组件 local state 提升到全局 store，
// 避免路由切换（MainView ↔ SettingsView）时组件卸载导致状态丢失。
import { create } from "zustand";
import { api, formatIpcError } from "../lib/api";
import type { LibmpvStatus } from "../lib/ipc-types";
import i18n from "../lib/i18n";

interface LibmpvState {
  // 下载状态
  downloading: boolean;
  downloadProgress: number;
  downloadStage: string;
  downloadMessage: string;
  downloadError: string;
  downloadSpeedMbps: number;
  downloadEtaSecs: number;
  // libmpv 安装状态
  status: LibmpvStatus | null;
  statusLoading: boolean;

  // 下载进度事件由 App.tsx 全局监听并调用此方法
  onProgressEvent: (payload: { stage: string; progress: number; message?: string; code?: string; args?: Record<string, unknown>; speed_mbps?: number; eta_secs?: number }) => void;
  // 刷新 libmpv 安装状态
  refreshStatus: () => Promise<void>;
  // 触发下载
  startDownload: () => Promise<void>;
}

export const useLibmpvStore = create<LibmpvState>((set, get) => ({
  downloading: false,
  downloadProgress: 0,
  downloadStage: "",
  downloadMessage: "",
  downloadError: "",
  downloadSpeedMbps: 0,
  downloadEtaSecs: 0,
  status: null,
  statusLoading: false,

  onProgressEvent: (payload) => {
    const { stage, progress, message, code, args, speed_mbps, eta_secs } = payload;
    if (stage === "failed") {
      const errorMsg = code
        ? (i18n.t(`error.${code}`, args ?? {}) === `error.${code}` ? code : i18n.t(`error.${code}`, args ?? {}))
        : message || "";
      set({
        downloading: false,
        downloadStage: "failed",
        downloadError: errorMsg,
      });
      return;
    }
    if (stage === "done") {
      set({
        downloading: false,
        downloadStage: "done",
        downloadProgress: 100,
        downloadMessage: message || "",
        downloadError: "",
        downloadSpeedMbps: 0,
        downloadEtaSecs: 0,
      });
      void get().refreshStatus();
      return;
    }
    set({
      downloadStage: stage,
      downloadProgress: progress,
      downloadMessage: message || "",
      downloadSpeedMbps: speed_mbps ?? 0,
      downloadEtaSecs: eta_secs ?? 0,
    });
  },

  refreshStatus: async () => {
    set({ statusLoading: true });
    try {
      const status = await api.getLibmpvStatus();
      set({ status, statusLoading: false });
    } catch {
      set({ status: { downloaded: false, path: null, version: null }, statusLoading: false });
    }
  },

  startDownload: async () => {
    if (get().downloading) return;
    set({
      downloading: true,
      downloadProgress: 0,
      downloadStage: "fetching",
      downloadMessage: "",
      downloadError: "",
    });
    try {
      await api.downloadLibmpv();
      // 下载完成（done 事件已处理状态刷新，这里兜底）
      await get().refreshStatus();
    } catch (e: any) {
      // failed 事件监听器已设置 downloadError，这里仅兜底
      const errMsg = formatIpcError(e);
      set((s) => ({
        downloading: false,
        downloadError: s.downloadError || errMsg,
      }));
    }
  },
}));
