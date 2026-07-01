// FfmpegDownloadDialog 组件测试
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { act } from "react";
import userEvent from "@testing-library/user-event";
import { FfmpegDownloadDialog } from "../../components/FfmpegDownloadDialog";

const { mockDownloadFfmpeg, mockUnlisten } = vi.hoisted(() => ({
  mockDownloadFfmpeg: vi.fn(),
  mockUnlisten: vi.fn(),
}));

let eventHandler: ((event: any) => void) | null = null;

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_event: string, handler: (event: any) => void) => {
    eventHandler = handler;
    return Promise.resolve(mockUnlisten);
  }),
}));

vi.mock("../../lib/api", () => ({
  api: {
    downloadFfmpeg: mockDownloadFfmpeg,
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

function renderDialog(props: { open?: boolean } = {}) {
  const onDownloaded = vi.fn();
  const onCancel = vi.fn();
  const { open = true } = props;
  const { unmount } = render(
    <FfmpegDownloadDialog open={open} onDownloaded={onDownloaded} onCancel={onCancel} />,
  );
  return { onDownloaded, onCancel, unmount };
}

beforeEach(() => {
  vi.clearAllMocks();
  eventHandler = null;
  mockDownloadFfmpeg.mockResolvedValue(undefined);
});

// === SECTION 1 END ===

describe("FfmpegDownloadDialog - 渲染", () => {
  it("显示标题和描述", () => {
    renderDialog();
    expect(screen.getByText("subtitle.ffmpegRequired.title")).toBeInTheDocument();
    expect(screen.getByText("subtitle.ffmpegRequired.message")).toBeInTheDocument();
  });

  it("idle 状态显示下载和取消按钮", () => {
    renderDialog();
    expect(screen.getByText("common.download")).toBeInTheDocument();
    expect(screen.getByText("common.cancel")).toBeInTheDocument();
  });

  it("注册进度监听", () => {
    renderDialog();
    expect(eventHandler).not.toBeNull();
  });

  it("卸载时取消监听", async () => {
    const { unmount } = renderDialog();
    unmount();
    await waitFor(() => expect(mockUnlisten).toHaveBeenCalled());
  });
});

// === SECTION 2 END ===

describe("FfmpegDownloadDialog - 下载", () => {
  it("点击下载触发 downloadFfmpeg", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("common.download"));
    expect(mockDownloadFfmpeg).toHaveBeenCalled();
  });

  it("下载中显示进度", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("common.download"));
    await act(async () => {
      eventHandler?.({ payload: { stage: "downloading", progress: 50, speed_mbps: 2, eta_secs: 30 } });
    });
    await waitFor(() => {
      expect(screen.getByText("请稍候...")).toBeInTheDocument();
    });
  });

  it("下载完成自动回调 onDownloaded", async () => {
    const { onDownloaded } = renderDialog();
    const user = userEvent.setup();
    await user.click(screen.getByText("common.download"));
    await act(async () => {
      eventHandler?.({ payload: { stage: "done", progress: 100 } });
    });
    await waitFor(() => {
      expect(onDownloaded).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it("下载失败显示重试按钮", async () => {
    mockDownloadFfmpeg.mockRejectedValue(new Error("network error"));
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByText("common.download"));
    await waitFor(() => {
      expect(screen.getByText("subtitle.ffmpegRequired.retry")).toBeInTheDocument();
    }, { timeout: 3000 });
  });

  it("点击取消触发 onCancel", async () => {
    const user = userEvent.setup();
    const { onCancel } = renderDialog();
    await user.click(screen.getByText("common.cancel"));
    await waitFor(() => {
      expect(onCancel).toHaveBeenCalled();
    });
  });
});

// === SECTION 3 END ===
