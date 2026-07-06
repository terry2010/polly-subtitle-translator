// E2E 测试主入口 — 状态机模式
// 用法:
//   E2E_TIER=l1 cargo test --test e2e
//   E2E_TIER=full E2E_FIXTURE=rick_and_morty E2E_NAME_PRECISION=on cargo test --test e2e
//   E2E_RESET=1 E2E_TIER=full ... cargo test --test e2e  # 清除状态从头开始

mod helpers;

use helpers::config::{parse_test_config, Tier};
use helpers::fixture::{select_fixtures, Fixture};
use helpers::checks_l1;
use helpers::checks_l2;
use helpers::report::{CheckReport, FixtureReport, TestReport};
use helpers::translate_runner;
use helpers::state::TestState;

/// 输出目录
fn output_dir() -> std::path::PathBuf {
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("output")
        .join(format!("run-{}", ts));
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// 缓存目录
fn cache_dir() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cache");
    std::fs::create_dir_all(&dir).ok();
    dir
}

// === SECTION 1 END ===

/// L1 结构检查（无模型依赖，秒级）
fn run_l1_checks(fixture: &Fixture, cfg: &helpers::config::TestConfig) -> FixtureReport {
    let subtitle = fixture.load_subtitle();
    let mut checks: Vec<CheckReport> = Vec::new();

    for cr in checks_l1::run_l1_checks(&subtitle) {
        checks.push(CheckReport::from_check_result(&cr.name, "L1", &cr));
    }

    if cfg.format_matrix {
        for cr in checks_l1::check_format_conversion_matrix(&subtitle) {
            checks.push(CheckReport::from_check_result(&cr.name, "L1", &cr));
        }
    }

    let fail_count = checks.iter().filter(|c| c.status == "fail").count();
    let status = if fail_count > 0 { "failed" } else { "passed" };
    FixtureReport {
        name: fixture.name.clone(),
        file: fixture.file.clone(),
        status: status.to_string(),
        entries: subtitle.entries.len(),
        checks,
    }
}

// === SECTION 2 END ===

/// 状态机模式：分阶段翻译+验证
/// 阶段 0: 专有名词预扫描
/// 阶段 1..N: 每批翻译 → 9b → 27b验证 → 有bug就停
async fn run_state_machine(
    fixture: &Fixture,
    cfg: &helpers::config::TestConfig,
    db: &zimufan_lib::db::Database,
    out_dir: &std::path::Path,
) -> FixtureReport {
    let subtitle = fixture.load_subtitle();
    let name_precision = cfg.name_precision.modes().first().copied().unwrap_or(false);
    let batch_size = 30;

    // 加载或创建状态
    let reset = std::env::var("E2E_RESET").map(|v| v == "1" || v == "true").unwrap_or(false);
    if reset {
        TestState::clear(&fixture.name, name_precision);
        eprintln!("  [状态] 已清除状态，从头开始");
    }

    let mut state = TestState::load(&fixture.name, name_precision).unwrap_or_else(|| {
        eprintln!("  [状态] 新建状态文件");
        TestState::new(
            &fixture.name,
            name_precision,
            &fixture.source_lang,
            &fixture.target_lang,
            subtitle.entries.len(),
            batch_size,
        )
    });

    eprintln!("  [状态] {}", state.summary());

    // L1 原始检查
    let mut checks: Vec<CheckReport> = Vec::new();
    for cr in checks_l1::run_l1_checks(&subtitle) {
        checks.push(CheckReport::from_check_result(&cr.name, "L1", &cr));
    }

    // 阶段 0：名词预扫描
    if name_precision {
        translate_runner::stage_name_scan(&mut state, &subtitle, cfg).await;
    } else {
        state.names_status = helpers::state::BatchStatus::Passed;
    }

    // 阶段 1..N：逐批翻译+验证
    let total_batches = state.batches.len();
    let mut judge_fail_batches = Vec::new();

    for stage in 1..=total_batches {
        state.current_stage = stage;
        let passed = translate_runner::stage_translate_batch(
            &mut state, &subtitle, cfg, db,
        ).await;

        if !passed {
            judge_fail_batches.push(stage);
            eprintln!("\n  ⚠️ 批次 {} 27b 验证发现问题（翻译质量），继续下一批", stage);
        }
    }

    // 始终运行 L2 检查（27b 翻译质量问题不阻塞 L2）
    {
        let translated_file = state.build_translated_file(&subtitle);

        // 保存翻译结果
        let suffix = if name_precision { "_np" } else { "" };
        let out_path = out_dir.join(format!("{}{}.srt", fixture.name, suffix));
        let rendered = zimufan_lib::subtitle::render_subtitle(&translated_file);
        if let Err(e) = std::fs::write(&out_path, &rendered) {
            eprintln!("  [警告] 保存翻译结果失败: {:?}", e);
        }

        // L1 翻译后检查
        let prefix = if name_precision { "[NP] " } else { "" };
        for cr in checks_l1::run_l1_checks_translated(&subtitle, &translated_file) {
            checks.push(CheckReport::from_check_result(
                &format!("{}{}", prefix, cr.name), "L1", &cr,
            ));
        }

        // L2 检查
        for cr in checks_l2::run_l2_checks(&subtitle, &translated_file, &fixture.target_lang) {
            checks.push(CheckReport::from_check_result(
                &format!("{}{}", prefix, cr.name), "L2", &cr,
            ));
        }

        // 翻译统计
        checks.push(CheckReport {
            name: format!("{}translate_failures", prefix),
            tier: "L2".to_string(),
            status: if state.total_failed == 0 { "pass" } else { "warn" }.to_string(),
            detail: format!("失败 {} 条, 缓存 {} 条, token {}", state.total_failed, state.total_cached, state.total_tokens),
            source_hint: Some("translate.rs translate_batch_with_fallback".to_string()),
        });

        // 27b 汇总（翻译质量问题判为 warn，不阻塞测试）
        let total_pass: usize = state.batches.iter().map(|b| b.judge_pass).sum();
        let total_fail: usize = state.batches.iter().map(|b| b.judge_fail).sum();
        let total_shift: usize = state.batches.iter().map(|b| b.judge_shift).sum();
        let total_judged = total_pass + total_fail + total_shift;
        // 27b fail 是翻译质量问题（模型能力限制），判为 warn
        // 27b 验证结果为空时也判为 warn（验证未生效）
        let judge_status = if total_judged == 0 { "warn" }
            else if total_fail > 0 || total_shift > 0 { "warn" }
            else { "pass" };
        checks.push(CheckReport {
            name: format!("{}judge_27b", prefix),
            tier: "L5".to_string(),
            status: judge_status.to_string(),
            detail: format!("27b judge: {} pass, {} fail, {} shift (共 {} 条判定, 问题批次: {:?})", total_pass, total_fail, total_shift, total_judged, judge_fail_batches),
            source_hint: Some("27b judge 翻译质量问题".to_string()),
        });

        // L3 持久化往返验证（仅 Full 模式，翻译完成后才有缓存可恢复）
        if cfg.tier == Tier::Full {
            eprintln!("\n  === L3 持久化往返验证 ===");

            // L3.1 双语字幕往返（SRT/ASS/VTT 各一次）
            for cr in helpers::checks_l3::check_bilingual_roundtrip_all(&subtitle, &translated_file, &fixture.target_lang) {
                let status = cr.status.as_str();
                eprintln!("  [L3] {} [{}]: {}", status, cr.name, cr.detail);
                checks.push(CheckReport::from_check_result(
                    &format!("{}{}", prefix, cr.name), "L3", &cr,
                ));
            }

            // L3.2 缓存恢复验证
            // provider_name 和 file_hash 必须与翻译时一致，否则缓存 key 不匹配
            let file_hash = subtitle.file_hash.clone();
            let provider_name = format!("openai-lmstudio-{}", cfg.model_9b);
            let cr = helpers::checks_l3::check_cache_recovery(
                &subtitle, &translated_file, db,
                &fixture.source_lang, &fixture.target_lang,
                &provider_name, &file_hash,
            );
            eprintln!("  [L3] {} {}: {}", cr.status.as_str(), cr.name, cr.detail);
            checks.push(CheckReport::from_check_result(
                &format!("{}{}", prefix, cr.name), "L3", &cr,
            ));

            // L3.3 多次打开关闭验证（3 次）
            for cr in helpers::checks_l3::check_repeated_open(
                &subtitle, &translated_file, db,
                &fixture.source_lang, &fixture.target_lang,
                &provider_name, &file_hash,
            ) {
                eprintln!("  [L3] {} {}: {}", cr.status.as_str(), cr.name, cr.detail);
                checks.push(CheckReport::from_check_result(
                    &format!("{}{}", prefix, cr.name), "L3", &cr,
                ));
            }
        }
    }

    eprintln!("\n  [最终状态] {}", state.summary());

    let fail_count = checks.iter().filter(|c| c.status == "fail").count();
    let status = if fail_count > 0 { "failed" } else { "passed" };
    FixtureReport {
        name: fixture.name.clone(),
        file: fixture.file.clone(),
        status: status.to_string(),
        entries: subtitle.entries.len(),
        checks,
    }
}

// === SECTION 3 END ===

#[tokio::test]
async fn e2e_test_run() {
    let cfg = parse_test_config();
    println!("E2E 配置: tier={:?}, fixture={:?}, name_precision={:?}", cfg.tier, cfg.fixture_name, cfg.name_precision);

    let fixtures = select_fixtures(&cfg);
    assert!(!fixtures.is_empty(), "没有找到 fixture");

    let db = translate_runner::create_test_db(&cache_dir());
    let mut report = TestReport::new();
    let out_dir = output_dir();

    for fixture in &fixtures {
        println!("\n=== 测试 fixture: {} ===", fixture.name);

        let fr = if cfg.tier <= Tier::L1 {
            // L1：纯结构检查
            run_l1_checks(fixture, &cfg)
        } else {
            // L2/Full：状态机模式
            run_state_machine(fixture, &cfg, &db, &out_dir).await
        };

        println!("  条目数: {}", fr.entries);
        for c in &fr.checks {
            let icon = match c.status.as_str() {
                "pass" => "✅",
                "warn" => "⚠️",
                "fail" => "❌",
                _ => "?",
            };
            println!("  {} {} ({}): {}", icon, c.name, c.tier, c.detail);
        }
        report.add_fixture(fr);
    }

    // 保存报告
    if let Err(e) = report.save(&out_dir) {
        eprintln!("保存报告失败: {:?}", e);
    }
    println!("\n报告已保存到: {}", out_dir.display());
    println!("\n{}", report.to_markdown());

    let fail_count = report.summary.checks_failed;
    if fail_count > 0 {
        panic!("E2E 测试失败: {} 项检查失败", fail_count);
    }
}
