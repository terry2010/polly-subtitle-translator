// Dashboard 任务列表 store（FP-D5）
// 管理官方翻译任务列表的加载、暂停、恢复、删除
import { create } from "zustand";
import { toast } from "sonner";
import { api, formatIpcError } from "../lib/api";
import i18n from "../lib/i18n";
import type { TranslateJobItem } from "../lib/ipc-types";

interface DashboardState {
  /// 任务列表
  jobs: TranslateJobItem[];
  /// 总数
  total: number;
  /// 当前页
  page: number;
  /// 每页条数
  pageSize: number;
  /// 状态筛选
  statusFilter: string | null;
  /// 加载中
  loading: boolean;
  /// 操作中的 job_id（防止重复操作）
  actingJobId: string | null;

  /// 加载任务列表
  loadJobs: () => Promise<void>;
  /// 设置状态筛选
  setStatusFilter: (status: string | null) => void;
  /// 暂停任务
  pauseJob: (jobId: string) => Promise<void>;
  /// 恢复任务
  resumeJob: (jobId: string) => Promise<void>;
  /// 删除任务
  deleteJob: (jobId: string) => Promise<void>;
}

export const useDashboardStore = create<DashboardState>((set, get) => ({
  jobs: [],
  total: 0,
  page: 1,
  pageSize: 50,
  statusFilter: null,
  loading: false,
  actingJobId: null,

  loadJobs: async () => {
    const { page, pageSize, statusFilter } = get();
    set({ loading: true });
    try {
      const result = await api.listTranslateJobs({
        status: statusFilter ?? undefined,
        page,
        pageSize,
      });
      set({ jobs: result.jobs, total: result.total, loading: false });
    } catch (e: any) {
      set({ loading: false });
      toast.error(i18n.t("dashboard.loadFailed") + ": " + formatIpcError(e));
    }
  },

  setStatusFilter: (status) => {
    set({ statusFilter: status, page: 1 });
    get().loadJobs();
  },

  pauseJob: async (jobId) => {
    set({ actingJobId: jobId });
    try {
      await api.pauseTranslateJob(jobId);
      toast.success(i18n.t("dashboard.pauseSuccess"));
      await get().loadJobs();
    } catch (e: any) {
      toast.error(i18n.t("dashboard.pauseFailed") + ": " + formatIpcError(e));
    } finally {
      set({ actingJobId: null });
    }
  },

  resumeJob: async (jobId) => {
    set({ actingJobId: jobId });
    try {
      await api.resumeTranslateJob(jobId);
      toast.success(i18n.t("dashboard.resumeSuccess"));
      await get().loadJobs();
    } catch (e: any) {
      toast.error(i18n.t("dashboard.resumeFailed") + ": " + formatIpcError(e));
    } finally {
      set({ actingJobId: null });
    }
  },

  deleteJob: async (jobId) => {
    set({ actingJobId: jobId });
    try {
      await api.deleteTranslateJob(jobId);
      toast.success(i18n.t("dashboard.deleteSuccess"));
      await get().loadJobs();
    } catch (e: any) {
      toast.error(i18n.t("dashboard.deleteFailed") + ": " + formatIpcError(e));
    } finally {
      set({ actingJobId: null });
    }
  },
}));
