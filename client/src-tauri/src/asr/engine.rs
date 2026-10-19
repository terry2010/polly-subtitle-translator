// ASR 识别后端抽象：macOS 原生 Speech.framework（仅 macOS）+ Qwen3-ASR Python 侧边（FP4 接入）。
//
// 移植自 srt-converter-demo/src-tauri/src/asr.rs 的 Mac 原生部分；
// 差异点：client 无 mac_speech feature——speech 是 macOS target 的非可选依赖，
// 平台差异直接用 cfg!(target_os = "macos") 表达；另加 macOS <13 的运行时门控。
use super::cues::Segment;
use std::path::Path;

/// 可选的识别后端。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Backend {
    /// Apple Speech.framework：设备端、离线、无需联网（仅 macOS 13+）。
    MacNative,
    /// Qwen3-ASR：托管 Python 环境 + 本地模型（跨平台，见 asr/env）。
    Qwen3,
}

impl Backend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::MacNative => "mac_native",
            Backend::Qwen3 => "qwen3_asr",
        }
    }
    /// 输出文件名的后缀标签：不同识别方式写不同的 `.srt`，避免相互覆盖。
    pub fn output_tag(&self) -> &'static str {
        match self {
            Backend::MacNative => "mac",
            Backend::Qwen3 => "qwen3",
        }
    }
}

/// 归一化前端/音轨传入的语言标签为 Speech/Qwen3 需要的 locale 码。
///
/// 容错输入：语言名（"中文"/"English"…）、短码（en/zh/ja）、BCP47（en-US/zh-CN）、
/// ffprobe 流标签（eng/jpn/chi/yue/cmn/kor）。未识别的原样返回。
pub fn normalize_language(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "中文" | "chinese" | "zh" | "zho" | "chi" | "cmn" | "zh-cn" | "zh-hans" => "zh-CN".into(),
        "粤语" | "cantonese" | "yue" | "yue-cn" | "zh-hk" | "zh-hant" => "yue-CN".into(),
        "english" | "英语" | "en" | "eng" | "en-us" => "en-US".into(),
        "日本語" | "japanese" | "日语" | "ja" | "jpn" | "ja-jp" => "ja-JP".into(),
        "한국어" | "korean" | "韩语" | "ko" | "kor" | "ko-kr" => "ko-KR".into(),
        _ => {
            // Unicode 脚本兜底：含假名→ja、谚文→ko、其余 CJK→zh（"日文"/"韩文"/"汉语" 等）。
            let s = raw.trim();
            if s.chars().any(|c| ('\u{3040}'..='\u{30FF}').contains(&c)) {
                return "ja-JP".into();
            }
            if s.chars().any(|c| ('\u{AC00}'..='\u{D7AF}').contains(&c) || ('\u{1100}'..='\u{11FF}').contains(&c)) {
                return "ko-KR".into();
            }
            if s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)) {
                return "zh-CN".into();
            }
            s.to_lowercase()
        }
    }
}

/// 当前平台是否编译了 Mac 原生识别（仅 macOS）。
pub fn native_speech_compiled() -> bool {
    cfg!(target_os = "macos")
}

/// macOS < 13 无 Swift 运行时/设备端识别保障，运行时门控。
/// 用 tauri-plugin-os 读系统版本；解析失败（非 macOS/异常）按不可用处理。
fn macos_at_least_13() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let v = tauri_plugin_os::version().to_string();
    v.split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .map(|major| major >= 13)
        .unwrap_or(false)
}

// ── TCC responsible-process 守护 ────────────────────────────────────────
// macOS TCC 把隐私授权请求归因到 *responsible 进程*：由 launchd/`.app`/`open`
// 或 Terminal 拉起的进程对自己负责，读自身（或嵌入的）Info.plist；被 IDE/
// Electron 应用（Devin/Windsurf/VSCode 集成终端）spawn 的裸二进制则被记到
// 父应用名下。父应用没声明 NSSpeechRecognitionUsageDescription 时，TCC 不
// 弹授权框而是直接 SIGABRT 整个进程。先探测归属，避免无提示崩溃。

/// libproc 公开 API：取某 pid 的可执行文件路径。
#[cfg(target_os = "macos")]
fn proc_exe_path(pid: i32) -> Option<std::path::PathBuf> {
    extern "C" {
        fn proc_pidpath(pid: i32, buf: *mut u8, bufsize: u32) -> i32;
    }
    let mut buf = vec![0u8; 4096];
    let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr(), buf.len() as u32) };
    (n > 0).then(|| {
        std::path::PathBuf::from(String::from_utf8_lossy(&buf[..n as usize]).into_owned())
    })
}

/// libSystem 私有 SPI（Chromium/Electron 同款用法）：本进程 TCC 责任归属的 pid。
/// 符号不在公开 tbd 里，只能 dlsym 运行时解析；取不到返回 None。
#[cfg(target_os = "macos")]
fn responsible_pid() -> Option<i32> {
    type RespFn = unsafe extern "C" fn(i32) -> i32;
    unsafe {
        let lib = libloading::os::unix::Library::this();
        let sym: libloading::os::unix::Symbol<RespFn> = lib
            .get(b"responsibility_get_pid_responsible_for_pid\0")
            .ok()?;
        let pid = (*sym)(std::process::id() as i32);
        (pid > 0).then_some(pid)
    }
}

/// 可执行路径所在的 `.app` 的 `Contents/Info.plist`（取最近的 bundle）。
#[cfg(target_os = "macos")]
fn enclosing_app_plist(exe: &Path) -> Option<std::path::PathBuf> {
    exe.ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
        .map(|app| app.join("Contents").join("Info.plist"))
}

/// plist 是否声明了 NSSpeechRecognitionUsageDescription。
/// 字节级包含判断即可：XML plist 是明文；binary plist 的 key 字符串
/// 也原样保存在对象表中。
#[cfg(target_os = "macos")]
fn plist_declares_speech_usage(plist: &Path) -> bool {
    const KEY: &[u8] = b"NSSpeechRecognitionUsageDescription";
    let Ok(bytes) = std::fs::read(plist) else {
        return false;
    };
    bytes.windows(KEY.len()).any(|w| w == KEY)
}

/// 当前启动上下文下发起 SFSpeechRecognizer 授权请求是否会触发 TCC abort。
/// 返回 Some(原因) 时应向用户返回错误而不是调用 request_authorization。
///
/// 判定：responsible 是自己 → 读自身 plist（bundle 或嵌入段），必然有 key；
/// responsible 是别的进程 → 只有它的 plist 声明了 key 才允许（授权框以它的
/// 名义弹出）；SPI 不可用（老系统）→ 保守放行，维持原行为。
#[cfg(target_os = "macos")]
fn speech_request_blocker() -> Option<String> {
    let self_pid = std::process::id() as i32;
    let owner = responsible_pid()?;
    if owner == self_pid {
        return None;
    }
    let owner_exe = proc_exe_path(owner);
    if owner_exe
        .as_deref()
        .and_then(enclosing_app_plist)
        .is_some_and(|plist| plist_declares_speech_usage(&plist))
    {
        return None;
    }
    let owner_desc = owner_exe
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("pid {owner}"));
    Some(format!(
        "speech authorization cannot be requested: the TCC-responsible owner is \
         {owner_desc}, which declares no NSSpeechRecognitionUsageDescription — \
         macOS kills this process instead of showing the prompt. Relaunch via the \
         .app bundle (npm run start), open it from Finder, run the binary from a \
         real Terminal, or use the Qwen3 backend instead."
    ))
}

/// Mac 原生识别不可用时的原因（供 UI 展示/禁用引擎）；可用时返回 None。
/// 返回英文技术 detail，由 IPC 层包成错误码。
pub fn native_speech_unavailable_reason() -> Option<String> {
    if !native_speech_compiled() {
        return Some("macOS-only backend (Speech.framework)".to_string());
    }
    if !macos_at_least_13() {
        return Some("requires macOS 13+ (on-device speech recognition)".to_string());
    }
    None
}

/// Mac 原生识别的**一次性**预检：授权 + 识别器可用性。
///
/// 授权对话框（speech crate 内部是阻塞信号量、30s 超时）只允许触发一次，
/// 因此必须在切片循环之前调用，而不是每个切片各调一次。
/// 返回 Ok(()) 后，切片循环里只需做廉价的 authorization_status() 检查。
#[cfg(target_os = "macos")]
pub fn prepare_mac_native(language: &str) -> Result<(), String> {
    use speech::prelude::*;
    use speech::AuthorizationStatus;

    // 1) 已授权/被拒直接读状态；仅在"未决定"时阻塞弹一次系统授权框。
    //    授权请求归因到未声明 key 的父应用时 TCC 直接 abort——先拦下。
    let mut status = SpeechRecognizer::authorization_status();
    if status == AuthorizationStatus::NotDetermined {
        if let Some(why) = speech_request_blocker() {
            return Err(why);
        }
        status = SpeechRecognizer::request_authorization()
            .map_err(|e| format!("request speech authorization failed: {e:?}"))?;
    }
    if !status.is_authorized() {
        return Err(format!(
            "speech recognition not authorized ({status:?}) — grant in System Settings > Privacy & Security > Speech Recognition"
        ));
    }

    // 2) 校验该 locale 的识别器存在且可用，避免进切片循环后逐片失败。
    let recognizer = SpeechRecognizer::with_locale(language)
        .map_err(|e| format!("create recognizer failed (lang {language}): {e}"))?;
    if !recognizer.is_available() {
        return Err(format!(
            "recognizer unavailable for {language} (language assets may not be installed)"
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn prepare_mac_native(_language: &str) -> Result<(), String> {
    Err(native_speech_unavailable_reason().unwrap_or_else(|| "Mac native ASR unavailable".to_string()))
}

/// Mac 原生识别：SFSpeechRecognizer，纯本地、设备端、无需联网。
///
/// 授权**不在此处**触发（见 [`prepare_mac_native`]）：这里只做廉价状态检查，
/// 避免 N 个切片重复走阻塞式授权。
#[cfg(target_os = "macos")]
pub fn transcribe_mac_native(path: &Path, language: &str) -> Result<Vec<Segment>, String> {
    use speech::prelude::*;

    let status = SpeechRecognizer::authorization_status();
    if !status.is_authorized() {
        return Err(format!(
            "speech recognition not authorized ({status:?}) — retry to trigger the prompt, or grant it in System Settings"
        ));
    }

    let recognizer = SpeechRecognizer::with_locale(language)
        .map_err(|e| format!("create recognizer failed (lang {language}): {e}"))?;
    if !recognizer.is_available() {
        return Err(format!("recognizer unavailable for {language}"));
    }

    // 默认走设备端识别（speech crate 的 RecognitionRequestOptions 默认
    // requires_on_device_recognition=true）：纯离线，但对背景音乐/噪声下的
    // 语音非常保守。设 SRT_MAC_ONDEVICE=0 切到 Apple 服务端模型（联网、准确率
    // 高很多、带自动标点）。仅作诊断/对比开关，默认保持离线。
    let on_device = std::env::var("SRT_MAC_ONDEVICE")
        .map(|v| v != "0")
        .unwrap_or(true);
    if !on_device {
        let options = RecognitionRequestOptions::new()
            .with_requires_on_device_recognition(false)
            .with_adds_punctuation(true)
            .with_task_hint(TaskHint::Dictation);
        let request = UrlRecognitionRequest::new(path).with_options(options);
        let detailed = recognizer
            .recognize_request(&request)
            .map_err(|e| format!("recognize failed: {e:?}"))?;
        return Ok(detailed
            .best_transcription
            .segments
            .into_iter()
            .map(|s| Segment {
                text: s.substring,
                start_seconds: s.timestamp,
                duration_seconds: s.duration,
            })
            .collect());
    }

    // recognize_in_path 返回带每段 timestamp（相对文件起点）和 duration 的结果。
    let result = recognizer
        .recognize_in_path(path)
        .map_err(|e| format!("recognize failed: {e:?}"))?;

    Ok(result
        .segments
        .into_iter()
        .map(|s| Segment {
            text: s.text,
            start_seconds: s.timestamp,
            duration_seconds: s.duration,
        })
        .collect())
}

#[cfg(not(target_os = "macos"))]
pub fn transcribe_mac_native(_path: &Path, _language: &str) -> Result<Vec<Segment>, String> {
    Err(native_speech_unavailable_reason().unwrap_or_else(|| "Mac native ASR unavailable".to_string()))
}

/// 把单个音频文件（16k 单声道）识别为段落。返回的段落 start_seconds 相对该文件起点。
/// Qwen3 逐片批转写在 sidecar 层（FP4）；单文件直转只用于诊断。
pub fn transcribe_file(
    path: &Path,
    language: &str,
    backend: Backend,
) -> Result<Vec<Segment>, String> {
    match backend {
        Backend::MacNative => transcribe_mac_native(path, language),
        // Qwen3 的批量/增量协议在 FP4（asr::qwen3）接入。
        Backend::Qwen3 => Err("Qwen3 backend is per-chunk batched — use asr::qwen3::transcribe_qwen3_chunks".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_language_maps_aliases() {
        assert_eq!(normalize_language("中文"), "zh-CN");
        assert_eq!(normalize_language("English"), "en-US");
        assert_eq!(normalize_language("en"), "en-US");
        assert_eq!(normalize_language("日本語"), "ja-JP");
        assert_eq!(normalize_language("한국어"), "ko-KR");
        assert_eq!(normalize_language("Cantonese"), "yue-CN");
        assert_eq!(normalize_language("eng"), "en-US"); // ffprobe 流标签
        assert_eq!(normalize_language("jpn"), "ja-JP");
        assert_eq!(normalize_language("yue"), "yue-CN");
        assert_eq!(normalize_language("にほんご"), "ja-JP"); // Unicode 脚本兜底：假名→ja
        assert_eq!(normalize_language("한국"), "ko-KR");   // 谚文→ko
        assert_eq!(normalize_language("日文"), "zh-CN");   // 纯 CJK 无法与"汉语"区分→zh
        assert_eq!(normalize_language("汉语"), "zh-CN");
        assert_eq!(normalize_language("custom-XX"), "custom-xx"); // 未识别原样小写返回
    }

    #[test]
    fn qwen3_single_file_dispatch_is_stub() {
        // FP4 前占位契约：单文件直转不允许走 Qwen3（它是逐片批转协议）。
        let e = transcribe_file(Path::new("/tmp/x.wav"), "en-US", Backend::Qwen3)
            .expect_err("Qwen3 single-file must be rejected");
        assert!(e.contains("asr::qwen3"), "{e}");
    }

    #[test]
    fn backend_tags_stable() {
        assert_eq!(Backend::MacNative.as_str(), "mac_native");
        assert_eq!(Backend::MacNative.output_tag(), "mac");
        assert_eq!(Backend::Qwen3.as_str(), "qwen3_asr");
        assert_eq!(Backend::Qwen3.output_tag(), "qwen3");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn enclosing_app_plist_finds_nearest_bundle() {
        let exe = Path::new("/Applications/Devin.app/Contents/MacOS/Devin");
        assert_eq!(
            enclosing_app_plist(exe),
            Some(std::path::PathBuf::from(
                "/Applications/Devin.app/Contents/Info.plist"
            ))
        );
        assert_eq!(enclosing_app_plist(Path::new("/usr/bin/zimufan")), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn plist_key_detects_both_ways() {
        let dir = std::env::temp_dir().join(format!("zimufan-plist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let with = dir.join("with.plist");
        let without = dir.join("without.plist");
        std::fs::write(
            &with,
            "<dict><key>NSSpeechRecognitionUsageDescription</key><string>x</string></dict>",
        )
        .unwrap();
        std::fs::write(&without, "<dict><key>NSMicrophoneUsageDescription</key><string>x</string></dict>")
            .unwrap();
        assert!(plist_declares_speech_usage(&with));
        assert!(!plist_declares_speech_usage(&without));
        assert!(!plist_declares_speech_usage(&dir.join("missing.plist")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tcc_guard_does_not_panic() {
        // responsible_pid 在 libSystem 上应能解析；blocker 结果取决于
        // 启动环境（IDE 下为 Some，Terminal/.app 下为 None），只断言不崩。
        assert!(responsible_pid().is_none_or(|p| p > 0));
        let _ = speech_request_blocker();
    }

    #[test]
    fn native_availability_matches_platform() {
        // 非 macOS 必然报不可用原因；macOS 上取决于系统版本（<13 报不可用）。
        if cfg!(target_os = "macos") {
            if macos_at_least_13() {
                assert_eq!(native_speech_unavailable_reason(), None);
            } else {
                assert!(native_speech_unavailable_reason().is_some());
            }
        } else {
            assert!(native_speech_unavailable_reason().is_some());
        }
    }
}
