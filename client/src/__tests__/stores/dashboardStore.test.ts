// Dashboard store 单元测试（FP-D5）
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useDashboardStore } from "../../stores/dashboardStore";

// mock api
vi.mock("../../lib/api", () => ({
  api: {
    listTranslateJobs: vi.fn(),
    pauseTranslateJob: vi.fn(),
    resumeTranslateJob: vi.fn(),
    deleteTranslateJob: vi.fn(),
  },
  formatIpcError: vi.fn((err: any) => (err && typeof err === "object" && "message" in err ? err.message : null) || String(err)),
}));

// mock toast
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

// mock i18n
vi.mock("../../lib/i18n", () => ({
  default: {
    t: vi.fn((key: string) => key),
  },
}));

import { api } from "../../lib/api";
import { toast } from "sonner";

const mockJob = {
  job_id: "uuid-1",
  filename: "test.srt",
  status: "running",
  source_language: "en",
  target_language: "zh",
  total_entries: 100,
  completed_entries: 30,
  estimated_points: 50,
  consumed_points: 15,
  frozen_points: 35,
  created_at: "2024-01-01T00:00:00Z",
};

// === SECTION 1 END ===

describe("dashboardStore", () => {
  beforeEach(() => {
    useDashboardStore.setState({
      jobs: [],
      total: 0,
      page: 1,
      pageSize: 50,
      statusFilter: null,
      loading: false,
      actingJobId: null,
    });
    vi.clearAllMocks();
  });

  // === SECTION 2 END ===

  describe("loadJobs", () => {
    it("成功加载任务列表", async () => {
      (api.listTranslateJobs as any).mockResolvedValue({
        jobs: [mockJob],
        total: 1,
        page: 1,
        page_size: 50,
      });

      await useDashboardStore.getState().loadJobs();

      const state = useDashboardStore.getState();
      expect(state.jobs).toHaveLength(1);
      expect(state.jobs[0].job_id).toBe("uuid-1");
      expect(state.total).toBe(1);
      expect(state.loading).toBe(false);
      expect(api.listTranslateJobs).toHaveBeenCalledWith({
        status: undefined,
        page: 1,
        pageSize: 50,
      });
    });

    it("加载失败时设置 loading=false 并 toast.error", async () => {
      (api.listTranslateJobs as any).mockRejectedValue({ message: "network error" });

      await useDashboardStore.getState().loadJobs();

      const state = useDashboardStore.getState();
      expect(state.loading).toBe(false);
      expect(state.jobs).toHaveLength(0);
      expect(toast.error).toHaveBeenCalled();
    });
  });

  // === SECTION 3 END ===

  describe("setStatusFilter", () => {
    it("设置筛选并重置 page=1 并触发 reload", async () => {
      (api.listTranslateJobs as any).mockResolvedValue({
        jobs: [],
        total: 0,
        page: 1,
        page_size: 50,
      });
      useDashboardStore.setState({ page: 3 });

      useDashboardStore.getState().setStatusFilter("running");

      const state = useDashboardStore.getState();
      expect(state.statusFilter).toBe("running");
      expect(state.page).toBe(1);
      expect(api.listTranslateJobs).toHaveBeenCalledWith({
        status: "running",
        page: 1,
        pageSize: 50,
      });
    });
  });

  // === SECTION 4 END ===

  describe("pauseJob", () => {
    it("成功暂停后 toast.success 并 reload", async () => {
      (api.pauseTranslateJob as any).mockResolvedValue({ status: "paused" });
      (api.listTranslateJobs as any).mockResolvedValue({
        jobs: [{ ...mockJob, status: "paused" }],
        total: 1,
        page: 1,
        page_size: 50,
      });

      await useDashboardStore.getState().pauseJob("uuid-1");

      expect(toast.success).toHaveBeenCalledWith("dashboard.pauseSuccess");
      const state = useDashboardStore.getState();
      expect(state.actingJobId).toBeNull();
      expect(state.jobs[0].status).toBe("paused");
    });

    it("失败时 toast.error 并清 actingJobId", async () => {
      (api.pauseTranslateJob as any).mockRejectedValue({ message: "error" });

      await useDashboardStore.getState().pauseJob("uuid-1");

      expect(toast.error).toHaveBeenCalled();
      expect(useDashboardStore.getState().actingJobId).toBeNull();
    });
  });

  // === SECTION 5 END ===

  describe("resumeJob", () => {
    it("成功恢复后 toast.success 并 reload", async () => {
      (api.resumeTranslateJob as any).mockResolvedValue({ status: "pending" });
      (api.listTranslateJobs as any).mockResolvedValue({
        jobs: [{ ...mockJob, status: "pending" }],
        total: 1,
        page: 1,
        page_size: 50,
      });

      await useDashboardStore.getState().resumeJob("uuid-1");

      expect(toast.success).toHaveBeenCalledWith("dashboard.resumeSuccess");
      expect(useDashboardStore.getState().actingJobId).toBeNull();
    });

    it("失败时 toast.error", async () => {
      (api.resumeTranslateJob as any).mockRejectedValue({ message: "error" });

      await useDashboardStore.getState().resumeJob("uuid-1");

      expect(toast.error).toHaveBeenCalled();
      expect(useDashboardStore.getState().actingJobId).toBeNull();
    });
  });

  // === SECTION 6 END ===

  describe("deleteJob", () => {
    it("成功删除后 toast.success 并 reload", async () => {
      (api.deleteTranslateJob as any).mockResolvedValue({ code: 0 });
      (api.listTranslateJobs as any).mockResolvedValue({
        jobs: [],
        total: 0,
        page: 1,
        page_size: 50,
      });

      await useDashboardStore.getState().deleteJob("uuid-1");

      expect(toast.success).toHaveBeenCalledWith("dashboard.deleteSuccess");
      expect(useDashboardStore.getState().actingJobId).toBeNull();
    });

    it("失败时 toast.error", async () => {
      (api.deleteTranslateJob as any).mockRejectedValue({ message: "error" });

      await useDashboardStore.getState().deleteJob("uuid-1");

      expect(toast.error).toHaveBeenCalled();
      expect(useDashboardStore.getState().actingJobId).toBeNull();
    });
  });

  // === SECTION 7 END ===
});
