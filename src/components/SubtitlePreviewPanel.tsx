import { useRef, useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import { toast } from "sonner";
import { Save, Plus, Trash2, Undo2, Redo2, Search, Clock, X, ArrowLeft, Download, Languages, Copy, Edit3, Check, RotateCcw, Eraser, Loader2, Play, SplitSquareHorizontal, ArrowLeftRight } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useTranslateStore } from "../stores/translateStore";
import { AutoTextarea } from "./AutoTextarea";
import { api } from "../lib/api";
import type { SubtitleEntry } from "../lib/ipc-types";
import { uiState } from "../lib/utils";
import { ExportDialog } from "./ExportDialog";

type PreviewMode = "original" | "bilingual" | "translated";

function formatTimecode(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const millis = ms % 1000;
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  return `${h.toString().padStart(2, "0")}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")},${millis.toString().padStart(3, "0")}`;
}

export function SubtitlePreviewPanel({ extracting = false, extractProgress = 0, currentPlayTime = 0 }: { extracting?: boolean; extractProgress?: number; currentPlayTime?: number }) {
  const { t } = useTranslation();
  const store = useSubtitleStore();
  const { file, bilingualDetect, isSplit } = store;
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [showFindReplace, setShowFindReplace] = useState(false);
  const [offsetInput, setOffsetInput] = useState("");
  const [showOffset, setShowOffset] = useState(false);
  const [previewMode, setPreviewMode] = useState<PreviewMode>("bilingual");
  const [exportOpen, setExportOpen] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entryIndex: number } | null>(null);
  const translateStore = useTranslateStore();

  const rowVirtualizer = useVirtualizer({
    count: file?.entries.length ?? 0,
    getScrollElement: () => parentRef.current,
    estimateSize: (index) => {
      if (file && file.entries[index]?.index === editingIndex) return 200;
      return 72;
    },
    overscan: 5,
    measureElement: (el) => el.getBoundingClientRect().height,
  });

  // 根据播放时间高亮当前字幕条目并自动滚动
  const currentPlayMs = currentPlayTime * 1000;
  const activeEntryIndex = file?.entries.findIndex(
    (e) => !e._deleted && currentPlayMs >= e.start_ms && currentPlayMs < e.end_ms
  ) ?? -1;

  // 鼠标是否悬停在字幕编辑器区域内（用 ref 避免 re-render）
  const isMouseOverPanelRef = useRef(false);
  // 是否正在执行自动滚动（防止 scroll 事件监听器与自身平滑滚动形成循环）
  const isAutoScrollingRef = useRef(false);

  // 检查当前播放字幕是否在可见区域外，若是且鼠标不在面板上，则平滑滚动到第三排
  const maybeScrollToActive = useCallback(() => {
    if (activeEntryIndex < 0) return;
    if (isMouseOverPanelRef.current) return;
    if (isAutoScrollingRef.current) return;
    // 有下拉框（如音轨选择）展开时暂停自动滚动，避免滚动动画导致下拉菜单被浏览器收起
    if (uiState.selectOpen) return;

    const scrollEl = parentRef.current;
    if (!scrollEl) return;

    const containerRect = scrollEl.getBoundingClientRect();
    const activeEl = scrollEl.querySelector(
      `[data-index="${activeEntryIndex}"]`
    ) as HTMLElement | null;

    // 判断当前字幕是否在可见区域外
    let isOutside: boolean;
    if (!activeEl) {
      // 未渲染（虚拟滚动裁掉了）→ 一定在可见区域外
      isOutside = true;
    } else {
      const elRect = activeEl.getBoundingClientRect();
      // 完全可见 = 元素顶部 >= 容器顶部 且 元素底部 <= 容器底部
      isOutside = elRect.top < containerRect.top - 1 || elRect.bottom > containerRect.bottom + 1;
    }

    if (!isOutside) return;

    // 平滑滚动：将 activeEntryIndex 定位到第三排（其上方留 2 行）
    isAutoScrollingRef.current = true;
    rowVirtualizer.scrollToIndex(Math.max(0, activeEntryIndex - 2), {
      align: "start",
      behavior: "smooth",
    });
    // 平滑滚动动画结束后释放锁（smooth scroll 一般 ≤ 800ms）
    window.setTimeout(() => {
      isAutoScrollingRef.current = false;
    }, 800);
  }, [activeEntryIndex, rowVirtualizer]);

  // 当前播放字幕变化时触发检查
  useEffect(() => {
    maybeScrollToActive();
  }, [activeEntryIndex, maybeScrollToActive]);

  // 用户手动滚动后（鼠标已离开面板）也触发检查，把跑出可见区的字幕拉回第三排
  useEffect(() => {
    const scrollEl = parentRef.current;
    if (!scrollEl) return;
    let timer: number | undefined;
    const onScroll = () => {
      // 忽略自身触发的滚动
      if (isAutoScrollingRef.current) return;
      if (timer) window.clearTimeout(timer);
      timer = window.setTimeout(() => maybeScrollToActive(), 250);
    };
    scrollEl.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      scrollEl.removeEventListener("scroll", onScroll);
      if (timer) window.clearTimeout(timer);
    };
  }, [maybeScrollToActive]);

  const handleSave = useCallback(() => {
    if (!file) return;
    setExportOpen(true);
  }, [file]);

  // Ctrl+S 快捷键：监听 MainView 分发的 "export-subtitle" 事件，打开 ExportDialog
  useEffect(() => {
    const onExport = () => { if (file) setExportOpen(true); };
    window.addEventListener("export-subtitle", onExport);
    return () => window.removeEventListener("export-subtitle", onExport);
  }, [file]);

  const handleAddEntry = useCallback(() => {
    if (!file) return;
    const maxIndex = file.entries.reduce((max, e) => Math.max(max, e.index), -1);
    const newEntry: SubtitleEntry = {
      index: maxIndex + 1,
      start_ms: 0,
      end_ms: 1000,
      text: "",
      translated: "",
      style: null,
    };
    store.addEntry(newEntry);
  }, [file, store]);

  const handleApplyOffset = useCallback(() => {
    const offset = parseInt(offsetInput, 10);
    if (!isNaN(offset)) {
      store.applyTimeOffset(offset);
      setShowOffset(false);
      setOffsetInput("");
    }
  }, [offsetInput, store]);

  // 右键菜单处理
  const handleContextMenu = useCallback((e: React.MouseEvent, entryIndex: number) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, entryIndex });
  }, []);

  // 关闭右键菜单
  const closeContextMenu = useCallback(() => setContextMenu(null), []);

  // 翻译单条字幕
  const handleTranslateOne = useCallback(async (entryIndex: number) => {
    if (!file) return;
    const entry = file.entries.find((e) => e.index === entryIndex);
    if (!entry) return;
    // 跳过 ass 矢量绘图指令（含 \p1 标记），不是字幕文本
    if (entry.text.includes("\\p1")) {
      closeContextMenu();
      return;
    }
    closeContextMenu();
    try {
      const result = await translateStore.startTranslate(
        [entry],
        (index, translated) => {
          // 单条翻译完成，立即更新
          store.updateEntry(index, { translated });
        }
      );
    } catch (e) {
      console.error("翻译单条失败:", e);
    }
  }, [file, translateStore, store, closeContextMenu]);

  // 复制原文到剪贴板
  const handleCopyOriginal = useCallback((entryIndex: number) => {
    if (!file) return;
    const entry = file.entries.find((e) => e.index === entryIndex);
    if (entry) {
      navigator.clipboard.writeText(entry.text);
    }
    closeContextMenu();
  }, [file, closeContextMenu]);

  // 复制译文到剪贴板
  const handleCopyTranslated = useCallback((entryIndex: number) => {
    if (!file) return;
    const entry = file.entries.find((e) => e.index === entryIndex);
    if (entry?.translated) {
      navigator.clipboard.writeText(entry.translated);
    }
    closeContextMenu();
  }, [file, closeContextMenu]);

  // 删除单条字幕
  const handleDeleteEntry = useCallback((entryIndex: number) => {
    store.deleteEntry(entryIndex);
    closeContextMenu();
  }, [store, closeContextMenu]);

  // 从该字幕开始时刻播放视频：精确 seek 到 start_ms / 1000 秒并播放
  const handlePlayFromHere = useCallback(async (entryIndex: number) => {
    if (!file) return;
    const entry = file.entries.find((e) => e.index === entryIndex);
    if (!entry) return;
    closeContextMenu();
    try {
      const startSec = entry.start_ms / 1000;
      // 先 seek 再 play，确保从精确时刻开始播放
      await api.playerSeek(startSec);
      await api.playerPlay();
    } catch (e) {
      console.error("跳转播放失败:", e);
      toast.error(t("subtitle.playFromHereFailed"));
    }
  }, [file, closeContextMenu, t]);

  // 点击外部关闭右键菜单
  useEffect(() => {
    if (!contextMenu) return;
    const handleClick = () => closeContextMenu();
    const handleEsc = (e: KeyboardEvent) => { if (e.key === "Escape") closeContextMenu(); };
    window.addEventListener("click", handleClick);
    window.addEventListener("keydown", handleEsc);
    return () => {
      window.removeEventListener("click", handleClick);
      window.removeEventListener("keydown", handleEsc);
    };
  }, [contextMenu, closeContextMenu]);

  // 点击外部关闭编辑框
  useEffect(() => {
    if (editingIndex === null) return;
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      // 如果点击的不是 textarea 或按钮，关闭编辑
      if (!target.closest("textarea") && !target.closest("button")) {
        setEditingIndex(null);
        toast.warning(t("subtitle.editCancelled"));
      }
    };
    // 延迟绑定，避免触发编辑的同一 click 事件
    const timer = setTimeout(() => {
      window.addEventListener("click", handleClickOutside);
    }, 100);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("click", handleClickOutside);
    };
  }, [editingIndex, t]);

  // === SECTION 1 END ===

  if (extracting && !file) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground">
        <div className="text-center w-48">
          <Loader2 className="mx-auto h-8 w-8 animate-spin mb-3" />
          <p className="text-sm mb-2">{t("subtitle.extracting")}</p>
          {extractProgress > 0 && (
            <>
              <div className="w-full h-1.5 bg-muted rounded-full overflow-hidden">
                <div
                  className="h-full bg-primary rounded-full transition-all duration-300"
                  style={{ width: `${Math.min(100, extractProgress)}%` }}
                />
              </div>
              <p className="text-xs mt-1 tabular-nums">{extractProgress.toFixed(0)}%</p>
            </>
          )}
        </div>
      </div>
    );
  }

  if (!file) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground">
        <div className="text-center">
          <p className="text-sm">{t("subtitle.empty", "未加载字幕")}</p>
          <p className="mt-1 text-xs opacity-60">{t("subtitle.emptyHint", "请打开字幕文件或从视频提取字幕")}</p>
        </div>
      </div>
    );
  }

  return (
    <div
      className="flex h-full flex-col overflow-hidden"
      onMouseEnter={() => { isMouseOverPanelRef.current = true; }}
      onMouseLeave={() => { isMouseOverPanelRef.current = false; }}
    >
      {/* 工具栏 */}
      <div className="flex items-center gap-1 border-b px-2 py-1 flex-shrink-0">
        <Button size="sm" variant="ghost" onClick={store.undo} disabled={store.undoStack.length === 0} className="h-7 w-7 p-0">
          <Undo2 className="h-3.5 w-3.5" />
        </Button>
        <Button size="sm" variant="ghost" onClick={store.redo} disabled={store.redoStack.length === 0} className="h-7 w-7 p-0">
          <Redo2 className="h-3.5 w-3.5" />
        </Button>
        <div className="w-px h-4 bg-border mx-1" />
        <Button size="sm" variant="ghost" onClick={() => setShowFindReplace(!showFindReplace)} className="h-7 px-2">
          <Search className="h-3.5 w-3.5" />
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setShowOffset(!showOffset)} className="h-7 px-2">
          <Clock className="h-3.5 w-3.5" />
        </Button>
        <Button size="sm" variant="ghost" onClick={handleAddEntry} className="h-7 px-2">
          <Plus className="h-3.5 w-3.5" />
        </Button>
        <div className="flex-1" />
        {/* 切换原译：将原文和译文对调（仅已拆分时可用） */}
        {isSplit && (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 px-2 text-xs"
            onClick={() => store.swapOriginalTranslated()}
            disabled={!file.entries.some((e) => e.translated)}
            title={t("subtitle.swapOriginalTranslated", "切换原译")}
          >
            <ArrowLeftRight className="mr-1 h-3.5 w-3.5" />
            {t("subtitle.swapOriginalTranslated", "切换原译")}
          </Button>
        )}
        {/* 拆分字幕 / 取消拆分 */}
        {isSplit ? (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 px-2 text-xs"
            onClick={() => store.unsplitBilingual()}
            title={t("subtitle.unsplitBilingual", "取消拆分")}
          >
            <SplitSquareHorizontal className="mr-1 h-3.5 w-3.5" />
            {t("subtitle.unsplitBilingual", "取消拆分")}
          </Button>
        ) : (
          bilingualDetect && (
            <Button
              size="sm"
              variant="ghost"
              className="h-7 px-2 text-xs"
              onClick={() => store.splitBilingual()}
              title={t("subtitle.splitBilingual", "拆分字幕")}
            >
              <SplitSquareHorizontal className="mr-1 h-3.5 w-3.5" />
              {t("subtitle.splitBilingual", "拆分字幕")}
            </Button>
          )
        )}
        {/* 清除翻译结果 */}
        <Button
          size="sm"
          variant="ghost"
          className="h-7 px-2 text-xs"
          onClick={() => {
            store.clearTranslations();
            toast.success(t("subtitle.translationsCleared"));
          }}
          disabled={!file.entries.some((e) => e.translated)}
        >
          <Eraser className="mr-1 h-3.5 w-3.5" />
          {t("subtitle.clearTranslations", "清除翻译")}
        </Button>
        {/* 预览模式选择 */}
        <select
          value={previewMode}
          onChange={(e) => setPreviewMode(e.target.value as PreviewMode)}
          className="h-7 rounded border border-input bg-transparent px-2 text-xs"
        >
          <option value="original">{t("subtitle.modeOriginal", "原文")}</option>
          <option value="bilingual">{t("subtitle.modeBilingual", "双语")}</option>
          <option value="translated">{t("subtitle.modeTranslated", "仅译文")}</option>
        </select>
        <div className="w-px h-4 bg-border mx-1" />
        <Button size="sm" onClick={handleSave} className="h-7">
          <Save className="mr-1 h-3.5 w-3.5" />
          {t("subtitle.save", "保存")}
        </Button>
      </div>

      {/* 查找替换 */}
      {showFindReplace && (
        <div className="flex items-center gap-2 border-b px-3 py-1.5 bg-muted/30 flex-shrink-0">
          <Input
            placeholder={t("subtitle.find", "查找")}
            value={store.findQuery}
            onChange={(e) => store.setFindQuery(e.target.value)}
            className="h-7 w-32 text-xs"
          />
          <Input
            placeholder={t("subtitle.replace", "替换")}
            value={store.replaceQuery}
            onChange={(e) => store.setReplaceQuery(e.target.value)}
            className="h-7 w-32 text-xs"
          />
          <Button size="sm" variant="secondary" className="h-7 text-xs" onClick={() => store.findReplace(store.findQuery, store.replaceQuery, false)}>
            {t("subtitle.replaceOne", "替换")}
          </Button>
          <Button size="sm" variant="secondary" className="h-7 text-xs" onClick={() => store.findReplace(store.findQuery, store.replaceQuery, true)}>
            {t("subtitle.replaceAll", "全部")}
          </Button>
          <Button size="sm" variant="ghost" className="h-7 w-7 p-0" onClick={() => setShowFindReplace(false)}>
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      )}

      {/* 时间轴偏移 */}
      {showOffset && (
        <div className="flex items-center gap-2 border-b px-3 py-1.5 bg-muted/30 flex-shrink-0">
          <span className="text-xs">{t("subtitle.timeOffset", "时间轴偏移")} (ms):</span>
          <Input
            type="number"
            value={offsetInput}
            onChange={(e) => setOffsetInput(e.target.value)}
            placeholder="±1000"
            className="h-7 w-24 text-xs"
          />
          <Button size="sm" className="h-7 text-xs" onClick={handleApplyOffset}>{t("common.apply", "应用")}</Button>
          <Button size="sm" variant="ghost" className="h-7 w-7 p-0" onClick={() => setShowOffset(false)}>
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      )}

      {/* 字幕对比预览列表（虚拟滚动） */}
      <div ref={parentRef} className="flex-1 min-h-0 overflow-auto">
        <div
          style={{
            height: `${rowVirtualizer.getTotalSize()}px`,
            width: "100%",
            position: "relative",
          }}
        >
          {rowVirtualizer.getVirtualItems().map((virtualRow) => {
            const entry = file.entries[virtualRow.index];
            const isEditing = editingIndex === entry.index;
            const hasTranslation = entry.translated && entry.translated.length > 0;
            const isActive = virtualRow.index === activeEntryIndex;
            return (
              <div
                key={entry.index}
                data-index={virtualRow.index}
                ref={rowVirtualizer.measureElement}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${virtualRow.start}px)`,
                }}
                className={`group border-b px-3 py-1.5 hover:bg-accent/30 ${isActive ? "bg-primary/10 border-l-2 border-l-primary" : ""}`}
                onContextMenu={(e) => handleContextMenu(e, entry.index)}
              >
                {/* 时间码行 */}
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-1.5 min-w-0">
                    <span className="font-mono text-xs text-muted-foreground truncate">
                      #{entry.index} · {formatTimecode(entry.start_ms)} → {formatTimecode(entry.end_ms)}
                    </span>
                    {/* 从该字幕开始时刻播放（仅 hover 显示，与删除按钮一致） */}
                    <button
                      onClick={(e) => { e.stopPropagation(); handlePlayFromHere(entry.index); }}
                      className="flex h-4 w-4 flex-shrink-0 items-center justify-center rounded text-muted-foreground/60 opacity-0 group-hover:opacity-100 transition-opacity hover:bg-primary hover:text-primary-foreground"
                      title={t("subtitle.playFromHereHint", "从该字幕开始时刻播放视频")}
                    >
                      <Play className="h-3 w-3 translate-x-[0.5px]" />
                    </button>
                  </div>
                  {isEditing ? (
                    /* 编辑态：完成、取消、删除按钮 */
                    <div className="flex gap-1 flex-shrink-0">
                      <Button
                        size="sm"
                        className="h-5 px-2 text-xs bg-green-600 hover:bg-green-700"
                        onClick={(e) => { e.stopPropagation(); setEditingIndex(null); }}
                      >
                        <Check className="h-3 w-3 mr-0.5" />
                        {t("common.done", "完成")}
                      </Button>
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-5 px-2 text-xs"
                        onClick={(e) => { e.stopPropagation(); setEditingIndex(null); toast.warning(t("subtitle.editCancelled", "编辑已取消")); }}
                      >
                        {t("common.cancel", "取消")}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-5 w-5 p-0"
                        onClick={(e) => { e.stopPropagation(); store.deleteEntry(entry.index); setEditingIndex(null); }}
                      >
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </div>
                  ) : !entry._deleted && (
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                      onClick={(e) => { e.stopPropagation(); store.deleteEntry(entry.index); }}
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  )}
                </div>

                {/* 字幕内容 */}
                {isEditing ? (
                  /* 编辑态：行内展开 */
                  <div className="mt-1 space-y-1">
                    {/* 原文只读 */}
                    <p className="text-xs text-muted-foreground bg-muted/30 rounded px-2 py-1 max-h-20 overflow-auto">
                      {entry.text || <span className="opacity-30">—</span>}
                    </p>
                    {/* 译文编辑 */}
                    <AutoTextarea
                      value={entry.translated}
                      onChange={(val) => store.updateEntry(entry.index, { translated: val })}
                      className="text-xs py-1 flex-1 resize-none"
                      placeholder={t("subtitle.translated", "译文")}
                      onClick={(e) => e.stopPropagation()}
                      autoFocus
                    />
                  </div>
                ) : entry._deleted ? (
                  /* 已删除态：显示删除线 + 撤销按钮 */
                  <div className="mt-0.5 flex items-center gap-2">
                    <span className="text-xs line-through text-muted-foreground">
                      {entry.text || <span className="opacity-30">—</span>}
                    </span>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-5 px-2 text-xs text-blue-600 hover:text-blue-700"
                      onClick={(e) => { e.stopPropagation(); store.undoDelete(entry.index); }}
                    >
                      <RotateCcw className="h-3 w-3 mr-0.5" />
                      {t("subtitle.undoDelete", "撤销删除")}
                    </Button>
                  </div>
                ) : (
                  <div className="mt-0.5 space-y-0.5">
                    {/* 原文行（只读，不可编辑） */}
                    {(previewMode === "original" || previewMode === "bilingual") && (
                      <p className="text-xs line-clamp-1">{entry.text || <span className="opacity-30">—</span>}</p>
                    )}
                    {/* 译文行（点击进入编辑） */}
                    {(previewMode === "translated" || previewMode === "bilingual") && hasTranslation && (
                      <p
                        className="text-xs text-primary line-clamp-1 cursor-text hover:bg-primary/10 rounded px-1 -mx-1"
                        onClick={(e) => { e.stopPropagation(); setEditingIndex(entry.index); }}
                      >
                        {entry.translated}
                      </p>
                    )}
                    {/* 翻译中占位 */}
                    {previewMode === "bilingual" && !hasTranslation && (
                      <p className="text-xs text-muted-foreground/50 line-clamp-1">{t("subtitle.pending", "(待翻译)")}</p>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>

      {/* 右键上下文菜单 */}
      {contextMenu && (
        <div
          className="fixed z-50 min-w-[160px] rounded-md border bg-popover p-1 shadow-md"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
            onClick={() => handlePlayFromHere(contextMenu.entryIndex)}
            title={t("subtitle.playFromHereHint", "从该字幕开始时刻播放视频")}
          >
            <Play className="h-3.5 w-3.5" />
            {t("subtitle.playFromHere", "从此处播放")}
          </button>
          <div className="my-1 h-px bg-border" />
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent disabled:opacity-40 disabled:cursor-not-allowed"
            onClick={() => handleTranslateOne(contextMenu.entryIndex)}
            disabled={translateStore.translating}
          >
            <Languages className="h-3.5 w-3.5" />
            {t("subtitle.translateOne", "翻译此条字幕")}
          </button>
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
            onClick={() => { setEditingIndex(contextMenu.entryIndex); closeContextMenu(); }}
          >
            <Edit3 className="h-3.5 w-3.5" />
            {t("subtitle.editTranslation", "编辑译文")}
          </button>
          <div className="my-1 h-px bg-border" />
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
            onClick={() => handleCopyOriginal(contextMenu.entryIndex)}
          >
            <Copy className="h-3.5 w-3.5" />
            {t("subtitle.copyOriginal", "复制原文")}
          </button>
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent disabled:opacity-50"
            onClick={() => handleCopyTranslated(contextMenu.entryIndex)}
            disabled={!file.entries.find((e) => e.index === contextMenu.entryIndex)?.translated}
          >
            <Copy className="h-3.5 w-3.5" />
            {t("subtitle.copyTranslated", "复制译文")}
          </button>
          <div className="my-1 h-px bg-border" />
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs text-destructive hover:bg-destructive/10"
            onClick={() => handleDeleteEntry(contextMenu.entryIndex)}
          >
            <Trash2 className="h-3.5 w-3.5" />
            {t("subtitle.deleteEntry", "删除此条")}
          </button>
        </div>
      )}

      {/* 底部状态 */}
      <div className="flex items-center justify-between border-t px-3 py-1 text-xs text-muted-foreground flex-shrink-0">
        <span>{t("subtitle.count", "条目数")}: {file.entries.length}</span>
        {store.undoStack.length > 0 && <span className="text-orange-500">● {t("subtitle.unsaved", "已修改")}</span>}
      </div>

      {/* 导出弹层（file 非空才挂载，避免 Props 类型不匹配） */}
      {file && <ExportDialog open={exportOpen} onOpenChange={setExportOpen} file={file} />}
    </div>
  );
}

// === SECTION 2 END ===
