// 逐词识别结果 → 可读字幕条：重叠去重 + 停顿断句 + SRT 渲染/解析。
//
// 移植自 srt-converter-demo/src-tauri/src/srt.rs；Silence 类型改用 crate::asr::audio::Silence。
// 输出仍走「写 SRT 文件 → 前端 loadSubtitle 进既有编辑/翻译链路」，
// 因此渲染格式与 demo 完全一致（HH:MM:SS.mmm、逗号替换为空格、空行分隔）。
use super::audio::Silence;
use std::path::{Path, PathBuf};

/// 一段识别结果：文本 + 绝对起止秒数。
#[derive(Debug, Clone)]
pub struct Segment {
    pub text: String,
    /// 在整个音频中的起始秒数（相对文件开头）。
    pub start_seconds: f64,
    /// 该段持续秒数。
    pub duration_seconds: f64,
}

impl Segment {
    pub fn end_seconds(&self) -> f64 {
        self.start_seconds + self.duration_seconds
    }
}

/// 把秒数转成 SRT 时间戳：HH:MM:SS,mmm。
pub fn format_timecode(seconds: f64) -> String {
    let total = seconds.floor() as i64;
    let millis = ((seconds - total as f64).abs() * 1000.0).round() as i64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, millis)
}

/// 解析 SRT 时间戳 "HH:MM:SS,mmm"（兼容 `.` 分隔毫秒）。
fn parse_timecode(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let mut it = s.split(':');
    let h: f64 = it.next()?.parse().ok()?;
    let m: f64 = it.next()?.parse().ok()?;
    let sec: f64 = it.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// 解析 render_srt 产物 / 常规 SRT：序号行、" --> " 时间行、文本行（可多行）。
/// 重试失败切片时用它把已有字幕读回来与补识别结果合并。
pub fn parse_srt(content: &str) -> Vec<Segment> {
    let mut segs = Vec::new();
    let mut cur: Option<(f64, f64)> = None;
    let mut text = String::new();
    let mut flush = |cur: &mut Option<(f64, f64)>, text: &mut String| {
        if let Some((start, end)) = cur.take() {
            let t = text.trim().to_string();
            if !t.is_empty() {
                segs.push(Segment {
                    text: t,
                    start_seconds: start,
                    duration_seconds: (end - start).max(0.001),
                });
            }
            text.clear();
        }
    };
    for line in content.lines() {
        if let Some((a, b)) = line.split_once("-->") {
            flush(&mut cur, &mut text);
            match (parse_timecode(a), parse_timecode(b)) {
                (Some(s), Some(e)) => cur = Some((s, e)),
                _ => cur = None,
            }
        } else if cur.is_some() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(line.trim());
        }
    }
    flush(&mut cur, &mut text);
    segs
}

/// 渲染 SRT 正文（不含 BOM）。每条段 3 行 + 一个空行分隔：序号、起止时间（` --> ` 分隔）、文本。
/// 时间戳中的逗号在渲染时替换为点，生成逗号-free、含 `-->` 的合法 SRT。
pub fn render_srt(segments: &[Segment]) -> String {
    let mut out = String::new();
    for (i, seg) in segments.iter().enumerate() {
        let start = format_timecode(seg.start_seconds).replace(',', ".");
        let end = format_timecode(seg.end_seconds()).replace(',', ".");
        // SRT 内不允许换行与逗号，统一替换为空格。
        let text = seg
            .text
            .split('\n')
            .map(|line| line.trim().replace(',', " "))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, start, end, text));
    }
    out
}

/// 根据视频文件路径与后端标签推导输出路径：`<stem>-<tag>.srt`。
///
/// 不同识别方式写不同文件，避免互相覆盖、也方便对比。
pub fn srt_path_for(video_path: &Path, tag: &str) -> PathBuf {
    let stem = Path::new(video_path).file_stem().unwrap_or_else(|| "output".as_ref());
    video_path.with_file_name(format!("{}-{}.srt", stem.to_string_lossy(), tag))
}

/// ASR 转写结果缓存路径：与内嵌流提取同一套键值——
/// extract_cache/{oshash}.{midhash}.asr.{tag}.{model}.{lang}.srt。
/// model/lang 中的 '.','/','\' 替换为 '_' 保证文件名可解析；hash 失败回退视频旁路径。
/// 对应恢复扫描见 ipc::list_asr_subtitles_cmd。
pub fn asr_cache_srt_path(video_path: &Path, tag: &str, model: Option<&str>, lang: &str) -> PathBuf {
    let san = |s: &str| s.replace(['.', '/', '\\'], "_");
    if let (Ok(vh), Some(dir)) = (
        crate::ffmpeg::compute_video_hash(&video_path.to_string_lossy()),
        crate::ffmpeg::extract_cache_dir(),
    ) {
        let model_s = model.map(san).unwrap_or_else(|| "none".to_string());
        return dir.join(format!(
            "{}.{}.asr.{}.{}.{}.srt",
            vh.oshash, vh.midhash, tag, model_s, san(lang)
        ));
    }
    srt_path_for(video_path, tag)
}

// ===== 重叠切片词归属过滤 =====

/// margin 词与已收词的「副本」判定距离：词中心相距不超过该值才可能为同一词。
const OVERLAP_DUP_CENTER_SECONDS: f64 = 0.35;

/// 文本归一化：小写 + 只留字母数字（"didn't"/"didnt"/"Didn’t" 视为同词）。
fn norm_text(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 两段文本是否「同一词的两份拷贝」：归一化相等或互相包含
/// （边界词可能被一片认成 "distillery"、另一片只听到 "tillery"）。
fn same_word_text(a: &str, b: &str) -> bool {
    let (a, b) = (norm_text(a), norm_text(b));
    !a.is_empty() && !b.is_empty() && (a == b || a.contains(&b) || b.contains(&a))
}

/// 按归属区间过滤逐词结果（重叠切片去重）。
///
/// `items` 是 (词片段, keep_start, keep_end)：词中心落在 [keep_start, keep_end)
/// 的直接收；落在重叠 margin 里的进入待定区——若已收词中存在中心相距 ≤0.35s
/// 且文本相似的副本则丢弃（邻片已收过它），否则补收。补收兜底是为防止边界词
/// 因两片词时间戳微漂被双双丢弃；要求文本相似可避免误丢紧邻的不同词。
///
/// 返回按起始时间排序的结果。
pub fn collect_kept_words(items: Vec<(Segment, f64, f64)>) -> Vec<Segment> {
    let mut kept: Vec<Segment> = Vec::new();
    let mut margin: Vec<Segment> = Vec::new();
    for (s, keep_start, keep_end) in items {
        let center = s.start_seconds + s.duration_seconds * 0.5;
        if center >= keep_start && center < keep_end {
            kept.push(s);
        } else {
            margin.push(s);
        }
    }
    for s in margin {
        let center = s.start_seconds + s.duration_seconds * 0.5;
        let covered = kept.iter().any(|k| {
            let kc = k.start_seconds + k.duration_seconds * 0.5;
            (kc - center).abs() <= OVERLAP_DUP_CENTER_SECONDS && same_word_text(&k.text, &s.text)
        });
        if !covered {
            kept.push(s);
        }
    }
    kept.sort_by(|a, b| {
        a.start_seconds
            .partial_cmp(&b.start_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    kept
}

// ===== 逐词结果 → 可读字幕条 =====

/// 静音段中点落在词边界 ±该容差内，即认为该边界是"说话的停顿"。
/// 识别出的词时间戳与实际音频通常有 ±0.2s 漂移，用重叠秒数判定过于脆弱。
const PAUSE_TOLERANCE_SECONDS: f64 = 0.3;
/// 认定为停顿的最短静音时长（秒）。0.4s 吞掉台词中的节拍停顿
///（"do not …… disturb" 这类 ~0.35s 的 dramatic beat），仍保留真实句界。
const PAUSE_MIN_SILENCE_SECONDS: f64 = 0.4;
/// 没有静音信息时的兜底断句阈值（秒）。1.0s 吞掉台词里的表演性停顿
///（"do not ... disturb" 这类句中换气），仍能对长停顿分句；
/// cue 长度另有 CUE_MAX_CHARS / CUE_MAX_DURATION_SECONDS 双封顶。
const CUE_FALLBACK_GAP_SECONDS: f64 = 1.0;
/// 单条字幕的最大时长（秒）。
const CUE_MAX_DURATION_SECONDS: f64 = 6.0;
/// 单条字幕的最大字符数（约两行）。
const CUE_MAX_CHARS: usize = 60;

/// 判定"碎条"的最短时长（秒）：识别器偶尔给出 0.1s 级的假词边界，这种一闪而过的字幕要回并。
const CUE_SHORT_DURATION_SECONDS: f64 = 0.25;
/// 孤立闪现判定：前后都超过该间隔、且时长/文本都极短的字幕，几乎都是模型在音乐/噪声上的幻觉。
const ISOLATED_FLASH_GAP_SECONDS: f64 = 1.5;
/// 时间重叠超过该值的同文本词视为切片边界的重复拷贝，合并时丢弃。
const OVERLAP_SAME_WORD_SECONDS: f64 = OVERLAP_DUP_CENTER_SECONDS;
/// 递归拆分时左右两侧的最少字符数：小于该值的碎尾（"disturb"/"with your"）不切。
const CUE_MIN_SPLIT_PART_CHARS: usize = 10;
/// 单条字幕的最短时长（秒），避免零长度字幕。
const CUE_MIN_DURATION_SECONDS: f64 = 0.08;

/// 稀疏段（孤词）时长钳制：对齐器/整片级时间戳偶尔把一两个字的孤词拉满整个
/// 区间（22.7s 的「おい」、8.8s 的「て」）。低于 1 字/秒不可能是真实对白密度，
/// 判为时间戳失真，按「每字 0.35s + 0.5s 尾巴」折算显示时长（夹在 1.0s ~ 上限内）。
const SPARSE_MIN_DURATION_SECONDS: f64 = 4.0;
const SPARSE_MIN_CHARS_PER_SECOND: f64 = 1.0;
const SPARSE_SECONDS_PER_CHAR: f64 = 0.35;
const SPARSE_TAIL_SECONDS: f64 = 0.5;
const SPARSE_FLOOR_SECONDS: f64 = 1.0;

/// 稀疏段的显示时长：密度正常原样返回；孤词被拉长时按文本长度钳回。
fn clamp_sparse_duration(text: &str, duration: f64) -> f64 {
    let chars = text.chars().count() as f64;
    if duration <= SPARSE_MIN_DURATION_SECONDS
        || chars / duration >= SPARSE_MIN_CHARS_PER_SECOND
    {
        return duration;
    }
    duration.min(
        (chars * SPARSE_SECONDS_PER_CHAR + SPARSE_TAIL_SECONDS)
            .clamp(SPARSE_FLOOR_SECONDS, CUE_MAX_DURATION_SECONDS),
    )
}

/// 是否 CJK 字符（中日文）：相邻时不加空格。
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F |   // CJK 标点
        0x3040..=0x30FF |   // 日文假名
        0x3400..=0x4DBF |
        0x4E00..=0x9FFF |
        0xF900..=0xFAFF |
        0x20000..=0x2FA1F)
}

/// 是否是"句末"标点：遇到就结束当前字幕条。
fn ends_sentence(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…' | ';' | '。' | '！' | '？' | '；')
}

/// 词边界 [a,b] 处的"停顿强度"：静音段中点落在边界 ± 容差内才计入，
/// 取重叠静音时长（截断到 0.6s）作为分数；没有停顿返回 0。
fn pause_score(a: f64, b: f64, silences: &[Silence]) -> f64 {
    let lo = a - PAUSE_TOLERANCE_SECONDS;
    let hi = b + PAUSE_TOLERANCE_SECONDS;
    silences
        .iter()
        .filter(|s| s.end - s.start >= PAUSE_MIN_SILENCE_SECONDS)
        .map(|s| {
            let mid = 0.5 * (s.start + s.end);
            if mid >= lo && mid <= hi { (s.end - s.start).min(0.6) } else { 0.0 }
        })
        .fold(0.0_f64, f64::max)
}

/// 把逐词/碎片化的识别结果合并成可读的字幕条。
///
/// 背景：macOS Speech.framework 与 Qwen3 + ForcedAligner 返回的都是**逐词**时间戳
///（每个词一条百余毫秒的字幕），直接渲染是「Let's / do / some / liquor / science」
/// 这种无法阅读的结果。
///
/// 分两步：
/// 1. 按"说话停顿"切句——优先用音频静音检测（`silences`，见 [`super::audio::detect_silences`]）
///    判断两个词之间是否真的停顿，其次看句末标点；没有静音信息时退化为按词间间隔。
///    系统识别器的词间间隔噪声大（同一句内 0.1~0.5s 都可能），单靠间隔无法区分换气与句界。
/// 2. 仍然过长的句子，在**内部最自然的停顿处**递归拆分（优先有静音的边界，
///    其次词间间隔最大的位置），而不是在固定字符数处硬切。
/// 3. 最后把一闪而过的碎条并入相邻字幕条。
///
/// 中日文相邻不加空格，其余语言用空格连接。
pub fn merge_into_cues(segments: Vec<Segment>, silences: &[Silence]) -> Vec<Segment> {
    let mut cues: Vec<Segment> = Vec::new();
    let mut sentence: Vec<Segment> = Vec::new();

    for seg in segments {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }
        let cur = Segment {
            text: text.to_string(),
            start_seconds: seg.start_seconds.max(0.0),
            duration_seconds: clamp_sparse_duration(text, seg.duration_seconds.max(0.0)),
        };
        if let Some(last) = sentence.last() {
            let last_end = last.end_seconds();
            let gap = cur.start_seconds - last_end;
            // 时间重叠的词几乎总是切片边界的同词重复/时间戳漂移（两片对同一词
            // 给出略不同的区间）：同文本的直接丢弃，不同文本的也视为连续语音
            // 不断句——真实话语的词是顺序排列的，大重叠不存在于正常语音里。
            if gap < -OVERLAP_SAME_WORD_SECONDS && same_word_text(&last.text, &cur.text) {
                continue;
            }
            let pause = pause_score(last_end, cur.start_seconds, silences) > 0.0;
            let fallback_break = silences.is_empty() && gap > CUE_FALLBACK_GAP_SECONDS;
            let sentence_end = ends_sentence(last.text.chars().last().unwrap_or(' '));
            if pause || fallback_break || sentence_end {
                flush_sentence(&mut sentence, &mut cues, silences);
            }
        }
        sentence.push(cur);
    }
    flush_sentence(&mut sentence, &mut cues, silences);
    cues = coalesce_short_cues(cues);
    cues
}

/// 把过短的碎条并入相邻字幕条（优先并入后一条，保持语序自然）。
///
/// 识别器给的词时长不可靠（例如 "up" 只有 0.09s），按停顿切句偶尔会在习语内部断开，
/// 产生「you」「so」「up」这种一闪而过的字幕。这里在不超过行数/时长上限的前提下回并。
fn coalesce_short_cues(mut cues: Vec<Segment>) -> Vec<Segment> {
    let short_at = |c: &Segment| {
        c.duration_seconds < CUE_SHORT_DURATION_SECONDS || (c.text.chars().count() <= 3 && c.duration_seconds < 0.6)
    };

    let mut i = 0;
    while i < cues.len() {
        if short_at(&cues[i]) {
            // 1) 优先并入后一条（保持语序自然）。
            if i + 1 < cues.len() && try_merge(&mut cues, i, i + 1) {
                continue; // 合并后可能仍然过短，重新判断
            }
            // 2) 否则并入前一条。
            if i > 0 && try_merge(&mut cues, i - 1, i) {
                i = i.saturating_sub(1);
                continue;
            }
            // 3) 两侧都并不可行，且前后都很久没有别的字幕 → 判定为音乐/噪声上的幻觉，丢弃。
            let gap_prev = if i > 0 { cues[i].start_seconds - cues[i - 1].end_seconds() } else { f64::INFINITY };
            let gap_next = if i + 1 < cues.len() { cues[i + 1].start_seconds - cues[i].end_seconds() } else { f64::INFINITY };
            if gap_prev > ISOLATED_FLASH_GAP_SECONDS
                && gap_next > ISOLATED_FLASH_GAP_SECONDS
                && cues[i].text.chars().count() <= 3
            {
                cues.remove(i);
                continue;
            }
        }
        i += 1;
    }
    cues
}

/// 把 `cues[b]` 并入 `cues[a]`（不超行数/时长上限时）；成功返回 true。
fn try_merge(cues: &mut Vec<Segment>, a: usize, b: usize) -> bool {
    let text = join_two(&cues[a].text, &cues[b].text);
    let duration = (cues[b].end_seconds() - cues[a].start_seconds).max(CUE_MIN_DURATION_SECONDS);
    if text.chars().count() > CUE_MAX_CHARS || duration > CUE_MAX_DURATION_SECONDS {
        return false;
    }
    cues[a].text = text;
    cues[a].duration_seconds = duration;
    cues.remove(b);
    true
}

/// 拼接两条字幕文本（中日文之间不加空格）。
fn join_two(a: &str, b: &str) -> String {
    let a = a.trim();
    let b = b.trim();
    if a.is_empty() {
        return b.to_string();
    }
    if b.is_empty() {
        return a.to_string();
    }
    let prev = a.chars().last().unwrap_or(' ');
    let next = b.chars().next().unwrap_or(' ');
    if is_cjk(prev) && is_cjk(next) {
        format!("{a}{b}")
    } else {
        format!("{a} {b}")
    }
}

/// 把当前累积的句子按长度/时长限制切成字幕条。
fn flush_sentence(sentence: &mut Vec<Segment>, out: &mut Vec<Segment>, silences: &[Silence]) {
    let words = std::mem::take(sentence);
    split_to_cues(&words, out, silences);
}

/// 递归拆分：句子太长时在内部最自然的停顿处断开。
fn split_to_cues(words: &[Segment], out: &mut Vec<Segment>, silences: &[Silence]) {
    if words.is_empty() {
        return;
    }
    let start = words[0].start_seconds;
    let end = words.iter().map(|w| w.end_seconds()).fold(start, f64::max);
    let text = join_words(words);
    let within_limits =
        end - start <= CUE_MAX_DURATION_SECONDS && text.chars().count() <= CUE_MAX_CHARS;
    if words.len() == 1 || within_limits {
        out.push(Segment {
            text,
            start_seconds: start,
            duration_seconds: (end - start).max(CUE_MIN_DURATION_SECONDS),
        });
        return;
    }
    let k = best_split(words, silences);
    split_to_cues(&words[..k], out, silences);
    split_to_cues(&words[k..], out, silences);
}

/// 选择最佳切分点：静音边界优先，其次词间间隔最大处，同分时取靠近中间的位置
/// （避免切出只有一两个词的碎条）。
fn best_split(words: &[Segment], silences: &[Silence]) -> usize {
    let mid = words.len() / 2;
    let mut best = (f64::NEG_INFINITY, mid.clamp(1, words.len().saturating_sub(1).max(1)));
    for k in 1..words.len() {
        let a = words[k - 1].end_seconds();
        let b = words[k].start_seconds;
        // 纯字符数（不含拼接空格）：<10 的碎尾（"disturb"/"with your"）不切。
        let left: usize = words[..k].iter().map(|w| w.text.chars().count()).sum();
        let right: usize = words[k..].iter().map(|w| w.text.chars().count()).sum();
        if left < CUE_MIN_SPLIT_PART_CHARS || right < CUE_MIN_SPLIT_PART_CHARS {
            continue;
        }
        let score = pause_score(a, b, silences) * 5.0 + (b - a)
            - 0.02 * ((k as f64) - (mid as f64)).abs();
        if score > best.0 {
            best = (score, k);
        }
    }
    best.1
}

/// 把若干词拼成一行文本（中日文之间不加空格）。
fn join_words(words: &[Segment]) -> String {
    let mut s = String::new();
    for w in words {
        let t = w.text.trim();
        if t.is_empty() {
            continue;
        }
        if !s.is_empty() {
            let prev = s.chars().last().unwrap_or(' ');
            let next = t.chars().next().unwrap_or(' ');
            if !(is_cjk(prev) && is_cjk(next)) {
                s.push(' ');
            }
        }
        s.push_str(t);
    }
    s
}

#[cfg(test)]
mod cue_tests {
    use super::*;

    fn word(text: &str, start: f64, dur: f64) -> Segment {
        Segment { text: text.to_string(), start_seconds: start, duration_seconds: dur }
    }

    #[test]
    fn merges_adjacent_english_words() {
        let words = vec![
            word("Let's", 78.78, 0.12),
            word("do", 79.02, 0.06),
            word("some", 79.20, 0.09),
            word("liquor", 79.44, 0.18),
            word("science", 80.13, 0.21),
        ];
        let cues = merge_into_cues(words, &[]);
        assert_eq!(cues.len(), 1, "同一句话的词应合并为一条: {cues:?}");
        assert_eq!(cues[0].text, "Let's do some liquor science");
        assert!((cues[0].duration_seconds - (80.34 - 78.78)).abs() < 0.01);
    }

    #[test]
    fn breaks_at_detected_speech_pause() {
        // 词间间隔只有 0.30s（同一句内也会这么大），但附近检出 0.5s 静音 → 断句。
        let words = vec![word("sign", 0.0, 0.5), word("that", 0.8, 0.5)];
        let silences = vec![Silence { start: 0.55, end: 1.05 }];
        assert_eq!(merge_into_cues(words.clone(), &silences).len(), 2, "静音处必须断句");

        // 同一份词，没有静音信息时不应断（间隔 0.30 < 1.0 兜底阈值）。
        assert_eq!(merge_into_cues(words, &[]).len(), 1);
    }

    #[test]
    fn pause_detection_tolerates_timing_drift() {
        // 词时间戳相对音频有漂移：静音在 5.0~5.4，词边界却在 5.6/5.9（偏差 0.3s 内）→ 仍应断句。
        let words = vec![word("there", 5.1, 0.5), word("friend", 5.9, 0.3)];
        let silences = vec![Silence { start: 5.25, end: 5.65 }];
        assert_eq!(merge_into_cues(words.clone(), &silences).len(), 2, "容差内的停顿应识别");

        // 静音太短（<0.4s）不算停顿。
        let silences = vec![Silence { start: 5.3, end: 5.45 }];
        assert_eq!(merge_into_cues(words, &silences).len(), 1, "过短静音不算停顿");
    }

    #[test]
    fn breaks_on_long_silence_without_silence_data() {
        let words = vec![word("science", 80.13, 0.21), word("Tell", 98.82, 0.09)];
        let cues = merge_into_cues(words, &[]);
        assert_eq!(cues.len(), 2, "18 秒间隔处必须断句: {cues:?}");
    }

    #[test]
    fn breaks_on_sentence_end_and_when_too_long() {
        let words = vec![word("done.", 0.0, 0.5), word("next", 0.5, 0.5)];
        assert_eq!(merge_into_cues(words, &[]).len(), 2, "句末标点后应断开");

        // 连续 20 秒无停顿的连读：必须在内部断开，且每条不超时长上限。
        let long: Vec<_> = (0..20).map(|i| word("word", i as f64, 1.0)).collect();
        let cues = merge_into_cues(long, &[]);
        assert!(cues.len() > 1, "过长连读必须拆分");
        for c in &cues {
            assert!(c.duration_seconds <= CUE_MAX_DURATION_SECONDS + 0.001, "单条时长超限: {c:?}");
        }
    }

    #[test]
    fn splits_long_run_at_largest_internal_gap() {
        // 一句 9.5s 连读：最大间隔在 4.0s 处（0.5s，未达兜底断句阈值但大于其他 0 间隔）
        // → 应切在那里，而不是按字符数在中间硬切。
        let mut words = Vec::new();
        for i in 0..4 {
            words.push(word("aa", i as f64, 1.0));
        }
        words.push(word("bb", 4.5, 1.0));
        for i in 0..4 {
            words.push(word("cc", 5.5 + i as f64, 1.0));
        }
        let cues = merge_into_cues(words, &[]);
        assert!(cues.len() >= 2, "过长连读必须拆分: {cues:?}");
        assert!(!cues[0].text.contains("bb"), "应在最大间隔处切分，实际首条: {:?}", cues[0]);
        assert!((cues[0].end_seconds() - 4.0).abs() < 1e-6, "切点应在 4.0s，实际 {:?}", cues[0]);
    }

    #[test]
    fn merges_overlapping_or_zero_length_fragments() {
        let words = vec![
            word("Ah", 11.68, 0.72),
            word("ah", 12.96, 0.08),
            word("ah", 13.04, 0.08),
            word("ah", 13.04, 0.08), // 与前一条重叠 0.08s
        ];
        let cues = merge_into_cues(words, &[]);
        assert_eq!(cues.len(), 1, "轻微重叠不应被拆成第二条字幕: {cues:?}");
        assert_eq!(cues[0].text, "Ah ah ah ah");
    }

    /// 切片边界的词戳漂移：同词两份拷贝重叠 >0.35s 去重；不同词的大重叠不再断句。
    #[test]
    fn boundary_overlap_dedups_and_does_not_split() {
        // "not" 与 "disturb" 分属相邻重叠切片，两片对 disturb 给了不同区间：
        let words = vec![
            word("not", 110.16, 0.24),
            word("disturb", 110.40, 0.56), // chunk A margin 版
            word("disturb", 110.05, 0.60), // chunk B 版（与上一份重叠、同文本→丢）
        ];
        let cues = merge_into_cues(words, &[]);
        assert_eq!(cues.len(), 1, "边界重复词不应断句: {cues:?}");
        assert_eq!(cues[0].text, "not disturb");
        // 不同词的大重叠（aligner 漂移）也不断句：
        let words2 = vec![word("not", 110.16, 0.90), word("disturb", 110.40, 0.56)];
        let cues2 = merge_into_cues(words2, &[]);
        assert_eq!(cues2.len(), 1, "词戳重叠不应断句: {cues2:?}");
        assert_eq!(cues2[0].text, "not disturb");
    }

    #[test]
    fn cjk_words_join_without_space() {
        let words = vec![word("我们", 0.0, 2.5), word("去", 2.5, 2.5), word("公园", 5.0, 2.5)];
        let cues = merge_into_cues(words, &[]);
        for c in &cues {
            assert!(!c.text.contains(' '), "中文相邻词之间不应有空格: {cues:?}");
        }
        assert_eq!(cues.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(""), "我们去公园");
    }

    #[test]
    fn keeps_chunk_level_segments_intact() {
        // 无 aligner 时的 chunk 级结果：单条 30s，不应被进一步合并。
        let chunks = vec![word("one chunk of text", 0.0, 30.0), word("another chunk", 30.0, 30.0)];
        let cues = merge_into_cues(chunks, &[]);
        assert_eq!(cues.len(), 2);
    }

    #[test]
    fn coalesces_flash_cues() {
        // 停顿判定在习语内部断开时会切出碎条；这里应回并为通顺字幕。
        let words = vec![
            word("It's", 0.0, 0.3),
            word("not", 0.3, 0.3),
            word("important", 0.6, 0.6),   // end 1.2，随后是真停顿
            word("you", 1.68, 0.21),       // 碎条（0.21s）
            word("just", 1.89, 0.3),
            word("destroyed", 2.19, 0.6),
            word("my", 2.79, 0.3),
            word("distillery", 3.09, 0.42),
        ];
        let silences = vec![Silence { start: 1.35, end: 1.85 }];
        let cues = merge_into_cues(words, &silences);
        assert_eq!(cues.len(), 2, "碎条应回并: {cues:?}");
        assert_eq!(cues[0].text, "It's not important");
        assert_eq!(cues[1].text, "you just destroyed my distillery");
    }

    #[test]
    fn drops_isolated_hallucination_flashes() {
        // 音乐/噪声段：模型偶尔吐出一个 0.08s 的孤立 token，前后都很久没有别的内容。
        let words = vec![
            word("Hello", 0.0, 1.0),
            word("world", 1.0, 1.0),
            word("Hmm", 30.0, 0.08),
            word("Bye", 60.0, 1.0),
            word("now", 61.0, 1.0),
        ];
        let cues = merge_into_cues(words, &[]);
        let texts: Vec<&str> = cues.iter().map(|c| c.text.as_str()).collect();
        assert!(!texts.iter().any(|t| t.contains("Hmm")), "孤立幻觉应被丢弃: {texts:?}");
        assert_eq!(cues.len(), 2, "{texts:?}");
    }

    #[test]
    fn keeps_short_but_legitimate_cues() {
        // "Yes." 只有 4 个字符，但持续 1 秒 → 是正常字幕，不应被回并。
        let words = vec![word("Yes.", 0.0, 1.0), word("No.", 2.0, 1.0)];
        let cues = merge_into_cues(words, &[]);
        assert_eq!(cues.len(), 2, "短但正常时长的字幕不应回并: {cues:?}");
    }

    #[test]
    fn clamps_stretched_lone_word() {
        // ForcedAligner/整片级时间戳把孤词拉满整段：22.7s 的「おい」应按文本长度钳回。
        let cues = merge_into_cues(vec![word("おい", 114.0, 22.7)], &[]);
        assert_eq!(cues.len(), 1);
        assert!(cues[0].duration_seconds <= 2.0, "孤词时长应被钳制: {cues:?}");
        assert!(cues[0].duration_seconds >= SPARSE_FLOOR_SECONDS, "{cues:?}");
    }

    #[test]
    fn clamp_leaves_dense_segments_alone() {
        // 正常密度段不触发钳制（整片级 15s/21字 = 1.4 字/秒）。
        let dense = "これはとても長いセリフで十分な密度の文章よ";
        let cues = merge_into_cues(vec![word(dense, 0.0, 15.0)], &[]);
        assert_eq!(cues.len(), 1);
        assert!((cues[0].duration_seconds - 15.0).abs() < 0.01, "正常密度段不应被钳: {cues:?}");

        // 未达触发时长的短段也不动。
        let cues2 = merge_into_cues(vec![word("て", 0.0, 3.9)], &[]);
        assert!((cues2[0].duration_seconds - 3.9).abs() < 0.01, "{cues2:?}");
    }

    #[test]
    fn render_srt_separates_cues_with_blank_line() {
        let out = render_srt(&[word("one", 0.0, 1.0), word("two", 2.0, 1.0)]);
        assert!(out.contains("one\n\n2\n"), "字幕条之间必须有空行: {out}");
        assert!(out.ends_with("\n\n"), "文件应以空行结尾: {out}");
    }

    /// 重叠切片去重：keep 内的词直接收；margin 词在邻片有副本时丢、没有时补收。
    #[test]
    fn collect_kept_words_dedup_and_rescue() {
        // 两片共享 forced 边界 20s：chunk0 audio [0,21] keep [0,20]，
        // chunk1 audio [19,41] keep [20,40]。
        let k0 = (0.0, 20.0);
        let k1 = (20.0, 40.0);
        let items = vec![
            // chunk0 内正常词。
            (word("hello", 18.0, 0.4), k0.0, k0.1),
            // 边界词 "world"：chunk0 margin 收一份（中心 20.2 > 20），chunk1 keep 收一份。
            (word("world", 20.1, 0.4), k0.0, k0.1),   // margin：应被丢（邻片有副本）
            (word("world", 20.05, 0.5), k1.0, k1.1), // keep：收
            // 只有 chunk0 识别到的 margin 词（chunk1 漏了）：应被补收。
            (word("again", 20.8, 0.3), k0.0, k0.1),
        ];
        let out = collect_kept_words(items);
        let texts: Vec<&str> = out.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["hello", "world", "again"], "{texts:?}");
    }

    #[test]
    fn srt_path_per_backend() {
        assert_eq!(
            srt_path_for(Path::new("/tmp/video.mkv"), "mac"),
            Path::new("/tmp/video-mac.srt")
        );
        assert_eq!(
            srt_path_for(Path::new("/tmp/video.mkv"), "qwen3"),
            Path::new("/tmp/video-qwen3.srt")
        );
    }
}
