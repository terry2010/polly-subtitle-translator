// SQLite 数据库管理
// 表：config / api_provider / search_provider / translate_cache / history / recent_files
// 对应需求文档 §6 数据模型

use crate::error::AppError;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let conn = Connection::open(path).map_err(|e| {
            tracing::error!("数据库打开失败: {:?} - {}", path, e);
            if e.to_string().contains("database disk image is malformed") {
                AppError::StorageSqliteCorrupted {
                    path: path.display().to_string(),
                }
            } else {
                AppError::Rusqlite(e)
            }
        })?;

        // 启用 WAL 模式（并发读 + 单写）
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;
             PRAGMA cache_size=-64000;",
        )?;

        tracing::info!("数据库已打开: {:?}", path);
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn with_conn<F, R>(&self, f: F) -> Result<R, AppError>
    where
        F: FnOnce(&Connection) -> Result<R, AppError>,
    {
        let conn = self.conn.lock().expect("数据库互斥锁中毒");
        f(&conn)
    }

    /// 执行迁移脚本（按版本号顺序执行，记录到 schema_migrations 表）
    pub fn migrate(&self) -> Result<(), AppError> {
        self.with_conn(|conn| {
            // 创建 schema_migrations 表
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );",
            )?;

            for migration in MIGRATIONS {
                let applied: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
                        rusqlite::params![migration.version],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);

                if !applied {
                    tracing::info!("执行数据库迁移 v{}", migration.version);
                    conn.execute_batch(migration.sql)?;
                    conn.execute(
                        "INSERT INTO schema_migrations (version) VALUES (?1)",
                        rusqlite::params![migration.version],
                    )?;
                }
            }

            tracing::info!("数据库迁移完成");
            Ok(())
        })
    }
}

// === SECTION 1 END ===

struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
-- 配置表（键值对存储通用配置）
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 翻译 API 提供商表
CREATE TABLE IF NOT EXISTS api_provider (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,          -- baidu / bing / google
    app_id TEXT,                     -- 百度 App ID / Bing 不需要
    secret_key_ref TEXT,             -- keyring 引用名（不存明文）
    region TEXT,                     -- Bing 区域（如 global / china）
    is_default INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 字幕搜索提供商表
CREATE TABLE IF NOT EXISTS search_provider (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,          -- opensubtitles
    api_key_ref TEXT,                -- keyring 引用名（不存明文）
    is_default INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 翻译缓存表
CREATE TABLE IF NOT EXISTS translate_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cache_key TEXT NOT NULL,         -- sha256(原文含标记 + 源语言 + 目标语言 + provider)
    source_text TEXT NOT NULL,
    translated_text TEXT NOT NULL,
    source_lang TEXT NOT NULL,
    target_lang TEXT NOT NULL,
    provider TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 翻译缓存唯一索引（cache_key 唯一，避免重复缓存）
CREATE UNIQUE INDEX IF NOT EXISTS idx_translate_cache_key
    ON translate_cache(cache_key);

-- 翻译缓存查询索引（按源语言+目标语言+provider 查询）
CREATE INDEX IF NOT EXISTS idx_translate_cache_lang_provider
    ON translate_cache(source_lang, target_lang, provider);

-- 历史记录表
CREATE TABLE IF NOT EXISTS history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    video_path TEXT,
    subtitle_path TEXT,
    source_lang TEXT,
    target_lang TEXT,
    provider TEXT,
    action TEXT NOT NULL,            -- extract / translate / merge / edit / search
    status TEXT NOT NULL,            -- success / failed / cancelled
    detail TEXT,                     -- JSON 附加信息
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 历史记录索引（按时间倒序查询 + 按视频路径查询）
CREATE INDEX IF NOT EXISTS idx_history_created_at
    ON history(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_history_video_path
    ON history(video_path);

-- 最近文件表
CREATE TABLE IF NOT EXISTS recent_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL UNIQUE,
    file_type TEXT NOT NULL,         -- video / subtitle
    opened_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 最近文件索引（按打开时间倒序）
CREATE INDEX IF NOT EXISTS idx_recent_files_opened_at
    ON recent_files(opened_at DESC);
"#,
}, Migration {
    version: 2,
    sql: r#"
-- v2: translate_cache_key 算法变更（加入 file_hash），旧缓存无法命中，清空旧缓存
DELETE FROM translate_cache;
"#,
}, Migration {
    version: 3,
    sql: r#"
-- v3: 凭据存储从系统 keyring 迁移到数据库
-- key = "provider:key"（与原 keyring entry_name 一致），value = 凭据明文
CREATE TABLE IF NOT EXISTS credentials (
    entry_name TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
"#,
},
Migration {
    version: 2,
    sql: r#"
-- 批量翻译任务持久化
CREATE TABLE IF NOT EXISTS batch_tasks (
    id TEXT PRIMARY KEY,
    video_path TEXT NOT NULL,
    source_path_type TEXT NOT NULL,
    status TEXT NOT NULL,
    subtitle_path TEXT,
    output_path TEXT,
    source_lang TEXT,
    target_lang TEXT,
    provider TEXT,
    total_entries INTEGER DEFAULT 0,
    done_entries INTEGER DEFAULT 0,
    cached_entries INTEGER DEFAULT 0,
    failed_entries INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER,
    error TEXT
);
"#,
}];

// === SECTION 2 END ===

impl Database {
    /// 读取配置项
    pub fn get_config(&self, key: &str) -> Result<Option<String>, AppError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT value FROM config WHERE key = ?1")?;
            let result = stmt
                .query_row(rusqlite::params![key], |row| row.get::<_, String>(0))
                .ok();
            Ok(result)
        })
    }

    /// 写入配置项（UPSERT）
    pub fn set_config(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO config (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
                rusqlite::params![key, value],
            )?;
            Ok(())
        })
    }

    /// 删除配置项
    pub fn delete_config(&self, key: &str) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM config WHERE key = ?1", rusqlite::params![key])?;
            Ok(())
        })
    }

    /// 读取所有配置项
    pub fn get_all_config(&self) -> Result<Vec<(String, String)>, AppError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT key, value FROM config ORDER BY key")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    /// 添加最近文件
    pub fn add_recent_file(&self, path: &str, file_type: &str) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO recent_files (file_path, file_type, opened_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(file_path) DO UPDATE SET opened_at = datetime('now')",
                rusqlite::params![path, file_type],
            )?;
            // 只保留最近 20 条
            conn.execute(
                "DELETE FROM recent_files WHERE id NOT IN (
                    SELECT id FROM recent_files ORDER BY opened_at DESC LIMIT 20
                )",
                [],
            )?;
            Ok(())
        })
    }

    /// 获取最近文件列表
    pub fn get_recent_files(&self, file_type: Option<&str>) -> Result<Vec<RecentFile>, AppError> {
        self.with_conn(|conn| {
            let mut stmt = if let Some(ft) = file_type {
                let mut s = conn.prepare(
                    "SELECT id, file_path, file_type, opened_at FROM recent_files
                     WHERE file_type = ?1 ORDER BY opened_at DESC LIMIT 20",
                )?;
                let rows = s.query_map(rusqlite::params![ft], |row| {
                    Ok(RecentFile {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        file_type: row.get(2)?,
                        opened_at: row.get(3)?,
                    })
                })?;
                let mut result = Vec::new();
                for row in rows {
                    result.push(row?);
                }
                return Ok(result);
            } else {
                conn.prepare(
                    "SELECT id, file_path, file_type, opened_at FROM recent_files
                     ORDER BY opened_at DESC LIMIT 20",
                )?
            };

            let rows = stmt.query_map([], |row| {
                Ok(RecentFile {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    file_type: row.get(2)?,
                    opened_at: row.get(3)?,
                })
            })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    /// 添加历史记录
    pub fn add_history(&self, record: &HistoryRecord) -> Result<i64, AppError> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO history (video_path, subtitle_path, source_lang, target_lang, provider, action, status, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    record.video_path,
                    record.subtitle_path,
                    record.source_lang,
                    record.target_lang,
                    record.provider,
                    record.action,
                    record.status,
                    record.detail,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// 获取翻译缓存
    pub fn get_translate_cache(&self, cache_key: &str) -> Result<Option<String>, AppError> {
        self.with_conn(|conn| {
            let result = conn
                .query_row(
                    "SELECT translated_text FROM translate_cache WHERE cache_key = ?1",
                    rusqlite::params![cache_key],
                    |row| row.get::<_, String>(0),
                )
                .ok();
            Ok(result)
        })
    }

    /// 写入翻译缓存
    pub fn set_translate_cache(
        &self,
        cache_key: &str,
        source_text: &str,
        translated_text: &str,
        source_lang: &str,
        target_lang: &str,
        provider: &str,
    ) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO translate_cache
                    (cache_key, source_text, translated_text, source_lang, target_lang, provider)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    cache_key,
                    source_text,
                    translated_text,
                    source_lang,
                    target_lang,
                    provider,
                ],
            )?;
            Ok(())
        })
    }

    /// 清除翻译缓存
    pub fn clear_translate_cache(&self) -> Result<usize, AppError> {
        self.with_conn(|conn| {
            let count = conn.execute("DELETE FROM translate_cache", [])?;
            Ok(count)
        })
    }

    /// 读取凭据（从 credentials 表）
    /// entry_name 格式 "provider:key"，与原 keyring entry_name 一致
    pub fn get_credential(&self, entry_name: &str) -> Result<Option<String>, AppError> {
        self.with_conn(|conn| {
            let result = conn
                .query_row(
                    "SELECT value FROM credentials WHERE entry_name = ?1",
                    rusqlite::params![entry_name],
                    |row| row.get::<_, String>(0),
                )
                .ok();
            Ok(result)
        })
    }

    /// 保存凭据（UPSERT 到 credentials 表）
    pub fn set_credential(&self, entry_name: &str, value: &str) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO credentials (entry_name, value, updated_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(entry_name) DO UPDATE SET value = ?2, updated_at = datetime('now')",
                rusqlite::params![entry_name, value],
            )?;
            Ok(())
        })
    }

    /// 删除凭据
    pub fn delete_credential(&self, entry_name: &str) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM credentials WHERE entry_name = ?1",
                rusqlite::params![entry_name],
            )?;
            Ok(())
        })
    }

    /// 清除"假翻译"缓存条目：
    /// 1. 译文=原文（AI 未实际翻译，原样返回）
    /// 2. 目标语言是中文但译文无 CJK 字符（且原文也无 CJK）
    /// 3. 音效标记类型不一致（AI 错位翻译）
    /// 用 Rust 逐条判断，避免 SQLite GLOB 对 Unicode 范围支持不完整。
    pub fn purge_fake_translate_cache(&self) -> Result<usize, AppError> {
        fn has_cjk_chars(s: &str) -> bool {
            s.chars().any(|c| {
                let code = c as u32;
                (0x4E00..=0x9FFF).contains(&code)
            })
        }
        fn looks_like_sound_effect(s: &str) -> bool {
            let s = s.trim();
            if s.is_empty() {
                return false;
            }
            if s.starts_with('[') && s.ends_with(']') {
                return true;
            }
            let re = regex::Regex::new(r"^\s*\[[^\]]+\]\s*(.*)$").unwrap();
            if let Some(caps) = re.captures(s) {
                let rest = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                if !rest.is_empty() && rest.starts_with('[') && rest.ends_with(']') {
                    return true;
                }
            }
            false
        }
        let rows_to_delete: Vec<String> = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT cache_key, source_text, translated_text, target_lang FROM translate_cache",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            let mut keys = Vec::new();
            for row in rows {
                let (key, source, translated, target): (String, String, String, String) = row?;
                let trimmed_source = source.trim();
                let trimmed_translated = translated.trim();
                let same = trimmed_translated == trimmed_source;
                let no_cjk = target.starts_with("zh")
                    && !has_cjk_chars(&translated)
                    && !has_cjk_chars(&source);
                let sound_mismatch = looks_like_sound_effect(&source) != looks_like_sound_effect(&translated);
                if same || no_cjk || sound_mismatch {
                    keys.push(key);
                }
            }
            Ok(keys)
        })?;
        let count = rows_to_delete.len();
        if count == 0 {
            return Ok(0);
        }
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("DELETE FROM translate_cache WHERE cache_key = ?1")?;
            for key in &rows_to_delete {
                stmt.execute(rusqlite::params![key])?;
            }
            Ok(count)
        })
    }

    // === 批量翻译任务持久化 ===

    /// 保存批量任务（完整字段）
    pub fn upsert_batch_task(&self, task: &crate::batch::BatchTask) -> Result<(), AppError> {
        let status_str = serde_json::to_string(&task.status)
            .unwrap_or_else(|_| "\"Queued\"".to_string());
        let path_type_str = serde_json::to_string(&task.source_path_type)
            .unwrap_or_else(|_| "\"Video\"".to_string());
        self.with_conn(|conn| {
            // 安全兜底：确保表存在
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS batch_tasks (
                    id TEXT PRIMARY KEY,
                    video_path TEXT NOT NULL,
                    source_path_type TEXT NOT NULL,
                    status TEXT NOT NULL,
                    subtitle_path TEXT,
                    output_path TEXT,
                    source_lang TEXT,
                    target_lang TEXT,
                    provider TEXT,
                    total_entries INTEGER DEFAULT 0,
                    done_entries INTEGER DEFAULT 0,
                    cached_entries INTEGER DEFAULT 0,
                    failed_entries INTEGER DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    started_at INTEGER,
                    finished_at INTEGER,
                    error TEXT
                );",
            )?;
            conn.execute(
                "INSERT INTO batch_tasks (id, video_path, source_path_type, status, subtitle_path, output_path,
                    source_lang, target_lang, provider, total_entries, done_entries, cached_entries,
                    failed_entries, created_at, started_at, finished_at, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                 ON CONFLICT(id) DO UPDATE SET
                    status=excluded.status, subtitle_path=excluded.subtitle_path,
                    output_path=excluded.output_path, total_entries=excluded.total_entries,
                    done_entries=excluded.done_entries, cached_entries=excluded.cached_entries,
                    failed_entries=excluded.failed_entries, started_at=excluded.started_at,
                    finished_at=excluded.finished_at, error=excluded.error",
                rusqlite::params![
                    task.id,
                    task.video_path,
                    path_type_str,
                    status_str,
                    task.subtitle_path,
                    task.output_path,
                    task.source_lang,
                    task.target_lang,
                    task.provider,
                    task.total_entries,
                    task.done_entries,
                    task.cached_entries,
                    task.failed_entries,
                    task.created_at,
                    task.started_at,
                    task.finished_at,
                    task.error,
                ],
            )?;
            Ok(())
        })
    }

    /// 加载所有批量翻译任务（重启恢复用）
    pub fn load_batch_tasks(&self) -> Result<Vec<crate::batch::BatchTask>, AppError> {
        self.with_conn(|conn| {
            // 安全兜底：确保表存在（防止迁移残留状态导致表缺失）
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS batch_tasks (
                    id TEXT PRIMARY KEY,
                    video_path TEXT NOT NULL,
                    source_path_type TEXT NOT NULL,
                    status TEXT NOT NULL,
                    subtitle_path TEXT,
                    output_path TEXT,
                    source_lang TEXT,
                    target_lang TEXT,
                    provider TEXT,
                    total_entries INTEGER DEFAULT 0,
                    done_entries INTEGER DEFAULT 0,
                    cached_entries INTEGER DEFAULT 0,
                    failed_entries INTEGER DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    started_at INTEGER,
                    finished_at INTEGER,
                    error TEXT
                );",
            )?;

            let mut stmt = conn.prepare(
                "SELECT id, video_path, source_path_type, status, subtitle_path, output_path,
                    source_lang, target_lang, provider, total_entries, done_entries, cached_entries,
                    failed_entries, created_at, started_at, finished_at, error
                 FROM batch_tasks ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let video_path: String = row.get(1)?;
                let path_type_str: String = row.get(2)?;
                let status_str: String = row.get(3)?;
                let subtitle_path: Option<String> = row.get(4)?;
                let output_path: Option<String> = row.get(5)?;
                let source_lang: String = row.get(6).unwrap_or_default();
                let target_lang: String = row.get(7).unwrap_or_default();
                let provider: String = row.get(8).unwrap_or_default();
                let total_entries: usize = row.get(9).unwrap_or(0);
                let done_entries: usize = row.get(10).unwrap_or(0);
                let cached_entries: usize = row.get(11).unwrap_or(0);
                let failed_entries: usize = row.get(12).unwrap_or(0);
                let created_at: i64 = row.get(13).unwrap_or(0);
                let started_at: Option<i64> = row.get(14).ok().flatten();
                let finished_at: Option<i64> = row.get(15).ok().flatten();
                let error: Option<String> = row.get(16).ok().flatten();

                let source_path_type: crate::batch::PathType =
                    serde_json::from_str(&path_type_str).unwrap_or_default();
                let status: crate::batch::BatchStatus =
                    serde_json::from_str(&status_str).unwrap_or_default();

                Ok(crate::batch::BatchTask {
                    id,
                    video_path,
                    source_path_type,
                    status,
                    subtitle_path,
                    output_path,
                    source_lang,
                    target_lang,
                    provider,
                    total_entries,
                    done_entries,
                    cached_entries,
                    failed_entries,
                    created_at,
                    started_at,
                    finished_at,
                    error,
                })
            })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    /// 删除批量翻译任务
    pub fn delete_batch_task(&self, task_id: &str) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM batch_tasks WHERE id = ?1", rusqlite::params![task_id])?;
            Ok(())
        })
    }

    /// 清空所有批量翻译任务
    pub fn clear_batch_tasks(&self) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM batch_tasks", [])?;
            Ok(())
        })
    }
}

// === SECTION 3 END ===

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecentFile {
    pub id: i64,
    pub file_path: String,
    pub file_type: String,
    pub opened_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryRecord {
    pub video_path: Option<String>,
    pub subtitle_path: Option<String>,
    pub source_lang: Option<String>,
    pub target_lang: Option<String>,
    pub provider: Option<String>,
    pub action: String,
    pub status: String,
    pub detail: Option<String>,
}

/// 计算翻译缓存 key：sha256(原文含标记 + 源语言 + 目标语言 + provider + file_hash)
/// file_hash 基于字幕内容，确保不同字幕文件的相同句子不会命中同一缓存。
pub fn translate_cache_key(
    source_text: &str,
    source_lang: &str,
    target_lang: &str,
    provider: &str,
    file_hash: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source_text.as_bytes());
    hasher.update(b"|");
    hasher.update(source_lang.as_bytes());
    hasher.update(b"|");
    hasher.update(target_lang.as_bytes());
    hasher.update(b"|");
    hasher.update(provider.as_bytes());
    hasher.update(b"|");
    hasher.update(file_hash.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_deterministic() {
        let key1 = translate_cache_key("hello", "en", "zh", "baidu", "hash1");
        let key2 = translate_cache_key("hello", "en", "zh", "baidu", "hash1");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_differs_by_provider() {
        let key1 = translate_cache_key("hello", "en", "zh", "baidu", "hash1");
        let key2 = translate_cache_key("hello", "en", "zh", "google", "hash1");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_differs_by_text() {
        let key1 = translate_cache_key("hello", "en", "zh", "baidu", "hash1");
        let key2 = translate_cache_key("world", "en", "zh", "baidu", "hash1");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_differs_by_file_hash() {
        // 相同句子、相同 provider，但不同字幕文件 → 不同 cache key
        let key1 = translate_cache_key("hello", "en", "zh", "baidu", "fileA_hash");
        let key2 = translate_cache_key("hello", "en", "zh", "baidu", "fileB_hash");
        assert_ne!(key1, key2);
    }

    // === SECTION 4 END ===

    use rusqlite::Connection;

    /// 创建内存测试数据库并执行迁移
    fn test_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        for migration in MIGRATIONS {
            conn.execute_batch(migration.sql).unwrap();
        }
        Database { conn: Mutex::new(conn) }
    }

    #[test]
    fn test_config_set_get_delete() {
        let db = test_db();
        // 初始不存在
        assert_eq!(db.get_config("key1").unwrap(), None);
        // 写入
        db.set_config("key1", "value1").unwrap();
        assert_eq!(db.get_config("key1").unwrap(), Some("value1".to_string()));
        // 更新（UPSERT）
        db.set_config("key1", "value2").unwrap();
        assert_eq!(db.get_config("key1").unwrap(), Some("value2".to_string()));
        // 删除
        db.delete_config("key1").unwrap();
        assert_eq!(db.get_config("key1").unwrap(), None);
    }

    #[test]
    fn test_config_get_all() {
        let db = test_db();
        db.set_config("b", "2").unwrap();
        db.set_config("a", "1").unwrap();
        db.set_config("c", "3").unwrap();
        let all = db.get_all_config().unwrap();
        assert_eq!(all.len(), 3);
        // 按 key 排序
        assert_eq!(all[0].0, "a");
        assert_eq!(all[1].0, "b");
        assert_eq!(all[2].0, "c");
    }

    // === SECTION 5 END ===

    #[test]
    fn test_recent_files_add_and_query() {
        let db = test_db();
        db.add_recent_file("/video1.mkv", "video").unwrap();
        db.add_recent_file("/sub1.srt", "subtitle").unwrap();
        db.add_recent_file("/video2.mkv", "video").unwrap();

        // 查全部
        let all = db.get_recent_files(None).unwrap();
        assert_eq!(all.len(), 3);

        // 按类型筛选
        let videos = db.get_recent_files(Some("video")).unwrap();
        assert_eq!(videos.len(), 2);
        assert!(videos.iter().all(|f| f.file_type == "video"));

        let subs = db.get_recent_files(Some("subtitle")).unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].file_path, "/sub1.srt");
    }

    #[test]
    fn test_recent_files_dedup_and_update_time() {
        let db = test_db();
        db.add_recent_file("/video.mkv", "video").unwrap();
        db.add_recent_file("/video.mkv", "video").unwrap();
        let all = db.get_recent_files(None).unwrap();
        assert_eq!(all.len(), 1); // 同路径去重
    }

    #[test]
    fn test_recent_files_limit_20() {
        let db = test_db();
        for i in 0..25 {
            db.add_recent_file(&format!("/file{}.mkv", i), "video").unwrap();
        }
        let all = db.get_recent_files(None).unwrap();
        assert_eq!(all.len(), 20); // 只保留最近 20 条
    }

    // === SECTION 6 END ===

    #[test]
    fn test_history_add() {
        let db = test_db();
        let record = HistoryRecord {
            video_path: Some("/video.mkv".into()),
            subtitle_path: Some("/sub.srt".into()),
            source_lang: Some("en".into()),
            target_lang: Some("zh".into()),
            provider: Some("baidu".into()),
            action: "translate".into(),
            status: "success".into(),
            detail: Some(r#"{"count":10}"#.into()),
        };
        let id = db.add_history(&record).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_history_add_minimal() {
        let db = test_db();
        let record = HistoryRecord {
            video_path: None,
            subtitle_path: None,
            source_lang: None,
            target_lang: None,
            provider: None,
            action: "extract".into(),
            status: "failed".into(),
            detail: None,
        };
        let id = db.add_history(&record).unwrap();
        assert!(id > 0);
    }

    // === SECTION 7 END ===

    #[test]
    fn test_translate_cache_set_get() {
        let db = test_db();
        let key = translate_cache_key("hello", "en", "zh", "baidu", "hash1");
        // 初始无缓存
        assert_eq!(db.get_translate_cache(&key).unwrap(), None);
        // 写入缓存
        db.set_translate_cache(&key, "hello", "你好", "en", "zh", "baidu").unwrap();
        // 读取缓存
        assert_eq!(db.get_translate_cache(&key).unwrap(), Some("你好".to_string()));
    }

    #[test]
    fn test_translate_cache_replace() {
        let db = test_db();
        let key = "test_key";
        db.set_translate_cache(key, "hello", "你好", "en", "zh", "baidu").unwrap();
        db.set_translate_cache(key, "hello", "您好", "en", "zh", "baidu").unwrap();
        // OR REPLACE 覆盖
        assert_eq!(db.get_translate_cache(key).unwrap(), Some("您好".to_string()));
    }

    #[test]
    fn test_translate_cache_clear() {
        let db = test_db();
        db.set_translate_cache("k1", "a", "甲", "en", "zh", "baidu").unwrap();
        db.set_translate_cache("k2", "b", "乙", "en", "zh", "baidu").unwrap();
        let count = db.clear_translate_cache().unwrap();
        assert_eq!(count, 2);
        assert_eq!(db.get_translate_cache("k1").unwrap(), None);
        assert_eq!(db.get_translate_cache("k2").unwrap(), None);
    }

    // === SECTION 8 END ===

    #[test]
    fn test_migrate_idempotent() {
        let db = test_db();
        // 再次执行 migrate 不应报错
        db.migrate().unwrap();
        // schema_migrations 应有 v1、v2、v3 三条
        let count: i64 = db.with_conn(|conn| {
            Ok(conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))?)
        }).unwrap();
        assert_eq!(count, 3);
    }

    // === SECTION 9 END ===
}
