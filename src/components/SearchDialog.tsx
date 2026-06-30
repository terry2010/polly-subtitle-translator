import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { Search, Download, Loader2, X, ExternalLink } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog";
import { ScrollArea } from "./ui/scroll-area";
import { api, formatIpcError } from "../lib/api";
import { save } from "@tauri-apps/plugin-dialog";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import type { SubtitleSearchResult } from "../lib/ipc-types";
import { withPlayerHidden } from "../lib/utils";
import { useThemeStore } from "../stores/themeStore";

interface SearchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  videoName?: string;
}

export function SearchDialog({ open, onOpenChange, videoName }: SearchDialogProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const uiLang = useThemeStore((s) => s.language);
  const [query, setQuery] = useState(videoName ?? "");
  // 默认搜索语言跟随界面语言：zh → zh-CN，en → en
  const [language, setLanguage] = useState(uiLang === "zh" ? "zh-CN" : "en");

  // 界面语言变化时同步默认搜索语言
  useEffect(() => {
    setLanguage(uiLang === "zh" ? "zh-CN" : "en");
  }, [uiLang]);
  const [source, setSource] = useState<string>("opensubtitles");
  const [results, setResults] = useState<SubtitleSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorIsNetwork, setErrorIsNetwork] = useState(false);
  // 验证码状态
  const [captchaImage, setCaptchaImage] = useState<string | null>(null);
  const [captchaInput, setCaptchaInput] = useState("");
  const [captchaCookie, setCaptchaCookie] = useState("");

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
    setSearching(true);
    setError(null);
    setErrorIsNetwork(false);
    setCaptchaImage(null);
    setResults([]);
    try {
      // OpenSubtitles 需要 API Key，SubHD/zimuku 不需要
      let apiKey = "";
      if (source === "opensubtitles") {
        apiKey = await api.getCredential("opensubtitles", "api_key") ?? "";
        if (!apiKey) {
          setError(t("search.notConfigured"));
          return;
        }
      }
      const r = await api.searchSubtitlesOnline(query, language, apiKey, source);
      setResults(r);
      if (r.length === 0) {
        setError(t("search.noResults"));
      }
    } catch (e) {
      const err = e as any;
      console.error("[Search] 搜索失败:", { source, query, error: err, args: err?.args });
      // 验证码错误：显示验证码图片和输入框
      if (err?.code === "search.captchaRequired") {
        setCaptchaImage(err.args?.captchaImage as string ?? null);
        setCaptchaCookie(err.args?.sessionCookie as string ?? "");
        setCaptchaInput("");
      } else {
        // 网络错误时显示详细错误信息
        const detail = err?.args?.detail as string;
        setError(detail ? `${formatIpcError(err)} (${detail})` : formatIpcError(err));
        // 网络错误时标记，用于显示"去设置代理"链接
        setErrorIsNetwork(err?.code === "search.networkError");
      }
    } finally {
      setSearching(false);
    }
  }, [query, language, source, t]);

  // 提交验证码后继续搜索
  const handleCaptchaSubmit = useCallback(async () => {
    if (!captchaInput.trim() || !query.trim()) return;
    setSearching(true);
    setError(null);
    setCaptchaImage(null);
    try {
      const r = await api.searchSubtitlesWithCaptcha(query, source, captchaInput.trim(), captchaCookie);
      setResults(r);
      if (r.length === 0) {
        setError(t("search.noResults"));
      }
    } catch (e) {
      const err = e as any;
      // 验证码错误：可能验证码输入错误，再次显示
      if (err?.code === "search.captchaRequired") {
        setCaptchaImage(err.args?.captchaImage as string ?? null);
        setCaptchaCookie(err.args?.sessionCookie as string ?? "");
        setCaptchaInput("");
      } else {
        setError(formatIpcError(err));
        setErrorIsNetwork(err?.code === "search.networkError");
      }
    } finally {
      setSearching(false);
    }
  }, [query, source, captchaInput, captchaCookie, t]);

  const handleClickResult = useCallback(async (result: SubtitleSearchResult) => {
    // SubHD / zimuku：打开浏览器访问网站
    if (source === "subhd" || source === "zimuku") {
      // subtitle_id 格式: "subhd:https://..." 或 "zimuku:https://..."
      const url = result.subtitle_id.replace(/^(subhd|zimuku):/, "");
      if (url.startsWith("http")) {
        try {
          await openUrl(url);
        } catch (e) {
          setError(formatIpcError(e as any));
        }
      }
      return;
    }

    // OpenSubtitles：应用内下载
    setDownloadingId(result.subtitle_id);
    try {
      const apiKey = await api.getCredential("opensubtitles", "api_key") ?? "";
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
  }, [source, t]);

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
              onClick={() => { setSource(s); setResults([]); setError(null); setErrorIsNetwork(false); setCaptchaImage(null); }}
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
          {source === "opensubtitles" && (
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
          )}
          <Button size="sm" onClick={handleSearch} disabled={searching}>
            {searching ? <Loader2 className="h-4 w-4 animate-spin" /> : <Search className="h-4 w-4" />}
          </Button>
        </div>

        {/* 错误提示 */}
        {error && (
          <div className="flex items-start gap-2 rounded bg-destructive/10 p-2 text-sm text-destructive">
            <span className="flex-1">
              {error}
              {/* 网络错误时提示去设置代理 */}
              {errorIsNetwork && (
                <button
                  onClick={() => { onOpenChange(false); navigate("/settings?tab=advanced"); }}
                  className="cursor-pointer underline ml-1 hover:text-destructive/80"
                >
                  {t("search.goProxySettings")}
                </button>
              )}
            </span>
            <Button size="sm" variant="ghost" onClick={() => setError(null)}>
              <X className="h-3 w-3" />
            </Button>
          </div>
        )}

        {/* 验证码输入区 */}
        {captchaImage && (
          <div className="flex items-center gap-3 rounded border border-yellow-500/30 bg-yellow-500/5 p-3">
            <img
              src={captchaImage}
              alt="captcha"
              className="h-10 w-32 rounded border border-border bg-white"
            />
            <Input
              placeholder={t("search.captchaPlaceholder")}
              value={captchaInput}
              onChange={(e) => setCaptchaInput(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCaptchaSubmit()}
              className="flex-1"
              autoFocus
            />
            <Button size="sm" onClick={handleCaptchaSubmit} disabled={searching || !captchaInput.trim()}>
              {searching ? <Loader2 className="h-4 w-4 animate-spin" /> : t("search.captchaSubmit")}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setCaptchaImage(null)}>
              <X className="h-4 w-4" />
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
                  onClick={() => handleClickResult(r)}
                  disabled={downloadingId === r.subtitle_id}
                >
                  {downloadingId === r.subtitle_id ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : source === "subhd" || source === "zimuku" ? (
                    <ExternalLink className="h-4 w-4" />
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
