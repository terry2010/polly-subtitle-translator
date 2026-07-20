// DashboardView：迅雷风格任务列表（FP-D5）
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { RefreshCw, Pause, Play, Trash2, FileText, Loader2, ArrowLeft } from "lucide-react";
import { useDashboardStore } from "../stores/dashboardStore";
import { Button } from "../components/ui/button";
import { Progress } from "../components/ui/progress";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "../components/ui/select";
import type { TranslateJobItem } from "../lib/ipc-types";

/// 状态颜色映射
function statusColor(status: string): string {
  switch (status) {
    case "running": return "text-blue-500";
    case "pending": return "text-cyan-500";
    case "paused": return "text-yellow-500";
    case "completed": return "text-green-500";
    case "failed": return "text-red-500";
    case "cancelled": return "text-gray-500";
    default: return "text-gray-500";
  }
}

/// 状态显示文本
function statusLabel(status: string, t: (k: string) => string): string {
  switch (status) {
    case "running": return t("dashboard.statusRunning");
    case "pending": return t("dashboard.statusPending");
    case "paused": return t("dashboard.statusPaused");
    case "completed": return t("dashboard.statusCompleted");
    case "failed": return t("dashboard.statusFailed");
    case "cancelled": return t("dashboard.statusCancelled");
    default: return status;
  }
}

/// 计算进度百分比
function progressPercent(job: TranslateJobItem): number {
  if (job.total_entries === 0) return 0;
  return Math.round((job.completed_entries / job.total_entries) * 100);
}

/// 单行任务组件
function JobRow({ job, actingJobId, onPause, onResume, onDelete }: {
  job: TranslateJobItem;
  actingJobId: string | null;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  const { t } = useTranslation();
  const percent = progressPercent(job);
  const isActing = actingJobId === job.job_id;
  const canPause = job.status === "running";
  const canResume = job.status === "paused";
  const canDelete = ["completed", "failed", "cancelled", "paused"].includes(job.status);

  return (
    <div className="flex items-center gap-3 px-4 py-3 border-b border-border hover:bg-muted/50 transition-colors">
      {/* 文件图标 */}
      <FileText className="h-5 w-5 text-muted-foreground shrink-0" />

      {/* 文件名 + 进度 */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-sm font-medium truncate">{job.filename}</span>
          <span className={`text-xs font-semibold ${statusColor(job.status)}`}>
            {statusLabel(job.status, t)}
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Progress value={percent} className="h-1.5 flex-1" />
          <span className="text-xs text-muted-foreground shrink-0">
            {job.completed_entries}/{job.total_entries} ({percent}%)
          </span>
        </div>
        {job.error_message && (
          <div className="text-xs text-red-500 mt-1 truncate">{job.error_message}</div>
        )}
      </div>

      {/* 点数 */}
      <div className="text-xs text-muted-foreground shrink-0 hidden sm:block">
        {job.consumed_points > 0 ? `${job.consumed_points} pts` : "-"}
      </div>

      {/* 操作按钮 */}
      <div className="flex items-center gap-1 shrink-0">
        {isActing && <Loader2 className="h-4 w-4 animate-spin" />}
        {canPause && !isActing && (
          <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => onPause(job.job_id)} title={t("dashboard.pause")}>
            <Pause className="h-4 w-4" />
          </Button>
        )}
        {canResume && !isActing && (
          <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => onResume(job.job_id)} title={t("dashboard.resume")}>
            <Play className="h-4 w-4" />
          </Button>
        )}
        {canDelete && !isActing && (
          <Button variant="ghost" size="icon" className="h-8 w-8 text-red-500" onClick={() => onDelete(job.job_id)} title={t("dashboard.delete")}>
            <Trash2 className="h-4 w-4" />
          </Button>
        )}
      </div>
    </div>
  );
}

export default function DashboardView() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { jobs, total, loading, statusFilter, actingJobId, loadJobs, setStatusFilter, pauseJob, resumeJob, deleteJob } = useDashboardStore();

  useEffect(() => {
    loadJobs();
  }, [loadJobs]);

  const handleDelete = async (jobId: string) => {
    // 简单确认：直接删除（迅雷风格不弹确认框）
    await deleteJob(jobId);
  };

  return (
    <div className="flex flex-col h-screen bg-background">
      {/* 顶部工具栏 */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => navigate("/")} title={t("dashboard.back")}>
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h1 className="text-base font-semibold">{t("dashboard.title")}</h1>

        <div className="flex-1" />

        {/* 状态筛选 */}
        <Select
          value={statusFilter ?? "all"}
          onValueChange={(v) => setStatusFilter(v === "all" ? null : v)}
        >
          <SelectTrigger className="w-32 h-8 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("dashboard.filterAll")}</SelectItem>
            <SelectItem value="running">{t("dashboard.statusRunning")}</SelectItem>
            <SelectItem value="pending">{t("dashboard.statusPending")}</SelectItem>
            <SelectItem value="paused">{t("dashboard.statusPaused")}</SelectItem>
            <SelectItem value="completed">{t("dashboard.statusCompleted")}</SelectItem>
            <SelectItem value="failed">{t("dashboard.statusFailed")}</SelectItem>
            <SelectItem value="cancelled">{t("dashboard.statusCancelled")}</SelectItem>
          </SelectContent>
        </Select>

        {/* 刷新 */}
        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => loadJobs()} title={t("dashboard.refresh")} disabled={loading}>
          {loading ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
        </Button>
      </div>

      {/* 任务列表 */}
      <div className="flex-1 overflow-y-auto">
        {jobs.length === 0 && !loading ? (
          <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-2">
            <FileText className="h-12 w-12 opacity-30" />
            <span className="text-sm">{t("dashboard.empty")}</span>
          </div>
        ) : (
          <>
            {jobs.map((job) => (
              <JobRow
                key={job.job_id}
                job={job}
                actingJobId={actingJobId}
                onPause={pauseJob}
                onResume={resumeJob}
                onDelete={handleDelete}
              />
            ))}
            {/* 底部统计 */}
            <div className="px-4 py-2 text-xs text-muted-foreground text-center">
              {t("dashboard.totalCount", { count: total })}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
