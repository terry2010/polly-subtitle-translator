// 恢复原文对话框（共享组件）
// SubtitlePreviewPanel 和 SubtitleListPanel 都用这个组件显示恢复原文确认弹窗
import { useTranslation } from "react-i18next";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "./ui/dialog";
import { Button } from "./ui/button";

interface RestoreOriginalDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  originalText: string;
  modifiedText: string;
  onRestore: () => void;
}

export function RestoreOriginalDialog({ open, onOpenChange, originalText, modifiedText, onRestore }: RestoreOriginalDialogProps) {
  const { t } = useTranslation();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("subtitle.restoreTitle", "恢复原文")}</DialogTitle>
          <DialogDescription>
            {t("subtitle.editOriginalHint", "点击编辑原文（编辑后需重新翻译）")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 py-2">
          <div>
            <p className="text-xs text-muted-foreground mb-1">{t("subtitle.originalText", "原始文本")}</p>
            <p className="text-sm bg-muted/30 rounded px-2 py-1.5 max-h-24 overflow-auto whitespace-pre-wrap">{originalText}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground mb-1">{t("subtitle.modifiedText", "修改后文本")}</p>
            <p className="text-sm bg-muted/30 rounded px-2 py-1.5 max-h-24 overflow-auto whitespace-pre-wrap">{modifiedText}</p>
          </div>
        </div>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel", "取消")}
          </Button>
          <Button onClick={() => { onRestore(); onOpenChange(false); }}>
            {t("subtitle.restore", "恢复")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
