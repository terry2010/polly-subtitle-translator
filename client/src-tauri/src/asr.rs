// 音频转字幕（ASR）模块
//
// 分层：
// - audio：音轨提取为 16kHz 单声道 WAV + 自适应静音检测 + 停顿对齐切片（重叠 margin + 词归属去重）
// - cues：逐词/逐段片段合并为可读字幕条（停顿断句、CJK 拼接、闪现丢弃）
// - engine：识别后端（macOS 原生 Speech.framework / Qwen3-ASR Python 侧边）
// - env：Qwen3 托管 Python 环境（python-build-standalone + pip + 模型下载，按需懒装）

pub mod audio;
pub mod cues;
pub mod engine;
pub mod env;
pub mod pipeline;
pub mod qwen3;
