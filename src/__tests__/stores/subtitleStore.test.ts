// subtitleStore 单元测试
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useSubtitleStore } from "../../stores/subtitleStore";
import type { SubtitleFile, SubtitleEntry } from "../../lib/ipc-types";

// mock api
import { api } from "../../lib/api";
vi.mock("../../lib/api", () => ({
  api: {
    parseSubtitleFile: vi.fn(),
    detectBilingual: vi.fn(() => Promise.resolve({ is_bilingual: false })),
    saveSubtitleFile: vi.fn(),
    splitBilingualSubtitle: vi.fn(),
    getCachedTranslations: vi.fn(() => Promise.resolve([])),
    clearTranslateCache: vi.fn(() => Promise.resolve(0)),
    getSourceEdits: vi.fn(() => Promise.resolve([])),
    saveSourceEdit: vi.fn(() => Promise.resolve()),
    deleteSourceEdit: vi.fn(() => Promise.resolve(0)),
    replaceSourceEdits: vi.fn(() => Promise.resolve()),
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

// mock i18n
vi.mock("../../lib/i18n", () => ({
  default: { t: (key: string, fallback: string) => fallback, exists: () => false },
}));

// mock translateStore（避免循环依赖）
vi.mock("../../stores/translateStore", () => ({
  useTranslateStore: { getState: () => ({ sourceLang: "en", targetLang: "zh", provider: "baidu", serviceId: null, model: "" }) },
}));

function makeEntry(index: number, text: string, translated = "", startMs = 0, endMs = 1000): SubtitleEntry {
  return { index, start_ms: startMs, end_ms: endMs, text, translated, style: null, pre_edit_text: null };
}

function makeFile(entries: SubtitleEntry[]): SubtitleFile {
  return { format: "srt", entries, raw_header: null, source_path: null };
}

function getStore() {
  return useSubtitleStore.getState();
}

function resetStore() {
  useSubtitleStore.setState({
    file: null, loading: false, error: null, bilingualDetect: null,
    isSplit: false, preSplitFile: null, preSplitBilingualDetect: null,
    undoStack: [], redoStack: [],
    findQuery: "", replaceQuery: "", findTarget: "all",
    findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null,
  });
}

beforeEach(() => {
  resetStore();
  vi.clearAllMocks();
});

// === SECTION 1 END ===

describe("subtitleStore - 基础操作", () => {
  it("setFile 设置文件并清空 undo/redo 栈", () => {
    const file = makeFile([makeEntry(0, "hello"), makeEntry(1, "world")]);
    getStore().setFile(file);
    const state = getStore();
    expect(state.file).toEqual(file);
    expect(state.undoStack).toHaveLength(0);
    expect(state.redoStack).toHaveLength(0);
    expect(state.isSplit).toBe(false);
  });

  it("updateEntry 更新单条字幕并压入 undo 栈", () => {
    const file = makeFile([makeEntry(0, "hello"), makeEntry(1, "world")]);
    getStore().setFile(file);
    getStore().updateEntry(0, { translated: "你好" });
    const state = getStore();
    expect(state.file!.entries[0].translated).toBe("你好");
    expect(state.undoStack).toHaveLength(1);
    expect(state.undoStack[0].entries[0].translated).toBe("");
  });

  it("cancelEditEntry 恢复原始 translated 并截断 undo 栈", () => {
    const file = makeFile([makeEntry(0, "hello", "original")]);
    getStore().setFile(file);
    // 先做一次 update 产生 undo 记录，再 cancelEdit 截断到 0
    getStore().updateEntry(0, { translated: "编辑中" });
    expect(getStore().undoStack).toHaveLength(1);
    getStore().cancelEditEntry(0, "original", 0);
    const state = getStore();
    expect(state.file!.entries[0].translated).toBe("original");
    expect(state.undoStack).toHaveLength(0);
  });

  it("cancelEditOriginal 恢复原始 text 和 pre_edit_text 并截断 undo 栈", () => {
    const file = makeFile([makeEntry(0, "Hello", "你好")]);
    getStore().setFile(file);
    // 编辑原文
    getStore().editOriginalText(0, "Hi");
    expect(getStore().file!.entries[0].text).toBe("Hi");
    expect(getStore().file!.entries[0].pre_edit_text).toBe("Hello");
    expect(getStore().undoStack).toHaveLength(1);
    // 取消编辑：恢复到编辑前
    getStore().cancelEditOriginal(0, "Hello", null, 0);
    const state = getStore();
    expect(state.file!.entries[0].text).toBe("Hello");
    expect(state.file!.entries[0].pre_edit_text).toBeNull();
    expect(state.undoStack).toHaveLength(0);
  });
});

// === SECTION 2 END ===

describe("subtitleStore - 增删行", () => {
  it("addEntry 追加新条目", () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    const newEntry = makeEntry(1, "world");
    getStore().addEntry(newEntry);
    expect(getStore().file!.entries).toHaveLength(2);
    expect(getStore().file!.entries[1].text).toBe("world");
    expect(getStore().undoStack).toHaveLength(1);
  });

  it("insertEntryAfter 在指定条目后插入", () => {
    const file = makeFile([makeEntry(0, "a"), makeEntry(1, "c")]);
    getStore().setFile(file);
    getStore().insertEntryAfter(makeEntry(2, "b"), 0);
    const entries = getStore().file!.entries;
    expect(entries[1].text).toBe("b");
    expect(entries).toHaveLength(3);
  });

  it("insertEntryAfter 找不到 afterIndex 时追加到末尾", () => {
    const file = makeFile([makeEntry(0, "a")]);
    getStore().setFile(file);
    getStore().insertEntryAfter(makeEntry(1, "b"), 999);
    const entries = getStore().file!.entries;
    expect(entries[entries.length - 1].text).toBe("b");
  });

  it("deleteEntry 标记 _deleted 而非真正删除", () => {
    const file = makeFile([makeEntry(0, "a"), makeEntry(1, "b")]);
    getStore().setFile(file);
    getStore().deleteEntry(0);
    const entries = getStore().file!.entries;
    expect(entries).toHaveLength(2);
    expect(entries[0]._deleted).toBe(true);
  });

  it("removeEntry 真正删除条目", () => {
    const file = makeFile([makeEntry(0, "a"), makeEntry(1, "b")]);
    getStore().setFile(file);
    getStore().removeEntry(0);
    const entries = getStore().file!.entries;
    expect(entries).toHaveLength(1);
    expect(entries[0].index).toBe(1);
  });

  it("undoDelete 恢复 _deleted 标记", () => {
    const file = makeFile([makeEntry(0, "a")]);
    getStore().setFile(file);
    getStore().deleteEntry(0);
    expect(getStore().file!.entries[0]._deleted).toBe(true);
    getStore().undoDelete(0);
    expect(getStore().file!.entries[0]._deleted).toBe(false);
  });
});

// === SECTION 3 END ===

describe("subtitleStore - undo/redo", () => {
  it("undo 恢复到上一个状态", () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    getStore().updateEntry(0, { translated: "你好" });
    expect(getStore().file!.entries[0].translated).toBe("你好");
    getStore().undo();
    expect(getStore().file!.entries[0].translated).toBe("");
  });

  it("redo 重做到下一个状态", () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    getStore().updateEntry(0, { translated: "你好" });
    getStore().undo();
    getStore().redo();
    expect(getStore().file!.entries[0].translated).toBe("你好");
  });

  it("undo 栈上限 50（slice(-49) + 当前 = 50）", () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    for (let i = 0; i < 60; i++) {
      getStore().updateEntry(0, { translated: `翻译${i}` });
    }
    expect(getStore().undoStack.length).toBe(50);
  });

  it("新操作清空 redo 栈", () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    getStore().updateEntry(0, { translated: "你好" });
    getStore().undo();
    expect(getStore().redoStack).toHaveLength(1);
    getStore().updateEntry(0, { translated: "新翻译" });
    expect(getStore().redoStack).toHaveLength(0);
  });

  it("undo 无历史时不操作", () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    getStore().undo();
    expect(getStore().file!.entries[0].text).toBe("hello");
  });
});

// === SECTION 4 END ===

describe("subtitleStore - 查找替换", () => {
  beforeEach(() => {
    const file = makeFile([
      makeEntry(0, "hello world", "你好世界"),
      makeEntry(1, "hello again", "你好再次"),
      makeEntry(2, "goodbye", "再见"),
    ]);
    getStore().setFile(file);
  });

  it("setFindQuery 重置匹配状态", () => {
    getStore().setFindQuery("hello");
    getStore().findNext();
    expect(getStore().findMatchCount).toBe(2);
    getStore().setFindQuery("");
    expect(getStore().findMatchCount).toBe(0);
  });

  it("findNext 找到所有匹配并定位第一个", () => {
    getStore().setFindQuery("hello");
    getStore().findNext();
    expect(getStore().findMatchCount).toBe(2);
    expect(getStore().findMatchEntryIndex).toBe(0);
  });

  it("findNext 循环到下一个匹配", () => {
    getStore().setFindQuery("hello");
    getStore().findNext();
    getStore().findNext();
    expect(getStore().findCurrentMatch).toBe(1);
    expect(getStore().findMatchEntryIndex).toBe(1);
  });

  it("findNext 循环回第一个", () => {
    getStore().setFindQuery("hello");
    getStore().findNext();
    getStore().findNext();
    getStore().findNext();
    expect(getStore().findCurrentMatch).toBe(0);
  });

  it("findPrev 向前查找", () => {
    getStore().setFindQuery("hello");
    getStore().findNext();
    getStore().findNext();
    getStore().findPrev();
    expect(getStore().findCurrentMatch).toBe(0);
  });

  it("findTarget=translated 只在译文中查找", () => {
    getStore().setFindTarget("translated");
    getStore().setFindQuery("你好");
    getStore().findNext();
    expect(getStore().findMatchCount).toBe(2);
  });

  it("findTarget=original 只在原文中查找", () => {
    getStore().setFindTarget("original");
    getStore().setFindQuery("hello");
    getStore().findNext();
    expect(getStore().findMatchCount).toBe(2);
  });

  it("replaceCurrent 替换当前匹配并跳到下一个", () => {
    getStore().setFindQuery("hello");
    getStore().setReplaceQuery("HI");
    getStore().findNext();
    getStore().replaceCurrent();
    expect(getStore().file!.entries[0].text).toContain("HI");
  });

  it("replaceAll 替换所有匹配", () => {
    getStore().setFindQuery("hello");
    getStore().setReplaceQuery("HI");
    const count = getStore().replaceAll();
    expect(count).toBe(2);
    expect(getStore().file!.entries[0].text).toBe("HI world");
    expect(getStore().file!.entries[1].text).toBe("HI again");
  });

  it("空查询不查找", () => {
    getStore().setFindQuery("");
    getStore().findNext();
    expect(getStore().findMatchCount).toBe(0);
  });
});

// === SECTION 5 END ===

describe("subtitleStore - 时间轴偏移", () => {
  beforeEach(() => {
    const file = makeFile([
      makeEntry(0, "a", "", 1000, 2000),
      makeEntry(1, "b", "", 3000, 4000),
      makeEntry(2, "c", "", 5000, 6000),
    ]);
    getStore().setFile(file);
  });

  it("正偏移：所有条目时间后移", () => {
    const result = getStore().applyTimeOffset(500, 0, 2);
    expect(result.applied).toBe(3);
    const entries = getStore().file!.entries;
    expect(entries[0].start_ms).toBe(1500);
    expect(entries[0].end_ms).toBe(2500);
    expect(entries[2].start_ms).toBe(5500);
  });

  it("负偏移：时间前移，开头裁剪到 0，保持时长", () => {
    // entry: start=1000, end=2000, duration=1000
    // offset=-1500 → newStart=-500 < 0 → 裁剪到 0，end=duration=1000
    const result = getStore().applyTimeOffset(-1500, 0, 0);
    expect(result.applied).toBe(1);
    const entry = getStore().file!.entries[0];
    expect(entry.start_ms).toBe(0);
    expect(entry.end_ms).toBe(1000); // 保持原始时长 1000ms
  });

  it("范围限制：只偏移 fromIndex 到 toIndex", () => {
    getStore().applyTimeOffset(1000, 1, 1);
    const entries = getStore().file!.entries;
    expect(entries[0].start_ms).toBe(1000);
    expect(entries[1].start_ms).toBe(4000);
    expect(entries[2].start_ms).toBe(5000);
  });

  it("偏移压入 undo 栈", () => {
    getStore().applyTimeOffset(500, 0, 2);
    expect(getStore().undoStack).toHaveLength(1);
  });
});

// === SECTION 6 END ===

describe("subtitleStore - clearTranslations", () => {
  it("清空所有译文", () => {
    const file = makeFile([makeEntry(0, "a", "甲"), makeEntry(1, "b", "乙")]);
    getStore().setFile(file);
    getStore().clearTranslations();
    const entries = getStore().file!.entries;
    expect(entries[0].translated).toBe("");
    expect(entries[1].translated).toBe("");
    expect(getStore().undoStack).toHaveLength(1);
  });
});

// === SECTION 7 END ===

describe("subtitleStore - swapOriginalTranslated", () => {
  it("原文译文对调", () => {
    const file = makeFile([makeEntry(0, "hello", "你好")]);
    getStore().setFile(file);
    getStore().swapOriginalTranslated();
    const entry = getStore().file!.entries[0];
    expect(entry.text).toBe("你好");
    expect(entry.translated).toBe("hello");
  });

  it("译文为空时保留原文", () => {
    const file = makeFile([makeEntry(0, "hello", "")]);
    getStore().setFile(file);
    getStore().swapOriginalTranslated();
    const entry = getStore().file!.entries[0];
    expect(entry.text).toBe("hello");
    expect(entry.translated).toBe("hello");
  });
});

// === SECTION 8 END ===

describe("subtitleStore - saveSubtitle", () => {
  it("保存时过滤 _deleted 条目", async () => {
    const { api } = await import("../../lib/api");
    const file = makeFile([makeEntry(0, "a"), makeEntry(1, "b")]);
    getStore().setFile(file);
    getStore().deleteEntry(0);
    await getStore().saveSubtitle("/output.srt");
    expect(api.saveSubtitleFile).toHaveBeenCalledTimes(1);
    const savedFile = (api.saveSubtitleFile as any).mock.calls[0][0] as SubtitleFile;
    expect(savedFile.entries).toHaveLength(1);
    expect(savedFile.entries[0].index).toBe(1);
  });
});

// === SECTION 9 END ===

// === SECTION 10: 拆分/交换/取消拆分 ===

describe("subtitleStore - 拆分/交换/取消拆分", () => {
  it("splitBilingual 无文件时不执行", async () => {
    await getStore().splitBilingual();
    expect(api.splitBilingualSubtitle).not.toHaveBeenCalled();
  });

  it("splitBilingual 无 bilingualDetect 时不执行", async () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    await getStore().splitBilingual();
    expect(api.splitBilingualSubtitle).not.toHaveBeenCalled();
  });

  it("splitBilingual 成功拆分", async () => {
    const file = makeFile([makeEntry(0, "hello"), makeEntry(1, "你好")]);
    getStore().setFile(file);
    useSubtitleStore.setState({
      bilingualDetect: { is_bilingual: true, split_mode: "interleave" } as any,
    });
    const splitFile = makeFile([makeEntry(0, "hello"), makeEntry(1, "你好")]);
    (api.splitBilingualSubtitle as any).mockResolvedValue(splitFile);
    await getStore().splitBilingual();
    const state = getStore();
    expect(state.isSplit).toBe(true);
    expect(state.preSplitFile).toEqual(file);
    expect(state.bilingualDetect).toBeNull();
  });

  it("splitBilingual 失败时不修改状态", async () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    useSubtitleStore.setState({
      bilingualDetect: { is_bilingual: true, split_mode: "interleave" } as any,
    });
    (api.splitBilingualSubtitle as any).mockRejectedValue(new Error("split fail"));
    await getStore().splitBilingual();
    expect(getStore().isSplit).toBe(false);
  });

  it("unsplitBilingual 无 preSplitFile 时不执行", () => {
    const file = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(file);
    getStore().unsplitBilingual();
    expect(getStore().file).toEqual(file);
  });

  it("unsplitBilingual 恢复拆分前状态", () => {
    const originalFile = makeFile([makeEntry(0, "hello"), makeEntry(1, "你好")]);
    const splitFile = makeFile([makeEntry(0, "hello")]);
    getStore().setFile(originalFile);
    useSubtitleStore.setState({
      file: splitFile,
      isSplit: true,
      preSplitFile: originalFile,
      preSplitBilingualDetect: { is_bilingual: true } as any,
    });
    getStore().unsplitBilingual();
    const state = getStore();
    expect(state.file).toEqual(originalFile);
    expect(state.isSplit).toBe(false);
    expect(state.preSplitFile).toBeNull();
    expect(state.bilingualDetect).toEqual({ is_bilingual: true });
  });

  it("swapOriginalTranslated 无文件时不执行", () => {
    getStore().swapOriginalTranslated();
    expect(getStore().file).toBeNull();
  });

  it("swapOriginalTranslated 交换原文和译文", () => {
    const file = makeFile([makeEntry(0, "hello", "你好")]);
    getStore().setFile(file);
    getStore().swapOriginalTranslated();
    const entries = getStore().file!.entries;
    expect(entries[0].text).toBe("你好");
    expect(entries[0].translated).toBe("hello");
  });

  it("swapOriginalTranslated 译文为空时保留原文", () => {
    const file = makeFile([makeEntry(0, "hello", "")]);
    getStore().setFile(file);
    getStore().swapOriginalTranslated();
    const entries = getStore().file!.entries;
    expect(entries[0].text).toBe("hello");
    expect(entries[0].translated).toBe("hello");
  });

  it("dismissBilingualDetect 清空 bilingualDetect", () => {
    useSubtitleStore.setState({ bilingualDetect: { is_bilingual: true } as any });
    getStore().dismissBilingualDetect();
    expect(getStore().bilingualDetect).toBeNull();
  });
});

// === SECTION 10 END ===

// === 原文编辑功能测试（T7-T19, T25-T27）===

describe("subtitleStore - 原文编辑", () => {
  function makeFileWithHash(entries: SubtitleEntry[], fileHash = "H1"): SubtitleFile {
    return { format: "srt", entries, raw_header: null, source_path: null, file_hash: fileHash };
  }

  // 等待所有异步 source_edit 操作完成
  async function flushAsync() {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  beforeEach(() => {
    resetStore();
    vi.clearAllMocks();
  });

  // T7: editOriginalText 首次编辑存原始文本
  it("首次编辑原文时存 pre_edit_text", () => {
    const file = makeFileWithHash([makeEntry(0, "Hello", "译Hello")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "Hi");
    const entries = getStore().file!.entries;
    expect(entries[0].text).toBe("Hi");
    expect(entries[0].pre_edit_text).toBe("Hello");
    // 编辑原文后保留已有译文，无需重新翻译
    expect(entries[0].translated).toBe("译Hello");
  });

  // T8: editOriginalText 改回原始文本时清除标记
  it("改回原始文本时清除 pre_edit_text 标记", () => {
    const file = makeFileWithHash([makeEntry(0, "Hello")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "Hi");
    getStore().editOriginalText(0, "Hello"); // 改回
    const entries = getStore().file!.entries;
    expect(entries[0].text).toBe("Hello");
    expect(entries[0].pre_edit_text).toBeNull();
  });

  // T9: 链式编辑 pre_edit_text 始终为最初值
  it("链式编辑 A→B→C 时 pre_edit_text 始终为 A", () => {
    const file = makeFileWithHash([makeEntry(0, "A")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "B");
    expect(getStore().file!.entries[0].pre_edit_text).toBe("A");
    getStore().editOriginalText(0, "C");
    expect(getStore().file!.entries[0].pre_edit_text).toBe("A");
    expect(getStore().file!.entries[0].text).toBe("C");
  });

  // T9b: 编辑后还原再重新编辑
  it("A→B（保存）→还原→B（再次编辑）正确恢复标记", async () => {
    const file = makeFileWithHash([makeEntry(0, "A", "译A")]);
    getStore().setFile(file);
    // A → B
    getStore().editOriginalText(0, "B");
    await flushAsync();
    expect(api.saveSourceEdit).toHaveBeenCalledWith(0, "B", "A", "H1");
    expect(getStore().file!.entries[0].pre_edit_text).toBe("A");
    // 还原
    getStore().restoreOriginalText(0);
    await flushAsync();
    expect(api.deleteSourceEdit).toHaveBeenCalledWith(0, "H1");
    expect(getStore().file!.entries[0].text).toBe("A");
    expect(getStore().file!.entries[0].pre_edit_text).toBeNull();
    // 再次编辑为 B
    (api.saveSourceEdit as any).mockClear();
    getStore().editOriginalText(0, "B");
    await flushAsync();
    expect(api.saveSourceEdit).toHaveBeenCalledWith(0, "B", "A", "H1");
    expect(getStore().file!.entries[0].pre_edit_text).toBe("A");
    expect(getStore().file!.entries[0].text).toBe("B");
  });

  // T10: restoreOriginalText 恢复原始文本并清除标记
  it("restoreOriginalText 恢复原始文本并清除标记", async () => {
    const file = makeFileWithHash([makeEntry(0, "Hello")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "Hi");
    getStore().restoreOriginalText(0);
    await flushAsync();
    const entries = getStore().file!.entries;
    expect(entries[0].text).toBe("Hello");
    expect(entries[0].pre_edit_text).toBeNull();
    expect(api.deleteSourceEdit).toHaveBeenCalledWith(0, "H1");
  });

  // T11: replaceAll 修改原文时写入 source_edit_cache
  it("replaceAll 修改原文时设置 pre_edit_text 并持久化", async () => {
    const file = makeFileWithHash([
      makeEntry(0, "Hello World"),
      makeEntry(1, "Hello Sky"),
    ]);
    getStore().setFile(file);
    getStore().setFindQuery("Hello");
    getStore().setReplaceQuery("Hi");
    getStore().setFindTarget("original");
    getStore().replaceAll();
    await flushAsync();
    const entries = getStore().file!.entries;
    expect(entries[0].text).toBe("Hi World");
    expect(entries[0].pre_edit_text).toBe("Hello World");
    expect(entries[1].text).toBe("Hi Sky");
    expect(entries[1].pre_edit_text).toBe("Hello Sky");
    expect(api.saveSourceEdit).toHaveBeenCalledWith(0, "Hi World", "Hello World", "H1");
    expect(api.saveSourceEdit).toHaveBeenCalledWith(1, "Hi Sky", "Hello Sky", "H1");
  });

  // T12: replaceAll 只替换译文时不写 source_edit_cache
  it("replaceAll target=translated 时不写 source_edit_cache", () => {
    const file = makeFileWithHash([makeEntry(0, "Hello", "你好")]);
    getStore().setFile(file);
    getStore().setFindQuery("你好");
    getStore().setReplaceQuery("您好");
    getStore().setFindTarget("translated");
    getStore().replaceAll();
    const entries = getStore().file!.entries;
    expect(entries[0].text).toBe("Hello"); // 原文不变
    expect(entries[0].pre_edit_text).toBeNull(); // 无标记
    expect(api.saveSourceEdit).not.toHaveBeenCalled();
  });

  // T13: undo 编辑后整体重建 source_edit_cache
  it("undo 编辑后整体重建 source_edit_cache（清空）", () => {
    const file = makeFileWithHash([makeEntry(0, "Hello")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "Hi");
    getStore().undo();
    // undo 后 replaceSourceEdits 被调用，edits 为空数组（无标记条目）
    expect(api.replaceSourceEdits).toHaveBeenCalledWith("H1", []);
    expect(getStore().file!.entries[0].text).toBe("Hello");
    expect(getStore().file!.entries[0].pre_edit_text).toBeNull();
  });

  // T14: redo 恢复编辑后整体重建 source_edit_cache
  it("redo 恢复编辑后整体重建 source_edit_cache", () => {
    const file = makeFileWithHash([makeEntry(0, "Hello")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "Hi");
    getStore().undo();
    (api.replaceSourceEdits as any).mockClear();
    getStore().redo();
    // redo 后 replaceSourceEdits 被调用，edits 含编辑记录
    expect(api.replaceSourceEdits).toHaveBeenCalledWith("H1", [[0, "Hi", "Hello"]]);
    expect(getStore().file!.entries[0].text).toBe("Hi");
    expect(getStore().file!.entries[0].pre_edit_text).toBe("Hello");
  });

  // T15: loadSubtitle 恢复 corrected text + 标记
  it("loadSubtitle 从 get_source_edits 恢复 corrected text + 标记", async () => {
    (api.parseSubtitleFile as any).mockResolvedValue({
      format: "srt",
      entries: [{ index: 0, start_ms: 0, end_ms: 1000, text: "Hello", translated: "", style: null, pre_edit_text: null }],
      raw_header: null, source_path: null, file_hash: "H1",
    });
    (api.getSourceEdits as any).mockResolvedValue([
      { entry_index: 0, corrected_text: "Hi", pre_edit_text: "Hello" },
    ]);
    (api.getCachedTranslations as any).mockResolvedValue([
      { index: 0, original: "Hi", translated: "你好", from_cache: true, failed: false, pre_edit_text: "Hello" },
    ]);

    await getStore().loadSubtitle("/test/sub.srt");
    const entries = getStore().file!.entries;
    expect(entries[0].text).toBe("Hi");       // corrected
    expect(entries[0].pre_edit_text).toBe("Hello"); // 标记
    expect(entries[0].translated).toBe("你好"); // 从缓存恢复
    expect(entries[0].from_cache).toBe(true);
  });

  // T16: swapOriginalTranslated 清除所有 pre_edit_text
  it("swapOriginalTranslated 清除所有 pre_edit_text 标记", async () => {
    const file = makeFileWithHash([makeEntry(0, "Hello", "你好")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "Hi");
    await flushAsync();
    expect(getStore().file!.entries[0].pre_edit_text).toBe("Hello");
    (api.deleteSourceEdit as any).mockClear();
    getStore().swapOriginalTranslated();
    await flushAsync();
    expect(getStore().file!.entries[0].pre_edit_text).toBeNull();
    expect(api.deleteSourceEdit).toHaveBeenCalledWith(0, "H1");
  });

  // T17: deleteEntry 删除有标记的条目时清理 DB
  it("deleteEntry 删除有标记的条目时清理 source_edit_cache", async () => {
    const file = makeFileWithHash([makeEntry(0, "Hello")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "Hi");
    await flushAsync();
    (api.deleteSourceEdit as any).mockClear();
    getStore().deleteEntry(0);
    await flushAsync();
    expect(api.deleteSourceEdit).toHaveBeenCalledWith(0, "H1");
  });

  // T18: resetToInitial 清空 source_edit_cache
  it("resetToInitial 清空 source_edit_cache", () => {
    const file = makeFileWithHash([makeEntry(0, "Hello")]);
    getStore().setFile(file);
    getStore().editOriginalText(0, "Hi");
    getStore().resetToInitial();
    expect(api.replaceSourceEdits).toHaveBeenCalledWith("H1", []);
  });

  // T19: updateEntry 修改 text 时打 warn
  it("updateEntry 修改 text 时打 warn 但不阻止", async () => {
    const { setDevModeEnabled } = await import("../../lib/logger");
    setDevModeEnabled(true);
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const file = makeFileWithHash([makeEntry(0, "Hello")]);
    getStore().setFile(file);
    getStore().updateEntry(0, { text: "Hi" });
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining("editOriginalText"));
    expect(getStore().file!.entries[0].text).toBe("Hi"); // 仍然生效
    warnSpy.mockRestore();
    setDevModeEnabled(false);
  });

  // T18b: 删除条目后其他条目的 source_edit_cache 不受影响
  it("deleteEntry 删除条目A不影响条目B的 source_edit_cache", async () => {
    const file = makeFileWithHash([
      makeEntry(0, "Hello", "译A"),
      makeEntry(1, "World", "译B"),
    ]);
    getStore().setFile(file);
    // 两条都编辑原文
    getStore().editOriginalText(0, "Hi");
    getStore().editOriginalText(1, "Wo");
    await flushAsync();
    (api.deleteSourceEdit as any).mockClear();
    (api.saveSourceEdit as any).mockClear();
    // 删除条目 0
    getStore().deleteEntry(0);
    await flushAsync();
    // 只删除条目 0 的 source_edit_cache，不影响条目 1
    expect(api.deleteSourceEdit).toHaveBeenCalledTimes(1);
    expect(api.deleteSourceEdit).toHaveBeenCalledWith(0, "H1");
    // 条目 1 的编辑标记仍在
    const entries = getStore().file!.entries;
    expect(entries[1].pre_edit_text).toBe("World");
    expect(entries[1].text).toBe("Wo");
    // 不应对条目 1 调用 deleteSourceEdit
    expect(api.deleteSourceEdit).not.toHaveBeenCalledWith(1, "H1");
  });

  // T19b: 插入条目后已有编辑记录不受影响
  it("insertEntryAfter 插入新条目不影响已有编辑记录", async () => {
    const file = makeFileWithHash([
      makeEntry(0, "Hello", "译A"),
      makeEntry(1, "World", "译B"),
    ]);
    getStore().setFile(file);
    // 编辑条目 0 的原文
    getStore().editOriginalText(0, "Hi");
    await flushAsync();
    (api.saveSourceEdit as any).mockClear();
    (api.deleteSourceEdit as any).mockClear();
    (api.replaceSourceEdits as any).mockClear();
    // 在条目 0 后插入新条目
    getStore().insertEntryAfter(makeEntry(2, "New", ""), 0);
    await flushAsync();
    // 不应触发任何 source_edit_cache 操作
    expect(api.saveSourceEdit).not.toHaveBeenCalled();
    expect(api.deleteSourceEdit).not.toHaveBeenCalled();
    expect(api.replaceSourceEdits).not.toHaveBeenCalled();
    // 条目 0 的编辑标记仍在
    const entries = getStore().file!.entries;
    const entry0 = entries.find((e) => e.index === 0);
    expect(entry0?.pre_edit_text).toBe("Hello");
    expect(entry0?.text).toBe("Hi");
    // 新条目无编辑标记
    const entry2 = entries.find((e) => e.index === 2);
    expect(entry2?.pre_edit_text).toBeNull();
  });
});

// === SECTION 11 END ===
