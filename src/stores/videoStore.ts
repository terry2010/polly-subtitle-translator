// 视频状态 store
import { create } from "zustand";
import type { ProbeResult, SubtitleStream } from "../lib/ipc-types";
import { api, formatIpcError } from "../lib/api";

interface VideoState {
  probeResult: ProbeResult | null;
  loading: boolean;
  error: string | null;
  selectedSubtitleStream: SubtitleStream | null;
  openVideo: (path: string) => Promise<void>;
  selectSubtitleStream: (stream: SubtitleStream | null) => void;
  clearVideo: () => void;
}

export const useVideoStore = create<VideoState>((set) => ({
  probeResult: null,
  loading: false,
  error: null,
  selectedSubtitleStream: null,

  openVideo: async (path: string) => {
    set({ loading: true, error: null });
    try {
      const result = await api.probeVideo(path);
      set({ probeResult: result, loading: false });
      // 自动选择字幕流：优先 eng SDH，其次 eng，最后第一条非图形字幕
      const subs = result.subtitle_streams.filter((s) => !s.is_graphic);
      const engSdh = subs.find((s) => s.language === "eng" && s.disposition_hearing_impaired);
      const eng = subs.find((s) => s.language === "eng");
      const firstSub = engSdh ?? eng ?? subs[0] ?? null;
      set({ selectedSubtitleStream: firstSub });
    } catch (e: any) {
      const msg = formatIpcError(e);
      set({ loading: false, error: msg });
    }
  },

  selectSubtitleStream: (stream) => set({ selectedSubtitleStream: stream }),
  clearVideo: () => set({ probeResult: null, error: null, selectedSubtitleStream: null }),
}));
