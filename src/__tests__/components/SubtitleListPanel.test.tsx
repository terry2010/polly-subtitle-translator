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

function makeEntry(index: number, text: string, translated = "", startMs = 0, endMs = 1000): SubtitleEntry {
  return { index, start_ms: startMs, end_ms: endMs, text, translated, style: null };
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
