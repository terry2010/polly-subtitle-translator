// Fixture 加载与管理
use std::path::{Path, PathBuf};
use zimufan_lib::subtitle::{self, SubtitleFile, SubtitleFormat};
#[allow(unused_imports)]
use super::config::TestConfig;

/// Fixture 元数据
#[derive(Debug, Clone)]
pub struct Fixture {
    pub name: String,
    pub file: String,
    pub format: SubtitleFormat,
    pub source_lang: String,
    pub target_lang: String,
    pub has_names: bool,
    pub has_sound_effects: bool,
    pub tags: Vec<String>,
}

impl Fixture {
    /// fixture 文件所在目录
    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
    }

    /// 加载 fixture 文件路径
    /// 如果 file 是绝对路径或相对路径（自定义 fixture），直接使用；
    /// 否则拼接 fixtures 目录（预定义 fixture）
    pub fn path(&self) -> PathBuf {
        let p = Path::new(&self.file);
        if p.is_absolute() || self.file.contains('/') || self.file.contains('\\') {
            p.to_path_buf()
        } else {
            Self::fixtures_dir().join(&self.file)
        }
    }

    /// 解析 fixture 为 SubtitleFile
    pub fn load_subtitle(&self) -> SubtitleFile {
        let path = self.path();
        subtitle::load_subtitle_file(path.to_str().unwrap())
            .unwrap_or_else(|e| panic!("加载 fixture {} 失败: {:?}", self.name, e))
    }

    /// 读取原始文件内容
    pub fn read_raw(&self) -> String {
        std::fs::read_to_string(self.path())
            .unwrap_or_else(|e| panic!("读取 fixture {} 失败: {:?}", self.name, e))
    }
}

/// 获取所有可用 fixture
pub fn all_fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "clarksons_farm".to_string(),
            file: "clarksons_farm.srt".to_string(),
            format: SubtitleFormat::Srt,
            source_lang: "en".to_string(),
            target_lang: "zh".to_string(),
            has_names: true,
            has_sound_effects: true,
            tags: vec!["documentary".into(), "long_sentences".into(), "sound_effects".into()],
        },
        Fixture {
            name: "rick_and_morty".to_string(),
            file: "rick_and_morty.srt".to_string(),
            format: SubtitleFormat::Srt,
            source_lang: "en".to_string(),
            target_lang: "zh".to_string(),
            has_names: true,
            has_sound_effects: false,
            tags: vec!["animation".into(), "dialogue".into(), "short_sentences".into()],
        },
        Fixture {
            name: "rick_s09e07".to_string(),
            file: "rick_s09e07.srt".to_string(),
            format: SubtitleFormat::Srt,
            source_lang: "en".to_string(),
            target_lang: "zh".to_string(),
            has_names: true,
            has_sound_effects: false,
            tags: vec!["animation".into(), "dialogue".into(), "real_world".into()],
        },
    ]
}

/// 按 name 过滤 fixture，或从文件路径创建自定义 fixture
pub fn select_fixtures(cfg: &super::config::TestConfig) -> Vec<Fixture> {
    // E2E_FIXTURE_FILE 优先：从文件路径创建自定义 fixture
    if let Some(path) = &cfg.fixture_file {
        let p = std::path::Path::new(path);
        let file_name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("custom").to_string();
        let format = if path.ends_with(".ass") || path.ends_with(".ssa") {
            SubtitleFormat::Ass
        } else if path.ends_with(".vtt") {
            SubtitleFormat::Vtt
        } else {
            SubtitleFormat::Srt
        };
        return vec![Fixture {
            name: file_name.clone(),
            file: path.to_string(),  // 直接用绝对/相对路径
            format,
            source_lang: "en".to_string(),
            target_lang: "zh".to_string(),
            has_names: true,
            has_sound_effects: false,
            tags: vec!["custom".into()],
        }];
    }

    // E2E_FIXTURE：按预定义名称过滤
    let all = all_fixtures();
    match &cfg.fixture_name {
        Some(name) => all.into_iter().filter(|f| &f.name == name).collect(),
        None => all,
    }
}
