# AI-SubTrans

[English](./README.md) | **中文**

<p align="center">
  <img src="docs/卡通Q版鹦鹉吉祥物-512.png" width="160" alt="AI-SubTrans 吉祥物">
</p>

> 你的私人 AI 字幕小助手：打开视频 → 取出字幕 → 一键翻译 → 合并回视频，全都在一个 App 里完成。

[![Windows 10+](https://img.shields.io/badge/Windows-10+-0078D6)](https://www.microsoft.com/windows)
[![macOS 11+](https://img.shields.io/badge/macOS-11+-000000)](https://www.apple.com/macos)
[![Version](https://img.shields.io/badge/version-1.0.5-green)](https://github.com/terry2010/polly-subtitle-translator/releases)
[![License](https://img.shields.io/badge/License-Apache--2.0-blue)](./LICENSE)

## 目录

- [它适合谁](#它适合谁)
- [三步完成字幕](#三步完成字幕)
- [下载安装](#下载安装)
- [零成本试用](#零成本试用)
- [主要功能](#主要功能)
- [快速开始](#快速开始)
- [翻译服务说明](#翻译服务说明)
- [隐私与安全](#隐私与安全)
- [常见问题](#常见问题)
- [给开发者](#给开发者)
- [许可证](#许可证)
- [致谢](#致谢)

---

## 它适合谁

- 看美剧、电影、生肉教程时，没有或找不到满意的中文字幕
- 想边学外语边对照原文与译文
- 下载的外挂字幕需要翻译、校轴、双语导出
- 想把字幕直接合并回视频，方便在电视或网盘播放
- 字幕组或翻译志愿者，需要批量处理、保证人名译名一致

## 三步完成字幕

1. 打开视频或字幕文件
2. 取出字幕并一键翻译
3. 校对后导出，或合并回视频

> 截图占位：后续补充主界面 / 翻译中 / 播放预览 / 右键菜单 2-4 张截图。

---

## 下载安装

前往 [Releases](https://github.com/terry2010/polly-subtitle-translator/releases) 下载对应平台安装包：

| 平台 | 安装包 |
|---|---|
| Windows 10+ | `AI-SubTrans_1.0.5_x64-setup.exe` |
| macOS 11+（Apple Silicon） | `AI-SubTrans_1.0.5_aarch64.dmg` |
| macOS 11+（Intel） | `AI-SubTrans_1.0.5_x64.dmg` |

安装后启动即可，软件会自动检查更新。

> **macOS 提示「无法验证开发者」**：本项目未购买苹果开发者证书，首次打开请右键点击应用 →「打开」，或在「系统设置 → 隐私与安全性」中点击「仍要打开」。

> **Windows SmartScreen 提示**：点击「更多信息 → 仍要运行」即可。安装包已用 Ed25519 密钥签名，应用内自动更新会校验签名。

> **macOS 版本说明**：macOS 安装包已提供，但当前主要测试环境为 Windows，右键菜单等系统集成项暂不可用，部分细节仍在完善中，欢迎反馈。

> **为何会有这种提示**： 因为作者没钱买证书 QAQ 
### 零成本试用

- **完全免费**：Groq、Ollama、LM Studio（本地运行，不花钱）
- **有免费额度**：智谱 GLM-4.7-Flash、硅基流动、混元、文心一言、Gemini 等
- 其他服务需前往对应官网申请 API Key

---

## 主要功能

### 从视频里取字幕
- 支持 mkv、mp4、avi、mov、wmv、flv、ts、m2ts 等常见格式
- 自动列出视频内所有字幕，优先选英文 SDH / 普通英文
- 自动跳过 PGS/VOBSUB 等图形字幕，避免取出一堆乱码
- 首次使用 FFmpeg 会自动下载，无需手动配置

### 一键翻译
- 12 家传统翻译：百度、Bing、Google、DeepL、有道、彩云小译、小牛、腾讯、火山、阿里、Amazon
- 16+ AI 大模型：OpenAI、Azure OpenAI、DeepSeek、智谱 GLM、硅基流动、Groq、通义千问、豆包、混元、零一万物、Kimi、文心一言、Gemini，以及本地 Ollama / LM Studio / 自定义端点
- 自动限速、失败重试、翻译缓存，避免重复花钱
- 人名精译（开发者模式）：自动扫描角色名并统一译名，避免前后不一致
- 实时显示翻译速度和剩余时间，可随时取消

### 字幕编辑器
- 表格化原文/译文并排，支持增删行、查找替换、撤销重做
- 自动检测双语字幕并拆成两栏
- 整体时间轴偏移，方便校轴
- 导出 srt / ass / vtt，支持单语、双语、ASS 样式预览
- 虚拟滚动，上万条字幕也能流畅滚动

### 边看边校对
- 内置 libmpv 播放器，支持硬解、倍速、音量、音轨切换
- 字幕不遮挡画面，下方随播放进度高亮，方便逐条校对
- HDR / 杜比视界视频会自动提示

### 没有内嵌字幕？在线搜索
- 接入 OpenSubtitles、SubHD、字幕库三个来源
- OpenSubtitles 需自备 API Key，SubHD / 字幕库无需 Key
- 自动简化文件名，提高搜索命中率
- 下载到的非中文可继续翻译

### 合并回视频
- 翻译完成后，一键合并字幕到视频
- 默认在原视频上直接合并（需保证足够磁盘空间），也可另存为 `*.merged.mkv`
- 不重新编码，速度秒级

### 更多贴心功能
- **自动更新**：启动后静默检查，有新版本弹窗提醒，确认后自动下载安装
- **Windows 右键菜单**：视频右键「AI-SubTrans 快速翻译」、字幕右键「AI-SubTrans 编辑字幕」、文件夹右键「AI-SubTrans 批量翻译」
- **中英双语界面**、**浅色/深色/跟随系统**三种主题
- **翻译代理**：可单独为翻译请求配置 HTTP/SOCKS5 代理（Google 等需翻墙的服务）
- **崩溃自诊断**：异常退出会写本地崩溃日志，设置页可查看和清理

---

## 快速开始

### 场景一：给外语视频配中文字幕

1. 启动 AI-SubTrans，点击「打开视频」或直接把视频拖进窗口。
2. 首次使用会提示下载 FFmpeg，完成后重新打开视频。
3. 在左侧选择一条字幕流，点击「提取字幕」。
4. 右侧选择「翻译」标签，挑选翻译引擎和语言（先去「设置 → 翻译」填好 API Key），点击「开始翻译」。
5. 在表格里校对译文，可调整时间轴、查找替换、增删行。
6. 点击「播放」按钮，边看视频边核对字幕。
7. 点击「导出」保存字幕，或「合并到视频」生成带字幕的视频。

### 场景二：翻译已有字幕文件

1. 把 `.srt` / `.ass` / `.vtt` 文件拖进窗口，或点击「打开字幕」。
2. 自动识别双语字幕并拆成原文/译文两栏。
3. 选择翻译引擎，点击翻译。
4. 校对后导出即可。

### 场景三：Windows 右键一键处理

1. 先在「设置 → 翻译」中配置好翻译服务。
2. 安装完成后，在文件资源管理器右键视频文件 →「AI-SubTrans 快速翻译」。
3. 后台自动完成「取字幕 → 翻译 → 合并」。
4. 完成后右下角弹出系统通知，点击即可回到主界面查看。

> 也可右键字幕文件 →「AI-SubTrans 编辑字幕」直接进入编辑器；右键文件夹 →「AI-SubTrans 批量翻译」添加到批量处理。

---

## 翻译服务说明

AI-SubTrans 支持 12 家传统翻译 + 16+ AI 大模型，**使用前需在「设置 → 翻译」里填入对应服务的 API Key**（部分服务有免费额度，见上方[零成本试用](#零成本试用)）：

- **传统翻译**：百度、Bing、Google、DeepL、有道、彩云小译、小牛、腾讯、火山、阿里、Amazon
- **AI 大模型**（OpenAI 兼容接口）：OpenAI、Azure OpenAI、DeepSeek、智谱 GLM、硅基流动、Groq、通义千问、豆包、混元、零一万物、Kimi、文心一言、Gemini、Ollama（本地）、LM Studio（本地）、自定义 OpenAI 兼容端点

AI 服务支持多实例配置（同一服务可配多套 API Key / BaseUrl）。各服务的申请方式见其官网。

---

## 隐私与安全

- **不上传你的文件**：视频和字幕都在本地处理，翻译时只把字幕文本发送给你配置的翻译服务。
- **敏感信息本地存储**：API Key、代理密码等保存在本地数据库，不上传任何服务器。
- **自动更新只传版本号**：检查更新时只请求版本清单，不发送任何用户数据。
- **崩溃日志本地保存**：崩溃信息只写本地磁盘，不会自动上传，需你手动提交给开发者。
- **组件按需下载**：FFmpeg 和 libmpv 首次使用时从 GitHub 下载（国内有 gh-proxy 加速），可随时在设置页删除重下。

---

## 常见问题

**Q：打开视频提示要下载 FFmpeg？**
A：FFmpeg 是取字幕和合视频的基础工具，首次使用会自动下载。网络不好时，可前往设置页手动切换下载源或重试。

**Q：翻译时提示「余额不足」怎么办？**
A：这说明你使用的翻译服务账号额度已用完，需要去对应官网充值。软件不会自动重试浪费你的额度。

**Q：Google / Gemini 等翻译连不上？**
A：这些服务在中国大陆可能需要代理。在「设置 → 高级」中配置 HTTP/SOCKS5 代理，并勾选「翻译请求走代理」。

**Q：翻译后人名前后不一致？**
A：在「设置 → 关于」连点版本号 7 次开启开发者模式，再打开「人名精译」开关，翻译前会自动扫描并统一角色名。

**Q：合并后视频在哪？**
A：默认在原视频文件上直接合并（不生成新文件），因此需要保证原视频所在磁盘有足够空间。如果空间不足，会提示你另存为 `*.merged.mkv`。

**Q：macOS 打不开怎么办？**
A：因未购买苹果开发者证书，首次右键应用 →「打开」即可；或在「系统设置 → 隐私与安全性」中点击「仍要打开」。

---

## 反馈与帮助

- 提交 Bug 或建议：[GitHub Issues](https://github.com/terry2010/polly-subtitle-translator/issues)
- 查看更新日志：[Releases](https://github.com/terry2010/polly-subtitle-translator/releases)
- 应用内「设置 → 关于」可查看当前版本、手动检查更新、管理崩溃日志。

---

## 给开发者

本项目基于 Tauri 2 + React 18 + Rust 构建。如需从源码构建：

```bash
# 前置：Node.js 18+、Rust 1.89+、系统依赖见 Tauri 官方文档
npm install
npm run start          # 开发模式
npm run build:release  # 生产构建
```

详细的架构设计、IPC 接口、模块说明等开发者文档见 [docs/开发文档-开发者版.md](./docs/开发文档-开发者版.md)（面向开发者，非普通用户文档）。

---

## 许可证

AI-SubTrans 使用 [Apache-2.0](./LICENSE) 协议开源。

- 主程序：Apache-2.0
- libmpv：LGPL-2.1+（运行时动态加载）
- FFmpeg：GPL-2.0+（独立子进程调用，不传染主程序）

---

## 致谢

- [Tauri](https://tauri.app) — 跨平台桌面应用框架
- [libmpv](https://mpv.io) / [mpv-winbuild](https://github.com/zhongfly/mpv-winbuild) / [libmpv-darwin-build](https://github.com/media-kit/libmpv-darwin-build) — 视频播放
- [FFmpeg](https://ffmpeg.org) / [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) — 字幕提取与合并
- [shadcn/ui](https://ui.shadcn.com) / [Radix UI](https://www.radix-ui.com) / [Tailwind CSS](https://tailwindcss.com) — UI 组件与样式
- [OpenSubtitles](https://www.opensubtitles.org) / SubHD / 字幕库 — 在线字幕来源
