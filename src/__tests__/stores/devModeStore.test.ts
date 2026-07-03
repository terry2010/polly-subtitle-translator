// devModeStore 单元测试
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useDevModeStore } from "../../stores/devModeStore";

const { mockGetConfig, mockSetConfig, mockToggleDevtools, mockSetDevMode, mockSetLogApiEnabled } = vi.hoisted(() => ({
  mockGetConfig: vi.fn(),
  mockSetConfig: vi.fn(),
  mockToggleDevtools: vi.fn(() => Promise.resolve()),
  mockSetDevMode: vi.fn(() => Promise.resolve()),
  mockSetLogApiEnabled: vi.fn(() => Promise.resolve()),
}));

vi.mock("../../lib/api", () => ({
  api: {
    getConfig: mockGetConfig,
    setConfig: mockSetConfig,
    toggleDevtools: mockToggleDevtools,
    setDevMode: mockSetDevMode,
    setLogApiEnabled: mockSetLogApiEnabled,
  },
}));

function getStore() {
  return useDevModeStore.getState();
}

function resetStore() {
  useDevModeStore.setState({ devMode: false, initialized: false, logApiEnabled: false });
}

beforeEach(() => {
  resetStore();
  vi.clearAllMocks();
});

// === SECTION 1 END ===

describe("devModeStore - initOnStartup", () => {
  it("devMode 关闭时初始化完成且不开启", async () => {
    mockGetConfig.mockResolvedValue("false");
    await getStore().initOnStartup();
    expect(getStore().devMode).toBe(false);
    expect(getStore().initialized).toBe(true);
    expect(mockSetConfig).toHaveBeenCalledWith("dev_mode_restart_count", "0");
  });

  it("devMode 开启时累加重启计数", async () => {
    mockGetConfig.mockImplementation(async (key: string) => {
      if (key === "dev_mode") return "true";
      if (key === "dev_mode_restart_count") return "1";
      return null;
    });
    await getStore().initOnStartup();
    expect(getStore().devMode).toBe(true);
    expect(mockSetConfig).toHaveBeenCalledWith("dev_mode_restart_count", "2");
    expect(mockToggleDevtools).toHaveBeenCalledWith(true);
  });

  it("重启 10 次后自动关闭 devMode", async () => {
    mockGetConfig.mockImplementation(async (key: string) => {
      if (key === "dev_mode") return "true";
      if (key === "dev_mode_restart_count") return "9";
      if (key === "dev_log_api_enabled") return "false";
      return null;
    });
    await getStore().initOnStartup();
    expect(getStore().devMode).toBe(false);
    expect(mockSetConfig).toHaveBeenCalledWith("dev_mode", "false");
    expect(mockSetConfig).toHaveBeenCalledWith("dev_mode_restart_count", "0");
  });

  it("重启 9 次不自动关闭 devMode", async () => {
    mockGetConfig.mockImplementation(async (key: string) => {
      if (key === "dev_mode") return "true";
      if (key === "dev_mode_restart_count") return "8";
      if (key === "dev_log_api_enabled") return "false";
      return null;
    });
    await getStore().initOnStartup();
    expect(getStore().devMode).toBe(true);
    expect(mockSetConfig).toHaveBeenCalledWith("dev_mode_restart_count", "9");
  });

  it("已经初始化过则不再执行", async () => {
    useDevModeStore.setState({ initialized: true });
    await getStore().initOnStartup();
    expect(mockGetConfig).not.toHaveBeenCalled();
  });
});

// === SECTION 2 END ===

describe("devModeStore - toggle", () => {
  it("开启 devMode 并持久化", async () => {
    await getStore().toggle();
    expect(getStore().devMode).toBe(true);
    expect(mockSetConfig).toHaveBeenCalledWith("dev_mode", "true");
    expect(mockSetConfig).toHaveBeenCalledWith("dev_mode_restart_count", "0");
    expect(mockToggleDevtools).toHaveBeenCalledWith(true);
  });

  it("关闭 devMode 并持久化", async () => {
    useDevModeStore.setState({ devMode: true });
    await getStore().toggle();
    expect(getStore().devMode).toBe(false);
    expect(mockSetConfig).toHaveBeenCalledWith("dev_mode", "false");
    expect(mockToggleDevtools).toHaveBeenCalledWith(false);
  });
});

// === SECTION 3 END ===
