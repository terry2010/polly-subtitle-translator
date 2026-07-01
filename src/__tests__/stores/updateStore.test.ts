// updateStore 单元测试
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useUpdateStore } from "../../stores/updateStore";

const { mockCheckForUpdate, mockDownloadAndInstall } = vi.hoisted(() => ({
  mockCheckForUpdate: vi.fn(),
  mockDownloadAndInstall: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    checkForUpdate: mockCheckForUpdate,
    downloadAndInstallUpdate: mockDownloadAndInstall,
  },
  formatIpcError: vi.fn((e: unknown) => String(e)),
}));

beforeEach(() => {
  vi.clearAllMocks();
  useUpdateStore.setState({
    dialogOpen: false, updateInfo: null, checking: false, lastCheckFailed: false,
  });
});

// === SECTION 1 END ===

describe("updateStore - checkOnStartup", () => {
  it("有更新时弹窗", async () => {
    mockCheckForUpdate.mockResolvedValue({
      available: true, version: "1.0.1", notes: "修复bug", pub_date: "2024-01-01",
    });
    await useUpdateStore.getState().checkOnStartup();
    const state = useUpdateStore.getState();
    expect(state.dialogOpen).toBe(true);
    expect(state.updateInfo).toEqual({ version: "1.0.1", notes: "修复bug" });
    expect(state.checking).toBe(false);
  });

  it("无更新时不弹窗", async () => {
    mockCheckForUpdate.mockResolvedValue({
      available: false, version: "", notes: "", pub_date: "",
    });
    await useUpdateStore.getState().checkOnStartup();
    expect(useUpdateStore.getState().dialogOpen).toBe(false);
    expect(useUpdateStore.getState().checking).toBe(false);
  });

  it("检查失败设置 lastCheckFailed", async () => {
    mockCheckForUpdate.mockRejectedValue(new Error("网络错误"));
    await useUpdateStore.getState().checkOnStartup();
    expect(useUpdateStore.getState().lastCheckFailed).toBe(true);
    expect(useUpdateStore.getState().checking).toBe(false);
  });
});

// === SECTION 2 END ===

describe("updateStore - checkManually", () => {
  it("有更新返回 available", async () => {
    mockCheckForUpdate.mockResolvedValue({
      available: true, version: "2.0.0", notes: "大版本", pub_date: "",
    });
    const result = await useUpdateStore.getState().checkManually();
    expect(result).toBe("available");
    expect(useUpdateStore.getState().dialogOpen).toBe(true);
  });

  it("无更新返回 latest", async () => {
    mockCheckForUpdate.mockResolvedValue({
      available: false, version: "", notes: "", pub_date: "",
    });
    const result = await useUpdateStore.getState().checkManually();
    expect(result).toBe("latest");
  });

  it("失败返回 failed", async () => {
    mockCheckForUpdate.mockRejectedValue(new Error("err"));
    const result = await useUpdateStore.getState().checkManually();
    expect(result).toBe("failed");
    expect(useUpdateStore.getState().lastCheckFailed).toBe(true);
  });
});

// === SECTION 3 END ===

describe("updateStore - closeDialog", () => {
  it("关闭弹窗", () => {
    useUpdateStore.setState({ dialogOpen: true, updateInfo: { version: "1", notes: "" } });
    useUpdateStore.getState().closeDialog();
    expect(useUpdateStore.getState().dialogOpen).toBe(false);
  });
});

// === SECTION 4 END ===
