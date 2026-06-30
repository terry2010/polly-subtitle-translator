import { useRef, useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Save, Plus, Trash2, Undo2, Redo2, Search, Clock, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";
import { ScrollArea } from "./ui/scroll-area";
import { useSubtitleStore } from "../stores/subtitleStore";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { SubtitleEntry } from "../lib/ipc-types";
import { withPlayerHidden } from "../lib/utils";

function formatTimecode(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const millis = ms % 1000;
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  return `${h.toString().padStart(2, "0")}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")},${millis.toString().padStart(3, "0")}`;
}

export function SubtitleListPanel() {
  const { t } = useTranslation();
  const store = useSubtitleStore();
  const { file } = store;
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [showFindReplace, setShowFindReplace] = useState(false);
  const [offsetInput, setOffsetInput] = useState("");
  const [showOffset, setShowOffset] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);

  const rowVirtualizer = useVirtualizer({
    count: file?.entries.length ?? 0,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 80,
    overscan: 5,
  });

  const handleSave = useCallback(async () => {
    if (!file) return;
    const outputPath = await withPlayerHidden(() => save({
      filters: [
        { name: "SRT", extensions: ["srt"] },
        { name: "ASS", extensions: ["ass"] },
        { name: "VTT", extensions: ["vtt"] },
      ],
    }));
    if (outputPath) {
      await store.saveSubtitle(outputPath);
    }
  }, [file, store]);

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

  const handleDeleteEntry = useCallback((index: number) => {
    store.deleteEntry(index);
  }, [store]);

  const handleApplyOffset = useCallback(() => {
    const offset = parseInt(offsetInput, 10);
    if (!isNaN(offset)) {
      store.applyTimeOffset(offset);
      setShowOffset(false);
      setOffsetInput("");
    }
  }, [offsetInput, store]);

  // === SECTION 1 END ===

  if (!file) {
    return (
      <div className="flex flex-1 items-center justify-center text-muted-foreground">
        <div className="text-center">
          <p className="text-sm">{t("subtitle.edit")}</p>
          <p className="mt-1 text-xs opacity-60">{t("common.noData")}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* 工具栏 */}
      <div className="flex items-center gap-1 border-b px-2 py-1">
        <Button size="sm" variant="ghost" onClick={store.undo} disabled={store.undoStack.length === 0}>
          <Undo2 className="h-4 w-4" />
        </Button>
        <Button size="sm" variant="ghost" onClick={store.redo} disabled={store.redoStack.length === 0}>
          <Redo2 className="h-4 w-4" />
        </Button>
        <div className="w-px h-5 bg-border mx-1" />
        <Button size="sm" variant="ghost" onClick={() => setShowFindReplace(!showFindReplace)}>
          <Search className="h-4 w-4" />
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setShowOffset(!showOffset)}>
          <Clock className="h-4 w-4" />
        </Button>
        <div className="w-px h-5 bg-border mx-1" />
        <Button size="sm" variant="ghost" onClick={handleAddEntry}>
          <Plus className="h-4 w-4" />
        </Button>
        <div className="flex-1" />
        <Button size="sm" onClick={handleSave}>
          <Save className="mr-1 h-4 w-4" />
          {t("subtitle.save")}
        </Button>
      </div>

      {/* 查找替换 */}
      {showFindReplace && (
        <div className="flex items-center gap-2 border-b px-3 py-2 bg-muted/30">
          <Input
            placeholder={t("subtitle.findReplace")}
            value={store.findQuery}
            onChange={(e) => store.setFindQuery(e.target.value)}
            className="h-7 w-40"
          />
          <Input
            placeholder="→"
            value={store.replaceQuery}
            onChange={(e) => store.setReplaceQuery(e.target.value)}
            className="h-7 w-40"
          />
          <Button
            size="sm"
            variant="secondary"
            onClick={() => store.findReplace(store.findQuery, store.replaceQuery, false)}
          >
            Replace
          </Button>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => store.findReplace(store.findQuery, store.replaceQuery, true)}
          >
            All
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setShowFindReplace(false)}>
            <X className="h-4 w-4" />
          </Button>
        </div>
      )}

      {/* 时间轴偏移 */}
      {showOffset && (
        <div className="flex items-center gap-2 border-b px-3 py-2 bg-muted/30">
          <span className="text-xs">{t("subtitle.timeOffset")} (ms):</span>
          <Input
            type="number"
            value={offsetInput}
            onChange={(e) => setOffsetInput(e.target.value)}
            placeholder="±1000"
            className="h-7 w-24"
          />
          <Button size="sm" onClick={handleApplyOffset}>Apply</Button>
          <Button size="sm" variant="ghost" onClick={() => setShowOffset(false)}>
            <X className="h-4 w-4" />
          </Button>
        </div>
      )}

      {/* 虚拟滚动字幕列表 */}
      <div ref={parentRef} className="flex-1 overflow-auto">
        <div
          style={{
            height: `${rowVirtualizer.getTotalSize()}px`,
            width: "100%",
            position: "relative",
          }}
        >
          {rowVirtualizer.getVirtualItems().map((virtualRow) => {
            const entry = file.entries[virtualRow.index];
            return (
              <div
                key={entry.index}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
                className="border-b px-3 py-1 hover:bg-accent/30"
                onClick={() => setEditingIndex(entry.index)}
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-1.5 min-w-0">
                    {entry.failed && (
                      <span
                        className="shrink-0 inline-flex items-center rounded bg-destructive/15 px-1 py-0.5 text-[10px] font-medium text-destructive"
                        title={t("subtitle.translateFailed")}
                      >
                        {t("subtitle.translateFailed")}
                      </span>
                    )}
                    <span className="font-mono text-xs text-muted-foreground truncate">
                      {formatTimecode(entry.start_ms)} → {formatTimecode(entry.end_ms)}
                    </span>
                  </div>
                  <div className="flex gap-1">
                    {editingIndex === entry.index && (
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-5 w-5 p-0"
                        onClick={(e) => { e.stopPropagation(); handleDeleteEntry(entry.index); }}
                      >
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    )}
                  </div>
                </div>
                {editingIndex === entry.index ? (
                  <div className="mt-1 space-y-1">
                    <Textarea
                      value={entry.text}
                      onChange={(e) => store.updateEntry(entry.index, { text: e.target.value })}
                      className="min-h-[40px] text-xs"
                      placeholder={t("subtitle.original")}
                    />
                    <Textarea
                      value={entry.translated}
                      onChange={(e) => store.updateEntry(entry.index, { translated: e.target.value })}
                      className="min-h-[40px] text-xs"
                      placeholder={t("subtitle.translated")}
                    />
                  </div>
                ) : (
                  <div className="mt-0.5 space-y-0.5">
                    <p className="text-xs line-clamp-1">{entry.text || <span className="opacity-30">—</span>}</p>
                    {entry.translated && (
                      <p className="text-xs text-primary line-clamp-1">{entry.translated}</p>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// === SECTION 2 END ===
