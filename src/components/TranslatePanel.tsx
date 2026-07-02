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
import { SERVICES, encodeAiSelectValue, decodeAiSelectValue } from "../lib/services";

export function TranslatePanel() {
  const { t } = useTranslation();
  const subtitleStore = useSubtitleStore();
  const translateStore = useTranslateStore();
  const [langs, setLangs] = useState<LanguageInfo[]>([]);
  // OpenAi：已选模型列表（含 per-model modelType）
  const [openaiModels, setOpenaiModels] = useState<{ id: string; modelType: string }[]>([]);
  // 已配置的引擎列表（动态从 db 加载）
  const [configuredEngines, setConfiguredEngines] = useState<{ value: string; label: string }[]>([]);

  // 初始化：从 db 加载保存的 provider + 已配置的引擎列表，然后自动选择
  useEffect(() => {
    let cancelled = false;
    (async () => {
      // 1. 加载保存的 provider + serviceId + model
      const [savedProvider, savedServiceId, savedModel] = await Promise.all([
        api.getConfig("translate_provider").catch(() => null),
        api.getConfig("translate_openai_service_id").catch(() => null),
        api.getConfig("translate_current_model").catch(() => null),
      ]);

      // 2. 加载已配置的引擎列表
      const engines: { value: string; label: string }[] = [];
      // 传统翻译：检查每个已实现服务
      const traditional = SERVICES.filter((s) => s.category === "traditional" && !s.comingSoon);
      await Promise.all(traditional.map(async (s) => {
        const appId = await api.getConfig(`translate_${s.id}_app_id`).catch(() => null);
        if (appId) {
          engines.push({ value: s.id, label: s.name });
        }
      }));
      // AI 大模型：检查每个 AI 服务的 baseUrl + selected_models
      const ai = SERVICES.filter((s) => s.category === "ai");
      await Promise.all(ai.map(async (s) => {
        const [baseUrl, selectedModels] = await Promise.all([
          api.getConfig(`translate_openai_${s.id}_base_url`).catch(() => null),
          api.getConfig(`translate_openai_${s.id}_selected_models`).catch(() => null),
        ]);
        if (baseUrl && selectedModels) {
          // 为每个模型创建一个引擎选项
          const models = selectedModels.split(",").filter(Boolean);
          for (const m of models) {
            engines.push({
              value: encodeAiSelectValue(s.id, m),
              label: `${s.name} - ${m}`,
            });
          }
        }
      }));

      if (cancelled) return;

      setConfiguredEngines(engines);

      // 3. 确定当前选中的引擎值
      let currentVal: string;
      if (savedProvider === "openai" && savedServiceId && savedModel) {
        currentVal = encodeAiSelectValue(savedServiceId, savedModel);
      } else if (savedProvider) {
        currentVal = savedProvider;
      } else {
        currentVal = translateStore.provider; // 默认 "baidu"
      }

      // 4. 如果当前选中的引擎不在列表中，自动选中第一个
      const found = engines.find((e) => e.value === currentVal);
      if (found) {
        // 恢复保存的选择
        const decoded = decodeAiSelectValue(found.value);
        if (decoded) {
          translateStore.setProvider("openai");
          translateStore.setServiceId(decoded.serviceId);
          translateStore.setModel(decoded.model);
        } else {
          translateStore.setProvider(found.value);
          translateStore.setServiceId(null);
          translateStore.setModel("");
        }
      } else if (engines.length > 0) {
        // 自动选中第一个已配置的引擎
        const first = engines[0];
        const decoded = decodeAiSelectValue(first.value);
        if (decoded) {
          translateStore.setProvider("openai");
          translateStore.setServiceId(decoded.serviceId);
          translateStore.setModel(decoded.model);
          // 持久化自动选择
          api.setConfig("translate_provider", "openai").catch(() => {});
          api.setConfig("translate_openai_service_id", decoded.serviceId).catch(() => {});
          api.setConfig("translate_current_model", decoded.model).catch(() => {});
        } else {
          translateStore.setProvider(first.value);
          translateStore.setServiceId(null);
          translateStore.setModel("");
          api.setConfig("translate_provider", first.value).catch(() => {});
          api.setConfig("translate_openai_service_id", "").catch(() => {});
          api.setConfig("translate_current_model", "").catch(() => {});
        }
      }
    })();
    return () => { cancelled = true; };
  }, []);

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

  // OpenAi：加载勾选的模型列表 + per-model modelType + 默认模型
  // 不依赖 translateStore.provider，因为 provider 可能还没从 config 加载完
  // 使用 serviceId 对应的 per-service 配置 key
  useEffect(() => {
    // 优先用 store 中的 serviceId，否则尝试从 db 读取
    const sid = translateStore.serviceId;
    const sidPromise = sid
      ? Promise.resolve(sid)
      : api.getConfig("translate_openai_service_id").catch(() => null);
    sidPromise.then((resolvedSid) => {
      const sidKey = resolvedSid ? `translate_openai_${resolvedSid}_` : "translate_openai_";
      Promise.all([
        api.getConfig(`${sidKey}selected_models`).catch(() => null),
        api.getConfig(`${sidKey}selected_model_types`).catch(() => null),
        api.getConfig("translate_current_model").catch(() => null),
      ]).then(([savedSelected, savedModelTypes, savedDefault]) => {
        const ids = savedSelected ? savedSelected.split(",").filter(Boolean) : [];
        let typeMap: Record<string, string> = {};
        if (savedModelTypes) {
          try { typeMap = JSON.parse(savedModelTypes); } catch { /* ignore */ }
        }
        const models = ids.map((id) => ({
          id,
          modelType: typeMap[id] || "generic",
        }));
        setOpenaiModels(models);
        // 始终设置默认模型（不检查 provider，因为 provider 可能还没加载完）
        if (!translateStore.model && savedDefault) {
          translateStore.setModel(savedDefault);
          const mt = typeMap[savedDefault] || "generic";
          translateStore.setModelType(mt);
        } else if (translateStore.model) {
          const found = models.find((m) => m.id === translateStore.model);
          if (found) translateStore.setModelType(found.modelType);
        }
      });
    });
  }, []);

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
              value={
                translateStore.provider === "openai" && translateStore.serviceId && translateStore.model
                  ? encodeAiSelectValue(translateStore.serviceId, translateStore.model)
                  : translateStore.provider
              }
              onValueChange={(val) => {
                const decoded = decodeAiSelectValue(val);
                if (decoded) {
                  translateStore.setProvider("openai");
                  translateStore.setServiceId(decoded.serviceId);
                  translateStore.setModel(decoded.model);
                  // 持久化选择
                  api.setConfig("translate_provider", "openai").catch(() => {});
                  api.setConfig("translate_openai_service_id", decoded.serviceId).catch(() => {});
                  api.setConfig("translate_current_model", decoded.model).catch(() => {});
                  // 加载该服务的模型列表以获取 modelType
                  api.getConfig(`translate_openai_${decoded.serviceId}_selected_model_types`).then((types) => {
                    let typeMap: Record<string, string> = {};
                    if (types) { try { typeMap = JSON.parse(types); } catch { /* ignore */ } }
                    translateStore.setModelType(typeMap[decoded.model] || "generic");
                  }).catch(() => {});
                } else {
                  translateStore.setProvider(val);
                  translateStore.setServiceId(null);
                  translateStore.setModel("");
                  // 持久化选择
                  api.setConfig("translate_provider", val).catch(() => {});
                  api.setConfig("translate_openai_service_id", "").catch(() => {});
                  api.setConfig("translate_current_model", "").catch(() => {});
                }
              }}
            >
              <SelectTrigger className="mt-1 h-8 text-xs">
                <SelectValue placeholder={t("translate.configInSettings", "请在设置中配置")} />
              </SelectTrigger>
              <SelectContent>
                {configuredEngines.length === 0 ? (
                  <div className="px-2 py-1.5 text-xs text-muted-foreground">
                    {t("translate.configInSettings", "请在设置中配置")}
                  </div>
                ) : (
                  configuredEngines.map((e) => (
                    <SelectItem key={e.value} value={e.value}>{e.label}</SelectItem>
                  ))
                )}
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
