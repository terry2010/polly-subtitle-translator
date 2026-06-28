import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Search, Download, Loader2, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog";
import { ScrollArea } from "./ui/scroll-area";
import { api, formatIpcError } from "../lib/api";
import { save } from "@tauri-apps/plugin-dialog";
import type { SubtitleSearchResult } from "../lib/ipc-types";
import { withPlayerHidden } from "../lib/utils";

interface SearchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  videoName?: string;
}

export function SearchDialog({ open, onOpenChange, videoName }: SearchDialogProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState(videoName ?? "");
  const [language, setLanguage] = useState("en");
  const [results, setResults] = useState<SubtitleSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handleSearch = useCallback(async () => {
    if (!query.trim()) return;
    setSearching(true);
    setError(null);
    try {
      const apiKey = await api.getCredential("opensubtitles", "api_key");
      if (!apiKey) {
        setError(t("search.notConfigured"));
        return;
      }
      const r = await api.searchSubtitlesOnline(query, language, apiKey);
      setResults(r);
      if (r.length === 0) {
        setError(t("search.noResults"));
      }
    } catch (e) {
      setError(formatIpcError(e as any));
    } finally {
      setSearching(false);
    }
  }, [query, language, t]);

  const handleDownload = useCallback(async (result: SubtitleSearchResult) => {
    setDownloadingId(result.subtitle_id);
    try {
      const apiKey = await api.getCredential("opensubtitles", "api_key");
      if (!apiKey) {
        setError(t("search.notConfigured"));
        return;
      }
      const outputPath = await withPlayerHidden(() => save({
        defaultPath: `${result.file_name}`,
        filters: [{ name: "Subtitle", extensions: ["srt"] }],
      }));
      if (outputPath) {
        await api.downloadSubtitleOnline(result.subtitle_id, apiKey, outputPath);
      }
    } catch (e) {
      setError(formatIpcError(e as any));
    } finally {
      setDownloadingId(null);
    }
  }, [t]);

  // === SECTION 1 END ===

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh]">
        <DialogHeader>
          <DialogTitle>{t("search.title")}</DialogTitle>
        </DialogHeader>

        {/* 搜索栏 */}
        <div className="flex items-center gap-2">
          <Input
            placeholder={t("search.keyword")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSearch()}
            className="flex-1"
          />
          <select
            value={language}
            onChange={(e) => setLanguage(e.target.value)}
            className="h-9 rounded-md border border-input bg-transparent px-2 text-sm"
          >
            <option value="en">English</option>
            <option value="zh-CN">中文</option>
            <option value="ja">日本語</option>
            <option value="ko">한국어</option>
            <option value="fr">Français</option>
            <option value="de">Deutsch</option>
            <option value="es">Español</option>
            <option value="ru">Русский</option>
          </select>
          <Button size="sm" onClick={handleSearch} disabled={searching}>
            {searching ? <Loader2 className="h-4 w-4 animate-spin" /> : <Search className="h-4 w-4" />}
          </Button>
        </div>

        {/* 错误提示 */}
        {error && (
          <div className="flex items-center gap-2 rounded bg-destructive/10 p-2 text-sm text-destructive">
            <span className="flex-1">{error}</span>
            <Button size="sm" variant="ghost" onClick={() => setError(null)}>
              <X className="h-3 w-3" />
            </Button>
          </div>
        )}

        {/* 搜索结果 */}
        <ScrollArea className="h-[400px] rounded-md border">
          <div className="p-2 space-y-1">
            {results.map((r) => (
              <div
                key={r.subtitle_id}
                className="flex items-center justify-between rounded px-3 py-2 hover:bg-accent/30"
              >
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium truncate">{r.file_name}</p>
                  <p className="text-xs text-muted-foreground">
                    {r.language} · ↓{r.download_count} · ★{r.rating.toFixed(1)}
                    {r.release_info && ` · ${r.release_info}`}
                  </p>
                </div>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => handleDownload(r)}
                  disabled={downloadingId === r.subtitle_id}
                >
                  {downloadingId === r.subtitle_id ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Download className="h-4 w-4" />
                  )}
                </Button>
              </div>
            ))}
            {results.length === 0 && !searching && !error && (
              <div className="text-center py-8 text-sm text-muted-foreground">
                {t("search.noResults")}
              </div>
            )}
          </div>
        </ScrollArea>
      </DialogContent>
    </Dialog>
  );
}

// === SECTION 2 END ===
