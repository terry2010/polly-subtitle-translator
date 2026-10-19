// Qwen3 托管运行环境：按需懒装到 app_data_dir/asr/，不碰系统 Python。
//
// 安装步骤（每步 emit `asr_env_progress` 事件，可被 cancel 中断并清理半成品）：
//   1. download_python：python-build-standalone tarball（~45MB，gh-proxy 镜像兜底）
//   2. extract_python：解压 tar.gz → asr/venv/
//   3. pip_install：venv python -m pip install <pinned deps>（子进程，可 kill）
//   4. download_models：huggingface_hub.snapshot_download 到 asr/models/（子进程，可 kill）
//   5. 写 asr/env.json 完成标记
use super::qwen3::{asr_home, is_valid_model_dir, managed_python, QWEN3_ALIGNER_NAME, QWEN3_MODEL_NAMES};
use crate::error::AppError;
use crate::ffmpeg::no_window;
use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Emitter;

/// python-build-standalone 固定版本（astral-sh 官方发布，2025-07-12 批次）。
const PBS_TAG: &str = "20250712";
const PBS_VER: &str = "3.12.11";

/// 当前平台的 python-build-standalone tarball URL（gh-proxy 优先，直连兜底）。
fn python_dist_urls() -> Vec<String> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    const TRIPLE: &str = "aarch64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    const TRIPLE: &str = "x86_64-apple-darwin";
    #[cfg(target_os = "windows")]
    const TRIPLE: &str = "x86_64-pc-windows-msvc";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const TRIPLE: &str = "x86_64-unknown-linux-gnu";
    let file = format!("cpython-{PBS_VER}+{PBS_TAG}-{TRIPLE}-install_only.tar.gz");
    let direct = format!("https://github.com/astral-sh/python-build-standalone/releases/download/{PBS_TAG}/{file}");
    vec![
        format!("https://gh-proxy.com/{direct}"),
        direct,
    ]
}

/// pip 依赖（与 demo .venv-asr 已验证版本对齐）。可用 env `ASR_PIP_INDEX_URL` 换镜像源。
const PIP_DEPS: &[&str] = &[
    "qwen-asr==0.0.6",
    "qwen-omni-utils==0.0.9",
    "silero-vad==6.2.3",
    "soundfile==0.14.0",
    "transformers==4.57.6",
    "torch==2.14.1",
];

/// 可选 ASR 模型：(本地目录名, HF repo id)。用户在设置页自选下载哪个。
const ASR_MODEL_REPOS: &[(&str, &str)] = &[
    (QWEN3_MODEL_NAMES[0], "Qwen/Qwen3-ASR-0.6B"),
    (QWEN3_MODEL_NAMES[1], "Qwen/Qwen3-ASR-1.7B"),
];

/// 必装的 ForcedAligner 对齐器。
const ALIGNER_REPO: (&str, &str) = (QWEN3_ALIGNER_NAME, "Qwen/Qwen3-ForcedAligner-0.6B");

/// 本次安装要下载的模型列表：选定的 ASR 模型（默认 0.6B）+ 对齐器。
fn install_repos(model: Option<&str>) -> Vec<(&'static str, &'static str)> {
    let asr = ASR_MODEL_REPOS
        .iter()
        .find(|(dir, _)| Some(*dir) == model)
        .copied()
        .unwrap_or(ASR_MODEL_REPOS[0]);
    vec![asr, ALIGNER_REPO]
}

/// 安装完成的标记文件（记录版本信息，供 env_status 快速判定）。
const MARKER: &str = "env.json";

/// 环境状态（给 asr_env_status IPC 返回）。
#[derive(Debug, Clone, Serialize)]
pub struct AsrEnvStatus {
    /// 托管 Python 解释器就位。
    pub python_ready: bool,
    /// 依赖已装（标记文件存在）。
    pub deps_ready: bool,
    /// 各模型目录就绪状态：(目录名, 是否就绪)。
    pub models: Vec<(String, bool)>,
    /// 全部就绪（python + deps + 默认模型）。
    pub ready: bool,
}

/// 查询托管环境状态（廉价文件检查）。
pub fn env_status() -> AsrEnvStatus {
    let Some(home) = asr_home() else {
        return AsrEnvStatus { python_ready: false, deps_ready: false, models: vec![], ready: false };
    };
    let python_ready = managed_python().is_some();
    let deps_ready = home.join(MARKER).is_file();
    let models: Vec<(String, bool)> = ASR_MODEL_REPOS
        .iter()
        .chain(std::iter::once(&ALIGNER_REPO))
        .map(|(dir, _)| {
            let p = home.join("models").join(dir);
            (dir.to_string(), is_valid_model_dir(&p))
        })
        .collect();
    // 就绪 = python + deps + 至少一个 ASR 模型 + 对齐器（用户自选装哪个 ASR 模型）。
    let any_asr = ASR_MODEL_REPOS.iter().any(|(dir, _)| {
        models.iter().any(|(d, ok)| d == dir && *ok)
    });
    let aligner_ok = models.iter().any(|(d, ok)| d == ALIGNER_REPO.0 && *ok);
    let ready = python_ready && deps_ready && any_asr && aligner_ok;
    AsrEnvStatus { python_ready, deps_ready, models, ready }
}

fn fail(detail: impl Into<String>) -> AppError {
    AppError::AsrEnvInstallFailed { detail: detail.into() }
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), AppError> {
    if cancel.load(Ordering::Relaxed) {
        Err(AppError::AsrCancelled)
    } else {
        Ok(())
    }
}

fn emit_progress(app: &tauri::AppHandle, stage: &str, progress: i32, message: impl Into<String>) {
    let _ = app.emit("asr_env_progress", serde_json::json!({
        "stage": stage, "progress": progress, "message": message.into(),
    }));
}

/// 下载进度统计（可选挂到 asr_env_progress 事件上）。
#[derive(Clone, Copy, Default)]
struct DlStats {
    downloaded_mb: f64,
    total_mb: f64,
    speed_mbps: f64,
    eta_secs: Option<f64>,
}

fn emit_progress_dl(app: &tauri::AppHandle, stage: &str, progress: i32, message: impl Into<String>, s: DlStats) {
    let _ = app.emit("asr_env_progress", serde_json::json!({
        "stage": stage, "progress": progress, "message": message.into(),
        "downloaded_mb": s.downloaded_mb, "total_mb": s.total_mb,
        "speed_mbps": s.speed_mbps, "eta_secs": s.eta_secs,
    }));
}

/// 递归累加目录字节数（snapshot_download 的 .cache/*.incomplete 半成品也算已下载量）。
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                if let Ok(md) = e.metadata() {
                    if md.is_dir() {
                        stack.push(e.path());
                    } else {
                        total += md.len();
                    }
                }
            }
        }
    }
    total
}

/// 查询 HF 仓库文件总字节数（/api/models/<repo>?blobs=true 的 siblings[].size 求和）。
/// 失败返回 None——进度条退化为只显示已下载量和速度。
fn repo_total_bytes(endpoint: &str, repo: &str, proxy: Option<&str>) -> Option<u64> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent("zimufan/1.0")
        .timeout(Duration::from_secs(15));
    match proxy {
        Some(p) if !p.is_empty() => {
            if let Ok(px) = reqwest::Proxy::all(p) {
                builder = builder.proxy(px);
            }
        }
        _ => builder = builder.no_proxy(),
    }
    let resp = builder
        .build()
        .ok()?
        .get(format!("{endpoint}/api/models/{repo}?blobs=true"))
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().ok()?;
    let sum: u64 = v
        .get("siblings")?
        .as_array()?
        .iter()
        .filter_map(|s| s.get("size").and_then(|x| x.as_u64()))
        .sum();
    (sum > 0).then_some(sum)
}

/// 运行子进程，200ms 轮询 try_wait；cancel 置位即 kill 并返回 AsrCancelled。
/// stderr 由后台线程持续排空（pip/HF 弱网重试输出可能超过 64KB 管道容量，
/// 不排空会写满阻塞子进程、挂死安装）；返回末尾 ~8K 字符供失败诊断。
fn run_cancellable(cmd: &mut Command, cancel: &AtomicBool) -> Result<String, AppError> {
    use std::io::Read as _;
    let mut child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(format!("spawn failed: {e}")))?;
    let err_handle = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf);
            let text = String::from_utf8_lossy(&buf);
            text.chars().rev().take(8192).collect::<String>().chars().rev().collect::<String>()
        })
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if cancel.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::AsrCancelled);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(fail(format!("wait failed: {e}"))),
        }
    };
    let tail = err_handle.and_then(|h| h.join().ok()).unwrap_or_default();
    if status.success() {
        Ok(tail)
    } else {
        Err(fail(format!("process exited {status}: {tail}")))
    }
}

/// 流式下载 URL 列表（逐个尝试）到目标文件，emit 下载进度；cancel 时中断并删除半成品。
fn download_to(
    urls: &[String],
    dest: &Path,
    proxy: Option<&str>,
    app: &tauri::AppHandle,
    cancel: &AtomicBool,
    stage: &str,
    progress_base: i32,
    progress_span: i32,
) -> Result<(), AppError> {
    use std::io::{Read as _, Write as _};
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent("zimufan/1.0")
        .timeout(Duration::from_secs(1800));
    match proxy {
        Some(p) if !p.is_empty() => {
            builder = builder
                .proxy(reqwest::Proxy::all(p).map_err(|e| fail(format!("proxy: {e}")))?);
        }
        _ => builder = builder.no_proxy(),
    }
    let client = builder.build().map_err(|e| fail(format!("http client: {e}")))?;

    let mut response = None;
    let mut last_err = String::new();
    for url in urls {
        match client.get(url).send() {
            Ok(r) if r.status().is_success() => {
                response = Some(r);
                break;
            }
            Ok(r) => last_err = format!("HTTP {}", r.status()),
            Err(e) => last_err = e.to_string(),
        }
        check_cancel(cancel)?;
    }
    let mut stream = response.ok_or_else(|| fail(format!("all mirrors failed: {last_err}")))?;
    let total = stream.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(dest).map_err(|e| fail(e.to_string()))?;
    let mut buf = [0u8; 65536];
    let mut done: u64 = 0;
    let mut last = Instant::now();
    let started = Instant::now();
    loop {
        if let Err(e) = check_cancel(cancel) {
            drop(file);
            let _ = std::fs::remove_file(dest);
            return Err(e);
        }
        let n = stream.read(&mut buf).map_err(|e| fail(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| fail(e.to_string()))?;
        done += n as u64;
        if last.elapsed() > Duration::from_millis(250) {
            let frac = if total > 0 { done as f64 / total as f64 } else { 0.0 };
            let elapsed = started.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 { done as f64 / elapsed } else { 0.0 };
            let eta = if total > done && speed > 0.0 {
                Some((total - done) as f64 / speed)
            } else {
                None
            };
            emit_progress_dl(
                app,
                stage,
                progress_base + (frac * progress_span as f64) as i32,
                format!("{:.0} / {:.0} MB", done as f64 / 1048576.0, total as f64 / 1048576.0),
                DlStats {
                    downloaded_mb: done as f64 / 1048576.0,
                    total_mb: total as f64 / 1048576.0,
                    speed_mbps: speed / 1048576.0,
                    eta_secs: eta,
                },
            );
            last = Instant::now();
        }
    }
    Ok(())
}

/// 解压 .tar.gz 到目录（flate2 + tar，python-build-standalone 全平台同格式）。
fn extract_tgz(archive: &Path, dest: &Path) -> Result<(), AppError> {
    let f = std::fs::File::open(archive).map_err(|e| fail(e.to_string()))?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut ar = tar::Archive::new(gz);
    ar.unpack(dest).map_err(|e| fail(format!("untar failed: {e}")))
}

const HF_ENDPOINT_DIRECT: &str = "https://huggingface.co";
const HF_ENDPOINT_MIRROR: &str = "https://hf-mirror.com";

/// 探测 HF 官方端点可达性：短超时 HEAD，拿到任意 HTTP 响应即算通
/// （DNS 污染 / TLS 重置 / 连接超时等传输层失败才判不可达）。
/// 显式 proxy 参数 → 经代理探测；否则 `.no_proxy()` 探测**纯直连**
/// （继承的环境代理不算"直连"，直连不通就回落镜像，流量不进代理）。
fn hf_reachable(proxy: Option<&str>) -> bool {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent("zimufan/1.0")
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(6));
    match proxy {
        Some(p) if !p.is_empty() => {
            if let Ok(px) = reqwest::Proxy::all(p) {
                builder = builder.proxy(px);
            }
        }
        _ => builder = builder.no_proxy(),
    }
    builder
        .build()
        .and_then(|c| c.head(HF_ENDPOINT_DIRECT).send())
        .is_ok()
}

/// 决定 snapshot_download 的 HF_ENDPOINT：直连可达 → None（用默认官方端点）；
/// 不可达 → hf-mirror 兜底。覆盖逃生口：ASR_HF_ENDPOINT。
fn resolve_hf_endpoint(reachable: bool) -> Option<String> {
    if reachable {
        None
    } else {
        Some(HF_ENDPOINT_MIRROR.to_string())
    }
}

/// 下载子进程必须清除的继承变量：与 venv 依赖组合后会造成实际故障。
/// - HF_ENDPOINT：用户全局镜像设置与代理叠加可能不可用（如 hf-mirror 经 HTTP
///   代理返回无 HF 元数据的响应），端点由探测结果统一接管；
/// - HF_HUB_ENABLE_HF_TRANSFER：venv 未装 hf_transfer，置 1 时下载直接 ValueError；
/// - *_OFFLINE：置 1 会让下载立即报 offline。
const HF_ENV_BLOCKLIST: &[&str] = &[
    "HF_ENDPOINT",
    "HF_HUB_ENABLE_HF_TRANSFER",
    "HF_HUB_OFFLINE",
    "TRANSFORMERS_OFFLINE",
];

/// 走官方直连/镜像直连时子进程需要清除的代理变量：
/// 继承的代理与端点叠加可能不可用（如 hf-mirror 经 HTTP 代理返回无 HF 元数据），
/// 且"能直连"的判定前提是流量不经代理，子进程应与探测路径一致。
const PROXY_ENV_VARS: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "http_proxy",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// 把 proxy 参数应用到子进程：显式给出 → 设 HTTPS_PROXY；否则清除继承的代理变量。
/// pip/模型下载等所有外网子进程统一走这里，保证与"直连优先"策略一致。
fn apply_proxy_env(cmd: &mut Command, proxy: Option<&str>) {
    match proxy {
        Some(p) if !p.is_empty() => {
            cmd.env("HTTPS_PROXY", p).env("https_proxy", p);
        }
        _ => {
            for k in PROXY_ENV_VARS {
                cmd.env_remove(k);
            }
        }
    }
}

/// 安装托管 ASR 环境：python-build-standalone → pip 依赖 → 默认模型（0.6B + ForcedAligner）。
///
/// `progress` 粗粒度分布：下载 python 0-15%，解压 15-20%，pip 20-55%，模型 55-95%，收尾 100%。
/// cancel 置位时中断当前步骤并杀掉运行中的子进程；失败/取消都会清理半成品目录。
pub fn install_asr_env(
    proxy: Option<&str>,
    model: Option<&str>,
    app: &tauri::AppHandle,
    cancel: &AtomicBool,
) -> Result<(), AppError> {
    let repos = install_repos(model);
    let home = asr_home().ok_or_else(|| fail("app_data_dir not initialized"))?;
    std::fs::create_dir_all(&home).map_err(|e| fail(e.to_string()))?;
    let venv = home.join("venv");
    let models_dir = home.join("models");

    let result = install_inner(proxy, app, cancel, &home, &venv, &models_dir, &repos);
    if result.is_err() {
        // 取消/失败：清理半成品（venv 与未完成的模型目录；已完成的模型保留，幂等续装）。
        if managed_python().is_none() && venv.exists() {
            let _ = std::fs::remove_dir_all(&venv);
        }
        for (dir, _) in ASR_MODEL_REPOS.iter().chain(std::iter::once(&ALIGNER_REPO)) {
            let m = models_dir.join(dir);
            if m.exists() && !m.join("config.json").is_file() {
                let _ = std::fs::remove_dir_all(&m);
            }
        }
        let _ = std::fs::remove_file(home.join(MARKER));
    }
    result
}

fn install_inner(
    proxy: Option<&str>,
    app: &tauri::AppHandle,
    cancel: &AtomicBool,
    home: &Path,
    venv: &Path,
    models_dir: &Path,
    repos: &[(&str, &str)],
) -> Result<(), AppError> {
    // 1) 下载 python 运行时（幂等：解释器已就位则跳过下载+解压）
    let python_fresh = managed_python().is_none();
    if python_fresh {
        emit_progress(app, "download_python", 0, "downloading standalone python");
        let tgz = home.join("python-runtime.tar.gz");
        download_to(&python_dist_urls(), &tgz, proxy, app, cancel, "download_python", 0, 15)?;
        check_cancel(cancel)?;
        emit_progress(app, "extract_python", 15, "extracting python runtime");
        let tmp = home.join("python_extract");
        let _ = std::fs::remove_dir_all(&tmp);
        extract_tgz(&tgz, &tmp)?;
        // install_only 归档顶层为 python/ 目录 → 挪为 asr/venv/
        let _ = std::fs::remove_dir_all(venv);
        std::fs::rename(tmp.join("python"), venv)
            .map_err(|e| fail(format!("relocate python failed: {e}")))?;
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_file(&tgz);
    }
    let py = managed_python().ok_or_else(|| fail("python interpreter missing after extract"))?;
    check_cancel(cancel)?;

    // 2) pip 安装依赖（幂等：标记文件在则跳过；本次新装了 python 则必须重装）。
    // env ASR_PIP_INDEX_URL 可换镜像源。
    if python_fresh || !home.join(MARKER).is_file() {
        emit_progress(app, "pip_install", 20, "installing qwen-asr / torch / silero-vad ...");
        let mut cmd = no_window(Command::new(&py));
        cmd.args(["-m", "pip", "install", "--no-cache-dir"]);
        if let Ok(idx) = std::env::var("ASR_PIP_INDEX_URL") {
            if !idx.is_empty() {
                cmd.args(["--index-url", &idx]);
            }
        }
        for dep in PIP_DEPS {
            cmd.arg(dep);
        }
        apply_proxy_env(&mut cmd, proxy);
        run_cancellable(&mut cmd, cancel)?;
    }
    check_cancel(cancel)?;

    // 3) 下载模型（每个模型一个 snapshot_download 子进程，幂等：完整目录在则跳过；
    // 半成品目录交给 snapshot_download 断点续传 .incomplete）。
    // HF 端点策略：ASR_HF_ENDPOINT 逃生口 > 探测 huggingface.co 直连（不经继承
    // 代理）> 不可达回落 hf-mirror。继承的 HF_ENDPOINT 一律清除后由探测结果接管；
    // 无显式 proxy 时子进程同时清除代理变量，与探测的直连路径保持一致。
    std::fs::create_dir_all(models_dir).map_err(|e| fail(e.to_string()))?;
    let need_download = repos
        .iter()
        .any(|(dir, _)| !is_valid_model_dir(&models_dir.join(dir)));
    let hf_endpoint = if !need_download {
        None
    } else {
        match std::env::var("ASR_HF_ENDPOINT") {
            Ok(ep) if !ep.is_empty() => Some(ep),
            _ => resolve_hf_endpoint(hf_reachable(proxy)),
        }
    };
    if hf_endpoint.as_deref() == Some(HF_ENDPOINT_MIRROR) {
        emit_progress(
            app,
            "download_model",
            55,
            format!("huggingface.co unreachable, fallback to {HF_ENDPOINT_MIRROR}"),
        );
    }
    for (i, (dir, repo)) in repos.iter().enumerate() {
        let dest = models_dir.join(dir);
        check_cancel(cancel)?;
        if is_valid_model_dir(&dest) {
            continue; // 已下载，幂等跳过
        }
        let base = 55 + (i as i32) * 20;
        emit_progress(app, "download_model", base, format!("downloading {repo}"));
        let code = format!(
            "import sys; from huggingface_hub import snapshot_download; \
             snapshot_download({repo:?}, local_dir={dst:?}); print('ok')",
            repo = repo,
            dst = dest.to_string_lossy(),
        );
        let mut cmd = no_window(Command::new(&py));
        cmd.args(["-c", &code]);
        for k in HF_ENV_BLOCKLIST {
            cmd.env_remove(k);
        }
        if let Some(ep) = &hf_endpoint {
            cmd.env("HF_ENDPOINT", ep);
        }
        apply_proxy_env(&mut cmd, proxy);

        // 进度轮询：snapshot_download 无输出进度，改为 1s 轮询 dest 目录体积
        // （.incomplete 半成品分块也计入），总量取自 HF API siblings[].size。
        let api_ep = hf_endpoint.as_deref().unwrap_or(HF_ENDPOINT_DIRECT);
        let total_bytes = repo_total_bytes(api_ep, repo, proxy);
        let stop = Arc::new(AtomicBool::new(false));
        let poller = {
            let (app2, dest2, flag) = (app.clone(), dest.clone(), stop.clone());
            let repo = (*repo).to_string();
            std::thread::spawn(move || {
                let mut last_bytes = dir_size(&dest2);
                let mut last_t = Instant::now();
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    if flag.load(Ordering::Relaxed) {
                        break;
                    }
                    let now_b = dir_size(&dest2);
                    let dt = last_t.elapsed().as_secs_f64();
                    let speed = if dt > 0.0 { now_b.saturating_sub(last_bytes) as f64 / dt } else { 0.0 };
                    last_bytes = now_b;
                    last_t = Instant::now();
                    let frac = total_bytes
                        .filter(|t| *t > 0)
                        .map(|t| (now_b as f64 / t as f64).min(1.0))
                        .unwrap_or(0.0);
                    let eta = match (total_bytes, speed > 0.0) {
                        (Some(t), true) if t > now_b => Some((t - now_b) as f64 / speed),
                        _ => None,
                    };
                    emit_progress_dl(
                        &app2,
                        "download_model",
                        base + (frac * 20.0) as i32,
                        format!("downloading {repo}"),
                        DlStats {
                            downloaded_mb: now_b as f64 / 1048576.0,
                            total_mb: total_bytes.unwrap_or(0) as f64 / 1048576.0,
                            speed_mbps: speed / 1048576.0,
                            eta_secs: eta,
                        },
                    );
                }
            })
        };
        let r = run_cancellable(&mut cmd, cancel);
        stop.store(true, Ordering::Relaxed);
        let _ = poller.join();
        r?;
    }

    // 4) 完成标记
    let marker = serde_json::json!({
        "python": format!("{PBS_VER}+{PBS_TAG}"),
        "deps": PIP_DEPS,
        "models": repos.iter().map(|(d, _)| d).collect::<Vec<_>>(),
        "installed_at": chrono_like_now(),
    });
    std::fs::write(home.join(MARKER), serde_json::to_string_pretty(&marker).unwrap_or_default())
        .map_err(|e| fail(e.to_string()))?;
    emit_progress(app, "done", 100, "ASR environment ready");
    Ok(())
}

/// 删除托管 ASR 环境或其中某个模型。
/// - `model: None` → 整个 asr/ 目录（venv + 全部模型 + env.json）
/// - `model: Some(name)` → name 必须在 ASR 模型白名单内（防路径穿越）；
///   若它是最后一个已装 ASR 模型则连整个环境一起删（venv/deps/aligner/env.json），
///   否则只删 models/<name>。目标不存在时幂等返回 Ok。
/// 调用方保证无安装/转写任务在进行。
pub fn delete_asr_env(model: Option<&str>) -> Result<(), AppError> {
    let Some(home) = asr_home() else {
        return Ok(());
    };
    delete_asr_env_in(&home, model)
}

fn delete_asr_env_in(home: &Path, model: Option<&str>) -> Result<(), AppError> {
    match model {
        Some(name) => {
            if !QWEN3_MODEL_NAMES.contains(&name) {
                return Err(fail(format!("unknown model: {name}")));
            }
            let models_dir = home.join("models");
            let target = models_dir.join(name);
            // "模型数据"按目录存在计（半成品也算）；目标是最后一个模型目录 →
            // 环境失去意义整体清除；目标不存在则什么都不删，绝不殃及其他模型。
            let wipe_all = target.exists()
                && QWEN3_MODEL_NAMES
                    .iter()
                    .filter(|n| models_dir.join(n).exists())
                    .count()
                    <= 1;
            if wipe_all {
                if home.exists() {
                    std::fs::remove_dir_all(home)
                        .map_err(|e| fail(format!("remove asr env failed: {e}")))?;
                }
            } else if target.exists() {
                std::fs::remove_dir_all(&target)
                    .map_err(|e| fail(format!("remove model failed: {e}")))?;
            }
        }
        None => {
            if home.exists() {
                std::fs::remove_dir_all(home)
                    .map_err(|e| fail(format!("remove asr env failed: {e}")))?;
            }
        }
    }
    Ok(())
}

/// 简单时间戳（避免为标记文件引 chrono 依赖）。
fn chrono_like_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_status_without_env_reports_not_ready() {
        // 未初始化 APP_DATA_DIR（测试进程）→ 全 False，不 panic。
        let s = env_status();
        assert!(!s.ready);
    }

    #[test]
    fn python_dist_urls_have_proxy_and_direct() {
        let urls = python_dist_urls();
        assert_eq!(urls.len(), 2);
        assert!(urls[0].starts_with("https://gh-proxy.com/"));
        assert!(urls[1].contains("python-build-standalone"));
        assert!(urls[1].contains(PBS_TAG));
    }

    #[test]
    fn pinned_deps_cover_sidecar_imports() {
        // sidecar 脚本 import 的包都要在 PIP_DEPS 里：qwen_asr/torch/transformers/silero_vad/soundfile。
        let joined = PIP_DEPS.join(" ");
        for pkg in ["qwen-asr", "torch", "transformers", "silero-vad", "soundfile"] {
            assert!(joined.contains(pkg), "missing dep: {pkg}");
        }
    }

    /// 造一个临时托管环境目录（venv/ + env.json + models/），测试完手动清理。
    fn fake_asr_home(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("asr_del_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("venv")).unwrap();
        std::fs::write(dir.join(MARKER), "{}").unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        dir
    }

    /// 造一个模型目录：complete=true 带权重（视为已安装），false 只给 config（半成品）。
    fn mk_model(home: &Path, name: &str, complete: bool) {
        let d = home.join("models").join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("config.json"), "{}").unwrap();
        if complete {
            std::fs::write(d.join("model.safetensors"), b"x").unwrap();
        }
    }

    #[test]
    fn delete_asr_env_rejects_unknown_model_before_touching_fs() {
        // 白名单之外的模型名（含路径穿越）必须报错且不动任何文件。
        let home = fake_asr_home("reject");
        mk_model(&home, "Qwen3-ASR-0.6B", true);
        for bad in ["../evil", "not-a-model", ""] {
            let e = delete_asr_env_in(&home, Some(bad)).expect_err("must reject unknown model");
            assert!(format!("{e:?}").contains("unknown model"), "{bad}");
        }
        assert!(is_valid_model_dir(&home.join("models/Qwen3-ASR-0.6B")));
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn delete_selected_model_keeps_env_when_other_model_remains() {
        // 两个 ASR 模型都在：只删选中的目录，venv/对齐器/env.json/另一模型原样保留。
        let home = fake_asr_home("two");
        mk_model(&home, "Qwen3-ASR-0.6B", true);
        mk_model(&home, "Qwen3-ASR-1.7B", true);
        mk_model(&home, QWEN3_ALIGNER_NAME, true);

        delete_asr_env_in(&home, Some("Qwen3-ASR-0.6B")).unwrap();

        assert!(!home.join("models/Qwen3-ASR-0.6B").exists());
        assert!(is_valid_model_dir(&home.join("models/Qwen3-ASR-1.7B")));
        assert!(is_valid_model_dir(&home.join("models").join(QWEN3_ALIGNER_NAME)));
        assert!(home.join("venv").exists() && home.join(MARKER).exists());
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn delete_last_model_wipes_whole_env() {
        // 只剩一个 ASR 模型：删它连 venv/对齐器/env.json 一起清。
        let home = fake_asr_home("last");
        mk_model(&home, "Qwen3-ASR-0.6B", true);
        mk_model(&home, QWEN3_ALIGNER_NAME, true);

        delete_asr_env_in(&home, Some("Qwen3-ASR-0.6B")).unwrap();

        assert!(!home.exists());
    }

    #[test]
    fn delete_absent_model_is_noop_and_preserves_env() {
        // 0.6B 已装、删不存在的 1.7B：必须什么都不动（不能因"只剩一个"误删环境）。
        let home = fake_asr_home("absent");
        mk_model(&home, "Qwen3-ASR-0.6B", true);
        mk_model(&home, QWEN3_ALIGNER_NAME, true);

        delete_asr_env_in(&home, Some("Qwen3-ASR-1.7B")).unwrap();

        assert!(is_valid_model_dir(&home.join("models/Qwen3-ASR-0.6B")));
        assert!(home.join("venv").exists() && home.join(MARKER).exists());
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn delete_half_downloaded_model_counts_as_data() {
        // 0.6B 半成品（有 config 无权重）+ 1.7B 完整：删 1.7B → 半成品目录
        // 也算"模型数据"，只删目标，环境保留（0.6B 可续传）。
        let home = fake_asr_home("half");
        mk_model(&home, "Qwen3-ASR-0.6B", false);
        mk_model(&home, "Qwen3-ASR-1.7B", true);
        mk_model(&home, QWEN3_ALIGNER_NAME, true);

        delete_asr_env_in(&home, Some("Qwen3-ASR-1.7B")).unwrap();

        assert!(!home.join("models/Qwen3-ASR-1.7B").exists());
        assert!(home.join("models/Qwen3-ASR-0.6B").exists());
        assert!(home.join("venv").exists());
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn delete_env_without_model_removes_everything() {
        let home = fake_asr_home("all");
        mk_model(&home, "Qwen3-ASR-0.6B", true);

        delete_asr_env_in(&home, None).unwrap();

        assert!(!home.exists());
    }

    #[test]
    fn delete_env_on_missing_home_is_ok() {
        let home = std::env::temp_dir().join(format!("asr_del_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        assert!(delete_asr_env_in(&home, None).is_ok());
        assert!(delete_asr_env_in(&home, Some("Qwen3-ASR-0.6B")).is_ok());
    }

    #[test]
    fn hf_endpoint_falls_back_to_mirror_only_when_unreachable() {
        assert_eq!(resolve_hf_endpoint(true), None);
        assert_eq!(
            resolve_hf_endpoint(false),
            Some(HF_ENDPOINT_MIRROR.to_string())
        );
        assert_eq!(HF_ENDPOINT_MIRROR, "https://hf-mirror.com");
    }

    #[test]
    fn cancel_flag_short_circuits() {
        let c = AtomicBool::new(true);
        assert!(matches!(check_cancel(&c), Err(AppError::AsrCancelled)));
        let c2 = AtomicBool::new(false);
        assert!(check_cancel(&c2).is_ok());
    }

    /// run_cancellable 的 cancel→kill 路径（无模型依赖，unix shell）。
    #[cfg(unix)]
    #[test]
    fn run_cancellable_kills_on_cancel() {
        let cancel = AtomicBool::new(false);
        let flag = &cancel;
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                flag.store(true, Ordering::Relaxed);
            });
            let mut cmd = no_window(Command::new("sh"));
            cmd.args(["-c", "exec sleep 60"]);
            let t0 = Instant::now();
            let r = run_cancellable(&mut cmd, &cancel);
            assert!(matches!(r, Err(AppError::AsrCancelled)));
            assert!(t0.elapsed() < Duration::from_secs(5));
        });
    }

    /// stderr 排空回归：子进程写 >64KB stderr（远超管道容量）也必须正常退出——
    /// 若排空线程被改回"退出后再读"，此测试会因死锁超时/挂起。
    #[cfg(unix)]
    #[test]
    fn run_cancellable_drains_large_stderr() {
        let cancel = AtomicBool::new(false);
        let mut cmd = no_window(Command::new("sh"));
        // ~3000 行 × ~30B ≈ 90KB stderr，超过 64KB 管道容量。
        cmd.args(["-c", "i=0; while [ $i -lt 3000 ]; do echo 'some stderr diagnostic line' >&2; i=$((i+1)); done"]);
        let t0 = Instant::now();
        let r = run_cancellable(&mut cmd, &cancel);
        assert!(r.is_ok(), "stderr 排空失效会死锁: {r:?}");
        assert!(t0.elapsed() < Duration::from_secs(30));
    }

    /// 非零退出路径：Err detail 必须带 stderr 尾部文本供诊断。
    #[cfg(unix)]
    #[test]
    fn run_cancellable_surfaces_stderr_tail_on_failure() {
        let cancel = AtomicBool::new(false);
        let mut cmd = no_window(Command::new("sh"));
        cmd.args(["-c", "echo 'boom-reason-42' >&2; exit 3"]);
        let e = run_cancellable(&mut cmd, &cancel).expect_err("nonzero exit must fail");
        match e {
            AppError::AsrEnvInstallFailed { detail } => {
                assert!(detail.contains("boom-reason-42"), "stderr tail missing: {detail}");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }
}
