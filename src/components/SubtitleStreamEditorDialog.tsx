// 字幕流编辑弹层：拖拽排序、删除、改名
// 触发：MainView 字幕流标题旁的"编辑字幕流"按钮
// 保存时弹出不可撤销确认提示

import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { GripVertical, Trash2, Loader2, Download } from "lucide-react";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { api } from "../lib/api";
import { save } from "@tauri-apps/plugin-dialog";
import { withPlayerHidden } from "../lib/utils";
import type { SubtitleStream, SubtitleStreamEdit } from "../lib/ipc-types";

interface SubtitleStreamEditorDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  videoPath: string;
  streams: SubtitleStream[];
  onSaved: () => void; // 保存成功后回调（重新 probe 视频）
}

// 编辑项：原始流信息 + 可编辑的 title/language
interface EditItem {
  originalIndex: number;
  title: string;
  language: string;
  isGraphic: boolean;
}

// === SECTION 1 END ===

// === 可排序行项 ===

function SortableRow({
  item,
  index,
  exporting,
  onTitleChange,
  onLanguageChange,
  onRemove,
  onExport,
}: {
  item: EditItem;
  index: number;
  exporting: boolean;
  onTitleChange: (v: string) => void;
  onLanguageChange: (v: string) => void;
  onRemove: () => void;
  onExport: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: item.originalIndex });
  const { t } = useTranslation();

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className="flex items-center gap-2 rounded-md border bg-background p-2"
    >
      {/* 拖拽手柄 */}
      <button
        className="flex-shrink-0 cursor-grab active:cursor-grabbing text-muted-foreground hover:text-foreground"
        {...attributes}
        {...listeners}
      >
        <GripVertical className="h-4 w-4" />
      </button>

      {/* 序号 */}
      <span className="flex-shrink-0 w-6 text-xs text-muted-foreground text-center">{index + 1}</span>

      {/* 原始流索引 */}
      <span className="flex-shrink-0 w-12 text-xs font-mono text-muted-foreground">#{item.originalIndex}</span>

      {/* 标题输入 */}
      <Input
        value={item.title}
        onChange={(e) => onTitleChange(e.target.value)}
        placeholder={t("subtitle.streamTitlePlaceholder", "字幕标题")}
        className="h-7 flex-1 text-xs"
        disabled={item.isGraphic}
      />

      {/* 语言输入 */}
      <Input
        value={item.language}
        onChange={(e) => onLanguageChange(e.target.value)}
        placeholder="lang"
        className="h-7 w-20 text-xs"
        disabled={item.isGraphic}
      />

      {/* 导出按钮 */}
      <button
        className="flex-shrink-0 text-muted-foreground hover:text-primary transition-colors disabled:opacity-50"
        onClick={onExport}
        disabled={exporting || item.isGraphic}
        aria-label={t("common.export", "导出")}
        title={item.isGraphic ? t("subtitle.graphicSubtitleNoExport", "图形字幕不支持导出") : t("common.export", "导出")}
      >
        {exporting ? <Loader2 className="h-4 w-4 animate-spin" /> : <Download className="h-4 w-4" />}
      </button>

      {/* 删除按钮 */}
      <button
        className="flex-shrink-0 text-muted-foreground hover:text-destructive transition-colors"
        onClick={onRemove}
        aria-label={t("common.delete", "删除")}
      >
        <Trash2 className="h-4 w-4" />
      </button>
    </div>
  );
}

// === SECTION 2 END ===

// === 主组件 ===

export function SubtitleStreamEditorDialog({
  open,
  onOpenChange,
  videoPath,
  streams,
  onSaved,
}: SubtitleStreamEditorDialogProps) {
  const { t } = useTranslation();
  // 从原始 streams 初始化编辑列表
  const [items, setItems] = useState<EditItem[]>([]);
  const [confirming, setConfirming] = useState(false);
  const [saving, setSaving] = useState(false);
  const [exportingIndex, setExportingIndex] = useState<number | null>(null);

  // libmpv 子窗口是原生 OS 窗口，z-order 高于 WebView2，会遮盖 Dialog。
  // 弹层打开时隐藏播放器子窗口，关闭时恢复。
  useEffect(() => {
    if (!open) return;
    api.playerHide().catch(() => { /* 播放器未初始化，忽略 */ });
    return () => {
      api.playerShow().catch(() => { /* 播放器未初始化，忽略 */ });
    };
  }, [open]);

  // 弹层打开时初始化 items
  const [prevOpen, setPrevOpen] = useState(false);
  if (open && !prevOpen) {
    setPrevOpen(true);
    setItems(streams.map((s) => ({
      originalIndex: s.index,
      title: s.title ?? "",
      language: s.language ?? "",
      isGraphic: s.is_graphic,
    })));
    setConfirming(false);
  }
  if (!open && prevOpen) {
    setPrevOpen(false);
  }

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const handleDragEnd = useCallback((event: DragEndEvent) => {
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    setItems((prev) => {
      const oldIndex = prev.findIndex((i) => i.originalIndex === active.id);
      const newIndex = prev.findIndex((i) => i.originalIndex === over.id);
      if (oldIndex < 0 || newIndex < 0) return prev;
      return arrayMove(prev, oldIndex, newIndex);
    });
  }, []);

  const handleRemove = useCallback((id: number) => {
    setItems((prev) => prev.filter((i) => i.originalIndex !== id));
  }, []);

  // 导出单条字幕流：从原视频中提取该流为独立字幕文件
  const handleExportStream = useCallback(async (item: EditItem) => {
    if (item.isGraphic) {
      toast.error(t("subtitle.graphicSubtitleNoExport"));
      return;
    }
    setExportingIndex(item.originalIndex);
    try {
      // 默认文件名：视频名.流标题.srt
      const baseName = videoPath.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
      const langSuffix = item.language.trim() || item.title.trim() || `stream${item.originalIndex}`;
      const defaultName = `${baseName}.${langSuffix}.srt`;
      const outputPath = await withPlayerHidden(() => save({
        defaultPath: defaultName,
        filters: [
          { name: "SRT", extensions: ["srt"] },
          { name: "ASS", extensions: ["ass"] },
          { name: "VTT", extensions: ["vtt"] },
        ],
      }));
      if (!outputPath) return; // 用户取消

      await api.extractSubtitle(videoPath, item.originalIndex, outputPath);
      toast.success(t("subtitle.streamExportSuccess"));
    } catch (e) {
      console.error("字幕流导出失败:", e);
      toast.error(t("subtitle.streamExportFailed"));
    } finally {
      setExportingIndex(null);
    }
  }, [videoPath, t]);

  const handleSave = useCallback(async () => {
    if (items.length === 0) {
      toast.error(t("subtitle.streamEditAtLeastOne"));
      return;
    }
    setSaving(true);
    try {
      const edits: SubtitleStreamEdit[] = items.map((i) => ({
        original_index: i.originalIndex,
        title: i.title.trim() || null,
        language: i.language.trim() || null,
      }));

      // 检测磁盘空间
      const spaceInfo = await api.checkMergeSpace(videoPath);
      let outputPath: string | null = null;
      if (!spaceInfo.enough) {
        // 空间不够，弹 save 对话框让用户选其他盘
        const baseName = videoPath.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
        const videoDir = videoPath.split(/[\\/]/).slice(0, -1).join(/[\\/]/.test(videoPath) ? "\\" : "/");
        const defaultOutput = `${videoDir}${videoDir ? "\\" : ""}${baseName}.edited.mkv`;
        outputPath = await withPlayerHidden(() => save({
          defaultPath: defaultOutput,
          filters: [{ name: "MKV", extensions: ["mkv"] }],
        }));
        if (!outputPath) {
          // 用户取消
          return;
        }
      }

      await api.editSubtitleStreams(videoPath, edits, outputPath);
      onOpenChange(false);
      onSaved();
      toast.success(t("subtitle.streamEditSuccess"));
    } catch (e) {
      console.error("字幕流编辑失败:", e);
      toast.error(t("subtitle.streamEditFailed"));
    } finally {
      setSaving(false);
      setConfirming(false);
    }
  }, [items, videoPath, onOpenChange, onSaved, t]);

  const hasChanges = (() => {
    if (items.length !== streams.length) return true;
    for (let i = 0; i < items.length; i++) {
      const orig = streams.find((s) => s.index === items[i].originalIndex);
      if (!orig) return true;
      if ((items[i].title || "") !== (orig.title ?? "")) return true;
      if ((items[i].language || "") !== (orig.language ?? "")) return true;
    }
    return false;
  })();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("subtitle.streamEditor", "编辑字幕流")}</DialogTitle>
        </DialogHeader>

        <div className="space-y-2 max-h-[60vh] overflow-y-auto pr-1">
          {items.length === 0 ? (
            <p className="text-sm text-muted-foreground text-center py-8">
              {t("subtitle.streamEditEmpty", "所有字幕流已被删除，请至少保留一条")}
            </p>
          ) : (
            <DndContext
              sensors={sensors}
              collisionDetection={closestCenter}
              onDragEnd={handleDragEnd}
            >
              <SortableContext
                items={items.map((i) => i.originalIndex)}
                strategy={verticalListSortingStrategy}
              >
                {items.map((item, idx) => (
                  <SortableRow
                    key={item.originalIndex}
                    item={item}
                    index={idx}
                    exporting={exportingIndex === item.originalIndex}
                    onTitleChange={(v) => setItems((prev) => prev.map((p) =>
                      p.originalIndex === item.originalIndex ? { ...p, title: v } : p))}
                    onLanguageChange={(v) => setItems((prev) => prev.map((p) =>
                      p.originalIndex === item.originalIndex ? { ...p, language: v } : p))}
                    onRemove={() => handleRemove(item.originalIndex)}
                    onExport={() => handleExportStream(item)}
                  />
                ))}
              </SortableContext>
            </DndContext>
          )}
        </div>

        {/* 底部按钮 */}
        <div className="flex justify-end gap-2 pt-2 border-t">
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            {t("common.cancel", "取消")}
          </Button>
          {confirming ? (
            <>
              <span className="text-xs text-destructive self-center mr-2">
                {t("subtitle.streamEditConfirmHint", "此操作不可撤销，确认要保存吗？")}
              </span>
              <Button variant="outline" size="sm" onClick={() => setConfirming(false)}>
                {t("common.no", "否")}
              </Button>
              <Button size="sm" variant="destructive" onClick={handleSave} disabled={saving}>
                {saving ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : null}
                {t("common.confirm", "确认保存")}
              </Button>
            </>
          ) : (
            <Button size="sm" onClick={() => setConfirming(true)} disabled={!hasChanges || saving}>
              {t("common.save", "保存")}
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

// === SECTION 3 END ===
