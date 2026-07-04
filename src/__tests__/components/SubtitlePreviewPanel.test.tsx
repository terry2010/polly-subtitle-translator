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
    devLog: vi.fn(() => Promise.resolve()),
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

function makeEntry(index: number, text: string, translated = "", startMs = 0, endMs = 1000): SubtitleEntry {
  return { index, start_ms: startMs, end_ms: endMs, text, translated, style: null };
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
