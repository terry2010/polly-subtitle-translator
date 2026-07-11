// AI 翻译模块
// 从 translate.rs 拆分出来的 AI 相关代码：
// OpenAiProvider + ModelType + AiService + ThinkingStyle + 人名提取 + Prompt 模板

use crate::db::Database;
use crate::error::AppError;
use crate::translate::{
    check_insufficient_balance, lang_full_name, translate_with_retry_provider, clean_json_leak,
    strip_markdown_code_fence, LanguageInfo,
    RateLimitPolicy, TokenUsage, TranslateProviderTrait,
};
use crate::translate::translate_utils::{
    cleanup_cjk_spaces, has_english_word, has_lost_non_sound_lines, is_music_or_symbol_only,
    is_partial_sound_effect, is_partial_translation, looks_like_sound_effect, strip_format_tags,
};
use serde::{Deserialize, Serialize};
use tauri::Emitter;


/// AI 模型类型（用于 prompt 分发）
/// 初期支持 qwen3 / deepseek，其他模型用 Generic 通用 prompt
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    Qwen3,
    Deepseek,
    Generic,
}

impl ModelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelType::Qwen3 => "qwen3",
            ModelType::Deepseek => "deepseek",
            ModelType::Generic => "generic",
        }
    }

    /// 从 serde 字符串构造（用于从 db config 读取的值）
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "qwen3" => Some(ModelType::Qwen3),
            "deepseek" => Some(ModelType::Deepseek),
            "generic" => Some(ModelType::Generic),
            _ => None,
        }
    }

    /// 根据模型 id 自动识别模型类型（大小写不敏感）
    /// qwen3-14b → Qwen3、deepseek-v4 → Deepseek、gemma-3 → Generic
    pub fn from_model_id(id: &str) -> Self {
        let lower = id.to_lowercase();
        if lower.contains("qwen3") {
            ModelType::Qwen3
        } else if lower.contains("deepseek") {
            ModelType::Deepseek
        } else {
            ModelType::Generic
        }
    }

    /// 按模型类型返回输入 token 预算（每批 API 请求的输入上限估算）
    /// 取模型上下文窗口的 ~10%，预留输出 + system prompt 空间
    pub fn max_input_tokens(&self) -> usize {
        match self {
            // Qwen3 系列：上下文 128K，取 12000
            ModelType::Qwen3 => 12000,
            // DeepSeek 系列：上下文 64K，取 8000
            ModelType::Deepseek => 8000,
            // Generic（含本地小模型）：保守取 3000，兼容 4K 上下文的小模型
            ModelType::Generic => 3000,
        }
    }
}

/// AI 服务商标识（与前端 src/lib/services.ts 的 id 一一对应）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiService {
    SiliconFlow,
    Zhipu,
    DeepSeek,
    Groq,
    Kimi,
    Qwen,
    Doubao,
    Hunyuan,
    Lingyi,
    OpenAI,
    AzureOpenAI,
    Gemini,
    Ernie,
    Ollama,
    Lmstudio,
    Custom,
}

impl AiService {
    /// 从 service_id 构造（与前端 services.ts 的 id 对应）
    pub fn from_service_id(id: &str) -> Self {
        match id {
            "deepseek" => AiService::DeepSeek,
            "zhipu" => AiService::Zhipu,
            "siliconflow" => AiService::SiliconFlow,
            "groq" => AiService::Groq,
            "qwen" => AiService::Qwen,
            "doubao" => AiService::Doubao,
            "hunyuan" => AiService::Hunyuan,
            "lingyi" => AiService::Lingyi,
            "kimi" => AiService::Kimi,
            "openai" => AiService::OpenAI,
            "azure_openai" => AiService::AzureOpenAI,
            "gemini" => AiService::Gemini,
            "ernie" => AiService::Ernie,
            "ollama" => AiService::Ollama,
            "lmstudio" => AiService::Lmstudio,
            "custom" => AiService::Custom,
            _ => {
                tracing::warn!("未知 AI 服务 id: {}，回退为 Custom", id);
                AiService::Custom
            }
        }
    }

    /// 是否使用中文 prompt（人名提取）
    pub fn use_chinese_prompt(&self) -> bool {
        matches!(self, AiService::Zhipu)
    }

    /// 服务商显示名（用于错误消息）
    pub fn display_name(&self) -> &'static str {
        match self {
            AiService::DeepSeek => "DeepSeek",
            AiService::Zhipu => "智谱GLM",
            AiService::SiliconFlow => "硅基流动",
            AiService::Groq => "Groq",
            AiService::Qwen => "通义千问",
            AiService::Doubao => "豆包",
            AiService::Hunyuan => "混元",
            AiService::Lingyi => "零一万物",
            AiService::Kimi => "Kimi",
            AiService::OpenAI => "OpenAI",
            AiService::AzureOpenAI => "Azure OpenAI",
            AiService::Gemini => "Gemini",
            AiService::Ernie => "文心一言",
            AiService::Ollama => "Ollama",
            AiService::Lmstudio => "LM Studio",
            AiService::Custom => "自定义端点",
        }
    }

    /// stop 序列：智谱 GLM / Groq 限制最多 4 个，其余用 6 个
    pub fn stop_sequences(&self) -> Vec<&'static str> {
        match self {
            AiService::Zhipu | AiService::Groq => vec!["\n\n", "\nNote:", "\nLet's", "\nHowever"],
            _ => vec!["\n\n", "\nNote:", "\nLet's", "\nHowever", "\nBut ", "\nAlso"],
        }
    }

    /// 逆映射：获取 service_id（用于 thinking 策略记忆的 key）
    pub fn service_id(&self) -> &'static str {
        match self {
            AiService::DeepSeek => "deepseek",
            AiService::Zhipu => "zhipu",
            AiService::SiliconFlow => "siliconflow",
            AiService::Groq => "groq",
            AiService::Qwen => "qwen",
            AiService::Doubao => "doubao",
            AiService::Hunyuan => "hunyuan",
            AiService::Lingyi => "lingyi",
            AiService::Kimi => "kimi",
            AiService::OpenAI => "openai",
            AiService::AzureOpenAI => "azure_openai",
            AiService::Gemini => "gemini",
            AiService::Ernie => "ernie",
            AiService::Ollama => "ollama",
            AiService::Lmstudio => "lmstudio",
            AiService::Custom => "custom",
        }
    }

    /// 默认 TPM 上限（0 = 不限制）
    /// 仅对已知有 TPM 限制的免费/低额度服务商设置
    pub fn default_tpm_limit(&self) -> u64 {
        match self {
            AiService::Groq => 8000,      // Groq 免费版 TPM=8000
            AiService::Zhipu => 100000,   // 智谱免费版有 TPM 限制但额度较大
            AiService::SiliconFlow => 100000, // 硅基流动免费模型有 TPM 限制
            _ => 0, // 其他服务商不主动限制（用户可通过 QPS 控制频率）
        }
    }
}
pub enum ThinkingStyle {
    /// 用 enable_thinking: false 禁用（Qwen3, DeepSeek, Generic）
    EnableThinkingParam,
    /// 同时用顶层 enable_thinking: false + chat_template_kwargs.enable_thinking: false
    /// 当首次请求检测到 thinking 未被禁用时，升级到此策略覆盖更多平台
    DualThinkingParam,
    /// 用 thinking.type: "disabled" 禁用（智谱 GLM 官方参数，siliconflow 也支持）
    ThinkingTypeDisabled,
    /// thinking.type: "disabled" + enable_thinking: false + chat_template_kwargs.enable_thinking: false
    /// 当 ThinkingTypeDisabled 仍检测到 thinking 时升级到此策略
    ThinkingTypeDisabledDual,
    /// Groq 专用：用 reasoning_effort:none 禁用 Qwen3 推理，reasoning_format:hidden 隐藏其他模型推理
    GroqReasoning,
    /// 不禁用（Qwen3 nothink 版本，本身已禁用）
    None,
}

/// 进程级记忆：记录哪些 model 检测到了 thinking（说明当前参数没生效）
/// key = "service_id:model"，value = true 表示需要用双参数
static THINKING_FALLBACK: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, bool>>> =
    std::sync::OnceLock::new();

fn thinking_fallback() -> &'static std::sync::Mutex<std::collections::HashMap<String, bool>> {
    THINKING_FALLBACK.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 标记某个 model 检测到了 thinking（当前参数没生效，下次用双参数）
pub fn mark_thinking_detected(service_id: &str, model: &str) {
    let key = format!("{}:{}", service_id, model);
    let mut map = thinking_fallback().lock().unwrap();
    if let std::collections::hash_map::Entry::Vacant(e) = map.entry(key) {
        tracing::warn!(
            "检测到 thinking 未被禁用（{}:{}），下次请求将使用双参数模式",
            service_id, model
        );
        e.insert(true);
    }
}

/// 查询某个 model 是否需要用双参数
fn needs_dual_param(service_id: &str, model: &str) -> bool {
    let key = format!("{}:{}", service_id, model);
    thinking_fallback().lock().unwrap().get(&key).copied().unwrap_or(false)
}

impl ThinkingStyle {
    /// 根据 model_type + model 名 + 服务商决定 thinking 策略
    pub fn resolve(model_type: &ModelType, model: &str, service: &AiService) -> Self {
        let model_lower = model.to_lowercase();
        match service {
            // Groq 用 reasoning_effort/reasoning_format 控制，不支持 enable_thinking
            // 通过 GroqReasoning 参数在 apply 中处理
            AiService::Groq => ThinkingStyle::GroqReasoning,
            AiService::Zhipu => ThinkingStyle::ThinkingTypeDisabled,
            // GLM 模型在所有服务商上都用 thinking.type: "disabled"
            // 实测 siliconflow 上 enable_thinking:false 对 GLM-5.2 间歇性失效，
            // thinking 内容会泄漏到 content 字段，产生乱码输出。
            // thinking.type: "disabled" 是智谱官方参数，siliconflow 也支持，更可靠。
            _ if model_lower.contains("glm") => ThinkingStyle::ThinkingTypeDisabled,
            _ if *model_type == ModelType::Qwen3
                && model_lower.contains("nothink") =>
            {
                ThinkingStyle::None
            }
            _ => ThinkingStyle::EnableThinkingParam,
        }
    }

    /// 根据 model_type + model 名 + 服务商 + 历史记忆决定 thinking 策略
    /// 如果之前检测到 thinking 未被禁用，升级到双参数模式
    pub fn resolve_with_memory(
        model_type: &ModelType,
        model: &str,
        service: &AiService,
        service_id: &str,
    ) -> Self {
        let base = Self::resolve(model_type, model, service);
        // 如果基础策略是 EnableThinkingParam，且历史记忆显示需要双参数，升级
        if matches!(base, Self::EnableThinkingParam) && needs_dual_param(service_id, model) {
            Self::DualThinkingParam
        } else if matches!(base, Self::ThinkingTypeDisabled) && needs_dual_param(service_id, model) {
            // ThinkingTypeDisabled 仍检测到 thinking，升级到组合模式
            Self::ThinkingTypeDisabledDual
        } else {
            base
        }
    }

    /// 应用到请求体
    pub fn apply(&self, body: &mut serde_json::Value, is_local: bool) {
        match self {
            ThinkingStyle::None => {}
            ThinkingStyle::ThinkingTypeDisabled => {
                body["thinking"] = serde_json::json!({"type": "disabled"});
            }
            ThinkingStyle::EnableThinkingParam => {
                // 云端平台（siliconflow / 通义千问 / DeepSeek 等）用顶层 enable_thinking，
                // 本地（LM Studio / Ollama）用 chat_template_kwargs.enable_thinking。
                // 实测：
                //   - siliconflow 认顶层 enable_thinking，忽略 chat_template_kwargs
                //   - LM Studio 认 chat_template_kwargs，忽略顶层 enable_thinking
                // 两者参数格式不同，必须按 is_local 区分。
                if is_local {
                    body["chat_template_kwargs"] = serde_json::json!({
                        "enable_thinking": false
                    });
                } else {
                    body["enable_thinking"] = serde_json::json!(false);
                }
            }
            ThinkingStyle::DualThinkingParam => {
                // 双参数模式：同时加顶层 + chat_template_kwargs
                // 当首次请求检测到 thinking 未被禁用时使用，覆盖更多平台
                body["enable_thinking"] = serde_json::json!(false);
                body["chat_template_kwargs"] = serde_json::json!({
                    "enable_thinking": false
                });
            }
            ThinkingStyle::ThinkingTypeDisabledDual => {
                // 组合模式：thinking.type: "disabled" + 顶层 + chat_template_kwargs
                // 当 ThinkingTypeDisabled 仍检测到 thinking 时使用
                body["thinking"] = serde_json::json!({"type": "disabled"});
                body["enable_thinking"] = serde_json::json!(false);
                body["chat_template_kwargs"] = serde_json::json!({
                    "enable_thinking": false
                });
            }
            ThinkingStyle::GroqReasoning => {
                // Groq 不支持 enable_thinking，用 reasoning_effort + reasoning_format
                // Qwen3 模型：reasoning_effort: "none" 彻底禁用推理
                // GPT-OSS 模型：不支持 reasoning_format，用 reasoning_effort: "medium"（默认）
                //   + max_tokens: 8192 给推理和 content 都留足空间
                //   （不加 max_tokens 时默认 2048，推理用完后 content 为空）
                // 其他模型：reasoning_format: "hidden" 隐藏推理内容（模型仍推理但不返回）
                let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");
                let model_lower = model.to_lowercase();
                if model_lower.contains("qwen3") || model_lower.contains("qwq") {
                    body["reasoning_effort"] = serde_json::json!("none");
                } else if model_lower.contains("gpt-oss") {
                    // GPT-OSS：用 medium 保证推理质量
                    // 必须移除 stop 序列 \n\n：推理后 content 前会有 \n\n，被截断导致 content 为空
                    // 必须设 max_tokens：Groq 默认 2048，推理用完后 content 为空（finish: length）
                    // max_tokens 计入 TPM（prompt + max_tokens <= 8000），4096 足够推理+content
                    body["reasoning_effort"] = serde_json::json!("low");
                    if let Some(stop) = body.get_mut("stop").and_then(|s| s.as_array_mut()) {
                        stop.retain(|s| s.as_str() != Some("\n\n"));
                    }
                } else {
                    // 非推理模型：不设置 reasoning 参数，避免 400 错误
                    // reasoning_format/reasoning_effort 仅部分模型支持
                }
            }
        }
    }
}

/// 内置 prompt 模板（编译进二进制，远程不可用时的兜底）
pub struct PromptTemplate {
    pub system: &'static str,
    pub user_line_format: &'static str,
}

impl PromptTemplate {
    /// 渲染 system prompt，替换 {src} / {tgt} 占位符
    pub fn render_system(&self, src: &str, tgt: &str) -> String {
        self.system.replace("{src}", src).replace("{tgt}", tgt)
    }
}

/// 内置模板表（按 ModelType::as_str() 索引）
/// 顺序：qwen3 / deepseek / generic
pub(crate) const BUILTIN_TEMPLATES: &[(&str, PromptTemplate)] = &[
    ("qwen3", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate the following {src} subtitles into {tgt}.\n\n\
                 Output format (JSON):\n\
                 - Return a JSON array of objects: [{\"n\": 1, \"t\": \"<translation1>\"}, {\"n\": 2, \"t\": \"<translation2>\"}]\n\
                 - Each object contains the input line number \"n\" and the translation \"t\".\n\
                 - Preserve special Unicode characters (like \u{E001}) exactly as-is.\n\
                 - Tags like <x0>, </x0>, <x1>, </x1> are formatting markers. KEEP the tags themselves unchanged, but TRANSLATE the text content between them into {tgt}.\n\
                 - Example: '<x0>Hello</x0> world' -> '<x0>你好</x0> 世界'\n\
                 - Each input line is an independent subtitle entry. Do NOT merge, split, or skip any lines, even if a sentence appears to span multiple lines.\n\
                 - The output array must contain exactly the same number of objects as the input lines.\n\
                 - Do not add explanations, notes, or any extra text outside the JSON.\n\n\
                 Example:\n\
                 Input:\n\
                 1. And it's 24 millimetres\n\
                 2. you need every week.\n\
                 3. [clattering continues]\n\
                 Output:\n\
                 [{\"n\": 1, \"t\": \"而每周需要二十四毫米。\"}, {\"n\": 2, \"t\": \"你每周都需要。\"}, {\"n\": 3, \"t\": \"[碰撞声持续]\"}]",
        user_line_format: "{index}. {text}",
    }),
    ("deepseek", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate from {src} to {tgt}.\n\n\
                 Output format (JSON):\n\
                 - Return a JSON array of objects: [{\"n\": 1, \"t\": \"<translation1>\"}, {\"n\": 2, \"t\": \"<translation2>\"}]\n\
                 - Each object contains the input line number \"n\" and the translation \"t\".\n\
                 - Preserve all special characters and placeholders unchanged.\n\
                 - Tags like <x0>, </x0>, <x1>, </x1> are formatting markers. KEEP the tags themselves unchanged, but TRANSLATE the text content between them into {tgt}.\n\
                 - Example: '<x0>Hello</x0> world' -> '<x0>你好</x0> 世界'\n\
                 - Each input line is an independent subtitle entry. Do NOT merge, split, or skip any lines, even if a sentence appears to span multiple lines.\n\
                 - The output array must contain exactly the same number of objects as the input lines.\n\
                 - Do not add any extra text outside the JSON.\n\n\
                 Example:\n\
                 Input:\n\
                 1. And it's 24 millimetres\n\
                 2. you need every week.\n\
                 3. [clattering continues]\n\
                 Output:\n\
                 [{\"n\": 1, \"t\": \"而每周需要二十四毫米。\"}, {\"n\": 2, \"t\": \"你每周都需要。\"}, {\"n\": 3, \"t\": \"[碰撞声持续]\"}]",
        user_line_format: "{index}. {text}",
    }),
    ("generic", PromptTemplate {
        system: "You are a professional subtitle translator.\n\
                 Translate the following {src} subtitles into {tgt}.\n\n\
                 Output format (JSON):\n\
                 - Return a JSON array of objects: [{\"n\": 1, \"t\": \"<translation1>\"}, {\"n\": 2, \"t\": \"<translation2>\"}]\n\
                 - Each object contains the input line number \"n\" and the translation \"t\".\n\
                 - Preserve special Unicode characters exactly as-is.\n\
                 - Tags like <x0>, </x0>, <x1>, </x1> are formatting markers. KEEP the tags themselves unchanged, but TRANSLATE the text content between them into {tgt}.\n\
                 - Example: '<x0>Hello</x0> world' -> '<x0>你好</x0> 世界'\n\
                 - Each input line is an independent subtitle entry. Do NOT merge, split, or skip any lines, even if a sentence appears to span multiple lines.\n\
                 - The output array must contain exactly the same number of objects as the input lines.\n\
                 - Do not add any extra text outside the JSON.\n\n\
                 Example:\n\
                 Input:\n\
                 1. And it's 24 millimetres\n\
                 2. you need every week.\n\
                 3. [clattering continues]\n\
                 Output:\n\
                 [{\"n\": 1, \"t\": \"而每周需要二十四毫米。\"}, {\"n\": 2, \"t\": \"你每周都需要。\"}, {\"n\": 3, \"t\": \"[碰撞声持续]\"}]",
        user_line_format: "{index}. {text}",
    }),
];

/// TPM（Tokens Per Minute）控制器：滑动窗口统计 token 用量，自动限速避免 429
/// 每次请求前调用 acquire 预估并等待，请求后调用 record 更新实际 token 数
/// 全局取消 generation：cancel_translate 时自增，TpmController 检查它来响应取消
static GLOBAL_CANCEL_GEN: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicU64>> =
    std::sync::OnceLock::new();

/// 获取全局取消 generation 计数器
pub fn global_cancel_gen() -> &'static std::sync::Arc<std::sync::atomic::AtomicU64> {
    GLOBAL_CANCEL_GEN.get_or_init(|| std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)))
}

/// 通知全局取消（cancel_translate 调用）
pub fn notify_global_cancel() {
    global_cancel_gen().fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub struct TpmController {
    /// TPM 上限（0 = 不限制）
    limit: u64,
    /// 滑动窗口：(时间戳秒, token 数)
    window: std::sync::Mutex<Vec<(u64, u64)>>,
    /// 创建时的全局取消 generation，不等于当前值表示已取消
    my_gen: u64,
}

impl TpmController {
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            window: std::sync::Mutex::new(Vec::new()),
            my_gen: global_cancel_gen().load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// 检查是否已被取消
    fn is_cancelled(&self) -> bool {
        global_cancel_gen().load(std::sync::atomic::Ordering::Relaxed) != self.my_gen
    }

    /// 请求前调用：预估本次 token 数，如果会超限则等待到窗口内最早一笔过期
    /// 返回一个 guard，请求完成后调用 record_actual 更新实际 token 数
    /// 如果被取消，返回 0 表示调用方应中止
    pub async fn acquire(&self, estimated_tokens: u64) -> u64 {
        if self.limit == 0 {
            return estimated_tokens;
        }
        loop {
            if self.is_cancelled() {
                return 0;
            }
            let wait_ms = self.check_and_wait(estimated_tokens);
            if wait_ms == 0 {
                return estimated_tokens;
            }
            tracing::info!(
                "TPM 控制器：等待 {}ms（预估 {} tokens，窗口已用 {}，上限 {}）",
                wait_ms, estimated_tokens, self.current_usage(), self.limit
            );
            // 分段 sleep（每次最多 500ms），以便及时响应取消
            let mut remaining = wait_ms;
            while remaining > 0 {
                if self.is_cancelled() {
                    return 0;
                }
                let step = remaining.min(500);
                tokio::time::sleep(std::time::Duration::from_millis(step)).await;
                remaining -= step;
            }
        }
    }

    /// 请求后调用：记录实际 token 用量到滑动窗口
    pub fn record(&self, tokens: u64) {
        if self.limit == 0 || tokens == 0 {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut window = self.window.lock().unwrap();
        // 清理 60 秒前的记录
        window.retain(|(ts, _)| now.saturating_sub(*ts) < 60);
        window.push((now, tokens));
    }

    /// 当前窗口内已用 token 数
    fn current_usage(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let window = self.window.lock().unwrap();
        window.iter()
            .filter(|(ts, _)| now.saturating_sub(*ts) < 60)
            .map(|(_, t)| t)
            .sum()
    }

    /// 检查是否可以发送，返回需要等待的毫秒数（0 = 可以发送）
    /// 精确计算：找到需要多少 token 过期才能腾出空间，然后计算最早过期时间
    fn check_and_wait(&self, estimated_tokens: u64) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut window = self.window.lock().unwrap();
        // 清理 60 秒前的记录
        window.retain(|(ts, _)| now.saturating_sub(*ts) < 60);
        let current: u64 = window.iter().map(|(_, t)| *t).sum();
        if current + estimated_tokens <= self.limit {
            return 0;
        }
        // 需要释放的 token 数
        let need_to_free = current + estimated_tokens - self.limit;
        // 从最早到最近，累积 token 直到覆盖 need_to_free
        // 那笔记录的过期时间就是我们需要等待的时间
        let mut accumulated: u64 = 0;
        for &(ts, tokens) in window.iter() {
            accumulated += tokens;
            if accumulated >= need_to_free {
                let expire_sec = ts + 60;
                let wait_sec = expire_sec.saturating_sub(now);
                // 至少等 0.5 秒，最多等 10 秒（避免过长阻塞）
                return (wait_sec.max(1).min(10) * 1000) as u64;
            }
        }
        // 理论上不会走到这里：所有记录都过期后窗口必然为空
        1000
    }
}


pub struct OpenAiProvider {
    base_url: String,
    model: String,
    model_type: ModelType,
    api_key: Option<String>,
    /// 服务商显示名（如 "DeepSeek" / "LM Studio"），用于错误消息
    service_name: String,
    /// AI 服务商标识（用于 thinking 策略、超时、stop 序列等行为分发）
    ai_service: AiService,
    client: reqwest::Client,
    /// 累计 token 用量（原子计数器，线程安全）
    prompt_tokens: std::sync::atomic::AtomicU64,
    completion_tokens: std::sync::atomic::AtomicU64,
    total_tokens: std::sync::atomic::AtomicU64,
    /// 译名表：(EnglishName, ChineseTranslation)，注入到 system prompt
    glossary: Vec<(String, String)>,
    /// 是否要求模型在译文中用 <name=EnglishName>中文</name> 标记人名
    name_tagging: bool,
    /// TPM 控制器（None = 不限制）
    tpm_controller: Option<TpmController>,
}

/// 编号行正则：匹配 "1. text" / "1、text" / "1: text" / "1) text"
static NUMBERED_LINE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

impl OpenAiProvider {
    pub fn new(
        base_url: String,
        model: String,
        model_type: ModelType,
        api_key: Option<String>,
    ) -> Self {
        Self::with_client(base_url, model, model_type, api_key, reqwest::Client::new())
    }

    pub fn with_client(
        base_url: String,
        model: String,
        model_type: ModelType,
        api_key: Option<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            base_url,
            model,
            model_type,
            api_key,
            service_name: "OpenAI".to_string(),
            ai_service: AiService::Custom,
            client,
            prompt_tokens: std::sync::atomic::AtomicU64::new(0),
            completion_tokens: std::sync::atomic::AtomicU64::new(0),
            total_tokens: std::sync::atomic::AtomicU64::new(0),
            glossary: Vec::new(),
            name_tagging: false,
            tpm_controller: None,
        }
    }

    /// 设置服务商显示名（用于错误消息中显示真实服务商名而非 "openai"）
    pub fn with_service_name(mut self, name: String) -> Self {
        self.service_name = name;
        self
    }

    /// 设置 AI 服务商标识（用于 thinking 策略、超时、stop 序列等行为分发）
    pub fn with_ai_service(mut self, service: AiService) -> Self {
        self.ai_service = service;
        self
    }

    /// 设置译名表（注入到 system prompt，保证跨 batch 人名一致）
    pub fn with_glossary(mut self, glossary: Vec<(String, String)>) -> Self {
        self.glossary = glossary;
        self
    }

    /// 启用/禁用人名标记模式（要求模型在译文中用 <name=En>Zh</name> 标记人名）
    pub fn with_name_tagging(mut self, enabled: bool) -> Self {
        self.name_tagging = enabled;
        self
    }

    /// 设置 TPM 上限（0 = 不限制），启用自适应限速
    pub fn with_tpm_limit(mut self, tpm: u64) -> Self {
        if tpm > 0 {
            self.tpm_controller = Some(TpmController::new(tpm));
        }
        self
    }

    /// 构建 system prompt（优先远程模板，回退内置）
    /// 如果设置了 glossary，将译名表注入到 system prompt 末尾，
    /// 保证跨批次人名翻译一致。
    /// 如果启用了 name_tagging，要求模型用 <name=En>Zh</name> 标记人名。
    fn build_system_prompt(&self, source_lang: &str, target_lang: &str) -> String {
        let src = lang_full_name(source_lang);
        let tgt = lang_full_name(target_lang);
        let view = PromptTemplateRegistry::get_template(&self.model_type);
        let mut prompt = view.render_system(src, tgt);

        // 注入译名表
        if !self.glossary.is_empty() {
            let glossary_text = self.glossary
                .iter()
                .map(|(en, zh)| format!("{} → {}", en, zh))
                .collect::<Vec<_>>()
                .join("\n");
            prompt.push_str(&format!(
                "\n\nGlossary (use these translations consistently for proper nouns):\n{}",
                glossary_text
            ));
        }

        // 注入人名标记指令
        if self.name_tagging {
            prompt.push_str(
                "\n\nWhen translating a proper noun (person name) that appears in the Glossary, \
                 wrap the translated name in tags like <name=EnglishName>ChineseName</name>. \
                 Example: <name=Reese>里斯</name>"
            );
        }

        prompt
    }

    /// 构建 user prompt（编号列表格式）
    fn build_user_prompt(&self, texts: &[&String]) -> String {
        let view = PromptTemplateRegistry::get_template(&self.model_type);
        view.render_user(texts)
    }

    /// 解析模型返回的编号列表响应，按编号对齐回输入
    pub(crate) fn parse_numbered_response(
        content: &str,
        expected_count: usize,
    ) -> Result<Vec<String>, AppError> {
        let re = NUMBERED_LINE_RE.get_or_init(|| {
            regex::Regex::new(r"^(\d+)[.、:)]\s*(.+)$").unwrap()
        });

        // 1. 尝试解析 JSON 数组格式：[{"n": 1, "t": "..."}, ...]
        // AI 可能用 ```json ... ``` 代码块包裹，先剥离 markdown 代码块
        let trimmed = content.trim();
        let json_content = strip_markdown_code_fence(trimmed);
        if json_content.starts_with('[') {
            // 1a. 标准 JSON 解析
            if let Ok(json) = serde_json::from_str::<Vec<serde_json::Value>>(&json_content) {
                let mut translations: std::collections::HashMap<usize, String> =
                    std::collections::HashMap::new();
                // 检测 AI 是否拆分了条目：如果返回的 n 超出 expected_count，
                // 说明 AI 把含多行的条目拆成了多个翻译（如 "Jerry:\nUhh" → n=1 "Jerry" + n=2 "Uhh"），
                // 导致后续所有翻译错位。此时整批对齐失败，触发降级重试（batch_size=1 时无法拆分）。
                let mut out_of_range = false;
                for item in json {
                    if let (Some(n), Some(t)) = (
                        item.get("n").and_then(|v| v.as_u64()).map(|n| n as usize),
                        item.get("t").and_then(|v| v.as_str()).map(|s| s.trim()),
                    ) {
                        if n > 0 && n <= expected_count {
                            translations.insert(n, t.to_string());
                        } else if n > expected_count {
                            out_of_range = true;
                        }
                    }
                }
                if out_of_range {
                    tracing::warn!(
                        "JSON 解析：AI 返回了超出范围的编号（n > {}），可能拆分了条目，返回对齐失败",
                        expected_count
                    );
                    return Err(AppError::TranslateAlignFailed {
                        missing: expected_count,
                    });
                }
                if translations.len() == expected_count {
                    let result: Vec<String> = (1..=expected_count)
                        .map(|i| translations.remove(&i).unwrap_or_default())
                        .collect();
                    return Ok(result);
                }
                if !translations.is_empty() {
                    // JSON 解析成功但数量不匹配
                    // 检测 AI 是否合并了条目：如果返回的 n 值从 1 开始连续无间隙
                    // （如期望 30 条，返回 n=1-29 连续），说明 AI 把两条合并成一条
                    // 并重新编号，导致后续所有译文错位。此时部分结果不可信，
                    // 返回对齐失败触发整批降级重试。
                    // 例外：如果 n 值有间隙（如 n=1-23, 25-30，缺 24），
                    // 说明 AI 只是跳过了某条，间隙前的译文可信，返回部分结果。
                    let mut sorted_ns: Vec<usize> = translations.keys().cloned().collect();
                    sorted_ns.sort();
                    let has_gap = sorted_ns.iter().enumerate()
                        .any(|(i, &n)| n != i + 1);
                    if !has_gap && translations.len() < expected_count {
                        tracing::warn!(
                            "JSON 解析：n 值连续 1-{} 但期望 {} 条，AI 可能合并了条目并重新编号，返回对齐失败",
                            translations.len(),
                            expected_count
                        );
                        return Err(AppError::TranslateAlignFailed {
                            missing: expected_count,
                        });
                    }
                    // n 值有间隙：返回部分结果（空字符串占位）
                    // 调度器会对空字符串的条目进行降级重试
                    tracing::warn!(
                        "JSON 解析数量不匹配：期望 {}，实际 {}（n 值有间隙），返回部分结果",
                        expected_count,
                        translations.len()
                    );
                    let result: Vec<String> = (1..=expected_count)
                        .map(|i| translations.remove(&i).unwrap_or_default())
                        .collect();
                    return Ok(result);
                }
            } else {
                // 1b. 标准 JSON 解析失败，尝试用正则从 JSON 文本中提取 {"n": N, "t": "..."} 对
                // AI 有时会输出未转义的双引号导致 JSON 格式错误
                let re_json = regex::Regex::new(
                    r#""n"\s*:\s*(\d+)\s*,\s*"t"\s*:\s*"((?:[^"\\]|\\.)*)""#
                ).unwrap();
                let mut translations: std::collections::HashMap<usize, String> =
                    std::collections::HashMap::new();
                let mut out_of_range = false;
                for cap in re_json.captures_iter(&json_content) {
                    if let (Ok(n), Some(t)) = (cap[1].parse::<usize>(), cap.get(2)) {
                        if n > 0 && n <= expected_count {
                            // 反转义 JSON 字符串中的转义字符
                            let text = t.as_str()
                                .replace("\\\"", "\"")
                                .replace("\\\\", "\\")
                                .replace("\\n", "\n")
                                .replace("\\t", "\t");
                            translations.insert(n, text.trim().to_string());
                        } else if n > expected_count {
                            out_of_range = true;
                        }
                    }
                }
                if out_of_range {
                    tracing::warn!(
                        "JSON 正则提取：AI 返回了超出范围的编号（n > {}），可能拆分了条目，返回对齐失败",
                        expected_count
                    );
                    return Err(AppError::TranslateAlignFailed {
                        missing: expected_count,
                    });
                }
                if !translations.is_empty() {
                    // 同 1a：检测 n 值连续但数量不足（AI 合并条目）
                    let mut sorted_ns: Vec<usize> = translations.keys().cloned().collect();
                    sorted_ns.sort();
                    let has_gap = sorted_ns.iter().enumerate()
                        .any(|(i, &n)| n != i + 1);
                    if !has_gap && translations.len() < expected_count {
                        tracing::warn!(
                            "JSON 正则提取：n 值连续 1-{} 但期望 {} 条，AI 可能合并了条目，返回对齐失败",
                            translations.len(),
                            expected_count
                        );
                        return Err(AppError::TranslateAlignFailed {
                            missing: expected_count,
                        });
                    }
                    tracing::warn!(
                        "JSON 正则提取：期望 {}，提取 {}（n 值有间隙），返回部分结果",
                        expected_count,
                        translations.len()
                    );
                    let result: Vec<String> = (1..=expected_count)
                        .map(|i| translations.remove(&i).unwrap_or_default())
                        .collect();
                    return Ok(result);
                }
            }
        }

        // 2. 尝试按编号解析（用剥离代码块后的内容，避免 ```json 行干扰）
        let parse_content = if json_content.starts_with('[') || json_content.starts_with("```") {
            &json_content
        } else {
            trimmed
        };
        let mut translations: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
        let mut out_of_range = false;
        for line in parse_content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(captures) = re.captures(line) {
                let num: usize = captures[1].parse().unwrap_or(0);
                let text = captures.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                if num > 0 && num <= expected_count {
                    translations.insert(num, text.to_string());
                } else if num > expected_count {
                    out_of_range = true;
                }
            }
        }
        if out_of_range {
            tracing::warn!(
                "编号解析：AI 返回了超出范围的编号（n > {}），可能拆分了条目，返回对齐失败",
                expected_count
            );
            return Err(AppError::TranslateAlignFailed {
                missing: expected_count,
            });
        }

        // 3. 按编号顺序组装结果
        if translations.len() == expected_count {
            let result: Vec<String> = (1..=expected_count)
                .map(|i| translations.remove(&i).unwrap_or_default())
                .collect();
            return Ok(result);
        }

        // 4. 编号解析失败 → 退化为按行对齐（去掉编号前缀）
        let lines: Vec<String> = parse_content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .map(|l| {
                // 去掉可能残留的编号前缀 "1. " / "1) " / "1、" 等，避免翻译结果含编号
                re.replace(&l, "${2}").to_string()
            })
            .collect();
        if lines.len() == expected_count {
            return Ok(lines);
        }

        // 5. 行数也不对 → 返回对齐失败，由调度器触发逐条重试
        tracing::warn!(
            "编号解析失败：期望 {} 条，编号匹配 {} 条，行对齐 {} 行。原始输出前 500 字符: {:?}",
            expected_count,
            translations.len(),
            lines.len(),
            content.chars().take(500).collect::<String>()
        );
        Err(AppError::TranslateAlignFailed {
            missing: expected_count.saturating_sub(lines.len()),
        })
    }

    /// 单批翻译（构造 chat completion 请求）
    async fn translate_single_batch(
        &self,
        texts: &[&String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        let system_prompt = self.build_system_prompt(source_lang, target_lang);
        let user_prompt = self.build_user_prompt(texts);

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user",   "content": user_prompt },
            ],
            "temperature": 0.3,
            "stream": true,
        });
        // 关闭 thinking 模式，避免推理过程干扰 JSON 解析
        let thinking_style = ThinkingStyle::resolve_with_memory(&self.model_type, &self.model, &self.ai_service, self.ai_service.service_id());
        thinking_style.apply(&mut request_body, self.is_local_url());
        // 超时时间：智谱 GLM 响应较慢，使用更长的超时
        let timeout_secs = match self.ai_service {
            AiService::Zhipu if !self.is_local_url() => 180,
            _ if self.is_local_url() => 1800,
            _ => 120,
        };
        let chunk_timeout_secs = match self.ai_service {
            AiService::Zhipu if !self.is_local_url() => 60,
            _ if self.is_local_url() => 300,
            _ => 60,
        };
        let mut req = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .json(&request_body);

        // api_key 非空时才带认证头（局域网无认证场景）
        // Azure OpenAI 使用 api-key 头而非 Authorization: Bearer
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                if self.base_url.contains("openai.azure.com") {
                    req = req.header("api-key", key);
                } else {
                    req = req.header("Authorization", format!("Bearer {}", key));
                }
            }
        }

        // 流式日志：从 task_local 读取当前并发槽位的文件句柄
        let stream_log_file = crate::STREAM_LOG_FILE.try_get().ok();
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n========== 翻译批次 ==========\n时间: {}\nProvider: {}\nModel: {}\n\n--- 请求体 ---\n{}\n\n--- 流式响应 ---\n",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                self.service_name, self.model,
                request_body,
            ));
        }

        // TPM 控制：预估本次 token 数，必要时等待
        if let Some(ref tpm) = self.tpm_controller {
            let estimated = (request_body.to_string().len() / 3) as u64 + 500;
            if tpm.acquire(estimated).await == 0 {
                return Err(AppError::TaskCancelled);
            }
        }

        let mut resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                AppError::TranslateTimeout {
                    provider: self.service_name.clone(),
                    timeout_secs: timeout_secs,
                }
            } else {
                AppError::TranslateNetworkError {
                    provider: self.service_name.clone(),
                    detail: e.to_string(),
                }
            }
        })?;

        let status = resp.status();

        // 限流：区分 TPM（可重试）和 TPD（不可重试）
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let error_body = resp.text().await.unwrap_or_default();
            let body_lower = error_body.to_lowercase();
            if body_lower.contains("tokens per day") || body_lower.contains("tpd") {
                tracing::error!(
                    "翻译触发每日限额（TPD），不重试, body={}",
                    error_body.chars().take(200).collect::<String>()
                );
                return Err(AppError::TranslateDailyLimitReached {
                    provider: self.service_name.clone(),
                    detail: error_body.chars().take(200).collect(),
                });
            }
            return Err(AppError::TranslateRateLimit {
                provider: self.service_name.clone(),
                retry_after: Some(60),
            });
        }

        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            // 余额不足：优先于认证失败判断（部分服务商余额不足时返回 403 而非 402）
            if let Some(detail) = check_insufficient_balance(status, &error_body) {
                return Err(AppError::TranslateInsufficientBalance {
                    provider: self.service_name.clone(),
                    detail,
                });
            }
            // 认证失败（401/403）：排除了余额不足后才判定为认证失败
            // 401 通常是凭据错误；403 可能是凭据错误，也可能是 IP/地区限制
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(AppError::TranslateAuthFailed {
                    provider: self.service_name.clone(),
                });
            }
            if status == reqwest::StatusCode::FORBIDDEN {
                // 403 + 通用 Forbidden/blocked：更可能是网络/地区限制而非凭据错误
                let is_generic_forbidden = error_body.contains("Forbidden") || error_body.contains("blocked") || error_body.contains("region") || error_body.contains("access denied");
                if is_generic_forbidden {
                    return Err(AppError::TranslateNetworkError {
                        provider: self.service_name.clone(),
                        detail: "HTTP 403 - 可能是地区限制或需要代理".to_string(),
                    });
                }
                return Err(AppError::TranslateAuthFailed {
                    provider: self.service_name.clone(),
                });
            }
            crate::log_api_debug(
                &self.service_name, &self.model, "auto", "auto",
                &request_body.to_string(), &error_body, status.as_u16(),
            );
            if let Some(ref log_file) = stream_log_file {
                crate::log_stream_to_file(log_file, &format!(
                    "\n[HTTP {}] {}\n\n========== 翻译批次结束（错误）==========\n",
                    status, error_body.chars().take(200).collect::<String>(),
                ));
            }
            return Err(AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
                detail: format!("HTTP {}: {}", status, error_body.chars().take(200).collect::<String>()),
            });
        }

        // 流式读取 SSE
        let mut buffer = String::new();
        let mut full_content = String::new();
        let mut seen_content = false;
        let mut has_thinking = false;
        let mut empty_chunk_count: u32 = 0;
        let mut prompt_tokens = 0u64;
        let mut completion_tokens = 0u64;

        loop {
            let chunk_result = tokio::time::timeout(
                std::time::Duration::from_secs(chunk_timeout_secs),
                resp.chunk(),
            ).await;

            // 流中断时保留已收到的部分内容（而非直接返回错误）：
            // Qwen3-8B 等小模型的流式响应经常中途断开（chunk error / timeout），
            // 但此时可能已收到 4/5 条正确翻译。丢弃它们会导致整批重试，浪费大量 API 调用。
            // 改为 break 跳出循环，让后续 parse_numbered_response 用正则提取已收到的部分结果。
            let chunk = match chunk_result {
                Ok(Ok(Some(c))) => c,       // 正常收到 chunk，继续处理
                Ok(Ok(None)) => break,      // 流正常结束
                Ok(Err(e)) => {
                    // chunk 解码错误（如 "error decoding response body"）
                    tracing::warn!(
                        "流中断（chunk error）：已收到 {} 字符，尝试解析部分结果。错误: {}",
                        full_content.len(), e
                    );
                    if let Some(ref log_file) = stream_log_file {
                        crate::log_stream_to_file(log_file, &format!(
                            "\n[stream interrupted: {}]\n", e
                        ));
                    }
                    break; // 不返回错误，继续解析 full_content
                }
                Err(_) => {
                    // chunk 超时（无数据超过 chunk_timeout_secs）
                    tracing::warn!(
                        "流中断（chunk timeout）：已收到 {} 字符，尝试解析部分结果",
                        full_content.len()
                    );
                    if let Some(ref log_file) = stream_log_file {
                        crate::log_stream_to_file(log_file, "\n[stream timeout]\n");
                    }
                    break; // 不返回错误，继续解析 full_content
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() { continue; }

                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str.trim() == "[DONE]" { continue; }

                    if let Ok(chunk_json) = serde_json::from_str::<serde_json::Value>(json_str) {
                        let delta_obj = &chunk_json["choices"][0]["delta"];

                        // 记录每个 chunk 的结构（即使 content 为空），方便调试 thinking 问题
                        // 当模型在 thinking 但不返回 reasoning_content 时，会发送大量空 content delta
                        let has_reasoning = delta_obj.get("reasoning_content").is_some();
                        let content_str = delta_obj["content"].as_str().unwrap_or("");
                        let finish_reason = chunk_json["choices"][0].get("finish_reason").and_then(|v| v.as_str()).unwrap_or("");
                        if has_reasoning {
                            // reasoning_content chunk：内容在下面单独记录
                        } else if content_str.is_empty() && finish_reason.is_empty() {
                            // 空 delta（thinking 期间的心跳）：记录计数而非每次都写，避免日志爆炸
                            empty_chunk_count += 1;
                        } else if !content_str.is_empty() {
                            // 有实际内容：如果之前有空 delta，先记录计数
                            if empty_chunk_count > 0 {
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, &format!("\n[{} empty deltas]\n", empty_chunk_count));
                                }
                                empty_chunk_count = 0;
                            }
                        }
                        if !finish_reason.is_empty() {
                            if empty_chunk_count > 0 {
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, &format!("\n[{} empty deltas]\n", empty_chunk_count));
                                }
                                empty_chunk_count = 0;
                            }
                            if let Some(ref log_file) = stream_log_file {
                                crate::log_stream_to_file(log_file, &format!("\n[finish: {}]\n", finish_reason));
                            }
                        }

                        // Qwen3 thinking 模式：reasoning_content 实时记录到日志
                        if let Some(reasoning) = delta_obj["reasoning_content"].as_str() {
                            if !reasoning.is_empty() {
                                has_thinking = true;
                                if empty_chunk_count > 0 {
                                    if let Some(ref log_file) = stream_log_file {
                                        crate::log_stream_to_file(log_file, &format!("\n[{} empty deltas before thinking]\n", empty_chunk_count));
                                    }
                                    empty_chunk_count = 0;
                                }
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, reasoning);
                                }
                            }
                        }

                        // 累积 content
                        if let Some(delta) = delta_obj["content"].as_str() {
                            if !delta.is_empty() {
                                if !seen_content {
                                    seen_content = true;
                                    if let Some(ref log_file) = stream_log_file {
                                        crate::log_stream_to_file(log_file, "\n\n--- 最终输出 ---\n");
                                    }
                                }
                                full_content.push_str(delta);
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, delta);
                                }
                            }
                        }

                        // 累积 usage
                        if let Some(usage) = chunk_json.get("usage") {
                            prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
                            completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
                        }
                    } else {
                        // JSON 解析失败：记录原始行方便调试
                        if let Some(ref log_file) = stream_log_file {
                            crate::log_stream_to_file(log_file, &format!("\n[parse error] {}\n", json_str));
                        }
                    }
                } else {
                    // 非 data: 开头的行（如错误信息、心跳等）：记录原始行
                    if let Some(ref log_file) = stream_log_file {
                        crate::log_stream_to_file(log_file, &format!("\n[raw] {}\n", line));
                    }
                }
            }
        }

        // LM Studio 流式响应可能不返回 usage，用字符数估算
        if prompt_tokens == 0 || completion_tokens == 0 {
            let estimated_prompt = (system_prompt.len() + user_prompt.len()) / 4;
            let estimated_completion = full_content.len() / 3;
            if prompt_tokens == 0 { prompt_tokens = estimated_prompt as u64; }
            if completion_tokens == 0 { completion_tokens = estimated_completion as u64; }
        }

        // 流式日志：结束汇总
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n=== 翻译批次结束 ===\n总字符数: {}\nprompt_tokens: {}\ncompletion_tokens: {}\n时间: {}\n",
                full_content.len(), prompt_tokens, completion_tokens,
                chrono::Local::now().format("%H:%M:%S"),
            ));
        }

        // 累计 token 用量
        if prompt_tokens > 0 || completion_tokens > 0 {
            use std::sync::atomic::Ordering;
            self.prompt_tokens.fetch_add(prompt_tokens, Ordering::Relaxed);
            self.completion_tokens.fetch_add(completion_tokens, Ordering::Relaxed);
            self.total_tokens.fetch_add(prompt_tokens + completion_tokens, Ordering::Relaxed);
            // 记录到 TPM 控制器
            if let Some(ref tpm) = self.tpm_controller {
                tpm.record(prompt_tokens + completion_tokens);
            }
        }

        // 检测到 thinking：标记此 model 需要双参数，下次请求自动升级
        if has_thinking {
            mark_thinking_detected(self.ai_service.service_id(), &self.model);
        }

        // 开发者模式：记录 API 调试日志
        crate::log_api_debug(
            &self.service_name,
            &self.model,
            "auto",
            "auto",
            &request_body.to_string(),
            &full_content,
            200,
        );

        Self::parse_numbered_response(&full_content, texts.len())
            .map(|translations| {
                translations
                    .into_iter()
                    .map(|t| clean_json_leak(&t))
                    .collect()
            })
    }

    /// 从 base_url 中提取 host
    fn extract_host_from_url(&self) -> Option<String> {
        extract_host_from_url_impl(&self.base_url)
    }

    /// 判断 base_url 是否为本地模型
    /// 通过 URL 解析提取 host，再判断是否为私有 IP 地址段
    fn is_local_url(&self) -> bool {
        // 先尝试 URL 解析提取 host
        let host = self.extract_host_from_url();
        if let Some(h) = host {
            return is_private_host(&h);
        }
        // URL 解析失败时回退到子串匹配（兼容非标准 URL 格式）
        self.base_url.contains("localhost")
            || self.base_url.contains("127.0.0.1")
            || self.base_url.contains("192.168.")
            || self.base_url.contains("10.")
            || self.base_url.contains("172.16.")
            || self.base_url.contains("172.17.")
            || self.base_url.contains("172.18.")
            || self.base_url.contains("172.19.")
            || self.base_url.contains("172.20.")
            || self.base_url.contains("172.21.")
            || self.base_url.contains("172.22.")
            || self.base_url.contains("172.23.")
            || self.base_url.contains("172.24.")
            || self.base_url.contains("172.25.")
            || self.base_url.contains("172.26.")
            || self.base_url.contains("172.27.")
            || self.base_url.contains("172.28.")
            || self.base_url.contains("172.29.")
            || self.base_url.contains("172.30.")
            || self.base_url.contains("172.31.")
    }
}

/// 从 URL 字符串中提取 host 部分
/// 支持 http://host:port/path 和 https://host/path 格式
fn extract_host_from_url_impl(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1)?;
    // 去掉 path 和 query
    let authority = after_scheme.split('/').next()?;
    // 去掉 port
    let host = authority.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

/// 判断 host 是否为本地/私有地址
fn is_private_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    // 尝试解析为 IP 地址
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return ip.is_loopback() || is_private_ip(&ip);
    }
    // 非 IP 的 host（如 api10.example.com）不是本地地址
    false
}

/// 判断 IP 地址是否为私有地址（RFC 1918）
fn is_private_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified()
        }
    }
}

#[cfg(test)]
mod local_url_tests {
    use super::*;

    #[test]
    fn test_extract_host_from_url() {
        assert_eq!(extract_host_from_url_impl("http://localhost:8080/api"), Some("localhost".to_string()));
        assert_eq!(extract_host_from_url_impl("https://api.openai.com/v1"), Some("api.openai.com".to_string()));
        assert_eq!(extract_host_from_url_impl("http://192.168.1.100:11434/v1"), Some("192.168.1.100".to_string()));
        assert_eq!(extract_host_from_url_impl("http://10.0.0.5/v1"), Some("10.0.0.5".to_string()));
        assert_eq!(extract_host_from_url_impl("not_a_url"), None);
    }

    #[test]
    fn test_is_private_host() {
        assert!(is_private_host("localhost"));
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("10.0.0.1"));
        assert!(is_private_host("192.168.1.1"));
        assert!(is_private_host("172.16.0.1"));
        assert!(is_private_host("172.31.255.255"));
        // 公网地址不应被识别为本地
        assert!(!is_private_host("api10.example.com"));
        assert!(!is_private_host("8.8.8.8"));
        assert!(!is_private_host("api.openai.com"));
    }
}

#[async_trait::async_trait]
impl TranslateProviderTrait for OpenAiProvider {
    async fn translate(
        &self,
        texts: &[String],
        source_lang: &str,
        target_lang: &str,
    ) -> Result<Vec<String>, AppError> {
        // token 预算分块：按模型类型取预算，粗估 token = chars / 3
        let max_input_tokens = self.model_type.max_input_tokens();

        let mut chunks: Vec<Vec<&String>> = Vec::new();
        let mut current: Vec<&String> = Vec::new();
        let mut current_tokens = 0usize;
        for text in texts {
            let tokens = text.chars().count() / 3 + 1;
            if !current.is_empty() && current_tokens + tokens > max_input_tokens {
                chunks.push(std::mem::take(&mut current));
                current_tokens = 0;
            }
            current.push(text);
            current_tokens += tokens;
        }
        if !current.is_empty() {
            chunks.push(current);
        }

        let mut results = Vec::with_capacity(texts.len());
        for chunk in &chunks {
            let translated = self.translate_single_batch(chunk, source_lang, target_lang).await?;
            results.extend(translated);
        }
        Ok(results)
    }

    async fn supported_target_langs(&self) -> Result<Vec<LanguageInfo>, AppError> {
        // AI 模型支持任意语言，返回常用列表
        Ok(vec![
            LanguageInfo { code: "zh".into(), name: "Chinese".into(), native_name: "中文".into() },
            LanguageInfo { code: "en".into(), name: "English".into(), native_name: "English".into() },
            LanguageInfo { code: "ja".into(), name: "Japanese".into(), native_name: "日本語".into() },
            LanguageInfo { code: "ko".into(), name: "Korean".into(), native_name: "한국어".into() },
            LanguageInfo { code: "fr".into(), name: "French".into(), native_name: "Français".into() },
            LanguageInfo { code: "de".into(), name: "German".into(), native_name: "Deutsch".into() },
            LanguageInfo { code: "es".into(), name: "Spanish".into(), native_name: "Español".into() },
            LanguageInfo { code: "ru".into(), name: "Russian".into(), native_name: "Русский".into() },
            LanguageInfo { code: "it".into(), name: "Italian".into(), native_name: "Italiano".into() },
            LanguageInfo { code: "pt".into(), name: "Portuguese".into(), native_name: "Português".into() },
        ])
    }

    async fn test_connection(&self) -> Result<(), AppError> {
        // 发一个最小翻译请求验证连通性
        self.translate(&["Hello".to_string()], "en", "zh").await?;
        Ok(())
    }

    fn token_usage(&self) -> Option<TokenUsage> {
        use std::sync::atomic::Ordering;
        let pt = self.prompt_tokens.load(Ordering::Relaxed);
        let ct = self.completion_tokens.load(Ordering::Relaxed);
        let tt = self.total_tokens.load(Ordering::Relaxed);
        if pt == 0 && ct == 0 && tt == 0 {
            None
        } else {
            Some(TokenUsage {
                prompt_tokens: pt,
                completion_tokens: ct,
                total_tokens: tt,
            })
        }
    }

    fn max_batch_tokens(&self) -> usize {
        self.model_type.max_input_tokens()
    }

    /// 人名提取：用自定义 system/user prompt 调用 chat completion，返回纯文本响应
    async fn extract_names_raw(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        // 根据服务商定制 stop 序列
        let stop_sequences = self.ai_service.stop_sequences();

        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user",   "content": user_prompt },
            ],
            "temperature": 0,
            "stream": true,
            "stop": stop_sequences,
        });
        // response_format: json_object 并非所有 OpenAI 兼容 API 都支持
        // 已知支持：OpenAI、DeepSeek、通义千问
        // 已知不支持/不确定：LM Studio、Ollama、其他本地推理引擎
        // 对云端 API 加 response_format，对本地 URL 不加（避免 400 错误）
        // 注意：人名提取需要返回 JSON 数组 [{...}, {...}]，而 json_object 模式
        // 强制返回单个 JSON 对象 {}，会导致模型只输出 1 个人名。
        // 因此人名提取不使用 response_format，依赖 prompt 约束输出格式。
        if !self.is_local_url() {
            // 不加 response_format，让模型自由输出 JSON 数组
        }
        // 关闭 thinking 模式，避免推理过程干扰 JSON 解析
        let thinking_style = ThinkingStyle::resolve_with_memory(&self.model_type, &self.model, &self.ai_service, self.ai_service.service_id());
        thinking_style.apply(&mut request_body, self.is_local_url());
        // 超时时间：智谱 GLM 响应较慢，使用更长的超时
        let timeout_secs = match self.ai_service {
            AiService::Zhipu if !self.is_local_url() => 180,
            _ if self.is_local_url() => 1800,
            _ => 120,
        };
        let chunk_timeout_secs = match self.ai_service {
            AiService::Zhipu if !self.is_local_url() => 60,
            _ if self.is_local_url() => 300,
            _ => 60,
        };
        let mut req = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .json(&request_body);
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                if self.base_url.contains("openai.azure.com") {
                    req = req.header("api-key", key);
                } else {
                    req = req.header("Authorization", format!("Bearer {}", key));
                }
            }
        }

        // 流式实时日志：从 task_local 读取当前并发槽位的文件句柄
        let stream_log_file = crate::STREAM_LOG_FILE.try_get().ok();
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n========== 人名预扫描开始 ==========\n时间: {}\nProvider: {}\nModel: {}\n\n--- 请求体 ---\n{}\n\n--- 流式响应 ---\n",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                self.service_name, self.model,
                request_body,
            ));
        }

        // TPM 控制：预估本次 token 数，必要时等待
        if let Some(ref tpm) = self.tpm_controller {
            let estimated = (request_body.to_string().len() / 3) as u64 + 200;
            if tpm.acquire(estimated).await == 0 {
                return Err(AppError::TaskCancelled);
            }
        }

        let mut resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                AppError::TranslateTimeout {
                    provider: self.service_name.clone(),
                    timeout_secs: timeout_secs,
                }
            } else {
                AppError::TranslateNetworkError {
                    provider: self.service_name.clone(),
                    detail: e.to_string(),
                }
            }
        })?;
        let status = resp.status();

        // 429 限流：区分 TPM（可重试）和 TPD（不可重试，需等次日）
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let error_body = resp.text().await.unwrap_or_default();
            let body_lower = error_body.to_lowercase();
            if body_lower.contains("tokens per day") || body_lower.contains("tpd") {
                // TPD 每日限额已用尽，不可重试
                tracing::error!(
                    "人名预扫描触发每日限额（TPD），不重试, body={}",
                    error_body.chars().take(200).collect::<String>()
                );
                return Err(AppError::TranslateDailyLimitReached {
                    provider: self.service_name.clone(),
                    detail: error_body.chars().take(200).collect(),
                });
            }
            // TPM/RPM 限制：等待 60 秒后可重试
            let retry_after: u64 = 60;
            tracing::warn!(
                "人名预扫描被限流（429），等待 {}s 后重试, body={}",
                retry_after,
                error_body.chars().take(200).collect::<String>()
            );
            return Err(AppError::TranslateRateLimit {
                provider: self.service_name.clone(),
                retry_after: Some(retry_after),
            });
        }

        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            crate::log_api_debug(
                &self.service_name,
                &self.model,
                "auto",
                "auto",
                &request_body.to_string(),
                &error_body,
                status.as_u16(),
            );
            if let Some(ref log_file) = stream_log_file {
                crate::log_stream_to_file(log_file, &format!(
                    "\n[HTTP {}] {}\n\n========== 人名预扫描结束（错误）==========\n",
                    status, error_body,
                ));
            }
            return Err(AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
                detail: format!("HTTP {}: {}", status, error_body.chars().take(200).collect::<String>()),
            });
        }

        // 流式读取 SSE
        let mut buffer = String::new();
        let mut full_content = String::new();
        let mut seen_content = false;
        let mut has_thinking = false;
        let mut empty_chunk_count: u32 = 0;
        let mut prompt_tokens = 0u64;
        let mut completion_tokens = 0u64;

        loop {
            let chunk_result = tokio::time::timeout(
                std::time::Duration::from_secs(chunk_timeout_secs),
                resp.chunk(),
            ).await.map_err(|_| {
                crate::log_api_debug(
                    &self.service_name,
                    &self.model,
                    "auto",
                    "auto",
                    &request_body.to_string(),
                    &format!("[chunk timeout after {} chars, {}s no data]", full_content.len(), chunk_timeout_secs),
                    200,
                );
                AppError::TranslateNetworkError {
                    provider: self.service_name.clone(),
                    detail: format!("stream chunk timeout: {}s no data", chunk_timeout_secs),
                }
            })?;

            let chunk = chunk_result.map_err(|e| AppError::TranslateNetworkError {
                provider: self.service_name.clone(),
                detail: format!("stream chunk error: {}", e),
            })?;

            let Some(chunk) = chunk else { break; };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() { continue; }

                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str.trim() == "[DONE]" { continue; }

                    if let Ok(chunk_json) = serde_json::from_str::<serde_json::Value>(json_str) {
                        let delta_obj = &chunk_json["choices"][0]["delta"];

                        // 记录每个 chunk 的结构（即使 content 为空），方便调试 thinking 问题
                        // 当模型在 thinking 但不返回 reasoning_content 时，会发送大量空 content delta
                        let has_reasoning = delta_obj.get("reasoning_content").is_some();
                        let content_str = delta_obj["content"].as_str().unwrap_or("");
                        let finish_reason = chunk_json["choices"][0].get("finish_reason").and_then(|v| v.as_str()).unwrap_or("");
                        if has_reasoning {
                            // reasoning_content chunk：内容在下面单独记录
                        } else if content_str.is_empty() && finish_reason.is_empty() {
                            // 空 delta（thinking 期间的心跳）：记录计数而非每次都写，避免日志爆炸
                            empty_chunk_count += 1;
                        } else if !content_str.is_empty() {
                            // 有实际内容：如果之前有空 delta，先记录计数
                            if empty_chunk_count > 0 {
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, &format!("\n[{} empty deltas]\n", empty_chunk_count));
                                }
                                empty_chunk_count = 0;
                            }
                        }
                        if !finish_reason.is_empty() {
                            if empty_chunk_count > 0 {
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, &format!("\n[{} empty deltas]\n", empty_chunk_count));
                                }
                                empty_chunk_count = 0;
                            }
                            if let Some(ref log_file) = stream_log_file {
                                crate::log_stream_to_file(log_file, &format!("\n[finish: {}]\n", finish_reason));
                            }
                        }

                        // Qwen3 thinking 模式：reasoning_content 实时记录到日志
                        if let Some(reasoning) = delta_obj["reasoning_content"].as_str() {
                            if !reasoning.is_empty() {
                                has_thinking = true;
                                if empty_chunk_count > 0 {
                                    if let Some(ref log_file) = stream_log_file {
                                        crate::log_stream_to_file(log_file, &format!("\n[{} empty deltas before thinking]\n", empty_chunk_count));
                                    }
                                    empty_chunk_count = 0;
                                }
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, reasoning);
                                }
                            }
                        }

                        // 累积 content
                        if let Some(delta) = delta_obj["content"].as_str() {
                            if !delta.is_empty() {
                                if !seen_content {
                                    seen_content = true;
                                    if let Some(ref log_file) = stream_log_file {
                                        crate::log_stream_to_file(log_file, "\n\n--- 最终输出 ---\n");
                                    }
                                }
                                full_content.push_str(delta);
                                if let Some(ref log_file) = stream_log_file {
                                    crate::log_stream_to_file(log_file, delta);
                                }
                            }
                        }

                        // 累积 usage
                        if let Some(usage) = chunk_json.get("usage") {
                            prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
                            completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
                        }
                    } else {
                        // JSON 解析失败：记录原始行方便调试
                        if let Some(ref log_file) = stream_log_file {
                            crate::log_stream_to_file(log_file, &format!("\n[parse error] {}\n", json_str));
                        }
                    }
                } else {
                    // 非 data: 开头的行（如错误信息、心跳等）：记录原始行
                    if let Some(ref log_file) = stream_log_file {
                        crate::log_stream_to_file(log_file, &format!("\n[raw] {}\n", line));
                    }
                }
            }
        }

        // LM Studio 流式响应可能不返回 usage 字段，用字符数估算
        // 估算必须在日志之前，否则日志显示的 token 数永远是 0
        if prompt_tokens == 0 || completion_tokens == 0 {
            // 估算：prompt token ≈ system+user 字符数 / 4，completion token ≈ 输出字符数 / 3
            let estimated_prompt = (system_prompt.len() + user_prompt.len()) / 4;
            let estimated_completion = full_content.len() / 3;
            if prompt_tokens == 0 { prompt_tokens = estimated_prompt as u64; }
            if completion_tokens == 0 { completion_tokens = estimated_completion as u64; }
        }

        // 流式日志：结束汇总
        if let Some(ref log_file) = stream_log_file {
            crate::log_stream_to_file(log_file, &format!(
                "\n\n=== 人名预扫描结束 ===\n总字符数: {}\nprompt_tokens: {}\ncompletion_tokens: {}\n时间: {}\n",
                full_content.len(), prompt_tokens, completion_tokens,
                chrono::Local::now().format("%H:%M:%S"),
            ));
        }

        // 累计 token 用量
        if prompt_tokens > 0 || completion_tokens > 0 {
            use std::sync::atomic::Ordering;
            self.prompt_tokens.fetch_add(prompt_tokens, Ordering::Relaxed);
            self.completion_tokens.fetch_add(completion_tokens, Ordering::Relaxed);
            self.total_tokens.fetch_add(prompt_tokens + completion_tokens, Ordering::Relaxed);
            // 记录到 TPM 控制器
            if let Some(ref tpm) = self.tpm_controller {
                tpm.record(prompt_tokens + completion_tokens);
            }
        }

        // 检测到 thinking：标记此 model 需要双参数，下次请求自动升级
        if has_thinking {
            mark_thinking_detected(self.ai_service.service_id(), &self.model);
        }

        // 开发者模式：记录 API 调试日志
        crate::log_api_debug(
            &self.service_name,
            &self.model,
            "auto",
            "auto",
            &request_body.to_string(),
            &full_content,
            200,
        );

        Ok(full_content)
    }

    fn service_name(&self) -> &str {
        &self.service_name
    }
}


/// 返回 (initial_batch_size, fallback_sizes)
/// initial_batch_size: 首次尝试的批次大小
/// fallback_sizes: 降级时的批次大小列表
pub(crate) fn get_model_batch_sizes(model: &str) -> (usize, Vec<usize>) {
    const DEFAULT_BATCH_SIZES: [usize; 5] = [30, 10, 5, 3, 1];
    
    // 按模型名称匹配（优先级最高）
    let model_lower = model.to_lowercase();
    if model_lower.contains("glm-4.7-flash") || model_lower.contains("glm-4-7-flash") {
        // GLM 4.7 flash 倾向于把跨条目的句子重新分配，批次越大错位范围越大
        // 用 10 条一批减少错位影响范围，降级路径 10→5→3→1
        return (10, vec![10, 5, 3, 1]);
    }
    if model_lower.contains("glm-4.5") || model_lower.contains("glm-4-5") {
        return (100, vec![100, 30, 10, 5, 3, 1]);
    }

    // 按 API 来源匹配（智谱 GLM 官方 API）
    if model_lower.starts_with("glm-") {
        return (150, vec![150, 30, 10, 5, 3, 1]);
    }

    // Qwen3 小模型（8B/4B/7B 等）：参数量小，JSON 输出能力有限
    // 实测 10 条批次失败率 43%，5 条批次成功率 76%
    // 直接从 5 条开始，降级路径 5→3→1，减少无效的 10 条尝试
    if model_lower.contains("qwen3") && (model_lower.contains("8b") || model_lower.contains("4b") || model_lower.contains("7b")) {
        return (5, vec![5, 3, 1]);
    }

    // GPT-OSS：推理模型，medium 模式下推理 token 消耗大
    // 30 条批次推理用完 4096 max_tokens 导致 content 为空
    // 用 10 条批次减少推理量，降级路径 10→5→3→1
    if model_lower.contains("gpt-oss") {
        return (10, vec![10, 5, 3, 1]);
    }
    
    // 默认配置
    (30, DEFAULT_BATCH_SIZES.to_vec())
}

/// 批次降级翻译：对齐失败时自动缩小批次重试（迭代实现，无递归）
/// 顺序：根据模型配置动态决定，默认 30 -> 10 -> 5 -> 3 -> 1
/// 每一级只重试仍然失败的条目，已成功的不重试
/// 返回 Vec<String>，失败的条目用空字符串占位（长度始终等于输入）
pub(crate) async fn translate_batch_with_fallback(
    provider: &dyn TranslateProviderTrait,
    texts: &[String],
    source_lang: &str,
    target_lang: &str,
    model: &str,
    cancel_counter: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    my_gen: u64,
    rate_limit: RateLimitPolicy,
) -> Vec<String> {
    let (_, batch_sizes) = get_model_batch_sizes(model);
    let min_interval = rate_limit.min_interval();

    // 结果数组，初始全空；pending 记录尚未翻译成功的索引
    let mut results: Vec<String> = vec![String::new(); texts.len()];
    let mut pending: Vec<usize> = (0..texts.len()).collect();

    for &batch_size in &batch_sizes {
        if pending.is_empty() {
            break;
        }
        if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
            break;
        }

        let mut still_pending: Vec<usize> = Vec::new();

        for chunk_indices in pending.chunks(batch_size) {
            if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                break;
            }
            // QPS 限流：降级重试的每个子批次请求前强制等待
            // 首批的第一个子批次已在调用方（translate_entries_full）sleep 过，跳过
            if !min_interval.is_zero() && !(batch_size == batch_sizes[0] && chunk_indices[0] == 0) {
                tokio::time::sleep(min_interval).await;
            }
            let chunk_texts: Vec<String> =
                chunk_indices.iter().map(|&i| texts[i].clone()).collect();
            match translate_with_retry_provider(
                provider,
                &chunk_texts,
                source_lang,
                target_lang,
                cancel_counter,
                my_gen,
            )
            .await
            {
                Ok(translations) if translations.len() == chunk_indices.len() => {
                    // 完全对齐：填入结果，但检查空译文和错位译文
                    // 错位检测：9b 模型在批次翻译时可能拆分/合并条目，
                    // 导致翻译内容错位到相邻条目。检测到异常的条目不填入 results，
                    // 而是放入 still_pending 进入下一级降级重试（更小批次/单条）。
                    // 例外：batch_size == 1 时跳过错位检测——单条翻译没有相邻条目，
                    // 不可能发生批次内错位；且确定性 API（百度/DeepL/Google 等）单条
                    // 翻译结果就是"正确答案"。后续仍有 failed 判定（空译文/无 CJK/音效
                    // 不一致等）兜底，不会放过真正的翻译失败。
                    let skip_shift_check = batch_size == 1;
                    let mut shift_detected = false;
                    for (&idx, t) in chunk_indices.iter().zip(translations.iter()) {
                        if t.is_empty() {
                            continue;
                        }
                        let orig_text = &texts[idx];
                        let orig_len = orig_text.chars().count().max(1);
                        let trans_len = t.chars().count();
                        // 模型返回原文检测：译文与原文相同（去掉标签后）时，
                        // 模型可能直接原样返回了输入而没有翻译。
                        // 例外：音效标记/音乐符号/非英语内容保持原样是正确行为。
                        // 检测到时标记为待重试，进入更小批次降级。
                        // batch_size==1 时跳过——已无更小批次可降级，接受结果即可。
                        {
                            let is_sound = looks_like_sound_effect(orig_text);
                            let is_music = is_music_or_symbol_only(orig_text);
                            let is_non_english = !has_english_word(orig_text, 3);
                            if !skip_shift_check && !is_sound && !is_music && !is_non_english {
                                // 去掉格式标签后比较
                                let orig_clean = strip_format_tags(orig_text).trim().to_string();
                                let trans_clean = strip_format_tags(t).trim().to_string();
                                if orig_clean == trans_clean && !orig_clean.is_empty() {
                                    tracing::warn!(
                                        "批次未翻译检测：idx={} 译文与原文相同，标记为待重试",
                                        idx
                                    );
                                    shift_detected = true;
                                    continue; // 不填入 results，进入 still_pending
                                }
                            }
                        }
                        // 部分翻译检测：译文含 CJK 但同时残留多个英文单词
                        // Qwen3-8B 常见模式：翻译后半部分但保留前半部分英文原文
                        // 例外：音效标记/音乐符号/非英语内容不检测
                        // batch_size==1 时跳过——已无更小批次可降级，
                        // 部分翻译总比回退到原文好。
                        {
                            let is_sound = looks_like_sound_effect(orig_text);
                            let is_music = is_music_or_symbol_only(orig_text);
                            let is_non_english = !has_english_word(orig_text, 3);
                            if !skip_shift_check && !is_music && !is_non_english {
                                // 普通文本的部分翻译检测
                                if !is_sound && is_partial_translation(orig_text, t) {
                                    tracing::warn!(
                                        "批次部分翻译检测：idx={} 译文残留多个英文单词，标记为待重试",
                                        idx
                                    );
                                    shift_detected = true;
                                    continue;
                                }
                                // 音效标记的半翻译检测
                                // 例如 [ All grunting ] → [ 所有人发出 grunt 声 ]
                                if is_sound && is_partial_sound_effect(orig_text, t) {
                                    tracing::warn!(
                                        "批次音效半翻译检测：idx={} 音效标记内残留英文单词，标记为待重试",
                                        idx
                                    );
                                    shift_detected = true;
                                    continue;
                                }
                            }
                        }
                        // 跳过音效标记和音乐符号（长度比值无意义）
                        let is_sound = looks_like_sound_effect(orig_text);
                        let is_music = is_music_or_symbol_only(orig_text);
                        // 单条翻译时也检测"非音效行丢失"：
                        // 多行原文含非音效行，但译文只是音效标记（如 "from our mothers!\n[ Mup crying ]" → "[Mup 哭泣]"）
                        // 这种情况即使单条翻译也需要重试，因为非音效行被丢失了
                        let lost_non_sound = skip_shift_check
                            && !is_sound
                            && !is_music
                            && trans_len > 0
                            && has_lost_non_sound_lines(orig_text, t);
                        if lost_non_sound {
                            tracing::warn!(
                                "单条翻译非音效行丢失：idx={} 原文含非音效行但译文仅为音效标记，标记为待重试",
                                idx
                            );
                            shift_detected = true;
                            continue; // 不填入 results，进入 still_pending
                        }
                        if !skip_shift_check && !is_sound && !is_music && trans_len > 0 {
                            let ratio = trans_len as f64 / orig_len as f64;
                            // 错位检测阈值根据原文长度动态调整：
                            // - 长原文（≥30字符）：ratio < 0.15 表示严重偏短（如两行只翻译了第一行）
                            //   0.15 能捕获真正的错位（ratio 通常 < 0.1），同时不误判正常的中英翻译
                            //   注意：中英翻译压缩比通常在 0.15-0.3（中文信息密度高），
                            //   之前的 0.25 阈值会误判正常翻译为错位，导致降级重试失败（空译文）
                            // - 中等原文（15~29字符）：启用 ratio < 0.1 检测。
                            //   中文信息密度高，28 字符英文 → 7 字符中文（ratio 0.25）是正常翻译，
                            //   但 25 字符 → 2 字符（ratio 0.08）通常是错位。
                            //   之前的固定字符差 > 20 阈值会误判 "But I have something to say."(28) → "但我有话要说。"(7)
                            //   差 21 > 20 触发误判，导致 5 级降级重试全部失败、译文留空。
                            // - 极短原文（<15字符）：ratio 检测不可靠（分母太小），用固定字符数差值检测：
                            //   译文比原文少 10 字符以上才判为错位
                            // - 长度比值 > 5.0 且 trans_len > 10：译文严重偏长（如把相邻条目合并了）
                            // - 多行原文（含\n）但译文不含换行：可能只翻译了第一行，降级重试
                            let short_ratio_threshold = if orig_len >= 30 { 0.15 }
                                else if orig_len >= 15 { 0.1 }
                                else { 0.0 };
                            let short_char_diff = if orig_len < 15 {
                                (orig_len as i64 - trans_len as i64) > 10
                            } else {
                                false
                            };
                            let multiline_split = orig_text.contains('\n')
                                && !t.contains('\n')
                                && orig_len >= 20
                                && trans_len > 0
                                && ratio < 0.5;
                            if ratio < short_ratio_threshold
                                || short_char_diff
                                || multiline_split
                                || (ratio > 5.0 && trans_len > 10)
                            {
                                tracing::warn!(
                                    "批次错位检测：idx={} 原文{}字符→译文{}字符 ratio={:.2}，标记为待重试",
                                    idx, orig_len, trans_len, ratio
                                );
                                shift_detected = true;
                                continue; // 不填入 results，进入 still_pending
                            }
                        }
                        results[idx] = t.clone();
                    }
                    if shift_detected && batch_size > 1 {
                        tracing::warn!(
                            "批次大小 {} 检测到错位，将异常条目降级到更小批次重试",
                            batch_size
                        );
                    }
                    for &idx in chunk_indices {
                        if results[idx].is_empty() {
                            still_pending.push(idx);
                        }
                    }
                }
                Ok(translations) => {
                    // 数量不匹配：把非空的填入，空的留到下一级
                    tracing::warn!(
                        "批次降级：大小 {} 返回数量不匹配（期望 {}，实际 {}），继续降级",
                        batch_size,
                        chunk_indices.len(),
                        translations.len()
                    );
                    for (&idx, t) in chunk_indices.iter().zip(translations.iter()) {
                        if !t.is_empty() {
                            results[idx] = t.clone();
                        }
                    }
                    for &idx in chunk_indices {
                        if results[idx].is_empty() {
                            still_pending.push(idx);
                        }
                    }
                }
                Err(e) => {
                    // 整批失败，全部留到下一级
                    tracing::warn!(
                        "批次降级：大小 {} 翻译失败（{}），继续降级",
                        batch_size,
                        e
                    );
                    still_pending.extend_from_slice(chunk_indices);
                }
            }
        }

        pending = still_pending;
    }

    // 最终兜底：经过所有降级重试（30→10→5→3→1）仍为空的条目，
    // 用原文填充译文，确保导出时不出现空白行。
    // 这些条目会在后续 failed 判定逻辑中被标记为 failed（译文=原文），
    // 用户可在 UI 中看到并手动重翻译或编辑。
    for (idx, r) in results.iter_mut().enumerate() {
        if r.is_empty() {
            tracing::warn!(
                "批次降级翻译：条目 {} 经所有重试仍为空，用原文兜底",
                idx
            );
            *r = texts[idx].clone();
        }
    }

    results
}



/// AI 服务 ID → 显示名映射（委托给 AiService::display_name，单一数据源）
pub fn ai_service_display_name(service_id: &str) -> &'static str {
    AiService::from_service_id(service_id).display_name()
}



/// 远程 prompt 配置文件结构
#[derive(Debug, Clone, Deserialize)]
pub struct RemotePromptConfig {
    pub version: String,
    pub templates: std::collections::HashMap<String, RemotePromptTemplate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemotePromptTemplate {
    pub system: String,
    pub user_line_format: String,
}

/// 远程 prompt 配置的 GitHub raw URL
/// 改 prompt 只需更新此文件并 git push，所有客户端下次启动自动生效
#[allow(dead_code)]
const REMOTE_PROMPT_URL: &str = "https://raw.githubusercontent.com/zimufan/ai-subtrans/main/config/prompts.json";

/// 远程 prompt 配置缓存 key（db config 表）
const PROMPT_CONFIG_DB_KEY: &str = "translate_prompt_remote_config";
#[allow(dead_code)]
const PROMPT_CONFIG_VERSION_DB_KEY: &str = "translate_prompt_remote_version";

/// 全局远程配置缓存（启动时拉取后写入，翻译时读取）
static REMOTE_CONFIG: std::sync::OnceLock<std::sync::RwLock<Option<RemotePromptConfig>>> = std::sync::OnceLock::new();

/// 模板视图（统一远程和内置的渲染接口）
pub enum PromptTemplateView {
    Builtin(&'static PromptTemplate),
    Remote(RemotePromptTemplate),
}

impl PromptTemplateView {
    pub fn render_system(&self, src: &str, tgt: &str) -> String {
        match self {
            Self::Builtin(t) => t.render_system(src, tgt),
            Self::Remote(t) => t.system.replace("{src}", src).replace("{tgt}", tgt),
        }
    }

    pub fn render_user(&self, texts: &[&String]) -> String {
        match self {
            Self::Builtin(t) => texts
                .iter()
                .enumerate()
                .map(|(i, txt)| {
                    t.user_line_format
                        .replace("{index}", &(i + 1).to_string())
                        .replace("{text}", txt)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Remote(t) => texts
                .iter()
                .enumerate()
                .map(|(i, txt)| {
                    t.user_line_format
                        .replace("{index}", &(i + 1).to_string())
                        .replace("{text}", txt)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// 模板注册表：优先远程，回退内置
pub struct PromptTemplateRegistry;

impl PromptTemplateRegistry {
    /// 初始化（启动时调用，从 db 加载已缓存的远程配置到内存）
    pub fn init_from_db(db: &Database) {
        let config = db
            .get_config(PROMPT_CONFIG_DB_KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<RemotePromptConfig>(&json).ok());
        let lock = REMOTE_CONFIG.get_or_init(|| std::sync::RwLock::new(None));
        *lock.write().unwrap() = config;
        if let Some(ref c) = *lock.read().unwrap() {
            tracing::info!("远程 prompt 配置已加载: version={}", c.version);
        } else {
            tracing::info!("无远程 prompt 配置，使用内置模板");
        }
    }

    /// 获取模板：远程优先，回退内置
    pub fn get_template(model_type: &ModelType) -> PromptTemplateView {
        let key = model_type.as_str();

        // 尝试远程
        if let Some(lock) = REMOTE_CONFIG.get() {
            if let Some(ref config) = *lock.read().unwrap() {
                if let Some(tmpl) = config.templates.get(key) {
                    return PromptTemplateView::Remote(tmpl.clone());
                }
            }
        }

        // 回退内置
        let builtin = BUILTIN_TEMPLATES
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, t)| t)
            .unwrap_or(&BUILTIN_TEMPLATES[2].1); // 兜底 generic
        PromptTemplateView::Builtin(builtin)
    }
}

/// 拉取远程 prompt 配置（应用启动时调用，失败静默回退内置）
#[allow(dead_code)]
pub async fn fetch_remote_prompt_config(db: &Database) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    match client.get(REMOTE_PROMPT_URL).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(text) => {
                // 校验 JSON 合法性
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    let version = json["version"].as_str().unwrap_or("");
                    let cached_version = db
                        .get_config(PROMPT_CONFIG_VERSION_DB_KEY)
                        .ok()
                        .flatten()
                        .unwrap_or_default();

                    if version != cached_version {
                        let _ = db.set_config(PROMPT_CONFIG_DB_KEY, &text);
                        let _ = db.set_config(PROMPT_CONFIG_VERSION_DB_KEY, version);
                        tracing::info!("远程 prompt 配置已更新: version={}", version);
                    } else {
                        tracing::info!("远程 prompt 配置版本未变: {}", version);
                    }
                }
            }
            Err(e) => tracing::warn!("远程 prompt 配置读取失败: {}", e),
        },
        Ok(resp) => tracing::warn!("远程 prompt 配置 HTTP {}", resp.status()),
        Err(e) => tracing::warn!("远程 prompt 配置拉取失败（使用内置模板）: {}", e),
    }
    // 任何失败都静默处理，翻译时回退内置模板
}

/// 获取当前已加载的远程 prompt 配置版本（供前端显示）
pub fn get_remote_prompt_version() -> Option<String> {
    REMOTE_CONFIG.get().and_then(|lock| {
        lock.read().unwrap().as_ref().map(|c| c.version.clone())
    })
}



/// 人名提取结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedName {
    pub english: String,
    pub chinese: String,
    /// 所有候选译名（按频率降序排列，chinese 是频率最高的）
    pub alternatives: Vec<String>,
}

/// 人名提取的 segment 结果（含来源 segment 索引，用于频率统计）
#[derive(Debug, Clone)]
struct SegmentNameResult {
    segment_idx: usize,
    names: Vec<ExtractedName>,
    /// 段超时时的错误消息（用于检测超时并中止整个预扫描）
    timeout_error: Option<String>,
}

/// 构建人名提取的 system prompt
fn build_name_extraction_system_prompt(source_lang: &str, target_lang: &str, ai_service: &AiService) -> String {
    let src = lang_full_name(source_lang);
    let tgt = lang_full_name(target_lang);

    // 智谱 GLM 使用更简洁的 prompt，避免响应慢
    if ai_service.use_chinese_prompt() {
        return format!(
            "从{src}字幕中提取专有名词并翻译为{tgt}。\n\
             只提取：人名、地名、品牌名、影视作品名、动物品种名。\n\
             不提取：农作物、普通动物、颜色、月份、季节、单位、天气、数字、日期、形容词、动词、普通名词。\n\
             不确定的不要提取。\n\
             必须将每个名字翻译为{tgt}，不要输出英文。\n\n\
             输出 JSON 数组，每个元素为 {{\"en\": \"EnglishName\", \"zh\": \"{tgt}Translation\"}}。\n\
             品牌名（全大写或包含数字）格式：zh = \"EnglishName（中文翻译）\"，请将\"中文翻译\"替换为实际的中文翻译。\n\n\
             示例：\n\
             [\n\
               {{\"en\": \"Zyx\", \"zh\": \"齐克斯\"}},\n\
               {{\"en\": \"Q7X\", \"zh\": \"Q7X（量子7型）\"}},\n\
               {{\"en\": \"Ploria\", \"zh\": \"普洛里亚\"}}\n\
             ]\n\n\
             只输出 JSON 数组，不要输出其他文字。",
            src = src, tgt = tgt
        );
    }

    // 默认 prompt（英文）
    format!(
        "Extract proper nouns from {src} subtitles and translate each to {tgt}.\n\
         ONLY extract: person names, place/farm/field names, brand/product names, movie/TV/song/band/game titles, organization names, named animals, bird species.\n\
         Do NOT extract: crops, generic animals, colors, months, seasons, units, weather, numbers, dates, adjectives, verbs, common nouns, farm terms.\n\
         Do NOT extract: pronouns (I, you, he, she, it, we, they, me, my, our), prepositions (in, on, at, to, for, with, up), interjections (oh, ah, wow, hey), onomatopoeia (hmm, ugh, aah), common verbs (run, let, go, get, make), common nouns (door, room, store, engine, metal, computer, device, emergency, portal, silo, vodka, modem, fishing, bones, blood, yard, shutoff, lockdown, implants, potato distillery, hog-men, hogs, catfish, multiverse, homeworld, password man, rig, reel, fellas, dad, mom, grandpa, god, captain, level, switch, vehicle, powering, dug, crackles, roars, howdy, salud, ding, owww, ooh, oh-ho-ho, aaaaah, aaaah, hootenanny, white coat, iron giant as common noun).\n\
         Do NOT extract stuttering patterns (M-o-o-o-ort-y, Y-o-o-ours, F-u-u-u-uck) — extract the base name instead (Morty, Yours, Fuck is not a proper noun).\n\
         If unsure, do NOT include it.\n\
         You MUST translate every name to {tgt}. Never output English as the translation.\n\n\
         Output a JSON array. Each element is {{\"en\": \"EnglishName\", \"zh\": \"{tgt}Translation\"}}.\n\
         For brand names (all-caps or containing numbers), zh = \"EnglishName（中文翻译）\" - replace \"中文翻译\" with actual Chinese translation.\n\n\
         Example:\n\
         [\n\
           {{\"en\": \"Zyx\", \"zh\": \"齐克斯\"}},\n\
           {{\"en\": \"Q7X\", \"zh\": \"Q7X（量子7型）\"}},\n\
           {{\"en\": \"Ploria\", \"zh\": \"普洛里亚\"}}\n\
         ]\n\n\
         Output ONLY the JSON array. No text before or after. No explanations.",
        src = src, tgt = tgt
    )
}

/// 构建人名提取的 user prompt（编号格式字幕文本）
fn build_name_extraction_user_prompt(texts: &[String]) -> String {
    texts
        .iter()
        .enumerate()
        .map(|(i, txt)| format!("{}. {}", i + 1, txt))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 解析人名提取响应，按 `EnglishName → ChineseTranslation` 格式逐行解析
/// 清理译名中的括号注释（如 `米奇 (Sheepdog's name)` → `米奇`）
/// 判断字符是否为中文（CJK 统一表意文字）
fn is_chinese_char(c: char) -> bool {
    matches!(c,
        '\u{4e00}'..='\u{9fff}'   // CJK 统一表意文字
        | '\u{3400}'..='\u{4dbf}'  // CJK 扩展 A
        | '\u{f900}'..='\u{faff}'  // CJK 兼容表意文字
    )
}

/// 判断字符串是否纯 ASCII（无中文字符）
fn is_pure_ascii(s: &str) -> bool {
    s.is_ascii()
}

/// 从括号内提取中文翻译
/// 模型有时输出 `EnglishName → EnglishName（中文解释）` 格式
/// 此时括号前的部分是英文原名，中文在括号内
/// 例如：`The FarmDroid（农场机器人）` → `农场机器人`
///       `Mounjaro Ramp（莫努佳罗斜坡，可能指某种设备或地形特征）` → `莫努佳罗斜坡`
fn extract_chinese_from_parenthetical(zh_raw: &str) -> Option<String> {
    // 找到第一对括号
    let start_idx = zh_raw.find(['(', '（', '[', '【'])?;
    let open_char = zh_raw.chars().nth(start_idx)?;
    let close_char = match open_char {
        '(' => ')',
        '（' => '）',
        '[' => ']',
        '【' => '】',
        _ => return None,
    };
    let after_open = &zh_raw[start_idx + open_char.len_utf8()..];
    let end_idx = after_open.find(close_char)?;
    let content = &after_open[..end_idx];

    // 按逗号分割取第一段（通常是翻译，后面是解释说明）
    let first_phrase = content
        .split(['，', ',', '、'])
        .next()?
        .trim();

    // 如果第一段包含中文，直接返回
    if first_phrase.chars().any(is_chinese_char) {
        return Some(first_phrase.to_string());
    }

    // 第一段无中文，检查整个括号内容是否有中文
    // 提取所有中文片段（过滤掉纯英文/标点部分）
    let chinese_only: String = content
        .chars()
        .filter(|c| is_chinese_char(*c) || *c == '·' || *c == '/' || *c == ' ')
        .collect();
    let trimmed = chinese_only.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }

    None
}

/// 判断品牌名：全大写（有字母且无小写）或包含数字 —— 与 prompt 定义一致
/// 命中：GS4、Q7X、GPS、NASA、FBI
/// 不命中：The FarmDroid、CornHub、Mounjaro ramp、Jeremy
///
/// 边界假设：has_digit 为 true 时直接判定为品牌名，意味着 Hello4、Page2 等
/// 含数字的普通词也会被当作品牌名保留完整格式。这与 prompt 定义一致（prompt 说
/// "全大写或包含数字"），模型理论上不会对 Hello4 使用品牌名格式。但如果模型
/// 不遵守 prompt，解析器会保留 Hello4（你好4）而不是提取你好4。
/// 这是可接受的——解析器信任模型按 prompt 规则输出。
fn is_brand_name_format(en: &str) -> bool {
    let trimmed = en.trim();
    if trimmed.is_empty() { return false; }
    let has_alpha = trimmed.chars().any(|c| c.is_ascii_alphabetic());
    let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
    let has_lower = trimmed.chars().any(|c| c.is_ascii_lowercase());
    // 全大写（有字母且无小写）或包含数字
    (has_alpha && !has_lower) || has_digit
}

/// 判断提取的英文名是否可能是专有名词
/// 过滤掉明显的短语、句子（9b 模型常把短语当专有名词输出）
pub(crate) fn is_likely_proper_noun(english: &str) -> bool {
    let trimmed = english.trim();
    if trimmed.is_empty() { return false; }

    // 按空格分词（连字符词算一个词）
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let word_count = words.len();

    // 4+ 词几乎肯定是短语/句子，不是专有名词
    if word_count > 3 {
        return false;
    }

    // 常见功能词/虚词 - 多词条目含这些词时判定为短语
    // 注意：冠词 the/a/an 作为首词在专有名词中常见（The Who, The FarmDroid），
    // 只在非首词位置才算短语标志
    let articles: &[&str] = &["the", "a", "an"];
    let function_words: &[&str] = &[
        "in", "on", "at", "to", "for", "of", "with", "by",
        "is", "are", "was", "were", "will", "be", "been", "being",
        "this", "that", "these", "those", "next", "last",
        "and", "or", "but", "not",
        "his", "her", "their", "our", "my", "your",
        "up", "down", "out", "off", "back", "here", "there",
        "when", "where", "what", "how", "why",
        "if", "then", "so", "because",
        "all", "some", "any", "more", "most",
        "two", "three", "four", "five", "six",
        "days", "weeks", "months", "years", "day", "week", "month", "year",
        "time", "later", "ago", "ready", "stuck",
    ];

    // 多词条目：检查是否含功能词
    if word_count > 1 {
        for (i, word) in words.iter().enumerate() {
            // 去掉首尾非字母数字字符后再比较
            let lower: String = word
                .to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            // 冠词只在非首词位置才算短语标志
            if i == 0 && articles.contains(&lower.as_str()) {
                continue;
            }
            if function_words.contains(&lower.as_str()) || articles.contains(&lower.as_str()) {
                return false;
            }
        }
    }

    // 常见非专有名词黑名单（9b 模型常把普通名词当专有名词输出）
    // 注意：鸟种名（skylark/yellowhammer/bunting 等）保留为专有名词，
    //       因为字幕翻译中需要统一译名。
    // 包含：农作物、颜色、月份/星期、度量单位、天气、
    //       农业术语、状态描述、季节、通用动物名（非特定物种名）等
    let common_nouns: &[&str] = &[
        // 农作物/植物
        "oats", "oat", "wheat", "wheats", "barley", "mustard", "onion", "onions",
        "beetroot", "beetroots", "sugar", "grass", "hay", "silage", "seed", "seeds",
        "crop", "crops", "grain", "maize", "corn", "rice", "soybean", "potato",
        // 通用动物名（非特定物种名/品种名）
        "cow", "cows", "cattle", "calf", "calves", "bull", "bulls", "sheep",
        "horse", "horses", "pig", "pigs", "chicken", "chickens", "dog", "dogs",
        "cat", "cats", "bird", "birds",
        // 颜色
        "red", "green", "blue", "white", "black", "yellow", "orange", "purple",
        "brown", "grey", "gray", "pink",
        // 月份/星期
        "january", "february", "march", "april", "may", "june", "july", "august",
        "september", "october", "november", "december",
        "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
        // 度量单位
        "inch", "inches", "millimetre", "millimetres", "millimeter", "millimeters",
        "centimetre", "centimetres", "meter", "meters", "kilometre", "kilometres",
        "mile", "miles", "acre", "acres", "hectare", "hectares", "ton", "tons",
        "tonne", "tonnes", "kilo", "kilos", "kilogram", "kilograms", "gram", "grams",
        "litre", "litres", "gallon", "gallons",
        // 天气/自然
        "rain", "rains", "snow", "wind", "sun", "sunshine", "drought", "flood",
        "storm", "weather", "moisture", "water", "fire",
        // 农业术语
        "harvest", "field", "fields", "farm", "barn", "shed", "tractor", "machine",
        "combine", "plough", "mower", "seeder", "chart", "callipers", "square",
        // 状态/描述
        "pass", "fail", "marginal", "inconclusive", "ready", "stuck", "clear",
        "good", "bad", "sad", "happy", "angry", "worried", "fine", "okay",
        // 通用物品
        "food", "dinner", "lunch", "breakfast", "drink", "tea", "coffee",
        // 季节
        "spring", "summer", "autumn", "fall", "winter",
        // 其他常见误提
        "herd", "operation", "treatment", "test", "ministry", "twins",
        "lumps", "lump", "pneumonia", "cancer", "biopsy", "medical",
        "fraction", "twenty",
        // 日期/年份/数字
        "dates", "date", "2019", "2020", "2021", "2022", "2023", "2024", "2025",
        "twelve", "twelve-twelve",
        // 广告/通用商业词
        "advertisement", "advertisements", "ad", "ads",
        // 通用描述词/状态
        "aggressive", "bovine", "cardboard", "cleaning", "clip", "decluttering",
        "desert", "disorganised", "early", "heads", "light", "necks", "office",
        "palatable", "peaky", "pisser", "posted", "prepped", "pub", "shop",
        "restrictions", "slaughtered", "speedy", "recovery", "stationary", "storage",
        "stressed", "sugars", "whispering", "shrivelled",
        // 通用动作/状态短语
        "big", "plan", "grain", "carting", "grains", "combine", "man",
        "reproducing", "calves", "telehandler", "reactor",
        // 身体部位/发型
        "middle", "parting", "side",
        // 通用短语组件
        "enough", "tall", "stage", "official", "status", "free", "tb",
        "silent", "end", "credits", "tweezer", "things",
        "plus", "one", "two", "inches", "below",
        "fat", "jabs",
        "ever", "shrinking",
    ];

    // 单词条目：检查是否在常见名词黑名单中
    if word_count == 1 {
        let lower: String = words[0]
            .to_lowercase()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        if common_nouns.contains(&lower.as_str()) {
            return false;
        }
    } else {
        // 多词条目：如果所有词都在黑名单中，也不是专有名词
        let all_common = words.iter().all(|w| {
            let lower: String = w
                .to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            common_nouns.contains(&lower.as_str()) || articles.contains(&lower.as_str())
        });
        if all_common {
            return false;
        }
        // 多词条目：如果首词是常见形容词/描述词，很可能是短语而非专有名词
        let first_lower: String = words[0]
            .to_lowercase()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        let adjective_starts: &[&str] = &[
            "big", "small", "large", "tiny", "early", "late", "old", "new",
            "young", "tall", "short", "long", "wide", "narrow", "high", "low",
            "fat", "thin", "slim", "heavy", "light", "dark", "bright",
            "happy", "sad", "angry", "worried", "stressed", "disorganised",
            "aggressive", "palatable", "peaky", "stationary", "shrivelled",
            "silent", "official", "speedy", "ever", "middle", "side",
            "tweezer", "plus", "storage", "reproducing", "grain", "carting",
            "combine", "fat", "cleaning", "decluttering", "whispering",
            "hairy", "paving", "slab",
        ];
        if adjective_starts.contains(&first_lower.as_str()) {
            return false;
        }
    }

    true
}

pub fn parse_name_extraction_response(content: &str) -> Vec<ExtractedName> {
    use std::collections::HashSet;
    let mut names = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // 1. 优先尝试 JSON 解析
    let trimmed = content.trim();
    let json_content = strip_markdown_code_fence(trimmed);
    let mut json_parse_ok = false; // JSON 解析成功（即使返回 0 个人名）
    if json_content.starts_with('[') || json_content.starts_with("{") {
        if let Ok(json) = serde_json::from_str::<Vec<serde_json::Value>>(&json_content) {
            json_parse_ok = true;
            for item in json {
                if let (Some(en), Some(zh)) = (
                    item.get("en").and_then(|v| v.as_str()),
                    item.get("zh").and_then(|v| v.as_str()),
                ) {
                    let en = en.trim();
                    let zh = zh.trim();
                    if !en.is_empty() && !zh.is_empty() && seen.insert(en.to_string()) {
                        if !is_likely_proper_noun(en) {
                            tracing::debug!("人名过滤：'{}' 不像专有名词，跳过", en);
                            continue;
                        }
                        if zh.eq_ignore_ascii_case(en) || is_pure_ascii(zh) {
                            tracing::debug!("人名过滤：'{}' 译名 '{}' 无中文，跳过", en, zh);
                            continue;
                        }
                        // 按 `/` 拆分候选译名
                        let zh_candidates: Vec<String> = zh
                            .split('/')
                            .map(|s| s.trim().trim_matches('"').trim().to_string())
                            .filter(|s| !s.is_empty())
                            .filter(|s| !s.contains("中文翻译") && !s.contains("ChineseTranslation"))
                            // 清理括号注释：`AgBot（农业机器人）` → `农业机器人`
                            // 品牌名（全大写+数字，如 GS4）：保留 `英文（中文）` 完整格式
                            .map(|s| {
                                if is_brand_name_format(en) {
                                    s
                                } else if let Some(chinese) = extract_chinese_from_parenthetical(&s) {
                                    chinese
                                } else {
                                    s
                                }
                            })
                            .collect();
                        if !zh_candidates.is_empty() {
                            names.push(ExtractedName {
                                english: en.to_string(),
                                chinese: zh_candidates[0].clone(),
                                alternatives: zh_candidates.into_iter().skip(1).collect(),
                            });
                        }
                    }
                }
            }
            if !names.is_empty() {
                tracing::info!("人名提取：JSON 解析成功，提取 {} 个人名", names.len());
                return names;
            }
        }
    }

    // 2. 正则提取 {"en": "...", "zh": "..."} 对（对任意内容都尝试）
    let re_json = regex::Regex::new(
        r#""en"\s*:\s*"([^"]+)"\s*,\s*"zh"\s*:\s*"([^"]+)""#
    ).unwrap();
    for cap in re_json.captures_iter(content) {
        let en = cap[1].trim();
        let zh = cap[2].trim();
        if !en.is_empty() && !zh.is_empty() && seen.insert(en.to_string()) {
            if !is_likely_proper_noun(en) { continue; }
            if zh.eq_ignore_ascii_case(en) || is_pure_ascii(zh) { continue; }
            let zh_candidates: Vec<String> = zh
                .split('/')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !zh_candidates.is_empty() {
                names.push(ExtractedName {
                    english: en.to_string(),
                    chinese: zh_candidates[0].clone(),
                    alternatives: zh_candidates.into_iter().skip(1).collect(),
                });
            }
        }
    }
    if !names.is_empty() {
        tracing::info!("人名提取：JSON 正则提取成功，提取 {} 个人名", names.len());
        return names;
    }

    // 3. 回退到旧的 → 格式解析（兼容模型不输出 JSON 的情况）
    if json_parse_ok {
        tracing::debug!("人名提取：JSON 解析成功但 0 个人名，尝试 → 格式解析");
    } else {
        tracing::warn!("人名提取：JSON 解析失败，回退到 → 格式解析");
    }

    // 9b nothink 模型的输出模式不确定：
    // - 有时先输出干净名单，再逐行分析（思考过程在后面）
    // - 有时先逐行分析，再输出干净名单（思考过程在前面）
    // 策略：扫描所有行，分出多段连续的有效名单行，取最长的一段。
    // "有效名单行"判定：含 → 或 -> 分隔符，且不含思考关键词。
    let thinking_keywords: &[&str] = &[
        "let's", "note:", "however,", "maybe", "i think", "i'd say",
        "strictly", "borderline", "usually", "might be", "referring to",
        "descriptive", "exclude", "include", "check", "re-eval", "refining",
        "final check", "correction", "re-check", "items to", "rule says",
        "exclude.", "generic term", "wait,", "so:", "yes.", "no.", "sure.",
        "exclude?", "line ", "extract", "proper noun", "common noun",
        "going through", "line by line",
        "but ", "but,", "also,", "furthermore", "in this", "in the",
        "for example", "for instance", "this means", "this is", "these are",
        "those are", "i will", "i'll", "i see", "perhaps", "probably",
        "likely", "often", "sometimes", "could be", "would be", "should be",
        "is it", "are they", "does it", "do they",
        "is often", "is usually", "is a generic", "is a common",
        "is a proper", "is a specific", "is a type",
        "treated as", "considered", "strictly speaking",
    ];

    let is_thinking_line = |lower: &str| -> bool {
        thinking_keywords.iter().any(|kw| lower.contains(kw))
    };

    let all_lines: Vec<&str> = content.lines().collect();
    // 分段：连续的有效名单行（允许空行间隔）为一段
    let mut blocks: Vec<(usize, usize)> = Vec::new(); // (start, end_exclusive)
    let mut block_start: Option<usize> = None;
    let mut last_valid: Option<usize> = None;

    for (i, line) in all_lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let has_sep = trimmed.contains('→') || trimmed.contains("->");
        let lower = trimmed.to_lowercase();
        let is_thinking = is_thinking_line(&lower);

        if has_sep && !is_thinking {
            // 有效名单行
            if block_start.is_none() {
                block_start = Some(i);
            }
            last_valid = Some(i);
        } else {
            // 非名单行或思考行：结束当前段
            if let (Some(start), Some(end)) = (block_start, last_valid) {
                blocks.push((start, end + 1));
                block_start = None;
                last_valid = None;
            }
        }
    }
    // 收集最后一段
    if let (Some(start), Some(end)) = (block_start, last_valid) {
        blocks.push((start, end + 1));
    }

    // 取最长的一段
    let effective_lines: Vec<&str> = if blocks.is_empty() {
        all_lines.clone()
    } else {
        let best = blocks.iter().max_by_key(|(s, e)| e - s).unwrap();
        all_lines[best.0..best.1].to_vec()
    };

    for line in effective_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        // 过滤含思考关键词的行（双重检查）
        let lower = trimmed.to_lowercase();
        if is_thinking_line(&lower) {
            tracing::debug!("人名过滤：行含思考关键词，跳过: {}", trimmed.chars().take(80).collect::<String>());
            continue;
        }
        // 支持 → 和 -> 两种分隔符
        let parts: Vec<&str> = if let Some(idx) = trimmed.find('→') {
            vec![&trimmed[..idx], &trimmed[idx + '→'.len_utf8()..]]
        } else if let Some(idx) = trimmed.find("->") {
            vec![&trimmed[..idx], &trimmed[idx + 2..]]
        } else {
            // 没有分隔符，跳过
            continue;
        };
        let en = parts[0].trim().trim_matches('"').trim();
        // 清理译名中的括号注释：`米奇 (Sheepdog's name)` → `米奇`
        // 支持中英文括号
        let zh_raw = parts[1].trim().trim_matches('"').trim();
        let zh_before_paren = zh_raw
            .split(['(', '（', '[', '【'])
            .next()
            .unwrap_or(zh_raw)
            .trim()
            .trim_matches('"')
            .trim();
        // 品牌名（全大写+数字，如 GS4）：保留 `英文（中文）` 完整格式
        // 非品牌名（括号前=英文名，如 Endgame → Endgame（终结者））：只取括号内中文
        // 其余：取括号前部分
        let zh_candidates: Vec<String> = if is_brand_name_format(en) {
            zh_raw
                .split('/')
                .map(|s| s.trim().trim_matches('"').trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else if zh_before_paren.eq_ignore_ascii_case(en) {
            // 括号前=英文名（如 Endgame → Endgame（终结者））：只取括号内中文
            if let Some(chinese) = extract_chinese_from_parenthetical(zh_raw) {
                chinese
                    .split('/')
                    .map(|s| s.trim().trim_matches('"').trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                vec![zh_before_paren.to_string()]
            }
        } else {
            zh_before_paren
                .split('/')
                .map(|s| s.trim().trim_matches('"').trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        if !en.is_empty() && !zh_candidates.is_empty() && seen.insert(en.to_string()) {
            // 过滤掉明显的短语/句子（9b 模型常把短语当专有名词输出）
            if !is_likely_proper_noun(en) {
                tracing::debug!("人名过滤：'{}' 不像专有名词，跳过", en);
                continue;
            }
            let chinese = zh_candidates[0].clone();
            // 如果译名和英文名完全相同（或译名是纯英文/纯ASCII），说明模型没翻译
            // 跳过这个条目，避免把英文当译名注入翻译批次
            if chinese.eq_ignore_ascii_case(en) || is_pure_ascii(&chinese) {
                tracing::debug!("人名过滤：'{}' 译名 '{}' 无中文，跳过", en, chinese);
                continue;
            }
            let alternatives = zh_candidates.into_iter().skip(1).collect();
            names.push(ExtractedName {
                english: en.to_string(),
                chinese,
                alternatives,
            });
        }
    }
    names
}

/// 合并多段人名提取结果，同一英文名多个译名时频率优先，平局取首次出现
fn merge_extracted_names(segment_results: &[SegmentNameResult]) -> Vec<ExtractedName> {
    use std::collections::{HashMap, HashSet};
    // en_name -> (zh_name -> (count, first_segment_idx))
    let mut stats: HashMap<String, HashMap<String, (usize, usize)>> = HashMap::new();

    for result in segment_results {
        for name in &result.names {
            let entry = stats.entry(name.english.clone()).or_default();
            // 把主译名和候选译名都纳入统计；同一 segment 内相同译名只计一次
            let all_zh: HashSet<&String> = std::iter::once(&name.chinese)
                .chain(name.alternatives.iter())
                .collect();
            for zh in all_zh {
                let counter = entry.entry(zh.clone()).or_insert((0, result.segment_idx));
                counter.0 += 1;
            }
        }
    }

    // 每个英文名：按频率降序排列所有候选译名，频率最高的作为 chinese，其余作为 alternatives
    let mut merged: Vec<ExtractedName> = stats
        .iter()
        .map(|(en, translations)| {
            let mut sorted: Vec<(&String, &(usize, usize))> = translations.iter().collect();
            // 按频率降序，平局取首次出现 segment 最小的
            sorted.sort_by(|a, b| {
                b.1.0.cmp(&a.1.0).then_with(|| a.1.1.cmp(&b.1.1))
            });
            let chinese = sorted.first().map(|(zh, _)| (*zh).clone()).unwrap_or_default();
            let alternatives: Vec<String> = sorted.iter().skip(1).map(|(zh, _)| (*zh).clone()).collect();
            ExtractedName {
                english: en.clone(),
                chinese,
                alternatives,
            }
        })
        .collect();

    // 按英文名排序，输出稳定
    merged.sort_by(|a, b| a.english.to_lowercase().cmp(&b.english.to_lowercase()));
    merged
}

/// 过滤掉模型误提取的普通词汇
/// 移除：代词、介词、感叹词、口吃模式、常见动词/名词等非专有名词
fn filter_common_words_from_glossary(names: Vec<ExtractedName>) -> Vec<ExtractedName> {
    // 常见英文普通词汇黑名单（小写匹配）
    // 这些是模型容易误提取的词，不是专有名词
    const COMMON_WORDS: &[&str] = &[
        // 代词
        "i", "you", "he", "she", "it", "we", "they", "me", "my", "your", "his", "her", "our", "their",
        // 介词/连词
        "in", "on", "at", "to", "for", "with", "up", "down", "out", "off", "over", "under", "from", "into",
        "and", "or", "but", "so", "if", "than", "then",
        // 感叹词/拟声词
        "oh", "ah", "wow", "hey", "hmm", "ugh", "aah", "ooh", "ow", "oww", "owww", "aah", "aaah", "aaaah", "aaaaah",
        "unh", "unf", "hunh", "uhh", "hnn", "mmm", "mm",
        "haaaa", "hahaha", "glug", "chuckles", "sniff conference",
        // 脏话/粗口
        "asshole", "fucking", "holy shit", "piece-of-shit", "damn", "shit",
        // 常见动词
        "run", "let", "go", "get", "make", "do", "did", "done", "come", "take", "give", "say", "see", "know",
        "dug", "fishing", "powering", "crackles", "roars", "wade", "squish", "stitch",
        // 常见名词
        "door", "room", "store", "engine", "metal", "computer", "device", "emergency", "portal", "silo",
        "vodka", "modem", "bones", "blood", "yard", "shutoff", "lockdown", "implants", "potato", "distillery",
        "hog-men", "hogs", "hog", "catfish", "multiverse", "homeworld", "password", "man", "rig", "reel",
        "fellas", "dad", "mom", "grandpa", "god", "captain", "level", "switch", "vehicle", "ding",
        "hootenanny", "white", "coat", "iron", "giant", "skyzone", "store", "metal", "metal",
        "computer", "science", "electricity", "fishing", "wade", "store", "room",
        "carpet", "drugs", "government", "worry", "worm", "wolves", "humans",
        "emperor", "general", "commander", "leader", "grandfather", "honey", "sweetie",
        "communion", "sub-hyena", "subway", "monorail", "martinis", "mech-suit",
        "eugenics", "infinite", "super", "frankly", "racist", "bigot",
        "client dinner", "dog commander", "dog spokesperson", "guest house",
        "mup facility", "mup video", "mup-a-cino", "mups", "smooth jazz",
        "surprise improv", "endgame", "countryfile",
        // GLM-4-9B 误提取的词
        "access", "ahh", "all", "awesome", "aye-aye, captain!", "badass",
        "bottles", "burps", "cellphone", "champ", "computer science", "dig",
        "emergency shutoff", "gulp", "gunfire", "hardware store", "hatch",
        "house", "lava", "leg", "level 17", "liberation", "muffled screaming",
        "muffled screaming continues", "neighbor", "ocean", "pipes",
        "portal thing", "potato distillery", "raccoon", "resistance",
        "robots", "salud", "sighs", "smiths", "threat nullified", "tools",
        "true", "vodka hogs", "warranty", "what", "white coat",
        "jesus fucking christ", "it guy", "oppressor", "sanchez",
        "huge clouds metallurgy", "iron giant", "skylark", "rex",
        "jeremy", "georgie boy", "hog planet", "hog resistance",
        // 口吃模式（含连字符重复字母）
        // 这些会被单独检测
    ];

    names
        .into_iter()
        .filter(|name| {
            let en_lower = name.english.to_lowercase();
            let en_lower = en_lower.trim();

            // 1. 黑名单匹配
            if COMMON_WORDS.contains(&en_lower) {
                tracing::debug!("人名过滤: 移除普通词汇 \"{}\"", name.english);
                return false;
            }

            // 2. 口吃模式检测：含连字符且重复字母（如 M-o-o-o-ort-y, Y-o-o-ours, F-u-u-u-uck）
            if en_lower.contains('-') {
                // 检查是否有重复的字母段（如 o-o-o, u-u-u）
                let parts: Vec<&str> = en_lower.split('-').collect();
                if parts.len() >= 3 {
                    // 统计单字符段的出现次数
                    let single_char_count = parts.iter().filter(|p| p.len() == 1).count();
                    if single_char_count >= 2 {
                        tracing::debug!("人名过滤: 移除口吃模式 \"{}\"", name.english);
                        return false;
                    }
                }
            }

            // 3. 过短词（≤2字符且非已知缩写）
            if en_lower.len() <= 2 && !en_lower.chars().all(|c| c.is_ascii_uppercase()) {
                tracing::debug!("人名过滤: 移除过短词 \"{}\"", name.english);
                return false;
            }

            // 4. 译文和原文完全相同（模型没翻译）
            if name.chinese.trim() == name.english.trim() {
                tracing::debug!("人名过滤: 移除未翻译条目 \"{}\"", name.english);
                return false;
            }

            // 5. 全大写拉长词（如 SNOWBALLLLL, WAAAAAAAAAAAR）
            // 原词被拉长重复字母，不是专有名词
            if name.english.chars().all(|c| c.is_ascii_alphabetic())
                && name.english.chars().all(|c| c.is_ascii_uppercase())
                && name.english.len() > 8
            {
                // 检查是否有重复字母（拉长模式）
                let chars: Vec<char> = name.english.chars().collect();
                let mut repeat_count = 0;
                for i in 1..chars.len() {
                    if chars[i] == chars[i-1] {
                        repeat_count += 1;
                    }
                }
                if repeat_count >= 3 {
                    tracing::debug!("人名过滤: 移除全大写拉长词 \"{}\"", name.english);
                    return false;
                }
            }

            // 6. 重复短语（如 "Take, take, take"）
            if en_lower.contains(',') {
                let parts: Vec<&str> = en_lower.split(',').map(|p| p.trim()).collect();
                if parts.len() >= 3 {
                    let first = parts[0];
                    if parts.iter().all(|p| *p == first) {
                        tracing::debug!("人名过滤: 移除重复短语 \"{}\"", name.english);
                        return false;
                    }
                }
            }

            true
        })
        .collect()
}

/// 过滤掉未出现在字幕文本中的名字
/// 模型可能把 system prompt 示例中的条目（如 GS4, Jeremy, Endgame, Skylark, Countryfile）
/// 原样复制到输出中，这些名字并不在实际字幕里，应该移除。
/// 匹配策略：大小写不敏感地检查 english 名字是否作为子串出现在任意字幕行中。
fn filter_names_not_in_text(names: Vec<ExtractedName>, texts: &[String]) -> Vec<ExtractedName> {
    // 预计算：把所有字幕文本拼成一个大字符串（小写），用于快速查找
    // 字幕总量通常 ~500 条，总字符 ~50KB，拼接后查找很快
    let all_text_lower: String = texts
        .iter()
        .flat_map(|t| t.chars())
        .map(|c| c.to_ascii_lowercase())
        .collect();

    names
        .into_iter()
        .filter(|name| {
            let en_lower = name.english.to_lowercase();
            let en_lower = en_lower.trim();

            // 空字符串跳过
            if en_lower.is_empty() {
                return false;
            }

            // 检查是否出现在字幕文本中（大小写不敏感）
            // 对于含空格的多词名字（如 "Rick Sanchez"），检查完整短语
            // 对于单词名字，检查单词边界以避免子串误匹配
            if en_lower.contains(' ') {
                // 多词：直接检查子串
                if all_text_lower.contains(en_lower) {
                    return true;
                }
                // 可能字幕中换行了，去掉空格再查
                let no_space = en_lower.replace(' ', "");
                if all_text_lower.replace(' ', "").contains(&no_space) {
                    return true;
                }
                tracing::debug!("人名过滤: \"{}\" 未出现在字幕文本中（多词）", name.english);
                return false;
            }

            // 单词：检查子串出现
            // 简单子串匹配即可，因为人名通常是独特的
            if all_text_lower.contains(en_lower) {
                return true;
            }

            tracing::debug!("人名过滤: \"{}\" 未出现在字幕文本中", name.english);
            false
        })
        .collect()
}

/// 从字幕文本中提取人名（分段并发扫描 + 合并去重）
/// 返回统一的人名译名表
/// app_handle: 用于向前端发送进度事件（extract-names-progress）
pub async fn extract_names_from_subtitles(
    provider: std::sync::Arc<dyn TranslateProviderTrait + Send + Sync>,
    texts: &[String],
    source_lang: &str,
    target_lang: &str,
    max_input_tokens: usize,
    cancel_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
    my_gen: u64,
    app_handle: Option<tauri::AppHandle>,
    user_concurrency: usize,
    rate_limit: RateLimitPolicy,
    model: &str,
    ai_service: AiService,
) -> Result<Vec<ExtractedName>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    // 按 token 预算分段
    // 9b 模型在内容过多时容易"逐行分析"产生大量思考过程，
    // 限制每段最多 150 条字幕（约 2500-3000 token），减少 AI 的分析量。
    // 同时保留 token 预算上限作为第二道限制。
    //
    // 人名提取分段上限受翻译批次大小约束：
    // - 翻译批次大小 >= 150（默认值）时，人名提取用默认 150 条/段
    // - 翻译批次大小 < 150（如小模型 10 条/批）时，人名提取也用更小的值
    // 这样小模型在人名提取时也不会因单段过多导致输出错乱
    const DEFAULT_MAX_LINES_PER_SEGMENT: usize = 150;
    let (translation_batch_size, _) = get_model_batch_sizes(model);
    let max_lines_per_segment = translation_batch_size.min(DEFAULT_MAX_LINES_PER_SEGMENT);
    let segment_budget = max_input_tokens.saturating_sub(2000).max(1000).min(3500);
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_tokens = 0usize;
    for text in texts {
        let tokens = text.chars().count() / 3 + 1;
        let would_exceed_tokens = !current.is_empty() && current_tokens + tokens > segment_budget;
        let would_exceed_lines = current.len() >= max_lines_per_segment;
        if would_exceed_tokens || would_exceed_lines {
            segments.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        current.push(text.clone());
        current_tokens += tokens;
    }
    if !current.is_empty() {
        segments.push(current);
    }

    let total_segments = segments.len();
    tracing::info!("人名预扫描: {} 段（token 预算: {}，每段上限: {} 条）", total_segments, segment_budget, max_lines_per_segment);
    let scan_start = std::time::Instant::now();

    // 发送初始进度事件
    if let Some(ref handle) = app_handle {
        let _ = handle.emit("extract-names-progress", serde_json::json!({
            "progress": 0,
            "total": total_segments,
            "done": false
        }));
    }

    // 并发扫描各段，并发数受用户配置和限流策略控制
    let concurrency = match rate_limit {
        RateLimitPolicy::Qps(_) => 1,
        RateLimitPolicy::Concurrency(max_n) => segments.len().min(user_concurrency.max(1).min(max_n)),
    };
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut join_set = tokio::task::JoinSet::new();
    let segments_len = segments.len();

    // 流式实时日志：预创建 concurrency 个文件（与翻译调度器相同的方式）
    let stream_log_slots = std::sync::Arc::new(crate::create_stream_log_slots(concurrency));
    let slot_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    // 进度计数器
    let completed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // 请求间隔（Qps 模式下为 1/N 秒，Concurrency 模式下为 500ms）
    // 不在 spawn 循环中 sleep，而是在任务获取信号量后 sleep，避免 spawn 循环阻塞
    let request_interval = rate_limit.min_interval();
    let delay_ms = if request_interval.is_zero() {
        500  // Concurrency 模式：500ms 间隔
    } else {
        request_interval.as_millis() as u64
    };

    for (idx, segment) in segments.iter().enumerate() {
        let segment = segment.clone();
        let source = source_lang.to_string();
        let target = target_lang.to_string();
        let provider = provider.clone();
        let semaphore = semaphore.clone();
        let stream_log_slots = stream_log_slots.clone();
        let slot_counter = slot_counter.clone();
        let cancel_counter = cancel_counter.clone();
        let ai_service = ai_service.clone();
        let completed_count = completed_count.clone();
        let app_handle = app_handle.clone();
        join_set.spawn(async move {
            let _permit = semaphore.acquire_owned().await.unwrap();
            // 取消检查：获取信号量后检查取消标志
            if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                tracing::info!("人名预扫描段 {} 已取消", idx + 1);
                return SegmentNameResult { segment_idx: idx, names: Vec::new(), timeout_error: None };
            }
            // 请求间隔：获取信号量后等待，确保 QPS 限速
            // 本地模型的 KV cache 可能在连续请求间被污染，给引擎时间清理缓存
            if idx > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            // 再次检查取消（sleep 期间可能被取消）
            if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                tracing::info!("人名预扫描段 {} 已取消", idx + 1);
                return SegmentNameResult { segment_idx: idx, names: Vec::new(), timeout_error: None };
            }
            tracing::info!("人名预扫描段 {}/{}，{} 条字幕", idx + 1, segments_len, segment.len());
            let seg_start = std::time::Instant::now();

            // 分配并发槽位：取模获取 slot index，对应一个复用的日志文件
            let slot_idx = (slot_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % stream_log_slots.len() as u64) as usize;
            let stream_log_file = stream_log_slots[slot_idx].clone();

            // 用 provider 的 extract_names_raw 方法发送自定义 prompt 请求
            let system_prompt = build_name_extraction_system_prompt(&source, &target, &ai_service);
            let user_prompt = build_name_extraction_user_prompt(&segment);

            // 用 task_local 传递日志文件句柄，extract_names_raw 中读取
            // 遇到 429 限流时自动等待重试（TPM/RPM 超限时 Groq 等服务商会返回 429）
            let content = {
                let max_retries = 3;
                let mut last_err: Option<AppError> = None;
                let mut content: Option<String> = None;
                for attempt in 0..max_retries {
                    if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
                        break;
                    }
                    let result = crate::STREAM_LOG_FILE.scope(stream_log_file.clone(), async {
                        provider.extract_names_raw(&system_prompt, &user_prompt).await
                    }).await;
                    match result {
                        Ok(c) => { content = Some(c); break; }
                        Err(AppError::TranslateRateLimit { retry_after, .. }) => {
                            let wait = retry_after.unwrap_or(30).min(60) as u64;
                            tracing::warn!(
                                "人名预扫描段 {} 被限流（第 {} 次重试），等待 {} 秒",
                                idx + 1, attempt + 1, wait
                            );
                            last_err = Some(AppError::TranslateRateLimit {
                                provider: "AI".to_string(),
                                retry_after: Some(wait),
                            });
                            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                        }
                        Err(e) => { last_err = Some(e); break; }
                    }
                }
                match content {
                    Some(c) => Ok(c),
                    None => Err(last_err.unwrap_or(AppError::TranslateRetriesExhausted)),
                }
            };

            let result = match content {
                Ok(content) => {
                    let names = parse_name_extraction_response(&content);
                    tracing::info!("人名预扫描段 {} 提取到 {} 个人名, 耗时 {:.2}s", idx + 1, names.len(), seg_start.elapsed().as_secs_f64());
                    SegmentNameResult { segment_idx: idx, names, timeout_error: None }
                }
                Err(e) => {
                    tracing::warn!("人名预扫描段 {} 失败: {}", idx + 1, e);
                    let is_timeout = matches!(e, AppError::TranslateTimeout { .. });
                    let is_daily_limit = matches!(e, AppError::TranslateDailyLimitReached { .. });
                    // 超时或每日限额：标记为中止条件，让外层中止剩余任务
                    let abort_msg = if is_timeout || is_daily_limit { Some(e.to_string()) } else { None };
                    SegmentNameResult { segment_idx: idx, names: Vec::new(), timeout_error: abort_msg }
                }
            };

            // 发送进度事件
            let completed = completed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if let Some(ref handle) = app_handle {
                let _ = handle.emit("extract-names-progress", serde_json::json!({
                    "progress": completed,
                    "total": segments_len,
                    "done": completed >= segments_len
                }));
            }

            result
        });
    }

    // 收集结果（支持取消：检测到取消时立即中止剩余任务）
    let mut segment_results = Vec::new();
    let mut timeout_error: Option<String> = None;
    while let Some(res) = join_set.join_next().await {
        if let Ok(result) = res {
            // 检测到超时或每日限额错误：记录并立即中止剩余任务
            if result.timeout_error.is_some() && timeout_error.is_none() {
                timeout_error = result.timeout_error.clone();
                tracing::error!("人名预扫描段 {} 失败（{}），中止剩余任务", result.segment_idx + 1, timeout_error.as_deref().unwrap_or("未知"));
                join_set.abort_all();
                // 发送完成事件，让前端停止等待
                if let Some(ref handle) = app_handle {
                    let _ = handle.emit("extract-names-progress", serde_json::json!({
                        "progress": total_segments,
                        "total": total_segments,
                        "done": true
                    }));
                }
                segment_results.push(result);
                break;
            }
            segment_results.push(result);
        }
        // 取消检查：收到取消信号时中止剩余任务
        if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
            tracing::info!("人名预扫描被取消，中止剩余任务");
            join_set.abort_all();
            break;
        }
    }

    // 超时或每日限额错误：返回对应错误，让前端弹 toast
    if let Some(msg) = timeout_error {
        // 区分每日限额和超时
        if msg.contains("daily token limit") {
            tracing::error!("人名预扫描因每日限额中止: {}", msg);
            return Err(AppError::TranslateDailyLimitReached {
                provider: provider.service_name().to_string(),
                detail: msg,
            });
        }
        tracing::error!("人名预扫描因超时中止: {}", msg);
        return Err(AppError::TranslateTimeout {
            provider: provider.service_name().to_string(),
            timeout_secs: 120,
        });
    }

    // 取消时返回空结果
    if cancel_counter.load(std::sync::atomic::Ordering::Relaxed) != my_gen {
        tracing::info!("人名预扫描已取消");
        // 发送完成事件
        if let Some(ref handle) = app_handle {
            let _ = handle.emit("extract-names-progress", serde_json::json!({
                "progress": total_segments,
                "total": total_segments,
                "done": true
            }));
        }
        return Ok(Vec::new());
    }

    // 按 segment_idx 排序
    segment_results.sort_by_key(|r| r.segment_idx);

    let merged = merge_extracted_names(&segment_results);
    // 后过滤：移除模型误提取的普通词汇（代词/介词/感叹词/口吃模式等）
    let merged = filter_common_words_from_glossary(merged);
    // 后过滤：只保留实际出现在字幕文本中的名字
    // 模型可能把 system prompt 示例中的条目（如 GS4, Jeremy, Endgame）原样复制到输出
    let merged = filter_names_not_in_text(merged, texts);
    tracing::info!("人名预扫描完成: 合并后 {} 个人名, 总耗时 {:.2}s", merged.len(), scan_start.elapsed().as_secs_f64());

    // 发送完成事件
    if let Some(ref handle) = app_handle {
        let _ = handle.emit("extract-names-progress", serde_json::json!({
            "progress": total_segments,
            "total": total_segments,
            "done": true
        }));
    }

    Ok(merged)
}



/// <name=EnglishName>ChineseTranslation</name> 标签正则
static NAME_TAG_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

/// 从译文中提取所有 <name=En>Zh</name> 标签
/// 返回 (english_name, chinese_translation) 列表
/// 支持三种格式：<name=En>Zh</name>、<name="En">Zh</name>、<name>Zh</name>（无英文名）
/// 英文名可含空格（如 <name=Georgie Boy>乔治男孩</name>）
pub fn extract_name_tags(text: &str) -> Vec<(String, String)> {
    let re = NAME_TAG_RE.get_or_init(|| {
        // 容错多种变体：<name=X>Y</name>、<Name=X>Y</Name>、<name="X">Y</name>、<name>Y</name>
        // 捕获组 1：英文名（可选，= 后到 > 前的全部内容，含空格）
        // 捕获组 2：中文译名
        regex::Regex::new(r#"(?i)<name(?:=([^>]*))?\s*>(.*?)</name\s*>"#).unwrap()
    });
    re.captures_iter(text)
        .filter_map(|cap| {
            // 组 1 可能为 None（<name>Zh</name> 格式）或空字符串
            let en = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("").to_string();
            let zh = cap.get(2)?.as_str().trim().to_string();
            if !zh.is_empty() {
                // 无英文名时用中文译名作为 en（用于一致性统计）
                let en = if en.is_empty() { zh.clone() } else { en };
                Some((en, zh))
            } else {
                None
            }
        })
        .collect()
}

/// 剥离译文中所有 <name=...>...</name> 标签，只保留中文部分
/// 多 pass 处理：先处理 well-formed 标签，再处理 9b 模型的畸形标签
pub fn strip_name_tags(text: &str) -> String {
    // Pass 0: 畸形格式 <name>EnglishName>ChineseName</name>
    // llama-3.1-8b 等小模型常见错误：<name>Snowball>雪球</name>
    // <name> 后多了英文名和 >，需要去掉 <name>EnglishName>，只保留 ChineseName
    static MALFORMED_NAME_GT_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re0 = MALFORMED_NAME_GT_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)<name\s*>([A-Za-z\s]+)>(.*?)</name\s*>"#).unwrap()
    });
    let pass0 = re0.replace_all(text, "$2").to_string();

    // Pass 1: well-formed <name=En>Zh</name> 或 <name>Zh</name>
    let re = NAME_TAG_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)<name(?:=([^>]*))?\s*>(.*?)</name\s*>"#).unwrap()
    });
    let pass1 = re.replace_all(&pass0, "$2").to_string();

    // Pass 2: 畸形开标签（缺少 >）但有 </name>：<name=En</name>Zh
    // 9b 模型常见错误：<name=Earth</name>地球 → 移除 <name=Earth</name>，保留后面的地球
    static MALFORMED_OPEN_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re2 = MALFORMED_OPEN_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)<name[^<]*?</name\s*>"#).unwrap()
    });
    let pass2 = re2.replace_all(&pass1, "").to_string();

    // Pass 3: 孤立开标签（缺少 > 且无 </name>）：<name=English中文
    // 9b 模型常见错误：<name=friends猪朋友们 → 移除 <name=friends，保留猪朋友们
    static ORPHAN_OPEN_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re3 = ORPHAN_OPEN_RE.get_or_init(|| {
        // 匹配 <name= 后跟 ASCII 字符（英文名），到非 ASCII 字符停止
        regex::Regex::new(r#"(?i)<name=[A-Za-z\s]*"#).unwrap()
    });
    let pass3 = re3.replace_all(&pass2, "").to_string();

    // Pass 4: 孤立闭标签 </name=...> 或 </name>（无对应开标签）
    static ORPHAN_CLOSE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re4 = ORPHAN_CLOSE_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)</name[^>]*>"#).unwrap()
    });
    re4.replace_all(&pass3, "").to_string()
}

/// 人名一致性后处理结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameConsistencyResult {
    /// 最终的译名表（合并预扫描 glossary + 标签提取的新名字）
    pub final_glossary: Vec<ExtractedName>,
    /// 发现的不一致人名（同一英文名有多个中文译名）
    pub inconsistencies: Vec<NameInconsistency>,
    /// 修正后的翻译条目（标签已剥离）
    pub corrected_indices: Vec<(usize, String)>,
}

/// 单个人名不一致记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameInconsistency {
    pub english: String,
    pub translations: Vec<String>,
    /// 选定的标准译名（频率优先，平局取首次出现）
    pub chosen: String,
}

/// 对翻译结果执行人名一致性后处理
/// 1. 从所有译文中提取 <name=En>Zh</name> 标签
/// 2. 合并预扫描 glossary 和标签提取的人名
/// 3. 检测不一致（同一英文名多个中文译名）
/// 4. 对不一致的译名执行全局替换
/// 5. 剥离所有 <name> 标签
pub fn post_process_name_tags(
    translations: &mut [String],
    pre_scan_glossary: &[(String, String)],
) -> NameConsistencyResult {
    use std::collections::HashMap;

    // 0. 规范化畸形标签：<name>Snowball>雪球</name> → <name=Snowball>雪球</name>
    // llama-3.1-8b 等小模型常见错误，<name> 后多了英文名和 >
    static MALFORMED_NAME_GT_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re_malformed = MALFORMED_NAME_GT_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)<name\s*>([A-Za-z\s]+)>(.*?)</name\s*>"#).unwrap()
    });
    for tr in translations.iter_mut() {
        let normalized = re_malformed.replace_all(tr, |caps: &regex::Captures| {
            let en = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let zh = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            format!("<name={}>{}</name>", en, zh)
        }).to_string();
        *tr = normalized;
    }

    // 1. 从所有译文中提取标签
    // en_name -> (zh_name -> count)
    let mut tag_stats: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for tr in translations.iter() {
        for (en, zh) in extract_name_tags(tr) {
            *tag_stats.entry(en).or_default().entry(zh).or_default() += 1;
        }
    }

    // 2. 合并预扫描 glossary 和标签提取的人名
    // 预扫描 glossary 优先（用户已确认），标签提取的作为补充
    let mut final_map: HashMap<String, String> = HashMap::new();
    // 先放预扫描结果
    for (en, zh) in pre_scan_glossary {
        final_map.insert(en.clone(), zh.clone());
    }
    // 再放标签提取的（不覆盖预扫描）
    let mut inconsistencies: Vec<NameInconsistency> = Vec::new();
    for (en, zh_map) in &tag_stats {
        if final_map.contains_key(en) {
            // 预扫描已有此名字，检查标签中的译名是否一致
            let chosen = final_map[en].clone();
            let mut all_translations: Vec<String> = zh_map.keys().cloned().collect();
            if !all_translations.contains(&chosen) {
                all_translations.push(chosen.clone());
            }
            if all_translations.len() > 1 {
                inconsistencies.push(NameInconsistency {
                    english: en.clone(),
                    translations: all_translations,
                    chosen: chosen.clone(),
                });
            }
            // 用预扫描的译名替换标签中的不一致译名
        } else {
            // 预扫描没有此名字，从标签中选频率最高的
            let best = zh_map
                .iter()
                .max_by_key(|(_, count)| *count)
                .map(|(zh, _)| zh.clone())
                .unwrap_or_default();
            let all_translations: Vec<String> = zh_map.keys().cloned().collect();
            if all_translations.len() > 1 {
                inconsistencies.push(NameInconsistency {
                    english: en.clone(),
                    translations: all_translations.clone(),
                    chosen: best.clone(),
                });
            }
            final_map.insert(en.clone(), best);
        }
    }

    // 3. 全局替换：把所有标签中的不一致译名替换为标准译名，然后剥离标签
    let mut corrected_indices: Vec<(usize, String)> = Vec::new();
    let re = NAME_TAG_RE.get_or_init(|| {
        regex::Regex::new(r#"(?i)<name(?:=([^>]*))?\s*>(.*?)</name\s*>"#).unwrap()
    });
    for (i, tr) in translations.iter_mut().enumerate() {
        let original = tr.clone();
        let replaced = re.replace_all(tr, |caps: &regex::Captures| {
            // 组 1 可能为 None（<name>Zh</name> 格式）
            let en = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let zh = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            // 无英文名时用中文译名查找 final_map
            let lookup_key = if en.is_empty() { zh } else { en };
            // llama4 等模型可能输出 <name>EnglishName</name>ChineseName 格式：
            // 标签内容是英文名而非中文译名，中文译名跟在标签外面。
            // 此时 strip_name_tags 会保留标签内容（英文名），导致译名重复。
            // 检测：en 为空且 zh 全为 ASCII 字母（是英文名而非中文译名），
            // 返回空字符串移除整个标签，让标签外的中文译名保留。
            if en.is_empty() && zh.chars().all(|c| c.is_ascii_alphabetic() || c == ' ') {
                return String::new();
            }
            if let Some(standard) = final_map.get(lookup_key) {
                format!("<name={}>{}</name>", lookup_key, standard)
            } else {
                format!("<name={}>{}</name>", lookup_key, zh)
            }
        }).to_string();
        // 再剥离标签
        let stripped = strip_name_tags(&replaced);
        // 清理 CJK 字符之间的异常空格（GLM-5.2 等模型常见问题）
        let cleaned = cleanup_cjk_spaces(&stripped);
        if cleaned != original {
            *tr = cleaned.clone();
            corrected_indices.push((i, cleaned));
        }
    }

    // 4. 构建最终 glossary
    let mut final_glossary: Vec<ExtractedName> = final_map
        .iter()
        .map(|(en, zh)| ExtractedName {
            english: en.clone(),
            chinese: zh.clone(),
            alternatives: Vec::new(),
        })
        .collect();
    final_glossary.sort_by(|a, b| a.english.to_lowercase().cmp(&b.english.to_lowercase()));

    NameConsistencyResult {
        final_glossary,
        inconsistencies,
        corrected_indices,
    }
}


