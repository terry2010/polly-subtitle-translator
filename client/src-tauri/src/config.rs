// 配置与凭据管理
// 配置和凭据均存储在 SQLite 中（config 表 + credentials 表）
// 凭据原存储在系统 keyring，现已迁移到数据库 credentials 表

use crate::db::Database;
use crate::error::AppError;
use serde::{Deserialize, Serialize};

/// 通用配置读写
pub struct ConfigManager<'a> {
    db: &'a Database,
}

impl<'a> ConfigManager<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        self.db.get_config(key)
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.db.set_config(key, value)
    }

    pub fn delete(&self, key: &str) -> Result<(), AppError> {
        self.db.delete_config(key)
    }

    pub fn get_or_default<T: serde::de::DeserializeOwned + Serialize>(
        &self,
        key: &str,
        default: T,
    ) -> Result<T, AppError> {
        match self.get(key)? {
            Some(json) => serde_json::from_str(&json).map_err(|e| {
                tracing::warn!("配置解析失败 key={}: {}", key, e);
                AppError::Unknown {
                    detail: format!("config parse error: {}", e),
                }
            }),
            None => Ok(default),
        }
    }

    pub fn set_json<T: Serialize>(&self, key: &str, value: &T) -> Result<(), AppError> {
        let json = serde_json::to_string(value)?;
        self.set(key, &json)
    }
}

/// 凭据存储（数据库 credentials 表）
pub struct CredentialStore;

impl CredentialStore {
    /// 保存凭据到数据库
    pub fn save(db: &Database, provider: &str, key: &str, value: &str) -> Result<(), AppError> {
        let entry_name = format!("{}:{}", provider, key);
        db.set_credential(&entry_name, value)?;
        tracing::info!("凭据已保存: {}", entry_name);
        Ok(())
    }

    /// 从数据库读取凭据
    /// reason: 查询原因，用于日志记录（如 "翻译字幕"、"启动检查配置状态"）
    pub fn load(db: &Database, provider: &str, key: &str, reason: &str) -> Result<String, AppError> {
        let entry_name = format!("{}:{}", provider, key);
        tracing::debug!("凭据查询 entry={} reason={}", entry_name, reason);
        match db.get_credential(&entry_name)? {
            Some(v) => Ok(v),
            None => Err(AppError::StorageCredentialNotFound {
                provider: provider.to_string(),
            }),
        }
    }

    /// 删除数据库中的凭据
    pub fn delete(db: &Database, provider: &str, key: &str) -> Result<(), AppError> {
        let entry_name = format!("{}:{}", provider, key);
        db.delete_credential(&entry_name)?;
        tracing::info!("凭据已删除: {}", entry_name);
        Ok(())
    }
}

// === SECTION 1 END ===

/// 翻译 API 提供商配置（存储在 api_provider 表 + credentials 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiProviderConfig {
    pub id: String,
    pub provider: String, // baidu / bing / google
    pub app_id: Option<String>,
    pub region: Option<String>,
    pub is_default: bool,
    pub enabled: bool,
}

/// 字幕搜索提供商配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchProviderConfig {
    pub id: String,
    pub provider: String, // opensubtitles
    pub is_default: bool,
    pub enabled: bool,
}

/// 通用设置（存储在 config 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub language: String,         // zh / en
    pub theme: String,            // light / dark / system
    pub default_source_lang: String,
    pub default_target_lang: String,
    pub log_level: String,        // trace / debug / info / warn / error
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            language: "zh".to_string(),
            theme: "system".to_string(),
            default_source_lang: "en".to_string(),
            default_target_lang: "zh".to_string(),
            log_level: "info".to_string(),
        }
    }
}

/// 播放器设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSettings {
    pub libmpv_downloaded: bool,
    pub libmpv_version: Option<String>,
    pub render_backend: String, // auto / software / gpu
    pub volume: u32,
    pub speed: f64,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            libmpv_downloaded: false,
            libmpv_version: None,
            render_backend: "auto".to_string(),
            volume: 100,
            speed: 1.0,
        }
    }
}

/// 配置键名常量
pub mod config_keys {
    pub const GENERAL: &str = "general";
    pub const PLAYER: &str = "player";
    pub const LIBMPV_DOWNLOADED: &str = "libmpv_downloaded";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_general_settings_default() {
        let s = GeneralSettings::default();
        assert_eq!(s.language, "zh");
        assert_eq!(s.default_source_lang, "en");
        assert_eq!(s.default_target_lang, "zh");
    }

    #[test]
    fn test_general_settings_serialize() {
        let s = GeneralSettings::default();
        let json = serde_json::to_string(&s).unwrap();
        let decoded: GeneralSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.language, s.language);
    }

    #[test]
    fn test_credential_store_db_roundtrip() {
        use crate::db::Database;
        let db = Database::open(std::path::Path::new(":memory:")).unwrap();
        db.migrate().unwrap();
        // 初始无凭据
        assert!(CredentialStore::load(&db, "test", "secret", "测试").is_err());
        // 保存
        CredentialStore::save(&db, "test", "secret", "abc123").unwrap();
        // 读取
        assert_eq!(CredentialStore::load(&db, "test", "secret", "测试").unwrap(), "abc123");
        // 删除
        CredentialStore::delete(&db, "test", "secret").unwrap();
        assert!(CredentialStore::load(&db, "test", "secret", "测试").is_err());
    }
}
