// ExportDialog 组件测试
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ExportDialog } from "../../components/ExportDialog";
import { useVideoStore } from "../../stores/videoStore";
import { useTranslateStore } from "../../stores/translateStore";
import type { SubtitleFile, SubtitleEntry } from "../../lib/ipc-types";

const { mockPlayerHide, mockPlayerShow, mockExportSubtitle, mockMergeSubtitle, mockCheckMergeSpace, mockSave } = vi.hoisted(() => ({
  mockPlayerHide: vi.fn(),
  mockPlayerShow: vi.fn(),
  mockExportSubtitle: vi.fn(),
  mockMergeSubtitle: vi.fn(),
  mockCheckMergeSpace: vi.fn(),
  mockSave: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    playerHide: mockPlayerHide,
    playerShow: mockPlayerShow,
    exportSubtitle: mockExportSubtitle,
    mergeSubtitle: mockMergeSubtitle,
    checkMergeSpace: mockCheckMergeSpace,
    devLog: vi.fn(() => Promise.resolve()),
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: mockSave,
}));

vi.mock("@tauri-apps/api/path", () => ({
  tempDir: vi.fn(() => Promise.resolve("/tmp/")),
}));

function makeEntry(index: number, text: string, translated = ""): SubtitleEntry {
  return { index, start_ms: 0, end_ms: 1000, text, translated, style: null };
}

function makeFile(entries: SubtitleEntry[]): SubtitleFile {
  return { format: "srt", entries, raw_header: null, source_path: "/test/sub.srt" };
}

function renderDialog(props: { open?: boolean; file?: SubtitleFile } = {}) {
  const onOpenChange = vi.fn();
  const { open = true, file = makeFile([makeEntry(0, "hello", "你好"), makeEntry(1, "world", "世界")]) } = props;
  render(<ExportDialog open={open} onOpenChange={onOpenChange} file={file} />);
  return { onOpenChange };
}

beforeEach(() => {
  vi.clearAllMocks();
  useVideoStore.setState({ probeResult: null, loading: false, error: null, selectedSubtitleStream: null });
  useTranslateStore.setState({
    translating: false, progress: 0, total: 0, result: null, error: null,
    sourceLang: "en", targetLang: "zh", provider: "baidu",
    model: "", modelType: "", serviceId: null,
  });
  mockPlayerHide.mockResolvedValue(undefined);
  mockPlayerShow.mockResolvedValue(undefined);
  mockExportSubtitle.mockResolvedValue(undefined);
  mockMergeSubtitle.mockResolvedValue(undefined);
  mockCheckMergeSpace.mockResolvedValue({ video_size: 1000, free_space: 100000, enough: true });
  mockSave.mockResolvedValue(null);
});

// === SECTION 1 END ===

describe("ExportDialog - 渲染", () => {
  it("显示标题", () => {
    renderDialog();
    expect(screen.getByText("subtitle.save")).toBeInTheDocument();
  });

  it("打开弹窗时隐藏播放器", () => {
    renderDialog();
    expect(mockPlayerHide).toHaveBeenCalled();
  });

  it("渲染格式按钮 SRT/ASS/VTT", () => {
    renderDialog();
    expect(screen.getByText("SRT")).toBeInTheDocument();
    expect(screen.getByText("ASS")).toBeInTheDocument();
    expect(screen.getByText("VTT")).toBeInTheDocument();
  });

  it("渲染模式选择单语/双语", () => {
    renderDialog();
    expect(screen.getByText("subtitle.exportMonolingual")).toBeInTheDocument();
    expect(screen.getByText("subtitle.exportBilingual")).toBeInTheDocument();
  });

  it("渲染预览区域", () => {
    renderDialog();
    expect(screen.getByText("subtitle.exportPreview")).toBeInTheDocument();
  });
});

// === SECTION 2 END ===

describe("ExportDialog - 格式与模式切换", () => {
  it("切换到 ASS 格式", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("ASS"));
    // ASS + 双语应显示样式配置
    expect(screen.getByText("subtitle.exportAssStyle")).toBeInTheDocument();
  });

  it("切换到单语模式", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("subtitle.exportMonolingual"));
    expect(screen.getByText("subtitle.exportLang")).toBeInTheDocument();
  });

  it("SRT 双语显示无样式提示", () => {
    renderDialog();
    expect(screen.getByText("subtitle.exportSrtNoStyleHint")).toBeInTheDocument();
  });

  it("ASS 双语显示样式配置", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("ASS"));
    expect(screen.getByText("subtitle.exportPrimaryLine")).toBeInTheDocument();
    expect(screen.getByText("subtitle.exportSecondaryLine")).toBeInTheDocument();
  });

  it("重置 ASS 样式", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("ASS"));
    await user.click(screen.getByText("subtitle.exportResetStyle"));
    // 不报错即可
  });
});

// === SECTION 3 END ===

describe("ExportDialog - 导出", () => {
  it("点击导出调用 exportSubtitle", async () => {
    mockSave.mockResolvedValue("/output/sub.srt");
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.click(screen.getByText("subtitle.exportConfirm"));
    await waitFor(() => {
      expect(mockExportSubtitle).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });

  it("用户取消保存时不导出", async () => {
    mockSave.mockResolvedValue(null);
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("subtitle.exportConfirm"));
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockExportSubtitle).not.toHaveBeenCalled();
  });

  it("导出失败显示错误 toast", async () => {
    mockSave.mockResolvedValue("/output/sub.srt");
    mockExportSubtitle.mockRejectedValue(new Error("export fail"));
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("subtitle.exportConfirm"));
    await waitFor(() => {
      expect(mockExportSubtitle).toHaveBeenCalled();
    });
  });
});

// === SECTION 4 END ===

describe("ExportDialog - 合并到视频", () => {
  it("无视频时不显示合并按钮", () => {
    renderDialog();
    expect(screen.queryByText("subtitle.mergeToVideo")).not.toBeInTheDocument();
  });

  it("有视频时显示合并按钮", () => {
    useVideoStore.setState({
      probeResult: {
        video_path: "/test/video.mkv",
        format: { format_name: "matroska", format_long_name: "Matroska", duration: 120, size: 1000, bit_rate: 8000 },
        video_stream: { width: 1920, height: 1080 } as any,
        audio_streams: [],
        subtitle_streams: [],
      },
    });
    renderDialog();
    expect(screen.getByText("subtitle.mergeToVideo")).toBeInTheDocument();
  });

  it("合并成功", async () => {
    useVideoStore.setState({
      probeResult: {
        video_path: "/test/video.mkv",
        format: { format_name: "matroska", format_long_name: "Matroska", duration: 120, size: 1000, bit_rate: 8000 },
        video_stream: { width: 1920, height: 1080 } as any,
        audio_streams: [],
        subtitle_streams: [],
      },
    });
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.click(screen.getByText("subtitle.mergeToVideo"));
    await waitFor(() => {
      expect(mockExportSubtitle).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(mockMergeSubtitle).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });
});

// === SECTION 5 END ===
