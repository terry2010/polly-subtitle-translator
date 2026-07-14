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
  /** 查看免费额度余量的控制台链接（可选，仅传统翻译有免费额度的引擎） */
  quotaUrl?: string;
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
  /** 预置 TPM 上限（每分钟 token 数，0 或不设 = 不限制） */
  presetTpm?: number;
  /** 是否走 OpenAi provider */
  isOpenAiCompatible?: boolean;
  /** 模型推荐说明（可选，显示在 AI 配置面板标题下方） */
  modelRecommendation?: string;
}

// === SECTION 1 END ===

export const SERVICES: ServiceDef[] = [
  // ── 传统翻译 ──
  { id: "baidu",      name: "百度翻译",   category: "traditional", freeQuota: "每月100万字符", price: "49元/100万字符", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: false, docUrl: "https://fanyi-api.baidu.com/", quotaUrl: "https://fanyi-api.baidu.com/manage/overview", description: "支持中英等多语言，每月 100 万字符免费（高级版 QPS=10）", appIdLabel: "App ID", appIdPlaceholder: "百度翻译 App ID", presetQps: 10 },
  { id: "bing",       name: "Microsoft",  category: "traditional", freeQuota: "每月200万字符（F0免费层）", price: "$10/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://learn.microsoft.com/azure/ai-services/translator/", quotaUrl: "https://portal.azure.com/#view/HubsExtension/BrowseResource/resourceType/Microsoft.CognitiveServices%2Faccounts", description: "Azure Translator，F0 免费层每月 200 万字符，每小时 200 万字符上限（约 33,300 字符/分钟），支持 100+ 语言", hasRegion: true, presetQps: 10 },
  { id: "google",     name: "Google",     category: "traditional", freeQuota: "每月50万字符",  price: "20美元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://cloud.google.com/translate/docs/", quotaUrl: "https://console.cloud.google.com/apis/api/translate.googleapis.com/metrics", description: "Google Cloud Translate，每月 50 万字符免费", presetQps: 10 },
  // 以下传统翻译为「待开发」
  { id: "tencent",    name: "腾讯翻译君", category: "traditional", freeQuota: "每月500万字符", price: "58元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://cloud.tencent.com/product/tmt", quotaUrl: "https://console.cloud.tencent.com/tmt/resource_bundle", description: "每月 500 万字符免费，每月 1 号自动发放免费资源包（仅当月有效）。需在控制台手动领取，未领取则按后付费计费", appIdLabel: "SecretId", appIdPlaceholder: "腾讯云 SecretId", presetQps: 5 },
  { id: "volcengine", name: "火山翻译",   category: "traditional", freeQuota: "每月200万字符", price: "49元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://www.volcengine.com/docs/4640/130872", quotaUrl: "https://console.volcengine.com/translate", description: "字节跳动翻译，每月 200 万字符免费", appIdLabel: "Access Key ID", appIdPlaceholder: "火山引擎 Access Key ID", presetQps: 5 },
  { id: "aliyun",     name: "阿里翻译",   category: "traditional", freeQuota: "每月100万字符", price: "50元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://www.aliyun.com/product/ai/alimt", quotaUrl: "https://mt.console.aliyun.com/", description: "阿里云机器翻译，每月 100 万字符免费", appIdLabel: "Access Key ID", appIdPlaceholder: "阿里云 Access Key ID", presetQps: 50 },
  { id: "deepl",      name: "DeepL",      category: "traditional", freeQuota: "一次性100万字符",  price: "$26/月+$27.50/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://developers.deepl.com/docs/getting-started/quickstart", quotaUrl: "https://www.deepl.com/pro-account/usage", description: "翻译质量高，免费 100 万字符（一次性，不按月重置）", presetQps: 5 },
  { id: "youdao",     name: "有道翻译",   category: "traditional", freeQuota: "新用户50元体验金", price: "48元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://ai.youdao.com/", quotaUrl: "https://ai.youdao.com/console/", description: "网易有道翻译 API，新用户赠送 50 元体验金", appIdLabel: "App ID", appIdPlaceholder: "有道翻译 App ID", presetQps: 1 },
  { id: "caiyun",     name: "彩云小译",   category: "traditional", freeQuota: "新用户100万字（1个月）", price: "39元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://docs.caiyunapp.com/lingocloud-api/", quotaUrl: "https://dashboard.caiyunapp.com/", description: "彩云科技翻译 API，新用户 100 万字免费（1 个月有效期）", presetQps: 5 },
  { id: "niutrans",   name: "小牛翻译",   category: "traditional", freeQuota: "每日20万字符", price: "500元/1000万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://niutrans.com/documents/contents/transapi_text_v2", quotaUrl: "https://niutrans.com/cloud/account_info/info", description: "每日赠送 100 积分（约 20 万字符），需绑定微信打卡签到领取。V2 API 需 AppID + APIKey 签名认证。免费用户 QPS=5，付费用户 QPS=50", appIdLabel: "App ID", appIdPlaceholder: "小牛翻译 App ID（控制台 → API 应用）", presetQps: 5 },
  { id: "amazon",     name: "Amazon 翻译", category: "traditional", freeQuota: "前12个月每月200万字符", price: "15美元/100万字符", completelyFree: false, hasFreeTier: true, comingSoon: false, requiresApiKey: false, docUrl: "https://aws.amazon.com/translate/pricing/", quotaUrl: "https://console.aws.amazon.com/billing/home#/freetier", description: "AWS Translate，前 12 个月每月 200 万字符免费", appIdLabel: "Access Key ID", appIdPlaceholder: "AWS Access Key ID", hasRegion: true, presetQps: 10 },

  // ── AI 大模型（全部 OpenAI 兼容，复用 OpenAi provider）──
  { id: "deepseek",   name: "DeepSeek",   category: "ai", freeQuota: "无", price: "2元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://platform.deepseek.com/", description: "推理强、价格低，适合长文本翻译", presetBaseUrl: "https://api.deepseek.com/v1", presetQps: 10, isOpenAiCompatible: true },
  { id: "zhipu",      name: "智谱GLM",    category: "ai", freeQuota: "GLM-4.7-Flash 无限免费（QPS<1）", price: "-", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: true,  docUrl: "https://www.bigmodel.cn/invite?icode=s0SYii89O7V66qkC26gLA%2Bnfet45IvM%2BqDogImfeLyI%3D", description: "GLM-4.7-Flash 无限免费，QPS 需小于 1", presetBaseUrl: "https://open.bigmodel.cn/api/paas/v4", presetQps: 0.5, isOpenAiCompatible: true },
  { id: "siliconflow",name: "硅基流动",   category: "ai", freeQuota: "多种免费小参数模型", price: "-", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: true,  docUrl: "https://cloud.siliconflow.cn/i/rSzssNX2", description: "聚合多厂商模型，部分模型免费", presetBaseUrl: "https://api.siliconflow.cn/v1", presetQps: 10, isOpenAiCompatible: true, modelRecommendation: "免费模型： GLM-4-9B 翻译质量较好，Qwen3-8B 质量可接受；不要选择比qwen3更早发布的模型，不要选择比8b参数更少的模型<br/>付费模型：建议选择 25B 以上参数且性价比高的型号，如 DeepSeek-V4-Flash。<br/> 作者资金有限，仅完整测试了少量收费模型，其余请自行试用评估。" },
  { id: "groq",       name: "Groq",       category: "ai", freeQuota: "0.4 QPS，8000 TPM，需代理", price: "-", completelyFree: true,  hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://console.groq.com/", description: "Llama/Qwen 等模型，完全免费，0.4 QPS 限速，每分钟最多 8000 tokens，需代理访问", presetBaseUrl: "https://api.groq.com/openai/v1", presetQps: 0.4, presetTpm: 8000, isOpenAiCompatible: true },
  { id: "qwen",       name: "通义千问",   category: "ai", freeQuota: "无", price: "0.6元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://dashscope.aliyun.com/", description: "阿里云大模型，多语种能力强", presetBaseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", presetQps: 10, isOpenAiCompatible: true },
  { id: "doubao",     name: "豆包",       category: "ai", freeQuota: "无", price: "0.6元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://www.volcengine.com/product/doubao", description: "字节跳动大模型，价格低", presetBaseUrl: "https://ark.cn-beijing.volces.com/api/v3", presetQps: 10, isOpenAiCompatible: true },
  { id: "hunyuan",    name: "混元",       category: "ai", freeQuota: "新用户每个模型 1M tokens（90 天）", price: "0.3元/百万tokens起", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: true,  docUrl: "https://console.cloud.tencent.com/tokenhub", description: "腾讯 TokenHub 平台，新用户有免费体验额度", presetBaseUrl: "https://tokenhub.tencentmaas.com/v1", presetQps: 10, isOpenAiCompatible: true ,modelRecommendation: "似乎每个api key 只能指定一个模型，在选择模型的时候请选用给这个api key 指定的模型，选用其他模型会报错"},
  { id: "lingyi",     name: "零一万物",   category: "ai", freeQuota: "无", price: "0.99元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://platform.lingyiwanwu.com/", description: "Yi 系列模型，性价比高", presetBaseUrl: "https://api.lingyiwanwu.com/v1", presetQps: 10, isOpenAiCompatible: true },
  { id: "kimi",       name: "Kimi",       category: "ai", freeQuota: "无", price: "12元/百万tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://platform.moonshot.cn/", description: "Moonshot 长文本模型", presetBaseUrl: "https://api.moonshot.cn/v1", presetQps: 5,  isOpenAiCompatible: true },
  { id: "openai",     name: "OpenAI",     category: "ai", freeQuota: "无", price: "$0.6/1M tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://platform.openai.com/", description: "GPT-4o / GPT-3.5 等官方模型", presetBaseUrl: "https://api.openai.com/v1", presetQps: 5,  isOpenAiCompatible: true },
  { id: "azure_openai", name: "Azure OpenAI", category: "ai", freeQuota: "无", price: "$0.6/1M tokens", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true, docUrl: "https://learn.microsoft.com/azure/ai-services/openai/", description: "Azure 托管的 OpenAI 模型，需填部署端点 URL", presetBaseUrl: "", presetQps: 5, isOpenAiCompatible: true },
  { id: "gemini",     name: "Gemini",     category: "ai", freeQuota: "部分模型有免费层（不同账号可用模型不同）", price: "-", completelyFree: false, hasFreeTier: true,  comingSoon: false, requiresApiKey: true,  docUrl: "https://ai.google.dev/", description: "Google Gemini，通过 OpenAI 兼容端点调用。免费模型名单随版本更新，且不同账号实际可用的免费模型可能不同，具体以官方价格页和实际调用结果为准；报 404 表示该模型对当前账号不可用", presetBaseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", presetQps: 0.17, isOpenAiCompatible: true },
  { id: "ernie",      name: "文心一言",   category: "ai", freeQuota: "无", price: "0.8元/百万tokens起", completelyFree: false, hasFreeTier: false, comingSoon: false, requiresApiKey: true,  docUrl: "https://qianfan.cloud.baidu.com/", description: "百度千帆 OpenAI 兼容接口，推理速度较快", presetBaseUrl: "https://qianfan.baidubce.com/v2", presetQps: 10, isOpenAiCompatible: true, modelRecommendation: "广告里说有很好的免费模型，因为bce后台极为复杂，作者找不到入口，故无法测试免费模型。<br /> 因为没钱，无力进行完整测试, 本接入仅保证能按兼容openai模式使用，如果购买过服务可以试试效果" },
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
    || s.price.toLowerCase().includes(q)
    || (s.completelyFree && "完全免费".includes(query))
    || (s.hasFreeTier && "有免费额度".includes(query));
}

/** 根据 id 查找服务定义 */
export function getServiceById(id: string): ServiceDef | undefined {
  return SERVICES.find((s) => s.id === id);
}

/** 获取指定分类的所有服务 */
export function getServicesByCategory(category: ServiceCategory): ServiceDef[] {
  return SERVICES.filter((s) => s.category === category);
}

// === SECTION 4: 免费模型 & 价格 URL ===

// ── SiliconFlow 免费模型列表（从官方价格页抓取，price=0 的 chat 模型）──
const SILICONFLOW_FREE_MODELS = new Set([
  "Qwen/Qwen2.5-7B-Instruct",
  "Qwen/Qwen3-8B",
  "Qwen/Qwen3.5-4B",
  "THUDM/GLM-4-9B-0414",
  "THUDM/GLM-Z1-9B-0414",
  "tencent/Hunyuan-MT-7B",
  "deepseek-ai/DeepSeek-R1-0528-Qwen3-8B",
  "PaddlePaddle/PaddleOCR-VL-1.5",
  "deepseek-ai/DeepSeek-OCR",
]);

// ── 智谱 GLM 免费模型（GLM-4.7-Flash 无限免费，QPS<1）──
const ZHIPU_FREE_MODELS = new Set([
  "glm-4.7-flash",
]);

// ── Gemini 免费模型（OpenAI 兼容接口的模型 id，不带 models/ 前缀）──
// Google 会动态调整免费/付费模型名单。本名单仅用于 UI 提示，每次发版前应根据
// https://ai.google.dev/gemini-api/docs/pricing 更新。未在名单中的模型不显示"可能免费"标签。
// 注意：名单中的模型仍需以服务器实际返回和账号可用性为准；调用时报 404 会有专门提示。
const GEMINI_FREE_MODELS = new Set([
  // Gemini 3.x（当前新账号主要可用）
  "gemini-3.5-flash",
  "gemini-3.1-flash-lite",
  "gemini-3.1-pro",
  // Gemini 2.5 系列（部分账号仍可用）
  "gemini-2.5-flash",
  "gemini-2.5-flash-lite",
  "gemini-2.5-pro",
  "gemini-2.5-flash-preview-05-20",
  "gemini-2.5-pro-preview-05-06",
  // Gemini 2.0 系列（旧版，部分账号已下线）
  "gemini-2.0-flash",
  "gemini-2.0-flash-lite",
  // latest 别名
  "gemini-flash-latest",
  "gemini-flash-lite-latest",
  "gemini-pro-latest",
]);

// ── 百度文心 ERNIE：Speed/Lite 已不再免费，V2 端点也不支持这些模型 ──
// 不再注入免费模型列表，仅使用 V2 /v2/models API 返回的付费模型
export const ERNIE_FREE_MODELS: string[] = [];

// 生成 SiliconFlow 模型详情页 URL（中国站，需登录）
function siliconflowModelUrl(modelId: string): string {
  return `https://cloud.siliconflow.cn/me/models?target=${encodeURIComponent(modelId)}`;
}

// 生成智谱模型价格页 URL
function zhipuModelUrl(_modelId: string): string {
  return "https://open.bigmodel.cn/pricing";
}

// 生成 Gemini 模型价格页 URL
function geminiModelUrl(_modelId: string): string {
  return "https://ai.google.dev/gemini-api/docs/pricing";
}

// 生成文心模型价格页 URL
function ernieModelUrl(_modelId: string): string {
  return "https://cloud.baidu.com/doc/qianfan/s/wmh4sv6ya";
}

/** 判断模型是否可能免费（静态名单，非实时抓取）
 *  注意：名单仅包含各服务官方宣称的免费模型；服务器实际返回、账号是否可用需以实时为准。 */
export function isMaybeFreeModel(serviceId: string, model: string): boolean {
  switch (serviceId) {
    case "siliconflow": return SILICONFLOW_FREE_MODELS.has(model);
    case "zhipu": return ZHIPU_FREE_MODELS.has(model);
    case "gemini": return GEMINI_FREE_MODELS.has(model.replace(/^models\//, ""));
    case "ernie": return ERNIE_FREE_MODELS.includes(model);
    default: return false;
  }
}

/** 判断模型是否已知收费（服务有免费名单但该模型不在其中） */
export function isKnownPaidModel(serviceId: string, model: string): boolean {
  switch (serviceId) {
    case "siliconflow": return !SILICONFLOW_FREE_MODELS.has(model);
    case "zhipu": return !ZHIPU_FREE_MODELS.has(model);
    case "gemini": return false;
    default: return false;
  }
}

/** 获取模型价格页 URL（无价格页的服务返回 null） */
export function getModelPriceUrl(serviceId: string, model: string): string | null {
  switch (serviceId) {
    case "siliconflow": return siliconflowModelUrl(model);
    case "zhipu": return zhipuModelUrl(model);
    case "gemini": return geminiModelUrl(model);
    case "ernie": return ernieModelUrl(model);
    default: return null;
  }
}

// === SECTION 4 END ===
