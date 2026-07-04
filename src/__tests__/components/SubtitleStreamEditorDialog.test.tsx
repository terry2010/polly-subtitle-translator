// SubtitleStreamEditorDialog 组件测试
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SubtitleStreamEditorDialog } from "../../components/SubtitleStreamEditorDialog";
import type { SubtitleStream } from "../../lib/ipc-types";

const { mockPlayerHide, mockPlayerShow, mockEditSubtitleStreams, mockCheckMergeSpace, mockExtractSubtitle, mockSave } = vi.hoisted(() => ({
  mockPlayerHide: vi.fn(),
  mockPlayerShow: vi.fn(),
  mockEditSubtitleStreams: vi.fn(),
  mockCheckMergeSpace: vi.fn(),
  mockExtractSubtitle: vi.fn(),
  mockSave: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    playerHide: mockPlayerHide,
    playerShow: mockPlayerShow,
    editSubtitleStreams: mockEditSubtitleStreams,
    checkMergeSpace: mockCheckMergeSpace,
    extractSubtitle: mockExtractSubtitle,
    devLog: vi.fn(() => Promise.resolve()),
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: mockSave,
}));

vi.mock("../../lib/utils", () => ({
  withPlayerHidden: vi.fn((fn: () => Promise<any>) => fn()),
  cn: vi.fn((...args: any[]) => args.filter(Boolean).join(" ")),
}));

function makeStream(index: number, opts: Partial<SubtitleStream> = {}): SubtitleStream {
  return {
    index,
    codec_name: "subrip",
    codec_long_name: "SubRip",
    duration: null,
    language: "eng",
    title: "English",
    disposition_default: false,
    disposition_forced: false,
    disposition_hearing_impaired: false,
    is_graphic: false,
    ...opts,
  };
}

function renderDialog(props: { open?: boolean; streams?: SubtitleStream[] } = {}) {
  const onOpenChange = vi.fn();
  const onSaved = vi.fn();
  const { open = true, streams = [makeStream(0), makeStream(1, { language: "jpn", title: "Japanese" })] } = props;
  render(
    <SubtitleStreamEditorDialog
      open={open}
      onOpenChange={onOpenChange}
      videoPath="/test/video.mkv"
      streams={streams}
      onSaved={onSaved}
    />,
  );
  return { onOpenChange, onSaved };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockPlayerHide.mockResolvedValue(undefined);
  mockPlayerShow.mockResolvedValue(undefined);
  mockEditSubtitleStreams.mockResolvedValue(undefined);
  mockCheckMergeSpace.mockResolvedValue({ video_size: 1000, free_space: 100000, enough: true });
  mockExtractSubtitle.mockResolvedValue(undefined);
  mockSave.mockResolvedValue(null);
});

// === SECTION 1 END ===

describe("SubtitleStreamEditorDialog - 渲染", () => {
  it("显示标题", () => {
    renderDialog();
    expect(screen.getByText("subtitle.streamEditor")).toBeInTheDocument();
  });

  it("打开弹窗时隐藏播放器", () => {
    renderDialog();
    expect(mockPlayerHide).toHaveBeenCalled();
  });

  it("渲染所有字幕流", () => {
    renderDialog();
    expect(screen.getByDisplayValue("English")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Japanese")).toBeInTheDocument();
  });

  it("图形字幕禁用输入框", () => {
    renderDialog({
      streams: [makeStream(0, { is_graphic: true, codec_name: "hdmv_pgs_subtitle", title: "", language: "" })],
    });
    const inputs = screen.getAllByRole("textbox");
    expect(inputs[0]).toBeDisabled();
    expect(inputs[1]).toBeDisabled();
  });

  it("无字幕流时显示空提示", () => {
    renderDialog({ streams: [] });
    expect(screen.getByText("subtitle.streamEditEmpty")).toBeInTheDocument();
  });
});

// === SECTION 2 END ===

describe("SubtitleStreamEditorDialog - 编辑", () => {
  it("修改标题", async () => {
    const user = userEvent.setup();
    renderDialog();
    const titleInput = screen.getByDisplayValue("English");
    await user.clear(titleInput);
    await user.type(titleInput, "New Title");
    expect(screen.getByDisplayValue("New Title")).toBeInTheDocument();
  });

  it("删除字幕流", async () => {
    renderDialog();
    const deleteButtons = screen.getAllByLabelText("common.delete");
    fireEvent.click(deleteButtons[0]);
    expect(screen.queryByDisplayValue("English")).not.toBeInTheDocument();
  });
});

// === SECTION 3 END ===

describe("SubtitleStreamEditorDialog - 保存", () => {
  it("保存调用 editSubtitleStreams", async () => {
    const user = userEvent.setup();
    const { onOpenChange, onSaved } = renderDialog();
    // 先修改标题让 hasChanges=true
    const titleInput = screen.getByDisplayValue("English");
    await user.clear(titleInput);
    await user.type(titleInput, "Modified");
    // 点击保存按钮
    const saveBtn = screen.getByText("common.save");
    await user.click(saveBtn);
    // 确认
    await waitFor(() => {
      expect(screen.getByText("common.confirm")).toBeInTheDocument();
    });
    await user.click(screen.getByText("common.confirm"));
    await waitFor(() => {
      expect(mockEditSubtitleStreams).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(onSaved).toHaveBeenCalled();
    });
  });

  it("空间不足时弹出保存对话框", async () => {
    mockCheckMergeSpace.mockResolvedValue({ video_size: 100000, free_space: 100, enough: false });
    mockSave.mockResolvedValue("/output/video.edited.mkv");
    const user = userEvent.setup();
    renderDialog();
    const titleInput = screen.getByDisplayValue("English");
    await user.clear(titleInput);
    await user.type(titleInput, "Modified");
    await user.click(screen.getByText("common.save"));
    await waitFor(() => screen.getByText("common.confirm"));
    await user.click(screen.getByText("common.confirm"));
    await waitFor(() => {
      expect(mockSave).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(mockEditSubtitleStreams).toHaveBeenCalledWith(
        "/test/video.mkv",
        expect.any(Array),
        "/output/video.edited.mkv",
      );
    });
  });

  it("空间不足时 defaultPath 使用平台正确的路径分隔符（macOS/Linux 正斜杠）", async () => {
    // videoPath 用 unix 风格路径（正斜杠），defaultPath 应拼接为 /test/video.edited.mkv
    mockCheckMergeSpace.mockResolvedValue({ video_size: 100000, free_space: 100, enough: false });
    mockSave.mockResolvedValue(null);
    const user = userEvent.setup();
    renderDialog();
    const titleInput = screen.getByDisplayValue("English");
    await user.clear(titleInput);
    await user.type(titleInput, "Modified");
    await user.click(screen.getByText("common.save"));
    await waitFor(() => screen.getByText("common.confirm"));
    await user.click(screen.getByText("common.confirm"));
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    const defaultPath = mockSave.mock.calls[0][0].defaultPath as string;
    // 正斜杠路径不应包含反斜杠，且应保留目录结构
    expect(defaultPath).not.toContain("\\");
    expect(defaultPath).toBe("/test/video.edited.mkv");
  });

  it("空间不足时 defaultPath 对 Windows 风格路径使用反斜杠", async () => {
    // videoPath 用 windows 风格路径（反斜杠），defaultPath 应拼接为 C:\test\video.edited.mkv
    mockCheckMergeSpace.mockResolvedValue({ video_size: 100000, free_space: 100, enough: false });
    mockSave.mockResolvedValue(null);
    const user = userEvent.setup();
    renderDialog({} as any); // 先用默认 props 渲染
    // 覆盖 videoPath 需要重新渲染，这里直接验证逻辑：用 windows 路径单独渲染
    cleanup();
    // JSX 字符串属性不处理转义，用 JS 表达式传递含反斜杠的路径
    const winPath = "C:\\test\\video.mkv";
    render(
      <SubtitleStreamEditorDialog
        open={true}
        onOpenChange={vi.fn()}
        videoPath={winPath}
        streams={[makeStream(0)]}
        onSaved={vi.fn()}
      />,
    );
    const titleInput = screen.getByDisplayValue("English");
    await user.clear(titleInput);
    await user.type(titleInput, "Modified");
    await user.click(screen.getByText("common.save"));
    await waitFor(() => screen.getByText("common.confirm"));
    await user.click(screen.getByText("common.confirm"));
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    const defaultPath = mockSave.mock.calls[0][0].defaultPath as string;
    expect(defaultPath).toBe("C:\\test\\video.edited.mkv");
  });

  it("空间不足且用户取消保存时不调用 editSubtitleStreams", async () => {
    mockCheckMergeSpace.mockResolvedValue({ video_size: 100000, free_space: 100, enough: false });
    mockSave.mockResolvedValue(null);
    const user = userEvent.setup();
    renderDialog();
    const titleInput = screen.getByDisplayValue("English");
    await user.clear(titleInput);
    await user.type(titleInput, "Modified");
    await user.click(screen.getByText("common.save"));
    await waitFor(() => screen.getByText("common.confirm"));
    await user.click(screen.getByText("common.confirm"));
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockEditSubtitleStreams).not.toHaveBeenCalled();
  });
});

// === SECTION 4 END ===

describe("SubtitleStreamEditorDialog - 导出", () => {
  it("导出字幕流调用 extractSubtitle", async () => {
    mockSave.mockResolvedValue("/output/sub.srt");
    renderDialog();
    const exportButtons = screen.getAllByLabelText("common.export");
    fireEvent.click(exportButtons[0]);
    await waitFor(() => {
      expect(mockExtractSubtitle).toHaveBeenCalledWith("/test/video.mkv", 0, "/output/sub.srt");
    });
  });

  it("用户取消导出时不调用 extractSubtitle", async () => {
    mockSave.mockResolvedValue(null);
    renderDialog();
    const exportButtons = screen.getAllByLabelText("common.export");
    fireEvent.click(exportButtons[0]);
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockExtractSubtitle).not.toHaveBeenCalled();
  });
});

// === SECTION 5 END ===
