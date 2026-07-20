// 认证状态 store（FP-P0-6）
// 管理登录状态、用户信息、离线模式
import { create } from "zustand";
import { api, formatIpcError } from "../lib/api";
import type { UserInfo } from "../lib/ipc-types";

// initOnStartup 的 promise 锁，防止 React StrictMode 双调用导致并发刷新
let initPromise: Promise<void> | null = null;

type LoginStatus = "idle" | "logging_in" | "logged_in" | "offline" | "error";

interface AuthState {
  /// 当前登录状态
  status: LoginStatus;
  /// 用户信息（登录成功后缓存）
  user: UserInfo | null;
  /// 是否离线模式（网络错误降级）
  offline: boolean;
  /// 错误信息
  error: string | null;
  /// 是否已初始化（启动时调用 authInitOnStartup 后设为 true）
  initialized: boolean;

  /// 应用启动时调用：refresh + /auth/me + 离线降级
  initOnStartup: () => Promise<void>;
  /// 启动 WebSocket 登录流程
  login: () => Promise<void>;
  /// 登出
  logout: () => Promise<void>;
  /// 手动刷新 token
  refresh: () => Promise<void>;
  /// 获取用户信息
  fetchUserInfo: () => Promise<void>;
  /// 清除错误
  clearError: () => void;
}

export const useAuthStore = create<AuthState>((set, get) => ({
  status: "idle",
  user: null,
  offline: false,
  error: null,
  initialized: false,

  initOnStartup: async () => {
    if (get().initialized) return;
    // 防止并发调用（React StrictMode 开发模式下 useEffect 会执行两次）
    // 用 promise 锁：如果已有 initOnStartup 在执行，复用同一个 promise
    if (initPromise) return initPromise;
    initPromise = (async () => {
      try {
        const result = await api.authInitOnStartup();
        if (result.user) {
          set({
            status: "logged_in",
            user: result.user,
            offline: false,
            error: null,
            initialized: true,
          });
        } else if (result.offline) {
          set({
            status: "offline",
            user: null,
            offline: true,
            error: result.error,
            initialized: true,
          });
        } else {
          set({
            status: "idle",
          user: null,
          offline: false,
          error: result.error,
          initialized: true,
        });
      }
    } catch (e: any) {
      set({
        status: "error",
        user: null,
        offline: false,
        error: formatIpcError(e),
        initialized: true,
      });
    } finally {
      initPromise = null;
    }
  })();

  return initPromise;
  },

  login: async () => {
    set({ status: "logging_in", error: null });
    try {
      const result = await api.authLogin();
      set({
        status: "logged_in",
        user: result.user,
        offline: false,
        error: null,
      });
    } catch (e: any) {
      set({
        status: "error",
        error: formatIpcError(e),
      });
    }
  },

  logout: async () => {
    try {
      await api.authLogout();
    } catch {
      // 忽略错误，前端仍清除状态
    }
    set({ status: "idle", user: null, offline: false, error: null });
  },

  refresh: async () => {
    try {
      const user = await api.authRefresh();
      if (user) {
        set({ status: "logged_in", user, offline: false, error: null });
      }
    } catch (e: any) {
      set({ error: formatIpcError(e) });
    }
  },

  fetchUserInfo: async () => {
    try {
      const user = await api.authGetUserInfo();
      set({ user, status: "logged_in" });
    } catch (e: any) {
      set({ error: formatIpcError(e) });
    }
  },

  clearError: () => set({ error: null }),
}));
