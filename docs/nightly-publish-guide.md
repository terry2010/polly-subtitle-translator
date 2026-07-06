# Nightly 每日构建发布指南

> 创建时间：2026-07-06
> 状态：设计中
> 关联功能：Tauri Updater 双通道更新 + GitHub Actions nightly 工作流

---

## 一、整体架构

在已有的正式版（stable）发布基础上，新增 nightly（每日构建）通道。两个通道独立运作，互不干扰。

```
┌──────────────────────────────────────────────────────────────────────┐
│  GitHub 仓库 terry2010/polly-subtitle-translator                      │
│                                                                       │
│  正式版:  push tag v1.0.1 ──→ release.yml 触发                         │
│             ├─ 构建 Win+Mac                                           │
│             ├─ 创建正式 Release（永久保留）                             │
│             └─ 更新 gh-pages/latest.json                              │
│                                                                       │
│  Nightly: gh workflow run nightly.yml ──→ nightly.yml 触发             │
│             ├─ 版本号 = 1.0.1-nightly.20260706.143025（自动生成）       │
│             ├─ 构建 Win+Mac                                           │
│             ├─ 创建 prerelease Release（保留最近 5 个）                 │
│             ├─ 更新 gh-pages/nightly.json                             │
│             └─ 删除第 6 个及更早的 nightly Release + tag               │
│                                                                       │
│  gh-pages 分支:                                                       │
│    ├─ latest.json   ← 正式版清单（只有正式版写入）                      │
│    └─ nightly.json  ← nightly 清单（只有 nightly 写入）                 │
└──────────────────────────────────────────────────────────────────────┘

客户端:
  设置 → 开发者选项 → 最下面「更新通道」
    ├─ 稳定版（默认）→ 检查 latest.json
    └─ 每日构建      → 检查 nightly.json
```

### 与正式版的对比

| | 正式版（Stable） | 每日构建（Nightly） |
| --- | --- | --- |
| 触发方式 | `git tag v* && git push` | `gh workflow run nightly.yml` |
| 版本号 | 手动指定，如 `1.0.1` | 自动生成，如 `1.0.1-nightly.20260706.143025` |
| 要不要改代码版本号 | 要（改三个文件 + 提交） | **不用**（CI 自动从 Cargo.toml 读 + 加时间戳） |
| Release 类型 | 正式 Release | prerelease Release |
| 清单文件 | `latest.json` | `nightly.json` |
| Release 保留 | 永久 | 最近 5 个 |
| 用户入口 | 所有人默认走 stable | 需开启开发者模式才能切换到 nightly |
| 适合人群 | 普通用户 | 测试人员 / 开发者 / 想吃最新功能的用户 |

---

## 二、版本号设计

### 2.1 格式

```
正式版:  1.0.1
Nightly: 1.0.1-nightly.20260706.143025
         │      │            │
         │      │            └─ 时分秒（HHMMSS，保证一天多次不重复、递增）
         │      └─ 日期（YYYYMMDD）
         └─ 基础版本（从 Cargo.toml 读取，和正式版同源）
```

### 2.2 semver 比较规则

```
1.0.1-nightly.20260706.143025  <  1.0.1-nightly.20260706.150000  <  1.0.1
│                                                           │              │
└─ 同一天早上的 nightly                                       └─ 同一天下午   └─ 正式版（最高）
```

- prerelease 标签（`-nightly.xxx`）在 semver 中比正式版**小**
- 时间戳递增 → 新 nightly 比旧 nightly 大 → nightly 用户能收到升级提示
- 正式版比所有 nightly 大 → 不会出现"正式版用户收到 nightly"的问题

### 2.3 tag 命名

| 类型 | tag 示例 |
| --- | --- |
| 正式版 | `v1.0.1` |
| Nightly | `nightly-20260706-143025` |

nightly tag 不带 `v` 前缀，避免误触发 release.yml（它只匹配 `v*`）。

---

## 三、发布操作流程

### 3.1 发布正式版

```bash
# 1. 改版本号（三个文件：package.json / tauri.conf.json / Cargo.toml）
#    可用脚本：node scripts/publish.mjs --set-version 1.0.1

# 2. 提交代码
git add -A
git commit -m "release: 1.0.1"

# 3. 打 tag 并推送
git tag v1.0.1
git push origin main v1.0.1

# → release.yml 自动触发，约 20-30 分钟完成
# → latest.json 更新，所有 stable 用户收到更新提示
```

### 3.2 发布 Nightly

**前提**：已安装 gh CLI 并登录（`gh auth login`）

```bash
# 方式一：命令行触发（推荐）
gh workflow run nightly.yml
# 带更新内容：
gh workflow run nightly.yml -f notes="修复字幕样式问题"

# 方式二：网页触发
# 打开仓库 → Actions → Nightly → Run workflow → 点按钮
```

**不用改版本号，不用提交代码，不用打 tag。** CI 自动：
1. 从 main 分支 checkout 最新代码
2. 读 Cargo.toml 当前版本（如 `1.0.1`）
3. 拼接时间戳生成版本号 `1.0.1-nightly.20260706.143025`
4. 构建 Win+Mac（带签名）
5. 创建 prerelease Release（tag: `nightly-20260706-143025`）
6. 上传安装包 + 签名到 Release
7. 合并双平台签名，更新 `gh-pages/nightly.json`
8. 删除第 6 个及更早的 nightly Release + tag（保留最近 5 个）

### 3.3 查看 Nightly 构建状态

```bash
# 列出最近的 workflow 运行
gh run list --workflow nightly.yml --limit 5

# 实时盯着当前构建（构建完自动退出）
gh run watch

# 看构建日志
gh run view --log

# 只看失败步骤的日志
gh run view <run-id> --log-failed
```

### 3.4 日常开发流程

```
平时开发
  │
  ├─ push 代码到 main（不触发任何构建）
  │
  ├─ 想让测试用户吃最新构建 → gh workflow run nightly.yml
  │     └─ nightly 用户收到更新提示
  │
  └─ 攒够功能要发正式版
        ├─ 改版本号
        ├─ git tag v1.0.x && git push
        └─ stable + nightly 用户都收到更新提示（见 §四 边界情况）
```

---

## 四、客户端通道切换

### 4.1 入口

设置 → 开发者选项 → 最下面「更新通道」

> 只有开启开发者模式（关于页点击版本号 7 下）才能看到此选项。

### 4.2 两个选项

| 通道 | 检查的清单 | 说明 |
| --- | --- | --- |
| 稳定版（默认） | `latest.json` | 只收到正式版更新 |
| 每日构建 | `nightly.json` | 收到 nightly 更新 |

### 4.3 切换后生效时机

切换通道**不需要重启**：
- 手动点"检查更新"→ 立即用新通道检查
- 重启应用 → 启动后 5 秒自动用新通道检查

### 4.4 边界情况

**场景 1：nightly 用户切到稳定版**

```
当前版本: 1.0.1-nightly.20260706.143025
切换到稳定版 → 检查 latest.json
  ├─ latest.json 是 1.0.1
  ├─ 1.0.1 > 1.0.1-nightly.20260706.143025（semver：正式版 > prerelease）
  └─ 提示升级到正式版 1.0.1 ✅
```

**场景 2：正式版用户切到每日构建**

```
当前版本: 1.0.1
切换到每日构建 → 检查 nightly.json
  ├─ nightly.json 是 1.0.1-nightly.20260706.150000
  ├─ 1.0.1-nightly.20260706.150000 < 1.0.1（semver：prerelease < 正式版）
  └─ 不提示更新（因为正式版比这个 nightly 新）
  → 要等下一个更新的 nightly（时间戳更晚的）才会收到提示
```

**场景 3：nightly 用户一直留在每日构建通道**

```
当前版本: 1.0.1-nightly.20260706.143025
检查 nightly.json
  ├─ nightly.json 更新为 1.0.1-nightly.20260706.150000
  ├─ 1.0.1-nightly.20260706.150000 > 1.0.1-nightly.20260706.143025
  └─ 提示升级到新 nightly ✅
```

---

## 五、CI 工作流详解

### 5.1 nightly.yml 结构

```
workflow_dispatch（手动触发）
      ↓
  prepare job
    ├─ checkout main 最新代码
    ├─ 读 Cargo.toml 版本号（如 1.0.1）
    ├─ 生成时间戳（YYYYMMDD.HHMMSS）
    ├─ 拼接完整版本号：1.0.1-nightly.20260706.143025
    └─ 输出 version + notes
      ↓
  build job（Win + Mac 并行）
    ├─ checkout
    ├─ set-version（用 prepare 输出的版本号）
    ├─ 带签名构建
    └─ 上传到 prerelease Release（tag: nightly-20260706-143025）
      ↓
  update-manifest job
    ├─ 从 Release assets 读取所有 .sig
    ├─ 合并双平台签名 → nightly.json
    └─ 推送到 gh-pages 分支
      ↓
  cleanup job
    ├─ 列出所有 tag 以 nightly- 开头的 Release
    ├─ 按 created_at 降序排序
    ├─ 保留前 5 个
    └─ 删除第 6 个及以后的 Release + tag
```

### 5.2 与 release.yml 的区别

| | release.yml | nightly.yml |
| --- | --- | --- |
| 触发 | push tag `v*` + 手动 | 仅手动 |
| 版本号来源 | tag 或手动输入 | Cargo.toml + 时间戳自动生成 |
| Release 类型 | 正式 | prerelease |
| 清单文件 | latest.json | nightly.json |
| 清理 | 不清理 | 保留最近 5 个 |
| tag 前缀 | `v` | `nightly-` |

### 5.3 清理策略

- 按 tag 前缀 `nightly-` 筛选 Release
- 按 `created_at` 降序排序
- 保留前 5 个，删除第 6 个及以后
- 删除 Release 的同时删除对应 tag
- **不会误删**：nightly.json 指向的永远是最新的，在保留范围内

---

## 六、gh-pages 分支文件结构

```
gh-pages/
  ├─ latest.json    ← 正式版清单（release.yml 写入）
  └─ nightly.json   ← nightly 清单（nightly.yml 写入）
```

### nightly.json 格式

```json
{
  "version": "1.0.1-nightly.20260706.143025",
  "notes": "Nightly build 2026-07-06",
  "pub_date": "2026-07-06T14:30:25.000Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "dW50cnVzdGVk...",
      "url": "https://gh-proxy.com/https://github.com/terry2010/polly-subtitle-translator/releases/download/nightly-20260706-143025/AI-SubTrans_1.0.1-nightly.20260706.143025_x64-setup.exe"
    },
    "darwin-aarch64": {
      "signature": "dW50cnVzdGVk...",
      "url": "https://gh-proxy.com/https://github.com/terry2010/polly-subtitle-translator/releases/download/nightly-20260706-143025/AI-SubTrans_1.0.1-nightly.20260706.143025_aarch64.app.tar.gz"
    }
  }
}
```

---

## 七、相关文件清单

| 文件 | 作用 |
| --- | --- |
| `.github/workflows/release.yml` | 正式版 CI 工作流（已存在） |
| `.github/workflows/nightly.yml` | nightly CI 工作流（新增） |
| `scripts/publish.mjs` | 发布脚本，新增 `--nightly` 相关模式 |
| `src-tauri/src/ipc.rs` | 后端 `check_for_update` 按通道选 endpoint |
| `src-tauri/src/lib.rs` | updater 插件注册 |
| `src/stores/devModeStore.ts` | 新增 `updateChannel` 状态 + 持久化 |
| `src/views/SettingsView.tsx` | DeveloperSettings 末尾加更新通道卡片 |
| `src-tauri/tauri.conf.json` | updater endpoints（保留作 fallback） |

---

## 八、首次配置检查清单

- [ ] gh CLI 已安装（`choco install gh`）
- [ ] gh 已登录（`gh auth login`）
- [ ] GitHub Secrets 已配置：`TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- [ ] GitHub Pages 已启用（gh-pages 分支，/(root)）
- [ ] `latest.json` 可访问：`https://terry2010.github.io/polly-subtitle-translator/latest.json`
- [ ] `nightly.json` 占位文件已推送到 gh-pages
- [ ] nightly.yml 已提交到 main 并推送
