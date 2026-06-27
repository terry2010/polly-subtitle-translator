import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Languages, Loader2, AlertCircle } from "lucide-react";
import { Button } from "./ui/button";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "./ui/select";
import { Card, CardHeader, CardTitle, CardContent } from "./ui/card";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useTranslateStore } from "../stores/translateStore";
import { api } from "../lib/api";
import type { LanguageInfo } from "../lib/ipc-types";

export function TranslatePanel() {
  const { t } = useTranslation();
  const subtitleStore = useSubtitleStore();
  const translateStore = useTranslateStore();
  const [langs, setLangs] = useState<LanguageInfo[]>([]);

  useEffect(() => {
    api.getSupportedTargetLangs(translateStore.provider).then(setLangs).catch(() => {
      setLangs([
        { code: "zh", name: "Chinese", native_name: "中文" },
        { code: "en", name: "English", native_name: "English" },
        { code: "ja", name: "Japanese", native_name: "日本語" },
        { code: "ko", name: "Korean", native_name: "한국어" },
      ]);
    });
  }, [translateStore.provider]);

  const handleTranslate = useCallback(async () => {
    if (!subtitleStore.file) return;
    const result = await translateStore.startTranslate(subtitleStore.file.entries);
    if (result) {
      // 将翻译结果回填到字幕
      const entries = subtitleStore.file.entries.map((e) => {
        const translated = result.translations.find((r) => r.index === e.index);
        return translated ? { ...e, translated: translated.translated } : e;
      });
      subtitleStore.setFile({ ...subtitleStore.file, entries });
    }
  }, [subtitleStore, translateStore]);

  const hasEntries = subtitleStore.file && subtitleStore.file.entries.length > 0;

  return (
    <div className="space-y-3">
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-1 text-sm">
            <Languages className="h-4 w-4" />
            {t("translate.title")}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          {/* 翻译引擎选择 */}
          <div>
            <label className="text-xs text-muted-foreground">{t("translate.provider")}</label>
            <Select
              value={translateStore.provider}
              onValueChange={translateStore.setProvider}
            >
              <SelectTrigger className="mt-1 h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="baidu">{t("settings.baidu")}</SelectItem>
                <SelectItem value="bing">{t("settings.bing")}</SelectItem>
                <SelectItem value="google">{t("settings.google")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* 源语言 */}
          <div>
            <label className="text-xs text-muted-foreground">{t("translate.sourceLang")}</label>
            <Select
              value={translateStore.sourceLang}
              onValueChange={translateStore.setSourceLang}
            >
              <SelectTrigger className="mt-1 h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="en">English</SelectItem>
                <SelectItem value="ja">日本語</SelectItem>
                <SelectItem value="ko">한국어</SelectItem>
                <SelectItem value="fr">Français</SelectItem>
                <SelectItem value="de">Deutsch</SelectItem>
                <SelectItem value="es">Español</SelectItem>
                <SelectItem value="ru">Русский</SelectItem>
                <SelectItem value="auto">Auto</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* 目标语言 */}
          <div>
            <label className="text-xs text-muted-foreground">{t("translate.targetLang")}</label>
            <Select
              value={translateStore.targetLang}
              onValueChange={translateStore.setTargetLang}
            >
              <SelectTrigger className="mt-1 h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {langs.map((l) => (
                  <SelectItem key={l.code} value={l.code}>
                    {l.native_name} ({l.code})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* 翻译按钮 */}
          <Button
            className="w-full"
            size="sm"
            onClick={handleTranslate}
            disabled={!hasEntries || translateStore.translating}
          >
            {translateStore.translating ? (
              <Loader2 className="mr-1 h-4 w-4 animate-spin" />
            ) : (
              <Languages className="mr-1 h-4 w-4" />
            )}
            {translateStore.translating ? t("translate.progress") : t("translate.start")}
          </Button>

          {!hasEntries && (
            <p className="text-xs text-muted-foreground">{t("translate.apiNotConfigured")}</p>
          )}

          {/* 错误提示 */}
          {translateStore.error && (
            <div className="flex items-start gap-1 rounded bg-destructive/10 p-2 text-xs text-destructive">
              <AlertCircle className="mt-0.5 h-3 w-3 flex-shrink-0" />
              <span>{translateStore.error}</span>
            </div>
          )}

          {/* 翻译结果统计 */}
          {translateStore.result && (
            <div className="text-xs text-muted-foreground">
              <p>✓ {translateStore.result.translations.length} {t("translate.title")}</p>
              <p>📦 {translateStore.result.cached_count} cache</p>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
