// 字幕状态 store
import { create } from "zustand";
import type { SubtitleFile, SubtitleEntry, BilingualDetectResult } from "../lib/ipc-types";
import { api, formatIpcError } from "../lib/api";
import { toast } from "sonner";
import i18n from "../lib/i18n";

interface SubtitleState {
  file: SubtitleFile | null;
  loading: boolean;
  error: string | null;
  // 双语检测结果
  bilingualDetect: BilingualDetectResult | null;
  // 是否已拆分双语字幕
  isSplit: boolean;
  // 拆分前的原始文件（用于取消拆分恢复）
  preSplitFile: SubtitleFile | null;
  // 拆分前的双语检测结果（用于取消拆分后恢复"拆分字幕"按钮）
  preSplitBilingualDetect: BilingualDetectResult | null;
  // undo/redo 栈
  undoStack: SubtitleFile[];
  redoStack: SubtitleFile[];
  // 查找替换
  findQuery: string;
  replaceQuery: string;

  loadSubtitle: (path: string) => Promise<void>;
  updateEntry: (index: number, patch: Partial<SubtitleEntry>) => void;
  /** 取消编辑：恢复 entry 的 translated 到原始值，并截断 undoStack 到编辑前长度。
   *  这样用户按 undo 会回到编辑前状态，而非编辑过程中的中间态。 */
  cancelEditEntry: (index: number, originalTranslated: string, undoStackLength: number) => void;
  addEntry: (entry: SubtitleEntry) => void;
  deleteEntry: (index: number) => void;
  undoDelete: (index: number) => void;
  clearTranslations: () => void;
  applyTimeOffset: (offsetMs: number) => void;
  findReplace: (find: string, replace: string, all: boolean) => number;
  undo: () => void;
  redo: () => void;
  saveSubtitle: (outputPath: string) => Promise<void>;
  setFile: (file: SubtitleFile | null) => void;
  setFindQuery: (q: string) => void;
  setReplaceQuery: (q: string) => void;
  splitBilingual: () => Promise<void>;
  unsplitBilingual: () => void;
  swapOriginalTranslated: () => void;
  dismissBilingualDetect: () => void;
}

function pushUndo(state: SubtitleState): Partial<SubtitleState> {
  if (!state.file) return {};
  return {
    undoStack: [...state.undoStack.slice(-49), structuredClone(state.file)],
    redoStack: [],
  };
}

export const useSubtitleStore = create<SubtitleState>((set, get) => ({
  file: null,
  loading: false,
  error: null,
  bilingualDetect: null,
  isSplit: false,
  preSplitFile: null,
  preSplitBilingualDetect: null,
  undoStack: [],
  redoStack: [],
  findQuery: "",
  replaceQuery: "",

  loadSubtitle: async (path: string) => {
    set({ loading: true, error: null, bilingualDetect: null, isSplit: false, preSplitFile: null, preSplitBilingualDetect: null });
    try {
      const file = await api.parseSubtitleFile(path);
      set({ file, loading: false, undoStack: [], redoStack: [] });
      // 自动检测双语
      try {
        const detect = await api.detectBilingual(file);
        if (detect.is_bilingual) {
          set({ bilingualDetect: detect });
          const langAName = langDisplayName(detect.lang_a);
          const langBName = langDisplayName(detect.lang_b);
          toast.info(i18n.t("subtitle.bilingualDetected", {
            langA: langAName,
            langB: langBName,
            matched: detect.matched_count,
            total: detect.total_count,
          }), {
            action: {
              label: i18n.t("subtitle.split"),
              onClick: () => get().splitBilingual(),
            },
            duration: 10000,
          });
        }
      } catch (e) {
        console.warn("双语检测失败:", e);
      }
    } catch (e: any) {
      const msg = formatIpcError(e);
      set({ loading: false, error: msg });
    }
  },

  setFile: (file) => set({ file, undoStack: [], redoStack: [], isSplit: false, preSplitFile: null, preSplitBilingualDetect: null }),

  updateEntry: (index, patch) => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    const entries = state.file.entries.map((e) =>
      e.index === index ? { ...e, ...patch } : e
    );
    set({ ...undoPatch, file: { ...state.file, entries } });
  },

  cancelEditEntry: (index, originalTranslated, undoStackLength) => {
    const state = get();
    if (!state.file) return;
    const entries = state.file.entries.map((e) =>
      e.index === index ? { ...e, translated: originalTranslated } : e
    );
    set({
      file: { ...state.file, entries },
      undoStack: state.undoStack.slice(0, undoStackLength),
      redoStack: [],
    });
  },

  addEntry: (entry) => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    set({
      ...undoPatch,
      file: { ...state.file, entries: [...state.file.entries, entry] },
    });
  },

  deleteEntry: (index) => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    const entries = state.file.entries.map((e) =>
      e.index === index ? { ...e, _deleted: true } : e
    );
    set({
      ...undoPatch,
      file: { ...state.file, entries },
    });
  },

  undoDelete: (index) => {
    const state = get();
    if (!state.file) return;
    const entries = state.file.entries.map((e) =>
      e.index === index ? { ...e, _deleted: false } : e
    );
    set({ file: { ...state.file, entries } });
  },

  applyTimeOffset: (offsetMs) => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    const entries = state.file.entries.map((e) => ({
      ...e,
      start_ms: Math.max(0, e.start_ms + offsetMs),
      end_ms: Math.max(0, e.end_ms + offsetMs),
    }));
    set({ ...undoPatch, file: { ...state.file, entries } });
  },

  findReplace: (find, replace, all) => {
    const state = get();
    if (!state.file || !find) return 0;
    const undoPatch = pushUndo(state);
    let count = 0;
    const entries = state.file.entries.map((e) => {
      let text = e.translated || e.text;
      if (all) {
        const newText = text.split(find).join(replace);
        if (newText !== text) count++;
        text = newText;
      } else {
        const idx = text.indexOf(find);
        if (idx >= 0) {
          text = text.slice(0, idx) + replace + text.slice(idx + find.length);
          count++;
        }
      }
      return { ...e, translated: e.translated ? text : text, text: e.translated ? e.text : text };
    });
    set({ ...undoPatch, file: { ...state.file, entries } });
    return count;
  },

  undo: () => {
    const state = get();
    if (state.undoStack.length === 0 || !state.file) return;
    const prev = state.undoStack[state.undoStack.length - 1];
    set({
      file: prev,
      undoStack: state.undoStack.slice(0, -1),
      redoStack: [...state.redoStack, structuredClone(state.file)],
    });
  },

  redo: () => {
    const state = get();
    if (state.redoStack.length === 0 || !state.file) return;
    const next = state.redoStack[state.redoStack.length - 1];
    set({
      file: next,
      redoStack: state.redoStack.slice(0, -1),
      undoStack: [...state.undoStack, structuredClone(state.file)],
    });
  },

  saveSubtitle: async (outputPath: string) => {
    const state = get();
    if (!state.file) return;
    // 保存时过滤掉已删除的条目
    const fileToSave = {
      ...state.file,
      entries: state.file.entries.filter((e) => !e._deleted),
    };
    await api.saveSubtitleFile(fileToSave, outputPath);
  },

  setFindQuery: (q) => set({ findQuery: q }),
  setReplaceQuery: (q) => set({ replaceQuery: q }),

  clearTranslations: () => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    const entries = state.file.entries.map((e) => ({ ...e, translated: "" }));
    set({ ...undoPatch, file: { ...state.file, entries } });
  },

  splitBilingual: async () => {
    const state = get();
    if (!state.file || !state.bilingualDetect) return;
    try {
      const splitFile = await api.splitBilingualSubtitle(state.file, state.bilingualDetect.split_mode);
      const undoPatch = pushUndo(state);
      // 保存拆分前的原始文件和双语检测结果，用于取消拆分恢复
      set({ ...undoPatch, file: splitFile, bilingualDetect: null, isSplit: true, preSplitFile: state.file, preSplitBilingualDetect: state.bilingualDetect });
      toast.success(i18n.t("subtitle.splitSuccess"));
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  unsplitBilingual: () => {
    const state = get();
    if (!state.file || !state.preSplitFile) return;
    const undoPatch = pushUndo(state);
    // 恢复拆分前的原始文件和双语检测结果（恢复"拆分字幕"按钮）
    set({ ...undoPatch, file: state.preSplitFile, isSplit: false, preSplitFile: null, bilingualDetect: state.preSplitBilingualDetect, preSplitBilingualDetect: null });
    toast.success(i18n.t("subtitle.splitCancelled"));
  },

  swapOriginalTranslated: () => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    // 将每条字幕的原文和译文对调
    const entries = state.file.entries.map((e) => ({
      ...e,
      text: e.translated || e.text,
      translated: e.text,
    }));
    set({ ...undoPatch, file: { ...state.file, entries } });
    toast.success(i18n.t("subtitle.swapSuccess"));
  },

  dismissBilingualDetect: () => set({ bilingualDetect: null }),
}));

/// 语言类别转可读名称
function langDisplayName(lang: string): string {
  switch (lang) {
    case "cjk": return "中文/汉字";
    case "hiragana": return "平假名";
    case "katakana": return "片假名";
    case "hangul": return "韩文";
    case "latin": return "拉丁字母";
    case "cyrillic": return "西里尔文";
    case "arabic": return "阿拉伯文";
    default: return lang;
  }
}
