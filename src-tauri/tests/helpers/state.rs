// 可恢复状态机：分阶段翻译+验证，每阶段保存状态可断点恢复
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zimufan_lib::subtitle::SubtitleFile;

/// 单条翻译记录（可序列化，用于状态保存）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationRecord {
    pub index: usize,
    pub original: String,
    pub translated: String,
    pub failed: bool,
}

/// 27b 验证记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeRecord {
    pub index: usize,
    pub verdict: String,
    pub reason: Option<String>,
    pub suggestion: Option<String>,
}

/// 专有名词记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameRecord {
    pub english: String,
    pub chinese: String,
}

/// 单个批次的状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchState {
    pub batch_num: usize,
    pub start: usize,
    pub end: usize,
    pub translations: Vec<TranslationRecord>,
    pub judge_results: Vec<JudgeRecord>,
    pub judge_pass: usize,
    pub judge_fail: usize,
    pub judge_shift: usize,
    pub status: BatchStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BatchStatus {
    /// 待翻译
    Pending,
    /// 9b 翻译完成
    Translated,
    /// 27b 验证完成
    Judged,
    /// 验证通过
    Passed,
    /// 验证发现 bug，需修复
    BugFound,
}

/// 完整的测试状态（序列化到 JSON 文件）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestState {
    pub fixture_name: String,
    pub name_precision: bool,
    pub source_lang: String,
    pub target_lang: String,
    pub total_entries: usize,
    pub batch_size: usize,

    /// 阶段 0：专有名词预扫描
    pub names: Vec<NameRecord>,
    pub names_status: BatchStatus,

    /// 阶段 1+：各批次翻译+验证
    pub batches: Vec<BatchState>,

    /// 当前阶段（0=名词预扫描, 1+=翻译批次N）
    pub current_stage: usize,

    /// 全局统计
    pub total_tokens: u64,
    pub total_failed: usize,
    pub total_cached: usize,
}

// === SECTION 1 END ===

impl TestState {
    /// 创建新状态
    pub fn new(
        fixture_name: &str,
        name_precision: bool,
        source_lang: &str,
        target_lang: &str,
        total_entries: usize,
        batch_size: usize,
    ) -> Self {
        // 预计算批次
        let mut batches = Vec::new();
        let mut batch_num = 1;
        for start in (0..total_entries).step_by(batch_size) {
            let end = (start + batch_size).min(total_entries);
            batches.push(BatchState {
                batch_num,
                start,
                end,
                translations: Vec::new(),
                judge_results: Vec::new(),
                judge_pass: 0,
                judge_fail: 0,
                judge_shift: 0,
                status: BatchStatus::Pending,
            });
            batch_num += 1;
        }

        Self {
            fixture_name: fixture_name.to_string(),
            name_precision,
            source_lang: source_lang.to_string(),
            target_lang: target_lang.to_string(),
            total_entries,
            batch_size,
            names: Vec::new(),
            names_status: BatchStatus::Pending,
            batches,
            current_stage: 0,
            total_tokens: 0,
            total_failed: 0,
            total_cached: 0,
        }
    }

    /// 状态文件路径
    pub fn state_file_path(fixture_name: &str, name_precision: bool) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("state");
        std::fs::create_dir_all(&dir).ok();
        let mode = if name_precision { "np" } else { "plain" };
        dir.join(format!("{}_{}.json", fixture_name, mode))
    }

    /// 保存状态到文件
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::state_file_path(&self.fixture_name, self.name_precision);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        eprintln!("  [状态] 已保存到 {}", path.display());
        Ok(())
    }

    /// 从文件加载状态
    pub fn load(fixture_name: &str, name_precision: bool) -> Option<Self> {
        let path = Self::state_file_path(fixture_name, name_precision);
        let json = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&json).ok()
    }

    /// 清除状态文件（重新开始）
    pub fn clear(fixture_name: &str, name_precision: bool) {
        let path = Self::state_file_path(fixture_name, name_precision);
        let _ = std::fs::remove_file(&path);
    }

    /// 获取当前待执行的批次索引
    pub fn current_batch(&self) -> Option<&BatchState> {
        if self.current_stage == 0 {
            return None; // 还在名词预扫描阶段
        }
        self.batches.get(self.current_stage - 1)
    }

    /// 获取当前待执行的批次索引（可变）
    pub fn current_batch_mut(&mut self) -> Option<&mut BatchState> {
        if self.current_stage == 0 {
            return None;
        }
        self.batches.get_mut(self.current_stage - 1)
    }

    /// 构建已翻译条目的 SubtitleFile（用于上下文）
    pub fn build_translated_file(&self, original: &SubtitleFile) -> SubtitleFile {
        let mut file = original.clone();
        for batch in &self.batches {
            for tr in &batch.translations {
                if let Some(entry) = file.entries.iter_mut().find(|e| e.index == tr.index) {
                    entry.translated = tr.translated.clone();
                    entry.failed = tr.failed;
                }
            }
        }
        file
    }

    /// 构建截至某批的上下文译文（已翻译的条目）
    pub fn context_translations(&self, up_to_batch: usize) -> Vec<TranslationRecord> {
        let mut all = Vec::new();
        for batch in &self.batches[..up_to_batch.min(self.batches.len())] {
            all.extend(batch.translations.iter().cloned());
        }
        all
    }

    /// 汇总报告
    pub fn summary(&self) -> String {
        let total_pass: usize = self.batches.iter().map(|b| b.judge_pass).sum();
        let total_fail: usize = self.batches.iter().map(|b| b.judge_fail).sum();
        let total_shift: usize = self.batches.iter().map(|b| b.judge_shift).sum();
        format!(
            "阶段 {} | 名词: {} | 批次: {}/{} | 27b: {}pass/{}fail/{}shift | token: {} | 缓存: {} | 失败: {}",
            self.current_stage,
            self.names.len(),
            self.batches.iter().filter(|b| b.status == BatchStatus::Passed).count(),
            self.batches.len(),
            total_pass, total_fail, total_shift,
            self.total_tokens, self.total_cached, self.total_failed,
        )
    }
}

// === SECTION 2 END ===
