// libmpvStore 单元测试
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useLibmpvStore } from "../../stores/libmpvStore";

const { mockGetLibmpvStatus, mockDownloadLibmpv } = vi.hoisted(() => ({
  mockGetLibmpvStatus: vi.fn(),
  mockDownloadLibmpv: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    getLibmpvStatus: mockGetLibmpvStatus,
    downloadLibmpv: mockDownloadLibmpv,
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

function getStore() {
  return useLibmpvStore.getState();
}

function resetStore() {
  useLibmpvStore.setState({
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

describe("libmpvStore - onProgressEvent", () => {
  it("progress 更新进度", () => {
    getStore().onProgressEvent({ stage: "downloading", progress: 50, message: "下载中" });
    expect(getStore().downloadStage).toBe("downloading");
    expect(getStore().downloadProgress).toBe(50);
    expect(getStore().downloadMessage).toBe("下载中");
  });

  it("done 完成下载并刷新状态", () => {
    getStore().onProgressEvent({ stage: "done", progress: 100, message: "完成" });
    expect(getStore().downloading).toBe(false);
    expect(getStore().downloadProgress).toBe(100);
    expect(getStore().downloadStage).toBe("done");
  });

  it("failed 设置错误", () => {
    getStore().onProgressEvent({ stage: "failed", progress: 0, message: "下载失败", code: "NETWORK_ERROR" });
    expect(getStore().downloading).toBe(false);
    expect(getStore().downloadStage).toBe("failed");
    expect(getStore().downloadError).toBe("NETWORK_ERROR");
  });
});

// === SECTION 2 END ===

describe("libmpvStore - refreshStatus", () => {
  it("成功获取状态", async () => {
    const status = { downloaded: true, path: "/libmpv.dylib", version: "0.38" };
    mockGetLibmpvStatus.mockResolvedValue(status);
    await getStore().refreshStatus();
    expect(getStore().status).toEqual(status);
    expect(getStore().statusLoading).toBe(false);
  });

  it("失败时设置默认状态", async () => {
    mockGetLibmpvStatus.mockRejectedValue(new Error("失败"));
    await getStore().refreshStatus();
    expect(getStore().status).toEqual({ downloaded: false, path: null, version: null });
    expect(getStore().statusLoading).toBe(false);
  });
});

// === SECTION 3 END ===

describe("libmpvStore - startDownload", () => {
  it("开始下载并更新状态", async () => {
    mockDownloadLibmpv.mockResolvedValue(undefined);
    mockGetLibmpvStatus.mockResolvedValue({ downloaded: true, path: null, version: null });
    await getStore().startDownload();
    // 下载完成后由进度事件把 downloading 置 false
    expect(getStore().downloading).toBe(true);
    expect(mockDownloadLibmpv).toHaveBeenCalled();
  });

  it("下载中不允许重复启动", async () => {
    useLibmpvStore.setState({ downloading: true });
    await getStore().startDownload();
    expect(mockDownloadLibmpv).not.toHaveBeenCalled();
  });
});

// === SECTION 4 END ===
