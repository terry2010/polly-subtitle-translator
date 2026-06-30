import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useNavigate, useSearchParams } from "react-router-dom";
import { getCurrentWindow, LogicalSize, LogicalPosition } from "@tauri-apps/api/window";
import { setWindowSizeInitialized } from "./MainView";
import { ArrowLeft, Check, Loader2, Download, Trash2, ExternalLink, Settings as SettingsIcon, Languages, Search, Film, Wrench, Info, RefreshCw } from "lucide-react";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "../components/ui/select";
import { useThemeStore } from "../stores/themeStore";
import { useDevModeStore } from "../stores/devModeStore";
import { useLibmpvStore } from "../stores/libmpvStore";
import { useFfmpegStore } from "../stores/ffmpegStore";
import { useUpdateStore } from "../stores/updateStore";
import { api, formatIpcError } from "../lib/api";

type SettingsTab = "general" | "translate" | "search" | "player" | "advanced" | "about";

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
    : searchParams.get("tab") === "advanced" ? "advanced"
    : "general"
  );

  const navItems: { key: SettingsTab; label: string; icon: React.ReactNode }[] = [
    { key: "general", label: t("settings.general"), icon: <SettingsIcon className="h-4 w-4" /> },
    { key: "translate", label: t("settings.translateApi"), icon: <Languages className="h-4 w-4" /> },
    { key: "search", label: t("settings.subtitleSearch"), icon: <Search className="h-4 w-4" /> },
    { key: "player", label: t("settings.player"), icon: <Film className="h-4 w-4" /> },
    { key: "advanced", label: t("settings.advanced"), icon: <Wrench className="h-4 w-4" /> },
    { key: "about", label: t("settings.about"), icon: <Info className="h-4 w-4" /> },
  ];

  return (
    <div className="flex h-screen flex-col">
      <div className="flex flex-1 overflow-hidden">
        {/* 左侧导航 */}
        <nav className="w-48 border-r bg-muted/30 p-2 space-y-1">
          {/* 返回项：固定在导航顶部 */}
          <button
            onClick={() => navigate("/")}
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

        {/* 右侧内容 */}
        <div className="flex-1 overflow-auto p-6">
          <div className="mx-auto max-w-2xl">
            {activeTab === "general" && (
              <GeneralSettings theme={theme} setTheme={setTheme} language={language} setLanguage={setLanguage} />
            )}
            {activeTab === "translate" && <TranslateApiSettings />}
            {activeTab === "search" && <SearchSettings />}
            {activeTab === "player" && <PlayerSettings />}
            {activeTab === "advanced" && <AdvancedSettings />}
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
const PROVIDER_LINKS: Record<string, { url: string; appIdLabel?: string; appIdPlaceholder?: string; hasRegion?: boolean }> = {
  baidu: {
    url: "https://fanyi-api.baidu.com/",
    appIdLabel: "App ID",
    appIdPlaceholder: "百度翻译 App ID",
  },
  bing: {
    url: "https://learn.microsoft.com/azure/cognitive-services/translator/",
    appIdLabel: "API Key",
    appIdPlaceholder: "Azure Translator API Key",
    hasRegion: true,
  },
  google: {
    url: "https://cloud.google.com/translate/docs/",
    appIdLabel: "API Key",
    appIdPlaceholder: "Google Cloud Translation API Key",
  },
};

function TranslateApiSettings() {
  const { t } = useTranslation();
  const [provider, setProvider] = useState("baidu");
  const [searchParams] = useSearchParams();
  const [appId, setAppId] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [region, setRegion] = useState("global");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<"ok" | "fail" | null>(null);
  const [testError, setTestError] = useState("");
  const [saved, setSaved] = useState(false);
  const [loading, setLoading] = useState(true);

  // 加载已保存的配置（URL 参数优先）
  useEffect(() => {
    const paramProvider = searchParams.get("provider");
    if (paramProvider) {
      setProvider(paramProvider);
      return;
    }
    api.getConfig("translate_provider").then((v) => {
      if (v) setProvider(v);
    });
  }, [searchParams]);

  useEffect(() => {
    setLoading(true);
    setAppId("");
    setSecretKey("");
    setRegion("global");
    Promise.all([
      api.getConfig(`translate_${provider}_app_id`).catch(() => null),
      api.getConfig(`translate_${provider}_region`).catch(() => null),
      api.getCredential(provider, "secret").catch(() => null),
    ]).then(([savedAppId, savedRegion, savedSecretKeyring]) => {
      if (savedAppId) setAppId(savedAppId);
      if (savedRegion) setRegion(savedRegion);
      // 凭据仅从 keyring 读取（不降级到明文数据库）
      if (savedSecretKeyring) setSecretKey("••••••••");
      setLoading(false);
    });
  }, [provider]);

  const handleSave = useCallback(async () => {
    try {
      await api.setConfig("translate_provider", provider);
      await api.setConfig(`translate_${provider}_app_id`, appId);
      await api.setConfig(`translate_${provider}_region`, region);
      if (secretKey && secretKey !== "••••••••") {
        // 凭据仅存 keyring（系统密钥环），不写入明文数据库
        try {
          await api.saveCredential(provider, "secret", secretKey);
        } catch (e) {
          console.error("keyring 保存失败:", e);
          throw e;
        }
      }
      setSaved(true);
      setTimeout(() => setSaved(false), 3000);
    } catch (e: any) {
      console.error("保存失败:", e);
    }
  }, [provider, appId, secretKey, region]);

  const handleTest = useCallback(async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const actualSecret = secretKey === "••••••••" ? undefined : secretKey;
      await api.testTranslateConnection(provider, appId || undefined, actualSecret, region || undefined);
      setTestResult("ok");
    } catch (e: any) {
      setTestResult("fail");
      const msg = formatIpcError(e);
      setTestError(msg);
    } finally {
      setTesting(false);
    }
  }, [provider, appId, secretKey, region]);

  const providerInfo = PROVIDER_LINKS[provider] ?? PROVIDER_LINKS.baidu;

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{t("settings.translateApi")}</h2>
        <p className="text-sm text-muted-foreground mt-1">{t("settings.translateApiDesc", "配置翻译服务凭据，支持百度/必应/谷歌")}</p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("translate.provider")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <label className="text-sm font-medium">{t("translate.provider")}</label>
              <p className="text-xs text-muted-foreground">{t("settings.providerDesc", "选择翻译服务提供商")}</p>
            </div>
            <Select value={provider} onValueChange={setProvider}>
              <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="baidu">{t("settings.baidu")}</SelectItem>
                <SelectItem value="bing">{t("settings.bing")}</SelectItem>
                <SelectItem value="google">{t("settings.google")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* 获取 API 链接 */}
          <a
            href={providerInfo.url}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
          >
            {t("settings.getApiKeyPrefix")} {t(`settings.${provider}`)} API Key
            <ExternalLink className="h-3 w-3" />
          </a>

          {/* App ID / API Key */}
          {providerInfo.appIdLabel && (
            <div>
              <label className="text-sm font-medium">{providerInfo.appIdLabel}</label>
              <p className="text-xs text-muted-foreground mb-1">{t("settings.appIdDesc", "翻译服务的 App ID / API Key")}</p>
              <Input
                value={appId}
                onChange={(e) => setAppId(e.target.value)}
                placeholder={providerInfo.appIdPlaceholder}
                disabled={loading}
              />
            </div>
          )}

          {/* Secret Key */}
          <div>
            <label className="text-sm font-medium">{t("settings.secretKey")}</label>
            <p className="text-xs text-muted-foreground mb-1">{t("settings.secretKeyDesc", "API 密钥，加密存储在系统密钥环")}</p>
            <div className="flex gap-2">
              <Input
                type="password"
                value={secretKey}
                onChange={(e) => setSecretKey(e.target.value)}
                placeholder="Secret Key"
                disabled={loading}
              />
              {secretKey === "••••••••" && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setSecretKey("")}
                >
                  {t("settings.edit", "修改")}
                </Button>
              )}
            </div>
          </div>

          {/* Region（仅 Bing） */}
          {providerInfo.hasRegion && (
            <div>
              <label className="text-sm font-medium">{t("settings.region")}</label>
              <p className="text-xs text-muted-foreground mb-1">{t("settings.regionDesc", "Azure 区域，如 global 或 china")}</p>
              <Input value={region} onChange={(e) => setRegion(e.target.value)} placeholder="global / china" />
            </div>
          )}

          {/* 保存 + 测试 */}
          <div className="flex items-center gap-3 pt-2">
            <Button size="sm" onClick={handleSave} disabled={loading}>
              {t("common.save")}
            </Button>
            <Button size="sm" variant="secondary" onClick={handleTest} disabled={testing || loading}>
              {testing ? <Loader2 className="mr-1 h-4 w-4 animate-spin" /> : null}
              {t("settings.testConnection")}
            </Button>
            {saved && (
              <span className="flex items-center gap-1 text-sm text-green-600">
                <Check className="h-4 w-4" /> {t("settings.saved", "已保存")}
              </span>
            )}
            {testResult === "ok" && (
              <span className="flex items-center gap-1 text-sm text-green-600">
                <Check className="h-4 w-4" /> {t("settings.testSuccess")}
              </span>
            )}
            {testResult === "fail" && (
              <span className="text-sm text-destructive">
                {t("settings.testFailed", { detail: testError })}
              </span>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

// === SECTION 3 END ===

// === 字幕搜索设置 ===
function SearchSettings() {
  const { t } = useTranslation();
  const [apiKey, setApiKey] = useState("");
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api.getCredential("opensubtitles", "api_key").then((v) => v && setApiKey("••••••••"));
  }, []);

  const handleSave = useCallback(async () => {
    if (apiKey && apiKey !== "••••••••") {
      await api.saveCredential("opensubtitles", "api_key", apiKey);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    }
  }, [apiKey]);

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xl font-semibold">{t("settings.subtitleSearch")}</h2>
        <p className="text-sm text-muted-foreground mt-1">{t("settings.searchDesc", "配置 OpenSubtitles API 密钥以启用在线字幕搜索")}</p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">OpenSubtitles</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div>
            <label className="text-sm font-medium">{t("settings.openSubtitlesApiKey")}</label>
            <p className="text-xs text-muted-foreground mb-1">{t("settings.apiKeyDesc", "在 opensubtitles.com 注册获取 API 密钥")}</p>
            <Input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="OpenSubtitles API Key" />
          </div>
          <div className="flex items-center gap-3">
            <Button size="sm" onClick={handleSave}>{t("common.save")}</Button>
            {saved && <span className="text-sm text-green-600"><Check className="inline h-4 w-4" /> {t("settings.testSuccess")}</span>}
            <a href="https://www.opensubtitles.com/consumers" target="_blank" rel="noreferrer" className="ml-auto text-xs text-primary hover:underline flex items-center gap-1">
              {t("settings.getApiKey")} <ExternalLink className="h-3 w-3" />
            </a>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

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
                <span className="truncate">{ffMessage}</span>
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
                <span className="truncate">{mpvMessage}</span>
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
    setSaving(true);
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
      setSaving(false);
    }
  }, [t]);

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
            <p className="text-xs text-muted-foreground">{t("settings.proxyModeDesc", "用于 Google 翻译等需翻墙服务")}</p>
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
