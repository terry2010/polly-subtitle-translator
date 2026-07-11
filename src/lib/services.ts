// 翻译服务注册表（前端静态数据）
// 额度/价格数据采集于 2026-07，以各服务商官网为准

export type ServiceCategory = "traditional" | "ai";

export interface ServiceDef {
  /** 唯一标识。传统服务时为 provider key（"baidu"）；AI 服务时为 service_id（provider 恒为 "openai"）。
   *  约束：id 只允许 [a-z0-9_]+，不含 | / : / / 等特殊字符
   *  （用于缓存 key 拼接和下拉 value 编码，避免歧义） */
  id: string;
  /** 显示名称 */
  name: string;
  /** 分类 */
  category: ServiceCategory;
  /** 免费额度描述（如 "每月500万字符" / "GLM-4-Flash 无限免费"） */
  freeQuota: string;
  /** 超出后价格（如 "58元/100万字符" / "2元/百万tokens"） */
  price: string;
  /** 是否完全免费（Groq / Ollama / LM Studio），显示「完全免费」 */
  completelyFree: boolean;
  /** 是否有免费额度但非完全免费（智谱 / 硅基 / 混元），显示「🆓 有免费额度」 */
  hasFreeTier: boolean;
  /** API 文档/申请链接 */
  docUrl: string;
  /** 一句话简介（用于左侧列表卡片） */
  description: string;
  /** 是否待开发（传统翻译私有协议未实现时为 true） */
  comingSoon: boolean;
  /** 是否必须配置 api_key（Ollama/LM Studio 为 false，其余 AI 服务为 true；传统翻译忽略此字段） */
  requiresApiKey: boolean;

  // 传统翻译字段
  appIdLabel?: string;
  appIdPlaceholder?: string;
  hasRegion?: boolean;

  // AI 大模型字段（OpenAI 兼容）
  /** 预置 base_url，默认填入但用户可修改；约定末尾不带斜杠 */
  presetBaseUrl?: string;
  /** 预置 QPS 上限（每秒请求数，注意 RPM 限流的服务需除以 60 换算）；custom 时由用户填写 */
  presetQps?: number;
  /** 是否走 OpenAi provider */
  isOpenAiCompatible?: boolean;
  /** 模型推荐说明（可选，显示在 AI 配置面板标题下方） */
  modelRecommendation?: string;
}

// === SECTION 1 END ===

export const SERVICES: ServiceDef[] = [
  // ── 传统翻译 ──
  { id: "baidu",      name: "百度翻译",   category: "traditional", freeQuota: "每月100万字符", price: "49元/100万字符", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: false, docUrl: "https://fanyi-api.baidu.com/", description: "支持中英等多语言，每月 100 万字符免费", appIdLabel: "App ID", appIdPlaceholder: "百度翻译 App ID", presetQps: 1 },
  { id: "bing",       name: "Microsoft",  category: "traditional", freeQuota: "每月200万字符", price: "10美元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://learn.microsoft.com/azure/cognitive-services/translator/", description: "Azure Translator，每月 200 万字符免费", appIdLabel: "API Key", appIdPlaceholder: "Azure Translator API Key", hasRegion: true, presetQps: 10 },
  { id: "google",     name: "Google",     category: "traditional", freeQuota: "每月50万字符",  price: "20美元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://cloud.google.com/translate/docs/", description: "Google Cloud Translate，每月 50 万字符免费", appIdLabel: "API Key", appIdPlaceholder: "Google Cloud Translation API Key", presetQps: 10 },
  // 以下传统翻译为「待开发」
  { id: "tencent",    name: "腾讯翻译君", category: "traditional", freeQuota: "每月500万字符", price: "58元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://cloud.tencent.com/product/tmt", description: "每月 500 万字符免费，支持多语种", appIdLabel: "SecretId", appIdPlaceholder: "腾讯云 SecretId", presetQps: 5 },
  { id: "volcengine", name: "火山翻译",   category: "traditional", freeQuota: "每月200万字符", price: "49元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://www.volcengine.com/product/translate", description: "字节跳动翻译，每月 200 万字符免费", appIdLabel: "Access Key ID", appIdPlaceholder: "火山引擎 Access Key ID", presetQps: 5 },
  { id: "aliyun",     name: "阿里翻译",   category: "traditional", freeQuota: "每月100万字符", price: "50元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://www.aliyun.com/product/ai/alimt", description: "阿里云机器翻译，每月 100 万字符免费", appIdLabel: "Access Key ID", appIdPlaceholder: "阿里云 Access Key ID", presetQps: 50 },
  { id: "deepl",      name: "DeepL",      category: "traditional", freeQuota: "每月50万字符",  price: "4.99€/月+20€/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://www.deepl.com/pro-api", description: "翻译质量高，每月 50 万字符免费", appIdLabel: "Auth Key", appIdPlaceholder: "DeepL Auth Key", presetQps: 5 },
  { id: "youdao",     name: "有道翻译",   category: "traditional", freeQuota: "无", price: "48元/100万字符", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: false, docUrl: "https://ai.youdao.com/", description: "网易有道翻译 API", appIdLabel: "App ID", appIdPlaceholder: "有道翻译 App ID", presetQps: 1 },
  { id: "caiyun",     name: "彩云小译",   category: "traditional", freeQuota: "无", price: "39元/100万字符", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: false, docUrl: "https://dashboard.caiyunapp.com/", description: "彩云科技翻译 API", appIdLabel: "Token", appIdPlaceholder: "彩云小译 Token", presetQps: 5 },
  { id: "niutrans",   name: "小牛翻译",   category: "traditional", freeQuota: "每日20万字符", price: "500元/1000万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://niutrans.com/", description: "每日 20 万字符免费", appIdLabel: "API Key", appIdPlaceholder: "小牛翻译 API Key", presetQps: 5 },
  { id: "amazon",     name: "Amazon 翻译", category: "traditional", freeQuota: "每月200万字符", price: "15美元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://aws.amazon.com/translate/", description: "AWS Translate，每月 200 万字符免费", appIdLabel: "Access Key ID", appIdPlaceholder: "AWS Access Key ID", hasRegion: true, presetQps: 10 },

  // ── AI 大模型（全部 OpenAI 兼容，复用 OpenAi provider）──
  { id: "deepseek",   name: "DeepSeek",   category: "ai", freeQuota: "无", price: "2元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://platform.deepseek.com/", description: "推理强、价格低，适合长文本翻译", presetBaseUrl: "https://api.deepseek.com/v1", presetQps: 10, isOpenAiCompatible: true },
  { id: "zhipu",      name: "智谱GLM",    category: "ai", freeQuota: "GLM-4.7-Flash 无限免费（QPS=1）", price: "-", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: true,  docUrl: "https://www.bigmodel.cn/invite?icode=s0SYii89O7V66qkC26gLA%2Bnfet45IvM%2BqDogImfeLyI%3D", description: "GLM-4.7-Flash 无限免费，QPS=1", presetBaseUrl: "https://open.bigmodel.cn/api/paas/v4", presetQps: 1, isOpenAiCompatible: true },
  { id: "siliconflow",name: "硅基流动",   category: "ai", freeQuota: "多种免费小参数模型", price: "-", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: true,  docUrl: "https://cloud.siliconflow.cn/i/rSzssNX2", description: "聚合多厂商模型，部分模型免费", presetBaseUrl: "https://api.siliconflow.cn/v1", presetQps: 10, isOpenAiCompatible: true, modelRecommendation: "免费模型： GLM-4-9B 翻译质量较好，Qwen3-8B 质量可接受；<br/>付费模型：建议选择 25B 以上参数且性价比高的型号，如 DeepSeek-V4-Flash。<br/>作者资金有限，仅完整测试了小部分收费模型，其余请自行试用评估。" },
  { id: "groq",       name: "Groq",       category: "ai", freeQuota: "完全免费", price: "-", completelyFree: true,  hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://console.groq.com/", description: "Llama 3 等模型，完全免费", presetBaseUrl: "https://api.groq.com/openai/v1", presetQps: 1, isOpenAiCompatible: true },
  { id: "qwen",       name: "通义千问",   category: "ai", freeQuota: "无", price: "0.6元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://dashscope.aliyun.com/", description: "阿里云大模型，多语种能力强", presetBaseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", presetQps: 10, isOpenAiCompatible: true },
  { id: "doubao",     name: "豆包",       category: "ai", freeQuota: "无", price: "0.6元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://www.volcengine.com/product/doubao", description: "字节跳动大模型，价格低", presetBaseUrl: "https://ark.cn-beijing.volces.com/api/v3", presetQps: 10, isOpenAiCompatible: true },
  { id: "hunyuan",    name: "混元",       category: "ai", freeQuota: "hunyuan-lite 免费", price: "5元/百万tokens", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: true,  docUrl: "https://cloud.tencent.com/product/hunyuan", description: "腾讯大模型，lite 版免费", presetBaseUrl: "https://api.hunyuan.cloud.tencent.com/v1", presetQps: 10, isOpenAiCompatible: true },
  { id: "lingyi",     name: "零一万物",   category: "ai", freeQuota: "无", price: "0.99元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://platform.lingyiwanwu.com/", description: "Yi 系列模型，性价比高", presetBaseUrl: "https://api.lingyiwanwu.com/v1", presetQps: 10, isOpenAiCompatible: true },
  { id: "kimi",       name: "Kimi",       category: "ai", freeQuota: "无", price: "12元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://platform.moonshot.cn/", description: "Moonshot 长文本模型", presetBaseUrl: "https://api.moonshot.cn/v1", presetQps: 5,  isOpenAiCompatible: true },
  { id: "openai",     name: "OpenAI",     category: "ai", freeQuota: "无", price: "$0.6/1M tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://platform.openai.com/", description: "GPT-4o / GPT-3.5 等官方模型", presetBaseUrl: "https://api.openai.com/v1", presetQps: 5,  isOpenAiCompatible: true },
  { id: "azure_openai", name: "Azure OpenAI", category: "ai", freeQuota: "无", price: "$0.6/1M tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true, docUrl: "https://learn.microsoft.com/azure/ai-services/openai/", description: "Azure 托管的 OpenAI 模型，需填部署端点 URL", presetBaseUrl: "", presetQps: 5, isOpenAiCompatible: true },
  { id: "gemini",     name: "Gemini",     category: "ai", freeQuota: "完全免费", price: "-", completelyFree: true,  hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://ai.google.dev/", description: "Google Gemini，通过 OpenAI 兼容端点调用", presetBaseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", presetQps: 5, isOpenAiCompatible: true },
  { id: "ernie",      name: "文心一言",   category: "ai", freeQuota: "ERNIE Lite 免费", price: "2元/百万tokens", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: true,  docUrl: "https://qianfan.cloud.baidu.com/", description: "百度千帆 OpenAI 兼容接口", presetBaseUrl: "https://qianfan.baidubce.com/v2", presetQps: 10, isOpenAiCompatible: true },
  { id: "ollama",     name: "Ollama",     category: "ai", freeQuota: "完全免费（本地）", price: "-", completelyFree: true,  hasFreeTier: false, comingSoon: false, requiresApiKey: false, docUrl: "https://ollama.ai/", description: "本地运行开源模型，完全免费", presetBaseUrl: "http://localhost:11434/v1", presetQps: 5,  isOpenAiCompatible: true },
  { id: "lmstudio",   name: "LM Studio",  category: "ai", freeQuota: "完全免费（本地）", price: "-", completelyFree: true,  hasFreeTier: false, comingSoon: false, requiresApiKey: false, docUrl: "https://lmstudio.ai/", description: "本地运行开源模型，完全免费", presetBaseUrl: "http://localhost:1234/v1", presetQps: 5,  isOpenAiCompatible: true },
  { id: "custom",     name: "自定义端点", category: "ai", freeQuota: "-", price: "-", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: false, docUrl: "", description: "兼容 OpenAI 协议的任意端点", presetBaseUrl: "", presetQps: 5, isOpenAiCompatible: true },
];

// === SECTION 2 END ===

// 引擎下拉 value 编解码工具
// 用 JSON + base64 编码，彻底避免模型名含特殊字符（: / || / 路径符号）的解析歧义
// 传统翻译的 value 直接是 provider key（"baidu" / "bing" / "google"），不经过编码
const AI_VALUE_PREFIX = "ai:"; // 前缀用于区分 AI 模型选项与传统翻译选项

export function encodeAiSelectValue(serviceId: string, model: string): string {
  const json = JSON.stringify({ serviceId, model });
  // 用 TextEncoder 替代已废弃的 escape/unescape
  const bytes = new TextEncoder().encode(json);
  let binary = "";
  bytes.forEach((b) => { binary += String.fromCharCode(b); });
  return AI_VALUE_PREFIX + btoa(binary);
}

export function decodeAiSelectValue(value: string): { serviceId: string; model: string } | null {
  if (!value.startsWith(AI_VALUE_PREFIX)) return null;
  try {
    const binary = atob(value.slice(AI_VALUE_PREFIX.length));
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    const json = new TextDecoder().decode(bytes);
    const parsed = JSON.parse(json);
    if (typeof parsed.serviceId === "string" && typeof parsed.model === "string") {
      return parsed;
    }
    return null;
  } catch {
    return null;
  }
}

// === SECTION 3 END ===

/** 匹配搜索：name + freeQuota + price，大小写不敏感 */
export function matchesSearch(s: ServiceDef, query: string): boolean {
  if (!query) return true;
  const q = query.toLowerCase();
  return s.name.toLowerCase().includes(q)
    || s.freeQuota.toLowerCase().includes(q)
    || s.price.toLowerCase().includes(q);
}

/** 根据 id 查找服务定义 */
export function getServiceById(id: string): ServiceDef | undefined {
  return SERVICES.find((s) => s.id === id);
}

/** 获取指定分类的所有服务 */
export function getServicesByCategory(category: ServiceCategory): ServiceDef[] {
  return SERVICES.filter((s) => s.category === category);
}
