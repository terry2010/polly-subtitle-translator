// FFmpeg 下载状态 store（仿照 libmpvStore）
import { create } from "zustand";
import { api, formatIpcError } from "../lib/api";
import i18n from "../lib/i18n";

interface FfmpegStatus {
  installed: boolean;
  source: string | null;
  path: string | null;
}

interface FfmpegState {
  downloading: boolean;
  downloadProgress: number;
  downloadStage: string;
  downloadMessage: string;
  downloadError: string;
  downloadSpeedMbps: number;
  downloadEtaSecs: number;
  status: FfmpegStatus | null;
  statusLoading: boolean;

  onProgressEvent: (payload: { stage: string; progress: number; message?: string; code?: string; args?: Record<string, unknown>; speed_mbps?: number; eta_secs?: number }) => void;
  refreshStatus: () => Promise<void>;
  startDownload: () => Promise<void>;
}

export const useFfmpegStore = create<FfmpegState>((set, get) => ({
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
      set({ downloading: false, downloadStage: "failed", downloadError: errorMsg });
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
      const status = await api.getFfmpegStatus();
      set({ status, statusLoading: false });
    } catch {
      set({ status: { installed: false, source: null, path: null }, statusLoading: false });
    }
  },

  startDownload: async () => {
    if (get().downloading) return;
    set({
      downloading: true,
      downloadProgress: 0,
      downloadStage: "downloading",
      downloadMessage: "",
      downloadError: "",
    });
    try {
      await api.downloadFfmpeg();
      await get().refreshStatus();
    } catch (e: any) {
      const errMsg = formatIpcError(e);
      set((s) => ({ downloading: false, downloadError: s.downloadError || errMsg }));
    }
  },
}));
