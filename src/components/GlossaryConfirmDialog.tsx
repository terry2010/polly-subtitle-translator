import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Check, X, Plus, Trash2, ChevronDown, Download, Copy } from "lucide-react";
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
}

export function GlossaryConfirmDialog({
  glossary,
  onGlossaryChange,
  onConfirm,
  onCancel,
}: GlossaryConfirmDialogProps) {
  const { t } = useTranslation();
  const [localGlossary, setLocalGlossary] = useState<GlossaryEntry[]>(glossary);

  useEffect(() => {
    setLocalGlossary(glossary);
  }, [glossary]);

  // libmpv 子窗口是原生 OS 窗口，z-order 高于 WebView2，会遮盖 Dialog。
  // 弹层打开时隐藏播放器子窗口，关闭时恢复。
  useEffect(() => {
    api.devLog("[GlossaryConfirmDialog] 调用 playerHide");
    api.playerHide().catch(() => { /* 播放器未初始化，忽略 */ });
    return () => {
      api.devLog("[GlossaryConfirmDialog] cleanup 调用 playerShow");
      api.playerShow().catch(() => { /* 播放器未初始化，忽略 */ });
    };
  }, []);

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
              <ComboSelect
                className="w-[40%]"
                value={entry.chinese}
                options={Array.from(new Set([entry.chinese, ...entry.alternatives])).filter(Boolean)}
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
              {t("translate.glossaryCancel", "取消翻译")}
            </Button>
            <Button onClick={handleConfirm}>
              <Check className="h-4 w-4 mr-1" />
              {t("translate.glossaryConfirm", "确认并翻译")}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

/// 复合下拉框：输入框 + 下拉候选列表组合（多选模式）
/// 候选列表用复选框，选中的译名用 / 分隔显示在输入框中
/// 用户也可以直接在输入框中编辑（手动输入的值不受复选框控制）
function ComboSelect({
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
  const ref = useRef<HTMLDivElement>(null);

  // 点击外部关闭下拉
  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [open]);

  // 无候选时用普通 Input
  if (!options || options.length === 0) {
    return (
      <Input
        className={className}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
      />
    );
  }

  // 当前选中的译名列表（按 / 分隔）
  const selected = value.split("/").map((s) => s.trim()).filter(Boolean);

  // 切换某个候选的选中状态
  const toggleOption = (opt: string) => {
    if (selected.includes(opt)) {
      // 取消选中
      const next = selected.filter((s) => s !== opt);
      onChange(next.join("/"));
    } else {
      // 选中（追加）
      const next = [...selected, opt];
      onChange(next.join("/"));
    }
  };

  return (
    <div ref={ref} className={`relative ${className ?? ""}`}>
      <div className="flex">
        <Input
          className="rounded-r-none flex-1"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
        />
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="rounded-l-none border-l-0 h-9 px-2"
          onClick={() => setOpen(!open)}
        >
          <ChevronDown className="h-4 w-4" />
        </Button>
      </div>
      {open && (
        <div className="absolute z-50 top-full left-0 right-0 mt-1 max-h-48 overflow-y-auto rounded-md border bg-popover shadow-md">
          {options.map((opt, i) => {
            const checked = selected.includes(opt);
            return (
              <button
                key={i}
                type="button"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-sm hover:bg-accent text-left"
                onClick={() => toggleOption(opt)}
              >
                <span className={`flex h-4 w-4 items-center justify-center rounded border ${checked ? "bg-primary border-primary" : "border-input"}`}>
                  {checked && <Check className="h-3 w-3 text-primary-foreground" />}
                </span>
                {opt}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
