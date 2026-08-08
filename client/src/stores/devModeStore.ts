// 开发者模式状态 store
// 开启方式：关于页面点击版本号 7 下
// 关闭方式：再点击 7 下，或重启程序 10 次后自动关闭
import { create } from "zustand";
import { api } from "../lib/api";
import { setDevModeEnabled } from "../lib/logger";

/// 自动关闭开发者模式所需的重启次数
const AUTO_DISABLE_RESTART_COUNT = 10;

interface DevModeState {
  devMode: boolean;
  initialized: boolean;
  /// "全量记录翻译数据"开关（仅在 devMode 开启时生效）
  logApiEnabled: boolean;
  /// "人名精译"开关（在开发者模式中启用，退出开发模式后仍保持启用，默认禁用）
  namePrecisionEnabled: boolean;
  /// 更新通道：stable（稳定版）或 nightly（每日构建）
  updateChannel: "stable" | "nightly";
  /// 测试版本号覆盖（开发者测试用，模拟旧版本检查更新）
  testVersionOverride: string;
  // 启动时调用：检查重启计数，若 devMode 开启且重启达 10 次则自动关闭
  initOnStartup: () => Promise<void>;
  // 切换开发者模式（关于页面点击 7 下时调用）
  toggle: () => Promise<void>;
  setDevMode: (v: boolean) => void;
  // 切换"全量记录翻译数据"开关
  toggleLogApi: () => Promise<void>;
  setLogApiEnabled: (v: boolean) => void;
  // 切换"人名精译"开关
  toggleNamePrecision: () => Promise<void>;
  setNamePrecisionEnabled: (v: boolean) => void;
  // 设置更新通道
  setUpdateChannel: (channel: "stable" | "nightly") => Promise<void>;
  setUpdateChannelState: (channel: "stable" | "nightly") => void;
  // 设置测试版本号覆盖
  setTestVersionOverride: (version: string) => Promise<void>;
  setTestVersionOverrideState: (version: string) => void;
}

export const useDevModeStore = create<DevModeState>((set, get) => ({
  devMode: false,
  initialized: false,
  logApiEnabled: false,
  namePrecisionEnabled: false,
  updateChannel: "stable",
  testVersionOverride: "",

  initOnStartup: async () => {
    if (get().initialized) return;
    try {
      const devModeStr = await api.getConfig("dev_mode");
      const isDevMode = devModeStr === "true";
      // 读取"全量记录翻译数据"开关（持久化，独立于 devMode）
      const logApiStr = await api.getConfig("dev_log_api_enabled");
      const logApiEnabled = logApiStr === "true";
      // 读取"人名精译"开关（持久化，独立于 devMode，退出开发模式后仍保持）
      const namePrecisionStr = await api.getConfig("name_precision_enabled");
      const namePrecisionEnabled = namePrecisionStr === "true";
      // 读取更新通道（持久化，默认 stable）
      const channelStr = await api.getConfig("update_channel");
      const updateChannel = channelStr === "nightly" ? "nightly" : "stable";
      // 读取测试版本号覆盖（开发者测试用）
      const testVersionOverride = (await api.getConfig("test_version_override")) || "";
      if (isDevMode) {
        // 读取重启计数
        const countStr = await api.getConfig("dev_mode_restart_count");
        const count = countStr ? parseInt(countStr, 10) : 0;
        const newCount = count + 1;
        if (newCount >= AUTO_DISABLE_RESTART_COUNT) {
          // 重启达 10 次，自动关闭开发者模式
          await api.setConfig("dev_mode", "false");
          await api.setConfig("dev_mode_restart_count", "0");
          set({ devMode: false, initialized: true, logApiEnabled, namePrecisionEnabled, updateChannel, testVersionOverride });
          setDevModeEnabled(false);
          api.setDevMode(false).catch(() => {});
          // devMode 关闭后，后端 should_log_api() 自然返回 false
        } else {
          await api.setConfig("dev_mode_restart_count", String(newCount));
          set({ devMode: true, initialized: true, logApiEnabled, namePrecisionEnabled, updateChannel, testVersionOverride });
          setDevModeEnabled(true);
          // 同步到后端
          api.setDevMode(true).catch(() => {});
          api.setLogApiEnabled(logApiEnabled).catch(() => {});
          // release 构建需要主动打开 DevTools
          api.toggleDevtools(true).catch(() => {});
        }
      } else {
        await api.setConfig("dev_mode_restart_count", "0");
        set({ devMode: false, initialized: true, logApiEnabled, namePrecisionEnabled, updateChannel, testVersionOverride });
        setDevModeEnabled(false);
        api.setDevMode(false).catch(() => {});
      }
    } catch {
      set({ devMode: false, initialized: true, logApiEnabled: false, namePrecisionEnabled: false, updateChannel: "stable", testVersionOverride: "" });
      setDevModeEnabled(false);
      api.setDevMode(false).catch(() => {});
    }
  },

  toggle: async () => {
    const newDevMode = !get().devMode;
    set({ devMode: newDevMode });
    setDevModeEnabled(newDevMode);
    try {
      await api.setConfig("dev_mode", String(newDevMode));
      // 切换时重置重启计数
      await api.setConfig("dev_mode_restart_count", "0");
      // 同步到后端
      api.setDevMode(newDevMode).catch(() => {});
      // 同步当前 logApiEnabled 状态到后端
      api.setLogApiEnabled(get().logApiEnabled).catch(() => {});
      // 同步打开/关闭 DevTools
      api.toggleDevtools(newDevMode).catch(() => {});
    } catch {
      // 回滚
      set({ devMode: !newDevMode });
      setDevModeEnabled(!newDevMode);
      api.setDevMode(!newDevMode).catch(() => {});
    }
  },

  setDevMode: (v) => set({ devMode: v }),

  toggleLogApi: async () => {
    const newVal = !get().logApiEnabled;
    set({ logApiEnabled: newVal });
    try {
      await api.setConfig("dev_log_api_enabled", String(newVal));
      // 同步到后端（后端会结合 devMode 判断是否记录）
      api.setLogApiEnabled(newVal).catch(() => {});
    } catch {
      // 回滚
      set({ logApiEnabled: !newVal });
    }
  },

  setLogApiEnabled: (v) => set({ logApiEnabled: v }),

  toggleNamePrecision: async () => {
    const newVal = !get().namePrecisionEnabled;
    set({ namePrecisionEnabled: newVal });
    try {
      await api.setConfig("name_precision_enabled", String(newVal));
    } catch {
      // 回滚
      set({ namePrecisionEnabled: !newVal });
    }
  },

  setNamePrecisionEnabled: (v) => set({ namePrecisionEnabled: v }),

  setUpdateChannel: async (channel) => {
    set({ updateChannel: channel });
    try {
      await api.setConfig("update_channel", channel);
    } catch {
      // 回滚
      set({ updateChannel: channel === "stable" ? "nightly" : "stable" });
    }
  },

  setUpdateChannelState: (channel) => set({ updateChannel: channel }),

  setTestVersionOverride: async (version) => {
    set({ testVersionOverride: version });
    try {
      await api.setConfig("test_version_override", version);
    } catch {
      // 忽略错误
    }
  },

  setTestVersionOverrideState: (version) => set({ testVersionOverride: version }),
}));
