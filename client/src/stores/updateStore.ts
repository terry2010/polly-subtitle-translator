// 应用更新 store
// 启动时自动检查更新，管理更新弹窗状态
import { create } from "zustand";
import { api } from "../lib/api";

interface UpdateState {
  dialogOpen: boolean;
  updateInfo: { version: string; notes: string } | null;
  checking: boolean;
  lastCheckFailed: boolean;

  // 启动时自动检查（静默，有更新才弹窗）
  checkOnStartup: () => Promise<void>;
  // 手动检查（设置页"检查更新"按钮）
  checkManually: () => Promise<"latest" | "available" | "failed">;
  // 关闭弹窗
  closeDialog: () => void;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  dialogOpen: false,
  updateInfo: null,
  checking: false,
  lastCheckFailed: false,

  checkOnStartup: async () => {
    set({ checking: true });
    try {
      const info = await api.checkForUpdate();
      if (info.available) {
        set({
          dialogOpen: true,
          updateInfo: { version: info.version, notes: info.notes },
          checking: false,
        });
      } else {
        set({ checking: false });
      }
    } catch {
      set({ checking: false, lastCheckFailed: true });
    }
  },

  checkManually: async () => {
    set({ checking: true });
    try {
      const info = await api.checkForUpdate();
      set({ checking: false });
      if (info.available) {
        set({
          dialogOpen: true,
          updateInfo: { version: info.version, notes: info.notes },
        });
        return "available";
      }
      return "latest";
    } catch {
      set({ checking: false, lastCheckFailed: true });
      return "failed";
    }
  },

  closeDialog: () => set({ dialogOpen: false }),
}));
