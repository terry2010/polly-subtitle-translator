# AI-SubTrans

> 跨平台桌面 AI 字幕提取、翻译与编辑工具（Windows + macOS）。
> 围绕「提取 → 翻译 → 编辑 → 预览 → 合并」提供一站式能力，同时支持独立字幕文件的处理。

[![Tauri](https://img.shields.io/badge/Tauri-2-blue)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-1.89+-orange)](https://www.rust-lang.org)
[![React](https://img.shields.io/badge/React-18-61dafb)](https://react.dev)
[![License](https://img.shields.io/badge/License-GPL--2.0+-blue)](./LICENSE)

---

## 目录

- [项目简介](#项目简介)
- [核心功能](#核心功能)
- [技术栈](#技术栈)
- [项目结构](#项目结构)
- [快速开始](#快速开始)
- [开发指南](#开发指南)
- [配置说明](#配置说明)
- [使用流程](#使用流程)
- [架构设计](#架构设计)
- [实现状态](#实现状态)
- [自动更新与发布](#自动更新与发布)
- [许可证](#许可证)

---

## 项目简介

AI-SubTrans（内部代号 `zimufan`）是一款基于 **Tauri 2 + React 18 + Rust** 构建的轻量级跨平台桌面字幕工具，支持 **Windows 10+** 与 **macOS 11+**（Universal Binary：Apple Silicon + Intel）。它把视频字幕的提取、机器翻译、表格化编辑、播放预览与软合并回视频整合到一个应用中，覆盖从片源到成品字幕的完整链路。

应用支持两种使用场景：

- **交互模式**：主界面打开/拖入视频或字幕文件，进入完整的提取 → 翻译 → 编辑 → 预览 → 合并流程。
- **静默模式**：通过系统右键菜单对视频或字幕文件一键触发「提取翻译合并」或「编辑翻译」，处理完成后通过系统通知告知用户。
  - Windows：写注册表 `HKCU\Software\Classes\SystemFileAssociations\<ext>\shell\zimufan`。
  - macOS：右键菜单暂不支持（无注册表机制），通过主界面或拖入文件使用。

应用内置自动更新功能，发布新版本后用户启动客户端即可收到升级提示，确认后自动下载安装。

详细需求规格见 [`docs/需求文档.md`](./docs/需求文档.md)。

---

## 核心功能

### 字幕提取

- 从 mkv / mp4 / avi / mov 视频中提取内嵌软字幕流。
- 默认输出 srt，原流为 ass/vtt 时可选保留原格式。
- 自动识别图形字幕流（hdmv_pgs_subtitle / dvd_subtitle / dvb_subtitle）并禁用提取按钮。
- 默认字幕选择规则：优先英文 SDH（title 含 SDH/HI/CC）→ 普通英文 → 任意字幕流兜底。
- 字幕流编辑器：拖拽排序、删除、改名、导出单条流。

### 多引擎翻译

- 支持 **12 家翻译服务**，分为两类：
  - **传统机器翻译**（8 家）：百度 / Bing / Google / DeepL / 有道 / 彩云小译 / 小牛翻译 / 腾讯 / 火山 / 阿里 / Amazon（按已配置引擎渲染对应按钮）。
  - **AI 大模型翻译**（OpenAI 兼容，含 16+ 服务实例）：OpenAI / Azure OpenAI / DeepSeek / 智谱GLM / 硅基流动 / Groq / 通义千问 / 豆包 / 混元 / 零一万物 / Kimi / 文心一言 / Gemini / Ollama（本地） / LM Studio（本地） / 自定义端点。AI 服务支持多实例配置（同一服务可配多个 API Key/BaseUrl）。
- **限流策略**（`RateLimitPolicy`）：各引擎按官方政策配置 QPS 或并发上限，避免触发 429。
  - QPS 模式：请求间强制间隔 1/N 秒，并发上限 1（串行 + 间隔）。
  - Concurrency 模式：最多 N 个并发请求，无间隔要求。
- **人名精译**（开发者模式开关）：翻译前预扫描字幕提取人名，生成译名表供用户确认，翻译时传入 glossary 保证人名翻译一致。
- **余额不足检测**：统一检测 HTTP 402 与响应体关键词（中英文），命中时返回 `TranslateInsufficientBalance` 错误，前端提示充值而非重试。
- 占位符保护算法：翻译前用 Unicode 私用区字符替换 ass 样式标记 `{\\...}`、HTML 标签、换行符，翻译后回填，避免标记被翻译破坏。
- 翻译分段：按字幕条数分段，按 API 单位（百度按字节、Google/Bing 按字符、AI 按 token）累计，单条超限按句号二次切分。
- 翻译缓存：相同原文 + 源语言 + 目标语言 + provider + service_id + model 的结果缓存到 SQLite（缓存 key 用 `escape_field` 防注入），避免重复计费。
- 失败重试：单条失败指数退避重试 3 次（1s/2s/4s），仍失败保留原文并标记 `failed` 布尔字段（前端以样式区分，未在译文文本中插入 `[翻译失败]` 标记）。
- 凭据通过系统密钥环（keyring）存储，不写入数据库。
- 支持单条翻译：在字幕预览区右键单条字幕可单独翻译。
- **翻译统计**：实时显示字符数、EMA 速度（字符/秒）、ETA（剩余时间），用指数移动平均避免速度反复跳。

### 字幕编辑

- 表格化编辑器，支持原文 / 译文编辑、增删行（时间码编辑、复制行暂未实现）。
- 虚拟滚动（@tanstack/react-virtual）支撑万级条数流畅滚动。
- 时间轴偏移（手动输入毫秒数批量偏移；快捷键 ±0.1s / ±1s 步进暂未实现）、查找替换、撤销重做（栈深 50）。
- 双语字幕自动检测：按 Unicode 范围分类语言，检测条目内「按语言分块」模式，阈值 60% 判定双语并拆分原文/译文。
- 导出对话框：支持 srt / ass / vtt 多格式，ASS 样式实时预览，双语/单语配置。

### 播放预览

- 内嵌 **libmpv**（按需从 GitHub 下载，动态加载），以原生悬浮窗方式叠加在主窗口播放区上方：
  - **Windows**：WS_POPUP 子窗口，通过 SetWinEventHook 同步位置跟随主窗口移动/缩放，D3D11/GPU 渲染器 + DXVA2/D3D11VA 硬件解码。
  - **macOS**：NSWindow 悬浮窗（NSView 子视图），通过 NSNotificationCenter 监听主窗口移动，vo_cocoa 渲染。按 arch 分支下载（arm64 / x86_64）。
- 字幕不叠加到视频画面，改为下方字幕对比预览区随播放进度滚动高亮对应条。
- 对比预览模式：原文 / 译文 / 双语三种显示模式。
- 播放控制：播放/暂停、进度拖动、音量调节、静音、倍速（0.5/0.75/1/1.25/1.5/2 六档）、音轨切换。
- HDR / Dolby Vision 视频检测并主动提示用户（HdrNotice 组件）。
- DPI 缩放：使用 Tauri scaleFactor 同步 libmpv 悬浮窗坐标。
- **空格键播放/暂停**：后端原生窗口捕获 + 前端 WebView keydown 双路径互补，焦点在输入框时放行。
- **防并发初始化**：`player_init` 用 initLock 防止 HMR/StrictMode 双调用，组件卸载用 cancelledRef 中止进行中的初始化。

### 合并回视频

- 在导出对话框中点击「合并到视频」，将字幕软合并回视频，生成同目录 `<videoname>.merged.mkv` 新文件（固定输出 mkv 容器，不保留原容器扩展名）。
- 合并为手动触发（导出对话框按钮），不采用"翻译后自动合并 checkbox"方案；固定输出 `.merged.mkv`，重复合并会覆盖。

### 在线搜索下载

- 接入三个搜索来源，前端可在搜索弹窗中切换：
  - **OpenSubtitles** 官方 REST API（用户自注册 API Key，凭据存 keyring）。
  - **SubHD**（`subhd.tv`，无需 Key，HTML 爬虫，使用 `scraper` crate 解析）。
  - **zimuku**（字幕库，无需 Key，HTML 爬虫）。
- 按文件名关键词搜索字幕，显示评分与下载次数，用户选择下载。
- **关键词简化**：`simplify_search_keyword` 命令自动剥离文件名中的画质/编码/组名等噪声（如 `The.Big.Bang.Theory.S12E24.1080p.WEB-DL.x264-RARBG` → `The Big Bang Theory S12E24`），提升搜索命中率。
- **验证码流程**：SubHD / zimuku 触发验证码时后端返回 `search.captchaRequired` 错误（含验证码图片 URL 和 session cookie），前端展示验证码图片与输入框，用户输入后调用 `search_subtitles_with_captcha` 续搜。
- **下载行为差异**：
  - OpenSubtitles：应用内直接下载到本地。
  - SubHD / zimuku：在系统浏览器中打开详情页由用户手动下载（站点下载链路复杂且多变，故采用半自动方式）。
- 下载的字幕若非目标语言可继续翻译。

### 组件按需下载

- **FFmpeg**：首次使用时按平台/arch 从 GitHub 下载（优先 gh-proxy 加速）：
  - Windows：BtbN/FFmpeg-Builds（`ffmpeg-master-latest-win64-gpl.zip`），fallback 到 gyan.dev。
  - macOS arm64：eugeneware/ffmpeg-static（`ffmpeg-darwin-arm64.gz`）。
  - macOS x86_64：eugeneware/ffmpeg-static（`ffmpeg-darwin-x64.gz`）。
  - Linux：BtbN/FFmpeg-Builds（`ffmpeg-master-latest-linux64-gpl.tar.xz`）。
- **libmpv**：首次播放时按平台/arch 从 GitHub 下载：
  - Windows：zhongfly/mpv-winbuild（GPL 构建，含 D3D11/GPU 渲染器和 DXVA2/D3D11VA 硬件解码），`mpv-dev-x86_64-*.7z`。
  - macOS arm64：media-kit/libmpv-darwin-build（`libmpv-libs_v0.7.2_macos-arm64-video-full.tar.gz`）。
  - macOS x86_64：media-kit/libmpv-darwin-build（`libmpv-libs_v0.7.2_macos-amd64-video-full.tar.gz`）。
- 下载进度显示实时速度（MB/s）和剩余时间（ETA）。
- 设置页可查看安装状态、手动下载或删除。

### 崩溃日志与诊断

- **panic hook**：捕获 Rust panic，写入 `<app_data_dir>/crashes/crash_YYYY-MM-DD_HH-MM-SS.log`（含调用栈）。
- **Windows 原生异常捕获**：同时安装 Vectored Exception Handler（VEH）和 Unhandled Exception Filter（UEF），捕获内存访问违规、栈溢出等原生异常（不会触发 panic hook），写入崩溃日志 + minidump（供 WinDbg/VS 分析调用栈）。VEH 只处理致命异常码，放行 C++ 异常等正常控制流，避免无限循环。
- **prompt 失败日志**：翻译对齐失败时记录 system/user prompt 与模型返回内容到 `<app_data_dir>/prompt_fails/`，供调试。
- **API 调试日志**：开发者模式开启"全量记录翻译数据"后，记录 API 请求/响应到 `<app_data_dir>/api_debug/`。
- 设置页可查看和清理崩溃日志、prompt 失败日志、API 调试日志。

### 开发者模式

- 开启方式：关于页面点击版本号 7 下。
- 关闭方式：再点击 7 下，或重启 10 次后自动关闭。
- 功能：
  - 打开/关闭 DevTools（release 构建也生效）。
  - 前端 logger 仅在开发者模式输出到 console。
  - "全量记录翻译数据"开关：开启后记录 API 请求/响应到 `api_debug/` 目录。
  - "人名精译"开关：开启后翻译前预扫描人名生成译名表（退出开发模式后仍保持启用）。

### 自动更新

- 基于 Tauri Updater 插件，启动后自动检查新版本。
- 发现新版本时弹窗显示版本号和更新内容，用户确认后自动下载安装。
- 下载过程显示进度/速度/ETA，签名验证后静默安装（不弹 SmartScreen）。
- 安装完成后提示重启应用。
- 设置页「关于」分区可手动检查更新。

### 多入口触发

- **系统右键菜单**：视频文件右键「AI-SubTrans：提取翻译字幕」，字幕文件右键「AI-SubTrans：编辑/翻译字幕」。
  - Windows：写注册表 `HKCU\Software\Classes\SystemFileAssociations\<ext>\shell\zimufan`（用户级，无需提权）。
  - macOS：暂不支持（无注册表机制）。
- **主界面**：打开按钮 / 拖入文件。
- **单实例转发**：第二个实例启动时解析 argv 并转发文件路径到主窗口。

---

## 技术栈

### 后端（Rust / Tauri 2）

| 领域 | 选型 |
| --- | --- |
| 应用框架 | Tauri 2 |
| 异步运行时 | tokio（full features） |
| 数据库 | rusqlite 0.32（bundled SQLite） |
| HTTP 客户端 | reqwest 0.12（default-tls / native-tls，Windows 用 SChannel） |
| 凭据存储 | keyring 3（系统密钥环：Windows Credential Manager / macOS Keychain） |
| 字幕解析 | srt/vtt 自写解析器；ass 使用 ass-core + ass-editor |
| 编码探测 | chardetng + encoding_rs |
| 日志 | tracing + tracing-subscriber + tracing-appender（按天滚动，保留 7 天） |
| 错误处理 | thiserror + anyhow，自定义 AppError / IpcError（含 severity 分级） |
| 哈希 / 签名 | sha2 / md-5 / hex / hmac / sha1（云厂商 HMAC 签名） |
| 解压 | sevenz-rust（Windows libmpv 7z）+ zip（Windows FFmpeg zip）+ flate2 + tar（macOS tar.gz） |
| HTML 解析 | scraper 0.22（SubHD / zimuku 搜索结果爬取） |
| 自动更新 | tauri-plugin-updater 2（Ed25519 签名验证） |
| Windows API | windows 0.61 + winreg 0.55 + libloading 0.8 |
| macOS API | objc 0.2（NSWindow/NSView/NSWorkspace）+ libloading 0.8 |

### 前端（React 18 / TypeScript / Vite 6）

| 领域 | 选型 |
| --- | --- |
| UI 框架 | React 18 + React Router 6 |
| 状态管理 | Zustand 4（含 persist 中间件） |
| 组件库 | shadcn/ui（源码拷贝方式）+ Radix UI 原语 |
| 样式 | Tailwind CSS 4 + tw-animate-css |
| 表格 / 虚拟滚动 | @tanstack/react-table + @tanstack/react-virtual |
| 拖拽 | react-dropzone + @dnd-kit/sortable（字幕流排序） |
| 图标 | lucide-react |
| 国际化 | i18next + react-i18next（中/英双语，576 个 key） |
| 通知 | sonner |
| 前端日志 | 自写 logger（仅开发者模式输出 console，避免 release 泄露调试信息） |
| Tauri 插件 | dialog / fs / shell / os / notification / process / updater / single-instance |

---

## 项目结构

```
ai-subtrans/
├── docs/
│   ├── mac-adaptation-plan.md         # macOS 平台适配方案
│   ├── translate-settings-redesign.md # 翻译设置 UI 重构设计
│   ├── phase2-competitor-analysis.md  # Phase 2 竞品分析
│   ├── phase2-roadmap.md              # Phase 2 路线图
│   ├── phase2-strategy-analysis.md    # Phase 2 策略分析
│   ├── videocaptioner-deep-analysis.md # 竞品 Videocaptioner 分析
│   ├── 代码签名调研报告.md            # macOS/Windows 代码签名调研
│   ├── 本地语音识别调研报告.md        # 本地语音识别技术调研
│   ├── bob-text-translation-providers.md # Bob 翻译 provider 参考
│   └── website-tech-stack-and-monetization-research.md # 官网技术栈调研
├── scripts/
│   ├── publish.mjs                    # 发布脚本（改版本号+构建+GitHub Release+latest.json）
│   └── gen-favicon.cjs                # 网站 favicon 生成脚本
├── src/                               # 前端 React 源码
│   ├── main.tsx                       # React 入口
│   ├── App.tsx                        # 根组件：路由、主题、CLI 参数监听、拖放、自动更新检查
│   ├── views/
│   │   ├── MainView.tsx               # 主界面（视频打开、字幕提取、翻译、合并）
│   │   └── SettingsView.tsx           # 设置页（6 个分区）
│   ├── components/
│   │   ├── VideoPlayer.tsx            # libmpv 悬浮窗播放器（Windows WS_POPUP / macOS NSWindow）
│   │   ├── SubtitleListPanel.tsx      # 字幕表格编辑器（虚拟滚动）
│   │   ├── SubtitlePreviewPanel.tsx   # 字幕对比预览（播放联动高亮、右键单条翻译）
│   │   ├── TranslatePanel.tsx         # 翻译控制面板
│   │   ├── ExportDialog.tsx           # 导出对话框（格式选择、ASS 样式预览）
│   │   ├── SearchDialog.tsx           # OpenSubtitles 搜索弹窗
│   │   ├── SubtitleStreamEditorDialog.tsx # 字幕流编辑器（拖拽排序、删除、导出）
│   │   ├── GlossaryConfirmDialog.tsx  # 译名表确认弹窗（人名精译）
│   │   ├── FfmpegDownloadDialog.tsx   # FFmpeg 下载对话框
│   │   ├── UpdateDialog.tsx           # 应用更新对话框
│   │   ├── HdrNotice.tsx              # HDR/Dolby 提示
│   │   ├── AutoTextarea.tsx           # 自适应高度文本框
│   │   └── ui/                        # shadcn/ui 组件
│   ├── stores/                        # Zustand stores
│   │   ├── subtitleStore.ts           # 字幕状态 + 撤销重做 + 查找替换
│   │   ├── videoStore.ts              # 视频探测结果 + 字幕流自动选择
│   │   ├── translateStore.ts          # 翻译进度 + 结果
│   │   ├── themeStore.ts              # 主题 + 语言（持久化）
│   │   ├── libmpvStore.ts             # libmpv 下载状态
│   │   ├── ffmpegStore.ts             # FFmpeg 下载状态
│   │   ├── updateStore.ts             # 自动更新状态
│   │   └── devModeStore.ts            # 开发者模式
│   ├── lib/
│   │   ├── api.ts                     # IPC 调用封装（90+ 方法）
│   │   ├── ipc-types.ts               # 与 Rust 后端对应的 TS 类型
│   │   ├── services.ts                # 翻译服务注册表（27 个 provider 元信息）
│   │   ├── logger.ts                  # 前端日志（开发者模式门控）
│   │   ├── i18n.ts                    # i18next 初始化
│   │   └── utils.ts                   # cn / 时间 / 字节格式化 / 路径处理
│   ├── locales/
│   │   ├── zh.json                    # 中文翻译（576 key）
│   │   └── en.json                    # 英文翻译（514 key）
│   └── styles/globals.css             # Tailwind + 主题变量
├── src-tauri/                         # Rust 后端源码
│   ├── Cargo.toml
│   ├── tauri.conf.json                # Tauri 配置 + updater 配置
│   ├── capabilities/default.json      # Tauri 权限声明
│   ├── build.rs
│   └── src/
│       ├── main.rs                    # 入口（调用 lib::run）
│       ├── lib.rs                     # 应用初始化、日志、崩溃捕获、CLI 解析、单实例
│       ├── ipc.rs                     # 95 个 Tauri 命令 handler
│       ├── translate.rs               # 翻译模块（12 provider + OpenAI 兼容 + 限流 + 人名精译）
│       ├── subtitle.rs                # 字幕解析（srt/vtt/ass）+ 双语检测
│       ├── ffmpeg.rs                  # FFmpeg 封装（probe/提取/合并/HDR 检测/跨平台按需下载）
│       ├── player.rs                  # libmpv 下载/加载/悬浮窗叠加/播放控制（Windows + macOS）
│       ├── search.rs                  # OpenSubtitles API 客户端
│       ├── db.rs                      # SQLite 表结构 + 迁移 + CRUD
│       ├── config.rs                  # 配置管理 + 凭据存储
│       ├── context_menu.rs            # Windows 右键菜单注册（macOS/Linux stub）
│       └── error.rs                   # AppError + IpcError（severity 分级）
├── package.json
├── vite.config.ts
├── tsconfig.json
├── index.html
└── AGENTS.md                          # AI 助手开发规范
```

---

## 快速开始

### 环境要求

- **Rust** ≥ 1.89（含 cargo）
- **Node.js** ≥ 18（含 npm）
- **Windows 10 2004+** 或 **macOS 11+**（Universal Binary：Apple Silicon + Intel）
- Windows 端需 WebView2 Runtime（Win11 自带，Win10 由安装包 bootstrapper 按需安装）
- macOS 端需 Xcode Command Line Tools（编译 objc FFI）
- FFmpeg / libmpv：无需预装，应用首次使用时按平台/arch 自动从 GitHub 下载（国内通过 gh-proxy 加速）

### 安装依赖

```bash
# 前端依赖
npm install

# Rust 依赖会在首次构建时自动拉取
```

### 开发模式运行

```bash
# 标准方式（使用项目根目录 target）
npm run tauri dev

# Windows 本机作者环境专用（指定 cargo 路径与独立 target 目录，避免锁冲突）
npm run start
```

开发模式下前端运行在 `http://localhost:5173`，Tauri 窗口加载该地址并热重载。

### 构建发布包

```bash
# Windows NSIS 安装包
npm run build:nsis

# macOS DMG 安装包
npm run build:dmg

# macOS .app 包（不打包 DMG，调试用）
npm run build:app

# 构建 + 发布（含签名，需配置环境变量）
npm run publish 1.0.1 "更新内容"
```

产物路径：
- Windows：`C:\Users\<用户名>\.cargo-target\zimufan\release\bundle\nsis\`
  - `AI-SubTrans_<版本>_x64-setup.exe` + `.sig`
- macOS：`target/release/bundle/dmg/`
  - `AI-SubTrans_<版本>_universal.dmg` + `.sig`

### 仅构建前端

```bash
npm run build    # tsc -b && vite build
npm run preview  # 预览构建产物
```

### 测试

```bash
# 前端测试（Vitest）
npm test                    # 运行全部前端测试
npm run test:watch          # 监听模式
npm run test:coverage       # 含覆盖率
npm run test:macos          # 仅运行 macOS 平台测试
npm run test:windows        # 仅运行 Windows 平台测试

# Rust 后端测试（cargo test）
npm run test:rust:macos     # macOS
npm run test:rust:windows   # Windows（作者环境专用路径）
```

测试覆盖：
- 前端：264 个测试（24 个测试文件），覆盖组件、stores、lib 工具函数。
- 后端：219 个单元测试，覆盖 translate（provider/限流/缓存 key/占位符）、subtitle（解析/双语检测）、ffmpeg（磁盘空间/HDR 检测）、ipc（locale 归一化）、player（libmpv 状态）、error（错误码/severity）等模块。

---

## 开发指南

### IPC 命令开发

后端命令定义在 `src-tauri/src/ipc.rs`，通过 `tauri::generate_handler!` 宏注册。新增命令步骤：

1. 在 `ipc.rs` 中添加 `#[tauri::command]` 函数。
2. 将函数名加入 `get_invoke_handlers()` 的 `generate_handler!` 列表。
3. 在 `src/lib/ipc-types.ts` 中添加对应的 TS 类型。
4. 在 `src/lib/api.ts` 中添加封装方法。
5. 错误统一返回 `IpcResult<T>`，通过 `error::ipc_result` 包装。

### 错误处理约定

所有 IPC 失败统一返回 `IpcError`：

```json
{
  "code": "error.<域>.<code>",
  "args": {},
  "severity": "recoverable | restart | reinstall"
}
```

- `recoverable`：用户可恢复（如翻译失败可重试），前端用 toast 提示。
- `restart`：需重启应用（如 SQLite 损坏），前端用模态对话框提示。
- `reinstall`：需重装或重配环境（如 libmpv 下载损坏），前端引导重装。

错误码定义见 `src-tauri/src/error.rs`。

### 国际化

- 翻译文件：`src/locales/zh.json` / `src/locales/en.json`。
- 所有 UI 文本应使用 `t('key')` 包裹，不硬编码。
- key 命名规范：`<模块>.<子项>`，如 `settings.api.baidu.appId`。

### 文件写入规范

参见 `AGENTS.md`：超过 200 行的文件必须分段写入（每次 ≤ 150 行），避免 GUI OOM。

### 日志

- 日志目录：`<app_data_dir>/logs/zimufan.log`（按天滚动，保留 7 天）。
- 默认级别 `info`，`zimufan_lib` 模块为 `debug`，可通过 `RUST_LOG` 环境变量覆盖。
- 同时输出到 stderr 与文件。

### 图标生成

应用图标（吉祥物 Polly 鹦鹉）分两步生成：

**1. 软件图标（tauri icon）**

```bash
# 从 1024x1024 源图生成到 src-tauri/icons/
npx tauri icon path/to/icon-1024.png
```

生成：
- `icon.ico`（Windows，含 16/24/32/48/64/256 六尺寸）
- `icon.icns`（macOS）
- `icon.png`（512x512）+ 各尺寸 PNG + iOS/Android 全套

**2. 网站 favicon**

```bash
node scripts/gen-favicon.cjs
```

从 512 源图生成到 `public/`：
- `favicon.ico`（16/32/48 三合一）
- `favicon-{16,32,48,180,192,512}.png`
- `apple-touch-icon.png`（180x180）

**重新构建前清缓存**

`tauri icon` 只更新 `src-tauri/icons/` 里的文件，Cargo 增量编译不会自动重嵌图标。换图标后必须清 debug 目录再构建：

```bash
# Windows
rmdir /s /q C:\Users\<用户名>\.cargo-target\zimufan\debug
npm run start

# macOS
rm -rf target/debug
npm run tauri dev
```

同时建议清一下 Windows 图标缓存（任务栏/桌面显示旧图标时）：

```cmd
taskkill /f /im explorer.exe
del /a /q %localappdata%\IconCache.db
del /a /f /q %localappdata%\Microsoft\Windows\Explorer\iconcache*.db
start explorer.exe
```

---

## 配置说明

### 配置存储

| 数据类型 | 存储位置 |
| --- | --- |
| 非敏感配置（key-value） | SQLite `config` 表 |
| 翻译 API 凭据（appid/secret/key） | 系统密钥环（keyring） |
| OpenSubtitles API Key | 系统密钥环 |
| 翻译缓存 | SQLite `translate_cache` 表 |
| 历史记录 | SQLite `history` 表 |
| 最近文件 | SQLite `recent_files` 表（保留 20 条） |
| API provider 元信息 | SQLite `api_provider` 表 |
| 搜索 provider 元信息 | SQLite `search_provider` 表 |

数据库文件位于 `<app_data_dir>/zimufan.db`。

### 核心 config 表 key 清单

> 以下为代码中实际读写的 key（前端 `api.getConfig/setConfig` + 后端 `db.get_config/set_config`）。
> `config.rs` 中的 `GeneralSettings`/`PlayerSettings` 结构体为预留封装，当前未被主流程使用。

| key | 说明 | 默认值 |
| --- | --- | --- |
| `default_target_lang` | 默认目标翻译语言（ISO 639-1） | 跟随系统语言 |
| `default_target_lang_follow_system` | 是否跟随系统语言 | `true` |
| `default_source_lang` | 默认源语言 | `en` |
| `default_api_provider` | 默认翻译 API（启动时回退用） | `baidu` |
| `translate_provider` | 当前设置的翻译 API | `baidu` |
| `translate_<provider>_app_id` | 各 provider 的 appid/账号（`<provider>` = baidu/bing/google） | — |
| `translate_<provider>_secret` | 各 provider 的 secret/key（baidu/google） | — |
| `translate_<provider>_region` | Bing 翻译的 region | — |
| `translate_concurrency` | 翻译并发数（实际并发 = `min(用户配置, QPS 上限)`） | `3` |
| `dev_mode` | 开发者模式开关 | `false` |
| `dev_mode_restart_count` | 开发者模式启用后的重启计数（用于自动关闭） | `0` |
| `dev_log_api_enabled` | 全量记录翻译数据开关（仅 devMode 开启时生效） | `false` |
| `name_precision_enabled` | 人名精译开关（退出开发模式后仍保持） | `false` |
| `proxy_mode` | 代理模式（`none` / `http` / `socks5`） | `none` |
| `proxy_host` / `proxy_port` / `proxy_user` | 代理主机/端口/用户名 | — |
| `translate_use_proxy` | 翻译请求是否走代理（独立于搜索代理设置） | `false` |
| `theme` | 主题（前端 themeStore 持久化，未走 config 表） | `system` |
| `log_level` | 日志级别（由 `RUST_LOG` 环境变量覆盖） | `info` |

代理密码通过 keyring 存储（provider=`proxy`, key=`pass`），不写入 config 表。
翻译 API 凭据（appid/secret/key/region）也通过 keyring 存储，详见下表。

### 翻译 API 配置

#### 传统机器翻译

| API | 限流策略 | 单次上限 | 单位 | 默认并发 |
| --- | --- | --- | --- | --- |
| 百度翻译 | QPS 1（免费版） | 6000 | 字节 | 3 |
| Bing 翻译 | Concurrency 10 | 5000 | 字符 | 3 |
| Google 翻译 | Concurrency 100 | 5000 | 字符 | 3 |
| DeepL | Concurrency 5 | — | 字符 | 5 |
| 有道翻译 | QPS 1 | — | 字符 | 1 |
| 彩云小译 | Concurrency 5 | — | 字符 | 5 |
| 小牛翻译 | Concurrency 5 | — | 字符 | 5 |
| Amazon 翻译 | Concurrency 10 | — | 字符 | 10 |

#### AI 大模型（OpenAI 兼容）

| 服务 | 默认并发 | 备注 |
| --- | --- | --- |
| OpenAI / Azure OpenAI | 5 | 官方及 Azure 托管 |
| DeepSeek | 10 | 推理强、价格低 |
| 智谱GLM | 10 | GLM-4-Flash 无限免费 |
| 硅基流动 | 10 | 聚合多厂商，部分免费 |
| Groq | 1 | 完全免费，Llama 3 |
| 通义千问 / 豆包 / 混元 / 零一万物 / Kimi / 文心一言 | 5-10 | 国内大模型 |
| Gemini | 5 | OpenAI 兼容端点 |
| Ollama / LM Studio | 5 | 本地运行，完全免费 |
| 自定义端点 | 5 | 任意 OpenAI 兼容服务 |

实际并发 = `min(用户配置并发, 限流策略上限)`。Google 翻译在中国大陆不可直连，需在「高级」配置 HTTP/SOCKS5 代理。翻译请求是否走代理由 `translate_use_proxy` 独立控制（与搜索代理分离）。

### 组件安装路径

| 组件 | Windows | macOS |
| --- | --- | --- |
| FFmpeg | `<app_data_dir>/ffmpeg/ffmpeg.exe` | `<app_data_dir>/ffmpeg/ffmpeg` |
| libmpv | `<app_data_dir>/libmpv/mpv-2.dll` | `<app_data_dir>/libmpv/libmpv.dylib` 等 |

`<app_data_dir>` 跨平台路径：

| 平台 | 路径 |
| --- | --- |
| Windows | `C:\Users\<用户名>\AppData\Roaming\com.zimufan.ai-subtrans\` |
| macOS | `~/Library/Application Support/com.zimufan.ai-subtrans/` |
| Linux | `~/.config/com.zimufan.ai-subtrans/` |

---

## 使用流程

### 交互模式（主界面）

1. **打开视频**：点击「打开视频文件」或拖入 `.mkv/.mp4/.avi/.mov` 文件。
   - 若 FFmpeg 未安装，会弹出下载对话框，下载完成后需重新点击「打开视频」。
2. **选择字幕流**：应用自动按优先级选择字幕流，可在字幕流下拉中手动切换。
3. **提取字幕**：点击「提取字幕」，选择保存位置与格式（默认 srt，ass/vtt 原流可保留格式）。
4. **翻译**：在翻译面板选择目标语言与翻译引擎，点击翻译按钮。翻译进度实时显示，可取消。
5. **编辑**：在字幕表格编辑器中修改原文、译文（时间码编辑暂未实现），支持查找替换、时间轴偏移（手动输入毫秒数）、撤销重做。
6. **预览**：点击播放按钮，libmpv 悬浮窗播放视频，下方字幕区随播放进度滚动高亮。
   - 若 libmpv 未安装，会自动触发下载。
7. **导出/合并**：点击「导出字幕」选择格式（srt/ass/vtt）和双语配置，或「合并到视频」生成软合并新文件。

### 静默模式（右键菜单）

1. 在文件资源管理器中右键视频文件 →「AI-SubTrans：提取翻译字幕」。
2. 应用以 `--mode=quick` 启动，自动执行：提取字幕 → 翻译 → 合并到视频。
3. 处理完成后通过系统通知告知用户，点击通知聚焦主窗口。
4. 字幕文件右键 →「AI-SubTrans：编辑/翻译字幕」以 `--mode=edit` 启动，直接进入编辑器。

### 在线搜索字幕

1. 打开无字幕视频后点击「搜索在线字幕」。
2. 在搜索弹窗中输入文件名（自动填充），后端调用 OpenSubtitles API 搜索。
3. 选择匹配的字幕（按评分/下载次数参考），点击下载。
4. 下载的字幕自动加载到编辑器，可继续翻译或编辑。

---

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────┐
│                  Tauri 2 主进程                      │
│  ┌───────────────┐    ┌───────────────────────────┐ │
│  │  Rust 后端     │    │   WebView (React 前端)     │ │
│  │               │    │                           │ │
│  │  ffmpeg.rs    │◄──►│  views/MainView           │ │
│  │  subtitle.rs  │IPC │  views/SettingsView       │ │
│  │  translate.rs │    │  components/*             │ │
│  │  player.rs    │    │  stores/* (Zustand)        │ │
│  │  search.rs    │    │  lib/api.ts               │ │
│  │  db.rs        │    │                           │ │
│  │  config.rs    │    └───────────────────────────┘ │
│  │  context_menu │                                   │
│  │  updater      │                                   │
│  └───────────────┘                                   │
└─────────────────────────────────────────────────────┘
         │                    │
         ▼                    ▼
   ┌──────────┐        ┌──────────────┐
   │ SQLite   │        │ libmpv 悬浮窗 │
   │ zimufan.db│       │ (按需下载)    │
   └──────────┘        └──────────────┘
         │                    │
         ▼                    ▼
   ┌──────────┐        ┌──────────────┐
   │ keyring  │        │ FFmpeg       │
   │ (凭据)   │        │ (按需下载)    │
   └──────────┘        └──────────────┘
```

### IPC 通信

后端注册了 95 个 Tauri 命令（`src-tauri/src/ipc.rs`），覆盖：

- 视频探测与字幕提取（`probe_video` / `extract_subtitle` / `cancel_extract_subtitle`）
- 字幕解析与编辑（`parse_subtitle_file` / `save_subtitle_file_cmd` / `export_subtitle_cmd` / `detect_bilingual` / `split_bilingual_subtitle` / `edit_subtitle_streams_cmd`）
- 翻译（`translate_subtitle` / `cancel_translate` / `test_translate_connection` / `get_supported_target_langs` / `get_cached_translations` / `clear_translate_cache` / `extract_names` / `list_openai_models`）
- 配置与凭据（`get_config` / `set_config` / `get_all_config` / `save_credential` / `get_credential` / `delete_credential`）
- 播放器控制（`player_init` / `player_load_cmd` / `player_play_cmd` / `player_pause_cmd` / `player_seek_cmd` / `player_set_volume_cmd` / `player_set_speed_cmd` / `player_set_audio_track_cmd` / `player_get_position_cmd` / `player_resize_cmd` / `player_show_cmd` / `player_hide_cmd` / `player_destroy_cmd` / `open_in_system_player_cmd` / `list_installed_players_cmd` / `open_with_player_cmd` / `reveal_in_explorer_cmd` / `extract_player_icons_cmd`）
- 字幕搜索（`search_subtitles_online` / `search_subtitles_with_captcha` / `download_subtitle_online` / `simplify_search_keyword`）
- 合并（`merge_subtitle` / `check_merge_space`）
- 右键菜单（`register_video_menu` / `unregister_video_menu` / `register_subtitle_menu` / `unregister_subtitle_menu` / `is_video_menu_registered` / `is_subtitle_menu_registered`）
- libmpv 管理（`get_libmpv_status_cmd` / `download_libmpv_cmd` / `delete_libmpv_cmd`）
- FFmpeg 管理（`get_ffmpeg_status_cmd` / `download_ffmpeg_cmd` / `delete_ffmpeg_cmd`）
- 自动更新（`check_for_update` / `download_and_install_update`）
- 最近文件与历史（`get_recent_files` / `add_recent_file` / `get_history` / `add_history_record`）
- 代理与系统（`set_proxy` / `get_proxy` / `get_translate_use_proxy` / `set_translate_use_proxy` / `test_proxy` / `get_system_lang` / `get_work_area` / `toggle_devtools`）
- 开发者模式与诊断（`set_dev_mode_cmd` / `set_log_api_enabled_cmd` / `dev_log_cmd` / `set_space_disabled_cmd` / `is_cursor_in_window_cmd` / `get_crash_log_dir_cmd` / `clear_crash_logs_cmd` / `get_prompt_fail_dir_cmd` / `list_prompt_fail_logs_cmd` / `read_prompt_fail_log_cmd` / `delete_prompt_fail_log_cmd` / `clear_prompt_fail_logs_cmd` / `list_api_debug_logs_cmd` / `clear_api_debug_logs_cmd` / `open_path_cmd`）

事件流（后端 → 前端）：

- `cli-args`：单实例转发文件路径。
- `player_position`：播放位置更新（供字幕高亮联动）。
- `libmpv_download_progress`：libmpv 下载进度（含 speed_mbps / eta_secs）。
- `ffmpeg_download_progress`：FFmpeg 下载进度（含 speed_mbps / eta_secs）。
- `extract_progress`：字幕提取进度。
- `translate-progress` / `translate-entry-done`：翻译进度与逐条回调。
- `update_download_progress`：应用更新下载进度（含 speed_mbps / eta_secs）。

### 翻译模块架构

```
translate.rs
├── PlaceholderProtector       # 占位符保护（U+E000~U+E0FF）
├── split_text()               # 分段算法（按字节/字符/token 累计）
├── RateLimitPolicy            # 限流策略（Qps / Concurrency）
├── check_insufficient_balance # 余额不足检测（HTTP 402 + 关键词）
├── escape_field / build_cache_provider_name  # 缓存 key 防注入
├── BaiduProvider              # 百度翻译（MD5 签名）
├── BingProvider               # Bing 翻译（Azure JWT 签名）
├── GoogleProvider             # Google 翻译（API Key）
├── DeepLProvider              # DeepL（Auth Key）
├── YoudaoProvider             # 有道翻译（SHA256 签名）
├── CaiyunProvider             # 彩云小译（Token）
├── NiutransProvider           # 小牛翻译（API Key）
├── TencentProvider            # 腾讯翻译（HMAC-SHA256 签名）
├── VolcengineProvider         # 火山翻译（HMAC-SHA256 签名）
├── AliyunProvider             # 阿里翻译（HMAC-SHA1 签名）
├── AmazonProvider             # Amazon Translate（AWS4 签名）
├── OpenAiProvider             # OpenAI 兼容（支持 16+ 服务实例）
└── translate_with_retry()     # 限流 + 指数退避重试（1s/2s/4s）
```

### 字幕模块架构

```
subtitle.rs
├── SubtitleFormat (Srt/Vtt/Ass/Ssa)
├── SubtitleEntry (index/start_ms/end_ms/text/translated/style)
├── parse_srt() / parse_vtt() / parse_ass()   # 解析
├── save_srt() / save_vtt() / save_ass()       # 回写
├── detect_bilingual()                         # 双语检测（Unicode 范围分类）
└── split_bilingual()                          # 双语拆分
```

### 播放器集成方案

当前实现为**悬浮窗叠加**方案，按平台使用不同原生窗口机制：

- **Windows**：libmpv 以 WS_POPUP 子窗口方式叠加在主窗口播放区上方，通过 SetWinEventHook 监听主窗口移动/缩放事件同步悬浮窗位置，DPI 同步使用 Tauri scaleFactor。libmpv 从 zhongfly/mpv-winbuild GitHub Releases 下载 GPL 构建，含 D3D11/GPU 渲染器和 DXVA2/D3D11VA 硬件解码。
- **macOS**：libmpv 以 NSWindow 悬浮窗（NSView 子视图）方式叠加，通过 NSNotificationCenter 监听主窗口移动事件同步位置，vo_cocoa 渲染。libmpv 从 media-kit/libmpv-darwin-build GitHub Releases 下载，按 arch 分 arm64 / x86_64。

### 组件下载策略

**FFmpeg**（按平台/arch 分支）：
- Windows：gh-proxy 加速 BtbN/FFmpeg-Builds → 直连 BtbN → gyan.dev fallback，下载 `ffmpeg-master-latest-win64-gpl.zip`，用 `zip` crate 解压
- macOS arm64：eugeneware/ffmpeg-static，下载 `ffmpeg-darwin-arm64.gz`，用 `flate2` 解压
- macOS x86_64：eugeneware/ffmpeg-static，下载 `ffmpeg-darwin-x64.gz`
- Linux：BtbN/FFmpeg-Builds，下载 `ffmpeg-master-latest-linux64-gpl.tar.xz`
- 安装到 `<app_data_dir>/ffmpeg/`

**libmpv**（按平台/arch 分支）：
- Windows：zhongfly/mpv-winbuild GitHub Releases（GPL 构建），下载 `mpv-dev-x86_64-*.7z`，用 `sevenz-rust` crate 解压
- macOS arm64：media-kit/libmpv-darwin-build，下载 `libmpv-libs_v0.7.2_macos-arm64-video-full.tar.gz`，用 `flate2` + `tar` 解压
- macOS x86_64：media-kit/libmpv-darwin-build，下载 `libmpv-libs_v0.7.2_macos-amd64-video-full.tar.gz`
- 安装到 `<app_data_dir>/libmpv/`

---

## 实现状态

### 已实现

| 模块 | 状态 | 说明 |
| --- | --- | --- |
| 视频探测 | ✅ | FFmpeg probe，含 HDR/Dolby/图形字幕检测 |
| 字幕提取 | ✅ | srt/vtt/ass 解析与回写 |
| 字幕流编辑 | ✅ | 拖拽排序、删除、改名、导出单条流 |
| 双语检测 | ✅ | Unicode 范围分类 + 60% 阈值判定 |
| 多引擎翻译 | ✅ | 12 家传统 + 16+ AI 大模型（OpenAI 兼容），含签名/分段/缓存/重试/并发控制 |
| 限流策略 | ✅ | RateLimitPolicy（Qps / Concurrency），按引擎官方政策配置 |
| 人名精译 | ✅ | 预扫描人名 → 译名表确认 → 翻译时传入 glossary |
| 余额不足检测 | ✅ | HTTP 402 + 中英文关键词检测，提示充值而非重试 |
| 占位符保护 | ✅ | 私用区字符方案（U+E000~U+E0FF） |
| 单条翻译 | ✅ | 字幕预览区右键单条翻译 |
| 翻译失败标记 | ✅ | `failed` 布尔字段标记单条翻译失败（不插入 `[翻译失败]` 文本） |
| 翻译统计 | ✅ | 实时字符数、EMA 速度、ETA |
| 字幕编辑器 | ⚠️ | 虚拟滚动、增删行、查找替换、时间轴偏移（手动输入）、撤销重做；时间码编辑/复制行/快捷键偏移暂未实现 |
| 导出对话框 | ✅ | srt/ass/vtt 多格式，ASS 样式实时预览，双语配置 |
| 播放预览 | ✅ | libmpv 悬浮窗叠加（Windows WS_POPUP / macOS NSWindow）、播放控制、音轨切换、字幕联动高亮 |
| 空格键播放/暂停 | ✅ | 后端原生窗口捕获 + 前端 keydown 双路径 |
| 对比预览 | ✅ | 原文/译文/双语三种模式 |
| 合并回视频 | ✅ | FFmpeg 软合并（导出对话框手动触发），固定输出 `.merged.mkv`，含磁盘空间预检 |
| 系统播放器降级 | ✅ | 枚举已安装播放器（含图标）、用指定播放器打开、在资源管理器/Finder 定位 |
| OpenSubtitles 搜索 | ✅ | REST API 关键词搜索与下载（不做 moviehash/时长精确匹配） |
| 系统右键菜单 | ✅ | Windows 注册表注册/注销/状态检测（macOS 不支持） |
| 单实例转发 | ✅ | argv 解析与事件转发 |
| 配置与凭据 | ✅ | SQLite + keyring（Windows Credential Manager / macOS Keychain） |
| FFmpeg 按需下载 | ✅ | 跨平台按 arch 下载（Windows/macOS/Linux），多源加速 |
| libmpv 按需下载 | ✅ | 跨平台按 arch 下载（Windows/macOS），进度显示速度/ETA |
| 自动更新 | ✅ | Tauri Updater，启动检查 + 手动检查，签名验证静默安装 |
| 崩溃日志 | ✅ | panic hook + Windows VEH/UEF 原生异常捕获 + minidump |
| prompt 失败日志 | ✅ | 翻译对齐失败时记录 prompt 与模型返回 |
| API 调试日志 | ✅ | 开发者模式开启后记录 API 请求/响应 |
| 错误处理 | ✅ | AppError + IpcError（severity 三级） |
| 国际化 | ✅ | 中/英双语，576 key |
| 主题 | ✅ | 浅色/深色/跟随系统 |
| 日志 | ✅ | tracing 按天滚动，保留 7 天（启动时清理旧日志） |
| 开发者模式 | ✅ | 关于页点击版本号 7 次开启，含 DevTools/全量记录/人名精译开关 |
| macOS 支持 | ✅ | Universal Binary（Apple Silicon + Intel），NSWindow 悬浮窗，Keychain 凭据 |
| 跨平台测试 | ✅ | 前端 264 个测试 + 后端 219 个单元测试 |

### 待完善 / 待实现

| 项 | 优先级 | 说明 |
| --- | --- | --- |
| 字幕时间码编辑 | P1 | 当前表格编辑器仅支持原文/译文编辑，时间码只读 |
| 时间轴偏移快捷键 | P1 | ±0.1s / ±1s 步进快捷键暂未实现，当前仅手动输入毫秒数 |
| 复制行 | P1 | 表格编辑器复制整行功能暂未实现 |
| macOS 右键菜单 | P1 | macOS 无注册表机制，需用 Services 菜单或 Finder 扩展实现 |
| Linux 支持 | P2 | 后端已适配 Linux 下载源，前端需补充平台测试 |
| 软件帧流方案 | P1 | 1080p 优先的帧流渲染（方案 A） |
| TitleBar 自绘 | P1 | 当前使用 Tauri 默认标题栏 |
| 历史记录面板 | P1 | 主界面缺少历史记录查看入口 |
| 最近文件快速访问 | P1 | 菜单栏「最近文件」子菜单 |
| 无障碍（a11y） | P2 | ARIA 标签、键盘导航、屏幕阅读器 |
| 响应式布局 | P2 | 最小窗口尺寸强制、sm/md/lg 断点 |
| HDR10+ 检测 | P2 | 动态元数据 SEI 检测 |
| 跳过版本功能 | P2 | 用户可选择跳过某个版本不提示 |

---

## 自动更新与发布

### 架构

- 使用 `tauri-plugin-updater` 实现自动更新
- 签名密钥：Ed25519 密钥对，私钥本地保存，公钥写入 `tauri.conf.json`
- 更新清单：`latest.json` 托管在 GitHub Pages（`gh-pages` 分支）
- 安装包存储：GitHub Releases
- 国内加速：`latest.json` 里的 URL 用 `gh-proxy.com` 前缀

### 发布流程

```cmd
set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=你的私钥密码
set GITHUB_TOKEN=ghp_xxxxxxxxxxxx

npm run publish 1.0.1 "修复字幕提取进度条
新增自动更新功能"
```

脚本自动完成：改版本号 → 带签名构建 → 创建 GitHub Release → 上传 .exe + .sig → 推送 latest.json

### 仅构建不发布

```cmd
set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=你的密码
npm run publish:dry 1.0.1 "测试内容"
```

详细指南见 [`docs/auto-update-publish-guide.md`](./docs/auto-update-publish-guide.md)。

---

## 许可证

本项目拟采用 **GPL-2.0+** 协议（与 FFmpeg 含 libass 后的 GPL-2.0+ 整体兼容，避免 LGPL 动态链接合规复杂度）。

- 软件开源协议：GPL-2.0+
- libmpv：LGPL-2.1+（下载的 GPL 构建含 GPL 组件）
- FFmpeg：含 libass 后为 GPL-2.0+

许可证兼容性详细分析见 [`docs/license-compatibility.md`](./docs/license-compatibility.md)。

---

## 相关文档

- [macOS 适配方案](./docs/mac-adaptation-plan.md) — macOS 平台适配技术方案
- [翻译设置重构](./docs/translate-settings-redesign.md) — 多引擎翻译设置 UI 重构设计
- [Phase 2 竞品分析](./docs/phase2-competitor-analysis.md) — 竞品功能对比分析
- [Phase 2 路线图](./docs/phase2-roadmap.md) — 第二阶段开发路线图
- [Phase 2 策略分析](./docs/phase2-strategy-analysis.md) — 第二阶段产品策略分析
- [Videocaptioner 深度分析](./docs/videocaptioner-deep-analysis.md) — 竞品 Videocaptioner 深度分析
- [代码签名调研](./docs/代码签名调研报告.md) — macOS/Windows 代码签名调研
- [本地语音识别调研](./docs/本地语音识别调研报告.md) — 本地语音识别技术调研
- [Bob 翻译提供商](./docs/bob-text-translation-providers.md) — Bob 翻译的 provider 实现参考
- [网站技术栈与变现调研](./docs/website-tech-stack-and-monetization-research.md) — 官网技术栈与变现方式调研
- [AI 助手开发规范](./AGENTS.md) — 文件写入规范（防 GUI OOM）等开发约定
