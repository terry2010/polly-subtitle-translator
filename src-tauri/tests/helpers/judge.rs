// 27b 裁判：用 27b 模型判断 9b 译文质量
use zimufan_lib::subtitle::SubtitleFile;
use serde::{Deserialize, Serialize};

/// 单条判断结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeResult {
    pub index: usize,
    pub verdict: String,      // "pass" / "fail" / "shift"
    pub reason: Option<String>,
    pub suggestion: Option<String>,
}

/// 批次判断结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJudgeResult {
    pub batch_start: usize,
    pub batch_end: usize,
    pub results: Vec<JudgeResult>,
}

/// 构建 judge system prompt
fn judge_system_prompt() -> String {
    "You are a translation quality judge. Given source subtitles and their translations, judge each translation.\n\n\
     Verdict:\n\
     - pass: correct translation\n\
     - fail: missing translation, semantic error, glossary term lost, empty translation, translation = source\n\
     - shift: translation content matches an adjacent entry, not the current entry\n\n\
     Edge cases:\n\
     - \"nice\" → \"好\" or \"不错\" → pass (synonyms)\n\
     - \"I love apples\" → \"苹果很好吃\" → pass (free translation, meaning correct)\n\
     - \"I love apples\" → \"我喜欢香蕉\" → fail (semantic error)\n\
     - Sound effect [clattering] → [碰撞声] → pass\n\
     - Sound effect lost or format changed → fail\n\
     - Translation clearly about another entry's content → shift\n\n\
     Output a JSON array: [{\"index\": 1, \"verdict\": \"pass\", \"reason\": null, \"suggestion\": null}]\n\
     For fail/shift, provide reason and suggestion (one of: prompt/batch/context/glossary/flow/alignment).\n\n\
     Output ONLY the JSON array. No other text.".to_string()
}

// === SECTION 1 END ===

/// 构建 judge user prompt（批次内多条对照）
fn judge_user_prompt(original: &SubtitleFile, translated: &SubtitleFile, start: usize, end: usize) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("Judge translations for entries {}-{}:\n\n", start + 1, end));
    for i in start..end {
        if i >= original.entries.len() || i >= translated.entries.len() {
            break;
        }
        let orig = &original.entries[i];
        let trans = &translated.entries[i];
        prompt.push_str(&format!("[{}] Source: {}\n", i + 1, orig.text.replace('\n', " ")));
        prompt.push_str(&format!("    Translation: {}\n", trans.translated.replace('\n', " ")));
    }
    prompt
}

/// 调用 27b 模型判断一个批次（直接发 HTTP 请求，不走 stop 序列）
pub async fn judge_batch(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    cfg: &super::config::TestConfig,
    start: usize,
    end: usize,
) -> BatchJudgeResult {
    let system = judge_system_prompt();
    let user = judge_user_prompt(original, translated, start, end);

    // 直接发 HTTP 请求（不带 stop 序列，避免截断 JSON 输出）
    let url = format!("{}/chat/completions", cfg.api_base.trim_end_matches('/'));
    let request_body = serde_json::json!({
        "model": cfg.model_27b,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user",   "content": user },
        ],
        "temperature": 0,
        "stream": false,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .timeout(std::time::Duration::from_secs(600))
        .json(&request_body)
        .send()
        .await;

    let mut results = Vec::new();
    match resp {
        Ok(r) => {
            match r.json::<serde_json::Value>().await {
                Ok(body) => {
                    let response = body
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("message"))
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    let json_content = strip_code_fence(response);
                    // 尝试解析为 JSON 数组 [{"index": 1, "verdict": "pass", ...}]
                    if let Ok(json) = serde_json::from_str::<Vec<serde_json::Value>>(&json_content) {
                        for item in json {
                            let index = item.get("index").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(0);
                            let verdict = item.get("verdict").and_then(|v| v.as_str()).unwrap_or("pass").to_string();
                            let reason = item.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                            let suggestion = item.get("suggestion").and_then(|v| v.as_str()).map(|s| s.to_string());
                            results.push(JudgeResult { index, verdict, reason, suggestion });
                        }
                    }
                    // 尝试解析为 JSON 对象 {"1": {"verdict": "pass", ...}, ...}
                    else if let Ok(obj) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json_content) {
                        for (key, item) in obj {
                            let index = key.parse::<usize>().unwrap_or(0);
                            let verdict = item.get("verdict").and_then(|v| v.as_str()).unwrap_or("pass").to_string();
                            let reason = item.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                            let suggestion = item.get("suggestion").and_then(|v| v.as_str()).map(|s| s.to_string());
                            results.push(JudgeResult { index, verdict, reason, suggestion });
                        }
                    }
                    // 尝试解析为 JSON Lines（多个对象用逗号分隔，无外层数组括号）
                    else {
                        let wrapped = format!("[{}]", json_content);
                        if let Ok(json) = serde_json::from_str::<Vec<serde_json::Value>>(&wrapped) {
                            eprintln!("  [27b judge] 批次 {}-{} 使用 JSON Lines 模式解析", start + 1, end);
                            for item in json {
                                let index = item.get("index").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(0);
                                let verdict = item.get("verdict").and_then(|v| v.as_str()).unwrap_or("pass").to_string();
                                let reason = item.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                                let suggestion = item.get("suggestion").and_then(|v| v.as_str()).map(|s| s.to_string());
                                results.push(JudgeResult { index, verdict, reason, suggestion });
                            }
                        } else {
                            // 最终兜底：用正则提取所有完整的判定对象
                            // 处理 27b 模型输出被截断导致 JSON 不完整的情况
                            // 支持两种格式：
                            //   1. 数组元素格式：{"index": N, "verdict": "pass", "reason": "...", ...}
                            //   2. 对象 key 格式："N": {"verdict": "pass", "reason": "...", ...}
                            let re_array = regex::Regex::new(
                                r#"\{\s*"index"\s*:\s*(\d+)\s*,\s*"verdict"\s*:\s*"(\w+)"\s*(?:,\s*"reason"\s*:\s*(?:"((?:[^"\\]|\\.)*)"|null))?\s*(?:,\s*"suggestion"\s*:\s*(?:"((?:[^"\\]|\\.)*)"|null))?\s*\}"#
                            ).unwrap();
                            let matches: Vec<_> = re_array.captures_iter(&json_content).collect();
                            if !matches.is_empty() {
                                eprintln!("  [27b judge] 批次 {}-{} 使用正则兜底提取 {} 条结果（数组格式）", start + 1, end, matches.len());
                                for cap in matches {
                                    let index = cap[1].parse::<usize>().unwrap_or(0);
                                    let verdict = cap[2].to_string();
                                    let reason = cap.get(3).map(|m| m.as_str().to_string());
                                    let suggestion = cap.get(4).map(|m| m.as_str().to_string());
                                    results.push(JudgeResult { index, verdict, reason, suggestion });
                                }
                            } else {
                                // 尝试对象 key 格式："N": {"verdict": "pass", ...}
                                let re_obj = regex::Regex::new(
                                    r#""(\d+)"\s*:\s*\{\s*"verdict"\s*:\s*"(\w+)"\s*(?:,\s*"reason"\s*:\s*(?:"((?:[^"\\]|\\.)*)"|null))?\s*(?:,\s*"suggestion"\s*:\s*(?:"((?:[^"\\]|\\.)*)"|null))?\s*\}"#
                                ).unwrap();
                                let obj_matches: Vec<_> = re_obj.captures_iter(&json_content).collect();
                                if !obj_matches.is_empty() {
                                    eprintln!("  [27b judge] 批次 {}-{} 使用正则兜底提取 {} 条结果（对象格式）", start + 1, end, obj_matches.len());
                                    for cap in obj_matches {
                                        let index = cap[1].parse::<usize>().unwrap_or(0);
                                        let verdict = cap[2].to_string();
                                        let reason = cap.get(3).map(|m| m.as_str().to_string());
                                        let suggestion = cap.get(4).map(|m| m.as_str().to_string());
                                        results.push(JudgeResult { index, verdict, reason, suggestion });
                                    }
                                } else {
                                    let preview = json_content.replace('\n', "\\n");
                                    eprintln!("  [27b judge] 批次 {}-{} JSON 解析失败: {:.500}", start + 1, end, preview);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  [27b judge] 批次 {}-{} 响应解析失败: {:?}", start + 1, end, e);
                }
            }
        }
        Err(e) => {
            eprintln!("  [27b judge] 批次 {}-{} 请求失败: {:?}", start + 1, end, e);
        }
    }

    BatchJudgeResult {
        batch_start: start,
        batch_end: end,
        results,
    }
}

/// 剥离 markdown 代码块
fn strip_code_fence(s: &str) -> String {
    let s = s.trim();
    if !s.starts_with("```") {
        return s.to_string();
    }
    let after = match s.find('\n') {
        Some(idx) => &s[idx + 1..],
        None => return s.to_string(),
    };
    let trimmed = after.trim_end();
    if trimmed.ends_with("```") {
        trimmed[..trimmed.len() - 3].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

// === SECTION 2 END ===

/// 对整个字幕运行 27b 判断
#[allow(dead_code)]
pub async fn judge_full(
    original: &SubtitleFile,
    translated: &SubtitleFile,
    cfg: &super::config::TestConfig,
) -> Vec<BatchJudgeResult> {
    const BATCH_SIZE: usize = 30;
    let total = original.entries.len();
    let mut all_results = Vec::new();

    eprintln!("  [27b judge] 开始判断 {} 条译文（每批 {} 条）...", total, BATCH_SIZE);

    for start in (0..total).step_by(BATCH_SIZE) {
        let end = (start + BATCH_SIZE).min(total);
        eprintln!("  [27b judge] 批次 {}-{}...", start + 1, end);
        let result = judge_batch(original, translated, cfg, start, end).await;
        let pass_count = result.results.iter().filter(|r| r.verdict == "pass").count();
        let fail_count = result.results.iter().filter(|r| r.verdict == "fail").count();
        let shift_count = result.results.iter().filter(|r| r.verdict == "shift").count();
        eprintln!("  [27b judge] 批次 {}-{}: {} pass, {} fail, {} shift", start + 1, end, pass_count, fail_count, shift_count);
        all_results.push(result);
    }

    all_results
}

/// 汇总判断结果
#[allow(dead_code)]
pub fn summarize_judge(results: &[BatchJudgeResult]) -> (usize, usize, usize) {
    let mut pass = 0;
    let mut fail = 0;
    let mut shift = 0;
    for batch in results {
        for r in &batch.results {
            match r.verdict.as_str() {
                "pass" => pass += 1,
                "fail" => fail += 1,
                "shift" => shift += 1,
                _ => {}
            }
        }
    }
    (pass, fail, shift)
}