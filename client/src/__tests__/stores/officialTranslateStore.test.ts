// 官方翻译 store 单元测试（FP-P1-5）
import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { useOfficialTranslateStore } from "../../stores/officialTranslateStore";
import { useAuthStore } from "../../stores/authStore";
import type { OfficialTranslateError } from "../../lib/ipc-types";

// mock api
vi.mock("../../lib/api", () => ({
  api: {
    translateOfficial: vi.fn(),
  },
  formatIpcError: vi.fn((err: any) => (err && typeof err === "object" && "message" in err ? err.message : null) || String(err)),
}));

// mock toast
vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
  },
}));

// mock @tauri-apps/api/event
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

// mock @tauri-apps/api/core (for invoke in api.ts)
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { api } from "../../lib/api";

const mockFile = {
  format: "srt" as const,
  entries: [
    { index: 0, start_ms: 0, end_ms: 1000, text: "Hello", translated: "", style: null, pre_edit_text: null },
    { index: 1, start_ms: 1000, end_ms: 2000, text: "World", translated: "", style: null, pre_edit_text: null },
  ],
  raw_header: null,
  source_path: null,
  file_hash: "test-hash",
};

describe("officialTranslateStore", () => {
  beforeEach(() => {
    useOfficialTranslateStore.setState({
      status: "idle",
      current: 0,
      total: 0,
      phase: "",
      step: "",
      result: null,
      error: null,
      selectedModel: "polly-standard",
      _unlisteners: [],
    });
    useAuthStore.setState({
      status: "logged_in",
      user: {
        id: 1, email: "test@example.com",
        token_balance: 1000, bonus_balance: 3300, traffic_pack_balance: 0,
        status: "active", vip_level: "vip1",
        vip_expires_at: null, bonus_expires_at: "2026-07-25T00:00:00Z",
        traffic_pack_expires_at: null,
        max_concurrent_jobs: 2, available_models: [],
      },
      offline: false, error: null, initialized: true,
      // mock fetchUserInfo（SSE error 后调 /auth/me 更新余额）
      fetchUserInfo: vi.fn().mockResolvedValue(undefined),
    });
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("初始状态为 idle", () => {
    const state = useOfficialTranslateStore.getState();
    expect(state.status).toBe("idle");
    expect(state.result).toBeNull();
    expect(state.error).toBeNull();
  });

  it("setSelectedModel 设置翻译档位", () => {
    useOfficialTranslateStore.getState().setSelectedModel("polly-fine");
    expect(useOfficialTranslateStore.getState().selectedModel).toBe("polly-fine");
  });

  it("translate 成功时更新 result 和 status", async () => {
    const mockResult = {
      entries: mockFile.entries,
      tokens_used: 100,
      cost: 50,
      token_balance: 900,
      bonus_balance: 3300,
    };
    (api.translateOfficial as any).mockResolvedValue(mockResult);

    await useOfficialTranslateStore.getState().translate(mockFile);

    const state = useOfficialTranslateStore.getState();
    expect(state.status).toBe("completed");
    expect(state.result).toEqual(mockResult);
    expect(state.error).toBeNull();
  });

  it("translate 开始时设置 translating 状态", async () => {
    let resolveFn: (value: any) => void;
    (api.translateOfficial as any).mockReturnValue(
      new Promise((resolve) => { resolveFn = resolve; })
    );

    const promise = useOfficialTranslateStore.getState().translate(mockFile);
    expect(useOfficialTranslateStore.getState().status).toBe("translating");
    expect(useOfficialTranslateStore.getState().total).toBe(2);

    resolveFn!({ entries: [], tokens_used: 0, cost: 0, token_balance: null, bonus_balance: null });
    await promise;
  });

  it("translate 失败时设置 error 状态", async () => {
    (api.translateOfficial as any).mockRejectedValue({ message: "网络错误" });

    await useOfficialTranslateStore.getState().translate(mockFile);

    const state = useOfficialTranslateStore.getState();
    expect(state.status).toBe("error");
    expect(state.error).toBe("网络错误");
  });

  it("translate 成功后同步余额到 authStore", async () => {
    const mockResult = {
      entries: [],
      tokens_used: 100,
      cost: 50,
      token_balance: 800,
      bonus_balance: 3200,
    };
    (api.translateOfficial as any).mockResolvedValue(mockResult);

    await useOfficialTranslateStore.getState().translate(mockFile);

    const authUser = useAuthStore.getState().user;
    expect(authUser?.token_balance).toBe(800);
    expect(authUser?.bonus_balance).toBe(3200);
  });

  it("translate 成功后余额为 null 时不覆盖 authStore", async () => {
    const mockResult = {
      entries: [],
      tokens_used: 100,
      cost: 50,
      token_balance: null,
      bonus_balance: null,
    };
    (api.translateOfficial as any).mockResolvedValue(mockResult);

    await useOfficialTranslateStore.getState().translate(mockFile);

    const authUser = useAuthStore.getState().user;
    expect(authUser?.token_balance).toBe(1000); // 保持原值
    expect(authUser?.bonus_balance).toBe(3300);
  });

  it("reset 重置所有状态", () => {
    useOfficialTranslateStore.setState({
      status: "completed",
      current: 100,
      total: 200,
      result: {} as any,
      error: "some error",
    });
    useOfficialTranslateStore.getState().reset();
    const state = useOfficialTranslateStore.getState();
    expect(state.status).toBe("idle");
    expect(state.current).toBe(0);
    expect(state.total).toBe(0);
    expect(state.result).toBeNull();
    expect(state.error).toBeNull();
  });

  it("translate 失败时显示 toast", async () => {
    (api.translateOfficial as any).mockRejectedValue({ message: "余额不足" });

    await useOfficialTranslateStore.getState().translate(mockFile);

    const { toast } = await import("sonner");
    expect(toast.error).toHaveBeenCalledWith("余额不足");
  });

  it("SSE error 事件触发后调 fetchUserInfo 更新余额", async () => {
    // 验证 SSE error 事件后调 /auth/me 更新余额（联调文档 02-P1 第 251-258 行）
    const { listen } = await import("@tauri-apps/api/event");
    const mockListen = listen as any;

    // 捕获 error listener 回调
    let errorCallback: ((event: { payload: OfficialTranslateError }) => void) | null = null;
    mockListen.mockImplementation((event: string, cb: (e: any) => void) => {
      if (event === "official-translate-error") {
        errorCallback = cb;
      }
      return Promise.resolve(() => {});
    });

    (api.translateOfficial as any).mockResolvedValue({
      entries: [], tokens_used: 0, cost: 0, token_balance: null, bonus_balance: null,
    });

    await useOfficialTranslateStore.getState().translate(mockFile);

    // 手动触发 error 事件
    expect(errorCallback).not.toBeNull();
    errorCallback!({
      payload: {
        phase: "translate",
        step: "translate",
        error_code: "translation_failed",
        message: "翻译失败",
      },
    });

    // 验证 fetchUserInfo 被调用
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(useAuthStore.getState().fetchUserInfo).toHaveBeenCalled();
  });
});
