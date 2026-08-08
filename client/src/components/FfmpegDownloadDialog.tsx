// FFmpeg 下载对话框
// 当检测到系统未安装 FFmpeg 时弹出，引导用户下载
// 下载完成后自动关闭并继续后续操作
import { useEffect, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Loader2, Download, AlertCircle, CheckCircle2 } from "lucide-react";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "./ui/dialog";
import { Button } from "./ui/button";
import { api, formatIpcError } from "../lib/api";

interface FfmpegDownloadDialogProps {
  open: boolean;
  onDownloaded: () => void; // 下载成功回调
  onCancel: () => void;
}

type Stage = "idle" | "downloading" | "extracting" | "done" | "failed";

/// 格式化剩余时间：秒 → "xx分yy秒" / "yy秒"
function formatEta(secs: number): string {
  if (secs <= 0) return "--";
  if (secs < 60) return `${Math.ceil(secs)}秒`;
  const m = Math.floor(secs / 60);
  const s = Math.ceil(secs % 60);
  return `${m}分${s}秒`;
}

export function FfmpegDownloadDialog({ open, onDownloaded, onCancel }: FfmpegDownloadDialogProps) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<Stage>("idle");
  const [progress, setProgress] = useState(0);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [speedMbps, setSpeedMbps] = useState(0);
  const [etaSecs, setEtaSecs] = useState(0);
  const downloadingRef = useRef(false);

  // 监听下载进度事件
  useEffect(() => {
    if (!open) return;
    const unlisten = listen<{
      stage: string; progress: number; message?: string; code?: string;
      speed_mbps?: number; eta_secs?: number;
    }>("ffmpeg_download_progress", (event) => {
      const { stage: s, progress: p, message: m, speed_mbps, eta_secs } = event.payload;
      setProgress(p);
      if (m) setMessage(m);
      if (speed_mbps != null) setSpeedMbps(speed_mbps);
      if (eta_secs != null) setEtaSecs(eta_secs);
      if (s === "downloading") setStage("downloading");
      else if (s === "extracting") setStage("extracting");
      else if (s === "done") setStage("done");
      else if (s === "failed") setStage("failed");
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [open]);

  // 下载完成后自动回调
  useEffect(() => {
    if (stage === "done") {
      const timer = setTimeout(() => onDownloaded(), 800);
      return () => clearTimeout(timer);
    }
  }, [stage, onDownloaded]);

  const handleDownload = async () => {
    if (downloadingRef.current) return;
    downloadingRef.current = true;
    setStage("downloading");
    setProgress(0);
    setError("");
    try {
      await api.downloadFfmpeg();
    } catch (e: any) {
      setStage("failed");
      setError(formatIpcError(e));
      downloadingRef.current = false;
    }
  };

  const handleRetry = () => {
    downloadingRef.current = false;
    handleDownload();
  };

  const isBusy = stage === "downloading" || stage === "extracting";

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v && !isBusy) onCancel(); }}>
      <DialogContent className="max-w-md" onEscapeKeyDown={(e) => { if (isBusy) e.preventDefault(); }}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {stage === "done" ? <CheckCircle2 className="h-5 w-5 text-green-500" /> :
             stage === "failed" ? <AlertCircle className="h-5 w-5 text-red-500" /> :
             <Download className="h-5 w-5" />}
            {t("subtitle.ffmpegRequired.title")}
          </DialogTitle>
          <DialogDescription className="text-left pt-2">
            {t("subtitle.ffmpegRequired.message")}
          </DialogDescription>
        </DialogHeader>

        {/* 进度区域 */}
        {(stage === "downloading" || stage === "extracting") && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>{stage === "downloading" ? t("subtitle.ffmpegRequired.downloading") : t("subtitle.ffmpegRequired.extracting")}</span>
            </div>
            {progress >= 0 && (
              <>
                <div className="w-full h-2 bg-muted rounded-full overflow-hidden">
                  <div
                    className="h-full bg-primary rounded-full transition-all duration-300"
                    style={{ width: `${stage === "extracting" ? 100 : Math.min(100, progress)}%` }}
                  />
                </div>
                <div className="flex justify-between text-xs text-muted-foreground tabular-nums">
                  <span>{stage === "downloading" ? t("subtitle.ffmpegRequired.downloading") : t("subtitle.ffmpegRequired.extracting")}</span>
                  {stage === "downloading" && speedMbps > 0 && (
                    <span>{speedMbps.toFixed(1)} MB/s · {formatEta(etaSecs)}</span>
                  )}
                </div>
              </>
            )}
            {stage === "extracting" && progress < 0 && (
              <p className="text-xs text-muted-foreground">{t("subtitle.ffmpegRequired.extracting")}</p>
            )}
          </div>
        )}

        {/* 完成状态 */}
        {stage === "done" && (
          <div className="flex items-center gap-2 text-sm text-green-600">
            <CheckCircle2 className="h-4 w-4" />
            <span>{t("subtitle.ffmpegRequired.done")}</span>
          </div>
        )}

        {/* 失败状态 */}
        {stage === "failed" && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-sm text-red-600">
              <AlertCircle className="h-4 w-4" />
              <span>{t("subtitle.ffmpegRequired.failed")}</span>
            </div>
            {error && <p className="text-xs text-muted-foreground break-all">{error}</p>}
          </div>
        )}

        {/* 按钮区域 */}
        <div className="flex justify-end gap-2 pt-2">
          {stage === "idle" && (
            <>
              <Button variant="outline" onClick={onCancel}>{t("common.cancel", "取消")}</Button>
              <Button onClick={handleDownload}>
                <Download className="h-4 w-4 mr-1" />
                {t("common.download", "下载")}
              </Button>
            </>
          )}
          {stage === "failed" && (
            <>
              <Button variant="outline" onClick={onCancel}>{t("common.cancel", "取消")}</Button>
              <Button onClick={handleRetry}>
                {t("subtitle.ffmpegRequired.retry")}
              </Button>
            </>
          )}
          {isBusy && (
            <Button disabled>
              <Loader2 className="h-4 w-4 mr-1 animate-spin" />
              请稍候...
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
