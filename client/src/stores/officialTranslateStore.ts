// 官方服务翻译状态 store（FP-P1-5）
// 管理官方服务模式的翻译状态、进度、余额同步
import { create } from "zustand";
import { toast } from "sonner";
import { api, formatIpcError } from "../lib/api";
import type {
  OfficialTranslateParams,
  OfficialTranslateResponse,
  OfficialTranslateProgress,
  OfficialTranslateError,
  OfficialTranslateBalance,
  SubtitleFile,
  SubtitleEntry,
} from "../lib/ipc-types";
import i18n from "../lib/i18n";
import { useAuthStore } from "./authStore";

type OfficialTranslateStatus = "idle" | "translating" | "completed" | "error";

/// partial 事件中的单条字幕（服务端格式）
interface PartialSubtitleEntry {
  index: number;
  translated_text: string;
  original_text: string;
}

interface OfficialTranslateState {
  /// 当前翻译状态
  status: OfficialTranslateStatus;
  /// 进度（当前行号）
  current: number;
  /// 总行数
  total: number;
  /// 当前阶段
  phase: string;
  /// 当前步骤
  step: string;
  /// 翻译结果
  result: OfficialTranslateResponse | null;
  /// 错误信息
  error: string | null;
  /// 选中的翻译档位
  selectedModel: string;
  /// partial 事件增量回传的译文（累计已翻译的全部）
  partialEntries: PartialSubtitleEntry[];
  /// Tauri event unlisten 函数
  _unlisteners: Array<() => void>;

  /// 设置翻译档位
  setSelectedModel: (model: string) => void;
  /// 执行官方翻译
  translate: (file: SubtitleFile) => Promise<void>;
  /// 取消官方翻译（调用 DELETE /v2/translate/{job_id}）
  cancel: () => Promise<void>;
  /// 重置状态
  reset: () => void;
  /// 清理 event listeners
  cleanup: () => void;
}

export const useOfficialTranslateStore = create<OfficialTranslateState>((set, get) => ({
  status: "idle",
  current: 0,
  total: 0,
  phase: "",
  step: "",
  result: null,
  error: null,
  selectedModel: "polly-fast",
  partialEntries: [],
  _unlisteners: [],

  setSelectedModel: (model: string) => set({ selectedModel: model }),

  translate: async (file: SubtitleFile) => {
    const model = get().selectedModel;
    set({
      status: "translating",
      current: 0,
      total: file.entries.length,
      phase: "",
      step: "",
      result: null,
      error: null,
    });

    // 设置 Tauri event listeners
    const { listen } = await import("@tauri-apps/api/event");
    const unlistenProgress = await listen<OfficialTranslateProgress>(
      "official-translate-progress",
      (event) => {
        const { phase, step, current, total } = event.payload;
        set({ phase, step, current, total });
      }
    );
    const unlistenPartial = await listen<{ subtitles: PartialSubtitleEntry[]; current: number; total: number }>(
      "official-translate-partial",
      (event) => {
        const { subtitles, current, total } = event.payload;
        set({ partialEntries: subtitles, current, total });
      }
    );
    const unlistenError = await listen<OfficialTranslateError>(
      "official-translate-error",
      (event) => {
        const { error_code, message } = event.payload;
        set({ status: "error", error: message || error_code });
        toast.error(message || error_code);
        // SSE error 事件不含 token_balance，需调 GET /auth/me 更新余额
        // 联调文档 02-P1 第 251-258 行
        useAuthStore.getState().fetchUserInfo().catch((e) => {
          console.warn("error 后刷新余额失败:", e);
        });
      }
    );
    const unlistenBalance = await listen<OfficialTranslateBalance>(
      "official-translate-balance",
      (event) => {
        // 余额同步：更新 authStore 中的 user 余额
        const { token_balance, bonus_balance } = event.payload;
        const authStore = useAuthStore.getState();
        if (authStore.user) {
          useAuthStore.setState({
            user: {
              ...authStore.user,
              token_balance: token_balance ?? authStore.user.token_balance,
              bonus_balance: bonus_balance ?? authStore.user.bonus_balance,
            },
          });
        }
      }
    );
    const unlistenDone = await listen("official-translate-done", () => {
      set({ status: "completed" });
    });

    set({
      _unlisteners: [unlistenProgress, unlistenPartial, unlistenError, unlistenBalance, unlistenDone],
    });

    try {
      const params: OfficialTranslateParams = { file, model };
      const result = await api.translateOfficial(params);
      set({ result, status: "completed" });

      // 余额同步：result 中的 token_balance/bonus_balance 更新 authStore
      if (result.token_balance !== null || result.bonus_balance !== null) {
        const authStore = useAuthStore.getState();
        if (authStore.user) {
          useAuthStore.setState({
            user: {
              ...authStore.user,
              token_balance: result.token_balance ?? authStore.user.token_balance,
              bonus_balance: result.bonus_balance ?? authStore.user.bonus_balance,
            },
          });
        }
      }
    } catch (e: any) {
      const errorMsg = formatIpcError(e);
      set({ status: "error", error: errorMsg });
      toast.error(errorMsg);
    } finally {
      // 清理 listeners
      get()._unlisteners.forEach((unlisten) => unlisten());
      set({ _unlisteners: [] });
    }
  },

  cancel: async () => {
    try {
      const result = await api.cancelTranslateOfficial();
      // 余额同步
      if (result.token_balance !== null && result.token_balance !== undefined) {
        const authStore = useAuthStore.getState();
        if (authStore.user) {
          useAuthStore.setState({
            user: {
              ...authStore.user,
              token_balance: result.token_balance,
            },
          });
        }
      }
      // 提示用户取消结果
      if (result.completed_lines !== undefined && result.total_lines !== undefined && result.total_lines > 0) {
        toast.info(i18n.t("translate.officialCancelled", { completed: result.completed_lines, total: result.total_lines }) + (result.refunded ? i18n.t("translate.officialRefunded", { count: result.refunded }) : ""));
      }
    } catch (e: any) {
      console.warn("取消官方翻译失败:", e);
      // 即使后端取消失败，前端也重置状态
    } finally {
      // 清理 listeners + 重置状态
      get()._unlisteners.forEach((unlisten) => unlisten());
      set({
        status: "idle",
        current: 0,
        total: 0,
        phase: "",
        step: "",
        result: null,
        error: null,
        partialEntries: [],
        _unlisteners: [],
      });
    }
  },

  reset: () => {
    get()._unlisteners.forEach((unlisten) => unlisten());
    set({
      status: "idle",
      current: 0,
      total: 0,
      phase: "",
      step: "",
      result: null,
      error: null,
      partialEntries: [],
      _unlisteners: [],
    });
  },

  cleanup: () => {
    get()._unlisteners.forEach((unlisten) => unlisten());
    set({ _unlisteners: [] });
  },
}));
