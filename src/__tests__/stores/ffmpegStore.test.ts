// ffmpegStore 单元测试
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useFfmpegStore } from "../../stores/ffmpegStore";

const { mockGetFfmpegStatus, mockDownloadFfmpeg } = vi.hoisted(() => ({
  mockGetFfmpegStatus: vi.fn(),
  mockDownloadFfmpeg: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    getFfmpegStatus: mockGetFfmpegStatus,
    downloadFfmpeg: mockDownloadFfmpeg,
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

function getStore() {
  return useFfmpegStore.getState();
}

function resetStore() {
  useFfmpegStore.setState({
    downloading: false,
    downloadProgress: 0,
    downloadStage: "",
    downloadMessage: "",
    downloadError: "",
    downloadSpeedMbps: 0,
    downloadEtaSecs: 0,
    status: null,
    statusLoading: false,
  });
}

beforeEach(() => {
  resetStore();
  vi.clearAllMocks();
});

// === SECTION 1 END ===

describe("ffmpegStore - onProgressEvent", () => {
  it("progress 更新进度", () => {
    getStore().onProgressEvent({ stage: "downloading", progress: 30, message: "下载 FFmpeg" });
    expect(getStore().downloadStage).toBe("downloading");
    expect(getStore().downloadProgress).toBe(30);
  });

  it("done 完成下载", () => {
    getStore().onProgressEvent({ stage: "done", progress: 100, message: "完成" });
    expect(getStore().downloading).toBe(false);
    expect(getStore().downloadStage).toBe("done");
    expect(getStore().downloadProgress).toBe(100);
  });

  it("failed 设置错误", () => {
    getStore().onProgressEvent({ stage: "failed", progress: 0, message: "下载失败", code: "DOWNLOAD_FAILED" });
    expect(getStore().downloading).toBe(false);
    expect(getStore().downloadStage).toBe("failed");
    expect(getStore().downloadError).toBe("DOWNLOAD_FAILED");
  });
});

// === SECTION 2 END ===

describe("ffmpegStore - refreshStatus", () => {
  it("成功获取状态", async () => {
    const status = { installed: true, source: "bundled", path: "/ffmpeg" };
    mockGetFfmpegStatus.mockResolvedValue(status);
    await getStore().refreshStatus();
    expect(getStore().status).toEqual(status);
    expect(getStore().statusLoading).toBe(false);
  });

  it("失败时设置默认状态", async () => {
    mockGetFfmpegStatus.mockRejectedValue(new Error("失败"));
    await getStore().refreshStatus();
    expect(getStore().status).toEqual({ installed: false, source: null, path: null });
    expect(getStore().statusLoading).toBe(false);
  });
});

// === SECTION 3 END ===

describe("ffmpegStore - startDownload", () => {
  it("开始下载", async () => {
    mockDownloadFfmpeg.mockResolvedValue(undefined);
    mockGetFfmpegStatus.mockResolvedValue({ installed: true, source: null, path: null });
    await getStore().startDownload();
    // 下载完成后由进度事件把 downloading 置 false
    expect(getStore().downloading).toBe(true);
    expect(mockDownloadFfmpeg).toHaveBeenCalled();
  });

  it("下载中不允许重复启动", async () => {
    useFfmpegStore.setState({ downloading: true });
    await getStore().startDownload();
    expect(mockDownloadFfmpeg).not.toHaveBeenCalled();
  });
});

// === SECTION 4 END ===
