import { useState, useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useNavigate, useSearchParams } from "react-router-dom";
import { getCurrentWindow, LogicalSize, LogicalPosition } from "@tauri-apps/api/window";
import { setWindowSizeInitialized } from "./MainView";
import { ArrowLeft, Check, Loader2, Download, Trash2, ExternalLink, Settings as SettingsIcon, Languages, Film, Wrench, Info, RefreshCw, X, Star, Plus, Bug, Terminal, FolderOpen, FileText } from "lucide-react";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { SERVICES, ServiceDef, matchesSearch, getServiceById } from "../lib/services";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "../components/ui/dialog";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "../components/ui/select";
import { useThemeStore } from "../stores/themeStore";
import { useDevModeStore } from "../stores/devModeStore";
import { useLibmpvStore } from "../stores/libmpvStore";
import { useFfmpegStore } from "../stores/ffmpegStore";
import { useUpdateStore } from "../stores/updateStore";
import { api, formatIpcError } from "../lib/api";
import type { PromptFailLogEntry } from "../lib/ipc-types";
import { warn } from "../lib/logger";
import { cn } from "../lib/utils";

type SettingsTab = "general" | "translate" | "player" | "advanced" | "developer" | "about";

export default function SettingsView() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { theme, setTheme, language, setLanguage } = useThemeStore();

  // 设置页内容较多，进入时放大窗口（返回 MainView 时由其 useEffect 恢复）
  // 同时调整位置保持窗口中心点不变
  // 卸载时通知 MainView 当前是大窗口状态，使其能正确缩回
  useEffect(() => {
    const win = getCurrentWindow();
    const newW = 1280, newH = 800;
    (async () => {
      try {
        const scaleFactor = await win.scaleFactor();
        // setSize 设置的是 inner size（客户区），所以用 innerSize 比较
        const inner = await win.innerSize();
        const curW = inner.width / scaleFactor;
        const curH = inner.height / scaleFactor;
        // 尺寸已匹配则跳过，避免 setPosition 的亚像素舍入导致窗口闪烁
        if (Math.abs(curW - newW) < 1 && Math.abs(curH - newH) < 1) return;

        // 获取原窗口中心点（setSize 之前），用于保持窗口大致在原位置
        const pos = await win.outerPosition();
        const outer = await win.outerSize();
        let cx = pos.x + outer.width / 2;
        let cy = pos.y + outer.height / 2;

        // 目标窗口物理尺寸（inner → physical）
        let winPhysW = Math.round(newW * scaleFactor);
        let winPhysH = Math.round(newH * scaleFactor);
        let finalW = newW;
        let finalH = newH;

        // 用工作区（排除任务栏）约束窗口尺寸和位置
        try {
          const wa = await api.getWorkArea();
          // 如果目标窗口物理尺寸超过工作区，缩小窗口以适应
          if (winPhysW > wa.width) {
            winPhysW = wa.width;
            finalW = Math.floor(wa.width / scaleFactor);
          }
          if (winPhysH > wa.height) {
            winPhysH = wa.height;
            finalH = Math.floor(wa.height / scaleFactor);
          }
          // 约束中心点在工作区内
          cx = Math.min(Math.max(cx, wa.x + winPhysW / 2), wa.x + wa.width - winPhysW / 2);
          cy = Math.min(Math.max(cy, wa.y + winPhysH / 2), wa.y + wa.height - winPhysH / 2);
        } catch {
          cx = Math.max(cx, winPhysW / 2);
          cy = Math.max(cy, winPhysH / 2);
        }

        const newX = Math.round(cx - winPhysW / 2);
        const newY = Math.round(cy - winPhysH / 2);
        // 先 setPosition 再 setSize：先移动到目标位置（保持旧尺寸），再设置新尺寸
        await win.setPosition(new LogicalPosition(newX / scaleFactor, newY / scaleFactor));
        await win.setSize(new LogicalSize(finalW, finalH));
      } catch {
        win.setSize(new LogicalSize(newW, newH)).catch(() => {});
      }
    })();

    // 卸载时标记当前为大窗口，使 MainView 重新挂载时能检测到状态变化并缩回
    return () => {
      setWindowSizeInitialized(true);
    };
  }, []);

  const [activeTab, setActiveTab] = useState<SettingsTab>(
    searchParams.get("provider") ? "translate"
    : searchParams.get("tab") === "translate" ? "translate"
    : searchParams.get("tab") === "advanced" ? "advanced"
    : "general"
  );
  const [apiListContainer, setApiListContainer] = useState<HTMLDivElement | null>(null);
  const devMode = useDevModeStore((s) => s.devMode);

  const navItems: { key: SettingsTab; label: string; icon: React.ReactNode }[] = [
    { key: "general", label: t("settings.general"), icon: <SettingsIcon className="h-4 w-4" /> },
    { key: "translate", label: t("settings.translateApi"), icon: <Languages className="h-4 w-4" /> },
    { key: "player", label: t("settings.player"), icon: <Film className="h-4 w-4" /> },
    { key: "advanced", label: t("settings.advanced"), icon: <Wrench className="h-4 w-4" /> },
    ...(devMode ? [{ key: "developer" as SettingsTab, label: t("settings.developer", "开发者选项"), icon: <Bug className="h-4 w-4" /> }] : []),
    { key: "about", label: t("settings.about"), icon: <Info className="h-4 w-4" /> },
  ];

  return (
    <div className="flex h-screen flex-col">
      <div className="flex flex-1 overflow-y-hidden overflow-x-auto relative min-w-[1280px]">
        {/* 左侧导航 */}
        <nav className="w-48 border-r bg-muted/30 p-2 space-y-1">
          {/* 返回项：固定在导航顶部 */}
          <button
            onClick={() => { toast.dismiss(); navigate("/"); }}
            className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors hover:bg-accent text-muted-foreground hover:text-foreground border-b mb-1 pb-2"
          >
            <ArrowLeft className="h-4 w-4" />
            <span>{t("common.back")}</span>
          </button>
          {navItems.map((item) => (
            <button
              key={item.key}
              onClick={() => setActiveTab(item.key)}
              className={`flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors ${
                activeTab === item.key
                  ? "bg-primary text-primary-foreground"
                  : "hover:bg-accent text-muted-foreground hover:text-foreground"
              }`}
            >
              {item.icon}
              <span>{item.label}</span>
            </button>
          ))}
        </nav>

        {/* API 列表 — 悬浮在左侧导航和正文卡片之间，保持最小宽度 192px */}
        {activeTab === "translate" && (
          <div
            ref={setApiListContainer}
            className="absolute left-48 top-0 bottom-0 w-48 border-r bg-background/95 flex flex-col overflow-hidden z-10"
          />
        )}

        {/* 正文卡片 — 所有标签统一位置 */}
        <div className="flex-1 overflow-auto p-6">
          <div className="mx-auto max-w-2xl">
            {activeTab === "general" && (
              <GeneralSettings theme={theme} setTheme={setTheme} language={language} setLanguage={setLanguage} />
            )}
            {activeTab === "translate" && (
              <TranslateApiSettings listContainer={apiListContainer} />
            )}
            {activeTab === "player" && <PlayerSettings />}
            {activeTab === "advanced" && <AdvancedSettings />}
            {activeTab === "developer" && devMode && <DeveloperSettings />}
            {activeTab === "about" && <AboutSettings />}
          </div>
        </div>
      </div>
    </div>
  );
}

// === SECTION 1 END ===

// === 通用设置 ===
function GeneralSettings({ theme, setTheme, language, setLanguage }: {
  theme: string;
  setTheme: (t: "light" | "dark" | "system") => void;
  language: string;
  setLanguage: (l: "zh" | "en") => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{t("settings.general")}</h2>
        <p className="text-sm text-muted-foreground mt-1">{t("settings.generalDesc", "应用基本外观与行为设置")}</p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.appearance", "外观")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* 界面语言 */}
          <div className="flex items-center justify-between">
            <div>
              <label className="text-sm font-medium">{t("settings.language")}</label>
              <p className="text-xs text-muted-foreground">{t("settings.languageDesc", "应用界面显示语言")}</p>
            </div>
            <Select value={language} onValueChange={setLanguage}>
              <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="zh">中文</SelectItem>
                <SelectItem value="en">English</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="border-t pt-4" />

          {/* 主题 */}
          <div className="flex items-center justify-between">
            <div>
              <label className="text-sm font-medium">{t("settings.theme")}</label>
              <p className="text-xs text-muted-foreground">{t("settings.themeDesc", "浅色/深色/跟随系统")}</p>
            </div>
            <Select value={theme} onValueChange={setTheme}>
              <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="light">{t("settings.light")}</SelectItem>
                <SelectItem value="dark">{t("settings.dark")}</SelectItem>
                <SelectItem value="system">{t("settings.system")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.defaultLangs", "默认翻译语言")}</CardTitle>
        </CardHeader>
        <CardContent>
          <DefaultLangSettings />
        </CardContent>
      </Card>
    </div>
  );
}

function DefaultLangSettings() {
  const { t } = useTranslation();
  const [sourceLang, setSourceLang] = useState("en");
  const [targetLang, setTargetLang] = useState("zh");
  const [followSystem, setFollowSystem] = useState(true);
  const [systemLang, setSystemLang] = useState("zh");

  useEffect(() => {
    // 探测系统语言
    api.getSystemLang().then((lang) => {
      setSystemLang(lang);
      api.getConfig("default_target_lang_follow_system").then((v) => {
        const follow = v === null ? true : v === "true";
        setFollowSystem(follow);
        if (follow) {
          setTargetLang(lang);
          api.setConfig("default_target_lang", lang);
        } else {
          api.getConfig("default_target_lang").then((saved) => {
            if (saved) setTargetLang(saved);
          });
        }
      });
    }).catch(() => {
      api.getConfig("default_target_lang").then((v) => v && setTargetLang(v));
    });
    api.getConfig("default_source_lang").then((v) => v && setSourceLang(v));
  }, []);

  const saveSource = (v: string) => { setSourceLang(v); api.setConfig("default_source_lang", v); };
  const saveTarget = (v: string) => {
    setTargetLang(v);
    api.setConfig("default_target_lang", v);
    if (followSystem) {
      setFollowSystem(false);
      api.setConfig("default_target_lang_follow_system", "false");
    }
  };
  const toggleFollowSystem = (follow: boolean) => {
    setFollowSystem(follow);
    api.setConfig("default_target_lang_follow_system", String(follow));
    if (follow) {
      setTargetLang(systemLang);
      api.setConfig("default_target_lang", systemLang);
    }
  };

  const langName = (code: string) => {
    const map: Record<string, string> = { zh: "中文", en: "English", ja: "日本語", ko: "한국어", fr: "Français", de: "Deutsch", es: "Español", ru: "Русский" };
    return map[code] ?? code;
  };

  return (
    <>
      <div className="flex items-center justify-between">
        <label className="text-sm">{t("settings.defaultSourceLang")}</label>
        <Select value={sourceLang} onValueChange={saveSource}>
          <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="en">English</SelectItem>
            <SelectItem value="ja">日本語</SelectItem>
            <SelectItem value="ko">한국어</SelectItem>
            <SelectItem value="auto">Auto</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div className="flex items-center justify-between">
        <div>
          <label className="text-sm">{t("settings.defaultTargetLang")}</label>
          <p className="text-xs text-muted-foreground">
            {t("settings.followSystem", "跟随系统语言")}（{langName(systemLang)}）
          </p>
        </div>
        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={followSystem}
            onChange={(e) => toggleFollowSystem(e.target.checked)}
            className="h-4 w-4 rounded border-gray-300"
          />
          <Select value={targetLang} onValueChange={saveTarget} disabled={followSystem}>
            <SelectTrigger className="w-32"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="zh">中文</SelectItem>
              <SelectItem value="en">English</SelectItem>
              <SelectItem value="ja">日本語</SelectItem>
              <SelectItem value="ko">한국어</SelectItem>
              <SelectItem value="fr">Français</SelectItem>
              <SelectItem value="de">Deutsch</SelectItem>
              <SelectItem value="es">Español</SelectItem>
              <SelectItem value="ru">Русский</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>
    </>
  );
}

// === SECTION 2 END ===

// === 翻译 API 设置 ===

// 检查所有服务是否已配置（用于左列表显示已配置项 + 添加面板显示未配置项）
// 判定逻辑：以用户显式保存的非敏感配置为准，不再把密钥环可读性作为左列表显示条件。
// 原因：部分环境（macOS Keychain/沙盒/权限）下密钥环写入可能返回成功但读取失败，
// 导致保存后左侧列表仍不显示。翻译时后端仍会校验密钥。
async function checkAllServiceConfigs(): Promise<Set<string>> {
  const configured = new Set<string>();
  const traditional = SERVICES.filter((s) => s.category === "traditional" && !s.comingSoon);
  await Promise.all(traditional.map(async (s) => {
    const appId = await api.getConfig(`translate_${s.id}_app_id`).catch(() => null);
    // 只要 App ID 存在即认为已添加；密钥用于翻译时后端校验
    if (appId) configured.add(s.id);
  }));
  const ai = SERVICES.filter((s) => s.category === "ai");
  await Promise.all(ai.map(async (s) => {
    const [baseUrl, selectedModels] = await Promise.all([
      api.getConfig(`translate_openai_${s.id}_base_url`).catch(() => null),
      api.getConfig(`translate_openai_${s.id}_selected_models`).catch(() => null),
    ]);
    // 只要填写了 baseUrl 即认为已添加（模型可选，密钥用于翻译时后端校验）
    if (baseUrl) configured.add(s.id);
  }));
  return configured;
}

// === SECTION 3A END ===

export function TranslateApiSettings({ listContainer }: { listContainer: HTMLDivElement | null }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const devMode = useDevModeStore((s) => s.devMode);
  const [searchParams] = useSearchParams();

  // 左列表选中项：null=官方API, "add:traditional"=添加传统, "add:ai"=添加AI, 否则=服务id
  const [selectedServiceId, setSelectedServiceId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [configuredIds, setConfiguredIds] = useState<Set<string>>(new Set());

  // 当前编辑的服务定义（null=官方API/添加面板）
  const currentService = selectedServiceId ? getServiceById(selectedServiceId) : null;

  // 表单状态
  const [appId, setAppId] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [region, setRegion] = useState("global");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<"ok" | "fail" | null>(null);
  const [loading, setLoading] = useState(true);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [proxyMode, setProxyMode] = useState("none");
  const [useProxy, setUseProxy] = useState<boolean | null>(null);
  // AI 专属
  const [baseUrl, setBaseUrl] = useState("");
  const [qps, setQps] = useState(5);
  const [model, setModel] = useState("");
  const [modelList, setModelList] = useState<string[]>([]);
  const [loadingModels, setLoadingModels] = useState(false);
  const [selectedModels, setSelectedModels] = useState<{ id: string; modelType: string }[]>([]);
  const [modelDropdownOpen, setModelDropdownOpen] = useState(false);
  const [modelFilter, setModelFilter] = useState("");
  const modelDropdownRef = useRef<HTMLDivElement>(null);
  const lastAutoFetchUrlRef = useRef<string>("");

  // === SECTION 3B END ===

  // 加载已配置服务列表
  useEffect(() => {
    checkAllServiceConfigs().then(setConfiguredIds);
  }, []);

  // URL 参数支持：?tab=translate&service=deepseek 直接跳转到某服务
  useEffect(() => {
    const paramService = searchParams.get("service");
    if (paramService) {
      setSelectedServiceId(paramService);
      return;
    }
    setSelectedServiceId(null); // 默认显示官方 API
  }, [searchParams]);

  // 非开发者模式下隐藏官方 API 卡片：若当前选中官方 API，则切到第一个已配置服务或添加面板
  useEffect(() => {
    if (!devMode && selectedServiceId === null) {
      const traditional = SERVICES.filter((s) => s.category === "traditional" && configuredIds.has(s.id));
      const ai = SERVICES.filter((s) => s.category === "ai" && configuredIds.has(s.id));
      const firstConfigured = [...traditional, ...ai][0];
      setSelectedServiceId(firstConfigured ? firstConfigured.id : "add:ai");
    }
  }, [devMode, selectedServiceId, configuredIds]);

  // 根据模型 id 自动识别 model_type
  const autoDetectModelTypeStr = useCallback((m: string): string => {
    const lower = m.toLowerCase();
    if (lower.includes("qwen3")) return "qwen3";
    if (lower.includes("deepseek")) return "deepseek";
    return "generic";
  }, []);

  // AI：刷新模型列表核心逻辑
  const fetchModels = useCallback(async (urlToFetch: string, keyForFetch?: string) => {
    const trimmedUrl = urlToFetch.trim();
    if (!trimmedUrl) return;
    try { new URL(trimmedUrl); } catch { return; }
    setLoadingModels(true);
    setModelDropdownOpen(false);
    try {
      const candidateUrls: string[] = [];
      const normalized = trimmedUrl.replace(/\/$/, "");
      if (normalized.includes("/v1")) {
        candidateUrls.push(normalized);
        const withoutV1 = normalized.replace(/\/v1\/?$/, "");
        if (withoutV1 && withoutV1 !== normalized) candidateUrls.push(withoutV1);
      } else {
        candidateUrls.push(normalized);
        candidateUrls.push(`${normalized}/v1`);
      }
      const uniqueUrls = Array.from(new Set(candidateUrls));
      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("timeout")), 3000)
      );
      let successUrl = "";
      let allModels: string[] = [];
      for (const url of uniqueUrls) {
        try {
          const models = await Promise.race([
            api.listOpenaiModels(url, keyForFetch),
            timeoutPromise,
          ]);
          if (models.length > 0) {
            successUrl = url;
            allModels = models;
            break;
          }
        } catch { /* 静默 */ }
      }
      if (!successUrl) return;
      if (successUrl !== trimmedUrl) setBaseUrl(successUrl);
      setModelList(allModels);
      lastAutoFetchUrlRef.current = successUrl;
      setSelectedModels((prev) => prev.filter((sm) => allModels.includes(sm.id)));
      setModel((prevModel) => {
        if (prevModel && !allModels.includes(prevModel)) return "";
        return prevModel;
      });
    } catch { /* 静默 */ } finally {
      setLoadingModels(false);
    }
  }, []);

  // 选中服务变化时加载配置
  useEffect(() => {
    if (!currentService) {
      setLoading(false);
      return;
    }
    setLoading(true);
    setAppId("");
    setSecretKey("");
    setRegion("global");
    setModel("");
    setModelList([]);
    setModelFilter("");
    setModelDropdownOpen(false);
    setSelectedModels([]);
    setBaseUrl("");
    setQps(currentService.presetQps || 5);
    lastAutoFetchUrlRef.current = "";

    const sid = currentService.id;
    const isAi = currentService.category === "ai";

    if (isAi) {
      Promise.all([
        api.getConfig(`translate_openai_${sid}_base_url`).catch(() => null),
        api.getConfig(`translate_openai_${sid}_selected_models`).catch(() => null),
        api.getConfig(`translate_openai_${sid}_selected_model_types`).catch(() => null),
        api.getConfig(`translate_openai_${sid}_qps`).catch(() => null),
        api.getConfig(`translate_openai_${sid}_use_proxy`).catch(() => null),
        api.getCredential(`openai_${sid}`, "secret").catch(() => null),
      ]).then(([savedBaseUrl, savedSelected, savedModelTypes, savedQps, savedUseProxy, savedSecret]) => {
        if (savedBaseUrl) {
          setBaseUrl(savedBaseUrl);
          fetchModels(savedBaseUrl, savedSecret || undefined);
        } else if (currentService.presetBaseUrl) {
          setBaseUrl(currentService.presetBaseUrl);
        }
        if (savedQps) setQps(parseInt(savedQps) || currentService.presetQps || 5);
        if (savedSecret) setSecretKey("••••••••");
        setUseProxy(savedUseProxy === "true" ? true : savedUseProxy === "false" ? false : null);
        if (savedSelected) {
          const ids = savedSelected.split(",").filter(Boolean);
          let typeMap: Record<string, string> = {};
          if (savedModelTypes) {
            try { typeMap = JSON.parse(savedModelTypes); } catch { /* ignore */ }
          }
          setSelectedModels(ids.map((id) => ({
            id,
            modelType: typeMap[id] || autoDetectModelTypeStr(id),
          })));
        }
        setLoading(false);
      });
    } else {
      Promise.all([
        api.getConfig(`translate_${sid}_app_id`).catch(() => null),
        api.getConfig(`translate_${sid}_region`).catch(() => null),
        api.getCredential(sid, "secret").catch(() => null),
        api.getConfig(`translate_${sid}_use_proxy`).catch(() => null),
        api.getConfig(`translate_${sid}_qps`).catch(() => null),
      ]).then(([savedAppId, savedRegion, savedSecret, savedUseProxy, savedQps]) => {
        if (savedAppId) setAppId(savedAppId);
        if (savedRegion) setRegion(savedRegion);
        if (savedSecret) setSecretKey("••••••••");
        if (savedQps) setQps(parseInt(savedQps) || currentService.presetQps || 5);
        setUseProxy(savedUseProxy === "true" ? true : savedUseProxy === "false" ? false : null);
        setLoading(false);
      });
    }
  }, [selectedServiceId, currentService, fetchModels, autoDetectModelTypeStr]);

  // 加载软件代理模式
  useEffect(() => {
    api.getProxy().then((cfg) => setProxyMode(cfg.mode)).catch(() => {});
  }, []);

  // 点击模型下拉外部时关闭
  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (modelDropdownRef.current && !modelDropdownRef.current.contains(e.target as Node)) {
        setModelDropdownOpen(false);
      }
    }
    if (modelDropdownOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [modelDropdownOpen]);

  // === SECTION 3C END ===

  // 保存配置
  const handleSave = useCallback(async () => {
    if (!currentService) return;
    const sid = currentService.id;

    // 校验必填项：requiresApiKey 为 true 的服务必须填写密钥
    if (currentService.requiresApiKey) {
      const isMasked = secretKey === "••••••••";
      if (!isMasked && !secretKey.trim()) {
        toast.error(t("settings.secretKeyRequired", "请填写 API Key"));
        return;
      }
    }
    // 传统翻译需要 app_id（百度/有道等）
    if (currentService.category === "traditional" && !appId.trim()) {
      toast.error(t("settings.appIdRequired", "请填写 App ID"));
      return;
    }

    try {
      if (currentService.category === "ai") {
        await api.setConfig(`translate_openai_${sid}_base_url`, baseUrl);
        await api.setConfig(`translate_openai_${sid}_selected_models`, selectedModels.map((x) => x.id).join(","));
        const typeMap: Record<string, string> = {};
        selectedModels.forEach((x) => { typeMap[x.id] = x.modelType; });
        await api.setConfig(`translate_openai_${sid}_selected_model_types`, JSON.stringify(typeMap));
        await api.setConfig(`translate_openai_${sid}_qps`, String(qps));
      } else {
        await api.setConfig(`translate_${sid}_app_id`, appId);
        await api.setConfig(`translate_${sid}_region`, region);
        await api.setConfig(`translate_${sid}_qps`, String(qps));
      }
      // 保存 per-service 代理开关（null→未设置，跟随软件代理）
      const proxyKey = currentService.category === "ai" ? `translate_openai_${sid}_use_proxy` : `translate_${sid}_use_proxy`;
      await api.setConfig(proxyKey, useProxy === null ? "" : useProxy ? "true" : "false");

      // 保存密钥到 keyring
      let credentialSaved = true;
      if (secretKey && secretKey !== "••••••••") {
        const keyringProvider = currentService.category === "ai" ? `openai_${sid}` : sid;
        try {
          await api.saveCredential(keyringProvider, "secret", secretKey);
        } catch (e: any) {
          credentialSaved = false;
          warn("saveCredential 失败:", e);
        }
      }

      // 刷新已配置列表，await 保证保存完成后左侧列表已更新
      const updated = await checkAllServiceConfigs();
      setConfiguredIds(updated);

      if (credentialSaved) {
        toast.success(t("settings.saveSuccess", "已保存"));
      } else {
        toast.warning(t("settings.saveSuccessButCredential", "配置已保存，但密钥保存失败（可能不支持系统钥匙串）"));
      }
    } catch (e: any) {
      toast.error(t("settings.saveFailed", "保存失败") + ": " + formatIpcError(e));
    }
  }, [currentService, appId, secretKey, region, baseUrl, qps, selectedModels, useProxy, t]);

  // 测试连接
  const handleTest = useCallback(async () => {
    if (!currentService) return;
    const sid = currentService.id;
    if (currentService.category === "ai") {
      const trimmedUrl = baseUrl.trim();
      if (!trimmedUrl) {
        toast.error(t("settings.openaiBaseUrlRequired", "请先填写 API 地址"));
        return;
      }
      try { new URL(trimmedUrl); } catch {
        toast.error(t("settings.openaiInvalidUrl", "API 地址格式无效"));
        return;
      }
    }
    // 校验必填项
    if (currentService.requiresApiKey) {
      const isMasked = secretKey === "••••••••";
      if (!isMasked && !secretKey.trim()) {
        toast.error(t("settings.secretKeyRequired", "请填写 API Key"));
        return;
      }
    }
    if (currentService.category === "traditional" && !appId.trim()) {
      toast.error(t("settings.appIdRequired", "请填写 App ID"));
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      const actualSecret = secretKey === "••••••••" ? undefined : secretKey;
      const provider = currentService.category === "ai" ? "openai" : sid;
      const serviceId = currentService.category === "ai" ? sid : undefined;
      const result = await api.testTranslateConnection(
        provider,
        appId || undefined,
        actualSecret,
        region || undefined,
        currentService.category === "ai" ? baseUrl.trim() : undefined,
        currentService.category === "ai" ? (selectedModels[0]?.id || undefined) : undefined,
        currentService.category === "ai" ? (selectedModels[0]?.modelType || undefined) : undefined,
        serviceId,
      );
      setTestResult("ok");
      if (result.original && result.translated) {
        toast.success(
          t("settings.openaiTestSuccess", "连接成功") + "\n" +
          `${result.original} → ${result.translated}`,
          { duration: 8000 }
        );
      } else {
        toast.success(t("settings.testSuccess", "连接成功"));
      }
    } catch (e: any) {
      setTestResult("fail");
      toast.error(formatIpcError(e));
    } finally {
      setTesting(false);
    }
  }, [currentService, appId, secretKey, region, baseUrl, selectedModels, t]);

  // 删除配置
  const handleDeleteConfig = useCallback(async () => {
    if (!currentService) return;
    const sid = currentService.id;
    setDeleting(true);
    try {
      if (currentService.category === "ai") {
        await api.setConfig(`translate_openai_${sid}_base_url`, "");
        await api.setConfig(`translate_openai_${sid}_selected_models`, "");
        await api.setConfig(`translate_openai_${sid}_selected_model_types`, "");
        await api.setConfig(`translate_openai_${sid}_qps`, "");
        try { await api.deleteCredential(`openai_${sid}`, "secret"); } catch { /* ignore */ }
      } else {
        await api.setConfig(`translate_${sid}_app_id`, "");
        await api.setConfig(`translate_${sid}_region`, "");
        await api.setConfig(`translate_${sid}_qps`, "");
        try { await api.deleteCredential(sid, "secret"); } catch { /* ignore */ }
      }
      setAppId("");
      setSecretKey("");
      setRegion("global");
      setBaseUrl("");
      setModel("");
      setModelFilter("");
      setModelList([]);
      setSelectedModels([]);
      setTestResult(null);
      checkAllServiceConfigs().then(setConfiguredIds);
      toast.success(t("settings.configDeleted", "配置已删除"));
    } catch (e: any) {
      toast.error(formatIpcError(e));
    } finally {
      setDeleting(false);
      setDeleteConfirmOpen(false);
    }
  }, [currentService, t]);

  // 多选：勾选/取消勾选模型
  const toggleModelSelection = useCallback((m: string) => {
    setSelectedModels((prev) => {
      const exists = prev.find((x) => x.id === m);
      if (exists) return prev.filter((x) => x.id !== m);
      return [...prev, { id: m, modelType: autoDetectModelTypeStr(m) }];
    });
  }, [autoDetectModelTypeStr]);

  const setModelTypeForModel = useCallback((modelId: string, newType: string) => {
    setSelectedModels((prev) =>
      prev.map((x) => x.id === modelId ? { ...x, modelType: newType } : x)
    );
  }, []);

  // AI：手动刷新模型列表
  const handleRefreshModels = useCallback(async () => {
    const trimmedUrl = baseUrl.trim();
    if (!trimmedUrl) {
      toast.error(t("settings.openaiBaseUrlRequired", "请先填写 API 地址"));
      return;
    }
    try { new URL(trimmedUrl); } catch {
      toast.error(t("settings.openaiInvalidUrl", "API 地址格式无效"));
      return;
    }
    setLoadingModels(true);
    setModelDropdownOpen(false);
    try {
      const actualSecret = secretKey === "••••••••" ? undefined : secretKey;
      const candidateUrls: string[] = [];
      const normalized = trimmedUrl.replace(/\/$/, "");
      if (normalized.includes("/v1")) {
        candidateUrls.push(normalized);
        const withoutV1 = normalized.replace(/\/v1\/?$/, "");
        if (withoutV1 && withoutV1 !== normalized) candidateUrls.push(withoutV1);
      } else {
        candidateUrls.push(normalized);
        candidateUrls.push(`${normalized}/v1`);
      }
      const uniqueUrls = Array.from(new Set(candidateUrls));
      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("timeout")), 3000)
      );
      let lastError = "";
      let successUrl = "";
      let allModels: string[] = [];
      for (const url of uniqueUrls) {
        try {
          const models = await Promise.race([
            api.listOpenaiModels(url, actualSecret),
            timeoutPromise,
          ]);
          if (models.length > 0) {
            successUrl = url;
            allModels = models;
            break;
          }
        } catch (e: any) {
          lastError = e.message === "timeout"
            ? t("settings.openaiFetchTimeout", "获取超时，请检查地址是否正确")
            : formatIpcError(e);
        }
      }
      if (!successUrl) {
        toast.error(lastError || t("settings.openaiNoModels", "未能获取模型列表"));
        return;
      }
      if (successUrl !== trimmedUrl) setBaseUrl(successUrl);
      setModelList(allModels);
      lastAutoFetchUrlRef.current = successUrl;
      setSelectedModels((prev) => prev.filter((sm) => allModels.includes(sm.id)));
      if (model && !allModels.includes(model)) setModel("");
      toast.success(t("settings.openaiModelsLoaded", "已加载 {{count}} 个模型", { count: allModels.length }));
    } catch (e: any) {
      toast.error(formatIpcError(e));
    } finally {
      setLoadingModels(false);
    }
  }, [baseUrl, secretKey, model, t]);

  const handleBaseUrlBlur = useCallback(() => {
    const trimmedUrl = baseUrl.trim();
    if (!trimmedUrl) return;
    try { new URL(trimmedUrl); } catch { return; }
    if (trimmedUrl === lastAutoFetchUrlRef.current) return;
    lastAutoFetchUrlRef.current = trimmedUrl;
    handleRefreshModels();
  }, [baseUrl, handleRefreshModels]);

  // === SECTION 3D END ===

  // 渲染：左列表项（已配置服务）
  const renderServiceMenuItem = (s: ServiceDef) => {
    const isSelected = selectedServiceId === s.id;
    return (
      <button
        key={s.id}
        onClick={() => setSelectedServiceId(s.id)}
        className={cn(
          "w-full text-left px-2 py-1.5 rounded-md text-sm transition-colors border",
          isSelected ? "bg-accent text-accent-foreground border-primary/50" : "hover:bg-accent/50 border-border"
        )}
      >
        <div className="flex items-center justify-between gap-1">
          <span className="truncate font-medium">{s.name}</span>
          {s.comingSoon && (
            <span className="text-[10px] text-muted-foreground ml-1 shrink-0">{t("settings.comingSoonTag", "即将支持")}</span>
          )}
          {!s.comingSoon && s.hasFreeTier && (
            <span className="text-[10px] text-green-600 shrink-0">🆓</span>
          )}
          {!s.comingSoon && s.completelyFree && (
            <span className="text-[10px] text-green-600 shrink-0">免费</span>
          )}
        </div>
        <p className="text-[10px] text-muted-foreground line-clamp-1 leading-tight mt-0.5">
          {s.description}
        </p>
      </button>
    );
  };

  // 渲染：添加 API 卡片
  const renderAddCard = (category: "traditional" | "ai") => (
    <button
      onClick={() => setSelectedServiceId(category === "traditional" ? "add:traditional" : "add:ai")}
      className={cn(
        "w-full text-left px-2 py-1.5 rounded-md text-sm border border-dashed border-border transition-colors hover:bg-accent/50",
        selectedServiceId === (category === "traditional" ? "add:traditional" : "add:ai") && "bg-accent text-accent-foreground"
      )}
    >
      <span className="flex items-center gap-1 text-primary">
        <Plus className="h-3 w-3" />
        {t("settings.addApi", "添加 API")}
      </span>
      <p className="text-[10px] text-muted-foreground line-clamp-1 leading-tight mt-0.5">
        {category === "traditional"
          ? t("settings.addTraditionalServiceDesc", "添加百度翻译等传统翻译服务")
          : t("settings.addAiServiceDesc", "添加 DeepSeek 等 AI 大模型")}
      </p>
    </button>
  );

  // 渲染：官方 API 面板
  const renderOfficialPanel = () => (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold flex items-center gap-2">
          <Star className="h-5 w-5 text-yellow-500" />
          {t("settings.officialApi", "官方 API")}
        </h2>
        <p className="text-sm text-muted-foreground mt-1">{t("settings.officialApiDesc", "精译·省钱·免费")}</p>
      </div>
      <Card>
        <CardContent className="space-y-4 pt-4">
          <div>
            <h3 className="text-base font-medium">{t("settings.officialApiJingyi", "精译")}</h3>
            <p className="text-sm text-muted-foreground mt-1">{t("settings.officialApiJingyiDesc", "对字幕翻译做了专门调优，翻译质量接近字幕组翻译效果。")}</p>
          </div>
          <div>
            <h3 className="text-base font-medium">{t("settings.officialApiZhongzhuan", "低价时段中转")}</h3>
            <p className="text-sm text-muted-foreground mt-1">{t("settings.officialApiZhongzhuanDesc", "低价时段中转到 DeepSeek，帮用户省钱。本功能按官方定价收取，不赚钱。")}</p>
          </div>
          <div>
            <h3 className="text-base font-medium">{t("settings.officialApiMianfei", "超慢免费 API")}</h3>
            <p className="text-sm text-muted-foreground mt-1">{t("settings.officialApiMianfeiDesc", "转发到作者的电脑上，使用 Qwen 3.5 9B 翻译。速度慢但完全免费。")}</p>
          </div>
          <div className="border-t pt-4 space-y-2">
            <p className="text-sm text-muted-foreground">{t("settings.officialApiLoginPrompt", "登陆认证后即可使用官方 API 服务。")}</p>
            <Button
              size="sm"
              onClick={() => {
                openUrl("https://www.baidu.com").catch(() => {
                  toast.error("无法打开浏览器");
                });
              }}
            >
              {t("settings.officialApiLoginButton", "登陆认证")}
            </Button>
            <p className="text-xs text-muted-foreground">{t("settings.officialApiLoginNote", "（当前版本登陆认证功能尚未实现，点击跳转到百度，后续版本替换为官方认证）")}</p>
          </div>
        </CardContent>
      </Card>
    </div>
  );

  // 渲染：添加 API 面板（未配置服务网格）
  const renderAddPanel = (category: "traditional" | "ai") => {
    const services = SERVICES.filter((s) => s.category === category && !configuredIds.has(s.id));
    const filtered = services.filter((s) => matchesSearch(s, searchQuery));
    return (
      <div className="space-y-4">
        <div>
          <h2 className="text-xl font-semibold">
            {category === "traditional"
              ? t("settings.addTraditionalService", "添加传统翻译服务")
              : t("settings.addAiService", "添加 AI 大模型服务")}
          </h2>
        </div>
        <Input
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="搜索服务名称..."
          className="max-w-sm"
        />
        {filtered.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t("settings.noServicesToAdd", "没有可添加的服务（全部已添加或搜索无匹配）")}</p>
        ) : (
          <div className="grid grid-cols-2 gap-3">
            {filtered.map((s) => (
              <button
                key={s.id}
                onClick={() => {
                  setSelectedServiceId(s.id);
                  setSearchQuery("");
                }}
                className="text-left p-3 rounded-md border border-border hover:border-primary hover:bg-accent/50 transition-colors"
              >
                <div className="flex items-center justify-between">
                  <span className="font-medium text-sm">{s.name}</span>
                  {s.comingSoon && (
                    <span className="text-xs text-muted-foreground">{t("settings.comingSoonTag", "即将支持")}</span>
                  )}
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                  {s.completelyFree ? "完全免费" : s.hasFreeTier ? `🆓 ${s.freeQuota}` : s.freeQuota}
                </p>
                <p className="text-xs text-muted-foreground">{s.price}</p>
              </button>
            ))}
          </div>
        )}
      </div>
    );
  };

  // === SECTION 3E END ===

  // 渲染：传统翻译配置面板
  const renderTraditionalConfig = (s: ServiceDef) => (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{s.name}</h2>
        <p className="text-sm text-muted-foreground mt-1">
          {s.completelyFree ? "完全免费" : s.hasFreeTier ? `🆓 ${s.freeQuota}` : s.freeQuota}
          {" · "}{s.price}
        </p>
      </div>
      {s.comingSoon ? (
        <Card>
          <CardContent className="py-8 text-center">
            <p className="text-muted-foreground">{t("settings.comingSoonTag", "即将支持")}</p>
          </CardContent>
        </Card>
      ) : (
        <Card>
          <CardContent className="space-y-4 pt-4">
            {s.docUrl && (
              <a href={s.docUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-xs text-primary hover:underline">
                {t("settings.getApiKeyPrefix", "获取")} {s.name} API Key
                <ExternalLink className="h-3 w-3" />
              </a>
            )}
            {s.appIdLabel && (
              <div>
                <label className="text-sm font-medium">{s.appIdLabel}</label>
                <p className="text-xs text-muted-foreground mb-1">{t("settings.appIdDesc", "翻译服务的 App ID / API Key")}</p>
                <Input value={appId} onChange={(e) => setAppId(e.target.value)} placeholder={s.appIdPlaceholder} disabled={loading} />
              </div>
            )}
            <div>
              <label className="text-sm font-medium">{t("settings.secretKey", "密钥")}</label>
              <p className="text-xs text-muted-foreground mb-1">{t("settings.secretKeyDesc", "API 密钥，加密存储在系统密钥环")}</p>
              <div className="flex gap-2">
                <Input type="password" value={secretKey} onChange={(e) => setSecretKey(e.target.value)} placeholder="Secret Key" disabled={loading} />
                {secretKey === "••••••••" && (
                  <Button size="sm" variant="ghost" onClick={() => setSecretKey("")}>{t("settings.edit", "修改")}</Button>
                )}
              </div>
            </div>
            {s.hasRegion && (
              <div>
                <label className="text-sm font-medium">{t("settings.region", "区域")}</label>
                <p className="text-xs text-muted-foreground mb-1">{t("settings.regionDesc", "Azure 区域，如 global 或 china")}</p>
                <Input value={region} onChange={(e) => setRegion(e.target.value)} placeholder="global / china" />
              </div>
            )}
            {/* QPS / 并发上限 */}
            <div>
              <label className="text-sm font-medium">{t("settings.qpsLabel", "QPS 上限")}</label>
              <p className="text-xs text-muted-foreground mb-1">{t("settings.qpsDescTraditional", "该服务的请求频率上限。免费版通常为 1-5，付费版可按套餐调高。")}</p>
              <Input type="number" value={qps} onChange={(e) => setQps(parseInt(e.target.value) || 1)} min={1} disabled={loading} className="w-24" />
            </div>
            {/* 代理 */}
            <div className="flex items-center justify-between border-t pt-3">
              <div>
                <label className="text-sm font-medium">{t("settings.useProxy", "使用软件代理")}</label>
                <p className="text-xs text-muted-foreground">
                  {proxyMode !== "none"
                    ? t("settings.useProxyDesc", "通过软件配置的代理访问此翻译 API")
                    : t("settings.useProxyNoProxy", "未配置代理，请在高级设置中先配置代理")}
                </p>
              </div>
              <Select
                value={useProxy === null ? "default" : useProxy ? "true" : "false"}
                onValueChange={(v) => setUseProxy(v === "default" ? null : v === "true")}
              >
                <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="default">{t("settings.proxyDefault", "跟随")}</SelectItem>
                  <SelectItem value="true">{t("settings.proxyOn", "启用")}</SelectItem>
                  <SelectItem value="false">{t("settings.proxyOff", "禁用")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {/* 操作按钮 */}
            <div className="flex items-center gap-2 border-t pt-3">
              <Button size="sm" onClick={handleSave} disabled={loading}>{t("settings.save", "保存")}</Button>
              <Button size="sm" variant="outline" onClick={handleTest} disabled={loading || testing}>
                {testing ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : null}
                {t("settings.testConnection", "测试连接")}
              </Button>
              {devMode && (
                <Button size="sm" variant="destructive" onClick={() => setDeleteConfirmOpen(true)} disabled={loading}>
                  <Trash2 className="mr-1 h-4 w-4" />
                  {t("settings.deleteConfig", "删除配置")}
                </Button>
              )}
              {testResult === "ok" && (
                <span className="flex items-center gap-1 text-sm text-green-600">
                  <Check className="h-4 w-4" /> {t("settings.testSuccess", "连接成功")}
                </span>
              )}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );

  // === SECTION 3F END ===

  // 渲染：AI 大模型配置面板
  const renderAiConfig = (s: ServiceDef) => (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{s.name}</h2>
        <p className="text-sm text-muted-foreground mt-1">
          {s.completelyFree ? "完全免费" : s.hasFreeTier ? `🆓 ${s.freeQuota}` : s.freeQuota}
          {" · "}{s.price}
        </p>
      </div>
      <Card>
        <CardContent className="space-y-4 pt-4">
          {s.docUrl && (
            <a href={s.docUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-xs text-primary hover:underline">
              {t("settings.getApiKeyPrefix", "获取")} {s.name} API Key
              <ExternalLink className="h-3 w-3" />
            </a>
          )}
          {/* API 地址 */}
          <div>
            <label className="text-sm font-medium">{t("settings.openaiBaseUrl", "API 地址")}</label>
            <p className="text-xs text-muted-foreground mb-1">{t("settings.openaiBaseUrlDesc", "OpenAI 兼容端点")}</p>
            <div className="flex gap-2">
              <Input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                onBlur={handleBaseUrlBlur}
                placeholder={t("settings.openaiBaseUrlPlaceholder", "必填，例如 http://localhost:1234/v1")}
                disabled={loading}
              />
              {s.presetBaseUrl && baseUrl !== s.presetBaseUrl && (
                <Button size="sm" variant="ghost" onClick={() => { setBaseUrl(s.presetBaseUrl!); lastAutoFetchUrlRef.current = ""; }}>
                  {t("settings.resetBaseUrl", "重置为默认")}
                </Button>
              )}
            </div>
          </div>
          {/* 模型多选 */}
          <div ref={modelDropdownRef}>
            <label className="text-sm font-medium">{t("settings.openaiModel", "模型")}</label>
            <p className="text-xs text-muted-foreground mb-1">{t("settings.openaiModelDesc", "勾选要使用的模型")}</p>
            <div className="flex gap-2">
              <div className="relative flex-1">
                <div
                  className="flex min-h-[36px] flex-wrap items-center gap-1 rounded-md border border-input bg-background px-2 py-1 text-sm focus-within:ring-1 focus-within:ring-ring"
                  onClick={() => setModelDropdownOpen(true)}
                >
                  {selectedModels.map((sm) => (
                    <span key={sm.id} className="group/tag inline-flex items-center gap-1 rounded border border-border bg-muted px-1.5 py-0.5 text-xs text-foreground hover:border-destructive/50">
                      {sm.id}
                      <span className="text-muted-foreground">|</span>
                      <span className="text-muted-foreground">{sm.modelType}</span>
                      <button
                        type="button"
                        onClick={(e) => { e.stopPropagation(); toggleModelSelection(sm.id); }}
                        className="ml-0.5 flex h-4 w-4 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-destructive hover:text-white"
                      >
                        <X className="h-3 w-3" />
                      </button>
                    </span>
                  ))}
                  <input
                    value={modelFilter}
                    onChange={(e) => setModelFilter(e.target.value)}
                    onFocus={() => setModelDropdownOpen(true)}
                    placeholder={selectedModels.length === 0 ? t("settings.openaiModelPlaceholder", "搜索模型名称") : ""}
                    className="flex-1 border-0 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
                    disabled={loading}
                  />
                </div>
                {modelDropdownOpen && (
                  <div className="absolute z-50 mt-1 max-h-80 w-full overflow-auto rounded-md border bg-popover text-popover-foreground shadow-md">
                    {(() => {
                      const filtered = modelList.filter((m) => m.toLowerCase().includes(modelFilter.toLowerCase()));
                      if (filtered.length === 0) {
                        return (
                          <button type="button" onClick={() => { setModelDropdownOpen(false); handleRefreshModels(); }} className="w-full px-3 py-2 text-left text-sm text-primary hover:underline">
                            {modelFilter ? t("settings.openaiNoMatch", "无匹配模型，点击刷新") : t("settings.openaiClickRefresh", "暂无模型，点击刷新")}
                          </button>
                        );
                      }
                      return filtered.map((m) => {
                        const selected = selectedModels.find((x) => x.id === m);
                        return (
                          <div key={m} className="flex w-full items-center gap-2 px-3 py-2 text-sm hover:bg-accent hover:text-accent-foreground">
                            <label className="flex cursor-pointer items-center gap-2 flex-1 min-w-0">
                              <input type="checkbox" className="h-4 w-4 cursor-pointer accent-primary flex-shrink-0" checked={!!selected} onChange={() => toggleModelSelection(m)} />
                              <span className="truncate">{m}</span>
                            </label>
                            {selected && (
                              <select
                                value={selected.modelType}
                                onChange={(e) => { e.stopPropagation(); setModelTypeForModel(m, e.target.value); }}
                                onClick={(e) => e.stopPropagation()}
                                className="flex-shrink-0 rounded border border-input bg-background px-1 py-0.5 text-xs outline-none cursor-pointer"
                              >
                                <option value="qwen3">Qwen3</option>
                                <option value="deepseek">DeepSeek</option>
                                <option value="generic">{t("settings.openaiModelTypeGeneric", "通用")}</option>
                              </select>
                            )}
                          </div>
                        );
                      });
                    })()}
                  </div>
                )}
              </div>
              <Button size="sm" variant="outline" onClick={handleRefreshModels} disabled={loadingModels}>
                {loadingModels ? t("settings.openaiLoading", "加载中...") : t("settings.openaiRefreshModels", "刷新模型")}
              </Button>
            </div>
          </div>
          {/* API Key */}
          <div>
            <label className="text-sm font-medium">{t("settings.openaiApiKey", "API Key（可选）")}</label>
            <p className="text-xs text-muted-foreground mb-1">{t("settings.openaiApiKeyDesc", "局域网部署可留空；云 API 必填，加密存储在系统密钥环")}</p>
            <div className="flex gap-2">
              <Input type="password" value={secretKey} onChange={(e) => setSecretKey(e.target.value)} placeholder={t("settings.openaiApiKeyPlaceholder", "留空表示无认证")} disabled={loading} />
              {secretKey === "••••••••" && (
                <Button size="sm" variant="ghost" onClick={() => setSecretKey("")}>{t("settings.edit", "修改")}</Button>
              )}
            </div>
          </div>
          {/* QPS */}
          <div>
            <label className="text-sm font-medium">{t("settings.qpsLabel", "QPS 上限")}</label>
            <p className="text-xs text-muted-foreground mb-1">{t("settings.qpsDesc", "该服务的并发请求上限。免费版通常较低，付费版可按套餐调高。")}</p>
            <Input type="number" value={qps} onChange={(e) => setQps(parseInt(e.target.value) || 1)} min={1} disabled={loading} className="w-24" />
          </div>
          {/* 代理 */}
          <div className="flex items-center justify-between border-t pt-3">
            <div>
              <label className="text-sm font-medium">{t("settings.useProxy", "使用软件代理")}</label>
              <p className="text-xs text-muted-foreground">
                {proxyMode !== "none"
                  ? t("settings.useProxyDesc", "通过软件配置的代理访问此翻译 API")
                  : t("settings.useProxyNoProxy", "未配置代理，请在高级设置中先配置代理")}
              </p>
            </div>
            <Select
              value={useProxy === null ? "default" : useProxy ? "true" : "false"}
              onValueChange={(v) => setUseProxy(v === "default" ? null : v === "true")}
            >
              <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="default">{t("settings.proxyDefault", "跟随")}</SelectItem>
                <SelectItem value="true">{t("settings.proxyOn", "启用")}</SelectItem>
                <SelectItem value="false">{t("settings.proxyOff", "禁用")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {/* 操作按钮 */}
          <div className="flex items-center gap-2 border-t pt-3">
            <Button size="sm" onClick={handleSave} disabled={loading}>{t("settings.save", "保存")}</Button>
            <Button size="sm" variant="outline" onClick={handleTest} disabled={loading || testing}>
              {testing ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : null}
              {t("settings.testConnection", "测试连接")}
            </Button>
            {devMode && (
              <Button size="sm" variant="destructive" onClick={() => setDeleteConfirmOpen(true)} disabled={loading}>
                <Trash2 className="mr-1 h-4 w-4" />
                {t("settings.deleteConfig", "删除配置")}
              </Button>
            )}
            {testResult === "ok" && (
              <span className="flex items-center gap-1 text-sm text-green-600">
                <Check className="h-4 w-4" /> {t("settings.testSuccess", "连接成功")}
              </span>
            )}
          </div>
          {/* 删除确认弹窗 */}
          <Dialog open={deleteConfirmOpen} onOpenChange={setDeleteConfirmOpen}>
            <DialogContent className="max-w-sm">
              <DialogHeader>
                <DialogTitle>{t("settings.deleteConfigConfirm", "确认删除配置？")}</DialogTitle>
              </DialogHeader>
              <p className="text-sm text-muted-foreground">{t("settings.deleteConfigDesc", "将清除当前引擎的所有配置和凭据，此操作不可撤销。")}</p>
              <div className="flex justify-end gap-2 pt-2">
                <Button size="sm" variant="outline" onClick={() => setDeleteConfirmOpen(false)}>{t("common.cancel", "取消")}</Button>
                <Button size="sm" variant="destructive" onClick={handleDeleteConfig} disabled={deleting}>
                  {deleting ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : null}
                  {t("common.confirm", "确认删除")}
                </Button>
              </div>
            </DialogContent>
          </Dialog>
        </CardContent>
      </Card>
    </div>
  );

  // === SECTION 3G END ===

  // 左列表数据
  const traditionalServices = SERVICES.filter((s) => s.category === "traditional");
  const aiServices = SERVICES.filter((s) => s.category === "ai");
  const configuredTraditional = traditionalServices.filter((s) => configuredIds.has(s.id));
  const configuredAi = aiServices.filter((s) => configuredIds.has(s.id));

  // 右面板内容
  const renderRightPanel = () => {
    if (selectedServiceId === null) return renderOfficialPanel();
    if (selectedServiceId === "add:traditional") return renderAddPanel("traditional");
    if (selectedServiceId === "add:ai") return renderAddPanel("ai");
    if (currentService) {
      return currentService.category === "ai"
        ? renderAiConfig(currentService)
        : renderTraditionalConfig(currentService);
    }
    return renderOfficialPanel();
  };

  const listContent = (
    <div className="flex-1 overflow-y-auto space-y-2 p-2">
      {/* 快速接入 */}
      {devMode && (
        <>
          <p className="text-xs text-muted-foreground px-3 pt-2">快速接入</p>
          {/* 官方 API */}
          <button
            onClick={() => setSelectedServiceId(null)}
            className={cn(
              "w-full text-left px-2 py-1.5 rounded-md text-sm transition-colors border",
              selectedServiceId === null ? "bg-accent text-accent-foreground border-primary/50" : "hover:bg-accent/50 border-border"
            )}
          >
            <span className="flex items-center gap-1 font-medium">
              <Star className="h-4 w-4 text-yellow-500" />
              {t("settings.officialApi", "官方 API")}
            </span>
            <p className="text-[10px] text-muted-foreground line-clamp-1 leading-tight mt-0.5">
              {t("settings.officialApiDesc", "精译·省钱·免费")}
            </p>
          </button>
        </>
      )}

      {/* 传统翻译 */}
      <div className="space-y-1">
        <p className="text-xs text-muted-foreground px-3 pt-2">传统翻译</p>
        {configuredTraditional.map(renderServiceMenuItem)}
        {renderAddCard("traditional")}
      </div>

      {/* AI 大模型 */}
      <div className="space-y-1">
        <p className="text-xs text-muted-foreground px-3 pt-2">AI 大模型</p>
        {configuredAi.map(renderServiceMenuItem)}
        {renderAddCard("ai")}
      </div>
    </div>
  );

  return (
    <>
      {listContainer && createPortal(listContent, listContainer)}
      {renderRightPanel()}
    </>
  );
}

// === SECTION 3 END ===

// === 播放器设置 ===

/// 格式化剩余时间
function formatEta(secs: number): string {
  if (secs <= 0) return "--";
  if (secs < 60) return `${Math.ceil(secs)}秒`;
  const m = Math.floor(secs / 60);
  const s = Math.ceil(secs % 60);
  return `${m}分${s}秒`;
}

function PlayerSettings() {
  const { t } = useTranslation();
  const devMode = useDevModeStore((s) => s.devMode);

  // === libmpv ===
  const mpvStatus = useLibmpvStore((s) => s.status);
  const mpvDownloading = useLibmpvStore((s) => s.downloading);
  const mpvProgress = useLibmpvStore((s) => s.downloadProgress);
  const mpvStage = useLibmpvStore((s) => s.downloadStage);
  const mpvMessage = useLibmpvStore((s) => s.downloadMessage);
  const mpvError = useLibmpvStore((s) => s.downloadError);
  const mpvSpeed = useLibmpvStore((s) => s.downloadSpeedMbps);
  const mpvEta = useLibmpvStore((s) => s.downloadEtaSecs);
  const mpvStartDownload = useLibmpvStore((s) => s.startDownload);
  const mpvRefreshStatus = useLibmpvStore((s) => s.refreshStatus);
  const [mpvDeleting, setMpvDeleting] = useState(false);

  // === FFmpeg ===
  const ffStatus = useFfmpegStore((s) => s.status);
  const ffDownloading = useFfmpegStore((s) => s.downloading);
  const ffProgress = useFfmpegStore((s) => s.downloadProgress);
  const ffStage = useFfmpegStore((s) => s.downloadStage);
  const ffMessage = useFfmpegStore((s) => s.downloadMessage);
  const ffError = useFfmpegStore((s) => s.downloadError);
  const ffSpeed = useFfmpegStore((s) => s.downloadSpeedMbps);
  const ffEta = useFfmpegStore((s) => s.downloadEtaSecs);
  const ffStartDownload = useFfmpegStore((s) => s.startDownload);
  const ffRefreshStatus = useFfmpegStore((s) => s.refreshStatus);
  const [ffDeleting, setFfDeleting] = useState(false);

  const mpvStageLabel = mpvStage === "fetching" ? t("player.libmpvStageFetching")
    : mpvStage === "downloading" ? t("player.libmpvStageDownloading")
    : mpvStage === "extracting" ? t("player.libmpvStageExtracting")
    : mpvStage === "done" ? t("player.libmpvStageDone")
    : mpvStage === "failed" ? t("player.libmpvStageFailed", "下载失败")
    : t("player.libmpvStagePreparing");

  const ffStageLabel = ffStage === "downloading" ? t("subtitle.ffmpegRequired.downloading")
    : ffStage === "extracting" ? t("subtitle.ffmpegRequired.extracting")
    : ffStage === "done" ? t("subtitle.ffmpegRequired.done")
    : ffStage === "failed" ? t("subtitle.ffmpegRequired.failed")
    : t("player.libmpvStagePreparing");

  const handleMpvDelete = useCallback(async () => {
    setMpvDeleting(true);
    try {
      await api.deleteLibmpv();
      await mpvRefreshStatus();
      toast.success(t("settings.libmpvDeleted"));
    } catch (e: any) {
      toast.error(formatIpcError(e));
    } finally {
      setMpvDeleting(false);
    }
  }, [mpvRefreshStatus, t]);

  const handleFfDelete = useCallback(async () => {
    setFfDeleting(true);
    try {
      await api.deleteFfmpeg();
      await ffRefreshStatus();
      toast.success(t("settings.ffmpegDeleted", "FFmpeg 已删除"));
    } catch (e: any) {
      toast.error(formatIpcError(e));
    } finally {
      setFfDeleting(false);
    }
  }, [ffRefreshStatus, t]);

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{t("settings.player")}</h2>
        <p className="text-sm text-muted-foreground mt-1">{t("settings.playerDesc")}</p>
      </div>

      {/* FFmpeg 卡片 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">FFmpeg</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium">{t("settings.ffmpegStatus", "安装状态")}</p>
              <p className="text-xs text-muted-foreground">
                {ffStatus?.installed ? t("settings.ffmpegInstalled", "已安装") : t("settings.ffmpegNotInstalled", "未安装")}
              </p>
              {ffStatus?.path && <p className="text-xs text-muted-foreground font-mono mt-1">{ffStatus.path}</p>}
            </div>
            <div className="flex gap-2">
              <Button size="sm" onClick={() => ffStartDownload()} disabled={ffDownloading || ffStatus?.installed}>
                {ffDownloading ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : <Download className="mr-1 h-4 w-4" />}
                {t("settings.libmpvDownload")}
              </Button>
              {devMode && ffStatus?.installed && (
                <Button size="sm" variant="destructive" onClick={handleFfDelete} disabled={ffDeleting}>
                  {ffDeleting ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : <Trash2 className="mr-1 h-4 w-4" />}
                  {t("settings.libmpvDelete")}
                </Button>
              )}
            </div>
          </div>

          {/* 下载进度区域 */}
          {ffDownloading && (
            <div className="space-y-2 border-t pt-3">
              <div className="flex items-center justify-between text-xs">
                <span className="text-muted-foreground">{ffStageLabel}...</span>
                <span className="font-mono tabular-nums text-muted-foreground">
                  {ffProgress >= 0 ? `${ffProgress}%` : ""}
                </span>
              </div>
              {ffProgress >= 0 ? (
                <div className="h-2 bg-muted rounded-full overflow-hidden">
                  <div className="h-full bg-primary rounded-full transition-all duration-300" style={{ width: `${ffProgress}%` }} />
                </div>
              ) : (
                <div className="h-2 bg-muted rounded-full overflow-hidden relative">
                  <div className="h-full bg-primary rounded-full absolute" style={{ width: "40%", left: "-40%", animation: "indeterminate 1.5s infinite linear" }} />
                </div>
              )}
              <div className="flex justify-between text-xs text-muted-foreground tabular-nums">
                <span className="truncate">{ffStageLabel}</span>
                {ffStage === "downloading" && ffSpeed > 0 && (
                  <span className="shrink-0">{ffSpeed.toFixed(1)} MB/s · {formatEta(ffEta)}</span>
                )}
              </div>
            </div>
          )}

          {/* 下载失败错误提示 */}
          {!ffDownloading && ffError && (
            <div className="border-t pt-3">
              <p className="text-xs text-red-500 line-clamp-3">{ffError}</p>
            </div>
          )}
        </CardContent>
      </Card>

      {/* libmpv 卡片 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">libmpv</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium">{t("settings.libmpvStatus")}</p>
              <p className="text-xs text-muted-foreground">
                {mpvStatus?.downloaded ? t("settings.libmpvDownloaded") : t("settings.libmpvNotDownloaded")}
              </p>
              {mpvStatus?.path && <p className="text-xs text-muted-foreground font-mono mt-1">{mpvStatus.path}</p>}
            </div>
            <div className="flex gap-2">
              <Button size="sm" onClick={() => mpvStartDownload()} disabled={mpvDownloading || mpvStatus?.downloaded}>
                {mpvDownloading ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : <Download className="mr-1 h-4 w-4" />}
                {t("settings.libmpvDownload")}
              </Button>
              {devMode && mpvStatus?.downloaded && (
                <Button size="sm" variant="destructive" onClick={handleMpvDelete} disabled={mpvDeleting}>
                  {mpvDeleting ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : <Trash2 className="mr-1 h-4 w-4" />}
                  {t("settings.libmpvDelete")}
                </Button>
              )}
            </div>
          </div>

          {/* 下载进度区域 */}
          {mpvDownloading && (
            <div className="space-y-2 border-t pt-3">
              <div className="flex items-center justify-between text-xs">
                <span className="text-muted-foreground">{mpvStageLabel}...</span>
                <span className="font-mono tabular-nums text-muted-foreground">
                  {mpvProgress >= 0 ? `${mpvProgress}%` : ""}
                </span>
              </div>
              {mpvProgress >= 0 ? (
                <div className="h-2 bg-muted rounded-full overflow-hidden">
                  <div className="h-full bg-primary rounded-full transition-all duration-300" style={{ width: `${mpvProgress}%` }} />
                </div>
              ) : (
                <div className="h-2 bg-muted rounded-full overflow-hidden relative">
                  <div className="h-full bg-primary rounded-full absolute" style={{ width: "40%", left: "-40%", animation: "indeterminate 1.5s infinite linear" }} />
                </div>
              )}
              <div className="flex justify-between text-xs text-muted-foreground tabular-nums">
                <span className="truncate">{mpvStageLabel}</span>
                {mpvStage === "downloading" && mpvSpeed > 0 && (
                  <span className="shrink-0">{mpvSpeed.toFixed(1)} MB/s · {formatEta(mpvEta)}</span>
                )}
              </div>
            </div>
          )}

          {/* 下载失败错误提示 */}
          {!mpvDownloading && mpvError && (
            <div className="border-t pt-3">
              <p className="text-xs text-red-500 line-clamp-3">{mpvError}</p>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

// === 高级设置 ===
function AdvancedSettings() {
  const { t } = useTranslation();
  const [cacheCleared, setCacheCleared] = useState(false);

  const handleClearCache = useCallback(async () => {
    await api.clearTranslateCache();
    // 同时清除播放器图标缓存
    await api.clearPlayerIconsCache().catch((e) => warn("清除图标缓存失败:", e));
    setCacheCleared(true);
    setTimeout(() => setCacheCleared(false), 2000);
  }, []);

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{t("settings.advanced")}</h2>
        <p className="text-sm text-muted-foreground mt-1">{t("settings.advancedDesc", "缓存清理、右键菜单注册")}</p>
      </div>

      <ProxySettings />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.translateCacheTitle")}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium">{t("settings.clearCache")}</p>
              <p className="text-xs text-muted-foreground">{t("settings.clearCacheDesc")}</p>
            </div>
            <Button size="sm" variant="destructive" onClick={handleClearCache}>
              <Trash2 className="mr-1 h-4 w-4" />
              {t("settings.clearCache")}
            </Button>
          </div>
          {cacheCleared && <p className="text-sm text-green-600 mt-2"><Check className="inline h-4 w-4" /> ✓</p>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.contextMenu", "右键菜单")}</CardTitle>
        </CardHeader>
        <CardContent>
          <ContextMenuSettings />
        </CardContent>
      </Card>
    </div>
  );
}

// === 代理设置 ===
function ProxySettings() {
  const { t } = useTranslation();
  const [mode, setMode] = useState("none");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [hasPassword, setHasPassword] = useState(false);
  const [saving, setSaving] = useState(false);
  // 代理测试
  const [testUrl, setTestUrl] = useState("https://www.google.com");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);

  useEffect(() => {
    api.getProxy().then((cfg) => {
      setMode(cfg.mode);
      setHost(cfg.host);
      setPort(cfg.port);
      setUsername(cfg.username);
      setHasPassword(cfg.hasPassword);
    }).catch(() => {});
  }, []);

  const save = useCallback(async (newMode: string, newHost: string, newPort: string, newUser: string, newPass: string) => {
    // 延迟显示 spinner，避免快速保存时按钮闪烁
    const spinnerTimer = setTimeout(() => setSaving(true), 200);
    try {
      // 密码字段为占位符时不覆盖已有密码
      const passToSend = newPass === "••••••••" ? undefined : newPass;
      await api.setProxy(newMode, newHost, newPort, newUser || undefined, passToSend);
      if (passToSend === undefined) {
        // 保留 hasPassword 状态
      } else {
        setHasPassword(!!passToSend);
      }
      toast.success(t("settings.proxySaved"));
    } catch {
      toast.error(t("settings.proxySaveFailed"));
    } finally {
      clearTimeout(spinnerTimer);
      setSaving(false);
    }
  }, [t]);

  const handleTest = useCallback(async () => {
    setTesting(true);
    setTestResult(null);
    try {
      // 先保存当前配置，确保测试用的是最新代理设置
      const passToSend = password === "••••••••" ? undefined : password;
      await api.setProxy(mode, host, port, username || undefined, passToSend);
      const result = await api.testProxy(testUrl);
      setTestResult({
        ok: true,
        msg: t("settings.proxyTestOk", "连接成功，耗时 {{ms}}ms，状态码 {{status}}", { ms: result.elapsed_ms, status: result.status }),
      });
    } catch (e: any) {
      setTestResult({ ok: false, msg: formatIpcError(e) });
    } finally {
      setTesting(false);
    }
  }, [mode, host, port, username, password, testUrl, t]);

  const showProxyFields = mode !== "none";

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("settings.proxy", "网络代理")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <p className="text-sm font-medium">{t("settings.proxyMode", "代理模式")}</p>
            <p className="text-xs text-muted-foreground">{t("settings.proxyModeDesc", "用于提升部分翻译服务的网络连接稳定性")}</p>
          </div>
          <Select value={mode} onValueChange={(v) => {
            setMode(v);
            save(v, host, port, username, password);
          }}>
            <SelectTrigger className="w-32"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="none">{t("settings.proxyNone", "无")}</SelectItem>
              <SelectItem value="http">HTTP</SelectItem>
              <SelectItem value="socks5">SOCKS5</SelectItem>
            </SelectContent>
          </Select>
        </div>

        {showProxyFields && (
          <div className="border-t pt-3 space-y-3">
            <div className="grid grid-cols-3 gap-2">
              <div className="col-span-2">
                <label className="text-xs text-muted-foreground">{t("settings.proxyHost", "主机")}</label>
                <Input value={host} onChange={(e) => setHost(e.target.value)} placeholder="127.0.0.1" />
              </div>
              <div>
                <label className="text-xs text-muted-foreground">{t("settings.proxyPort", "端口")}</label>
                <Input value={port} onChange={(e) => setPort(e.target.value)} placeholder="7890" />
              </div>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <div>
                <label className="text-xs text-muted-foreground">{t("settings.proxyUser", "用户名（可选）")}</label>
                <Input value={username} onChange={(e) => setUsername(e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-muted-foreground">{t("settings.proxyPass", "密码（可选）")}</label>
                <Input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={hasPassword ? "••••••••" : ""}
                />
              </div>
            </div>
            <Button size="sm" disabled={saving} onClick={() => save(mode, host, port, username, password)}>
              {saving ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : null}
              {t("common.save", "保存")}
            </Button>

            {/* 代理测试 */}
            <div className="border-t pt-3 space-y-2">
              <label className="text-xs text-muted-foreground">{t("settings.proxyTestUrl", "测试网址")}</label>
              <div className="flex gap-2">
                <Input
                  value={testUrl}
                  onChange={(e) => setTestUrl(e.target.value)}
                  placeholder="https://www.google.com"
                  className="flex-1"
                />
                <Button size="sm" variant="secondary" disabled={testing} onClick={handleTest}>
                  {testing ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : null}
                  {t("settings.proxyTest", "测试连接")}
                </Button>
              </div>
              {testResult && (
                <p className={`text-xs ${testResult.ok ? "text-green-600" : "text-destructive"}`}>
                  {testResult.msg}
                </p>
              )}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function ContextMenuSettings() {
  const { t } = useTranslation();
  const [videoRegistered, setVideoRegistered] = useState(false);
  const [subtitleRegistered, setSubtitleRegistered] = useState(false);

  const refresh = useCallback(() => {
    api.isVideoMenuRegistered().then(setVideoRegistered).catch(() => {});
    api.isSubtitleMenuRegistered().then(setSubtitleRegistered).catch(() => {});
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const getExePath = useCallback(async () => {
    // 获取当前可执行文件路径
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      // 简化：使用空字符串让后端自行判断
      return "";
    } catch {
      return "";
    }
  }, []);

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm font-medium">{t("settings.videoContextMenu", "视频右键菜单")}</p>
          <p className="text-xs text-muted-foreground">{t("settings.videoContextMenuDesc", "右键视频文件添加\"快速翻译\"选项")}</p>
        </div>
        <Button
          size="sm"
          variant={videoRegistered ? "destructive" : "secondary"}
          onClick={async () => {
            if (videoRegistered) {
              await api.unregisterVideoMenu();
            } else {
              await api.registerVideoMenu(await getExePath());
            }
            refresh();
          }}
        >
          {videoRegistered ? t("settings.unregister", "注销") : t("settings.register", "注册")}
        </Button>
      </div>

      <div className="border-t pt-3 flex items-center justify-between">
        <div>
          <p className="text-sm font-medium">{t("settings.subtitleContextMenu", "字幕右键菜单")}</p>
          <p className="text-xs text-muted-foreground">{t("settings.subtitleContextMenuDesc", "右键字幕文件添加\"编辑字幕\"选项")}</p>
        </div>
        <Button
          size="sm"
          variant={subtitleRegistered ? "destructive" : "secondary"}
          onClick={async () => {
            if (subtitleRegistered) {
              await api.unregisterSubtitleMenu();
            } else {
              await api.registerSubtitleMenu(await getExePath());
            }
            refresh();
          }}
        >
          {subtitleRegistered ? t("settings.unregister", "注销") : t("settings.register", "注册")}
        </Button>
      </div>
    </div>
  );
}

// === 开发者选项 ===
function DeveloperSettings() {
  const { t } = useTranslation();
  const logApiEnabled = useDevModeStore((s) => s.logApiEnabled);
  const toggleLogApi = useDevModeStore((s) => s.toggleLogApi);
  const devMode = useDevModeStore((s) => s.devMode);
  const namePrecisionEnabled = useDevModeStore((s) => s.namePrecisionEnabled);
  const toggleNamePrecision = useDevModeStore((s) => s.toggleNamePrecision);
  const [crashDir, setCrashDir] = useState<string>("");
  const [crashCount, setCrashCount] = useState<number>(0);
  const [promptFailDir, setPromptFailDir] = useState<string>("");
  const [promptFailLogs, setPromptFailLogs] = useState<PromptFailLogEntry[]>([]);
  const [selectedLog, setSelectedLog] = useState<string | null>(null);
  const [logContent, setLogContent] = useState<string>("");
  const [loadingLog, setLoadingLog] = useState(false);
  const [apiDebugDir, setApiDebugDir] = useState<string>("");
  const [apiDebugCount, setApiDebugCount] = useState<number>(0);

  useEffect(() => {
    api.getCrashLogDir().then((dir) => {
      setCrashDir(dir);
      import("@tauri-apps/plugin-fs").then(({ readDir }) => {
        readDir(dir).then((entries) => {
          setCrashCount(entries.filter((e) => e.name?.endsWith(".log")).length);
        }).catch(() => setCrashCount(0));
      }).catch(() => setCrashCount(0));
    }).catch(() => {});

    // 加载 prompt 失败日志
    api.getPromptFailDir().then((dir) => {
      setPromptFailDir(dir);
    }).catch(() => {});
    api.listPromptFailLogs().then((logs) => {
      setPromptFailLogs(logs);
    }).catch(() => {});

    // 加载 API 调试日志目录和文件列表
    api.getApiDebugDir().then((dir) => {
      setApiDebugDir(dir);
    }).catch(() => {});
    api.listApiDebugLogs().then((logs) => {
      setApiDebugCount(logs.length);
    }).catch(() => setApiDebugCount(0));
  }, []);

  const refreshPromptFailLogs = useCallback(() => {
    api.listPromptFailLogs().then((logs) => {
      setPromptFailLogs(logs);
    }).catch(() => {});
  }, []);

  const handleOpenCrashDir = useCallback(async () => {
    if (!crashDir) return;
    try {
      await api.openPath(crashDir);
    } catch (e) {
      toast.error(t("settings.openCrashDirFailed", "打开目录失败"));
    }
  }, [crashDir, t]);

  const handleOpenDevtools = useCallback(async () => {
    try {
      await api.toggleDevtools(true);
      toast.success(t("settings.devtoolsOpened", "DevTools 已打开"));
    } catch (e) {
      toast.error(t("settings.devtoolsFailed", "打开 DevTools 失败"));
    }
  }, [t]);

  const handleOpenPromptFailDir = useCallback(async () => {
    if (!promptFailDir) return;
    try {
      await api.openPath(promptFailDir);
    } catch (e) {
      toast.error(t("settings.openPromptFailDirFailed", "打开目录失败"));
    }
  }, [promptFailDir, t]);

  const handleOpenApiDebugDir = useCallback(async () => {
    if (!apiDebugDir) return;
    try {
      await api.openPath(apiDebugDir);
    } catch (e) {
      toast.error(t("settings.openApiDebugDirFailed", "打开目录失败"));
    }
  }, [apiDebugDir, t]);

  const refreshCrashCount = useCallback(() => {
    if (!crashDir) return;
    import("@tauri-apps/plugin-fs").then(({ readDir }) => {
      readDir(crashDir).then((entries) => {
        setCrashCount(entries.filter((e) => e.name?.endsWith(".log")).length);
      }).catch(() => setCrashCount(0));
    }).catch(() => setCrashCount(0));
  }, [crashDir]);

  const handleClearCrashLogs = useCallback(async () => {
    try {
      const n = await api.clearCrashLogs();
      refreshCrashCount();
      toast.success(t("settings.clearLogsOk", "已清空 {{count}} 个日志", { count: n }));
    } catch (e) {
      toast.error(t("settings.clearLogsFailed", "清空失败"));
    }
  }, [refreshCrashCount, t]);

  const handleClearPromptFailLogs = useCallback(async () => {
    try {
      const n = await api.clearPromptFailLogs();
      refreshPromptFailLogs();
      toast.success(t("settings.clearLogsOk", "已清空 {{count}} 个日志", { count: n }));
    } catch (e) {
      toast.error(t("settings.clearLogsFailed", "清空失败"));
    }
  }, [refreshPromptFailLogs, t]);

  const handleClearApiDebugLogs = useCallback(async () => {
    try {
      const n = await api.clearApiDebugLogs();
      api.listApiDebugLogs().then((logs) => setApiDebugCount(logs.length)).catch(() => setApiDebugCount(0));
      toast.success(t("settings.clearLogsOk", "已清空 {{count}} 个日志", { count: n }));
    } catch (e) {
      toast.error(t("settings.clearLogsFailed", "清空失败"));
    }
  }, [t]);

  const handleViewLog = useCallback(async (name: string) => {
    setSelectedLog(name);
    setLoadingLog(true);
    setLogContent("");
    try {
      const content = await api.readPromptFailLog(name);
      setLogContent(content);
    } catch (e) {
      toast.error(t("settings.readPromptFailFailed", "读取日志失败"));
      setLogContent(t("settings.readPromptFailFailed", "读取日志失败"));
    } finally {
      setLoadingLog(false);
    }
  }, [t]);

  const handleDeleteLog = useCallback(async (name: string) => {
    try {
      await api.deletePromptFailLog(name);
      toast.success(t("settings.deletePromptFailOk", "已删除"));
      if (selectedLog === name) {
        setSelectedLog(null);
        setLogContent("");
      }
      refreshPromptFailLogs();
    } catch (e) {
      toast.error(t("settings.deletePromptFailFailed", "删除失败"));
    }
  }, [selectedLog, refreshPromptFailLogs, t]);

  const formatTime = (ts: number) => {
    if (!ts) return "";
    const d = new Date(ts * 1000);
    return d.toLocaleString();
  };

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  };

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{t("settings.developer", "开发者选项")}</h2>
        <p className="text-sm text-muted-foreground mt-1">{t("settings.developerDesc", "调试与诊断工具")}</p>
      </div>

      {/* 崩溃日志 */}
      <Card>
        <CardContent className="pt-6 space-y-3">
          <div className="flex items-center gap-2">
            <Bug className="h-5 w-5 text-muted-foreground" />
            <h3 className="text-base font-medium">{t("settings.crashLogs", "崩溃日志")}</h3>
          </div>
          <p className="text-sm text-muted-foreground">
            {t("settings.crashLogsDesc", "程序崩溃时自动生成日志文件，用于诊断问题。")}
          </p>
          {crashDir && (
            <p className="text-xs text-muted-foreground font-mono break-all bg-muted/50 rounded px-2 py-1">
              {crashDir}
            </p>
          )}
          <div className="flex items-center justify-between">
            <span className="text-xs text-muted-foreground">
              {crashCount > 0
                ? t("settings.crashCount", { count: crashCount, defaultValue: "{{count}} 个崩溃日志" })
                : t("settings.noCrashes", "暂无崩溃日志")}
            </span>
            <div className="flex gap-2">
              <Button size="sm" variant="outline" onClick={handleClearCrashLogs} disabled={!crashDir || crashCount === 0}>
                <Trash2 className="h-4 w-4 mr-1" />
                {t("settings.clearLogs", "清空")}
              </Button>
              <Button size="sm" variant="outline" onClick={handleOpenCrashDir} disabled={!crashDir}>
                <FolderOpen className="h-4 w-4 mr-1" />
                {t("settings.openCrashDir", "打开目录")}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Prompt 失败日志 */}
      <Card>
        <CardContent className="pt-6 space-y-3">
          <div className="flex items-center gap-2">
            <FileText className="h-5 w-5 text-muted-foreground" />
            <h3 className="text-base font-medium">{t("settings.promptFailLogs", "Prompt 失败日志")}</h3>
          </div>
          <p className="text-sm text-muted-foreground">
            {t("settings.promptFailLogsDesc", "翻译对齐失败时自动记录发送的 prompt 和模型返回内容，用于诊断翻译问题。")}
          </p>
          {promptFailDir && (
            <p className="text-xs text-muted-foreground font-mono break-all bg-muted/50 rounded px-2 py-1">
              {promptFailDir}
            </p>
          )}
          <div className="flex items-center justify-between">
            <span className="text-xs text-muted-foreground">
              {promptFailLogs.length > 0
                ? t("settings.promptFailCount", { count: promptFailLogs.length, defaultValue: "{{count}} 个失败日志" })
                : t("settings.noPromptFails", "暂无失败日志")}
            </span>
            <div className="flex gap-2">
              <Button size="sm" variant="outline" onClick={refreshPromptFailLogs}>
                <RefreshCw className="h-4 w-4 mr-1" />
                {t("settings.refresh", "刷新")}
              </Button>
              <Button size="sm" variant="outline" onClick={handleClearPromptFailLogs} disabled={!promptFailDir || promptFailLogs.length === 0}>
                <Trash2 className="h-4 w-4 mr-1" />
                {t("settings.clearLogs", "清空")}
              </Button>
              <Button size="sm" variant="outline" onClick={handleOpenPromptFailDir} disabled={!promptFailDir}>
                <FolderOpen className="h-4 w-4 mr-1" />
                {t("settings.openPromptFailDir", "打开目录")}
              </Button>
            </div>
          </div>
          {/* 日志列表 */}
          {promptFailLogs.length > 0 && (
            <div className="border rounded-md max-h-48 overflow-y-auto">
              {promptFailLogs.map((log) => (
                <div
                  key={log.name}
                  className={cn(
                    "flex items-center justify-between px-3 py-2 text-xs border-b last:border-b-0 cursor-pointer hover:bg-muted/50",
                    selectedLog === log.name && "bg-muted"
                  )}
                  onClick={() => handleViewLog(log.name)}
                >
                  <div className="flex-1 min-w-0">
                    <span className="font-mono truncate block">{log.name}</span>
                    <span className="text-muted-foreground">{formatTime(log.modified)} · {formatSize(log.size)}</span>
                  </div>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-6 px-2 text-destructive"
                    onClick={(e) => { e.stopPropagation(); handleDeleteLog(log.name); }}
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              ))}
            </div>
          )}
          {/* 日志内容查看 */}
          {selectedLog && (
            <div className="border rounded-md">
              <div className="flex items-center justify-between px-3 py-2 border-b bg-muted/30">
                <span className="text-xs font-mono truncate">{selectedLog}</span>
                <Button size="sm" variant="ghost" className="h-6 px-2" onClick={() => { setSelectedLog(null); setLogContent(""); }}>
                  <X className="h-3 w-3" />
                </Button>
              </div>
              <div className="p-3 max-h-96 overflow-y-auto">
                {loadingLog ? (
                  <div className="flex items-center justify-center py-4">
                    <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                  </div>
                ) : (
                  <pre className="text-xs font-mono whitespace-pre-wrap break-all">{logContent}</pre>
                )}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* API 调试日志 */}
      <Card>
        <CardContent className="pt-6 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <FileText className="h-5 w-5 text-muted-foreground" />
              <h3 className="text-base font-medium">{t("settings.apiDebugLogs", "翻译日志")}</h3>
            </div>
            <input
              type="checkbox"
              checked={logApiEnabled}
              onChange={() => toggleLogApi()}
              disabled={!devMode}
              className="h-4 w-4 rounded border-gray-300"
            />
          </div>
          <p className="text-sm text-muted-foreground">
            {t("settings.apiDebugLogsDesc", "开启后记录所有翻译 API 的请求和响应数据。仅在开发者模式下生效。")}
          </p>
          {!devMode && (
            <p className="text-xs text-orange-600">
              {t("settings.apiDebugRequiresDevMode", "需先开启开发者模式才能使用此功能")}
            </p>
          )}
          {apiDebugDir && (
            <p className="text-xs text-muted-foreground font-mono break-all bg-muted/50 rounded px-2 py-1">
              {apiDebugDir}
            </p>
          )}
          <div className="flex items-center justify-between">
            <span className="text-xs text-muted-foreground">
              {apiDebugCount > 0
                ? t("settings.apiDebugCount", { count: apiDebugCount, defaultValue: "{{count}} 个调试日志" })
                : t("settings.noApiDebugLogs", "暂无调试日志")}
            </span>
            <div className="flex gap-2">
              <Button size="sm" variant="outline" onClick={handleClearApiDebugLogs} disabled={!apiDebugDir || apiDebugCount === 0}>
                <Trash2 className="h-4 w-4 mr-1" />
                {t("settings.clearLogs", "清空")}
              </Button>
              <Button size="sm" variant="outline" onClick={handleOpenApiDebugDir} disabled={!apiDebugDir}>
                <FolderOpen className="h-4 w-4 mr-1" />
                {t("settings.openApiDebugDir", "打开文件夹")}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* 人名精译 */}
      <Card>
        <CardContent className="pt-6 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Languages className="h-5 w-5 text-muted-foreground" />
              <h3 className="text-base font-medium">{t("settings.namePrecision", "人名精译")}</h3>
            </div>
            <input
              type="checkbox"
              checked={namePrecisionEnabled}
              onChange={() => toggleNamePrecision()}
              className="h-4 w-4 rounded border-gray-300"
            />
          </div>
          <p className="text-sm text-muted-foreground">
            {t("settings.namePrecisionDesc", "翻译前自动扫描字幕提取人名，建立统一译名表注入每个翻译批次，保证跨批次人名一致。同时要求 AI 在译文中标记人名，翻译后自动检测不一致并修正。")}
          </p>
          <div className="space-y-1 text-xs text-muted-foreground">
            <p>• {t("settings.namePrecisionFlow1", "预扫描：翻译前用一次 API 调用从全部字幕中提取人名，按模型大小自动分段")}</p>
            <p>• {t("settings.namePrecisionFlow2", "译名表注入：提取的人名表注入每个翻译批次的 system prompt，所有批次使用同一份译名表")}</p>
            <p>• {t("settings.namePrecisionFlow3", "人名标记：AI 在译文中用 <name=Kaleb>卡莱布</name> 标记人名，翻译后自动检测不一致")}</p>
            <p>• {t("settings.namePrecisionFlow4", "后处理修正：发现同一人名的多个译名时，按频率选定标准译名，全局替换并剥离标签")}</p>
          </div>
          <div className="space-y-1 text-xs">
            <p className="text-orange-600">
              {t("settings.namePrecisionCost", "额外开销：翻译前多一次 API 调用（约 3-15 秒），翻译时多约 5% token 消耗（人名标记标签）")}
            </p>
            <p className="text-green-600">
              {t("settings.namePrecisionBenefit", "优点：彻底解决跨批次人名翻译不一致问题，尤其适合教学/纪录片等分批引入人名的长视频")}
            </p>
            <p className="text-muted-foreground">
              {t("settings.namePrecisionNote", "仅 AI 翻译引擎支持。启用后退出开发模式仍保持启用，可在下方开关随时关闭。")}
            </p>
          </div>
        </CardContent>
      </Card>

      {/* DevTools */}
      <Card>
        <CardContent className="pt-6 space-y-3">
          <div className="flex items-center gap-2">
            <Terminal className="h-5 w-5 text-muted-foreground" />
            <h3 className="text-base font-medium">{t("settings.devtoolsTitle", "开发者工具")}</h3>
          </div>
          <p className="text-sm text-muted-foreground">
            {t("settings.devtoolsDesc", "打开浏览器开发者工具，查看控制台日志、网络请求和元素。")}
          </p>
          <Button size="sm" variant="outline" onClick={handleOpenDevtools}>
            <Terminal className="h-4 w-4 mr-1" />
            {t("settings.openDevtools", "打开 DevTools")}
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}

// === 关于 ===
function AboutSettings() {
  const { t } = useTranslation();
  const devMode = useDevModeStore((s) => s.devMode);
  const toggleDevMode = useDevModeStore((s) => s.toggle);
  const [clickCount, setClickCount] = useState(0);
  const checkManually = useUpdateStore((s) => s.checkManually);
  const updateChecking = useUpdateStore((s) => s.checking);
  const [updateResult, setUpdateResult] = useState<"latest" | "failed" | null>(null);

  const handleCheckUpdate = useCallback(async () => {
    setUpdateResult(null);
    const result = await checkManually();
    if (result === "latest") setUpdateResult("latest");
    else if (result === "failed") setUpdateResult("failed");
    // available 时弹窗会自动打开，不需要在这里处理
    setTimeout(() => setUpdateResult(null), 3000);
  }, [checkManually]);

  const handleVersionClick = useCallback(() => {
    const next = clickCount + 1;
    if (next >= 7) {
      void toggleDevMode();
      setClickCount(0);
    } else {
      setClickCount(next);
      const remaining = 7 - next;
      if (remaining <= 3 && remaining > 0) {
        toast.info(devMode
          ? t("settings.devModeDisableHint", { count: remaining })
          : t("settings.devModeEnableHint", { count: remaining }));
      }
    }
  }, [clickCount, devMode, toggleDevMode, t]);

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{t("settings.about")}</h2>
        <p className="text-sm text-muted-foreground mt-1">{t("settings.aboutDesc")}</p>
      </div>

      <Card>
        <CardContent className="pt-6 space-y-3 text-center">
          <div className="mx-auto h-16 w-16 rounded-lg bg-primary/10 flex items-center justify-center">
            <Languages className="h-8 w-8 text-primary" />
          </div>
          <h3 className="text-lg font-semibold">AI-SubTrans</h3>
          <p
            className="text-sm text-muted-foreground select-none"
            onClick={handleVersionClick}
          >
            v1.0.0 (zimufan)
          </p>
          <p className="text-sm text-muted-foreground">{t("settings.aboutTagline")}</p>
          <div className="border-t pt-3 text-xs text-muted-foreground space-y-1">
            <p>Powered by Tauri + React + ass-rs</p>
            <p>FFmpeg · libmpv · OpenSubtitles</p>
          </div>
          {/* 检查更新 */}
          <div className="border-t pt-3 flex flex-col items-center gap-2">
            <Button size="sm" variant="outline" onClick={handleCheckUpdate} disabled={updateChecking}>
              {updateChecking ? <Loader2 className="h-4 w-4 mr-1 animate-spin" /> : <RefreshCw className="h-4 w-4 mr-1" />}
              {t("update.checkButton")}
            </Button>
            {updateResult === "latest" && (
              <p className="text-xs text-green-600">{t("update.alreadyLatest")}</p>
            )}
            {updateResult === "failed" && (
              <p className="text-xs text-red-500">{t("update.checkFailed")}</p>
            )}
          </div>
          {devMode && (
            <p className="text-xs text-amber-600 font-medium pt-2 border-t">
              {t("settings.devModeEnabled")}
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

// === SECTION 4 END ===
