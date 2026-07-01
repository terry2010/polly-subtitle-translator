# 二期竞品全景分析

> 本文档调研字幕翻译赛道的全部主要竞品，分析其对 AI-SubTrans 二期规划的影响。
> 更新日期：2026-07-01

---

## 目录

- [一、竞品全景概览](#一竞品全景概览)
- [二、第一梯队：巨头级（10K+ stars）](#二第一梯队巨头级10k-stars)
  - [pyVideoTrans（18K stars）](#pyvideotrans18k-stars)
  - [VideoLingo（17K stars）](#videolingo17k-stars)
  - [VideoCaptioner / 卡卡字幕助手（13.9K stars）](#videocaptioner--卡卡字幕助手139k-stars)
- [三、第二梯队：中坚级（1K+ stars）](#三第二梯队中坚级1k-stars)
  - [SmartSub / 妙幕（4K stars）](#smartsub--妙幕4k-stars)
  - [VoiceTransl / 灵译（1.2K stars）](#voicetransl--灵译12k-stars)
- [四、第三梯队：小众 / 商业](#四第三梯队小众--商业)
  - [GeekLink / 极客连](#geeklink--极客连)
  - [译幕 / Zimoo](#译幕--zimoo)
  - [V2sub / AI 字幕机](#v2sub--ai-字幕机)
  - [Subtitle Translator](#subtitle-translator)
- [五、全竞品功能对比矩阵](#五全竞品功能对比矩阵)
- [六、市场表现对比](#六市场表现对比)
- [七、唯一没人做的功能](#七唯一没人做的功能)
- [八、对 AI-SubTrans 的影响与应对](#八对-ai-subtrans-的影响与应对)
- [九、关键结论](#九关键结论)

---

## 一、竞品全景概览

```
第一梯队（10K+ stars，功能全面，快速迭代）：
  pyVideoTrans    18K stars   视频翻译+配音全流程王者
  VideoLingo      17K stars   Netflix 级翻译质量王者
  VideoCaptioner  13.9K stars  免费+LLM 断句王者

第二梯队（1K+ stars，各有特色）：
  SmartSub         4K stars   跨平台+多引擎（Tauri 技术栈，同本项目）
  VoiceTransl    1.2K stars   离线+下载+合成

第三梯队（小众/商业）：
  GeekLink        失败        闭源付费 Mac only，已验证失败
  译幕/Zimoo      活跃        纯字幕文件翻译，BYOK
  V2sub           商业闭源    ASR+翻译+配音+实时播放器
  Subtitle Translator 小众    批量字幕翻译在线工具

头部 3 个项目合计：49K stars
```

**这个赛道不是蓝海，是红海中的红海。**

---

## 二、第一梯队：巨头级（10K+ stars）

### pyVideoTrans（18K stars）

| 项 | 内容 |
| --- | --- |
| GitHub | https://github.com/jianchang512/pyvideotrans |
| 官网 | https://pyvideotrans.com |
| Stars | **18,019** |
| Forks | 2,242 |
| Releases | 83 个（极活跃） |
| 最新版本 | v4.02（2026-06-11） |
| 技术栈 | Python |
| 平台 | Windows / macOS / Linux |
| 开源 | ✅ GPL-3.0 |
| 价格 | 完全免费 |

**定位**：开源视频翻译 / 语音转录 / AI 配音 / 字幕翻译工具

**核心功能**：
- 全自动视频翻译：ASR → 字幕翻译 → TTS → 视频合成，一键完成
- 语音转录：批量音视频转 SRT，支持说话人分离
- 多角色 AI 配音：不同说话人分配不同 AI 配音
- 声音克隆：F5-TTS / CosyVoice / GPT-SoVITS 零样本克隆
- ASR 引擎：Faster-Whisper / WhisperX / 阿里 Qwen / 字节火山 / Azure / Google
- LLM 翻译：DeepSeek / ChatGPT / Claude / Gemini / MiniMax / Ollama / 阿里百炼
- TTS 引擎：Edge-TTS（免费）/ OpenAI / Azure / ChatTTS / ChatterBox
- 交互式编辑：每个阶段可暂停人工校对
- 工具集：人声分离、视频/字幕合并、音画对齐、文稿匹配
- CLI 模式：无头运行，支持服务器部署和批处理

**市场表现**：
- 18K stars，83 个 release，30 个 contributors
- 活跃维护，2026-06 仍在更新（支持 RTX 50 系列）
- 有独立 BBS 问答站
- 防骗声明（有人淘宝卖它的打包版），侧面证明知名度高

**与 AI-SubTrans 的关系**：全面碾压。ASR、翻译引擎数、配音、合成、批量、CLI、跨平台全部覆盖。

---

### VideoLingo（17K stars）

| 项 | 内容 |
| --- | --- |
| GitHub | https://github.com/Huanshere/VideoLingo |
| Stars | **17,000** |
| 技术栈 | Python（Streamlit UI） |
| 平台 | Windows / macOS / Linux |
| 开源 | ✅ Apache-2.0 |
| 最新版本 | v3.0.1（2026-02-28） |

**定位**：Netflix 级字幕切割、翻译、对齐、配音，一键全自动 AI 字幕组

**核心功能**：
- WhisperX 单词级时间轴识别 + 低幻觉
- NLP + AI 字幕分割（按句意，不是固定字数）
- 自定义 + AI 生成术语库，保证翻译连贯性
- **三步翻译：直译 → 反思 → 意译**，影视级翻译质量
- Netflix 标准单行长度检查，绝无双行字幕
- 配音：GPT-SoVITS / Azure / OpenAI / edge-TTS / fish-tts / custom-tts
- YouTube 链接下载（yt-dlp）
- Streamlit 网页 UI，一键出片
- 任务控制：随时暂停、继续、停止

**市场表现**：
- 17K stars，极活跃
- 有多个一键整合包衍生项目（VideoLingo-OneClick 等）
- OSCHINA 有专页

**与 AI-SubTrans 的关系**：翻译质量碾压（三步翻译法），字幕分割/术语库/配音全面覆盖。

---

### VideoCaptioner / 卡卡字幕助手（13.9K stars）

| 项 | 内容 |
| --- | --- |
| GitHub | https://github.com/WEIFENG2333/VideoCaptioner |
| 官网 | https://www.videocaptioner.cn |
| Stars | **13,891** |
| Forks | 1,132 |
| 技术栈 | Python（98.6%） |
| 最新版本 | v1.4.1 |
| 开源 | ✅ GPL-3.0 |

**定位**：基于 LLM 的智能字幕助手，语音识别、字幕优化、翻译、视频合成全流程

**核心功能**：
- 语音转录：faster-whisper / whisper-api / **bijian（必剪，免费无需 API Key）** / jianying（剪映，免费） / whisper-cpp
- 字幕翻译：LLM / **bing（免费）** / **google（免费）**
- LLM 语义理解断句，自然流畅
- 上下文感知翻译，支持反思优化机制
- 批量并发处理
- 字幕烧录（软字幕/硬字幕）
- CLI + GUI 双模式
- **免费功能无需任何配置，安装即用**

**市场表现**：
- 13.9K stars，1.1K forks
- 有多个 fork 版本（VideoCaptioner_bix 等）
- pip 安装，极低门槛

**与 AI-SubTrans 的关系**：免费零配置门槛极低，LLM 断句+翻译质量高，CLI+GUI 双模式覆盖全场景。

---

## 三、第二梯队：中坚级（1K+ stars）

### SmartSub / 妙幕（4K stars）

| 项 | 内容 |
| --- | --- |
| GitHub | https://github.com/buxuku/SmartSub |
| 官网 | https://smartsub.linxiaodong.com/ |
| Stars | **3,957** |
| Forks | 269 |
| Contributors | 11 |
| 技术栈 | **Tauri + TypeScript**（与 AI-SubTrans 相同） |
| 平台 | Windows / macOS（Apple + Intel）/ Linux |
| 开源 | ✅ MIT |
| 最新版本 | v2.16.0（2026-06-10） |
| 安装 | Homebrew / GitHub Releases / 夸克网盘 |

**定位**：本地媒体创作工作台，一站式「转字幕 → 翻译 → 校对 → 合成」

**核心功能**：
- 6 大 ASR 模型（Whisper 系列 + FunASR）
- **17 个翻译服务**：百度/谷歌/阿里云/火山引擎/豆包/小牛/腾讯/讯飞/DeepLX/Azure/Ollama/DeepSeek/Azure OpenAI/DeerAPI/Gemini/SiliconFlow/通义千问 + 任意 OpenAI 风格 API
- GPU 加速：CUDA / Metal / Vulkan 全覆盖
- 批量处理，文件夹递归扫描
- 独立字幕校对界面（SRT/VTT/ASS/LRC/TXT）
- 视频字幕软合并 + 硬烧录
- 双语字幕
- 自定义翻译参数（界面配置请求头/请求体）
- Homebrew 一行安装

**市场表现**：
- 4K stars，活跃开发
- V2EX 有真实用户反馈帖
- 单版本下载 ~1800+（Mac arm64 DMG）
- 用户反馈："m4 mini 全本地模型，2 小时影片只需 10 分钟"

**与 AI-SubTrans 的关系**：同技术栈（Tauri），功能几乎完全覆盖 AI-SubTrans 二期规划。17 翻译引擎 vs 3 个，6 ASR vs 0 个，跨平台 vs Windows only。

---

### VoiceTransl / 灵译（1.2K stars）

| 项 | 内容 |
| --- | --- |
| GitHub | https://github.com/shinnpuru/VoiceTransl |
| 官网 | http://voicetransl.shinnpuru.online/ |
| Stars | **1,160** |
| 技术栈 | Python（基于 GalTransl） |
| 平台 | Windows / macOS |
| 开源 | ✅ GPL-3.0 |

**定位**：一站式离线 AI 视频字幕生成和翻译软件

**核心功能**：
- YouTube / Bilibili 直接下载视频
- 音频提取 + 听写打轴 + 字幕翻译 + 视频合成 + 字幕总结
- 多种 ASR 模型 + 多种翻译模型（OpenAI 兼容接口 / LlamaCpp）
- VAD 语音活动检测
- 人声分离（人声和伴奏分离）
- 五种模式：下载/翻译/听写/完整/工具
- 批量处理，自动识别文件类型

**与 AI-SubTrans 的关系**：功能重叠度高，且有 YouTube/B站下载、人声分离等 AI-SubTrans 完全没有的能力。

---

## 四、第三梯队：小众 / 商业

### GeekLink / 极客连

| 项 | 内容 |
| --- | --- |
| 官网 | https://geeklink.dev/ |
| 平台 | 仅 macOS 13.0+，仅 Apple Silicon |
| 开源 | ❌ 闭源 |
| 价格 | 免费带署名 / Pro $12.99/月、$99/年、$169 终身 |

**功能**：Whisper 语音转写 + RapidOCR 硬字幕提取 + AI 翻译（Claude/GPT-4o/DeepSeek）+ 批量 + Watch 文件夹 + 字幕烧录 + 智能断行 + 复查标记

**市场表现**：**失败**。上线约一年，GitHub 下载量个位数，discussions 仅作者自问自答，几乎无用户反馈。

**失败原因**：闭源 + 付费 + 仅 Apple Silicon + 无营销 + 免费版带署名。同赛道 SmartSub 开源免费跨平台，GeekLink 无竞争力。

**与 AI-SubTrans 的关系**：可忽略。它自己都没跑出来。

---

### 译幕 / Zimoo

| 项 | 内容 |
| --- | --- |
| GitHub | https://github.com/1c7/Translate-Subtitle-File |
| 官网 | https://zimoo.app/ |
| 最新版本 | v5.5.8（2026-06-20） |
| 平台 | 网页版 + 桌面版 |

**定位**：纯字幕文件翻译，支持 .srt .ass .vtt

**核心卖点**：**BYOK（Bring Your Own Key）**——用户自己填 API key，价格最低。

**与 AI-SubTrans 的关系**：在"纯字幕文件翻译"这个细分点上直接竞争。译幕专注做精，支持网页版（零安装），且活跃更新。

---

### V2sub / AI 字幕机

| 项 | 内容 |
| --- | --- |
| 官网 | https://www.zimuji.com/ |
| 平台 | Windows / macOS |
| 开源 | ❌ 闭源商业 |
| 价格 | 付费制（强化版/豪华版） |

**功能**：ASR + 翻译（ChatGPT/DeepL/谷歌/微软）+ 配音合成 + 视频合成 + **实时翻译播放器**（业内首创，加载视频几秒出双语字幕）

**与 AI-SubTrans 的关系**：商业闭源，定位不同，但有"实时翻译播放器"这个独特功能。

---

### Subtitle Translator

| 项 | 内容 |
| --- | --- |
| 官网 | https://tools.newzone.top/zh/subtitle-translator |
| 类型 | 在线工具 |

**功能**：批量字幕翻译，支持 .srt/.ass/.vtt，35 种语言，多种翻译接口（API + AI 大模型）

**与 AI-SubTrans 的关系**：在线工具，无需安装，在"快速翻译一个字幕文件"场景下比桌面 app 便捷。

---

## 五、全竞品功能对比矩阵

| 功能 | pyVideoTrans | VideoLingo | VideoCaptioner | SmartSub | VoiceTransl | GeekLink | 译幕 | V2sub | AI-SubTrans |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Stars** | 18K | 17K | 13.9K | 4K | 1.2K | 失败 | 活跃 | 商业 | 未发布 |
| 语音转写 | ✅ 多ASR | ✅ WhisperX | ✅ 5引擎 | ✅ 6模型 | ✅ | ✅ Whisper | ❌ | ✅ | ❌ |
| 字幕翻译 | ✅ 多LLM | ✅ GPT三步 | ✅ LLM+免费 | ✅ 17引擎 | ✅ | ✅ 4引擎 | ✅ BYOK | ✅ 4引擎 | ✅ 3引擎 |
| 批量处理 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| 字幕编辑 | ✅ | ⚠️ | ✅ | ✅ 校对 | ⚠️ | ✅ | ⚠️ | ⚠️ | ✅ |
| 视频合成 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | ✅ 软合并 |
| AI 配音 | ✅ 多角色 | ✅ GPT-SoVITS | ❌ | ❌ | ✅ | ❌ | ❌ | ✅ | ❌ |
| 声音克隆 | ✅ F5-TTS | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| 在线搜索字幕 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| 内嵌流提取 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| 播放预览 | ⚠️ | ⚠️ | ⚠️ | ✅ | ✅ | ⚠️ | ❌ | ✅ 实时 | ✅ libmpv |
| 跨平台 | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ Mac | ✅ 网页 | ✅ | ❌ Win |
| GPU 加速 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ CoreML | ❌ | ✅ | ❌ |
| 开源 | ✅ GPL | ✅ Apache | ✅ GPL | ✅ MIT | ✅ GPL | ❌ | ✅ | ❌ | ✅ GPL |
| 免费 | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ 付费 | ✅ BYOK | ❌ 付费 | ✅ |
| YouTube/B站下载 | ❌ | ✅ | ❌ | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ |
| CLI 模式 | ✅ | ⚠️ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Watch 文件夹 | ⚠️ | ❌ | ❌ | ⚠️ 扫描 | ❌ | ✅ | ❌ | ❌ | ❌ |
| OCR 硬字幕 | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ | ❌ |

---

## 六、市场表现对比

```
                Stars    活跃度    市场验证
                ─────    ──────    ─────────
pyVideoTrans    18K      83 releases  ✅ 王者
VideoLingo      17K      活跃         ✅ 王者
VideoCaptioner  13.9K    活跃         ✅ 王者
SmartSub         4K      活跃         ✅ 成功
VoiceTransl    1.2K      活跃         ✅ 小众成功
译幕/Zimoo       ?       活跃         ⚠️ 细分存活
V2sub           商业      活跃         ⚠️ 商业存活
GeekLink         ~0      低           ❌ 失败
AI-SubTrans     未发布    —            ❓ 未验证
```

**头部 3 个项目合计 49K stars。** 这个赛道需求已被充分验证，但也已被充分占领。

---

## 七、唯一没人做的功能

纵观全部 8 个竞品，**只有两个功能是所有竞品都不做的**：

### 1. 在线字幕搜索（OpenSubtitles / SubHD / zimuku）

**为什么没人做**：
- 用户习惯直接去字幕站网站找字幕
- Infuse / VLC 内置 OpenSubtitles 下载
- Bazarr 在 Jellyfin/Emby 里自动下载字幕
- "在翻译工具里搜索字幕"不是高频需求——找到字幕后还需要翻译的场景，比"找字幕"本身小得多

**诚实评估**：有价值但不是决定性因素，用户不会因为"能搜字幕"而选择一个翻译工具。

### 2. 内嵌字幕流提取（从 mkv 提取已有字幕流）

**为什么没人做**：
- `ffmpeg -i video.mkv -map 0:s:0 subtitle.srt` 一行命令搞定
- mkvtoolnix 也能提取
- 太基础，不值得做成独立功能

**诚实评估**：不构成差异化壁垒。

### 其他"没人做"的功能

| 功能 | 没人做的可能原因 |
| --- | --- |
| Jellyfin/Emby 插件 | 需求 = "需要翻译" × "有 NAS" × "愿意配置插件"，三层叠加极小众；Bazarr 已占"自动下载字幕"位 |
| NAS/Docker 服务化 | pyVideoTrans 有 CLI 模式部分覆盖；SmartSub 是桌面 app 不做；需求层小众 |
| Apple TV/Infuse 链路 | 流程太长（Mac 处理→写回 NAS→Apple TV 播放），愿意走的人极少 |
| libmpv 播放预览 | 锦上添花，不是选择工具的决定性因素 |

---

## 八、对 AI-SubTrans 的影响与应对

### 竞品格局

```
头部三巨头（49K stars）：
  pyVideoTrans   18K  → 视频翻译+配音全流程，83 releases
  VideoLingo     17K  → Netflix 级翻译质量，三步翻译法
  VideoCaptioner 13.9K → 免费零配置，LLM 断句，CLI+GUI

中坚（5K stars）：
  SmartSub        4K  → 同技术栈 Tauri，17 引擎，6 ASR，跨平台
  VoiceTransl   1.2K  → 离线+下载+合成

细分存活：
  译幕/Zimoo          → 纯字幕文件翻译，BYOK
  V2sub               → 商业，实时翻译播放器

失败：
  GeekLink            → 闭源付费 Mac only

AI-SubTrans          → 未发布，3 引擎，Win only，无 ASR
```

### AI-SubTrans 的功能被覆盖情况

| AI-SubTrans 功能 | 覆盖该功能的竞品数 | 差异化空间 |
| --- | --- | --- |
| 字幕翻译（3 引擎） | 8 个竞品全有 | ❌ 引擎数远落后（最多 17 个） |
| 字幕编辑 | 5+ 个竞品有 | ❌ 不构成差异 |
| 软合并回 mkv | 5+ 个竞品有 | ❌ 不构成差异 |
| 播放预览 | 3+ 个竞品有 | ⚠️ 锦上添花 |
| 在线字幕搜索 | 0 个竞品 | ⚠️ 没人做但可能没人需要 |
| 内嵌流提取 | 0 个竞品 | ⚠️ ffmpeg 一行命令可替代 |
| 批量处理（二期） | 8 个竞品全有 | ❌ 追不上 |
| 语音转写（二期） | 6 个竞品有 | ❌ 追不上 |
| 多引擎（二期） | 8 个竞品全有 | ❌ 追不上 |

### 残酷现实

```
AI-SubTrans 能做的，竞品全做了。
AI-SubTrans 二期想做的，竞品也全做了。
AI-SubTrans 唯一独有的（在线搜索、流提取），可能没人需要。

这个赛道头部 49K stars，还在快速迭代。
AI-SubTrans 作为未发布的后来者，没有明显差异化空间。
```

### 可能的应对方向

| 方向 | 可行性 | 说明 |
| --- | --- | --- |
| 放弃产品化，保留为个人工具/学习项目 | ⭐⭐⭐⭐ | 技术栈质量高，自用够用 |
| 给 SmartSub/pyVideoTrans 贡献代码 | ⭐⭐⭐⭐ | 比从零做竞品影响力大 |
| 转向 Jellyfin/Emby 插件细分 | ⭐⭐ | 没人做但需求可能极小众 |
| 正面竞争 | ⭐ | 没有赢面 |

---

## 九、关键结论

### 结论一：需求真实且已被充分验证

```
49K stars（头部三项目）= 需求真实存在
但需求已被充分满足 = 赛道已红海化
```

### 结论二：AI-SubTrans 没有差异化空间

```
所有核心功能（翻译/编辑/合并/批量/ASR）都有 4-8 个竞品覆盖
唯一独有的功能（在线搜索/流提取）可能没人需要
```

### 结论三：GeekLink 失败不是方向问题，是策略问题

```
GeekLink：闭源 + 付费 + 仅 Apple Silicon → 失败
SmartSub：开源 + 免费 + 跨平台 → 4K stars 成功
pyVideoTrans：开源 + 免费 + 跨平台 + 全功能 → 18K stars 王者

开源免费跨平台是必要条件，但 AI-SubTrans 即使满足也追不上头部
```

### 结论四：二期规划需要重新审视

```
原二期规划核心功能（批量/ASR/多引擎）全部被竞品做到极致
差异化方向（在线搜索/Jellyfin/NAS/Apple TV）可能没人需要

建议：
  1. 认真考虑是否继续作为独立产品开发
  2. 如果继续，聚焦竞品完全不做的极细分领域（Jellyfin 插件）
  3. 如果不继续，保留为个人工具或给竞品贡献代码
```

---

> 相关文档：
> - 方案分析详见 `docs/phase2-strategy-analysis.md`
> - 功能优先级详见 `docs/phase2-roadmap.md`
