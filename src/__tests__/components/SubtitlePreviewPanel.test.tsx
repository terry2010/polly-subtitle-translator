// SubtitlePreviewPanel 组件测试（覆盖核心渲染和交互）
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SubtitlePreviewPanel } from "../../components/SubtitlePreviewPanel";
import { useSubtitleStore } from "../../stores/subtitleStore";
import { useVideoStore } from "../../stores/videoStore";
import { useTranslateStore } from "../../stores/translateStore";
import type { SubtitleEntry, SubtitleFile } from "../../lib/ipc-types";

vi.mock("../../lib/api", () => ({
  api: {
    playerHide: vi.fn(() => Promise.resolve()),
    playerShow: vi.fn(() => Promise.resolve()),
    devLog: vi.fn(),
    exportSubtitle: vi.fn(() => Promise.resolve()),
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(() => Promise.resolve(null)),
  open: vi.fn(() => Promise.resolve(null)),
}));

vi.mock("../../lib/utils", () => ({
  withPlayerHidden: vi.fn((fn: () => Promise<any>) => fn()),
  uiState: { mouseInSubtitleEditor: false },
  cn: vi.fn((...args: any[]) => args.filter(Boolean).join(" ")),
  stripExt: vi.fn((p: string) => p.replace(/\.[^.]+$/, "")),
  fileDir: vi.fn((p: string) => p.split(/[\\/]/).slice(0, -1).join("/") + "/"),
  buildExportFileName: vi.fn(() => "output.srt"),
  buildSubtitleTitle: vi.fn(() => "title"),
  hexToAssColor: vi.fn(() => "&H00FFFFFF"),
  assColorToCss: vi.fn(() => "#ffffff"),
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: any) => ({
    getVirtualItems: () => Array.from({ length: count }, (_, i) => ({ index: i, start: i * 72, size: 72 })),
    getTotalSize: () => count * 72,
    scrollToIndex: vi.fn(),
  }),
}));

function makeEntry(index: number, text: string, translated = "", startMs = 0, endMs = 1000): SubtitleEntry {
  return { index, start_ms: startMs, end_ms: endMs, text, translated, style: null, pre_edit_text: null };
}

function makeFile(entries: SubtitleEntry[]): SubtitleFile {
  return { format: "srt", entries, raw_header: null, source_path: "/test/sub.srt" };
}

beforeEach(() => {
  vi.clearAllMocks();
  useSubtitleStore.setState({
    file: null, loading: false, error: null, bilingualDetect: null,
    isSplit: false, preSplitFile: null, preSplitBilingualDetect: null,
    undoStack: [], redoStack: [],
    findQuery: "", replaceQuery: "", findTarget: "all",
    findMatchCount: 0, findCurrentMatch: 0, findMatchEntryIndex: null,
  });
  useVideoStore.setState({ probeResult: null, loading: false, error: null, selectedSubtitleStream: null });
  useTranslateStore.setState({
    translating: false, progress: 0, total: 0, result: null, error: null,
    sourceLang: "en", targetLang: "zh", provider: "baidu",
    model: "", modelType: "", serviceId: null,
  });
});

// === SECTION 1 END ===

describe("SubtitlePreviewPanel - 无字幕状态", () => {
  it("无文件时显示空状态", () => {
    render(<SubtitlePreviewPanel />);
    expect(screen.getByText("subtitle.empty")).toBeInTheDocument();
  });
});

// === SECTION 2 END ===

describe("SubtitlePreviewPanel - 有字幕状态", () => {
  function setFile(entries: SubtitleEntry[]) {
    useSubtitleStore.setState({ file: makeFile(entries) });
  }

  it("渲染工具栏按钮", () => {
    setFile([makeEntry(0, "hello", "你好")]);
    render(<SubtitlePreviewPanel />);
    expect(screen.getByText("subtitle.save")).toBeInTheDocument();
  });

  it("渲染预览模式 select", () => {
    setFile([makeEntry(0, "hello", "你好")]);
    render(<SubtitlePreviewPanel />);
    expect(screen.getByText("subtitle.modeOriginal")).toBeInTheDocument();
    expect(screen.getByText("subtitle.modeBilingual")).toBeInTheDocument();
    expect(screen.getByText("subtitle.modeTranslated")).toBeInTheDocument();
  });

  it("undo 按钮在无历史时禁用", () => {
    setFile([makeEntry(0, "hello", "你好")]);
    render(<SubtitlePreviewPanel />);
    const buttons = screen.getAllByRole("button");
    expect(buttons[0]).toBeDisabled();
  });

  it("redo 按钮在无历史时禁用", () => {
    setFile([makeEntry(0, "hello", "你好")]);
    render(<SubtitlePreviewPanel />);
    const buttons = screen.getAllByRole("button");
    expect(buttons[1]).toBeDisabled();
  });

  it("点击保存按钮打开导出弹层", async () => {
    const user = userEvent.setup();
    setFile([makeEntry(0, "hello", "你好")]);
    render(<SubtitlePreviewPanel />);
    await user.click(screen.getByText("subtitle.save"));
    await waitFor(() => {
      expect(screen.getByText("subtitle.exportFormat")).toBeInTheDocument();
    });
  });

  it("切换预览模式到原文", async () => {
    setFile([makeEntry(0, "hello", "你好")]);
    render(<SubtitlePreviewPanel />);
    const select = screen.getByDisplayValue("subtitle.modeBilingual");
    fireEvent.change(select, { target: { value: "original" } });
    // 不报错即可
  });

  it("切换预览模式到译文", async () => {
    setFile([makeEntry(0, "hello", "你好")]);
    render(<SubtitlePreviewPanel />);
    const select = screen.getByDisplayValue("subtitle.modeBilingual");
    fireEvent.change(select, { target: { value: "translated" } });
    // 不报错即可
  });

  it("有 undo 历史时 undo 按钮可用", () => {
    setFile([makeEntry(0, "hello", "你好")]);
    useSubtitleStore.setState({ undoStack: [makeFile([makeEntry(0, "hello", "")])] });
    render(<SubtitlePreviewPanel />);
    const buttons = screen.getAllByRole("button");
    expect(buttons[0]).not.toBeDisabled();
  });
});

// === SECTION 3 END ===

// === 原文编辑测试 ===

describe("SubtitlePreviewPanel - 原文编辑", () => {
  function setFile(entries: SubtitleEntry[]) {
    useSubtitleStore.setState({
      file: { format: "srt", entries, raw_header: null, source_path: "/test/sub.srt", file_hash: "H1" },
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
      editOriginalText: vi.fn(),
      restoreOriginalText: vi.fn(),
      updateEntry: vi.fn(),
    });
    useVideoStore.setState({ probeResult: null, loading: false, error: null, selectedSubtitleStream: null });
    useTranslateStore.setState({
      translating: false, progress: 0, total: 0, result: null, error: null,
      sourceLang: "en", targetLang: "zh", provider: "baidu",
      model: "", modelType: "", serviceId: null,
    });
  });

  it("有 pre_edit_text 标记的条目渲染编辑标记按钮", () => {
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hi", translated: "你好", style: null, pre_edit_text: "Hello" }]);
    const { container } = render(<SubtitlePreviewPanel />);
    const markBtn = container.querySelector('button[title="subtitle.edited"]');
    expect(markBtn).not.toBeNull();
  });

  it("无 pre_edit_text 标记的条目不渲染编辑标记按钮", () => {
    setFile([makeEntry(0, "Hello", "你好")]);
    const { container } = render(<SubtitlePreviewPanel />);
    const markBtn = container.querySelector('button[title="subtitle.edited"]');
    expect(markBtn).toBeNull();
  });

  it("有编辑条目时显示编辑计数跳转按钮", () => {
    setFile([
      { index: 0, start_ms: 0, end_ms: 1000, text: "Hi", translated: "你好", style: null, pre_edit_text: "Hello" },
      { index: 1, start_ms: 1000, end_ms: 2000, text: "Wld", translated: "世界", style: null, pre_edit_text: "World" },
    ]);
    render(<SubtitlePreviewPanel />);
    // 跳转按钮含 Pencil icon + 数量 "2"
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("无编辑条目时不显示编辑计数跳转按钮", () => {
    setFile([makeEntry(0, "Hello", "你好")]);
    render(<SubtitlePreviewPanel />);
    // 没有数字 "1"（编辑计数）
    expect(screen.queryByText("1")).not.toBeInTheDocument();
  });

  it("点击编辑标记按钮打开恢复对话框显示原始文本", () => {
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hi", translated: "你好", style: null, pre_edit_text: "Hello" }]);
    const { container } = render(<SubtitlePreviewPanel />);
    const markBtn = container.querySelector('button[title="subtitle.edited"]') as HTMLElement;
    fireEvent.click(markBtn);
    // 恢复对话框应显示原始文本 "Hello"
    expect(screen.getByText("Hello")).toBeInTheDocument();
    // "Hi" 出现在条目行和对话框中（修改后文本），应至少有 2 处
    expect(screen.getAllByText("Hi").length).toBeGreaterThanOrEqual(2);
  });

  // 恢复对话框点击恢复按钮调用 restoreOriginalText
  it("恢复对话框点击恢复按钮调用 restoreOriginalText", () => {
    const restoreOriginalTextSpy = vi.fn();
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hi", translated: "你好", style: null, pre_edit_text: "Hello" }]);
    useSubtitleStore.setState({ restoreOriginalText: restoreOriginalTextSpy });
    const { container } = render(<SubtitlePreviewPanel />);
    // 点击编辑标记按钮打开恢复对话框
    const markBtn = container.querySelector('button[title="subtitle.edited"]') as HTMLElement;
    fireEvent.click(markBtn);
    // 恢复对话框中的恢复按钮：找含 "subtitle.restore" 文本的按钮
    const restoreBtn = screen.getByText("subtitle.restore");
    fireEvent.click(restoreBtn);
    expect(restoreOriginalTextSpy).toHaveBeenCalledWith(0);
  });

  // 编辑原文 AutoTextarea → onChange → 调用 editOriginalText 持久化
  it("编辑原文 AutoTextarea → onChange → 调用 editOriginalText", () => {
    const editOriginalTextSpy = vi.fn();
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hello", translated: "你好", style: null, pre_edit_text: null }]);
    useSubtitleStore.setState({ editOriginalText: editOriginalTextSpy });
    const { container } = render(<SubtitlePreviewPanel />);
    // 点击原文文本 "Hello" 进入原文编辑模式
    const originalText = screen.getByText("Hello");
    fireEvent.click(originalText);
    // AutoTextarea 渲染为 textbox role
    const textbox = screen.getByRole("textbox");
    // 模拟 onChange（AutoTextarea 的 onChange 传递 value 字符串）
    fireEvent.change(textbox, { target: { value: "Hi" } });
    // editOriginalText 被调用
    expect(editOriginalTextSpy).toHaveBeenCalledWith(0, "Hi");
  });

  // 内联恢复按钮（编辑面板中的"恢复"按钮）调用 restoreOriginalText
  it("内联恢复按钮调用 restoreOriginalText", () => {
    const restoreOriginalTextSpy = vi.fn();
    setFile([{ index: 0, start_ms: 0, end_ms: 1000, text: "Hi", translated: "你好", style: null, pre_edit_text: "Hello" }]);
    useSubtitleStore.setState({ restoreOriginalText: restoreOriginalTextSpy });
    render(<SubtitlePreviewPanel />);
    // 点击原文文本 "Hi" 进入原文编辑模式
    const originalText = screen.getByText("Hi");
    fireEvent.click(originalText);
    // 编辑面板中有两个按钮：完成 + 恢复（仅当 pre_edit_text != null 时）
    // "subtitle.restore" 是内联恢复按钮的文本
    const inlineRestoreBtn = screen.getByText("subtitle.restore");
    fireEvent.click(inlineRestoreBtn);
    expect(restoreOriginalTextSpy).toHaveBeenCalledWith(0);
  });
});

// === SECTION 4 END ===
