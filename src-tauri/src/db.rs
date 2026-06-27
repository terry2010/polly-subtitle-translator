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

/// 计算翻译缓存 key：sha256(原文含标记 + 源语言 + 目标语言 + provider)
pub fn translate_cache_key(
    source_text: &str,
    source_lang: &str,
    target_lang: &str,
    provider: &str,
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
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_deterministic() {
        let key1 = translate_cache_key("hello", "en", "zh", "baidu");
        let key2 = translate_cache_key("hello", "en", "zh", "baidu");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_differs_by_provider() {
        let key1 = translate_cache_key("hello", "en", "zh", "baidu");
        let key2 = translate_cache_key("hello", "en", "zh", "google");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_differs_by_text() {
        let key1 = translate_cache_key("hello", "en", "zh", "baidu");
        let key2 = translate_cache_key("world", "en", "zh", "baidu");
        assert_ne!(key1, key2);
    }
}
