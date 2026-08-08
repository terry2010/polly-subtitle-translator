// authStore 单元测试（FP-P0-6）
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useAuthStore } from "../../stores/authStore";

const {
  mockAuthLogin,
  mockAuthLogout,
  mockAuthRefresh,
  mockAuthGetUserInfo,
  mockAuthInitOnStartup,
} = vi.hoisted(() => ({
  mockAuthLogin: vi.fn(),
  mockAuthLogout: vi.fn(),
  mockAuthRefresh: vi.fn(),
  mockAuthGetUserInfo: vi.fn(),
  mockAuthInitOnStartup: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  api: {
    authLogin: mockAuthLogin,
    authLogout: mockAuthLogout,
    authRefresh: mockAuthRefresh,
    authGetUserInfo: mockAuthGetUserInfo,
    authInitOnStartup: mockAuthInitOnStartup,
  },
  formatIpcError: (e: any) => e?.code ?? String(e),
}));

const mockUser = {
  id: 1,
  email: "test@example.com",
  token_balance: 1000,
  bonus_balance: 3300,
  traffic_pack_balance: 0,
  status: "active",
  vip_level: "vip1",
  vip_expires_at: null,
  bonus_expires_at: "2026-07-25T00:00:00Z",
  traffic_pack_expires_at: null,
  max_concurrent_jobs: 2,
  available_models: [],
};

function getStore() {
  return useAuthStore.getState();
}

function resetStore() {
  useAuthStore.setState({
    status: "idle",
    user: null,
    offline: false,
    error: null,
    initialized: false,
  });
}

beforeEach(() => {
  resetStore();
  vi.clearAllMocks();
});

// === SECTION 1 END ===

describe("authStore - initOnStartup", () => {
  it("有用户信息时设为 logged_in", async () => {
    mockAuthInitOnStartup.mockResolvedValue({
      user: mockUser,
      offline: false,
      error: null,
    });
    await getStore().initOnStartup();
    expect(getStore().status).toBe("logged_in");
    expect(getStore().user).toEqual(mockUser);
    expect(getStore().offline).toBe(false);
    expect(getStore().initialized).toBe(true);
  });

  it("离线模式时设为 offline", async () => {
    mockAuthInitOnStartup.mockResolvedValue({
      user: null,
      offline: true,
      error: "网络错误",
    });
    await getStore().initOnStartup();
    expect(getStore().status).toBe("offline");
    expect(getStore().user).toBeNull();
    expect(getStore().offline).toBe(true);
    expect(getStore().error).toBe("网络错误");
  });

  it("未登录时设为 idle", async () => {
    mockAuthInitOnStartup.mockResolvedValue({
      user: null,
      offline: false,
      error: "未登录",
    });
    await getStore().initOnStartup();
    expect(getStore().status).toBe("idle");
    expect(getStore().user).toBeNull();
    expect(getStore().error).toBe("未登录");
  });

  it("IPC 异常时设为 error", async () => {
    mockAuthInitOnStartup.mockRejectedValue({ code: "auth.networkError" });
    await getStore().initOnStartup();
    expect(getStore().status).toBe("error");
    expect(getStore().error).toBe("auth.networkError");
    expect(getStore().initialized).toBe(true);
  });

  it("已初始化时不重复调用", async () => {
    useAuthStore.setState({ initialized: true });
    await getStore().initOnStartup();
    expect(mockAuthInitOnStartup).not.toHaveBeenCalled();
  });
});

// === SECTION 2 END ===

describe("authStore - login", () => {
  it("登录成功设为 logged_in", async () => {
    mockAuthLogin.mockResolvedValue({ user: mockUser, expires_in: 3600 });
    await getStore().login();
    expect(getStore().status).toBe("logged_in");
    expect(getStore().user).toEqual(mockUser);
  });

  it("登录前设为 logging_in", async () => {
    mockAuthLogin.mockResolvedValue({ user: mockUser, expires_in: 3600 });
    const promise = getStore().login();
    expect(getStore().status).toBe("logging_in");
    await promise;
  });

  it("登录失败设为 error", async () => {
    mockAuthLogin.mockRejectedValue({ code: "auth.loginFailed" });
    await getStore().login();
    expect(getStore().status).toBe("error");
    expect(getStore().error).toBe("auth.loginFailed");
  });
});

// === SECTION 3 END ===

describe("authStore - logout", () => {
  it("登出清除状态", async () => {
    useAuthStore.setState({ status: "logged_in", user: mockUser });
    mockAuthLogout.mockResolvedValue(undefined);
    await getStore().logout();
    expect(getStore().status).toBe("idle");
    expect(getStore().user).toBeNull();
  });

  it("登出 IPC 失败仍清除前端状态", async () => {
    useAuthStore.setState({ status: "logged_in", user: mockUser });
    mockAuthLogout.mockRejectedValue(new Error("network"));
    await getStore().logout();
    expect(getStore().status).toBe("idle");
    expect(getStore().user).toBeNull();
  });
});

// === SECTION 4 END ===

describe("authStore - refresh", () => {
  it("刷新成功更新用户信息", async () => {
    useAuthStore.setState({ status: "logged_in", user: mockUser });
    mockAuthRefresh.mockResolvedValue({ ...mockUser, id: 2 });
    await getStore().refresh();
    expect(getStore().user?.id).toBe(2);
  });

  it("刷新返回 null 不改变状态", async () => {
    useAuthStore.setState({ status: "logged_in", user: mockUser });
    mockAuthRefresh.mockResolvedValue(null);
    await getStore().refresh();
    expect(getStore().user).toEqual(mockUser);
  });

  it("刷新失败设 error", async () => {
    mockAuthRefresh.mockRejectedValue({ code: "auth.refreshFailed" });
    await getStore().refresh();
    expect(getStore().error).toBe("auth.refreshFailed");
  });
});

// === SECTION 5 END ===

describe("authStore - fetchUserInfo", () => {
  it("成功更新用户信息", async () => {
    mockAuthGetUserInfo.mockResolvedValue(mockUser);
    await getStore().fetchUserInfo();
    expect(getStore().user).toEqual(mockUser);
    expect(getStore().status).toBe("logged_in");
  });

  it("失败设 error", async () => {
    mockAuthGetUserInfo.mockRejectedValue({ code: "auth.networkError" });
    await getStore().fetchUserInfo();
    expect(getStore().error).toBe("auth.networkError");
  });
});

// === SECTION 6 END ===

describe("authStore - clearError", () => {
  it("清除 error 字段", () => {
    useAuthStore.setState({ error: "some error" });
    getStore().clearError();
    expect(getStore().error).toBeNull();
  });
});

// === SECTION 7 END ===
