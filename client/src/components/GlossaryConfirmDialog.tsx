import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Check, X, Plus, Trash2, Download, Copy } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog";
import { api } from "../lib/api";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { toast } from "sonner";
import type { GlossaryEntry } from "../stores/translateStore";

interface GlossaryConfirmDialogProps {
  glossary: GlossaryEntry[];
  onGlossaryChange: (g: GlossaryEntry[]) => void;
  onConfirm: () => void;
  onCancel: () => void;
  /// 自动翻译模式：翻译已在后台进行，确认按钮变为"确认"仅关闭弹窗
  autoTranslating?: boolean;
  /// 翻译已完成：确认按钮变为禁用的"字幕翻译已完成"
  translateDone?: boolean;
  /// 是否显示"提取后自动翻译" checkbox
  showAutoTranslateCheckbox?: boolean;
  /// "提取后自动翻译"的当前值
  autoTranslateAfterExtract?: boolean;
  /// "提取后自动翻译"变更回调
  onAutoTranslateChange?: (checked: boolean) => void;
}

interface GlossaryConfirmDialogProps {
  glossary: GlossaryEntry[];
  onGlossaryChange: (g: GlossaryEntry[]) => void;
  onConfirm: () => void;
  onCancel: () => void;
  /// 自动翻译模式：翻译已在后台进行，确认按钮变为"确认"仅关闭弹窗
  autoTranslating?: boolean;
  /// 翻译已完成：确认按钮变为禁用的"字幕翻译已完成"
  translateDone?: boolean;
}

export function GlossaryConfirmDialog({
  glossary,
  onGlossaryChange,
  onConfirm,
  onCancel,
  autoTranslating = false,
  translateDone = false,
  showAutoTranslateCheckbox = false,
  autoTranslateAfterExtract = false,
  onAutoTranslateChange,
}: GlossaryConfirmDialogProps) {
  const { t } = useTranslation();
  const [localGlossary, setLocalGlossary] = useState<GlossaryEntry[]>(glossary);

  useEffect(() => {
    setLocalGlossary(glossary);
  }, [glossary]);

  const handleEdit = (index: number, field: "english" | "chinese", value: string) => {
    const updated = [...localGlossary];
    updated[index] = { ...updated[index], [field]: value };
    setLocalGlossary(updated);
  };

  const handleDelete = (index: number) => {
    setLocalGlossary(localGlossary.filter((_, i) => i !== index));
  };

  const handleAdd = () => {
    setLocalGlossary([...localGlossary, { english: "", chinese: "", alternatives: [] }]);
  };

  const handleConfirm = () => {
    // 过滤掉空行
    const filtered = localGlossary.filter((g) => g.english.trim() && g.chinese.trim());
    onGlossaryChange(filtered);
    onConfirm();
  };

  const handleExport = async (format: "txt" | "csv") => {
    const filtered = localGlossary.filter((g) => g.english.trim() && g.chinese.trim());
    if (filtered.length === 0) return;

    let content: string;
    let defaultName: string;
    if (format === "csv") {
      // CSV 格式：英文名,中文译名（用逗号分隔，值用双引号包裹防注入）
      content = "\uFEFF" + filtered.map((g) => `"${g.english.replace(/"/g, '""')}","${g.chinese.replace(/"/g, '""')}"`).join("\n");
      defaultName = "glossary.csv";
    } else {
      // TXT 格式：英文名 → 中文译名
      content = filtered.map((g) => `${g.english} → ${g.chinese}`).join("\n");
      defaultName = "glossary.txt";
    }

    try {
      const outputPath = await save({
        defaultPath: defaultName,
        filters: [{ name: format.toUpperCase(), extensions: [format] }],
      });
      if (!outputPath) return;
      await writeTextFile(outputPath, content);
      toast.success(t("translate.glossaryExportSuccess", "导出成功"));
    } catch (e) {
      toast.error(t("translate.glossaryExportFailed", "导出失败"));
    }
  };

  const handleCopy = async () => {
    const filtered = localGlossary.filter((g) => g.english.trim() && g.chinese.trim());
    if (filtered.length === 0) return;
    const text = filtered.map((g) => `${g.english} → ${g.chinese}`).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t("translate.glossaryCopySuccess", "已复制到剪贴板"));
    } catch {
      toast.error(t("translate.glossaryCopyFailed", "复制失败"));
    }
  };

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onCancel(); }}>
      <DialogContent className="max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>{t("translate.glossaryTitle", "人名译名表")}</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground -mt-2">
          {t("translate.glossaryDesc", "AI 已从字幕中提取以下人名及建议译名。请检查并修改，确认后将用于所有翻译批次保证一致性。")}
        </p>
        {showAutoTranslateCheckbox && (
          <label className="flex items-center gap-2 text-xs text-muted-foreground cursor-pointer select-none py-2">
            <input
              type="checkbox"
              checked={autoTranslateAfterExtract}
              onChange={(e) => {
                const checked = e.target.checked;
                onAutoTranslateChange?.(checked);
              }}
              className="h-3.5 w-3.5 rounded border-gray-300 accent-primary flex-shrink-0"
            />
            <span>{t("translate.autoTranslateAfterExtract", "提取完毕后自动翻译字幕")}</span>
          </label>
        )}
        <div className="flex-1 overflow-y-auto space-y-2 min-h-[200px]">
          <div className="flex gap-2 text-xs font-medium text-muted-foreground px-1">
            <span className="w-[40%]">{t("translate.glossaryEnglish", "英文名")}</span>
            <span className="w-[40%]">{t("translate.glossaryChinese", "中文译名")}</span>
            <span className="w-20" />
          </div>
          {localGlossary.map((entry, index) => (
            <div key={index} className="flex gap-2 items-center">
              <Input
                className="w-[40%]"
                value={entry.english}
                onChange={(e) => handleEdit(index, "english", e.target.value)}
                placeholder="English"
              />
              <TagInput
                className="w-[40%]"
                value={entry.chinese}
                options={entry.alternatives}
                onChange={(value) => handleEdit(index, "chinese", value)}
                placeholder="中文"
              />
              <Button
                size="sm"
                variant="ghost"
                className="h-8 w-8 p-0 text-destructive"
                onClick={() => handleDelete(index)}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
          ))}
          <Button size="sm" variant="outline" onClick={handleAdd} className="w-full">
            <Plus className="h-4 w-4 mr-1" />
            {t("translate.glossaryAdd", "添加人名")}
          </Button>
        </div>
        <div className="flex justify-between gap-2 pt-2">
          <div className="flex gap-1">
            <Button size="sm" variant="outline" onClick={handleCopy}>
              <Copy className="h-4 w-4 mr-1" />
              {t("translate.glossaryCopy", "复制")}
            </Button>
            <Button size="sm" variant="outline" onClick={() => handleExport("txt")}>
              <Download className="h-4 w-4 mr-1" />
              TXT
            </Button>
            <Button size="sm" variant="outline" onClick={() => handleExport("csv")}>
              <Download className="h-4 w-4 mr-1" />
              CSV
            </Button>
          </div>
          <div className="flex gap-2">
            <Button variant="outline" onClick={onCancel}>
              <X className="h-4 w-4 mr-1" />
              {autoTranslating
                ? t("translate.glossaryCloseDialog", "关闭弹窗")
                : t("translate.glossaryCancel", "取消翻译")}
            </Button>
            <Button
              onClick={handleConfirm}
              disabled={translateDone || (autoTranslating && !translateDone)}
            >
              <Check className="h-4 w-4 mr-1" />
              {translateDone
                ? t("translate.glossaryTranslateDone", "字幕翻译已完成")
                : autoTranslating
                  ? t("translate.glossaryTranslating", "字幕翻译中")
                  : t("translate.glossaryConfirm", "确认并翻译")}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

/// 标签输入框：用于多译名词，支持单个/多个标签 + 候选下拉
/// - 输入框内显示 tag（按 / 分隔）
/// - 只有一个 tag 时也显示 tag
/// - 点击 tag 进入编辑模式
/// - tag 右侧有 X 可删除
/// - 点击输入框空白区域可输入新 tag
/// - 下拉框只显示候选译名（options）
/// - 没有候选译名（options 为空）时不显示下拉框
function TagInput({
  className,
  value,
  options,
  onChange,
  placeholder,
}: {
  className?: string;
  value: string;
  options: string[];
  onChange: (value: string) => void;
  placeholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [editValue, setEditValue] = useState("");
  const [newTagValue, setNewTagValue] = useState("");
  const containerRef = useRef<HTMLDivElement>(null);
  const newInputRef = useRef<HTMLInputElement>(null);

  const tags = value.split("/").map((s) => s.trim()).filter(Boolean);
  // 候选词去重，且不再包含已选 tag
  const candidateOptions = Array.from(new Set(options.filter((o) => !tags.includes(o.trim())))).filter(Boolean);

  // 点击外部关闭下拉
  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [open]);

  const updateTags = (newTags: string[]) => {
    onChange(newTags.join("/"));
  };

  // 点击标签进入编辑模式
  const handleTagClick = (index: number, e: React.MouseEvent) => {
    e.stopPropagation();
    setEditingIndex(index);
    setEditValue(tags[index]);
    setOpen(false);
  };

  // 完成编辑标签
  const finishEdit = () => {
    if (editingIndex === null) return;
    const newTags = [...tags];
    if (editValue.trim()) {
      newTags[editingIndex] = editValue.trim();
    } else {
      newTags.splice(editingIndex, 1);
    }
    updateTags(newTags);
    setEditingIndex(null);
    setEditValue("");
  };

  // 删除标签
  const handleDeleteTag = (index: number, e: React.MouseEvent) => {
    e.stopPropagation();
    const newTags = tags.filter((_, i) => i !== index);
    updateTags(newTags);
  };

  // 添加新标签
  const handleAddTag = () => {
    if (!newTagValue.trim()) return;
    const newTags = [...tags, newTagValue.trim()];
    updateTags(newTags);
    setNewTagValue("");
  };

  // 下拉选项切换（选中候选词）
  const selectOption = (opt: string) => {
    if (!tags.includes(opt)) {
      updateTags([...tags, opt]);
    }
  };

  return (
    <div ref={containerRef} className={`relative ${className ?? ""}`}>
      <div
        className="flex flex-wrap items-center gap-1 min-h-9 px-2 py-1 border rounded-md bg-background cursor-text focus-within:ring-1 focus-within:ring-ring"
        onClick={() => {
          newInputRef.current?.focus();
          if (candidateOptions.length > 0) {
            setOpen(true);
          }
        }}
      >
        {tags.map((tag, i) =>
          editingIndex === i ? (
            <input
              key={i}
              type="text"
              className="w-24 px-1 py-0.5 text-sm border rounded outline-none"
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onBlur={finishEdit}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  finishEdit();
                } else if (e.key === "Escape") {
                  setEditingIndex(null);
                  setEditValue("");
                }
              }}
              autoFocus
              onClick={(e) => e.stopPropagation()}
            />
          ) : (
            <span
              key={i}
              className="inline-flex items-center gap-1 px-2 py-0.5 text-sm bg-secondary rounded-md cursor-pointer hover:bg-secondary/80"
              onClick={(e) => handleTagClick(i, e)}
            >
              {tag}
              <button
                type="button"
                className="text-muted-foreground hover:text-destructive"
                onClick={(e) => handleDeleteTag(i, e)}
              >
                <X className="h-3 w-3" />
              </button>
            </span>
          )
        )}
        <input
          ref={newInputRef}
          type="text"
          className="flex-1 min-w-[60px] bg-transparent outline-none text-sm"
          value={newTagValue}
          onChange={(e) => setNewTagValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              handleAddTag();
            }
          }}
          onBlur={handleAddTag}
          placeholder={tags.length === 0 ? placeholder : ""}
          onClick={(e) => e.stopPropagation()}
        />
      </div>
      {open && candidateOptions.length > 0 && (
        <div className="absolute z-50 top-full left-0 right-0 mt-1 max-h-48 overflow-y-auto rounded-md border bg-popover shadow-md">
          {candidateOptions.map((opt, i) => (
            <button
              key={i}
              type="button"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-sm hover:bg-accent text-left"
              onClick={(e) => {
                e.stopPropagation();
                selectOption(opt);
                setOpen(false);
              }}
            >
              <span className="flex h-4 w-4 items-center justify-center rounded border border-input" />
              {opt}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
