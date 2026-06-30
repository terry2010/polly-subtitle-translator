// 字幕搜索模块 - OpenSubtitles REST API 客户端
// 对应需求文档 §5：字幕搜索与下载
// 使用 reqwest blocking client（搜索 IPC 命令为同步调用）

use crate::error::AppError;
use serde::{Deserialize, Serialize};

/// 遍历错误链，提取完整的错误信息
fn format_error_chain(e: &dyn std::error::Error) -> String {
    let mut msg = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        msg.push_str(" -> ");
        msg.push_str(&s.to_string());
        source = s.source();
    }
    msg
}

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
                detail: format!("HTTP {}", status),
            },
        }
    }

    /// 将 reqwest 网络错误映射为 AppError
    fn map_network_error(e: reqwest::Error) -> AppError {
        AppError::SearchNetworkError {
            provider: "opensubtitles".to_string(),
            detail: e.to_string(),
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

// === 关键词简化 ===

/// 简化视频文件名为搜索关键词。
/// 去掉字幕组、编码、分辨率、来源等信息，保留片名 + 季集信息。
///
/// 示例：
///   "The.Big.Bang.Theory.S12E24.1080p.WEB-DL.x264-RARBG" → "The Big Bang Theory S12E24"
///   "三体.3Body.Problem.S01E05.2160p.NETFLIX.WEB-DL" → "3 Body Problem S01E05"
///   "Blade.Runner.2049.2017.2160p.UHD.BluRay.x265" → "Blade Runner 2049 2017"
pub fn simplify_keyword(filename: &str) -> String {
    // 去扩展名
    let name = match filename.rfind('.') {
        Some(i) => &filename[..i],
        None => filename,
    };

    // 把分隔符 . _ - 统一替换为空格
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '.' | '_' | '-' => ' ',
            _ => c,
        })
        .collect();

    // 多空格合并
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    let s = s.trim();

    // 按空格分词
    let tokens: Vec<&str> = s.split(' ').collect();
    let mut result: Vec<&str> = Vec::new();

    // 需要过滤掉的编码/来源/字幕组关键词（小写匹配）
    let skip_tokens: &[&str] = &[
        "1080p", "2160p", "720p", "480p", "4k", "uhd", "hdr", "hdr10",
        "web-dl", "webdl", "webrip", "web",
        "bluray", "blu-ray", "bdrip", "brrip",
        "hdtv", "hdrip", "dvdrip", "dvd",
        "x264", "x265", "h264", "h265", "avc", "hevc", "avc1",
        "aac", "ac3", "ddp5", "dd5", "ddp", "dd", "mp3", "flac", "dts", "truehd", "atmos",
        "10bit", "8bit", "sdr",
        "amzn", "nf", "netflix", "hulu", "hbo", "atvp", "disney", "dcu",
        "hmax", "pmtp", "sho", "stz", "tubi", "crav",
        "rarbg", "ettv", "eztv", "rartv", "ntb", "galaxyrg", "fgt",
        "yts", "yify", "mx", "rmteam", "psa", "tomorrowland",
        "cm", "cmrg", "evo", "hive", "cm8", "stb", "fgt",
        "nzb", "torrent", "download",
        "internal", "proper", "repack", "remux", "extended", "unrated",
        "directors", "cut", "remastered", "imax", "hybrid",
        "dual", "audio", "subbed", "sub", "subs",
        "retail", "ws", "complete", "french", "german", "spanish", "italian",
        "multi", "vff", "vfi", "vo", "vostfr",
        "gbps", "mbps",
    ];

    // 中文常见字幕组/标记（精确匹配 token）
    let skip_cn: &[&str] = &[
        "简体", "繁体", "繁體", "简體", "双语", "雙語", "中英", "英中",
        "内封", "内嵌", "外挂", "字幕", "字幕组", "字幕組",
        "人人影视", "圣城家园", "圣城", "电波", "电波字幕组",
        "深影", "深影字幕组", "小幻影视", "幻影字幕组",
        "猫影", "片源", "压制", "译制", "翻译",
        "国英", "国语", "粤语", "中字", "中文字幕",
        "原盘", "原声", "导演剪辑", "加长版",
    ];

    for token in &tokens {
        let lower = token.to_lowercase();
        // 跳过编码/来源等英文标记
        if skip_tokens.iter().any(|&s| lower == s || lower.starts_with(s)) {
            continue;
        }
        // 跳过中文字幕组/标记
        if skip_cn.iter().any(|&s| *token == s) {
            continue;
        }
        // 跳过纯数字的分辨率标记如 "1920x1080"
        if token.contains('x') && token.chars().filter(|c| c.is_ascii_digit()).count() > 4 {
            continue;
        }
        // 跳过方括号/圆括号包裹的标记 [xxx] (xxx)
        if (token.starts_with('[') || token.starts_with('('))
            && (token.ends_with(']') || token.ends_with(')'))
        {
            continue;
        }
        result.push(*token);
    }

    // 如果过滤后为空，返回原始名（去扩展名）
    if result.is_empty() {
        return name.replace('.', " ").replace('_', " ").replace('-', " ");
    }

    result.join(" ")
}

// === SECTION 4 END ===

// === SubHD 搜索 ===

use scraper::{Html, Selector};
use std::str::FromStr;

const SUBHD_BASE: &str = "https://subhd.tv";

/// SubHD 搜索结果条目（内部用，含详情页链接和下载所需的元数据）
#[derive(Debug, Clone)]
struct SubhdEntry {
    title: String,
    detail_url: String,
    language: String,
    ext: String,
    download_count: u64,
}

/// 搜索 SubHD 字幕
/// 解析 SubHD 搜索结果 HTML（2024+ 新版结构）
fn subhd_parse_search_results(html: &str) -> Vec<SubhdEntry> {
    let document = Html::parse_document(html);
    let mut entries = Vec::new();

    // 新版结构：每个字幕条目在 div.bg-white.shadow-sm.rounded-3.mb-4 中
    let item_sel = match Selector::parse("div.bg-white.shadow-sm.rounded-3.mb-4") {
        Ok(s) => s,
        Err(_) => return entries,
    };

    // 条目内的链接：div.float-start.f16.fw-bold > a（标题链接）
    let title_sel = match Selector::parse("div.float-start.f16.fw-bold a") {
        Ok(s) => s,
        Err(_) => return entries,
    };

    // 副标题链接：div.view-text > a
    let subtitle_sel = match Selector::parse("div.view-text a") {
        Ok(s) => s,
        Err(_) => return entries,
    };

    // 语言/格式：div.text-truncate.py-2
    let format_sel = match Selector::parse("div.text-truncate.py-2") {
        Ok(s) => s,
        Err(_) => return entries,
    };

    for div in document.select(&item_sel) {
        // 主标题链接（如"瑞克和莫蒂 第九季"）
        let title_a = match div.select(&title_sel).next() {
            Some(a) => a,
            None => continue,
        };
        let main_title = title_a.text().collect::<String>().trim().to_string();
        let href = title_a.value().attr("href").unwrap_or_default().trim().to_string();
        if href.is_empty() {
            continue;
        }
        let detail_url = if href.starts_with("http") {
            href.clone()
        } else {
            format!("{}{}", SUBHD_BASE, href)
        };

        // 副标题（如 Rick.and.Morty.S09E06.CHS&ENG）
        let subtitle = div
            .select(&subtitle_sel)
            .next()
            .map(|a| a.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 组合标题：优先用副标题（含剧集信息），主标题作为补充
        let title = if !subtitle.is_empty() {
            subtitle
        } else {
            main_title
        };

        // 语言和格式
        let format_text = div
            .select(&format_sel)
            .next()
            .map(|d| d.text().collect::<String>())
            .unwrap_or_default();

        let language = subhd_detect_language(&format_text);
        let ext = subhd_detect_ext(&format_text);

        // 下载次数：在 div.pt-2 内，bi-download 图标后面的 span
        let download_count = {
            let pt2_sel = match Selector::parse("div.pt-2") {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut count = 0u64;
            if let Some(pt2) = div.select(&pt2_sel).next() {
                // bi-download 后面的 span 包含下载次数
                let text = pt2.text().collect::<String>();
                // 找到类似 "60" 的数字（下载次数通常较小，在文件大小之后）
                let nums: Vec<&str> = text.split_whitespace().collect();
                // 第二个数字通常是下载次数（第一个是文件大小如 25k）
                for (i, n) in nums.iter().enumerate() {
                    if i == 1 {
                        // 第二个数字是下载次数
                        let cleaned: String = n.chars().filter(|c| c.is_ascii_digit()).collect();
                        if let Ok(v) = cleaned.parse::<u64>() {
                            count = v;
                        }
                    }
                }
            }
            count
        };

        entries.push(SubhdEntry {
            title,
            detail_url,
            language,
            ext,
            download_count,
        });
    }

    entries
}

/// 从格式文本检测语言
fn subhd_detect_language(text: &str) -> String {
    if text.contains("双语") || text.contains("中英") || text.contains("英中") {
        "zh-en".to_string()
    } else if text.contains("简体") || text.contains("简體") {
        "zh-CN".to_string()
    } else if text.contains("繁體") || text.contains("繁体") {
        "zh-TW".to_string()
    } else if text.contains("English") || text.contains("英文") {
        "en".to_string()
    } else {
        "unknown".to_string()
    }
}

/// 从格式文本检测字幕格式
fn subhd_detect_ext(text: &str) -> String {
    let lower = text.to_lowercase();
    if lower.contains("ass") {
        "ass".to_string()
    } else if lower.contains("srt") {
        "srt".to_string()
    } else if lower.contains("vtt") {
        "vtt".to_string()
    } else {
        "srt".to_string()
    }
}

/// 下载 SubHD 字幕。
/// 先尝试正常下载接口，遇到验证码则走预览接口。
fn subhd_download(subtitle_id: &str, output_path: &std::path::Path) -> Result<(), AppError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|_| AppError::SearchNetworkError { provider: "subhd".to_string(), detail: String::new() })?;

    // subtitle_id 格式: "subhd:https://subhd.tv/detail/12345"
    let detail_url = subtitle_id
        .strip_prefix("subhd:")
        .ok_or_else(|| AppError::SearchDownloadFailed { provider: "subhd".to_string() })?;

    // 1. 访问详情页，获取下载按钮的 sid 和 dtoken1
    let resp = client
        .get(detail_url)
        .header("Referer", SUBHD_BASE)
        .send()
        .map_err(|_| AppError::SearchNetworkError { provider: "subhd".to_string(), detail: String::new() })?;

    if !resp.status().is_success() {
        return Err(AppError::SearchNetworkError { provider: "subhd".to_string(), detail: String::new() });
    }

    let html_text = resp
        .text()
        .map_err(|_| AppError::SearchNetworkError { provider: "subhd".to_string(), detail: String::new() })?;
    let document = Html::parse_document(&html_text);

    // 尝试下载接口
    if let Some(content) = subhd_try_download(&client, &document, detail_url) {
        return subhd_write_content(&content, output_path);
    }

    // 下载接口失败（验证码），走预览接口
    tracing::info!("SubHD 下载接口遇到验证码，尝试预览接口");
    if let Some(content) = subhd_try_preview(&client, &document, detail_url) {
        return subhd_write_content(&content, output_path);
    }

    Err(AppError::SearchDownloadFailed { provider: "subhd".to_string() })
}

/// 尝试 SubHD 下载接口 /ajax/down_ajax
fn subhd_try_download(
    client: &reqwest::blocking::Client,
    document: &Html,
    detail_url: &str,
) -> Option<Vec<u8>> {
    let btn_sel = Selector::parse("button[sid][dtoken1]").ok()?;
    let btn = document.select(&btn_sel).next()?;
    let sid = btn.value().attr("sid")?;
    let dtoken1 = btn.value().attr("dtoken1")?;

    let api_url = format!("{}/ajax/down_ajax", SUBHD_BASE);
    let resp = client
        .post(&api_url)
        .json(&serde_json::json!({
            "sub_id": sid,
            "dtoken1": dtoken1,
        }))
        .header("Referer", detail_url)
        .send()
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: serde_json::Value = resp.json().ok()?;
    if data.get("success")?.as_bool()? {
        let download_url = data.get("url")?.as_str()?;
        // 下载实际文件
        let file_resp = client.get(download_url).send().ok()?;
        if file_resp.status().is_success() {
            return file_resp.bytes().ok().map(|b| b.to_vec());
        }
    }
    None
}

/// 尝试 SubHD 预览接口 /ajax/file_ajax
fn subhd_try_preview(
    client: &reqwest::blocking::Client,
    document: &Html,
    detail_url: &str,
) -> Option<Vec<u8>> {
    let a_sel = Selector::parse(r##"a[data-target="#fileModal"][data-sid]"##).ok()?;
    let a = document.select(&a_sel).next()?;
    let sid = a.value().attr("data-sid")?;
    let fname = a.value().attr("data-fname")?;

    let api_url = format!("{}/ajax/file_ajax", SUBHD_BASE);
    let resp = client
        .post(&api_url)
        .form(&serde_json::json!({
            "dasid": sid,
            "dafname": fname,
        }))
        .header("Referer", detail_url)
        .send()
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: serde_json::Value = resp.json().ok()?;
    if data.get("success")?.as_bool()? {
        let filedata = data.get("filedata")?.as_str()?;
        return Some(filedata.as_bytes().to_vec());
    }
    None
}

/// 写入字幕内容到文件
fn subhd_write_content(content: &[u8], output_path: &std::path::Path) -> Result<(), AppError> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, content)?;
    Ok(())
}

// === SECTION 5 END ===

// === zimuku 搜索 ===

const ZIMUKU_BASE: &str = "https://zimuku.org";

/// 搜索 zimuku 字幕（首次请求，可能触发验证码）
fn zimuku_search(query: &str, proxy: &crate::translate::ProxyConfig) -> Result<Vec<SubtitleSearchResult>, AppError> {
    zimuku_search_inner(query, proxy, None, None)
}

/// SubHD 搜索内部实现
fn subhd_search_inner(query: &str, proxy: &crate::translate::ProxyConfig) -> Result<Vec<SubtitleSearchResult>, AppError> {
    let proxy_info = format!("mode={}, host={}, port={}", proxy.mode, proxy.host, proxy.port);
    tracing::info!("SubHD 搜索，代理配置: {}", proxy_info);
    let client = proxy.build_blocking_client();

    let search_url = format!("{}/search/{}", SUBHD_BASE, url_encode(query));
    tracing::info!("SubHD 搜索: {}", search_url);
    let resp = client
        .get(&search_url)
        .header("Referer", SUBHD_BASE)
        .send()
        .map_err(|e| {
            let detail = format!("{} [代理: {}]", format_error_chain(&e), proxy_info);
            tracing::warn!("SubHD 请求失败: {}", detail);
            AppError::SearchNetworkError { provider: "subhd".to_string(), detail }
        })?;

    let status = resp.status();
    tracing::info!("SubHD 响应状态: {}", status);

    let html_text = resp
        .text()
        .map_err(|e| {
            let detail = format!("读取响应失败: {}", e);
            tracing::warn!("SubHD {}", detail);
            AppError::SearchNetworkError { provider: "subhd".to_string(), detail }
        })?;

    // 检测验证码页面
    if html_text.contains("verifyimg") || html_text.contains("security_verify_img") {
        tracing::info!("SubHD 遇到验证码");
        // SubHD 验证码处理逻辑和 zimuku 类似
        let captcha_image = html_text
            .find("verifyimg")
            .and_then(|start| {
                let rest = &html_text[start..];
                rest.find("src=\"").map(|s| s + 5)
            })
            .and_then(|src_start| {
                let abs_start = html_text.find("verifyimg")? + src_start;
                html_text[abs_start..].find('"').map(|end| html_text[abs_start..abs_start + end].to_string())
            });

        if let Some(img) = captcha_image {
            return Err(AppError::SearchCaptchaRequired {
                provider: "subhd".to_string(),
                captcha_image: img,
                session_cookie: String::new(),
                original_url: search_url,
            });
        }
    }

    let entries = subhd_parse_search_results(&html_text);
    tracing::info!("SubHD 解析到 {} 条结果", entries.len());

    // 调试：如果没解析到结果，记录 HTML 片段帮助排查
    if entries.is_empty() {
        tracing::debug!("SubHD HTML 片段（前2000字符）: {}", &html_text[..html_text.len().min(2000)]);
    }

    let results = entries
        .into_iter()
        .map(|e| SubtitleSearchResult {
            file_name: e.title,
            language: e.language,
            download_count: e.download_count,
            rating: 0.0,
            release_info: e.ext,
            subtitle_id: format!("subhd:{}", e.detail_url),
        })
        .collect();

    Ok(results)
}

/// zimuku 搜索内部实现
/// captcha: 用户输入的验证码（第二次请求时传）
/// session_cookie: 第一次请求获得的 cookie（第二次请求时传）
fn zimuku_search_inner(
    query: &str,
    proxy: &crate::translate::ProxyConfig,
    captcha: Option<&str>,
    session_cookie: Option<&str>,
) -> Result<Vec<SubtitleSearchResult>, AppError> {
    let proxy_info = format!("mode={}, host={}, port={}", proxy.mode, proxy.host, proxy.port);
    tracing::info!("zimuku 搜索，代理配置: {}", proxy_info);
    let client = proxy.build_blocking_client();

    // 1. 搜索影片
    let search_url = format!("{}/search?q={}", ZIMUKU_BASE, url_encode(query));

    // 如果有验证码，构建带验证码的 URL
    let request_url = if let Some(captcha) = captcha {
        // stringToHex: 把验证码字符串转成 hex
        let hex_captcha: String = captcha.chars().map(|c| format!("{:02x}", c as u32)).collect();
        // 设置 srcurl cookie + security_verify_img 参数
        let srcurl_hex: String = search_url.chars().map(|c| format!("{:02x}", c as u32)).collect();
        let mut req = client
            .get(&format!("{}&security_verify_img={}", search_url, hex_captcha))
            .header("Referer", ZIMUKU_BASE);
        // 带 session cookie
        if let Some(cookie) = session_cookie {
            req = req.header("Cookie", format!("{}; srcurl={}", cookie, srcurl_hex));
        }
        req
    } else {
        client
            .get(&search_url)
            .header("Referer", ZIMUKU_BASE)
    };

    tracing::info!("zimuku 搜索: {}", search_url);
    let resp = request_url
        .send()
        .map_err(|e| {
            let detail = format!("{} [代理: {}]", format_error_chain(&e), proxy_info);
            tracing::warn!("zimuku 请求失败: {}", detail);
            AppError::SearchNetworkError { provider: "zimuku".to_string(), detail }
        })?;

    tracing::info!("zimuku 响应状态: {}", resp.status());

    // 在 text() 之前获取 cookie 和 url
    let session_cookie_from_resp = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|s| {
            if s.contains("security_session_verify") {
                let start = s.find("security_session_verify=")?;
                let rest = &s[start + "security_session_verify=".len()..];
                let end = rest.find(';').unwrap_or(rest.len());
                Some(format!("security_session_verify={}", &rest[..end]))
            } else {
                None
            }
        });
    let original_url = resp.url().to_string();

    let html_text = resp
        .text()
        .map_err(|e| {
            let detail = format!("读取响应失败: {}", e);
            tracing::warn!("zimuku {}", detail);
            AppError::SearchNetworkError { provider: "zimuku".to_string(), detail }
        })?;

    // 检测验证码页面
    if html_text.contains("security_verify_img") || html_text.contains("verifyimg") {
        // 提取验证码图片
        let captcha_image = html_text
            .find("verifyimg")
            .and_then(|start| {
                let rest = &html_text[start..];
                rest.find("src=\"").map(|s| s + 5)
            })
            .and_then(|src_start| {
                // src_start 是相对于 verifyimg 位置的偏移
                let abs_start = html_text.find("verifyimg")? + src_start;
                html_text[abs_start..].find('"').map(|end| html_text[abs_start..abs_start + end].to_string())
            });

        if let Some(img) = captcha_image {
            return Err(AppError::SearchCaptchaRequired {
                provider: "zimuku".to_string(),
                captcha_image: img,
                session_cookie: session_cookie_from_resp.unwrap_or_default(),
                original_url,
            });
        }
    }

    // 2. 解析搜索结果，获取第一个影片的字幕列表页链接
    let sublist_url = match zimuku_parse_search_results(&html_text) {
        Some(url) => {
            tracing::info!("zimuku 字幕列表页: {}", url);
            url
        }
        None => {
            tracing::debug!("zimuku 未找到影片，HTML 片段（前2000字符）: {}", &html_text[..html_text.len().min(2000)]);
            return Ok(Vec::new());
        }
    };

    // 3. 访问字幕列表页
    let resp = client
        .get(&sublist_url)
        .header("Referer", &search_url)
        .send()
        .map_err(|e| {
            let detail = format!("字幕列表页请求失败: {}", e);
            tracing::warn!("zimuku {}", detail);
            AppError::SearchNetworkError { provider: "zimuku".to_string(), detail }
        })?;

    tracing::info!("zimuku 字幕列表页响应状态: {}", resp.status());

    let html_text = resp
        .text()
        .map_err(|e| {
            let detail = format!("读取字幕列表页失败: {}", e);
            tracing::warn!("zimuku {}", detail);
            AppError::SearchNetworkError { provider: "zimuku".to_string(), detail }
        })?;

    // 字幕列表页也可能有验证码
    if html_text.contains("security_verify_img") || html_text.contains("verifyimg") {
        let captcha_image = html_text
            .find("verifyimg")
            .and_then(|start| {
                let rest = &html_text[start..];
                rest.find("src=\"").map(|s| s + 5)
            })
            .and_then(|src_start| {
                let abs_start = html_text.find("verifyimg")? + src_start;
                html_text[abs_start..].find('"').map(|end| html_text[abs_start..abs_start + end].to_string())
            });

        if let Some(img) = captcha_image {
            return Err(AppError::SearchCaptchaRequired {
                provider: "zimuku".to_string(),
                captcha_image: img,
                session_cookie: String::new(),
                original_url: sublist_url,
            });
        }
    }

    // 4. 解析字幕列表
    let entries = zimuku_parse_sublist(&html_text);
    let results = entries
        .into_iter()
        .map(|e| SubtitleSearchResult {
            file_name: e.title,
            language: e.language,
            download_count: e.download_count,
            rating: e.rating,
            release_info: e.ext,
            subtitle_id: format!("zimuku:{}", e.detail_url),
        })
        .collect();

    Ok(results)
}

/// zimuku 字幕条目
#[derive(Debug, Clone)]
struct ZimukuEntry {
    title: String,
    detail_url: String,
    language: String,
    ext: String,
    download_count: u64,
    rating: f64,
}

/// 解析 zimuku 搜索结果页，返回第一个影片的字幕列表页 URL
fn zimuku_parse_search_results(html: &str) -> Option<String> {
    let document = Html::parse_document(html);

    // 搜索结果在 div.item.prel 中
    let item_sel = Selector::parse("div.item.prel").ok()?;
    let item = document.select(&item_sel).next()?;

    // 影片链接在 p.tt > a
    let a_sel = Selector::parse("p.tt a").ok()?;
    let a = item.select(&a_sel).next()?;
    let href = a.value().attr("href")?;

    let url = if href.starts_with("http") {
        href.to_string()
    } else {
        format!("{}{}", ZIMUKU_BASE, href)
    };

    Some(url)
}

/// 解析 zimuku 字幕列表页
fn zimuku_parse_sublist(html: &str) -> Vec<ZimukuEntry> {
    let document = Html::parse_document(html);
    let mut entries = Vec::new();

    // 字幕在 div.subs > table tr
    let tr_sel = match Selector::parse("div.subs table tr") {
        Ok(s) => s,
        Err(_) => return entries,
    };

    for tr in document.select(&tr_sel) {
        // 第一列 td.first > a
        let td_sel = match Selector::parse("td.first a") {
            Ok(s) => s,
            Err(_) => continue,
        };
        let a = match tr.select(&td_sel).next() {
            Some(a) => a,
            None => continue,
        };
        let title = a.value().attr("title").unwrap_or_default().trim().to_string();
        let href = a.value().attr("href").unwrap_or_default().trim().to_string();
        if title.is_empty() || href.is_empty() {
            continue;
        }
        let detail_url = if href.starts_with("http") {
            href
        } else {
            format!("{}{}", ZIMUKU_BASE, href)
        };

        // 格式标签 span.label.label-info
        let ext_sel = match Selector::parse("span.label.label-info") {
            Ok(s) => s,
            Err(_) => continue,
        };
        let ext: String = tr
            .select(&ext_sel)
            .map(|s| s.text().collect::<String>().trim().to_lowercase())
            .collect::<Vec<_>>()
            .join("/");

        // 语言 td.tac.lang > img[title]
        let lang_sel = match Selector::parse("td.tac.lang img") {
            Ok(s) => s,
            Err(_) => continue,
        };
        let language = tr
            .select(&lang_sel)
            .next()
            .and_then(|img| {
                img.value()
                    .attr("title")
                    .or_else(|| img.value().attr("alt"))
            })
            .map(|l| zimuku_detect_language(l))
            .unwrap_or_else(|| "unknown".to_string());

        // 评分 i.rating-star
        let rating_sel = match Selector::parse("td.tac i.rating-star") {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rating = tr
            .select(&rating_sel)
            .next()
            .and_then(|i| {
                i.value()
                    .attr("title")
                    .and_then(|t| t.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|s| s.parse::<f64>().ok())
            })
            .map(|r| r / 10.0)
            .unwrap_or(0.0);

        // 下载次数（最后一个 td.tac）
        let tac_sel = match Selector::parse("td.tac") {
            Ok(s) => s,
            Err(_) => continue,
        };
        let download_count = tr
            .select(&tac_sel)
            .last()
            .map(|td| {
                let text = td.text().collect::<String>();
                zimuku_parse_download_count(&text)
            })
            .unwrap_or(0);

        entries.push(ZimukuEntry {
            title,
            detail_url,
            language,
            ext,
            download_count,
            rating,
        });
    }

    entries
}

/// zimuku 语言检测
fn zimuku_detect_language(text: &str) -> String {
    if text.contains("双语") || text.contains("中英") {
        "zh-en".to_string()
    } else if text.contains("简体") {
        "zh-CN".to_string()
    } else if text.contains("繁體") || text.contains("繁体") {
        "zh-TW".to_string()
    } else if text.contains("English") || text.contains("英文") {
        "en".to_string()
    } else {
        "unknown".to_string()
    }
}

/// 解析下载次数（支持 "1万" "1.5万" "1234" 等格式）
fn zimuku_parse_download_count(text: &str) -> u64 {
    let text = text.trim();
    if text.contains("万") {
        let num: String = text
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(n) = num.parse::<f64>() {
            return (n * 10000.0) as u64;
        }
    }
    if text.contains("千") {
        let num: String = text
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(n) = num.parse::<f64>() {
            return (n * 1000.0) as u64;
        }
    }
    let num: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    num.parse::<u64>().unwrap_or(0)
}

/// 下载 zimuku 字幕
fn zimuku_download(subtitle_id: &str, output_path: &std::path::Path) -> Result<(), AppError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|_| AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() })?;

    let detail_url = subtitle_id
        .strip_prefix("zimuku:")
        .ok_or_else(|| AppError::SearchDownloadFailed { provider: "zimuku".to_string() })?;

    // 1. 访问字幕详情页，找到下载链接 a#down1
    let resp = client
        .get(detail_url)
        .header("Referer", ZIMUKU_BASE)
        .send()
        .map_err(|_| AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() })?;

    if !resp.status().is_success() {
        return Err(AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() });
    }

    let html_text = resp
        .text()
        .map_err(|_| AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() })?;
    let document = Html::parse_document(&html_text);

    let down_sel = Selector::parse("a#down1")
        .map_err(|_| AppError::SearchDownloadFailed { provider: "zimuku".to_string() })?;
    let down_a = document
        .select(&down_sel)
        .next()
        .ok_or_else(|| AppError::SearchDownloadFailed { provider: "zimuku".to_string() })?;
    let download_page_url = down_a
        .value()
        .attr("href")
        .ok_or_else(|| AppError::SearchDownloadFailed { provider: "zimuku".to_string() })?;
    let download_page_url = if download_page_url.starts_with("http") {
        download_page_url.to_string()
    } else {
        format!("{}{}", ZIMUKU_BASE, download_page_url)
    };

    // 2. 访问下载页，找到真实下载链接
    let resp = client
        .get(&download_page_url)
        .header("Referer", detail_url)
        .send()
        .map_err(|_| AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() })?;

    if !resp.status().is_success() {
        return Err(AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() });
    }

    let html_text = resp
        .text()
        .map_err(|_| AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() })?;
    let document = Html::parse_document(&html_text);

    let btn_sel = Selector::parse("a.btn.btn-sm")
        .map_err(|_| AppError::SearchDownloadFailed { provider: "zimuku".to_string() })?;
    let btns: Vec<_> = document.select(&btn_sel).collect();
    // 第二个按钮是真实下载链接
    let download_btn = btns
        .get(1)
        .or_else(|| btns.first())
        .ok_or_else(|| AppError::SearchDownloadFailed { provider: "zimuku".to_string() })?;
    let download_link = download_btn
        .value()
        .attr("href")
        .ok_or_else(|| AppError::SearchDownloadFailed { provider: "zimuku".to_string() })?;
    let download_link = if download_link.starts_with("http") {
        download_link.to_string()
    } else {
        format!("{}{}", ZIMUKU_BASE, download_link)
    };

    // 3. 下载实际文件
    let file_resp = client
        .get(&download_link)
        .header("Referer", &download_page_url)
        .send()
        .map_err(|_| AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() })?;

    if !file_resp.status().is_success() {
        return Err(AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() });
    }

    let bytes = file_resp
        .bytes()
        .map_err(|_| AppError::SearchNetworkError { provider: "zimuku".to_string(), detail: String::new() })?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // zimuku 下载的可能是压缩包，直接写入，后续由前端/用户处理
    std::fs::write(output_path, &bytes)?;
    Ok(())
}

// === SECTION 6 END ===

// === 统一搜索入口 ===

/// 搜索来源类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchSource {
    /// OpenSubtitles（需要 API Key）
    OpenSubtitles,
    /// SubHD（无需 Key，爬虫）
    Subhd,
    /// zimuku（无需 Key，爬虫）
    Zimuku,
}

impl std::fmt::Display for SearchSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchSource::OpenSubtitles => write!(f, "opensubtitles"),
            SearchSource::Subhd => write!(f, "subhd"),
            SearchSource::Zimuku => write!(f, "zimuku"),
        }
    }
}

impl std::str::FromStr for SearchSource {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "opensubtitles" => Ok(SearchSource::OpenSubtitles),
            "subhd" => Ok(SearchSource::Subhd),
            "zimuku" => Ok(SearchSource::Zimuku),
            _ => Err(AppError::SearchNotConfigured),
        }
    }
}

/// 统一搜索入口。根据 source 分发到不同 provider。
/// - OpenSubtitles 需要 api_key
/// - SubHD / zimuku 不需要 api_key，传空即可
/// - proxy: 代理配置（从数据库读取），SubHD/zimuku 会使用代理访问
pub fn search_subtitles_multi(
    query: &str,
    language: &str,
    api_key: &str,
    source: &str,
    proxy: &crate::translate::ProxyConfig,
) -> Result<Vec<SubtitleSearchResult>, AppError> {
    let provider = SearchSource::from_str(source)?;
    match provider {
        SearchSource::OpenSubtitles => search_subtitles(query, language, api_key),
        SearchSource::Subhd => subhd_search_inner(query, proxy),
        SearchSource::Zimuku => zimuku_search(query, proxy),
    }
}

/// 带验证码的搜索入口（zimuku 云锁验证码通过后继续搜索）
/// captcha: 用户输入的验证码
/// session_cookie: 第一次请求返回的 security_session_verify cookie
pub fn search_subtitles_with_captcha(
    query: &str,
    source: &str,
    captcha: &str,
    session_cookie: &str,
    proxy: &crate::translate::ProxyConfig,
) -> Result<Vec<SubtitleSearchResult>, AppError> {
    let provider = SearchSource::from_str(source)?;
    match provider {
        SearchSource::Zimuku => zimuku_search_inner(query, proxy, Some(captcha), Some(session_cookie)),
        // SubHD 验证码机制不同，暂不支持自动重试，直接重新搜索
        SearchSource::Subhd => subhd_search_inner(query, proxy),
        _ => Err(AppError::SearchNotConfigured),
    }
}

/// 统一下载入口。subtitle_id 中带 provider 前缀（如 "subhd:..." / "zimuku:..."）。
/// OpenSubtitles 的 subtitle_id 为纯数字，无前缀。
pub fn download_subtitle_multi(
    subtitle_id: &str,
    api_key: &str,
    output_path: &std::path::Path,
) -> Result<(), AppError> {
    if let Some(_) = subtitle_id.strip_prefix("subhd:") {
        return subhd_download(subtitle_id, output_path);
    }
    if let Some(_) = subtitle_id.strip_prefix("zimuku:") {
        return zimuku_download(subtitle_id, output_path);
    }
    // 无前缀 = OpenSubtitles
    download_subtitle(subtitle_id, api_key, output_path)
}

// === SECTION 7 END ===

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

    #[test]
    fn test_simplify_rick_and_morty() {
        let input = "Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.mkv";
        let result = simplify_keyword(input);
        println!("simplify_keyword('{}') = '{}'", input, result);
        assert!(result.contains("Rick"), "结果应包含 Rick: {}", result);
        assert!(result.contains("Morty"), "结果应包含 Morty: {}", result);
    }
}

// === SECTION FINAL END ===
