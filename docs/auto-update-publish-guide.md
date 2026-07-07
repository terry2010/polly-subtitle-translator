# 自动更新与发布指南

> 创建时间：2025-06-24
> 状态：已实施
> 关联功能：Tauri Updater 自动更新 + GitHub Release 发布脚本

---

## 一、整体架构

```
┌─────────────┐         ┌──────────────────┐         ┌─────────────────┐
│  客户端 App  │ ──检查──→ │  latest.json     │         │  GitHub Release  │
│  (用户电脑)  │ ←─JSON── │  (gh-pages 分支)  │         │  (安装包存储)     │
│             │ ──下载──→ │                  │ ──────→ │  xxx-setup.exe   │
│  验证签名    │         └──────────────────┘         │  xxx-setup.exe   │
│  静默安装    │                                      │  .sig 签名文件   │
└─────────────┘                                      └─────────────────┘
```

### 组件说明

| 组件 | 位置 | 作用 |
| --- | --- | --- |
| 签名密钥 | `~/.tauri/ai-subtrans.key` | Ed25519 密钥对，构建时签名安装包 |
| 公钥 | `tauri.conf.json` 的 `plugins.updater.pubkey` | 客户端验证下载的安装包 |
| latest.json | GitHub Pages（`gh-pages` 分支根目录） | 更新清单，包含版本号/签名/下载URL |
| GitHub Release | 主仓库的 Releases 页面 | 存放 `.exe` 安装包和 `.sig` 签名文件 |
| 发布脚本 | `scripts/publish.mjs` | 一键完成改版本号+构建+发布 |
| 客户端检查 | `src/stores/updateStore.ts` | 启动后 5 秒自动检查 |
| 更新弹窗 | `src/components/UpdateDialog.tsx` | 显示版本信息/下载进度/安装重启 |

### 客户端更新流程

```
应用启动
  │
  ├─→ 延迟 5 秒，静默请求 latest.json
  │     │
  │     ├─ 无新版本 → 静默结束
  │     │
  │     └─ 有新版本 → 弹窗显示版本号 + 更新内容
  │           │
  │           ├─ 用户点"立即更新"
  │           │     ├─ 下载安装包（显示进度/速度/ETA）
  │           │     ├─ 下载 .sig 签名文件
  │           │     ├─ 用公钥验证签名（失败则中止）
  │           │     ├─ 静默执行安装（不弹 SmartScreen）
  │           │     └─ 提示重启 → 用户确认 → 应用重启
  │           │
  │           └─ 用户点"稍后" → 下次启动再检查
  │
  └─ 设置页"关于"→"检查更新"按钮 → 手动触发同样流程
```

---

## 二、首次配置（只做一次）

### 2.1 生成签名密钥

```bash
npx tauri signer generate -w C:\Users\<你的用户名>\.tauri\ai-subtrans.key
```

- 会提示输入密码，记住密码
- 生成两个文件：
  - `.key`：私钥（**务必备份，丢了永远无法发更新**）
  - `.key.pub`：公钥（已写入 `tauri.conf.json`）
- 当前项目的密钥已生成，公钥已配置在 `tauri.conf.json` 的 `plugins.updater.pubkey`

### 2.2 配置环境变量

在系统环境变量或每次发布前设置：

```cmd
set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=你的私钥密码
set GITHUB_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxx
```

**GitHub Token 生成方法**：
1. GitHub → Settings → Developer settings → Personal access tokens → Tokens (classic)
2. 点 Generate new token (classic)
3. 勾选 `repo` 权限（包含 repo:status、repo_deployment、public_repo 等）
4. 生成后复制 token，**只显示一次**

### 2.3 创建 gh-pages 分支

```bash
git checkout --orphan gh-pages
git rm -rf .
echo "{}" > latest.json
git add latest.json
git commit -m "init gh-pages"
git push origin gh-pages
git checkout main
```

然后在 GitHub 仓库：
- Settings → Pages → Source → 选 `gh-pages` 分支 → /(root)
- 保存后等待几分钟，Pages 会激活
- 访问地址：`https://<你的用户名>.github.io/<仓库名>/latest.json`

### 2.4 备份私钥

将 `C:\Users\<你的用户名>\.tauri\ai-subtrans.key` 复制到安全位置：
- 网盘（加密后上传）
- U 盘
- 密码管理器（如 1Password、Bitwarden）

**私钥 + 密码都丢了 = 永远无法发布更新，所有已安装的客户端都无法升级。**

---

## 三、发布新版本

### 3.1 GitHub Actions 云端发布（优先使用，Win + Mac 双平台）

> **推荐方式**：不需要本地构建环境，GitHub 提供 Windows 和 macOS runner 免费构建。
> 以后发布版本优先使用此方式，确保双平台同时发布。

#### 前提：配置 GitHub Secrets

在仓库 **Settings → Secrets and variables → Actions** 中添加：

| Secret 名 | 值 | 说明 |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | 私钥文件内容 | `cat ~/.tauri/ai-subtrans.key` 的输出，不是路径 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 私钥密码 | 生成密钥时设置的密码 |

> `GITHUB_TOKEN` 由 Actions 自动提供，无需手动配置。

#### 触发方式

**方式一：GitHub Actions 页面手动触发（推荐）**

1. 打开仓库 → Actions → Release 工作流
2. 点 "Run workflow"
3. 输入版本号（如 `1.0.1`）和更新内容
4. 点绿色按钮触发
5. 约 20-30 分钟完成，自动构建 Win + Mac 并发布

**方式二：push tag 自动触发**

```bash
# 1. 确保代码已提交
git add -A && git commit -m "release: 1.0.1"

# 2. 打 tag 并推送
git tag v1.0.1
git push origin v1.0.1

# 3. GitHub Actions 自动触发，约 20-30 分钟完成
```

**方式三：gh CLI 触发（命令行）**

```bash
gh workflow run release.yml -f version=1.0.1 -f notes="修复内容1
修复内容2"
```

#### 工作流执行过程

```
触发 release.yml
      ↓
  prepare job（提取版本号 + 更新内容）
      ↓
  ┌─────────────────┬─────────────────┐
  │  build (windows) │  build (macos)  │  并行构建
  │  nsis 安装包      │  dmg + app.tar.gz │
  │  签名 → .exe+.sig │  签名 → .tar.gz+.sig │
  │  上传到 Release   │  上传到 Release   │
  └─────────────────┴─────────────────┘
      ↓
  update-manifest job
  读取 Release 所有 .sig → 合并多平台 latest.json → 推送 gh-pages
      ↓
  发布完成，客户端自动检测到新版本
```

#### 产物说明

| 平台 | Release 中的文件 | latest.json 中的 URL 指向 |
| --- | --- | --- |
| Windows x86_64 | `AI-SubTrans_x.y.z_x64-setup.exe` + `.sig` | `.exe`（NSIS 安装包） |
| macOS arm64 | `AI-SubTrans_x.y.z_aarch64.dmg` + `AI-SubTrans_x.y.z_aarch64.app.tar.gz` + `.sig` | `.app.tar.gz`（updater 用） |

> macOS 的 `.dmg` 供用户首次手动下载安装，`.app.tar.gz` 供 Tauri updater 增量更新用。

### 3.2 本地脚本发布（备选，仅 Windows）

> 仅在 CI 不可用或需要快速发布 Windows 版本时使用。
> 本地脚本只能构建 Windows NSIS 安装包，无法构建 macOS 包。

#### 交互式发布（推荐）

```powershell
cd c:\code\ai-subtrans
node scripts/publish.mjs
```

会依次提示输入：版本号、更新说明、私钥密码，确认后自动执行。
自动获取 GitHub Token（从 gh CLI）和私钥密码（从 `~/.tauri/ai-subtrans.key.password` 文件）。

#### 参数式发布

```powershell
node scripts/publish.mjs 1.0.1 "修复内容" --password=你的密码
```

如果已创建密码文件 `~/.tauri/ai-subtrans.key.password`，则无需 `--password` 参数：

```powershell
node scripts/publish.mjs 1.0.1 "修复内容"
```

脚本自动完成：
1. 修改 `package.json` / `tauri.conf.json` / `Cargo.toml` 的版本号
2. 带签名构建（生成 `.exe` + `.sig`，仅 Windows）
3. 创建 GitHub Release（tag: `v1.0.1`）
4. 上传 `.exe` 和 `.sig` 到 Release
5. 生成 `latest.json` 并推送到 `gh-pages` 分支

#### 只构建不发布（本地测试）

```powershell
node scripts/publish.mjs 1.0.1 "测试内容" --build-only
```

产物在：
```
C:\Users\terry\.cargo-target\zimufan\release\bundle\nsis\AI-SubTrans_1.0.1_x64-setup.exe
C:\Users\terry\.cargo-target\zimufan\release\bundle\nsis\AI-SubTrans_1.0.1_x64-setup.exe.sig
```

### 3.3 版本号规范

使用语义化版本（semver）：`主版本.次版本.修订号`

| 变更类型 | 示例 | 说明 |
| --- | --- | --- |
| 修复 bug | 1.0.0 → 1.0.1 | 修订号 +1 |
| 新功能 | 1.0.1 → 1.1.0 | 次版本 +1，修订号归 0 |
| 不兼容改动 | 1.1.0 → 2.0.0 | 主版本 +1，次版本和修订号归 0 |

### 3.4 更新内容格式

更新内容用 `\n` 分隔多行，会显示在：
- GitHub Release 的描述
- 客户端更新弹窗的文本框

示例：
```
"修复字幕提取进度条显示问题
新增 FFmpeg 按需下载
优化 libmpv 下载速度显示"
```

#### latest.json 多平台格式

```json
{
  "version": "1.0.1",
  "notes": "修复字幕提取进度条\n新增 FFmpeg 按需下载",
  "pub_date": "2026-07-02T10:00:00.000Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "dW50cnVzdGVk...",
      "url": "https://gh-proxy.com/https://github.com/terry2010/ai-subtrans/releases/download/v1.0.1/AI-SubTrans_1.0.1_x64-setup.exe"
    },
    "darwin-aarch64": {
      "signature": "dW50cnVzdGVk...",
      "url": "https://gh-proxy.com/https://github.com/terry2010/ai-subtrans/releases/download/v1.0.1/AI-SubTrans_1.0.1_aarch64.app.tar.gz"
    }
  }
}
```

#### macOS 未签名说明

当前 macOS 版本**未做 Apple 代码签名与公证**（`signingIdentity: null`）。用户首次打开时：
- 会被 Gatekeeper 拦截
- 需右键 → 打开，或在终端执行 `xattr -cr /Applications/AI-SubTrans.app`
- Tauri updater 的 Ed25519 签名（`.sig`）仍然有效，自动更新不受影响

> 后续如需正式分发，需 Apple Developer ID 证书 + `codesign` + `notarytool` 公证。

#### 相关文件

| 文件 | 作用 |
| --- | --- |
| `.github/workflows/release.yml` | GitHub Actions 工作流定义 |
| `scripts/publish.mjs --set-version` | CI 构建前改版本号 |
| `scripts/publish.mjs --update-latest` | CI 构建后从 Release assets 合并 latest.json |

---

## 四、latest.json 格式

脚本自动生成，格式如下：

```json
{
  "version": "1.0.1",
  "notes": "修复字幕提取进度条显示问题\n新增 FFmpeg 按需下载",
  "pub_date": "2025-06-24T10:30:00.000Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ6...",
      "url": "https://gh-proxy.com/https://github.com/owner/repo/releases/download/v1.0.1/AI-SubTrans_1.0.1_x64-setup.exe"
    }
  }
}
```

### 字段说明

| 字段 | 说明 |
| --- | --- |
| `version` | 新版本号（不含 `v` 前缀） |
| `notes` | 更新内容，`\n` 换行 |
| `pub_date` | 发布时间，ISO 8601 格式 |
| `platforms.windows-x86_64.signature` | `.sig` 文件内容（base64） |
| `platforms.windows-x86_64.url` | 安装包下载 URL（已加 gh-proxy 前缀） |

### 国内加速

`url` 字段使用 `gh-proxy.com` 前缀加速 GitHub 下载：
```
https://gh-proxy.com/https://github.com/owner/repo/releases/download/v1.0.1/AI-SubTrans_1.0.1_x64-setup.exe
```

如果 gh-proxy 不可用，可手动改为其他镜像或直连 GitHub。

---

## 五、客户端更新行为详解

### 5.1 自动检查

- **触发时机**：应用启动后 5 秒（不阻塞启动）
- **检查方式**：请求 `latest.json`，对比 `version` 与当前版本
- **有新版本**：弹出 UpdateDialog
- **无新版本**：静默结束
- **检查失败**：静默结束（不弹错误，避免打扰用户）

### 5.2 手动检查

- **入口**：设置页 → 关于 → "检查更新"按钮
- **有新版本**：弹出 UpdateDialog
- **无新版本**：显示"当前已是最新版本"（绿色提示，3 秒后消失）
- **检查失败**：显示"检查更新失败，请检查网络"（红色提示）

### 5.3 下载安装

- **进度显示**：百分比 + 下载速度（MB/s）+ 剩余时间（ETA）
- **进度更新频率**：每 200ms 一次（避免 UI 卡顿）
- **签名验证**：下载完成后自动验证，失败则中止并提示错误
- **安装方式**：静默执行 NSIS 安装包，不弹 SmartScreen
- **安装后**：提示"需要重启应用以完成更新"，用户点"立即重启"

### 5.4 不弹 SmartScreen 的原因

自动更新是**当前已运行的应用进程**拉起新安装包，不是用户双击 `.exe`，所以不经过 Windows SmartScreen 的交互式拦截。

但首次安装（用户手动双击 `.exe`）仍会弹 SmartScreen 警告，因为没有购买代码签名证书。用户需点击"更多信息"→"仍要运行"。

---

## 六、注意要点

### 6.1 私钥安全

- **永远不要**把私钥提交到 git 仓库
- **永远不要**把私钥密码写在代码里
- 私钥丢了 = 所有已安装的客户端都无法升级到新版本
- 建议备份到多个安全位置

### 6.2 版本号一致性

发布脚本会同时修改三个文件的版本号：
- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

三者必须一致，脚本已自动处理。手动修改时注意三个文件都要改。

### 6.3 构建环境

构建需要以下工具已安装：
- Node.js + npm
- Rust + Cargo
- Tauri CLI（`@tauri-apps/cli`）
- NSIS（Tauri 会自动下载）

构建命令使用的环境变量：
```
PATH=C:\Users\terry\.cargo\bin;%PATH%
CARGO_TARGET_DIR=C:\Users\terry\.cargo-target\zimufan
TAURI_SIGNING_PRIVATE_KEY=<私钥内容，脚本自动从文件读取>
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<私钥密码>
```

### 6.4 GitHub Pages 缓存

GitHub Pages 有缓存（通常 10 分钟左右）。发布后如果 `latest.json` 没立即更新：
- 等待 10-15 分钟
- 或在 URL 后加 `?t=时间戳` 强制刷新验证

### 6.5 gh-pages 分支保护

不要在 `gh-pages` 分支上开发代码，它只用于存放 `latest.json`。如果误操作导致分支损坏：
1. 重新创建 `latest.json`
2. 用脚本重新发布当前版本（会覆盖 GitHub Release 和 latest.json）

### 6.6 回滚已发布的版本

如果发布的版本有严重 bug：
1. **不要删除** GitHub Release（会导致已下载的用户签名验证失败）
2. 立即发布一个修复版本（如 1.0.2）
3. 更新 `latest.json` 指向新版本

如果必须回滚到旧版本：
1. 手动编辑 `gh-pages` 分支上的 `latest.json`
2. 把 `version` / `signature` / `url` 改回旧版本
3. 旧版本的 `.exe` 和 `.sig` 仍在 GitHub Release 里（不要删除旧 Release）

### 6.7 多平台扩展

当前已支持 Windows + macOS（arm64）双平台云端发布，详见 §3.5。

`latest.json` 的 `platforms` 已包含多平台：
```json
"platforms": {
  "windows-x86_64": { ... },
  "darwin-aarch64": { ... }
}
```

如需新增平台（如 macOS x86_64 / Linux）：
1. 在 `.github/workflows/release.yml` 的 matrix 中添加对应 os
2. `publish.mjs` 的 `detectPlatform()` 已支持 `darwin-x86_64` / `darwin-universal`，新增平台需扩展该函数
3. 每个平台需要各自的安装包和签名文件

### 6.8 跳过版本功能（未实现）

当前设计没有"跳过此版本"功能。如果需要：
- 在 `updateStore` 中添加 `skippedVersion` 状态
- 检查更新时，如果 `latest.version === skippedVersion`，不弹窗
- UpdateDialog 添加"跳过此版本"按钮

---

## 七、故障排查

### 7.1 客户端不弹更新提示

**检查清单**：
1. `latest.json` 的 `version` 是否大于客户端当前版本？
2. `latest.json` 是否能正常访问？（浏览器打开 URL 验证）
3. GitHub Pages 是否已激活？（Settings → Pages 查看状态）
4. 客户端启动后是否等了 5 秒以上？
5. 网络是否能访问 GitHub Pages？

### 7.2 下载失败

**可能原因**：
- gh-proxy 不可用 → 手动改 `latest.json` 的 URL 为其他镜像或直连
- 网络问题 → 检查代理设置
- GitHub Release 被删除 → 重新创建 Release

### 7.3 签名验证失败

**可能原因**：
- `.sig` 文件内容不正确 → 重新构建发布
- `tauri.conf.json` 的 `pubkey` 与私钥不匹配 → 检查公钥是否被误改
- 安装包被篡改 → 从 GitHub Release 重新下载验证

### 7.4 构建时签名失败

**错误信息**：`A public key has been found, but no private key`

**解决**：
- 确认 `TAURI_SIGNING_PRIVATE_KEY` 环境变量已设置（是私钥内容，不是路径）
- 脚本会自动从 `~/.tauri/ai-subtrans.key` 读取内容并设置环境变量
- 如果手动构建，需要自己设置：
  ```cmd
  set TAURI_SIGNING_PRIVATE_KEY=<私钥文件内容>
  set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<密码>
  ```

### 7.5 GitHub API 调用失败

**错误信息**：`GitHub API POST .../releases 失败: 401`

**解决**：
- `GITHUB_TOKEN` 未设置或已过期
- Token 权限不足，需要 `repo` 权限
- 仓库地址与 Token 账号不匹配

### 7.6 gh-pages 推送失败

**错误信息**：`GitHub API PUT .../contents/latest.json 失败: 404`

**解决**：
- `gh-pages` 分支不存在 → 按 §2.3 创建
- 分支名拼写错误 → 确认是 `gh-pages` 不是 `gh_pages`

---

## 八、文件清单

| 文件 | 作用 |
| --- | --- |
| `src-tauri/Cargo.toml` | 添加 `tauri-plugin-updater` 依赖 |
| `src-tauri/tauri.conf.json` | updater 配置 + 公钥 + endpoint |
| `src-tauri/capabilities/default.json` | updater 权限 |
| `src-tauri/src/lib.rs` | 注册 updater 插件 |
| `src-tauri/src/ipc.rs` | `check_for_update` + `download_and_install_update` 命令 |
| `src/lib/api.ts` | 前端 API 封装 |
| `src/stores/updateStore.ts` | 更新状态管理 |
| `src/components/UpdateDialog.tsx` | 更新弹窗组件 |
| `src/App.tsx` | 启动时自动检查 + 渲染弹窗 |
| `src/views/SettingsView.tsx` | 关于页"检查更新"按钮 |
| `src/locales/zh.json` | 中文文案 |
| `src/locales/en.json` | 英文文案 |
| `scripts/publish.mjs` | 发布脚本（本地全流程 / CI --set-version / CI --update-latest） |
| `.github/workflows/release.yml` | GitHub Actions 双平台云端发布工作流 |
| `~/.tauri/ai-subtrans.key` | 私钥（不入仓库，CI 中存为 GitHub Secret） |
