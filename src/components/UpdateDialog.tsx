// 应用更新对话框
// 发现新版本时弹出，显示版本信息 + 下载进度 + 安装重启
import { useEffect, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { Loader2, Download, AlertCircle, CheckCircle2, RefreshCw } from "lucide-react";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "./ui/dialog";
import { Button } from "./ui/button";
import { api, formatIpcError } from "../lib/api";

interface UpdateDialogProps {
  open: boolean;
  version: string;
  notes: string;
  onClose: () => void;
}

type Stage = "prompt" | "downloading" | "done" | "failed";

/// 格式化剩余时间
function formatEta(secs: number): string {
  if (secs <= 0) return "--";
  if (secs < 60) return `${Math.ceil(secs)}秒`;
  const m = Math.floor(secs / 60);
  const s = Math.ceil(secs % 60);
  return `${m}分${s}秒`;
}

export function UpdateDialog({ open, version, notes, onClose }: UpdateDialogProps) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<Stage>("prompt");
  const [progress, setProgress] = useState(0);
  const [message, setMessage] = useState("");
  const [speedMbps, setSpeedMbps] = useState(0);
  const [etaSecs, setEtaSecs] = useState(0);
  const [error, setError] = useState("");
  const installingRef = useRef(false);

  // 监听下载进度事件
  useEffect(() => {
    if (!open) return;
    const unlisten = listen<{
      stage: string; progress: number; message?: string;
      speed_mbps?: number; eta_secs?: number;
    }>("update_download_progress", (event) => {
      const { stage: s, progress: p, message: m, speed_mbps, eta_secs } = event.payload;
      setProgress(p);
      if (m) setMessage(m);
      if (speed_mbps != null) setSpeedMbps(speed_mbps);
      if (eta_secs != null) setEtaSecs(eta_secs);
      if (s === "downloading") setStage("downloading");
      else if (s === "done") setStage("done");
      else if (s === "failed") { setStage("failed"); if (m) setError(m); }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [open]);

  const handleInstall = async () => {
    if (installingRef.current) return;
    installingRef.current = true;
    setStage("downloading");
    setProgress(0);
    setError("");
    try {
      await api.downloadAndInstallUpdate();
      // 下载安装成功后，stage 会被设为 done
    } catch (e: any) {
      setStage("failed");
      setError(formatIpcError(e));
      installingRef.current = false;
    }
  };

  const handleRelaunch = async () => {
    await relaunch();
  };

  const handleRetry = () => {
    installingRef.current = false;
    handleInstall();
  };

  const isBusy = stage === "downloading";

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v && !isBusy) onClose(); }}>
      <DialogContent className="max-w-md" onEscapeKeyDown={(e) => { if (isBusy) e.preventDefault(); }}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {stage === "done" ? <CheckCircle2 className="h-5 w-5 text-green-500" /> :
             stage === "failed" ? <AlertCircle className="h-5 w-5 text-red-500" /> :
             <Download className="h-5 w-5" />}
            {t("update.title")}
          </DialogTitle>
          {stage === "prompt" && (
            <DialogDescription asChild>
              <div className="text-left pt-2 space-y-2">
                <p>{t("update.newVersionAvailable", { version })}</p>
                {notes && (
                  <div className="mt-2 p-3 bg-muted rounded-md text-xs max-h-40 overflow-auto whitespace-pre-wrap">
                    {notes}
                  </div>
                )}
              </div>
            </DialogDescription>
          )}
        </DialogHeader>

        {/* 下载进度 */}
        {stage === "downloading" && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>{t("update.downloading")}</span>
            </div>
            {progress >= 0 && (
              <>
                <div className="w-full h-2 bg-muted rounded-full overflow-hidden">
                  <div className="h-full bg-primary rounded-full transition-all duration-300" style={{ width: `${Math.min(100, progress)}%` }} />
                </div>
                <div className="flex justify-between text-xs text-muted-foreground tabular-nums">
                  <span>{t("update.downloading")}</span>
                  {speedMbps > 0 && <span>{speedMbps.toFixed(1)} MB/s · {formatEta(etaSecs)}</span>}
                </div>
              </>
            )}
          </div>
        )}

        {/* 完成 */}
        {stage === "done" && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-sm text-green-600">
              <CheckCircle2 className="h-4 w-4" />
              <span>{t("update.installed")}</span>
            </div>
            <p className="text-xs text-muted-foreground">{t("update.relaunchHint")}</p>
          </div>
        )}

        {/* 失败 */}
        {stage === "failed" && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-sm text-red-600">
              <AlertCircle className="h-4 w-4" />
              <span>{t("update.failed")}</span>
            </div>
            {error && <p className="text-xs text-muted-foreground break-all">{error}</p>}
          </div>
        )}

        {/* 按钮 */}
        <div className="flex justify-end gap-2 pt-2">
          {stage === "prompt" && (
            <>
              <Button variant="outline" onClick={onClose}>{t("update.later")}</Button>
              <Button onClick={handleInstall}>
                <Download className="h-4 w-4 mr-1" />
                {t("update.installNow")}
              </Button>
            </>
          )}
          {stage === "downloading" && (
            <Button disabled>
              <Loader2 className="h-4 w-4 mr-1 animate-spin" />
              {t("update.downloading")}
            </Button>
          )}
          {stage === "done" && (
            <Button onClick={handleRelaunch}>
              <RefreshCw className="h-4 w-4 mr-1" />
              {t("update.relaunch")}
            </Button>
          )}
          {stage === "failed" && (
            <>
              <Button variant="outline" onClick={onClose}>{t("update.later")}</Button>
              <Button onClick={handleRetry}>{t("update.retry")}</Button>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
