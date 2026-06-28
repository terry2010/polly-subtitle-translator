# AI-SubTrans

> 跨平台（Windows / macOS）AI 字幕提取、翻译与编辑桌面工具。
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
- [许可证](#许可证)

---

## 项目简介

AI-SubTrans（内部代号 `zimufan`）是一款基于 **Tauri 2 + React 18 + Rust** 构建的轻量级桌面字幕工具。它把视频字幕的提取、机器翻译、表格化编辑、播放预览与软合并回视频整合到一个应用中，覆盖从片源到成品字幕的完整链路。

应用支持两种使用场景：

- **交互模式**：主界面打开/拖入视频或字幕文件，进入完整的提取 → 翻译 → 编辑 → 预览 → 合并流程。
- **静默模式**：通过系统右键菜单（Windows 注册表 / macOS Quick Action）对视频或字幕文件一键触发「提取翻译合并」或「编辑翻译」，处理完成后通过系统通知告知用户。

详细需求规格见 [`docs/需求文档.md`](./docs/需求文档.md)（v1.7，需求评审定稿）。

---

## 核心功能

### 字幕提取

- 从 mkv / mp4 / avi / mov 视频中提取内嵌软字幕流。
- 默认输出 srt，原流为 ass/vtt 时可选保留原格式。
- 自动识别图形字幕流（hdmv_pgs_subtitle / dvd_subtitle / dvb_subtitle）并禁用提取按钮。
- 默认字幕选择规则：优先英文 SDH（title 含 SDH/HI/CC）→ 普通英文 → 任意字幕流兜底。

### 多引擎翻译

- 支持 **百度翻译 / Bing 翻译 / Google 翻译** 三家 API，按已配置引擎渲染对应按钮。
- 占位符保护算法：翻译前用 Unicode 私用区字符替换 ass 样式标记 `{\\...}`、HTML 标签、换行符，翻译后回填，避免标记被翻译破坏。
- 翻译分段：按字幕条数分段，按 API 单位（百度按字节、Google/Bing 按字符）累计，单条超限按句号二次切分。
- 翻译缓存：相同原文 + 源语言 + 目标语言 + API 的结果缓存到 SQLite，避免重复计费。
- 失败重试：单条失败指数退避重试 3 次（1s/2s/4s），仍失败保留原文并标记 `[翻译失败]`。
- 凭据通过系统密钥环（keyring）存储，不写入数据库。

### 字幕编辑

- 表格化编辑器，支持时间码 / 原文 / 译文编辑、增删行、复制行。
- 虚拟滚动（@tanstack/react-virtual）支撑万级条数流畅滚动。
- 时间轴偏移（快捷键 ±0.1s / ±1s 步进）、查找替换、撤销重做（栈深 50）。
- 双语字幕自动检测：按 Unicode 范围分类语言，检测条目内「按语言分块」模式，阈值 60% 判定双语并拆分原文/译文。
- 导出 srt / ass / vtt 多格式，命名规则见需求文档 §4.4。

### 播放预览

- 内嵌 **libmpv**（按需从 SourceForge 下载，动态加载），子窗口嵌入主窗口播放视频。
- 字幕不叠加到视频画面，改为下方字幕对比预览区随播放进度滚动高亮对应条。
- 对比预览模式：原文 / 译文 / 双语三种显示模式。
- 播放控制：播放/暂停、进度拖动、音量调节、静音、倍速（0.5/0.75/1/1.25/1.5/2 六档）。
- HDR / Dolby Vision 视频检测并主动提示用户（HdrNotice 组件）。
- DPI 缩放：使用 Tauri scaleFactor 同步 libmpv 子窗口坐标。

### 合并回视频

- 翻译或提取后可将字幕软合并回视频，生成同目录 `<videoname>.merged.<ext>` 新文件。
- 同名冲突自动递增命名（`.merged.1.<ext>`、`.merged.2.<ext>`……）。

### 在线搜索下载

- 一期接入 **OpenSubtitles** 官方 API（用户自注册 API Key）。
- 按文件名与时长搜索字幕，显示评分与下载次数，用户选择下载。
- 下载的字幕若非目标语言可继续翻译。
- SubHD / 字幕组等国内站点移至二期。

### 多入口触发

- **系统右键菜单**：视频文件右键「AI-SubTrans：提取翻译字幕」，字幕文件右键「AI-SubTrans：编辑/翻译字幕」。
  - Windows：写注册表 `HKCU\Software\Classes\SystemFileAssociations\<ext>\shell\zimufan`（用户级，无需提权）。
  - macOS：Automator Quick Action 部署到 `~/Library/Services/` + CFBundleDocumentTypes 文件关联（二期实现）。
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
| HTTP 客户端 | reqwest 0.12（rustls-tls） |
| 凭据存储 | keyring 3（系统密钥环） |
| 字幕解析 | srt/vtt 自写解析器；ass 使用 ass-core + ass-editor |
| 编码探测 | chardetng + encoding_rs |
| 日志 | tracing + tracing-subscriber + tracing-appender（按天滚动，保留 7 天） |
| 错误处理 | thiserror + anyhow，自定义 AppError / IpcError（含 severity 分级） |
| 哈希 | sha2 / md-5 / hex |
| 解压 | sevenz-rust（libmpv 7z 包解压） |
| Windows API | windows 0.61 + winreg 0.55 + libloading 0.8 |

### 前端（React 18 / TypeScript / Vite 6）

| 领域 | 选型 |
| --- | --- |
| UI 框架 | React 18 + React Router 6 |
| 状态管理 | Zustand 4（含 persist 中间件） |
| 组件库 | shadcn/ui（源码拷贝方式）+ Radix UI 原语 |
| 样式 | Tailwind CSS 4 + tw-animate-css |
| 表格 / 虚拟滚动 | @tanstack/react-table + @tanstack/react-virtual |
| 拖拽 | react-dropzone |
| 图标 | lucide-react |
| 国际化 | i18next + react-i18next（中/英双语，174 个 key） |
| 通知 | sonner |
| Tauri 插件 | dialog / fs / shell / os / notification / process / single-instance |

---

## 项目结构

```
ai-subtrans/
├── docs/
│   └── 需求文档.md              # v1.7 需求评审定稿（1542 行）
├── src/                          # 前端 React 源码
│   ├── main.tsx                  # React 入口
│   ├── App.tsx                   # 根组件：路由、主题、CLI 参数监听、拖放
│   ├── views/
│   │   ├── MainView.tsx          # 主界面（视频打开、字幕提取、翻译、合并）
│   │   └── SettingsView.tsx      # 设置页（6 个分区）
│   ├── components/
│   │   ├── VideoPlayer.tsx       # libmpv 子窗口播放器
│   │   ├── SubtitleListPanel.tsx # 字幕表格编辑器（虚拟滚动）
│   │   ├── SubtitlePreviewPanel.tsx # 字幕对比预览（播放联动高亮）
│   │   ├── TranslatePanel.tsx    # 翻译控制面板
│   │   ├── SearchDialog.tsx      # OpenSubtitles 搜索弹窗
│   │   ├── HdrNotice.tsx         # HDR/Dolby 提示
│   │   ├── AutoTextarea.tsx      # 自适应高度文本框
│   │   └── ui/                   # shadcn/ui 组件
│   ├── stores/                   # Zustand stores
│   │   ├── subtitleStore.ts      # 字幕状态 + 撤销重做 + 查找替换
│   │   ├── videoStore.ts         # 视频探测结果 + 字幕流自动选择
│   │   ├── translateStore.ts     # 翻译进度 + 结果
│   │   └── themeStore.ts         # 主题 + 语言（持久化）
│   ├── lib/
│   │   ├── api.ts                # IPC 调用封装（30+ 方法）
│   │   ├── ipc-types.ts          # 与 Rust 后端对应的 TS 类型
│   │   ├── i18n.ts               # i18next 初始化
│   │   └── utils.ts              # cn / 时间 / 字节格式化
│   ├── locales/
│   │   ├── zh.json               # 中文翻译（174 key）
│   │   └── en.json               # 英文翻译（174 key）
│   └── styles/globals.css        # Tailwind + 主题变量
├── src-tauri/                    # Rust 后端源码
│   ├── Cargo.toml
│   ├── tauri.conf.json           # Tauri 配置（窗口 1280×800，最小 1024×600）
│   ├── capabilities/default.json # Tauri 权限声明
│   ├── build.rs
│   └── src/
│       ├── main.rs               # 入口（调用 lib::run）
│       ├── lib.rs                # 应用初始化、日志、CLI 解析、单实例
│       ├── ipc.rs                # 49 个 Tauri 命令 handler
│       ├── translate.rs          # 翻译模块（占位符/分段/缓存/三家 provider）
│       ├── subtitle.rs           # 字幕解析（srt/vtt/ass）+ 双语检测
│       ├── ffmpeg.rs             # FFmpeg 封装（probe/提取/合并/HDR 检测）
│       ├── player.rs             # libmpv 下载/加载/子窗口嵌入/播放控制
│       ├── search.rs             # OpenSubtitles API 客户端
│       ├── db.rs                 # SQLite 表结构 + 迁移 + CRUD
│       ├── config.rs             # 配置管理 + 凭据存储
│       ├── context_menu.rs       # Windows 右键菜单注册
│       └── error.rs              # AppError + IpcError（severity 分级）
├── package.json
├── vite.config.ts
├── tsconfig.json
├── index.html
└── AGENTS.md                     # AI 助手开发规范
```



---

## 快速开始

### 环境要求

- **Rust** ≥ 1.89（含 cargo）
- **Node.js** ≥ 18（含 npm）
- **Windows 10 2004+**（开发与运行）或 **macOS 12+**
- Windows 端需 WebView2 Runtime（Win11 自带，Win10 由安装包 bootstrapper 按需安装）
- FFmpeg：默认使用内置精简构建，也可在设置页指向系统完整版

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
npm run tauri build
```

产物位于 `src-tauri/target/release/bundle/`：
- Windows：`.msi` 安装包
- macOS：`.dmg`（分架构 arm64 / x86_64）

### 仅构建前端

```bash
npm run build    # tsc -b && vite build
npm run preview  # 预览构建产物
```

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

所有 IPC 失败统一返回：

```json
{
  "ok": false,
  "error": {
    "code": "error.<域>.<code>",
    "i18nKey": "error.<域>.<code>",
    "args": {},
    "message": "兜底英文/中文文案",
    "severity": "recoverable | restart | reinstall"
  }
}
```

- `recoverable`：用户可恢复（如翻译失败可重试），前端用 toast 提示。
- `restart`：需重启应用（如 SQLite 损坏），前端用模态对话框提示。
- `reinstall`：需重装或重配环境（如 libmpv 下载损坏），前端引导重装。

错误码定义见 `src-tauri/src/error.rs`（30+ 个 AppError 变体）。

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
| 最近文件 | SQLite `recent_files` 表（保留 50 条） |
| API provider 元信息 | SQLite `api_provider` 表 |
| 搜索 provider 元信息 | SQLite `search_provider` 表 |

数据库文件位于 `<app_data_dir>/zimufan.db`。

### 核心 config 表 key 清单

| key | 说明 | 默认值 |
| --- | --- | --- |
| `default_target_lang` | 默认目标翻译语言（ISO 639-1） | 跟随系统语言 |
| `default_target_lang_follow_system` | 是否跟随系统语言 | `true` |
| `default_api_provider` | 默认翻译 API | `baidu` |
| `source_lang_priority` | 源语言优先级列表（JSON 数组） | `["en-sdh","en"]` |
| `auto_merge` | 翻译完成后默认是否合并到视频 | `false` |
| `subtitle_name_template` | 字幕文件命名模板 | `<videoname>.<targetlang>.srt` |
| `ffmpeg_path` | FFmpeg 路径（空=内置） | `""` |
| `player_subtitle_mode` | 播放器下方字幕区显示模式 | `bilingual` |
| `hdr_dolby_prompt` | HDR/杜比提示开关 | `true` |
| `enabled_search_providers` | 启用的字幕站 | `["opensubtitles"]` |
| `theme` | 主题 | `system` |
| `log_level` | 日志级别 | `info` |
| `proxy_*` | 网络代理配置 | — |

### 翻译 API 配置

| API | 默认 QPS | 单次上限 | 单位 | 默认并发 |
| --- | --- | --- | --- | --- |
| 百度翻译 | 1（免费版） | 6000 | 字节 | 3 |
| Bing 翻译 | 10 | 5000 | 字符 | 3 |
| Google 翻译 | 100 | 5000 | 字符 | 3 |

实际并发 = `min(用户配置并发, QPS 上限)`。Google 翻译在中国大陆不可直连，需在「高级」配置 HTTP/SOCKS5 代理。

---

## 使用流程

### 交互模式（主界面）

1. **打开视频**：点击「打开视频文件」或拖入 `.mkv/.mp4/.avi/.mov` 文件。
2. **选择字幕流**：应用自动按优先级选择字幕流，可在字幕流下拉中手动切换。
3. **提取字幕**：点击「提取字幕」，选择保存位置与格式（默认 srt，ass/vtt 原流可保留格式）。
4. **翻译**：在翻译面板选择目标语言与翻译引擎，点击翻译按钮。翻译进度实时显示，可取消。
5. **编辑**：在字幕表格编辑器中修改时间码、原文、译文，支持查找替换、时间轴偏移、撤销重做。
6. **预览**：点击播放按钮，libmpv 子窗口播放视频，下方字幕区随播放进度滚动高亮。
7. **合并/导出**：点击「合并到视频」生成软合并新文件，或「导出字幕」另存为 srt/ass/vtt。

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
│  └───────────────┘                                   │
└─────────────────────────────────────────────────────┘
         │                    │
         ▼                    ▼
   ┌──────────┐        ┌──────────────┐
   │ SQLite   │        │ libmpv 子窗口 │
   │ zimufan.db│       │ (按需下载)    │
   └──────────┘        └──────────────┘
         │
         ▼
   ┌──────────┐
   │ keyring  │  系统密钥环（凭据）
   └──────────┘
```

### IPC 通信

后端注册了 49 个 Tauri 命令（`src-tauri/src/ipc.rs`），覆盖：

- 视频探测与字幕提取（`probe_video` / `extract_subtitle`）
- 字幕解析与编辑（`parse_subtitle_file` / `save_subtitle_file_cmd` / `detect_bilingual` / `split_bilingual_subtitle`）
- 翻译（`translate_subtitle` / `cancel_translate` / `test_translate_connection` / `get_supported_target_langs`）
- 配置与凭据（`get_config` / `set_config` / `save_credential` / `get_credential`）
- 播放器控制（`player_init` / `player_load_cmd` / `player_play_cmd` / `player_seek_cmd` / `player_set_volume_cmd` / `player_set_speed_cmd` 等）
- 字幕搜索（`search_subtitles_online` / `download_subtitle_online`）
- 合并（`merge_subtitle`）
- 右键菜单（`register_video_menu` / `register_subtitle_menu` / `is_video_menu_registered` 等）
- libmpv 管理（`get_libmpv_status_cmd` / `download_libmpv_cmd`）

事件流（后端 → 前端）：

- `cli-args`：单实例转发文件路径。
- `player_position`：播放位置更新（供字幕高亮联动）。
- `libmpv_download_progress`：libmpv 下载进度。
- `translate-progress` / `translate-entry-done`：翻译进度与逐条回调。

### 翻译模块架构

```
translate.rs
├── PlaceholderProtector     # 占位符保护（U+E000~U+E0FF）
├── split_text()             # 分段算法
├── BaiduProvider            # 百度翻译（MD5 签名）
├── BingProvider             # Bing 翻译（Azure Translator）
├── GoogleProvider           # Google 翻译（API Key）
└── translate_with_retry()   # 限流 + 指数退避重试（1s/2s/4s）
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

需求文档定义了三种渲染方案（按优先级）：

1. **方案 A：软件帧流**（1080p 优先）— libmpv 解码后通过 IPC 推送帧数据到前端 canvas 渲染。
2. **方案 B：原生子窗口嵌入**（4K/HDR）— libmpv 作为子窗口嵌入主窗口，DPI 同步。
3. **降级：系统播放器** — 方案 A/B 均不达标时调用系统默认播放器，仅保留时间轴联动。

当前实现为方案 B（子窗口嵌入），方案 A 与降级路径为待实现项。

---

## 实现状态

本项目对照需求文档 v1.7 开发，主流程已跑通。以下为各模块实现状态摘要。

### 已实现（主流程可用）

| 模块 | 状态 | 说明 |
| --- | --- | --- |
| 视频探测 | ✅ | FFmpeg probe，含 HDR/Dolby/图形字幕检测 |
| 字幕提取 | ✅ | srt/vtt 解析与回写，ass 解析待完善 |
| 双语检测 | ✅ | Unicode 范围分类 + 60% 阈值判定 |
| 三引擎翻译 | ✅ | 百度/Bing/Google 完整实现，含签名/分段/缓存/重试 |
| 占位符保护 | ✅ | 私用区字符方案（U+E000~U+E0FF） |
| 字幕编辑器 | ✅ | 虚拟滚动、增删行、查找替换、时间轴偏移、撤销重做 |
| 播放预览 | ✅ | libmpv 子窗口嵌入、播放控制、字幕联动高亮 |
| 对比预览 | ✅ | 原文/译文/双语三种模式 |
| 合并回视频 | ✅ | FFmpeg 软合并，同名递增 |
| OpenSubtitles 搜索 | ✅ | REST API 搜索与下载 |
| 系统右键菜单 | ✅ | Windows 注册表注册/注销/状态检测 |
| 单实例转发 | ✅ | Windows argv 解析与事件转发 |
| 配置与凭据 | ✅ | SQLite + keyring |
| 错误处理 | ✅ | AppError + IpcError（severity 三级） |
| 国际化 | ✅ | 中/英双语，174 key |
| 主题 | ✅ | 浅色/深色/跟随系统 |
| 日志 | ✅ | tracing 按天滚动 |

### 待完善 / 待实现

| 项 | 优先级 | 说明 |
| --- | --- | --- |
| ass 解析与回写 | P0 | 当前 ass 解析为占位实现，需接入 ass-core/ass-editor 完整实现 |
| 翻译分段条数硬限制 ≤ 30 | P0 | 当前仅按字节/字符累计，未限制条数 |
| api_provider 表初始化数据 | P0 | 需插入 baidu/bing/google 三行默认值 |
| SDH 识别 | P0 | 字幕流 title 含 SDH/HI/CC 判定 |
| 系统播放器降级路径 | P1 | 方案 A/B 均不达标时的降级 |
| 软件帧流方案（方案 A） | P1 | 1080p 优先的帧流渲染 |
| 占位符备选方案 | P1 | 全角方括号 / Base64 编码方案 |
| 翻译取消按钮 UI | P1 | 后端已实现，前端未提供按钮 |
| TitleBar 自绘 | P1 | 当前使用 Tauri 默认标题栏 |
| macOS 右键菜单 | P1 | Automator Quick Action + CFBundleDocumentTypes |
| OpenSubtitles 注册引导 | P1 | 首次未配置 Key 时的注册指引 |
| 命名模板设置分区 | P1 | 设置页缺少字幕导出命名规则配置 |
| 历史记录面板 | P1 | 主界面缺少历史记录查看入口 |
| 最近文件快速访问 | P1 | 菜单栏「最近文件」子菜单 |
| 无障碍（a11y） | P2 | ARIA 标签、键盘导航、屏幕阅读器 |
| 响应式布局 | P2 | 最小窗口尺寸强制、sm/md/lg 断点 |
| 崩溃 minidump | P2 | crash-handler + minidump 本地留存 |
| 令牌桶限流 | P2 | 翻译 QPS 精确控制 |
| HDR10+ 检测 | P2 | 动态元数据 SEI 检测 |
| 错误码文档 | P2 | `docs/error-codes.md` |
| 许可证兼容性文档 | P2 | `docs/license-compatibility.md` |

### 已知问题

- `SubtitleListPanel.tsx` 查找替换逻辑存在条件赋值错误（`text: e.translated ? text : text` 始终为 text）。
- 翻译对齐按索引，未实现需求文档要求的「按唯一 ID 严格对齐」。
- 翻译缓存 key 使用 sha256，需求文档要求直接用 source_text 原文。
- `MainView.tsx` 静默模式使用 `setTimeout` 等待 probe，不够可靠。

---

## 许可证

本项目拟采用 **GPL-2.0+** 协议（与 FFmpeg 含 libass 后的 GPL-2.0+ 整体兼容，避免 LGPL 动态链接合规复杂度）。

- 软件开源协议：GPL-2.0+
- libmpv：LGPL-2.1+
- FFmpeg：含 libass 后为 GPL-2.0+

许可证兼容性详细分析待产出 `docs/license-compatibility.md`。

---

## 相关文档

- [需求文档 v1.7](./docs/需求文档.md) — 完整需求规格（1542 行，需求评审定稿）
- [AI 助手开发规范](./AGENTS.md) — 文件写入规范（防 GUI OOM）等开发约定


