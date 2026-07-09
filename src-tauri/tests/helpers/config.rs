// 测试配置与命令行参数解析
use std::env;

/// 测试层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// L1 结构断言
    L1,
    /// L1 + L2 语言正确性
    L2,
    /// L1 + L2 + L5 27b 判断（用缓存的翻译结果）
    Judge,
    /// 全量：L1 + L2 + 9b 翻译 + 27b 判断
    Full,
}

impl Tier {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "l1" => Some(Tier::L1),
            "l2" => Some(Tier::L2),
            "judge" => Some(Tier::Judge),
            "full" => Some(Tier::Full),
            _ => None,
        }
    }
}

/// 人名精译模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamePrecisionMode {
    On,
    Off,
    /// 跑两种模式
    Both,
}

impl NamePrecisionMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "on" => Some(NamePrecisionMode::On),
            "off" => Some(NamePrecisionMode::Off),
            "both" => Some(NamePrecisionMode::Both),
            _ => None,
        }
    }

    pub fn modes(&self) -> Vec<bool> {
        match self {
            NamePrecisionMode::On => vec![true],
            NamePrecisionMode::Off => vec![false],
            NamePrecisionMode::Both => vec![false, true],
        }
    }
}

/// 测试配置
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub tier: Tier,
    pub use_cache: bool,
    pub fixture_name: Option<String>,
    /// 自定义字幕文件路径（E2E_FIXTURE_FILE），优先于 fixture_name
    pub fixture_file: Option<String>,
    pub model_9b: String,
    pub model_27b: String,
    pub api_base: String,
    pub name_precision: NamePrecisionMode,
    pub no_cache: bool,
    pub stability: bool,
    pub format_matrix: bool,
    /// 翻译引擎：openai（默认，走 9b AI）/ baidu / deepl / google / bing / youdao / ...
    /// 不设置时默认 "openai"，走 9b AI 翻译（完全向后兼容）
    /// 设置为传统翻译引擎时，凭据从用户数据库自动读取
    pub translate_provider: String,
    /// OpenAI 兼容服务的 service_id（如 deepseek / siliconflow / lmstudio）
    /// 仅当 translate_provider=openai 且使用非默认 AI 服务时需要
    pub service_id: Option<String>,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            tier: Tier::L2,
            use_cache: false,
            fixture_name: None,
            fixture_file: None,
            model_9b: "qwen3.5-9b-uncensored-nothink".to_string(),
            model_27b: "qwen/qwen3.6-27b".to_string(),
            api_base: "http://localhost:1234/v1".to_string(),
            name_precision: NamePrecisionMode::Both,
            no_cache: false,
            stability: false,
            format_matrix: false,
            translate_provider: "openai".to_string(),
            service_id: None,
        }
    }
}

/// 从环境变量解析测试参数
/// 用法: E2E_TIER=full E2E_FIXTURE=clarksons_farm cargo test --test e2e
pub fn parse_test_config() -> TestConfig {
    let mut cfg = TestConfig::default();

    if let Ok(v) = env::var("E2E_TIER") {
        if let Some(t) = Tier::from_str(&v) {
            cfg.tier = t;
        }
    }
    if let Ok(v) = env::var("E2E_USE_CACHE") {
        if v == "1" || v == "true" {
            cfg.use_cache = true;
        }
    }
    if let Ok(v) = env::var("E2E_FIXTURE") {
        cfg.fixture_name = Some(v);
    }
    if let Ok(v) = env::var("E2E_FIXTURE_FILE") {
        cfg.fixture_file = Some(v);
    }
    if let Ok(v) = env::var("E2E_MODEL_9B") {
        eprintln!("  [配置] E2E_MODEL_9B 环境变量: {}", v);
        cfg.model_9b = v;
    } else {
        eprintln!("  [配置] E2E_MODEL_9B 环境变量未设置，使用默认值: {}", cfg.model_9b);
    }
    if let Ok(v) = env::var("E2E_MODEL_27B") {
        cfg.model_27b = v;
    }
    if let Ok(v) = env::var("E2E_API_BASE") {
        cfg.api_base = v;
    }
    if let Ok(v) = env::var("E2E_NAME_PRECISION") {
        if let Some(m) = NamePrecisionMode::from_str(&v) {
            cfg.name_precision = m;
        }
    }
    if let Ok(v) = env::var("E2E_NO_CACHE") {
        if v == "1" || v == "true" {
            cfg.no_cache = true;
        }
    }
    if let Ok(v) = env::var("E2E_STABILITY") {
        if v == "1" || v == "true" {
            cfg.stability = true;
        }
    }
    if let Ok(v) = env::var("E2E_FORMAT_MATRIX") {
        if v == "1" || v == "true" {
            cfg.format_matrix = true;
        }
    }
    if let Ok(v) = env::var("E2E_PROVIDER") {
        cfg.translate_provider = v;
    }
    if let Ok(v) = env::var("E2E_SERVICE_ID") {
        cfg.service_id = Some(v);
    }

    cfg
}
