// 字幕状态 store
import { create } from "zustand";
import type { SubtitleFile, SubtitleEntry, BilingualDetectResult } from "../lib/ipc-types";
import { api, formatIpcError } from "../lib/api";
import { toast } from "sonner";
import i18n from "../lib/i18n";
import { warn } from "../lib/logger";
import { useTranslateStore } from "./translateStore";

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
  findTarget: "all" | "translated" | "original";
  findMatchCount: number;
  findCurrentMatch: number; // 0-based，当前匹配序号
  findMatchEntryIndex: number | null; // 当前匹配的 entry index

  loadSubtitle: (path: string) => Promise<void>;
  updateEntry: (index: number, patch: Partial<SubtitleEntry>) => void;
  /** 取消编辑：恢复 entry 的 translated 到原始值，并截断 undoStack 到编辑前长度。
   *  这样用户按 undo 会回到编辑前状态，而非编辑过程中的中间态。 */
  cancelEditEntry: (index: number, originalTranslated: string, undoStackLength: number) => void;
  addEntry: (entry: SubtitleEntry) => void;
  insertEntryAfter: (entry: SubtitleEntry, afterIndex: number) => void;
  deleteEntry: (index: number) => void;
  removeEntry: (index: number) => void;
  undoDelete: (index: number) => void;
  clearTranslations: () => void;
  applyTimeOffset: (offsetMs: number, fromIndex: number, toIndex: number) => { applied: number; exceeded: number; maxExceedSec: number };
  /** 查找：返回匹配总数，设置 findMatchEntryIndex 到第一个匹配 */
  findNext: () => void;
  findPrev: () => void;
  /** 替换当前匹配并自动跳到下一个 */
  replaceCurrent: () => void;
  /** 全部替换，返回替换条数 */
  replaceAll: () => number;
  undo: () => void;
  redo: () => void;
  /** 重置到初始状态（加载时的状态），返回已执行的步数 */
  resetToInitial: () => number;
  saveSubtitle: (outputPath: string) => Promise<void>;
  setFile: (file: SubtitleFile | null) => void;
  setFindQuery: (q: string) => void;
  setReplaceQuery: (q: string) => void;
  setFindTarget: (target: "all" | "translated" | "original") => void;
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

/** 查找所有匹配的条目 index 列表 */
function findAllMatches(file: SubtitleFile, query: string, target: "all" | "translated" | "original"): number[] {
  if (!query) return [];
  const q = query.toLowerCase();
  const results: number[] = [];
  for (const e of file.entries) {
    if (e._deleted) continue;
    const fields = target === "translated" ? [e.translated] : target === "original" ? [e.text] : [e.translated, e.text];
    if (fields.some((f) => f.toLowerCase().includes(q))) {
      results.push(e.index);
    }
  }
  return results;
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
  findTarget: "all",
  findMatchCount: 0,
  findCurrentMatch: 0,
  findMatchEntryIndex: null,

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
        warn("双语检测失败:", e);
      }
      // 查询翻译缓存，自动填充已翻译的条目
      // 双语文件跳过缓存查询：双语文件中失败条目只输出原文（1行），
      // 其文本与原始单语条目的cache key相同，会被错误填入旧缓存译文，
      // 导致"未翻译"数不一致（SRT/VTT有缓存命中而ASS因样式标签无命中）。
      // 双语文件的翻译状态已编码在格式中（1行=失败，2行=成功），无需缓存补充。
      const isBilingual = get().bilingualDetect?.is_bilingual;
      if (!isBilingual) {
        try {
          const { sourceLang, targetLang, provider, serviceId, model } = useTranslateStore.getState();
          const cached = await api.getCachedTranslations(
            file.entries, sourceLang, targetLang, provider,
            provider === "openai" ? (serviceId || undefined) : undefined,
            provider === "openai" ? (model || undefined) : undefined,
            file.file_hash || undefined,
          );
          if (cached && cached.length > 0) {
            const currentState = get();
            if (!currentState.file) return;
            const entries = currentState.file.entries.map((e) => {
              const tr = cached.find((c) => c.index === e.index);
              return tr ? { ...e, translated: tr.translated, from_cache: true } : e;
            });
            set({ file: { ...currentState.file, entries } });
          }
        } catch (e) {
          warn("查询翻译缓存失败:", e);
        }
      }
    } catch (e: any) {
      const msg = formatIpcError(e);
      set({ loading: false, error: msg });
    }
  },

  setFile: (file) => set({ file, error: null, undoStack: [], redoStack: [], isSplit: false, preSplitFile: null, preSplitBilingualDetect: null }),

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

  insertEntryAfter: (entry, afterIndex) => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    const entries = [...state.file.entries];
    const insertPos = entries.findIndex((e) => e.index === afterIndex);
    if (insertPos === -1) {
      // 找不到就追加到末尾
      entries.push(entry);
    } else {
      entries.splice(insertPos + 1, 0, entry);
    }
    set({
      ...undoPatch,
      file: { ...state.file, entries },
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

  removeEntry: (index) => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    const entries = state.file.entries.filter((e) => e.index !== index);
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

  applyTimeOffset: (offsetMs, fromIndex, toIndex) => {
    const state = get();
    if (!state.file) return { applied: 0, exceeded: 0, maxExceedSec: 0 };
    const undoPatch = pushUndo(state);
    let applied = 0;
    const entries = state.file.entries.map((e) => {
      // 只处理 fromIndex 到 toIndex 范围内的条目
      if (e.index < fromIndex || e.index > toIndex || e._deleted) return e;
      const duration = e.end_ms - e.start_ms;
      const newStart = e.start_ms + offsetMs;
      let start_ms: number;
      let end_ms: number;
      if (newStart < 0) {
        // 裁剪开头，保持时长
        start_ms = 0;
        end_ms = duration;
      } else {
        start_ms = newStart;
        end_ms = e.end_ms + offsetMs;
      }
      applied++;
      return { ...e, start_ms, end_ms };
    });
    set({ ...undoPatch, file: { ...state.file, entries } });
    return { applied, exceeded: 0, maxExceedSec: 0 };
  },

  findNext: () => {
    const state = get();
    if (!state.file || !state.findQuery) {
      set({ findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null });
      return;
    }
    const matches = findAllMatches(state.file, state.findQuery, state.findTarget);
    if (matches.length === 0) {
      set({ findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null });
      return;
    }
    // 从当前位置之后查找
    const currentEntryIdx = state.findMatchEntryIndex;
    let nextPos = 0;
    if (currentEntryIdx != null) {
      const currentPos = matches.indexOf(currentEntryIdx);
      if (currentPos >= 0) {
        nextPos = (currentPos + 1) % matches.length;
      }
    }
    set({ findMatchCount: matches.length, findCurrentMatch: nextPos, findMatchEntryIndex: matches[nextPos] });
  },

  findPrev: () => {
    const state = get();
    if (!state.file || !state.findQuery) {
      set({ findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null });
      return;
    }
    const matches = findAllMatches(state.file, state.findQuery, state.findTarget);
    if (matches.length === 0) {
      set({ findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null });
      return;
    }
    const currentEntryIdx = state.findMatchEntryIndex;
    let prevPos = 0;
    if (currentEntryIdx != null) {
      const currentPos = matches.indexOf(currentEntryIdx);
      if (currentPos >= 0) {
        prevPos = (currentPos - 1 + matches.length) % matches.length;
      }
    }
    set({ findMatchCount: matches.length, findCurrentMatch: prevPos, findMatchEntryIndex: matches[prevPos] });
  },

  replaceCurrent: () => {
    const state = get();
    if (!state.file || !state.findQuery || state.findMatchEntryIndex == null) return;
    const undoPatch = pushUndo(state);
    const find = state.findQuery;
    const replace = state.replaceQuery;
    const q = find.toLowerCase();
    const target = state.findTarget;
    const matchEntryIdx = state.findMatchEntryIndex;
    let count = 0;
    const entries = state.file.entries.map((e) => {
      if (e.index !== matchEntryIdx) return e;
      let newText = e.text;
      let newTranslated = e.translated;
      if (target === "all" || target === "original") {
        const idx = newText.toLowerCase().indexOf(q);
        if (idx >= 0) {
          newText = newText.slice(0, idx) + replace + newText.slice(idx + find.length);
          count++;
        }
      }
      if (target === "all" || target === "translated") {
        const idx = newTranslated.toLowerCase().indexOf(q);
        if (idx >= 0) {
          newTranslated = newTranslated.slice(0, idx) + replace + newTranslated.slice(idx + find.length);
          count++;
        }
      }
      return { ...e, text: newText, translated: newTranslated };
    });
    if (count > 0) {
      set({ ...undoPatch, file: { ...state.file, entries } });
    }
    // 替换后重新查找，跳到下一个匹配
    get().findNext();
  },

  replaceAll: () => {
    const state = get();
    if (!state.file || !state.findQuery) return 0;
    const undoPatch = pushUndo(state);
    const find = state.findQuery;
    const replace = state.replaceQuery;
    const q = find.toLowerCase();
    const target = state.findTarget;
    let count = 0;
    const entries = state.file.entries.map((e) => {
      if (e._deleted) return e;
      let newText = e.text;
      let newTranslated = e.translated;
      let changed = false;
      if (target === "all" || target === "original") {
        const newText2 = newText.split(find).join(replace);
        if (newText2 !== newText) { newText = newText2; changed = true; }
      }
      if (target === "all" || target === "translated") {
        const newTranslated2 = newTranslated.split(find).join(replace);
        if (newTranslated2 !== newTranslated) { newTranslated = newTranslated2; changed = true; }
      }
      if (changed) count++;
      return { ...e, text: newText, translated: newTranslated };
    });
    set({ ...undoPatch, file: { ...state.file, entries }, findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null });
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

  resetToInitial: () => {
    const state = get();
    if (!state.file) return 0;
    const steps = state.undoStack.length;
    if (steps === 0) return 0;
    // undoStack[0] 是第一次操作前的快照（初始状态）
    const initialFile = state.undoStack[0];
    // 恢复 bilingualDetect：如果拆分过，用 preSplitBilingualDetect；否则保持当前
    const bilingualDetect = state.preSplitBilingualDetect ?? state.bilingualDetect;
    set({
      file: structuredClone(initialFile),
      undoStack: [],
      redoStack: [],
      isSplit: false,
      preSplitFile: null,
      preSplitBilingualDetect: null,
      bilingualDetect,
    });
    return steps;
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

  setFindQuery: (q) => set({ findQuery: q, findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null }),
  setReplaceQuery: (q) => set({ replaceQuery: q }),
  setFindTarget: (target) => set({ findTarget: target, findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null }),

  clearTranslations: async () => {
    const state = get();
    if (!state.file) return;
    const undoPatch = pushUndo(state);
    // 清空译文的同时重置 from_cache 和 failed 标记，
    // 否则清除缓存前从缓存加载的条目会残留 from_cache=true，
    // 导致导出统计误报"缓存=N条"。
    const entries = state.file.entries.map((e) => ({
      ...e, translated: "", from_cache: false, failed: false,
    }));
    set({ ...undoPatch, file: { ...state.file, entries } });
    // 同时清除后端翻译缓存，避免重新翻译时读取旧的错位缓存
    await api.clearTranslateCache();
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

/// 语言类别转可读名称（i18n）
function langDisplayName(lang: string): string {
  const key = `subtitle.langType.${lang}`;
  const fallback: Record<string, string> = {
    cjk: "中文/汉字",
    hiragana: "平假名",
    katakana: "片假名",
    hangul: "韩文",
    latin: "拉丁字母",
    cyrillic: "西里尔文",
    arabic: "阿拉伯文",
  };
  return i18n.exists(key) ? i18n.t(key) : (fallback[lang] ?? lang);
}
