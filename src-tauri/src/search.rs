// 字幕搜索模块 - OpenSubtitles REST API 客户端
// 对应需求文档 §5：字幕搜索与下载
// 使用 reqwest blocking client（搜索 IPC 命令为同步调用）

use crate::error::AppError;
use serde::{Deserialize, Serialize};

/// OpenSubtitles REST API 基础地址
const OPENSUBTITLES_API_BASE: &str = "https://api.opensubtitles.com/v1";

/// 字幕搜索结果（序列化给前端）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubtitleSearchResult {
    /// 字幕文件名
    pub file_name: String,
    /// 语言代码（如 "en"、"zh-CN"）
    pub language: String,
    /// 下载次数
    pub download_count: u64,
    /// 评分
    pub rating: f64,
    /// 发行信息（release）
    pub release_info: String,
    /// OpenSubtitles 字幕文件 ID
    pub subtitle_id: String,
}

/// OpenSubtitles 搜索提供商
pub struct SearchProvider {
    /// HTTP 客户端（blocking）
    client: reqwest::blocking::Client,
}

impl SearchProvider {
    /// 创建新的 SearchProvider 实例
    pub fn new() -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .user_agent("zimufan v1.0")
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new()),
        }
    }

    /// 构造带鉴权头的请求 builder
    fn request(&self, url: &str, api_key: &str) -> reqwest::blocking::RequestBuilder {
        self.client
            .get(url)
            .header("Api-Key", api_key)
            .header("Content-Type", "application/json")
            .header("User-Agent", "zimufan v1.0")
    }

    /// 根据 HTTP 状态码映射为 AppError（provider 固定为 "opensubtitles"）
    fn map_status_error(status: u16) -> AppError {
        match status {
            401 | 403 => AppError::SearchAuthFailed {
                provider: "opensubtitles".to_string(),
            },
            429 => AppError::SearchQuotaExhausted {
                provider: "opensubtitles".to_string(),
            },
            _ => AppError::SearchNetworkError {
                provider: "opensubtitles".to_string(),
            },
        }
    }

    /// 将 reqwest 网络错误映射为 AppError
    fn map_network_error(_e: reqwest::Error) -> AppError {
        AppError::SearchNetworkError {
            provider: "opensubtitles".to_string(),
        }
    }
}

impl Default for SearchProvider {
    fn default() -> Self {
        Self::new()
    }
}

// === SECTION 1 END ===

/// 对查询字符串进行 URL 百分号编码（用于 GET 查询参数）
fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

/// OpenSubtitles /features 响应（仅提取需要的字段）
#[derive(Debug, Deserialize)]
struct FeaturesResponse {
    /// data 数组，每个元素含 attributes
    data: Vec<FeatureItem>,
}

#[derive(Debug, Deserialize)]
struct FeatureItem {
    attributes: FeatureAttributes,
}

#[derive(Debug, Deserialize)]
struct FeatureAttributes {
    /// 字幕文件名
    file_name: Option<String>,
    /// 语言代码
    language: Option<String>,
    /// 下载次数
    download_count: Option<u64>,
    /// 评分
    ratings: Option<f64>,
    /// 发行信息
    release: Option<String>,
    /// OpenSubtitles 文件 ID
    file_id: Option<i64>,
}

/// 搜索字幕
/// GET /features?query={query}&languages={language}
pub fn search_subtitles(
    query: &str,
    language: &str,
    api_key: &str,
) -> Result<Vec<SubtitleSearchResult>, AppError> {
    let provider = SearchProvider::new();
    let encoded_query = url_encode(query);
    let encoded_lang = url_encode(language);
    let url = format!(
        "{}/features?query={}&languages={}",
        OPENSUBTITLES_API_BASE, encoded_query, encoded_lang
    );

    let resp = provider
        .request(&url, api_key)
        .send()
        .map_err(SearchProvider::map_network_error)?;

    let status = resp.status().as_u16();
    if status != 200 {
        return Err(SearchProvider::map_status_error(status));
    }

    let body: FeaturesResponse = resp
        .json()
        .map_err(SearchProvider::map_network_error)?;

    let results = body
        .data
        .into_iter()
        .map(|item| {
            let a = item.attributes;
            SubtitleSearchResult {
                file_name: a.file_name.unwrap_or_default(),
                language: a.language.unwrap_or_default(),
                download_count: a.download_count.unwrap_or(0),
                rating: a.ratings.unwrap_or(0.0),
                release_info: a.release.unwrap_or_default(),
                subtitle_id: a
                    .file_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            }
        })
        .collect();

    Ok(results)
}

// === SECTION 2 END ===

/// OpenSubtitles /download 响应
#[derive(Debug, Deserialize)]
struct DownloadResponse {
    /// 实际字幕文件的下载链接
    link: String,
}

/// 下载字幕
/// POST /download { "file_id": {id} }，然后跟随响应中的 link 下载文件到 output_path
pub fn download_subtitle(
    subtitle_id: &str,
    api_key: &str,
    output_path: &std::path::Path,
) -> Result<(), AppError> {
    let provider = SearchProvider::new();
    let url = format!("{}/download", OPENSUBTITLES_API_BASE);

    // file_id 为数字字符串，构造 JSON body
    let file_id: i64 = subtitle_id
        .parse()
        .map_err(|_| AppError::SearchDownloadFailed {
            provider: "opensubtitles".to_string(),
        })?;

    let resp = provider
        .client
        .post(&url)
        .header("Api-Key", api_key)
        .header("Content-Type", "application/json")
        .header("User-Agent", "zimufan v1.0")
        .json(&serde_json::json!({ "file_id": file_id }))
        .send()
        .map_err(SearchProvider::map_network_error)?;

    let status = resp.status().as_u16();
    if status != 200 {
        return Err(SearchProvider::map_status_error(status));
    }

    let dl: DownloadResponse = resp
        .json()
        .map_err(SearchProvider::map_network_error)?;

    // 跟随 link 下载实际字幕文件
    let file_resp = provider
        .client
        .get(&dl.link)
        .header("User-Agent", "zimufan v1.0")
        .send()
        .map_err(SearchProvider::map_network_error)?;

    let file_status = file_resp.status().as_u16();
    if file_status != 200 {
        return Err(SearchProvider::map_status_error(file_status));
    }

    let bytes = file_resp
        .bytes()
        .map_err(SearchProvider::map_network_error)?;

    // 确保父目录存在
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(output_path, &bytes)?;

    Ok(())
}

// === SECTION 3 END ===

#[cfg(test)]
mod tests {
    use super::*;

    // === URL 编码测试 ===

    #[test]
    fn test_url_encode_plain_ascii() {
        assert_eq!(url_encode("hello"), "hello");
    }

    #[test]
    fn test_url_encode_unreserved_chars() {
        // - _ . ~ 不应被编码
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn test_url_encode_space() {
        // 空格编码为 %20（非 +）
        assert_eq!(url_encode("a b"), "a%20b");
    }

    #[test]
    fn test_url_encode_chinese() {
        // 中文字符按 UTF-8 字节百分号编码
        // "中" = E4 B8 AD, "文" = E6 96 87
        assert_eq!(url_encode("中文"), "%E4%B8%AD%E6%96%87");
    }

    #[test]
    fn test_url_encode_special_chars() {
        // & ? = / 等需编码
        assert_eq!(url_encode("a&b?c=d/e"), "a%26b%3Fc%3Dd%2Fe");
    }

    #[test]
    fn test_url_encode_empty() {
        assert_eq!(url_encode(""), "");
    }

    // === 结果解析测试 ===

    #[test]
    fn test_parse_subtitle_search_result_full() {
        let json = r#"{
            "file_name": "movie.zh-CN.srt",
            "language": "zh-CN",
            "download_count": 42,
            "rating": 8.5,
            "release_info": "1080p.BluRay",
            "subtitle_id": "123456"
        }"#;
        let r: SubtitleSearchResult = serde_json::from_str(json).unwrap();
        assert_eq!(r.file_name, "movie.zh-CN.srt");
        assert_eq!(r.language, "zh-CN");
        assert_eq!(r.download_count, 42);
        assert_eq!(r.rating, 8.5);
        assert_eq!(r.release_info, "1080p.BluRay");
        assert_eq!(r.subtitle_id, "123456");
    }

    #[test]
    fn test_parse_subtitle_search_result_minimal() {
        let json = r#"{
            "file_name": "sub.srt",
            "language": "en",
            "download_count": 0,
            "rating": 0.0,
            "release_info": "",
            "subtitle_id": "1"
        }"#;
        let r: SubtitleSearchResult = serde_json::from_str(json).unwrap();
        assert_eq!(r.file_name, "sub.srt");
        assert_eq!(r.language, "en");
        assert_eq!(r.download_count, 0);
        assert_eq!(r.rating, 0.0);
        assert_eq!(r.subtitle_id, "1");
    }

    #[test]
    fn test_parse_features_response() {
        let json = r#"{
            "data": [
                {
                    "attributes": {
                        "file_name": "a.srt",
                        "language": "en",
                        "download_count": 10,
                        "ratings": 7.0,
                        "release": "720p",
                        "file_id": 100
                    }
                },
                {
                    "attributes": {
                        "file_name": "b.srt",
                        "language": "zh",
                        "download_count": 5,
                        "ratings": 6.5,
                        "release": "1080p",
                        "file_id": 200
                    }
                }
            ]
        }"#;
        let resp: FeaturesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0].attributes.file_name.as_deref(), Some("a.srt"));
        assert_eq!(resp.data[1].attributes.file_id, Some(200));
    }

    #[test]
    fn test_parse_features_response_missing_fields() {
        // 缺失字段应回退为默认值（Option -> None）
        let json = r#"{ "data": [ { "attributes": {} } ] }"#;
        let resp: FeaturesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 1);
        assert!(resp.data[0].attributes.file_name.is_none());
        assert!(resp.data[0].attributes.file_id.is_none());
    }

    #[test]
    fn test_features_to_search_result_mapping() {
        let item = FeatureItem {
            attributes: FeatureAttributes {
                file_name: Some("x.srt".to_string()),
                language: Some("en".to_string()),
                download_count: Some(3),
                ratings: Some(9.0),
                release: Some("WEB".to_string()),
                file_id: Some(999),
            },
        };
        let a = item.attributes;
        let r = SubtitleSearchResult {
            file_name: a.file_name.unwrap_or_default(),
            language: a.language.unwrap_or_default(),
            download_count: a.download_count.unwrap_or(0),
            rating: a.ratings.unwrap_or(0.0),
            release_info: a.release.unwrap_or_default(),
            subtitle_id: a.file_id.map(|id| id.to_string()).unwrap_or_default(),
        };
        assert_eq!(r.subtitle_id, "999");
        assert_eq!(r.download_count, 3);
    }

    #[test]
    fn test_download_response_parse() {
        let json = r#"{ "link": "https://example.com/sub.srt" }"#;
        let dl: DownloadResponse = serde_json::from_str(json).unwrap();
        assert_eq!(dl.link, "https://example.com/sub.srt");
    }

    // === 错误映射测试 ===

    #[test]
    fn test_map_status_error_401() {
        let e = SearchProvider::map_status_error(401);
        match e {
            AppError::SearchAuthFailed { provider } => {
                assert_eq!(provider, "opensubtitles");
            }
            _ => panic!("expected SearchAuthFailed"),
        }
    }

    #[test]
    fn test_map_status_error_403() {
        let e = SearchProvider::map_status_error(403);
        assert!(matches!(
            e,
            AppError::SearchAuthFailed { .. }
        ));
    }

    #[test]
    fn test_map_status_error_429() {
        let e = SearchProvider::map_status_error(429);
        match e {
            AppError::SearchQuotaExhausted { provider } => {
                assert_eq!(provider, "opensubtitles");
            }
            _ => panic!("expected SearchQuotaExhausted"),
        }
    }

    #[test]
    fn test_map_status_error_other() {
        let e = SearchProvider::map_status_error(500);
        match e {
            AppError::SearchNetworkError { provider, .. } => {
                assert_eq!(provider, "opensubtitles");
            }
            _ => panic!("expected SearchNetworkError"),
        }
    }
}

// === SECTION FINAL END ===
