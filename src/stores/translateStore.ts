// 翻译状态 store
import { create } from "zustand";
import type { TranslateResult, SubtitleEntry } from "../lib/ipc-types";
import { api, formatIpcError } from "../lib/api";

interface TranslateState {
  translating: boolean;
  progress: number;
  total: number;
  result: TranslateResult | null;
  error: string | null;
  sourceLang: string;
  targetLang: string;
  provider: string;
  model: string;
  modelType: string;
  serviceId: string | null; // AI 服务 ID（如 "deepseek"），传统翻译为 null

  setSourceLang: (lang: string) => void;
  setTargetLang: (lang: string) => void;
  setProvider: (provider: string) => void;
  setModel: (model: string) => void;
  setModelType: (modelType: string) => void;
  setServiceId: (id: string | null) => void;
  startTranslate: (entries: SubtitleEntry[], onEntryDone?: (index: number, translated: string, failed: boolean) => void) => Promise<TranslateResult | null>;
  cancelTranslate: () => Promise<void>;
  reset: () => void;
}

export const useTranslateStore = create<TranslateState>((set, get) => ({
  translating: false,
  progress: 0,
  total: 0,
  result: null,
  error: null,
  sourceLang: "en",
  targetLang: "zh",
  provider: "baidu",
  model: "",
  modelType: "",
  serviceId: null,

  setSourceLang: (lang) => set({ sourceLang: lang }),
  setTargetLang: (lang) => set({ targetLang: lang }),
  setProvider: (provider) => set({ provider }),
  setModel: (model) => set({ model }),
  setModelType: (modelType) => set({ modelType }),
  setServiceId: (id) => set({ serviceId: id }),

  startTranslate: async (entries: SubtitleEntry[], onEntryDone?: (index: number, translated: string, failed: boolean) => void) => {
    // 如果正在翻译，不允许启动新的翻译任务
    if (get().translating) {
      console.warn("翻译正在进行中，跳过新任务");
      return null;
    }
    const { sourceLang, targetLang, provider, model, modelType, serviceId } = get();
    set({ translating: true, progress: 0, total: entries.length, error: null, result: null });

    // 监听进度事件
    let unlistenProgress: (() => void) | null = null;
    let unlistenEntry: (() => void) | null = null;
    try {
      unlistenProgress = await api.onTranslateProgress((progress, total, done) => {
        set({ progress, total });
        if (done) {
          set({ translating: false });
        }
      });
    } catch (e) {
      console.warn("进度监听失败:", e);
    }

    // 监听单条翻译完成事件，逐条回调
    if (onEntryDone) {
      try {
        unlistenEntry = await api.onTranslateEntryDone((entry) => {
          onEntryDone(entry.index, entry.translated, entry.failed);
        });
      } catch (e) {
        console.warn("单条监听失败:", e);
      }
    }

    try {
      const result = await api.translateSubtitle(entries, sourceLang, targetLang, provider, model || undefined, modelType || undefined, serviceId || undefined);
      set({ translating: false, progress: entries.length, result });
      return result;
    } catch (e: any) {
      const error = formatIpcError(e);
      set({ translating: false, error });
      return null;
    } finally {
      if (unlistenProgress) unlistenProgress();
      if (unlistenEntry) unlistenEntry();
    }
  },

  cancelTranslate: async () => {
    try {
      await api.cancelTranslate();
      set({ translating: false });
    } catch (e) {
      console.error("取消翻译失败:", e);
    }
  },

  reset: () => set({ translating: false, progress: 0, total: 0, result: null, error: null }),
}));
