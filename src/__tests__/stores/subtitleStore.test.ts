// subtitleStore 单元测试
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useSubtitleStore } from "../../stores/subtitleStore";
import type { SubtitleFile, SubtitleEntry } from "../../lib/ipc-types";

// mock api
vi.mock("../../lib/api", () => ({
  api: {
    parseSubtitleFile: vi.fn(),
    detectBilingual: vi.fn(() => Promise.resolve({ is_bilingual: false })),
    saveSubtitleFile: vi.fn(),
    splitBilingualSubtitle: vi.fn(),
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

// mock i18n
vi.mock("../../lib/i18n", () => ({
  default: { t: (key: string, fallback: string) => fallback, exists: () => false },
}));

function makeEntry(index: number, text: string, translated = "", startMs = 0, endMs = 1000): SubtitleEntry {
  return { index, start_ms: startMs, end_ms: endMs, text, translated, style: null };
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
