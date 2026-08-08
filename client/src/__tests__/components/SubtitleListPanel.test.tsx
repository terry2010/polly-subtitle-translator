// SubtitleListPanel 组件测试
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SubtitleListPanel } from "../../components/SubtitleListPanel";
import { useSubtitleStore } from "../../stores/subtitleStore";
import type { SubtitleEntry } from "../../lib/ipc-types";

const { mockSave, mockSaveSubtitle } = vi.hoisted(() => ({
  mockSave: vi.fn(),
  mockSaveSubtitle: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: mockSave,
}));

vi.mock("../../lib/api", () => ({
  api: {},
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

vi.mock("../../lib/utils", () => ({
  withPlayerHidden: vi.fn((fn: () => Promise<any>) => fn()),
  cn: vi.fn((...args: any[]) => args.filter(Boolean).join(" ")),
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: any) => ({
    getVirtualItems: () => Array.from({ length: count }, (_, i) => ({ index: i, start: i * 80, size: 80 })),
    getTotalSize: () => count * 80,
  }),
}));

function makeEntry(index: number, text: string, translated = "", startMs = 0, endMs = 1000): SubtitleEntry {
  return { index, start_ms: startMs, end_ms: endMs, text, translated, style: null, pre_edit_text: null };
}

beforeEach(() => {
  vi.clearAllMocks();
  useSubtitleStore.setState({
    file: null, loading: false, error: null, bilingualDetect: null,
    isSplit: false, preSplitFile: null, preSplitBilingualDetect: null,
    undoStack: [], redoStack: [],
    findQuery: "", replaceQuery: "", findTarget: "all",
    findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null,
    saveSubtitle: mockSaveSubtitle,
  });
  mockSave.mockResolvedValue(null);
  mockSaveSubtitle.mockResolvedValue(undefined);
});

// === SECTION 1 END ===

describe("SubtitleListPanel - 无字幕状态", () => {
  it("无文件时显示空状态", () => {
    render(<SubtitleListPanel />);
    expect(screen.getByText("subtitle.edit")).toBeInTheDocument();
    expect(screen.getByText("common.noData")).toBeInTheDocument();
  });
});

// === SECTION 2 END ===

describe("SubtitleListPanel - 有字幕状态", () => {
  function setFile(entries: SubtitleEntry[]) {
    useSubtitleStore.setState({
      file: { format: "srt", entries, raw_header: null, source_path: null },
    });
  }

  it("渲染工具栏按钮", () => {
    setFile([makeEntry(0, "hello")]);
    render(<SubtitleListPanel />);
    expect(screen.getByText("subtitle.save")).toBeInTheDocument();
  });

  it("undo 按钮在无历史时禁用", () => {
    setFile([makeEntry(0, "hello")]);
    render(<SubtitleListPanel />);
    const undoBtn = screen.getAllByRole("button")[0];
    expect(undoBtn).toBeDisabled();
  });

  it("点击添加按钮调用 addEntry", async () => {
    const user = userEvent.setup();
    const addEntrySpy = vi.fn();
    setFile([makeEntry(0, "hello")]);
    useSubtitleStore.setState({ addEntry: addEntrySpy });
    render(<SubtitleListPanel />);
    const buttons = screen.getAllByRole("button");
    // Plus 按钮是工具栏中第 5 个按钮（undo/redo/find/offset/add）
    await user.click(buttons[4]);
    expect(addEntrySpy).toHaveBeenCalled();
  });

  it("点击保存按钮调用 save 对话框", async () => {
    const user = userEvent.setup();
    mockSave.mockResolvedValue("/output.srt");
    setFile([makeEntry(0, "hello")]);
    render(<SubtitleListPanel />);
    await user.click(screen.getByText("subtitle.save"));
    await waitFor(() => {
      expect(mockSave).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(mockSaveSubtitle).toHaveBeenCalledWith("/output.srt");
    });
  });

  it("save 返回 null 时不保存", async () => {
    const user = userEvent.setup();
    mockSave.mockResolvedValue(null);
    setFile([makeEntry(0, "hello")]);
    render(<SubtitleListPanel />);
    await user.click(screen.getByText("subtitle.save"));
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockSaveSubtitle).not.toHaveBeenCalled();
  });

  it("切换查找替换面板", async () => {
    const user = userEvent.setup();
    setFile([makeEntry(0, "hello")]);
    render(<SubtitleListPanel />);
    // 查找替换按钮是工具栏第 3 个按钮
    const buttons = screen.getAllByRole("button");
    await user.click(buttons[2]);
    expect(screen.getByPlaceholderText("subtitle.findReplace")).toBeInTheDocument();
  });

  it("切换时间偏移面板", async () => {
    const user = userEvent.setup();
    setFile([makeEntry(0, "hello")]);
    render(<SubtitleListPanel />);
    const buttons = screen.getAllByRole("button");
    await user.click(buttons[3]);
    expect(screen.getByPlaceholderText("±1000")).toBeInTheDocument();
  });

  it("应用时间偏移", async () => {
    const user = userEvent.setup();
    const applyTimeOffsetSpy = vi.fn();
    setFile([makeEntry(0, "hello")]);
    useSubtitleStore.setState({ applyTimeOffset: applyTimeOffsetSpy });
    render(<SubtitleListPanel />);
    const buttons = screen.getAllByRole("button");
    await user.click(buttons[3]);
    const input = screen.getByPlaceholderText("±1000");
    await user.type(input, "500");
    const applyBtn = screen.getByText("Apply");
    await user.click(applyBtn);
    expect(applyTimeOffsetSpy).toHaveBeenCalledWith(500, 0, 999999);
  });
});

// === SECTION 3 END ===

// === 原文编辑测试 ===

describe("SubtitleListPanel - 原文编辑", () => {
  function setFile(entries: SubtitleEntry[]) {
    useSubtitleStore.setState({
      file: { format: "srt", entries, raw_header: null, source_path: null, file_hash: "H1" },
    });
  }

  beforeEach(() => {
    vi.clearAllMocks();
    useSubtitleStore.setState({
      file: null, loading: false, error: null, bilingualDetect: null,
      isSplit: false, preSplitFile: null, preSplitBilingualDetect: null,
      undoStack: [], redoStack: [],
      findQuery: "", replaceQuery: "", findTarget: "all",
      findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null,
      saveSubtitle: mockSaveSubtitle,
      editOriginalText: vi.fn(),
      restoreOriginalText: vi.fn(),
      updateEntry: vi.fn(),
    });
    mockSave.mockResolvedValue(null);
    mockSaveSubtitle.mockResolvedValue(undefined);
  });

  it("有 pre_edit_text 标记的条目渲染编辑标记按钮", () => {
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hi", translated: "你好", style: null, pre_edit_text: "Hello" }]);
    const { container } = render(<SubtitleListPanel />);
    // 编辑标记按钮有 title="subtitle.edited"（i18n key，未 mock 时返回 key 本身）
    const markBtn = container.querySelector('button[title="subtitle.edited"]');
    expect(markBtn).not.toBeNull();
  });

  it("无 pre_edit_text 标记的条目不渲染编辑标记按钮", () => {
    setFile([makeEntry(0, "Hello", "你好")]);
    const { container } = render(<SubtitleListPanel />);
    const markBtn = container.querySelector('button[title="subtitle.edited"]');
    expect(markBtn).toBeNull();
  });

  it("点击编辑标记按钮打开恢复对话框", () => {
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hi", translated: "你好", style: null, pre_edit_text: "Hello" }]);
    const { container } = render(<SubtitleListPanel />);
    const markBtn = container.querySelector('button[title="subtitle.edited"]') as HTMLElement;
    fireEvent.click(markBtn);
    // 恢复对话框应显示原始文本 "Hello"
    expect(screen.getByText("Hello")).toBeInTheDocument();
    // "Hi" 出现在条目行和对话框中（修改后文本），应至少有 2 处
    expect(screen.getAllByText("Hi").length).toBeGreaterThanOrEqual(2);
  });

  // T27: 编辑原文后走 editOriginalText 持久化
  it("编辑原文 textarea → onBlur → 调用 editOriginalText 持久化", async () => {
    const editOriginalTextSpy = vi.fn();
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hello", translated: "你好", style: null, pre_edit_text: null }]);
    useSubtitleStore.setState({ editOriginalText: editOriginalTextSpy });
    const { container } = render(<SubtitleListPanel />);
    // 点击条目行（含 "Hello" 文本的行）进入编辑态
    const entryText = screen.getByText("Hello");
    fireEvent.click(entryText);
    // 修改原文 Textarea（第一个 textarea 是原文）
    const textareas = container.querySelectorAll("textarea");
    expect(textareas.length).toBeGreaterThan(0);
    fireEvent.focus(textareas[0]);
    fireEvent.change(textareas[0], { target: { value: "Hi" } });
    // 失焦确认
    fireEvent.blur(textareas[0]);
    // editOriginalText 被调用（不是 updateEntry）
    expect(editOriginalTextSpy).toHaveBeenCalledWith(0, "Hi");
  });

  // 恢复对话框点击恢复按钮调用 restoreOriginalText
  it("恢复对话框点击恢复按钮调用 restoreOriginalText", () => {
    const restoreOriginalTextSpy = vi.fn();
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hi", translated: "你好", style: null, pre_edit_text: "Hello" }]);
    useSubtitleStore.setState({ restoreOriginalText: restoreOriginalTextSpy });
    const { container } = render(<SubtitleListPanel />);
    // 点击编辑标记按钮打开恢复对话框
    const markBtn = container.querySelector('button[title="subtitle.edited"]') as HTMLElement;
    fireEvent.click(markBtn);
    // 恢复对话框中的恢复按钮：找含 "subtitle.restore" 文本的按钮
    const restoreBtn = screen.getByText("subtitle.restore");
    fireEvent.click(restoreBtn);
    expect(restoreOriginalTextSpy).toHaveBeenCalledWith(0);
  });
});

// === SECTION 4 END ===
