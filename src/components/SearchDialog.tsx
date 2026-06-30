import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Search, X } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog";
import { api, formatIpcError } from "../lib/api";
import { open as openUrl } from "@tauri-apps/plugin-shell";

interface SearchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  videoName?: string;
}

export function SearchDialog({ open, onOpenChange, videoName }: SearchDialogProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState(videoName ?? "");
  const [source, setSource] = useState<string>("opensubtitles");
  const [error, setError] = useState<string | null>(null);

  // libmpv 子窗口是原生 OS 窗口，z-order 高于 WebView2，会遮盖 Dialog。
  // 弹层打开时隐藏播放器子窗口，关闭时恢复。
  useEffect(() => {
    if (!open) return;
    api.playerHide().catch(() => { /* 播放器未初始化，忽略 */ });
    return () => {
      api.playerShow().catch(() => { /* 播放器未初始化，忽略 */ });
    };
  }, [open]);

  // 打开弹窗时自动简化关键词
  useEffect(() => {
    if (open && videoName) {
      api.simplifySearchKeyword(videoName).then((simplified) => {
        setQuery(simplified);
      }).catch(() => {
        setQuery(videoName);
      });
    }
  }, [open, videoName]);

  const handleSearch = useCallback(async () => {
    if (!query.trim()) return;
    // 所有源：直接跳转到网站搜索页
    const searchUrls: Record<string, string> = {
      opensubtitles: `https://www.opensubtitles.com/search?q=${encodeURIComponent(query.trim())}`,
      subhd: `https://subhd.tv/search/${encodeURIComponent(query.trim())}`,
      zimuku: `https://zimuku.org/search?q=${encodeURIComponent(query.trim())}`,
    };
    const searchUrl = searchUrls[source] ?? searchUrls.opensubtitles;
    try {
      await openUrl(searchUrl);
    } catch (e) {
      setError(formatIpcError(e as any));
    }
  }, [query, source]);

  // === SECTION 1 END ===

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh]">
        <DialogHeader>
          <DialogTitle>{t("search.title")}</DialogTitle>
        </DialogHeader>

        {/* 来源切换 - Tab 样式 */}
        <div className="flex items-center gap-2">
          {(["opensubtitles", "subhd", "zimuku"] as const).map((s) => (
            <button
              key={s}
              onClick={() => { setSource(s); setError(null); }}
              className={`cursor-pointer px-4 py-2 text-sm rounded-t-md transition-all border ${
                source === s
                  ? "bg-background text-primary font-semibold border-b-2 border-b-primary border-t border-l border-r border-border shadow-sm"
                  : "bg-muted/50 text-muted-foreground border-transparent hover:bg-muted hover:text-foreground"
              }`}
            >
              {t(`search.source.${s}`)}
            </button>
          ))}
        </div>

        {/* 搜索栏 */}
        <div className="flex items-center gap-2">
          <Input
            placeholder={t("search.keyword")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSearch()}
            className="flex-1"
          />
          <Button size="sm" onClick={handleSearch}>
            <Search className="h-4 w-4" />
          </Button>
        </div>

        {/* 错误提示 */}
        {error && (
          <div className="flex items-start gap-2 rounded bg-destructive/10 p-2 text-sm text-destructive">
            <span className="flex-1">{error}</span>
            <Button size="sm" variant="ghost" onClick={() => setError(null)}>
              <X className="h-3 w-3" />
            </Button>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

// === SECTION 2 END ===
