# macOS 适配与特有功能开发计划

> 本文档基于 Windows 版功能完成后的现状评估，规划 macOS 适配的工作内容、优先级与执行顺序。
> 更新日期：2026-07-01

---

## 目录

- [一、项目现状与适配范围](#一项目现状与适配范围)
- [二、跨平台模块（无需改动）](#二跨平台模块无需改动)
- [三、Windows 强耦合模块清单](#三windows-强耦合模块清单)
- [四、工作内容分解](#四工作内容分解)
  - [P0 编译跑通](#p0-编译跑通)
  - [P1 Mac 特有功能](#p1-mac-特有功能)
  - [P2 体验优化](#p2-体验优化)
- [五、工作量估算](#五工作量估算)
- [六、关键风险点](#六关键风险点)
- [七、建议执行顺序](#七建议执行顺序)
- [八、本次启动阶段已完成事项](#八本次启动阶段已完成事项)

---

## 一、项目现状与适配范围

**技术栈**：Tauri 2 + React 18 + Rust，后端约 11,770 行 Rust + 前端 React。

**当前状态**：100% Windows 实现，共 62 处 `cfg(windows)` 条件编译。

**本次适配范围说明**：
- 播放预览（libmpv 悬浮窗） ，player.rs 在 Mac 上提供 stub（命令返回错误，前端降级提示）。
- 公证与分发（notarization）**暂不做**，等开发测试完毕后再补。
- 优先目标是让 Mac 能本地编译运行，核心功能（字幕提取/翻译/编辑/合并/搜索）可用。

---

## 二、跨平台模块（无需改动）

| 模块 | 行数 | 说明 |
| --- | --- | --- |
| `subtitle.rs` | 1981 | 字幕解析/双语检测，纯逻辑 |
| `translate.rs` | 1588 | 三引擎翻译/占位符/缓存/重试 |
| `search.rs` | 1684 | OpenSubtitles/SubHD/zimuku 爬虫 |
| `db.rs` | 431 | SQLite，rusqlite bundled 跨平台 |
| `config.rs` | 205 | 配置 + keyring（keyring 3 已支持 macOS Keychain）|
| `error.rs` | 699 | 错误定义 |
| 前端主体 | — | React/Tauri API，基本跨平台 |

---

## 三、Windows 强耦合模块清单

| 模块 | 行数 | `cfg(windows)` 数 | 适配难度 |
| --- | --- | --- | --- |
| `player.rs` | 1847 | 37 | 极高（本次用 stub 绕过） |
| `context_menu.rs` | 246 | 11 | 中（已有 stub，后续做 Mac 特有方案） |
| `ffmpeg.rs` | 1381 | 9 | 中（需改下载源+解压） |
| `ipc.rs` | 1437 | 3 | 中（补 stub + 系统语言/工作区） |
| `lib.rs` | 265 | 2 | 低（窗口居中逻辑） |

---

## 四、工作内容分解

### P0 编译跑通

#### 1. 构建链路适配

**`Cargo.toml`**
- Windows 依赖已用 `[target.'cfg(windows)'.dependencies]` 隔离 ✓
- Mac 不需要新增原生窗口依赖（因为播放器用 stub），仅保留 `libloading` 可选
- 确认所有依赖在 Mac 上可编译

**`tauri.conf.json`**
- `bundle` 增加 macOS 配置（`macOS.minimumSystemVersion` 等）
- 图标 `icon.icns` 已存在 ✓
- 窗口配置适配 Mac 标题栏

**`package.json`**
- 当前脚本全部硬编码 `C:\Users\terry\.cargo` 路径和 Windows `set` 语法
- 改造为多平台适配：用跨平台变量语法或按平台分支
- 新增 Mac 构建脚本

**`scripts/publish.mjs`**
- 当前硬编码 Windows 路径和 `bundle/nsis`
- 后续做分发时再改造（本次不做）

#### 2. `ffmpeg.rs` Mac 下载源

- `FFMPEG_DOWNLOAD_URLS` 的 `#[cfg(not(windows))]` 当前指向 Linux 构建，需改为 macOS 构建源
- Mac 架构区分：arm64（Apple Silicon）+ x86_64（Intel），需按 `cfg(target_arch)` 选择
- 解压逻辑：Mac 的 tar.xz 需新增解压支持（用 `tar` crate 或调用系统 `tar`）
- 下载后复制逻辑硬编码 `.exe`，需改为按平台判断扩展名

#### 3. `player.rs` Mac stub

- 本次**不做** Mac 播放器实现
- Mac 上 `Player` 结构体、`player_init` 等保持 stub（返回错误）
- 前端在 Mac 上检测到播放器不可用时，降级提示"播放预览暂不支持，可导出后用系统播放器观看"
- libmpv 下载相关命令在 Mac 上返回未安装状态

#### 4. `ipc.rs` Mac 补全

- `detect_system_lang`：Mac 用 `std::env::var("LANG")` 或 `core-foundation` 的 `CFLocaleCopyCurrent`
- `get_work_area`：Mac 用 Tauri 的 `available_monitors()` API 获取屏幕尺寸（跨平台方案，避免直接调 NSScreen）
- `player_init` 等：保持 stub

#### 5. `lib.rs` 窗口居中

- Windows 用 `GetCursorPos`/`MonitorFromPoint` 居中窗口
- Mac 改用 Tauri 跨平台 API `window.center()` 或 `available_monitors()`

#### 6. `context_menu.rs`

- 已有非 Windows stub（返回 `Ok`/`false`），能编译
- Mac 特有右键菜单方案留到 P1

---

### P1 Mac 特有功能

#### 7. macOS 右键菜单 / 快速操作

Mac 没有注册表式右键菜单，等价方案：
- **方案 A（推荐）**：打包时生成 **Automator Quick Action**（`.workflow`），用户手动安装到 `~/Library/Services/`
- **方案 B**：Finder Sync Extension（需额外 target + 签名 + 沙盒，复杂）
- **方案 C**：安装脚本写入 `~/Library/Services/`
- 设置页新增 Mac 版"安装右键菜单"按钮

#### 8. Mac 文件关联（双击打开）

- `tauri.conf.json` 的 `bundle.macOS` 配置 `CFBundleDocumentTypes` + UTI
- 单实例转发的 argv 解析需适配 Mac（`.app` 包路径处理）

#### 9. Mac 系统播放器枚举

- Mac 用 **LaunchServices** API（`LSCopyApplicationURLsForURL`）枚举可打开视频的 app
- 图标提取：`NSWorkspace.iconForFile:` → NSImage → PNG
- `open_in_system_player`：`open <file>`

#### 10. 公证与分发（本次不做，后续补）

- `codesign --deep --options runtime` + `xcrun notarytool submit` + `stapler staple`
- 需 Apple Developer ID 证书
- `publish.mjs` 集成公证流程
- libmpv/FFmpeg 的 dylib 重新签名

---

### P2 体验优化

- Mac 红绿灯按钮与自绘 TitleBar 协调
- Retina HiDPI 坐标处理
- Mac 沙盒（App Sandbox）：若上架 Mac App Store 需沙盒化
- 拖放验证（Tauri DragDropEvent 已跨平台封装）
- Mac 通知授权弹窗

---

## 五、工作量估算

| 阶段 | 内容 | 复杂度 | 估算占比 |
| --- | --- | --- | --- |
| **P0 编译跑通** | Cargo.toml + tauri.conf + 构建脚本 | 中 | ~15% |
| | ffmpeg.rs Mac 下载源+解压 | 中 | ~12% |
| | player.rs Mac stub | 低 | ~5% |
| | ipc.rs / lib.rs stub 补全 | 低 | ~8% |
| | 本地编译运行验证 | 中 | ~10% |
| **P1 Mac 特有功能** | 右键菜单（Quick Action） | 中高 | ~15% |
| | 文件关联 | 低 | ~5% |
| | 系统播放器枚举 | 中 | ~10% |
| | 公证与分发（后续） | 中高 | ~15% |
| **P2 体验** | Retina/沙盒/通知 | 中 | ~5% |

> 注：因播放预览（libmpv 悬浮窗）本次用 stub 绕过，工作量相比完整适配减少约 35%。

---

## 六、关键风险点

1. **ffmpeg Mac 构建来源**：需确认可靠的 macOS arm64 + x86_64 GPL 构建源，国内下载速度需考虑 gh-proxy 加速。
2. **Mac 坐标系**：Mac NSView 坐标原点在左下角，Windows 在左上角（本次因播放器用 stub，此风险降级）。
3. **keyring 在 Mac 上的行为**：keyring 3 支持 macOS Keychain，但需确认访问权限和钥匙串弹窗体验。
4. **Tauri 单实例在 Mac 上的行为**：`tauri-plugin-single-instance` 在 Mac 上通过 IPC 文件锁实现，需验证 argv 转发。
5. **libmpv/FFmpeg 的许可证**：Mac 构建源需确认许可证（FFmpeg GPL-2.0+、libmpv LGPL-2.1+）与项目 Apache-2.0 兼容（子进程调用/运行时 dlopen 不构成链接，不传染主程序）。

---

## 七、建议执行顺序

1. **构建链路**（Cargo.toml / tauri.conf.json / package.json）→ Mac 能 `cargo build` 通过
2. **stub 补全**（player.rs / ipc.rs / lib.rs / context_menu.rs）→ Mac 能编译
3. **ffmpeg.rs Mac 下载**→ 核心功能（提取/翻译/合并）可用
4. **本地运行验证**→ 确认核心流程跑通
5. **P1 Mac 特有功能**（右键菜单/文件关联/播放器枚举）→ 后续迭代
6. **公证与分发**→ 开发测试完毕后补

---

## 八、本次启动阶段已完成事项

> 本节记录本次"搭建开发环境让程序本地跑起来"阶段实际完成的改动。

### 环境
- Rust 1.93.1 (Homebrew, aarch64-apple-darwin)
- Node v22.22.3, npm 10.9.8
- Apple Silicon (arm64)

### 已完成改动

| 文件 | 改动 |
| --- | --- |
| `docs/mac-adaptation-plan.md` | 新建，完整开发计划 |
| `package.json` | 脚本多平台适配：新增跨平台 `start`/`build:release`/`build:dmg`/`build:app`，原 Windows 脚本保留为 `*:win` 后缀 |
| `src-tauri/tauri.conf.json` | bundle 增加 `macOS.minimumSystemVersion: "11.0"`、`signingIdentity: null`（暂不签名） |
| `src-tauri/src/player.rs` | 修复 `GetCurrentThreadId` 缺少 `cfg(windows)`；`create_child_window` 加 `cfg(windows)` + 非 Windows stub；新增非 Windows `Player` stub 结构体（所有方法返回错误） |
| `src-tauri/src/ffmpeg.rs` | 下载源按 `target_os`+`target_arch` 分支：macOS arm64/x86_64 各自 BtbN 源；解压新增 `tar.xz` 支持（调用系统 `tar`）；可执行文件名按平台判断（无 `.exe`）；macOS/Linux 复制后设置 `0o755` 可执行权限 |
| `src-tauri/src/ipc.rs` | `detect_system_lang` 非 Windows 改为读 `LANG`/`LC_ALL`/`LC_MESSAGES` 环境变量；`get_work_area` 非 Windows 改为用 Tauri `available_monitors()` API |
| `src-tauri/src/lib.rs` | 窗口居中非 Windows 改为用 Tauri `available_monitors()` API |

### 验证结果
- `cargo check` 通过（仅 Windows 代码被 cfg 掉产生的 unused 警告，无 error）
- `npm run build`（前端）通过
- `npm run tauri dev` 成功启动，日志确认：
  - SQLite 数据库打开：`~/Library/Application Support/com.zimufan.ai-subtrans/zimufan.db`
  - `AI-SubTrans 启动完成`
- 唯一非致命错误：updater endpoint 无 Mac release（预期内，分发阶段再处理）

### 未做（留待后续）
- player.rs Mac 原生播放器实现（本次按用户要求用 stub，无悬浮窗）
- 公证与分发（notarization，等开发测试完毕后再做）
- Mac 右键菜单 / 文件关联 / 系统播放器枚举（P1 Mac 特有功能）
- `scripts/publish.mjs` Mac 适配（分发阶段再做）

---

## 附录：Mac 适配涉及文件清单

| 文件 | 改动类型 |
| --- | --- |
| `Cargo.toml` | 确认依赖跨平台 |
| `tauri.conf.json` | 增加 macOS bundle 配置 |
| `package.json` | 多平台脚本适配 |
| `src-tauri/src/ffmpeg.rs` | Mac 下载源 + 解压 + 扩展名 |
| `src-tauri/src/player.rs` | Mac stub（已有部分 stub） |
| `src-tauri/src/ipc.rs` | 系统语言/工作区 Mac 实现 |
| `src-tauri/src/lib.rs` | 窗口居中跨平台 |
| `src-tauri/src/context_menu.rs` | 已有 stub，暂不改 |
| `src/components/VideoPlayer.tsx` | Mac 降级提示（可选） |
