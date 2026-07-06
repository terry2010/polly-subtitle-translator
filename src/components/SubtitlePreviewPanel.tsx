import { useRef, useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import { toast } from "sonner";
import { Save, Plus, Trash2, Undo2, Redo2, Search, Clock, X, ArrowLeft, Download, Languages, Copy, Edit3, Check, RotateCcw, Eraser, Loader2, Play, SplitSquareHorizontal, ArrowLeftRight, ChevronUp, ChevronDown, AlertTriangle, FileText } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useTranslateStore } from "../stores/translateStore";
import { useVideoStore } from "../stores/videoStore";
import { useDevModeStore } from "../stores/devModeStore";
import { AutoTextarea } from "./AutoTextarea";
import { api } from "../lib/api";
import { error } from "../lib/logger";
import type { SubtitleEntry } from "../lib/ipc-types";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { uiState, withPlayerHidden } from "../lib/utils";
import { ExportDialog } from "./ExportDialog";

type PreviewMode = "original" | "bilingual" | "translated";

/// 判断文本是否为音效/环境声标记，如 [clattering continues] / [碰撞声持续]
function looksLikeSoundEffect(s: string): boolean {
  // 先去掉 ASS 定位/样式标签（如 {\an8}），与 translate.rs 的实现一致
  // 否则含 {\an8} 前缀的音效标记（如 {\an8}[phone buzzing]）会被误判为非音效标记，
  // 导致翻译时 isUntranslated 与导出往返后 isUntranslated 不一致
  const stripped = s.replace(/\{[^}]*\}/g, "");
  const t = stripped.trim();
  if (!t) return false;
  if (t.startsWith("[") && t.endsWith("]")) return true;
  // 去掉 [Name] 前缀后，剩余部分仍被 [] 包裹
  const m = t.match(/^\s*\[[^\]]+\]\s*(.*)$/);
  if (m) {
    const rest = m[1].trim();
    if (rest && rest.startsWith("[") && rest.endsWith("]")) return true;
  }
  return false;
}

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
  const videoStore = useVideoStore();
  const devMode = useDevModeStore((s) => s.devMode);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [showFindReplace, setShowFindReplace] = useState(false);
  const [previewMode, setPreviewMode] = useState<PreviewMode>("bilingual");
  const [exportOpen, setExportOpen] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entryIndex: number } | null>(null);
  const translateStore = useTranslateStore();
  // 时间轴偏移：行内面板
  const [offsetRowIndex, setOffsetRowIndex] = useState<number | null>(null);
  const [offsetValue, setOffsetValue] = useState("");
  const [offsetEndIndex, setOffsetEndIndex] = useState("");
  const [offsetAppliedMsg, setOffsetAppliedMsg] = useState<string | null>(null);
  // 已应用过偏移的行 → 偏移提示消息（永久显示）
  const [offsetAppliedRows, setOffsetAppliedRows] = useState<Map<number, string>>(new Map());
  // 已应用过偏移的行 → 上次填入的偏移值和结束编号（再次打开时恢复）
  const [offsetLastInput, setOffsetLastInput] = useState<Map<number, { value: string; endIndex: string }>>(new Map());
  // 超出视频时长确认弹窗
  const [offsetExceedDialog, setOffsetExceedDialog] = useState<{ count: number; maxExceedSec: number; offsetMs: number; fromIndex: number; toIndex: number } | null>(null);
  // 新增字幕：时间编辑面板
  const [insertEditingIndex, setInsertEditingIndex] = useState<number | null>(null);
  const [insertStartMs, setInsertStartMs] = useState(0);
  const [insertEndMs, setInsertEndMs] = useState(0);
  const [insertText, setInsertText] = useState("");
  const [insertTranslated, setInsertTranslated] = useState("");
  // 已完成编辑的新增字幕 → 保存最终状态（永久提示 + 恢复）
  const [insertDoneRows, setInsertDoneRows] = useState<Map<number, { start_ms: number; end_ms: number; translated: string }>>(new Map());
  // 重置确认弹窗
  const [resetDialogOpen, setResetDialogOpen] = useState(false);
  const [resetSteps, setResetSteps] = useState(0);

  const rowVirtualizer = useVirtualizer({
    count: file?.entries.length ?? 0,
    getScrollElement: () => parentRef.current,
    // 用 entry.index 作为测量缓存 key，避免插入/删除条目后虚拟索引移位导致
    // 旧测量值（如新增字幕的 280px）残留在移位后的索引上，产生大块空白
    getItemKey: (index) => file?.entries[index]?.index ?? index,
    estimateSize: (index) => {
      if (file && file.entries[index]?.index === editingIndex) return 200;
      if (file && file.entries[index]?.index === offsetRowIndex) return 120;
      if (file && file.entries[index]?.index === insertEditingIndex) return 280;
      if (file && offsetAppliedRows.has(file.entries[index]?.index)) return 90;
      if (file && insertDoneRows.has(file.entries[index]?.index)) return 90;
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
  // 同时同步到全局 uiState.mouseInSubtitleEditor，供 VideoPlayer 判断空格键是否响应
  const isMouseOverPanelRef = useRef(false);
  // 是否正在执行自动滚动（防止 scroll 事件监听器与自身平滑滚动形成循环）
  const isAutoScrollingRef = useRef(false);

  // 编辑取消支持：进入编辑时记录原始译文值和 undoStack 长度。
  // ESC 或"取消"按钮调用 store.cancelEditEntry 恢复，让 undo 回到编辑前状态。
  const editingOriginalRef = useRef<string>("");
  const editingUndoStackLenRef = useRef<number>(0);

  // 进入编辑态：记录原始译文和 undoStack 长度，用于 ESC/取消时恢复
  const beginEdit = useCallback((entryIndex: number, originalTranslated: string) => {
    editingOriginalRef.current = originalTranslated;
    editingUndoStackLenRef.current = store.undoStack.length;
    setEditingIndex(entryIndex);
  }, [store]);

  // 退出编辑态：不保存（恢复原始译文，截断 undoStack 到编辑前）
  const cancelEdit = useCallback((entryIndex: number) => {
    store.cancelEditEntry(entryIndex, editingOriginalRef.current, editingUndoStackLenRef.current);
    setEditingIndex(null);
  }, [store]);

  // 退出编辑态：保存（仅退出，编辑过程中的 onChange 已实时写入 store）
  const commitEdit = useCallback(() => {
    setEditingIndex(null);
  }, []);

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

  // 在当前字幕下方插入新字幕
  const handleInsertEntry = useCallback((entryIndex: number) => {
    if (!file) return;
    const curIdx = file.entries.findIndex((e) => e.index === entryIndex);
    if (curIdx === -1) return;
    const cur = file.entries[curIdx];
    // 找下一条非删除条目
    let nextEntry: SubtitleEntry | null = null;
    for (let i = curIdx + 1; i < file.entries.length; i++) {
      if (!file.entries[i]._deleted) { nextEntry = file.entries[i]; break; }
    }
    const maxIndex = file.entries.reduce((max, e) => Math.max(max, e.index), -1);
    const start_ms = cur.end_ms;
    const end_ms = nextEntry ? nextEntry.start_ms : start_ms + 1000;
    const newEntry: SubtitleEntry = {
      index: maxIndex + 1,
      start_ms,
      end_ms: Math.max(end_ms, start_ms), // 保证 end >= start
      text: "",
      translated: "",
      style: null,
    };
    store.insertEntryAfter(newEntry, entryIndex);
    // 进入新增字幕的时间编辑面板
    setInsertEditingIndex(newEntry.index);
    setInsertStartMs(newEntry.start_ms);
    setInsertEndMs(newEntry.end_ms);
    setInsertText("");
    setInsertTranslated("");
  }, [file, store]);

  // 打开行内时间轴偏移面板
  const openOffsetPanel = useCallback((entryIndex: number) => {
    if (!file) return;
    setOffsetRowIndex(entryIndex);
    setOffsetAppliedMsg(null);
    // 已偏移过的行恢复上次的输入值，否则用默认值
    const last = offsetLastInput.get(entryIndex);
    if (last) {
      setOffsetValue(last.value);
      setOffsetEndIndex(last.endIndex);
    } else {
      setOffsetValue("");
      const lastEntry = file.entries[file.entries.length - 1];
      setOffsetEndIndex(lastEntry ? String(lastEntry.index) : String(entryIndex));
    }
  }, [file, offsetLastInput]);

  // 关闭偏移面板
  const closeOffsetPanel = useCallback(() => {
    setOffsetRowIndex(null);
    setOffsetValue("");
    setOffsetAppliedMsg(null);
  }, []);

  // 本地计算偏移后的条目（不修改 store），用于检查
  const computeOffsetEntries = useCallback((offsetMs: number, fromIndex: number, toIndex: number) => {
    if (!file) return [];
    return file.entries.map((e) => {
      if (e.index < fromIndex || e.index > toIndex || e._deleted) return e;
      const duration = e.end_ms - e.start_ms;
      const newStart = e.start_ms + offsetMs;
      if (newStart < 0) {
        return { ...e, start_ms: 0, end_ms: duration };
      }
      return { ...e, start_ms: newStart, end_ms: e.end_ms + offsetMs };
    });
  }, [file]);

  // 实际应用偏移到 store + 更新 UI 状态
  const commitOffset = useCallback((offsetMs: number, fromIndex: number, toIndex: number, offsetSec: number) => {
    store.applyTimeOffset(offsetMs, fromIndex, toIndex);
    // 构建提示消息
    const msgs: string[] = [`已偏移 ${offsetSec > 0 ? "+" : ""}${offsetSec} 秒`];
    // 本地计算裁剪和重叠（基于偏移前的 file）
    if (file) {
      let clippedCount = 0;
      let overlapCount = 0;
      const newEntries = computeOffsetEntries(offsetMs, fromIndex, toIndex);
      for (const e of file.entries) {
        if (e.index < fromIndex || e.index > toIndex || e._deleted) continue;
        if (e.start_ms + offsetMs < 0 && e.start_ms > 0) clippedCount++;
      }
      // 检查重叠：用计算后的条目和全局前一条比较
      for (let i = 0; i < newEntries.length; i++) {
        const e = newEntries[i];
        if (e.index < fromIndex || e.index > toIndex || e._deleted) continue;
        for (let j = i - 1; j >= 0; j--) {
          const prev = newEntries[j];
          if (prev._deleted) continue;
          if (e.start_ms < prev.end_ms) overlapCount++;
          break;
        }
      }
      if (clippedCount > 0) msgs.push(`${clippedCount} 条开始时间裁剪到 0`);
      if (overlapCount > 0) msgs.push(`${overlapCount} 条开始时间早于上一条`);
    }
    const msgStr = msgs.join("，");
    setOffsetAppliedRows((prev) => new Map(prev).set(fromIndex, msgStr));
    setOffsetLastInput((prev) => new Map(prev).set(fromIndex, { value: offsetValue, endIndex: offsetEndIndex }));
    setOffsetAppliedMsg(msgStr);
  }, [file, store, computeOffsetEntries, offsetValue, offsetEndIndex]);

  // 执行时间轴偏移：先本地检查，再决定是否应用
  const handleApplyOffset = useCallback(() => {
    if (!file || offsetRowIndex == null) return;
    const offsetSec = parseFloat(offsetValue);
    if (isNaN(offsetSec)) return;
    const offsetMs = Math.round(offsetSec * 1000);
    const toIndex = parseInt(offsetEndIndex, 10);
    if (isNaN(toIndex)) return;
    const fromIndex = offsetRowIndex;

    // 本地计算偏移后的条目（不修改 store）
    const newEntries = computeOffsetEntries(offsetMs, fromIndex, toIndex);

    // 检查超出视频时长
    const videoDuration = videoStore.probeResult?.format?.duration;
    let exceeded = 0;
    let maxExceed = 0;
    if (videoDuration) {
      const durationMs = videoDuration * 1000;
      for (const e of newEntries) {
        if (e.index < fromIndex || e.index > toIndex || e._deleted) continue;
        if (e.end_ms > durationMs) {
          exceeded++;
          const exceed = (e.end_ms - durationMs) / 1000;
          if (exceed > maxExceed) maxExceed = exceed;
        }
      }
    }

    if (exceeded > 0) {
      // 有超出：弹窗确认，暂不应用
      setOffsetExceedDialog({ count: exceeded, maxExceedSec: maxExceed, offsetMs, fromIndex, toIndex });
    } else {
      // 无超出：直接应用
      commitOffset(offsetMs, fromIndex, toIndex, offsetSec);
    }
  }, [file, offsetRowIndex, offsetValue, offsetEndIndex, computeOffsetEntries, videoStore, commitOffset]);

  // 强制应用（超出时长仍然应用）
  const handleForceApplyOffset = useCallback(() => {
    if (!offsetExceedDialog) return;
    const { offsetMs, fromIndex, toIndex } = offsetExceedDialog;
    const offsetSec = offsetMs / 1000;
    commitOffset(offsetMs, fromIndex, toIndex, offsetSec);
    setOffsetExceedDialog(null);
  }, [offsetExceedDialog, commitOffset]);

  // 完成新增字幕的编辑（时间 + 原文 + 译文）
  const handleInsertDone = useCallback(() => {
    if (insertEditingIndex == null) return;
    // 更新 store 中的时间、原文、译文
    store.updateEntry(insertEditingIndex, { start_ms: insertStartMs, end_ms: insertEndMs, text: insertText, translated: insertTranslated });
    // 保存最终状态（用于永久提示和恢复）
    setInsertDoneRows((prev) => new Map(prev).set(insertEditingIndex, {
      start_ms: insertStartMs,
      end_ms: insertEndMs,
      translated: insertTranslated,
    }));
    setInsertEditingIndex(null);
  }, [insertEditingIndex, insertStartMs, insertEndMs, insertText, insertTranslated, store]);

  // 取消新增字幕的时间编辑
  // 从未保存过的新增字幕 → 彻底删除；已保存过的 → 仅关闭面板
  const handleInsertCancel = useCallback(() => {
    if (insertEditingIndex == null) return;
    if (!insertDoneRows.has(insertEditingIndex)) {
      // 从未保存过，彻底删除
      store.removeEntry(insertEditingIndex);
    }
    setInsertEditingIndex(null);
  }, [insertEditingIndex, insertDoneRows, store]);

  // 重新打开已完成编辑的新增字幕（恢复上次状态）
  const reopenInsertEdit = useCallback((entryIndex: number) => {
    const saved = insertDoneRows.get(entryIndex);
    if (!saved) return;
    const entry = file?.entries.find((e) => e.index === entryIndex);
    setInsertEditingIndex(entryIndex);
    setInsertStartMs(saved.start_ms);
    setInsertEndMs(saved.end_ms);
    setInsertText(entry?.text ?? "");
    setInsertTranslated(saved.translated);
  }, [insertDoneRows, file]);

  // 重置到初始状态
  const handleResetClick = useCallback(() => {
    const steps = store.undoStack.length;
    if (steps === 0) {
      toast.info(t("subtitle.resetNoChanges", "没有需要重置的修改"));
      return;
    }
    setResetSteps(steps);
    setResetDialogOpen(true);
  }, [store, t]);

  const handleResetConfirm = useCallback(() => {
    const steps = store.resetToInitial();
    // 清理所有行内编辑状态
    setOffsetRowIndex(null);
    setOffsetAppliedRows(new Map());
    setOffsetLastInput(new Map());
    setInsertEditingIndex(null);
    setInsertDoneRows(new Map());
    setEditingIndex(null);
    setShowFindReplace(false);
    setResetDialogOpen(false);
    toast.success(t("subtitle.resetDone", "已重置为初始状态"));
  }, [store, t]);

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
    // 已有翻译时跳过缓存，强制重新请求 API
    const skipCache = !!entry.translated;
    try {
      const result = await translateStore.startTranslate(
        [entry],
        (index, translated, failed) => {
          // 单条翻译完成，立即更新（含翻译失败标记）
          store.updateEntry(index, { translated, failed });
        },
        skipCache,
        undefined,
        undefined,
        store.file?.file_hash || undefined,
      );
    } catch (e: any) {
      error("翻译单条失败:", e);
      toast.error(typeof e === "string" ? e : (e?.message || "翻译单条失败"));
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
      error("跳转播放失败:", e);
      toast.error(t("subtitle.playFromHereFailed"));
    }
  }, [file, closeContextMenu, t]);

  // 点击外部关闭右键菜单
  useEffect(() => {
    if (!contextMenu) return;
    const handleClick = () => closeContextMenu();
    const handleEsc = (e: KeyboardEvent) => { if (e.key === "Escape") closeContextMenu(); };
    // 右键时也关闭旧菜单（新右键事件会触发 handleContextMenu 打开新菜单或 stopPropagation）
    const handleCtx = () => closeContextMenu();
    window.addEventListener("click", handleClick);
    window.addEventListener("keydown", handleEsc);
    window.addEventListener("contextmenu", handleCtx, true);
    return () => {
      window.removeEventListener("click", handleClick);
      window.removeEventListener("keydown", handleEsc);
      window.removeEventListener("contextmenu", handleCtx, true);
    };
  }, [contextMenu, closeContextMenu]);

  // 查找匹配时滚动到对应条目
  useEffect(() => {
    if (store.findMatchEntryIndex == null) return;
    const entryIdx = file?.entries.findIndex((e) => e.index === store.findMatchEntryIndex);
    if (entryIdx == null || entryIdx < 0) return;
    // 虚拟滚动 scrollToIndex
    rowVirtualizer.scrollToIndex(entryIdx, { align: "center" });
  }, [store.findMatchEntryIndex]); // eslint-disable-line react-hooks/exhaustive-deps

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

  if (extracting) {
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
    <div className="flex h-full flex-col overflow-hidden">
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
        <Button size="sm" variant="ghost" onClick={handleResetClick} disabled={store.undoStack.length === 0} className="h-7 px-2" title={t("subtitle.reset", "重置")}>
          <RotateCcw className="h-3.5 w-3.5" />
        </Button>
        {/* 开发者模式：翻译状态统计图标 */}
        {devMode && file && (() => {
          const total = file.entries.length;
          const targetLang = useTranslateStore.getState().targetLang;
          // CJK 字符检测：目标语言是中文时，译文应包含 CJK 字符
          const hasCjk = (s: string) => /[一-鿿]/.test(s);
          // "未翻译"判定：译文为空、译文=原文、目标语言是中文但译文无 CJK（且原文也无 CJK）、
          // 或音效标记类型不一致（如 "you need every week." → "[碰撞声持续]"，AI 错位翻译）
          const isUntranslated = (e: typeof file.entries[0]) =>
            !e.translated
            || e.translated.trim() === e.text.trim()
            || (targetLang.startsWith("zh") && !hasCjk(e.translated) && !hasCjk(e.text))
            || looksLikeSoundEffect(e.text) !== looksLikeSoundEffect(e.translated);
          const translatedCount = file.entries.filter((e) => e.translated && !e.failed && !isUntranslated(e)).length;
          const cacheCount = file.entries.filter((e) => e.from_cache).length;
          const failedCount = file.entries.filter((e) => e.failed).length;
          const missingCount = file.entries.filter((e) => isUntranslated(e)).length;
          const hasIssues = failedCount > 0 || missingCount > 0;
          const tooltip = `共 ${total} 条 | 已翻译 ${translatedCount} 条（缓存 ${cacheCount}）| 失败 ${failedCount} 条 | 未翻译 ${missingCount} 条`;
          if (!hasIssues) {
            // 全部翻译成功，显示绿色成功图标
            return (
              <Button
                size="sm"
                variant="ghost"
                className="h-7 px-2 text-xs text-green-600 hover:text-green-700"
                title={tooltip}
              >
                <Check className="mr-1 h-3.5 w-3.5" />
                {total}
              </Button>
            );
          }
          // 收集所有问题条目的索引（用于循环跳转）
          const issueEntries = file.entries.filter((e) => e.failed || isUntranslated(e));
          const jumpToNextIssue = () => {
            if (issueEntries.length === 0) return;
            // 找当前编辑条目在 issueEntries 中的位置，跳到下一个
            const currentIdx = issueEntries.findIndex((e) => e.index === editingIndex);
            const nextIdx = (currentIdx + 1) % issueEntries.length;
            const target = issueEntries[nextIdx];
            const entryIdx = file.entries.findIndex((e) => e.index === target.index);
            if (entryIdx >= 0) {
              // 设置自动滚动锁，防止 onScroll → maybeScrollToActive 把滚动拉回当前播放位置
              isAutoScrollingRef.current = true;
              rowVirtualizer.scrollToIndex(entryIdx, { align: "center" });
              setEditingIndex(target.index);
              // 释放锁：给滚动动画 + onScroll debounce 足够时间
              window.setTimeout(() => {
                isAutoScrollingRef.current = false;
              }, 1200);
            }
          };
          return (
            <Button
              size="sm"
              variant="ghost"
              className="h-7 px-2 text-xs text-orange-500 hover:text-orange-600"
              title={tooltip}
              onClick={jumpToNextIssue}
            >
              <AlertTriangle className="mr-1 h-3.5 w-3.5" />
              {failedCount}/{missingCount}
            </Button>
          );
        })()}
        {/* 开发者模式：导出源语言/翻译后字幕为 txt */}
        {devMode && file && (
          <>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 w-7 p-0"
              title={t("subtitle.exportSourceTxt", "导出源语言字幕（txt）")}
              onClick={async () => {
                try {
                  const outputPath = await withPlayerHidden(() => save({
                    defaultPath: "source.txt",
                    filters: [{ name: "Text", extensions: ["txt"] }],
                  }));
                  if (!outputPath) return;
                  const lines = file.entries
                    .filter((e) => !e._deleted)
                    .map((e) => {
                      const start = formatTimecode(e.start_ms);
                      const end = formatTimecode(e.end_ms);
                      return `[${start} --> ${end}] ${e.text}`;
                    });
                  await writeTextFile(outputPath, lines.join("\n"));
                  toast.success(t("subtitle.exportSourceTxtOk", "已导出源语言字幕"));
                } catch (e) {
                  toast.error(t("subtitle.exportFailed", "导出失败") + ": " + String(e));
                }
              }}
            >
              <FileText className="h-3.5 w-3.5" />
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 w-7 p-0"
              title={t("subtitle.exportTranslatedTxt", "导出翻译后字幕（txt）")}
              onClick={async () => {
                try {
                  const outputPath = await withPlayerHidden(() => save({
                    defaultPath: "translated.txt",
                    filters: [{ name: "Text", extensions: ["txt"] }],
                  }));
                  if (!outputPath) return;
                  const lines = file.entries
                    .filter((e) => !e._deleted)
                    .map((e) => {
                      const start = formatTimecode(e.start_ms);
                      const end = formatTimecode(e.end_ms);
                      return `[${start} --> ${end}] ${e.translated || e.text}`;
                    });
                  await writeTextFile(outputPath, lines.join("\n"));
                  toast.success(t("subtitle.exportTranslatedTxtOk", "已导出翻译后字幕"));
                } catch (e) {
                  toast.error(t("subtitle.exportFailed", "导出失败") + ": " + String(e));
                }
              }}
            >
              <Languages className="h-3.5 w-3.5" />
            </Button>
          </>
        )}
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
        <div
          className="flex items-center gap-2 border-b px-3 py-1.5 bg-muted/30 flex-shrink-0 flex-wrap"
          onMouseEnter={() => { isMouseOverPanelRef.current = true; uiState.mouseInSubtitleEditor = true; }}
          onMouseLeave={() => { isMouseOverPanelRef.current = false; uiState.mouseInSubtitleEditor = false; }}
        >
          <Input
            placeholder={t("subtitle.find", "查找")}
            value={store.findQuery}
            onChange={(e) => store.setFindQuery(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") store.findNext(); }}
            className="h-7 w-28 text-xs"
          />
          <Input
            placeholder={t("subtitle.replace", "替换")}
            value={store.replaceQuery}
            onChange={(e) => store.setReplaceQuery(e.target.value)}
            className="h-7 w-28 text-xs"
          />
          {/* 查找目标选择 */}
          <select
            value={store.findTarget}
            onChange={(e) => store.setFindTarget(e.target.value as "all" | "translated" | "original")}
            className="h-7 rounded border border-input bg-transparent px-1 text-xs"
          >
            <option value="all">{t("subtitle.findTargetAll", "全部")}</option>
            <option value="translated">{t("subtitle.findTargetTranslated", "译文")}</option>
            <option value="original">{t("subtitle.findTargetOriginal", "原文")}</option>
          </select>
          {/* 查找按钮 */}
          <Button size="sm" variant="secondary" className="h-7 text-xs" onClick={() => store.findNext()}>
            {t("subtitle.findBtn", "查找")}
          </Button>
          {/* 替换按钮 */}
          <Button size="sm" variant="secondary" className="h-7 text-xs" onClick={() => store.replaceCurrent()} disabled={store.findMatchEntryIndex == null}>
            {t("subtitle.replaceOne", "替换")}
          </Button>
          {/* 全部替换 */}
          <Button size="sm" variant="secondary" className="h-7 text-xs" onClick={() => {
            const n = store.replaceAll();
            toast.success(t("subtitle.replacedCount", "已替换 {{count}} 条", { count: n }));
          }}>
            {t("subtitle.replaceAll", "全部替换")}
          </Button>
          {/* 匹配计数 */}
          {store.findMatchCount > 0 && (
            <span className="text-xs text-muted-foreground tabular-nums">
              {store.findCurrentMatch + 1}/{store.findMatchCount}
            </span>
          )}
          {/* 上一个/下一个 */}
          {store.findMatchCount > 1 && (
            <div className="flex gap-0.5">
              <Button size="sm" variant="ghost" className="h-7 w-7 p-0" onClick={() => store.findPrev()} title={t("subtitle.findPrev", "上一个")}>
                <ChevronUp className="h-3.5 w-3.5" />
              </Button>
              <Button size="sm" variant="ghost" className="h-7 w-7 p-0" onClick={() => store.findNext()} title={t("subtitle.findNext", "下一个")}>
                <ChevronDown className="h-3.5 w-3.5" />
              </Button>
            </div>
          )}
          <Button size="sm" variant="ghost" className="h-7 w-7 p-0" onClick={() => setShowFindReplace(false)}>
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      )}

      {/* 字幕对比预览列表（虚拟滚动） */}
      <div
        ref={parentRef}
        className="flex-1 min-h-0 overflow-auto"
        onMouseEnter={() => { isMouseOverPanelRef.current = true; uiState.mouseInSubtitleEditor = true; }}
        onMouseLeave={() => { isMouseOverPanelRef.current = false; uiState.mouseInSubtitleEditor = false; }}
      >
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
            const isFindMatch = store.findMatchEntryIndex === entry.index;
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
                className={`group border-b px-3 py-1.5 hover:bg-accent/30 ${isActive ? "bg-primary/10 border-l-2 border-l-primary" : ""} ${isFindMatch ? "bg-yellow-200/50 border-l-2 border-l-yellow-500" : ""}`}
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
                    {/* 时间轴偏移（仅 hover 显示） */}
                    {!entry._deleted && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          if (offsetRowIndex === entry.index) closeOffsetPanel();
                          else openOffsetPanel(entry.index);
                        }}
                        className={`flex h-4 w-4 flex-shrink-0 items-center justify-center rounded text-muted-foreground/60 transition-opacity hover:bg-primary hover:text-primary-foreground ${offsetRowIndex === entry.index ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}
                        title={t("subtitle.timeOffset", "时间轴偏移")}
                      >
                        <Clock className="h-3 w-3" />
                      </button>
                    )}
                    {/* 在下方新增字幕（仅 hover 显示） */}
                    {!entry._deleted && (
                      <button
                        onClick={(e) => { e.stopPropagation(); handleInsertEntry(entry.index); }}
                        className="flex h-4 w-4 flex-shrink-0 items-center justify-center rounded text-muted-foreground/60 opacity-0 group-hover:opacity-100 transition-opacity hover:bg-primary hover:text-primary-foreground"
                        title={t("subtitle.insertBelow", "在下方新增字幕")}
                      >
                        <Plus className="h-3 w-3" />
                      </button>
                    )}
                  </div>
                  {isEditing ? (
                    /* 编辑态：完成、取消、删除按钮 */
                    <div className="flex gap-1 flex-shrink-0">
                      <Button
                        size="sm"
                        className="h-5 px-2 text-xs bg-green-600 hover:bg-green-700"
                        onClick={(e) => { e.stopPropagation(); commitEdit(); }}
                      >
                        <Check className="h-3 w-3 mr-0.5" />
                        {t("common.done", "完成")}
                      </Button>
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-5 px-2 text-xs"
                        onClick={(e) => { e.stopPropagation(); cancelEdit(entry.index); toast.warning(t("subtitle.editCancelled", "编辑已取消")); }}
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
                      onContextMenu={(e) => e.stopPropagation()}
                      onKeyDown={(e) => {
                        if (e.key === "Escape") {
                          e.preventDefault();
                          e.stopPropagation();
                          cancelEdit(entry.index);
                        }
                      }}
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
                        onClick={(e) => { e.stopPropagation(); beginEdit(entry.index, entry.translated); }}
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
                {/* 行内时间轴偏移面板：当前编辑行显示完整面板 */}
                {offsetRowIndex === entry.index && (
                  <div
                    className="mt-1.5 flex items-center gap-2 rounded bg-muted/40 px-2 py-1.5"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <span className="text-xs text-muted-foreground flex-shrink-0">偏移(秒)</span>
                    <Input
                      type="number"
                      value={offsetValue}
                      onChange={(e) => { setOffsetValue(e.target.value); setOffsetAppliedMsg(null); }}
                      placeholder="±5.0"
                      className="h-6 w-20 text-xs"
                      onKeyDown={(e) => { if (e.key === "Enter") handleApplyOffset(); }}
                    />
                    <span className="text-xs text-muted-foreground flex-shrink-0">至编号</span>
                    <Input
                      type="number"
                      value={offsetEndIndex}
                      onChange={(e) => { setOffsetEndIndex(e.target.value); setOffsetAppliedMsg(null); }}
                      className="h-6 w-16 text-xs"
                    />
                    <Button size="sm" className="h-6 text-xs" onClick={handleApplyOffset} disabled={!offsetValue}>
                      {t("subtitle.applyOffset", "应用偏移")}
                    </Button>
                    {offsetAppliedMsg && (
                      <span className={`text-xs ${offsetAppliedMsg.includes("裁剪") || offsetAppliedMsg.includes("早于") ? "text-orange-600" : "text-green-600"}`}>{offsetAppliedMsg}</span>
                    )}
                    <Button size="sm" variant="ghost" className="h-6 w-6 p-0 ml-auto" onClick={closeOffsetPanel}>
                      <X className="h-3 w-3" />
                    </Button>
                  </div>
                )}
                {/* 已偏移行（非当前编辑）的永久提示 */}
                {offsetRowIndex !== entry.index && offsetAppliedRows.has(entry.index) && (
                  <div
                    className="mt-1 flex items-center gap-1.5 text-xs"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <Clock className="h-3 w-3 text-muted-foreground" />
                    <span className={offsetAppliedRows.get(entry.index)?.includes("裁剪") || offsetAppliedRows.get(entry.index)?.includes("早于") ? "text-orange-600" : "text-green-600"}>
                      {offsetAppliedRows.get(entry.index)}
                    </span>
                  </div>
                )}
                {/* 新增字幕：时间编辑面板（当前正在编辑的行） */}
                {insertEditingIndex === entry.index && (() => {
                  // 计算滑块约束范围
                  const curIdx = file.entries.findIndex((e) => e.index === entry.index);
                  // 上一条非删除条目（用于 start 滑块下限）
                  let prevEntry: SubtitleEntry | null = null;
                  for (let i = curIdx - 1; i >= 0; i--) {
                    if (!file.entries[i]._deleted) { prevEntry = file.entries[i]; break; }
                  }
                  // 下一条非删除条目（用于 end 滑块上限）
                  let nextEntry: SubtitleEntry | null = null;
                  for (let i = curIdx + 1; i < file.entries.length; i++) {
                    if (!file.entries[i]._deleted) { nextEntry = file.entries[i]; break; }
                  }
                  const startMin = prevEntry ? prevEntry.start_ms : 0;
                  const startMax = insertEndMs; // 不能晚于自己的结束时间
                  const endMin = insertStartMs; // 不能早于自己的开始时间
                  const endMax = nextEntry ? nextEntry.end_ms : (videoStore.probeResult?.format?.duration ? videoStore.probeResult.format.duration * 1000 : insertStartMs + 10000);
                  return (
                    <div
                      className="mt-1.5 rounded bg-blue-50/50 px-2 py-1.5 space-y-1.5"
                      onClick={(e) => e.stopPropagation()}
                    >
                      {/* 开始时间滑块 */}
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-muted-foreground flex-shrink-0 w-16">开始时间</span>
                        <input
                          type="range"
                          min={startMin}
                          max={startMax}
                          step={10}
                          value={insertStartMs}
                          onChange={(e) => {
                            const v = Math.min(Number(e.target.value), insertEndMs);
                            setInsertStartMs(v);
                          }}
                          className="flex-1 h-1.5"
                        />
                        <span className="font-mono text-xs w-24 text-right">{formatTimecode(insertStartMs)}</span>
                      </div>
                      {/* 结束时间滑块 */}
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-muted-foreground flex-shrink-0 w-16">结束时间</span>
                        <input
                          type="range"
                          min={endMin}
                          max={endMax}
                          step={10}
                          value={insertEndMs}
                          onChange={(e) => {
                            const v = Math.max(Number(e.target.value), insertStartMs);
                            setInsertEndMs(v);
                          }}
                          className="flex-1 h-1.5"
                        />
                        <span className="font-mono text-xs w-24 text-right">{formatTimecode(insertEndMs)}</span>
                      </div>
                      {/* 操作按钮 */}
                      <div className="flex items-center gap-2">
                        <Button size="sm" className="h-6 text-xs bg-green-600 hover:bg-green-700" onClick={handleInsertDone}>
                          <Check className="h-3 w-3 mr-0.5" />
                          {t("common.done", "完成")}
                        </Button>
                        <Button size="sm" variant="outline" className="h-6 text-xs" onClick={handleInsertCancel}>
                          {t("common.cancel", "取消")}
                        </Button>
                      </div>
                      {/* 原文编辑 */}
                      <div className="flex items-start gap-2">
                        <span className="text-xs text-muted-foreground flex-shrink-0 w-16 pt-1">原文</span>
                        <AutoTextarea
                          value={insertText}
                          onChange={setInsertText}
                          className="text-xs py-1 flex-1 resize-none"
                          placeholder={t("subtitle.original", "原文")}
                          onClick={(e) => e.stopPropagation()}
                          onContextMenu={(e) => e.stopPropagation()}
                        />
                      </div>
                      {/* 译文编辑 */}
                      <div className="flex items-start gap-2">
                        <span className="text-xs text-muted-foreground flex-shrink-0 w-16 pt-1">译文</span>
                        <AutoTextarea
                          value={insertTranslated}
                          onChange={setInsertTranslated}
                          className="text-xs py-1 flex-1 resize-none"
                          placeholder={t("subtitle.translated", "译文")}
                          onClick={(e) => e.stopPropagation()}
                          onContextMenu={(e) => e.stopPropagation()}
                        />
                      </div>
                    </div>
                  );
                })()}
                {/* 新增字幕：已完成编辑的永久提示（非当前编辑行） */}
                {insertEditingIndex !== entry.index && insertDoneRows.has(entry.index) && (() => {
                  const saved = insertDoneRows.get(entry.index)!;
                  return (
                    <div
                      className="mt-1 flex items-center gap-1.5 text-xs cursor-pointer hover:bg-blue-100/40 rounded px-1 -mx-1"
                      onClick={(e) => { e.stopPropagation(); reopenInsertEdit(entry.index); }}
                    >
                      <Plus className="h-3 w-3 text-blue-500" />
                      <span className="text-green-600">
                        已新增 · {formatTimecode(saved.start_ms)} → {formatTimecode(saved.end_ms)}
                      </span>
                      <span className="text-muted-foreground/50">点击重新编辑</span>
                    </div>
                  );
                })()}
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
            {file.entries.find((e) => e.index === contextMenu.entryIndex)?.translated
              ? t("subtitle.retranslateOne", "重新翻译字幕")
              : t("subtitle.translateOne", "翻译此条字幕")}
          </button>
          <button
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent"
            onClick={() => {
              const e = file?.entries.find((x) => x.index === contextMenu.entryIndex);
              beginEdit(contextMenu.entryIndex, e?.translated ?? "");
              closeContextMenu();
            }}
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

      {/* 时间轴偏移超出视频时长确认弹窗 */}
      {offsetExceedDialog && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40">
          <div className="rounded-lg border bg-popover p-5 shadow-lg max-w-sm">
            <p className="text-sm font-medium mb-2">{t("subtitle.offsetExceedTitle", "字幕超出视频时长")}</p>
            <p className="text-xs text-muted-foreground mb-4">
              {t("subtitle.offsetExceedMsg", "{{count}} 条字幕的结束时间超出视频时长 {{seconds}} 秒，是否仍然应用？", { count: offsetExceedDialog.count, seconds: offsetExceedDialog.maxExceedSec.toFixed(1) })}
            </p>
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="outline" onClick={() => {
                // 取消：不应用偏移，直接关闭弹窗
                setOffsetExceedDialog(null);
              }}>
                {t("common.cancel", "取消")}
              </Button>
              <Button size="sm" onClick={handleForceApplyOffset}>
                {t("subtitle.forceApply", "仍然应用")}
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* 重置确认弹窗 */}
      {resetDialogOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40">
          <div className="rounded-lg border bg-popover p-5 shadow-lg max-w-sm">
            <p className="text-sm font-medium mb-2">{t("subtitle.resetTitle", "重置字幕")}</p>
            <p className="text-xs text-muted-foreground mb-4">
              {t("subtitle.resetConfirm", "已执行的 {{count}} 步操作将被撤销，字幕将恢复为初始加载状态。确定要重置吗？", { count: resetSteps })}
            </p>
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="outline" onClick={() => setResetDialogOpen(false)}>
                {t("common.cancel", "取消")}
              </Button>
              <Button size="sm" variant="destructive" onClick={handleResetConfirm}>
                {t("subtitle.resetConfirmBtn", "确认重置")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// === SECTION 2 END ===
