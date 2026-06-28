// 开发者模式状态 store
// 开启方式：关于页面点击版本号 7 下
// 关闭方式：再点击 7 下，或重启程序 3 次后自动关闭
import { create } from "zustand";
import { api } from "../lib/api";

interface DevModeState {
  devMode: boolean;
  initialized: boolean;
  // 启动时调用：检查重启计数，若 devMode 开启且重启达 3 次则自动关闭
  initOnStartup: () => Promise<void>;
  // 切换开发者模式（关于页面点击 7 下时调用）
  toggle: () => Promise<void>;
  setDevMode: (v: boolean) => void;
}

export const useDevModeStore = create<DevModeState>((set, get) => ({
  devMode: false,
  initialized: false,

  initOnStartup: async () => {
    if (get().initialized) return;
    try {
      const devModeStr = await api.getConfig("dev_mode");
      const isDevMode = devModeStr === "true";
      if (isDevMode) {
        // 读取重启计数
        const countStr = await api.getConfig("dev_mode_restart_count");
        const count = countStr ? parseInt(countStr, 10) : 0;
        const newCount = count + 1;
        if (newCount >= 3) {
          // 重启达 3 次，自动关闭开发者模式
          await api.setConfig("dev_mode", "false");
          await api.setConfig("dev_mode_restart_count", "0");
          set({ devMode: false, initialized: true });
        } else {
          await api.setConfig("dev_mode_restart_count", String(newCount));
          set({ devMode: true, initialized: true });
          // release 构建需要主动打开 DevTools
          api.toggleDevtools(true).catch(() => {});
        }
      } else {
        await api.setConfig("dev_mode_restart_count", "0");
        set({ devMode: false, initialized: true });
      }
    } catch {
      set({ devMode: false, initialized: true });
    }
  },

  toggle: async () => {
    const newDevMode = !get().devMode;
    set({ devMode: newDevMode });
    try {
      await api.setConfig("dev_mode", String(newDevMode));
      // 切换时重置重启计数
      await api.setConfig("dev_mode_restart_count", "0");
      // 同步打开/关闭 DevTools
      api.toggleDevtools(newDevMode).catch(() => {});
    } catch {
      // 回滚
      set({ devMode: !newDevMode });
    }
  },

  setDevMode: (v) => set({ devMode: v }),
}));
