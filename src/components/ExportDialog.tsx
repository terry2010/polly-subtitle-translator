// 字幕导出弹层（export-dialog-plan.md §5.1）
// 触发：SubtitlePreviewPanel 的"保存字幕"按钮 / Ctrl+S 快捷键
// 功能：格式选择 + 单语/双语 + ASS 样式配置 + 实时预览 + 默认文件名

import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { save } from "@tauri-apps/plugin-dialog";
import { ArrowUpDown, RotateCcw, Merge, Loader2 } from "lucide-react";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { api } from "../lib/api";
import { useVideoStore } from "../stores/videoStore";
import { useTranslateStore } from "../stores/translateStore";
import type { SubtitleFile, ExportOptions, AssBilingualStyle } from "../lib/ipc-types";
import { DEFAULT_ASS_STYLE } from "../lib/ipc-types";
import {
  buildExportFileName,
  buildSubtitleTitle,
  assColorToCss,
  hexToAssColor,
  stripExt,
  fileDir,
} from "../lib/utils";

interface ExportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  file: SubtitleFile; // 调用方保证非 null（见 §5.2 条件渲染）
}

type FormatKind = "srt" | "ass" | "vtt";
type ModeKind = "monolingual" | "bilingual";
type MonoLang = "source" | "translated";

// === SECTION 1 END ===

// === 实时预览子组件（§5.1.4） ===

function ExportPreview({ file, options, assStyle }: {
  file: SubtitleFile;
  options: ExportOptions;
  assStyle: AssBilingualStyle;
}) {
  // 取非删除条目，优先有译文的；按译文+原文总长度降序，挑长度够长的做预览样本
  // （短字幕看不出双语/样式效果，挑长的更有代表性）
  const activeEntries = file.entries.filter((e) => !e._deleted);
  const withTranslated = activeEntries.filter((e) => e.translated);
  const pool = withTranslated.length > 0 ? withTranslated : activeEntries;
  const samples = [...pool]
    .map((e) => ({
      entry: e,
      len: (e.translated || e.text).length + (e.text || "").length,
    }))
    .sort((a, b) => b.len - a.len)
    .slice(0, 3)
    .map((x) => x.entry);

  // ASS 样式 → CSS 样式映射
  // - ASS + 双语：用 assStyle 的 primary/secondary 参数
  // - ASS + 单语：套 §4.4 硬编码 Default 样式（字号 48/白色/描边 2/阴影 1）
  // - SRT/VTT：无样式（color 由外层 text-white 提供，避免黑字黑底）
  //
  // 预览缩放：不再按视频分辨率绝对缩放，而是把预览第一行字号缩放到 24-32px 区间，
  // 这样无论视频是 720p 还是 4K，预览都保持可读，同时保留原/译文字号比例。
  const baseFontSize =
    options.format === "ass" && options.mode === "bilingual"
      ? assStyle.primary_font_size
      : 48;
  const PREVIEW_SCALE = Math.min(0.8, Math.max(0.5, 28 / baseFontSize));
  const lineStyle = (isPrimary: boolean): React.CSSProperties => {
    if (options.format !== "ass") return {};
    if (options.mode !== "bilingual") {
      // ASS 单语 Default：字号 48/描边 2/阴影 1 → 缩放后字号 29px/描边 1.2px/阴影 0.6px
      const size = 48 * PREVIEW_SCALE;
      return {
        fontSize: `${size}px`,
        color: assColorToCss("&HFFFFFF&"),
        WebkitTextFillColor: assColorToCss("&HFFFFFF&"),
        WebkitTextStroke: `${2 * PREVIEW_SCALE}px rgba(0,0,0,0.9)`,
        // paint-order: stroke 让描边画在文字填充之下，避免描边盖住中文细笔画导致发黑看不清
        paintOrder: "stroke",
        textShadow: `1px 1px 0 rgba(0,0,0,0.7)`,
        fontWeight: "bold",
        fontFamily: 'Arial, "Helvetica Neue", Helvetica, sans-serif',
        lineHeight: 1.5,
        whiteSpace: "pre-line",
      };
    }
    const s = assStyle;
    const cssColor = assColorToCss(isPrimary ? s.primary_color : s.secondary_color);
    const rawSize = isPrimary ? s.primary_font_size : s.secondary_font_size;
    const size = rawSize * PREVIEW_SCALE;
    const outlineCss = assColorToCss(s.outline_color);
    const shadowCss = assColorToCss(s.shadow_color);
    // 描边和阴影都按 PREVIEW_SCALE 放大，保持与字号的比例
    const scaledOutline = s.outline * PREVIEW_SCALE;
    const scaledShadow = s.shadow * PREVIEW_SCALE;
    return {
      fontSize: `${size}px`,
      color: cssColor,
      // WebkitTextStroke 会覆盖文字填充色，必须显式用 WebkitTextFillColor 保留文字颜色
      WebkitTextFillColor: cssColor,
      // paint-order: stroke 让描边画在文字填充之下（与 ASS 播放器一致），
      // 否则描边画在填充之上会盖住中文细笔画，使文字发黑看不清
      paintOrder: "stroke",
      // 默认加粗模拟视频里字笔画的粗壮感（ASS 播放器渲染的笔画比浏览器 CSS 渲染粗）
      fontWeight: (isPrimary ? s.primary_bold : s.secondary_bold) ? "bold" : "500",
      fontStyle: (isPrimary ? s.primary_italic : s.secondary_italic) ? "italic" : "normal",
      textDecoration: (isPrimary ? s.primary_underline : s.secondary_underline) ? "underline" : "none",
      WebkitTextStroke: `${scaledOutline}px ${outlineCss}`,
      textShadow: scaledShadow > 0 ? `${scaledShadow}px ${scaledShadow}px 0 ${shadowCss}` : "none",
      fontFamily: 'Arial, "Helvetica Neue", Helvetica, sans-serif',
      lineHeight: 1.5,
      whiteSpace: "pre-line",
    };
  };

  // 清理预览文本：去掉 ASS 覆盖标记 {\...}、HTML 标签 <...>，并把 \N / \n 转为真实换行
  // 否则预览会显示字面 "\N" 且中英文挤在同一行，与导出/播放效果不符
  const stripAssTags = (s: string) =>
    s
      .replace(/\{[^}]*\}/g, "")
      .replace(/<[^>]*>/g, "")
      .replace(/\\[Nn]/g, "\n")
      .trim();

  if (samples.length === 0) {
    return (
      <div className="rounded-md p-4 min-h-[200px] flex items-center justify-center text-white/50 text-sm w-full"
        style={{ background: "linear-gradient(135deg, #2a2a2a 0%, #1a1a1a 100%)" }}
      >
        —
      </div>
    );
  }

  return (
    <div
      className="rounded-md p-4 min-h-[200px] flex flex-col justify-center gap-1 w-full text-white"
      style={{ background: "linear-gradient(135deg, #2a2a2a 0%, #1a1a1a 100%)" }}
    >
      {samples.map((entry, i) => (
        <div key={i} className="mb-2 w-full">
          {options.mode === "bilingual" ? (
            <>
              <div style={lineStyle(true)} className="w-full whitespace-pre-line">
                {stripAssTags(options.bilingual_translated_first ? entry.translated : entry.text)}
              </div>
              <div style={lineStyle(false)} className="w-full whitespace-pre-line">
                {stripAssTags(options.bilingual_translated_first ? entry.text : entry.translated)}
              </div>
            </>
          ) : (
            <div style={lineStyle(true)} className="w-full whitespace-pre-line">
              {stripAssTags(options.monolingual_lang === "source" ? entry.text : entry.translated)}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

// === SECTION 2 END ===

// === ASS 样式配置行（字号/颜色/B/I/U） ===

function StyleRow({
  label,
  size,
  color,
  bold,
  italic,
  underline,
  onSize,
  onColor,
  onBold,
  onItalic,
  onUnderline,
}: {
  label: string;
  size: number;
  color: string;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  onSize: (v: number) => void;
  onColor: (v: string) => void;
  onBold: (v: boolean) => void;
  onItalic: (v: boolean) => void;
  onUnderline: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  // ASS 颜色 → <input type="color"> 需要的 #RRGGBB
  const cssColor = assColorToCss(color);
  const btnCls = "h-7 w-7 text-xs border rounded";
  return (
    <div className="flex items-center gap-2">
      <span className="text-xs w-12 flex-shrink-0">{label}</span>
      <Input
        type="number"
        value={size}
        onChange={(e) => onSize(parseInt(e.target.value, 10) || 0)}
        className="h-7 w-16 text-xs"
        aria-label={t("subtitle.exportFontSize", "字号")}
      />
      <input
        type="color"
        value={cssColor}
        onChange={(e) => onColor(hexToAssColor(e.target.value))}
        className="h-7 w-7 rounded border cursor-pointer"
        aria-label={t("subtitle.exportColor", "颜色")}
      />
      <Button
        size="sm"
        variant={bold ? "default" : "outline"}
        className={btnCls}
        onClick={() => onBold(!bold)}
        aria-label={t("subtitle.exportBold", "粗体")}
      >
        B
      </Button>
      <Button
        size="sm"
        variant={italic ? "default" : "outline"}
        className={`${btnCls} italic`}
        onClick={() => onItalic(!italic)}
        aria-label={t("subtitle.exportItalic", "斜体")}
      >
        I
      </Button>
      <Button
        size="sm"
        variant={underline ? "default" : "outline"}
        className={`${btnCls} underline`}
        onClick={() => onUnderline(!underline)}
        aria-label={t("subtitle.exportUnderline", "下划线")}
      >
        U
      </Button>
    </div>
  );
}

// === SECTION 3 END ===

// === 主组件 ===

export function ExportDialog({ open, onOpenChange, file }: ExportDialogProps) {
  const { t } = useTranslation();
  const [format, setFormat] = useState<FormatKind>("srt");
  const [mode, setMode] = useState<ModeKind>("bilingual");
  const [monolingualLang, setMonolingualLang] = useState<MonoLang>("translated");
  const [translatedFirst, setTranslatedFirst] = useState(true);
  const [assStyle, setAssStyle] = useState<AssBilingualStyle>(DEFAULT_ASS_STYLE);
  const [merging, setMerging] = useState(false);

  const videoPath = useVideoStore((s) => s.probeResult?.video_path ?? null);
  const videoWidth = useVideoStore((s) => s.probeResult?.video_stream?.width ?? null);
  const videoHeight = useVideoStore((s) => s.probeResult?.video_stream?.height ?? null);
  const sourceLang = useTranslateStore((s) => s.sourceLang);
  const targetLang = useTranslateStore((s) => s.targetLang);

  // libmpv 子窗口是原生 OS 窗口（HWND），z-order 高于 WebView2，
  // 会遮盖 React 渲染的 Dialog。弹层打开时隐藏播放器子窗口，关闭时恢复。
  // playerHide/playerShow 在播放器未初始化时返回 Err，catch 掉即可。
  // 弹层打开时隐藏播放器（原生 OS 窗口 z-order 高于 WebView2，会遮盖弹层）
  // 弹层关闭时恢复播放器
  // handleExport/handleMergeToVideo 中的 save() 对话框不需要额外隐藏播放器，
  // 因为弹层打开时播放器已经被隐藏了
  useEffect(() => {
    if (!open) return;
    api.playerHide().catch(() => { /* 播放器未初始化，忽略 */ });
    return () => {
      api.playerShow().catch(() => { /* 播放器未初始化，忽略 */ });
    };
  }, [open]);

  // 组装当前选项（供预览 + 导出共用）
  const currentOptions = (): ExportOptions => ({
    format,
    mode,
    monolingual_lang: mode === "monolingual" ? monolingualLang : undefined,
    bilingual_translated_first: mode === "bilingual" ? translatedFirst : undefined,
    ass_style: format === "ass" && mode === "bilingual" ? assStyle : undefined,
    video_width: videoWidth ?? undefined,
    video_height: videoHeight ?? undefined,
  });

  const handleExport = useCallback(async () => {
    const options = currentOptions();
    // 前端过滤 _deleted 条目并剥离 _deleted 字段（Rust 端 SubtitleEntry 无此字段）
    const fileToExport: SubtitleFile = {
      ...file,
      entries: file.entries
        .filter((e) => !e._deleted)
        .map(({ _deleted, ...rest }) => rest),
    };
    // 默认保存到视频所在目录（用户最期望的行为）
    const baseName = videoPath ? stripExt(videoPath)
      : file.source_path ? stripExt(file.source_path)
      : "subtitle";
    const defaultFileName = buildExportFileName(options, sourceLang, targetLang, baseName);
    const videoDir = videoPath ? fileDir(videoPath)
      : file.source_path ? fileDir(file.source_path)
      : "";
    const defaultPath = videoDir ? `${videoDir}${defaultFileName}` : defaultFileName;
    const outputPath = await save({
      defaultPath,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (!outputPath) return;
    try {
      await api.exportSubtitle(fileToExport, outputPath, options);
      onOpenChange(false);
      toast.success(t("subtitle.exportSuccess"));
    } catch (e) {
      console.error("导出失败:", e);
      toast.error(t("subtitle.exportFailed"));
    }
  }, [format, mode, monolingualLang, translatedFirst, assStyle, file, videoPath, sourceLang, targetLang, onOpenChange, t]);

  // 合并字幕到视频：按当前选项渲染字幕到临时文件，再调用 mergeSubtitle
  // 智能选择输出位置：磁盘空间足够则直接修改原文件，不够则弹 save 对话框选其他盘
  const handleMergeToVideo = useCallback(async () => {
    if (!videoPath) return;
    setMerging(true);
    try {
      const options = currentOptions();
      const fileToExport: SubtitleFile = {
        ...file,
        entries: file.entries
          .filter((e) => !e._deleted)
          .map(({ _deleted, ...rest }) => rest),
      };
      // 渲染字幕到临时文件
      const tempDir = await import("@tauri-apps/api/path").then((m) => m.tempDir());
      const baseName = videoPath.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
      const tempSub = `${tempDir}${baseName}.merge.${format}`;
      await api.exportSubtitle(fileToExport, tempSub, options);

      const lang = mode === "monolingual"
        ? (monolingualLang === "source" ? sourceLang : targetLang)
        : targetLang;
      const title = buildSubtitleTitle(options, sourceLang, targetLang);

      // 检测磁盘空间
      const spaceInfo = await api.checkMergeSpace(videoPath);
      let outputPath: string | null = null;
      if (!spaceInfo.enough) {
        // 空间不够，弹 save 对话框让用户选其他盘
        const defaultMerged = `${fileDir(videoPath)}${baseName}.merged.mkv`;
        outputPath = await save({
          defaultPath: defaultMerged,
          filters: [{ name: "MKV", extensions: ["mkv"] }],
        });
        if (!outputPath) return; // 用户取消
      }

      await api.mergeSubtitle(videoPath, tempSub, outputPath, lang, title);
      onOpenChange(false);
      toast.success(t("subtitle.mergeSuccess"));
    } catch (e) {
      console.error("合并失败:", e);
      toast.error(t("subtitle.mergeFailed"));
    } finally {
      setMerging(false);
    }
  }, [videoPath, file, format, mode, monolingualLang, sourceLang, targetLang, onOpenChange, t]);

  // 分段控件按钮样式
  const segBtn = (active: boolean) =>
    `h-7 px-3 text-xs border rounded transition-colors ${active ? "bg-primary text-primary-foreground border-primary" : "bg-transparent hover:bg-accent"}`;

  const showAssStyle = format === "ass" && mode === "bilingual";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("subtitle.save", "保存字幕")}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 max-h-[70vh] overflow-y-auto pr-1 w-full min-w-0">
          {/* 格式选择 */}
          <div className="flex items-center gap-2">
            <span className="text-sm w-16 flex-shrink-0">{t("subtitle.exportFormat", "格式")}</span>
            <div className="flex gap-1">
              {(["srt", "ass", "vtt"] as FormatKind[]).map((f) => (
                <button
                  key={f}
                  className={segBtn(format === f)}
                  onClick={() => setFormat(f)}
                >
                  {f.toUpperCase()}
                </button>
              ))}
            </div>
          </div>

          {/* 模式选择 */}
          <div className="flex items-center gap-4">
            <span className="text-sm w-16 flex-shrink-0">{t("subtitle.exportMode", "模式")}</span>
            <label className="flex items-center gap-1 text-sm cursor-pointer">
              <input
                type="radio"
                name="export-mode"
                checked={mode === "monolingual"}
                onChange={() => setMode("monolingual")}
              />
              {t("subtitle.exportMonolingual", "单语")}
            </label>
            <label className="flex items-center gap-1 text-sm cursor-pointer">
              <input
                type="radio"
                name="export-mode"
                checked={mode === "bilingual"}
                onChange={() => setMode("bilingual")}
              />
              {t("subtitle.exportBilingual", "双语")}
            </label>
          </div>

          {/* 单语分支：语言选择 */}
          {mode === "monolingual" && (
            <div className="flex items-center gap-2 pl-4">
              <span className="text-sm w-12 flex-shrink-0">{t("subtitle.exportLang", "语言")}</span>
              <select
                value={monolingualLang}
                onChange={(e) => setMonolingualLang(e.target.value as MonoLang)}
                className="h-7 rounded border border-input bg-transparent px-2 text-xs"
              >
                <option value="translated">{t("subtitle.exportLangTranslated", "译文")}</option>
                <option value="source">{t("subtitle.exportLangSource", "原文")}</option>
              </select>
            </div>
          )}

          {/* 双语分支：顺序 + 翻转按钮 */}
          {mode === "bilingual" && (
            <div className="flex items-center gap-2 pl-4">
              <span className="text-sm w-12 flex-shrink-0">{t("subtitle.exportOrder", "顺序")}</span>
              <label className="flex items-center gap-1 text-sm cursor-pointer">
                <input
                  type="radio"
                  name="export-order"
                  checked={translatedFirst}
                  onChange={() => setTranslatedFirst(true)}
                />
                {t("subtitle.exportTranslatedFirst", "译文在上")}
              </label>
              <label className="flex items-center gap-1 text-sm cursor-pointer">
                <input
                  type="radio"
                  name="export-order"
                  checked={!translatedFirst}
                  onChange={() => setTranslatedFirst(false)}
                />
                {t("subtitle.exportSourceFirst", "原文在上")}
              </label>
              <Button
                size="sm"
                variant="outline"
                className="h-7 w-7 p-0"
                onClick={() => setTranslatedFirst((v) => !v)}
                aria-label={t("subtitle.exportSwapOrder", "翻转顺序")}
              >
                <ArrowUpDown className="h-3.5 w-3.5" />
              </Button>
            </div>
          )}

          {/* SRT/VTT 双语提示 */}
          {format !== "ass" && mode === "bilingual" && (
            <p className="text-xs text-muted-foreground pl-4">
              {t("subtitle.exportSrtNoStyleHint", "SRT/VTT 不支持样式，仅 ASS 可配置字号/颜色/特效")}
            </p>
          )}

          {/* ASS 样式配置区（仅 ASS + 双语） */}
          {showAssStyle && (
            <div className="space-y-3 border rounded-md p-3 bg-muted/30">
              <div className="flex items-center justify-between">
                <div className="text-sm font-medium">{t("subtitle.exportAssStyle", "ASS 样式")}</div>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-6 px-2 text-xs"
                  onClick={() => setAssStyle({ ...DEFAULT_ASS_STYLE })}
                >
                  <RotateCcw className="h-3 w-3 mr-1" />
                  {t("subtitle.exportResetStyle", "重置样式")}
                </Button>
              </div>
              <StyleRow
                label={t("subtitle.exportPrimaryLine", "第一行")}
                size={assStyle.primary_font_size}
                color={assStyle.primary_color}
                bold={assStyle.primary_bold}
                italic={assStyle.primary_italic}
                underline={assStyle.primary_underline}
                onSize={(v) => setAssStyle((s) => ({ ...s, primary_font_size: v }))}
                onColor={(v) => setAssStyle((s) => ({ ...s, primary_color: v }))}
                onBold={(v) => setAssStyle((s) => ({ ...s, primary_bold: v }))}
                onItalic={(v) => setAssStyle((s) => ({ ...s, primary_italic: v }))}
                onUnderline={(v) => setAssStyle((s) => ({ ...s, primary_underline: v }))}
              />
              <StyleRow
                label={t("subtitle.exportSecondaryLine", "第二行")}
                size={assStyle.secondary_font_size}
                color={assStyle.secondary_color}
                bold={assStyle.secondary_bold}
                italic={assStyle.secondary_italic}
                underline={assStyle.secondary_underline}
                onSize={(v) => setAssStyle((s) => ({ ...s, secondary_font_size: v }))}
                onColor={(v) => setAssStyle((s) => ({ ...s, secondary_color: v }))}
                onBold={(v) => setAssStyle((s) => ({ ...s, secondary_bold: v }))}
                onItalic={(v) => setAssStyle((s) => ({ ...s, secondary_italic: v }))}
                onUnderline={(v) => setAssStyle((s) => ({ ...s, secondary_underline: v }))}
              />
              <div className="flex items-center gap-4 flex-wrap">
                <label className="flex items-center gap-1 text-xs">
                  {t("subtitle.exportOutline", "描边")}
                  <Input
                    type="number"
                    value={assStyle.outline}
                    onChange={(e) => setAssStyle((s) => ({ ...s, outline: parseInt(e.target.value, 10) || 0 }))}
                    className="h-7 w-16 text-xs"
                  />
                  <input
                    type="color"
                    value={assColorToCss(assStyle.outline_color)}
                    onChange={(e) => setAssStyle((s) => ({ ...s, outline_color: hexToAssColor(e.target.value) }))}
                    className="h-7 w-7 rounded border cursor-pointer"
                    aria-label={t("subtitle.exportOutlineColor", "描边颜色")}
                  />
                </label>
                <label className="flex items-center gap-1 text-xs">
                  {t("subtitle.exportShadow", "阴影")}
                  <Input
                    type="number"
                    value={assStyle.shadow}
                    onChange={(e) => setAssStyle((s) => ({ ...s, shadow: parseInt(e.target.value, 10) || 0 }))}
                    className="h-7 w-16 text-xs"
                  />
                  <input
                    type="color"
                    value={assColorToCss(assStyle.shadow_color)}
                    onChange={(e) => setAssStyle((s) => ({ ...s, shadow_color: hexToAssColor(e.target.value) }))}
                    className="h-7 w-7 rounded border cursor-pointer"
                    aria-label={t("subtitle.exportShadowColor", "阴影颜色")}
                  />
                </label>
              </div>
            </div>
          )}

          {/* 实时预览 */}
          <div className="space-y-1">
            <div className="text-sm font-medium">{t("subtitle.exportPreview", "预览")}</div>
            <ExportPreview file={file} options={currentOptions()} assStyle={assStyle} />
          </div>
        </div>

        {/* 底部按钮 */}
        <div className="flex justify-between gap-2 pt-2 border-t">
          {/* 合并字幕到视频：仅视频模式下可用 */}
          {videoPath ? (
            <Button variant="outline" size="sm" onClick={handleMergeToVideo} disabled={merging}>
              {merging ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : <Merge className="mr-1 h-4 w-4" />}
              {t("subtitle.mergeToVideo", "合并到视频")}
            </Button>
          ) : <div />}
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
              {t("subtitle.exportCancel", "取消")}
            </Button>
            <Button size="sm" onClick={handleExport}>
              {t("subtitle.exportConfirm", "保存字幕")}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// === SECTION 4 END ===
