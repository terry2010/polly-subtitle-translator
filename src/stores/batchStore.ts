// 批量翻译状态 store
import { create } from "zustand";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";
import type { BatchTask, BatchConfig, BatchStatus } from "../lib/ipc-types";
import { api, formatIpcError } from "../lib/api";
import { log, error } from "../lib/logger";

interface BatchState {
  tasks: BatchTask[];
  config: BatchConfig;
  isLoading: boolean;
  isWatching: boolean;
  isPaused: boolean;
  pauseReason: string | null;
  unlisten: (() => void) | null;

  init: () => Promise<void>;
  destroy: () => void;
  refreshStatus: () => Promise<void>;
  submitFiles: (paths: string[]) => Promise<void>;
  submitSubtitleFiles: (paths: string[]) => Promise<void>;
  cancelTask: (taskId: string) => Promise<void>;
  deleteTask: (taskId: string) => Promise<void>;
  startTask: (taskId: string) => Promise<void>;
  reorderTasks: (taskIds: string[]) => Promise<void>;
  retryTask: (taskId: string) => Promise<void>;
  clearQueue: () => Promise<void>;
  pauseQueue: () => Promise<void>;
  resumeQueue: () => Promise<void>;
  startWatch: (paths: string[], recursive: boolean) => Promise<void>;
  stopWatch: () => Promise<void>;
  scanExistingFiles: (paths?: string[], recursive?: boolean) => Promise<void>;
  cancelScan: () => Promise<void>;
  addFilesToQueue: (files: string[]) => Promise<number>;
  saveConfig: (config: BatchConfig) => Promise<void>;
  loadConfig: () => Promise<void>;
}

const defaultConfig: BatchConfig = {
  source_lang: "en",
  source_langs: ["en"],
  target_lang: "zh",
  skip_langs: ["zh"],
  provider: "",
  model: null,
  model_type: null,
  service_id: null,
  file_concurrency: 1,
  entry_concurrency: 3,
  output_mode: "Bilingual",
  output_formats: ["srt"],
  output_format: "srt",
  embed_to_video: false,
  output_suffix: ".zh",
  check_external: true,
  check_embedded: true,
  watch_paths: [],
  watch_recursive: true,
  scan_on_start: false,
  schedule: "Always",
  min_file_size_mb: 1,
  min_duration_secs: 10,
  skip_cache: false,
  debounce_secs: 3,
};

/// 获取任务状态的可读文本
export function getStatusText(status: BatchStatus): string {
  if (typeof status === "string") {
    const map: Record<string, string> = {
      Queued: "排队中",
      Probing: "探测中",
      CheckingSubtitle: "检查字幕",
      Parsing: "解析中",
      Exporting: "导出中",
      Done: "已完成",
      Cancelled: "已取消",
    };
    return map[status] ?? status;
  }
  if ("Extracting" in status) return `提取字幕 ${Math.round(status.Extracting * 100)}%`;
  if ("Translating" in status) return `翻译中 ${Math.round(status.Translating * 100)}%`;
  if ("Skipped" in status) return `跳过: ${status.Skipped}`;
  if ("Failed" in status) return `失败: ${status.Failed}`;
  return "未知";
}

/// 获取任务进度百分比（0-1）
export function getTaskProgress(status: BatchStatus): number {
  if (typeof status === "string") {
    if (status === "Done") return 1;
    if (status === "Queued" || status === "Cancelled") return 0;
    return 0;
  }
  if ("Extracting" in status) return status.Extracting;
  if ("Translating" in status) return status.Translating;
  return 0;
}

export const useBatchStore = create<BatchState>((set, get) => ({
  tasks: [],
  config: defaultConfig,
  isLoading: false,
  isWatching: false,
  isPaused: false,
  pauseReason: null,
  unlisten: null,

  init: async () => {
    const unlistens: (() => void)[] = [];

    // 监听任务状态变更
    unlistens.push(
      await listen<{ id: string; status: BatchStatus }>(
        "batch-task-status",
        (e) => {
          set((state) => ({
            tasks: state.tasks.map((t) =>
              t.id === e.payload.id ? { ...t, status: e.payload.status } : t
            ),
          }));
        }
      )
    );

    // 监听新任务添加
    unlistens.push(
      await listen<BatchTask>(
        "batch-task-added",
        (e) => {
          set((state) => {
            // 避免重复添加
            if (state.tasks.some((t) => t.id === e.payload.id)) return state;
            return { tasks: [...state.tasks, e.payload] };
          });
        }
      )
    );

    // 监听任务删除
    unlistens.push(
      await listen<{ id: string }>(
        "batch-task-deleted",
        (e) => {
          set((state) => ({
            tasks: state.tasks.filter((t) => t.id !== e.payload.id),
          }));
        }
      )
    );

    // 监听任务进度
    unlistens.push(
      await listen<{
        id: string;
        stage: string;
        progress: number;
        done?: number;
        total?: number;
      }>("batch-file-progress", (e) => {
        const { id, stage, progress, done, total } = e.payload;
        set((state) => ({
          tasks: state.tasks.map((t) => {
            if (t.id !== id) return t;
            const status: BatchStatus =
              stage === "extracting"
                ? { Extracting: progress }
                : { Translating: progress };
            return {
              ...t,
              status,
              done_entries: done ?? t.done_entries,
              total_entries: total ?? t.total_entries,
            };
          }),
        }));
      })
    );

    // 监听任务完成
    unlistens.push(
      await listen<BatchTask>("batch-file-done", (e) => {
        log("批量翻译任务完成:", e.payload);
        set((state) => ({
          tasks: state.tasks.map((t) =>
            t.id === e.payload.id ? e.payload : t
          ),
        }));
      })
    );

    // 监听任务失败
    unlistens.push(
      await listen<{ id: string; error: string }>(
        "batch-file-error",
        (e) => {
          error("批量翻译任务失败:", e.payload);
          set((state) => ({
            tasks: state.tasks.map((t) =>
              t.id === e.payload.id
                ? { ...t, status: { Failed: e.payload.error }, error: e.payload.error }
                : t
            ),
          }));
        }
      )
    );

    // 监听任务跳过
    unlistens.push(
      await listen<{ id: string; reason: string }>(
        "batch-file-skipped",
        (e) => {
          log("批量翻译任务跳过:", e.payload);
          set((state) => ({
            tasks: state.tasks.map((t) =>
              t.id === e.payload.id
                ? { ...t, status: { Skipped: e.payload.reason } }
                : t
            ),
          }));
        }
      )
    );

    // 监听队列完成
    unlistens.push(
      await listen<{ total: number; done: number; skipped: number; failed: number }>(
        "batch-queue-complete",
        (e) => {
          toast.success(
            `批量翻译完成：共 ${e.payload.total} 个，成功 ${e.payload.done}，跳过 ${e.payload.skipped}，失败 ${e.payload.failed}`
          );
        }
      )
    );

    // 监听队列暂停
    unlistens.push(
      await listen<{ reason: string }>("batch-queue-paused", (e) => {
        set({ isPaused: true, pauseReason: e.payload.reason });
        toast.error(`队列已暂停: ${e.payload.reason}`);
      })
    );

    set({ unlisten: () => unlistens.forEach((u) => u()) });
  },

  destroy: () => {
    get().unlisten?.();
    set({ unlisten: null });
  },

  refreshStatus: async () => {
    set({ isLoading: true });
    try {
      const tasks = await api.getBatchStatus();
      set({ tasks, isLoading: false });
    } catch {
      set({ isLoading: false });
    }
  },

  submitFiles: async (paths) => {
    try {
      const ids = await api.batchTranslateFiles(paths);
      toast.success(`已添加 ${ids.length} 个文件到批量翻译队列`);
      await get().refreshStatus();
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  submitSubtitleFiles: async (paths) => {
    try {
      const ids = await api.batchTranslateFiles(paths);
      toast.success(`已添加 ${ids.length} 个字幕文件到批量翻译队列`);
      await get().refreshStatus();
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  cancelTask: async (taskId) => {
    try {
      await api.cancelBatchTask(taskId);
      await get().refreshStatus();
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  deleteTask: async (taskId) => {
    try {
      await api.deleteBatchTask(taskId);
      // 乐观更新：立即从本地列表移除
      set((state) => ({ tasks: state.tasks.filter((t) => t.id !== taskId) }));
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  startTask: async (taskId) => {
    try {
      await api.startBatchTask(taskId);
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  reorderTasks: async (taskIds) => {
    // 乐观更新：立即按新顺序排列
    set((state) => {
      const map = new Map(state.tasks.map((t) => [t.id, t]));
      const reordered = taskIds.map((id) => map.get(id)).filter(Boolean) as BatchTask[];
      // 追加不在 taskIds 中的任务
      const remaining = state.tasks.filter((t) => !taskIds.includes(t.id));
      return { tasks: [...reordered, ...remaining] };
    });
    try {
      await api.reorderBatchTasks(taskIds);
    } catch (e: any) {
      toast.error(formatIpcError(e));
      // 失败时刷新恢复真实状态
      await get().refreshStatus();
    }
  },

  retryTask: async (taskId) => {
    try {
      await api.retryBatchTask(taskId);
      await get().refreshStatus();
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  clearQueue: async () => {
    try {
      await api.clearBatchQueue();
      await get().refreshStatus();
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  pauseQueue: async () => {
    try {
      await api.pauseBatchQueue();
      set({ isPaused: true });
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  resumeQueue: async () => {
    try {
      await api.resumeBatchQueue();
      set({ isPaused: false, pauseReason: null });
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  startWatch: async (paths, recursive) => {
    try {
      await api.startFolderWatch(paths, recursive, get().config);
      set({ isWatching: true });
      toast.success(`已启动文件夹监视: ${paths.length} 个目录`);
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  stopWatch: async () => {
    try {
      await api.stopFolderWatch();
      set({ isWatching: false });
      toast.success("已停止文件夹监视");
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  scanExistingFiles: async (paths, recursive) => {
    try {
      await api.scanExistingFiles(paths, recursive);
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  cancelScan: async () => {
    try {
      await api.cancelScan();
      toast.success("已取消扫描检查");
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  addFilesToQueue: async (files) => {
    try {
      const count = await api.addFilesToQueue(files);
      if (count > 0) {
        toast.success(`已添加 ${count} 个文件到队列`);
      } else {
        toast.info("所选文件已在队列中");
      }
      return count;
    } catch (e: any) {
      toast.error(formatIpcError(e));
      return 0;
    }
  },

  saveConfig: async (config) => {
    try {
      await api.saveBatchConfig(config);
      set({ config });
      toast.success("批量翻译配置已保存");
    } catch (e: any) {
      toast.error(formatIpcError(e));
    }
  },

  loadConfig: async () => {
    try {
      const config = await api.getBatchConfig();
      // 兼容旧配置：缺失的新字段补默认值
      if (!config.source_langs) config.source_langs = config.source_lang ? [config.source_lang] : ["en"];
      if (!config.skip_langs) config.skip_langs = config.target_lang ? [config.target_lang] : [];
      // 检查保存的 provider 是否已配置，未配置则清空
      if (config.provider && config.provider !== "openai") {
        const appId = await api.getConfig(`translate_${config.provider}_app_id`).catch(() => null);
        if (!appId) {
          config.provider = "";
        }
      }
      set({ config, isWatching: config.watch_paths.length > 0 });
    } catch {
      // 使用默认配置
    }
  },
}));
