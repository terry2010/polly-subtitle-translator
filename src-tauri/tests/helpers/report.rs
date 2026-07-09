// 报告生成：JSON + Markdown
use super::checks_l1::CheckResult;
use serde::{Deserialize, Serialize};

/// 单个 fixture 的测试结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureReport {
    pub name: String,
    pub file: String,
    pub status: String,
    pub entries: usize,
    pub checks: Vec<CheckReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckReport {
    pub name: String,
    pub tier: String,
    pub status: String,
    pub detail: String,
    pub source_hint: Option<String>,
}

impl CheckReport {
    pub fn from_check_result(name: &str, tier: &str, cr: &CheckResult) -> Self {
        Self {
            name: name.to_string(),
            tier: tier.to_string(),
            status: cr.status.as_str().to_string(),
            detail: cr.detail.clone(),
            source_hint: cr.source_hint.clone(),
        }
    }
}

/// 完整测试报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    pub timestamp: String,
    pub summary: Summary,
    pub fixtures: Vec<FixtureReport>,
    /// 总用时（人类可读格式，如 "12分34秒"）
    pub elapsed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub total_fixtures: usize,
    pub passed: usize,
    pub warned: usize,
    pub failed: usize,
    pub total_checks: usize,
    pub checks_passed: usize,
    pub checks_warned: usize,
    pub checks_failed: usize,
}

// === SECTION 1 END ===

impl TestReport {
    pub fn new() -> Self {
        Self {
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            summary: Summary {
                total_fixtures: 0,
                passed: 0,
                warned: 0,
                failed: 0,
                total_checks: 0,
                checks_passed: 0,
                checks_warned: 0,
                checks_failed: 0,
            },
            fixtures: Vec::new(),
            elapsed: String::new(),
        }
    }

    pub fn add_fixture(&mut self, fr: FixtureReport) {
        let total = fr.checks.len();
        let passed = fr.checks.iter().filter(|c| c.status == "pass").count();
        let warned = fr.checks.iter().filter(|c| c.status == "warn").count();
        let failed = fr.checks.iter().filter(|c| c.status == "fail").count();

        self.summary.total_fixtures += 1;
        self.summary.total_checks += total;
        self.summary.checks_passed += passed;
        self.summary.checks_warned += warned;
        self.summary.checks_failed += failed;

        let status = if failed > 0 {
            "failed"
        } else if warned > 0 {
            "warned"
        } else {
            "passed"
        };

        match status {
            "failed" => self.summary.failed += 1,
            "warned" => self.summary.warned += 1,
            _ => self.summary.passed += 1,
        }

        self.fixtures.push(FixtureReport { status: status.to_string(), ..fr });
    }

    /// 生成 JSON 报告
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\": \"{:?}\"}}", e))
    }

    /// 生成 Markdown 报告
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!("# E2E 测试报告\n\n"));
        md.push_str(&format!("**运行时间**: {}\n\n", self.timestamp));
        if !self.elapsed.is_empty() {
            md.push_str(&format!("**总用时**: {}\n\n", self.elapsed));
        }
        md.push_str(&format!("**结果**: {} 通过 / {} 警告 / {} 失败\n\n", self.summary.passed, self.summary.warned, self.summary.failed));
        md.push_str("## 概览\n\n");
        md.push_str("| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |\n");
        md.push_str("|---------|--------|------|----------------|\n");
        for f in &self.fixtures {
            let p = f.checks.iter().filter(|c| c.status == "pass").count();
            let w = f.checks.iter().filter(|c| c.status == "warn").count();
            let fl = f.checks.iter().filter(|c| c.status == "fail").count();
            let icon = match f.status.as_str() {
                "passed" => "✅",
                "warned" => "⚠️",
                "failed" => "❌",
                _ => "?",
            };
            md.push_str(&format!("| {} | {} | {} {} | {}/{}/{} |\n", f.name, f.entries, icon, f.status, p, w, fl));
        }

        for f in &self.fixtures {
            md.push_str(&format!("\n## {} {}\n\n", f.name, match f.status.as_str() {
                "passed" => "✅",
                "warned" => "⚠️",
                "failed" => "❌",
                _ => "",
            }));
            for c in &f.checks {
                let icon = match c.status.as_str() {
                    "pass" => "✅",
                    "warn" => "⚠️",
                    "fail" => "❌",
                    _ => "?",
                };
                md.push_str(&format!("- {} **{}** ({}): {}\n", icon, c.name, c.tier, c.detail));
                if let Some(hint) = &c.source_hint {
                    md.push_str(&format!("  - 相关代码: {}\n", hint));
                }
            }
        }
        md
    }

    /// 保存报告到文件
    pub fn save(&self, output_dir: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(output_dir)?;
        std::fs::write(output_dir.join("report.json"), self.to_json())?;
        std::fs::write(output_dir.join("report.md"), self.to_markdown())?;
        std::fs::write(output_dir.join("report.html"), self.to_html())?;
        Ok(())
    }

    /// 生成 HTML 报告
    pub fn to_html(&self) -> String {
        let mut html = String::new();
        html.push_str("<!DOCTYPE html>\n<html lang=\"zh\">\n<head>\n");
        html.push_str("<meta charset=\"UTF-8\">\n");
        html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n");
        html.push_str("<title>E2E 测试报告</title>\n");
        html.push_str("<style>\n");
        html.push_str("body { font-family: -apple-system, sans-serif; margin: 20px; background: #f5f5f5; }\n");
        html.push_str(".header { background: #fff; padding: 20px; border-radius: 8px; margin-bottom: 20px; }\n");
        html.push_str(".summary { display: flex; gap: 20px; }\n");
        html.push_str(".stat { padding: 15px 25px; border-radius: 8px; color: #fff; font-size: 24px; font-weight: bold; }\n");
        html.push_str(".stat.pass { background: #22c55e; }\n");
        html.push_str(".stat.warn { background: #f59e0b; }\n");
        html.push_str(".stat.fail { background: #ef4444; }\n");
        html.push_str(".fixture { background: #fff; padding: 20px; border-radius: 8px; margin-bottom: 15px; }\n");
        html.push_str(".fixture-header { font-size: 18px; font-weight: bold; margin-bottom: 10px; }\n");
        html.push_str("table { width: 100%; border-collapse: collapse; }\n");
        html.push_str("th, td { padding: 8px 12px; text-align: left; border-bottom: 1px solid #e5e5e5; }\n");
        html.push_str("th { background: #f0f0f0; }\n");
        html.push_str(".pass { color: #22c55e; }\n");
        html.push_str(".warn { color: #f59e0b; }\n");
        html.push_str(".fail { color: #ef4444; }\n");
        html.push_str("</style>\n</head>\n<body>\n");

        html.push_str("<div class=\"header\">\n");
        html.push_str(&format!("<h1>E2E 测试报告</h1>\n"));
        html.push_str(&format!("<p>运行时间: {}</p>\n", self.timestamp));
        if !self.elapsed.is_empty() {
            html.push_str(&format!("<p>总用时: {}</p>\n", self.elapsed));
        }
        html.push_str("<div class=\"summary\">\n");
        html.push_str(&format!("<div class=\"stat pass\">{}<br><small>通过</small></div>\n", self.summary.passed));
        html.push_str(&format!("<div class=\"stat warn\">{}<br><small>警告</small></div>\n", self.summary.warned));
        html.push_str(&format!("<div class=\"stat fail\">{}<br><small>失败</small></div>\n", self.summary.failed));
        html.push_str("</div>\n</div>\n");

        for f in &self.fixtures {
            let p = f.checks.iter().filter(|c| c.status == "pass").count();
            let w = f.checks.iter().filter(|c| c.status == "warn").count();
            let fl = f.checks.iter().filter(|c| c.status == "fail").count();
            let icon = match f.status.as_str() {
                "passed" => "✅",
                "warned" => "⚠️",
                "failed" => "❌",
                _ => "?",
            };
            html.push_str("<div class=\"fixture\">\n");
            html.push_str(&format!("<div class=\"fixture-header\">{} {} ({} 条目, {}/{}/{})</div>\n",
                icon, f.name, f.entries, p, w, fl));
            html.push_str("<table>\n<thead><tr><th>检查项</th><th>层级</th><th>状态</th><th>详情</th><th>相关代码</th></tr></thead>\n<tbody>\n");
            for c in &f.checks {
                let cls = match c.status.as_str() {
                    "pass" => "pass",
                    "warn" => "warn",
                    "fail" => "fail",
                    _ => "",
                };
                let icon = match c.status.as_str() {
                    "pass" => "✅",
                    "warn" => "⚠️",
                    "fail" => "❌",
                    _ => "?",
                };
                html.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td class=\"{}\">{} {}</td><td>{}</td><td>{}</td></tr>\n",
                    c.name, c.tier, cls, icon, c.status,
                    html_escape(&c.detail),
                    c.source_hint.as_deref().map(html_escape).unwrap_or_default()
                ));
            }
            html.push_str("</tbody></table>\n</div>\n");
        }

        html.push_str("</body>\n</html>\n");
        html
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\n', "<br>")
}

// === SECTION 2 END ===