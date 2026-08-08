// 视频状态 store
import { create } from "zustand";
import type { ProbeResult, SubtitleStream } from "../lib/ipc-types";
import { api, formatIpcError } from "../lib/api";

interface VideoState {
  probeResult: ProbeResult | null;
  loading: boolean;
  error: string | null;
  selectedSubtitleStream: SubtitleStream | null;
  /** 是否在播放器加载后自动播放（仅首次打开视频时 true，从设置返回时不自动播放） */
  autoPlayOnLoad: boolean;
  /** 播放器卸载时保存的位置（秒），用于从设置返回时恢复 */
  savedPosition: number;
  /** 播放器卸载时是否正在播放，用于从设置返回时恢复 */
  wasPlaying: boolean;
  openVideo: (path: string) => Promise<void>;
  selectSubtitleStream: (stream: SubtitleStream | null) => void;
  clearVideo: () => void;
  setAutoPlayOnLoad: (v: boolean) => void;
  setSavedPosition: (pos: number) => void;
  setWasPlaying: (playing: boolean) => void;
}

export const useVideoStore = create<VideoState>((set) => ({
  probeResult: null,
  loading: false,
  error: null,
  selectedSubtitleStream: null,
  autoPlayOnLoad: false,
  savedPosition: 0,
  wasPlaying: false,

  openVideo: async (path: string) => {
    set({ loading: true, error: null, autoPlayOnLoad: true, savedPosition: 0, wasPlaying: false });
    try {
      const result = await api.probeVideo(path);
      set({ probeResult: result, loading: false });
      // 自动选择字幕流：优先英文 SDH（disposition 标志或 title 含 SDH/HI/CC），
      // 其次普通英文，最后第一条非图形字幕兜底
      const subs = result.subtitle_streams.filter((s) => !s.is_graphic);
      const isSdhTitle = (s: SubtitleStream) => {
        const t = (s.title ?? "").toUpperCase();
        return t.includes("SDH") || t.includes("HI") || t.includes("CC");
      };
      const engSdh = subs.find(
        (s) => s.language === "eng" && (s.disposition_hearing_impaired || isSdhTitle(s))
      );
      const eng = subs.find((s) => s.language === "eng");
      const firstSub = engSdh ?? eng ?? subs[0] ?? null;
      set({ selectedSubtitleStream: firstSub });
    } catch (e: any) {
      const msg = formatIpcError(e);
      set({ loading: false, error: msg });
    }
  },

  selectSubtitleStream: (stream) => set({ selectedSubtitleStream: stream }),
  clearVideo: () => set({ probeResult: null, error: null, selectedSubtitleStream: null, autoPlayOnLoad: false, savedPosition: 0, wasPlaying: false }),
  setAutoPlayOnLoad: (v) => set({ autoPlayOnLoad: v }),
  setSavedPosition: (pos) => set({ savedPosition: pos }),
  setWasPlaying: (playing) => set({ wasPlaying: playing }),
}));
