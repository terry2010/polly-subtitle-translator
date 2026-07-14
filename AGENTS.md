# AI 助手开发规范

## 文件写入规范（防止 GUI OOM）

写入文件时必须**一段一段地写入**，不能一次性写入整个文件的全部内容。

一次性写入大文件会导致 GUI OOM（内存溢出），工具会崩溃或卡死。

禁止使用 subagent，**仅允许 `reviewer` 只读评审 agent 和 `auditor` 甲方专家审计 agent 作为例外**。其他 subagent（如 `subagent_explore`、`subagent_general` 等）会导致任务失败、GUI OOM 崩溃。

`reviewer` 和 `auditor` 例外的原因：它们都是只读角色，不修改文件、不 spawn 其他 subagent、不向用户提问，不会触发 OOM。


### 正确做法

1. 先用 `write` 工具写入文件的第一段内容（如文件头、imports、第一个类/函数）
2. 再用 `edit` 工具逐段追加后续内容

### 分段阈值

- 超过 **200 行**的文件，必须分 **3 次以上**写入
- 每次写入建议 ≤ 150 行
- 写入前先估算行数，规划分段点（按类/函数/逻辑块切分）

### 示例

```text
# 错误 ❌
write(file_path="big_file.rs", content="<800 行完整内容>")

# 正确 ✅
write(file_path="big_file.rs", content="<第 1 段：imports + 结构体定义>")
edit(file_path="big_file.rs", old_string="// === SECTION 1 END ===",
     new_string="// === SECTION 1 END ===\n<第 2 段：方法实现>")
edit(file_path="big_file.rs", old_string="// === SECTION 2 END ===",
     new_string="// === SECTION 2 END ===\n<第 3 段：测试>")
```

### 实施要点

- 在 `write` 的内容末尾留一个明确的分隔标记注释（如 `// === SECTION N END ===`），便于后续 `edit` 用唯一锚点追加
- 不要用文件末尾的空行作为锚点，容易因空白处理不一致导致 `old_string` 不唯一
- 若文件已存在且需大改，先 `read` 全文，再分段 `edit`，不要 `write` 覆盖

## 需求规模分级（收到需求后第一步判断）

收到需求后，先判断需求规模，决定走哪条流程：

| 级别 | 判断标准 | 流程 |
|------|---------|------|
| 轻量 | 单文件改动、无新功能、无新公共 API、无数据结构变更。例：改 typo、改样式、改文案、加日志、修小 bug | 开发 → 测试 → 完成 |
| 完整 | 有 design doc，或涉及新功能、跨模块、数据结构变更、架构改动 | 拆分功能点 → 逐个（开发 → 测试 → 文档确认 → reviewer）→ auditor 审计 → 完成 |

**判断规则**：
- 拿不准时，按完整流程处理。
- 主 agent 判断为轻量时，必须说明理由（改了什么、为什么不需要 reviewer/auditor）。
- 用户可以随时 override 级别。

## 功能点推进规则（完整流程适用）

1. **收到需求后，第一件事是拆分功能点**：
   - 读取 design doc，按"修改清单"中的独立模块/文件组拆分功能点
   - 每个功能点 = 一个可独立测试、独立验收的最小交付单元
   - 用 `todo_write` 工具记录功能点清单，每个功能点一个 todo 项
   - 输出功能点清单给用户确认后再开始开发

2. **逐个功能点推进，禁止跨功能点开发**：
   - 一次只开发一个功能点（一个 todo 项）
   - 该功能点完成开发 + 测试 + 文档确认后，**立即调用 reviewer 评审**
   - reviewer 通过后，标记该 todo 项为 completed，才能开始下一个 todo 项
   - reviewer 不通过时，必须修复后重新 review，禁止跳到下一个功能点

3. **禁止的行为**：
   - 禁止把整个需求文档当成一个功能点
   - 禁止把所有功能点开发完后再一次性 review
   - 禁止跳过功能点拆分直接开始写代码

## 功能点开发流程（完整流程，不可跳过任何环节）

```text
┌─────────────────────────────────────────────────────────────┐
│  功能点开发流程（不可跳过任何环节）                              │
│                                                             │
│  1. 开发                                                     │
│     ├── 按 design doc 规格实现代码                             │
│     ├── 遵循现有代码风格和架构约定                                │
│     └── 实现过程中记录与 design doc 的偏差（如有）                 │
│                                                             │
│  2. 单元测试                                                  │
│     ├── 为每个公开 API 编写单元测试                              │
│     ├── 覆盖正常路径 + 边界 case + 错误路径                       │
│     └── cargo test / vitest 全部通过                          │
│                                                             │
│  3. 集成测试（如涉及多模块交互）                                  │
│     ├── 用 mock SSH server 测试 SSH 相关逻辑                   │
│     └── 验证模块间接口契约                                      │
│                                                             │
│  4. E2E 测试                                                 │
│     ├── 模拟真实用户操作流程                                     │
│     ├── 覆盖 design doc 中的用户故事和任务流                       │
│     └── Tauri WebDriver / Playwright 驱动 GUI                 │
│                                                             │
│  5. 对比文档确认                                                │
│     ├── 逐条核对 design doc 中该功能点的验收标准                    │
│     ├── 列出已实现 / 未实现 / 偏差项                              │
│     └── 未实现项必须补完，偏差项需记录理由                           │
│                                                             │
│  6. reviewer 评审（强制，每个功能点完成后立即执行）                  │
│     ├── 调用 `reviewer` 只读评审 agent                           │
│     ├── 逐条核对验收标准并输出带证据的报告                           │
│     ├── 测试未通过或验收标准不满足：评审不通过                         │
│     └── 不通过时必须返回第 1 步继续开发，直至 reviewer 通过            │
│                                                             │
│  7. 进入下一功能点                                              │
│     ├── 上一功能点 100% 通过 reviewer 评审后才能开始                 │
│     └── 如还有未完成的下一个功能点，返回第 1 步继续开发                │
│                                                             │
│  8. 甲方专家审计（所有功能点完成后执行）                            │
│     ├── 调用 `auditor` 甲方专家审计 agent                          │
│     ├── 对照 design doc 做整体验收（功能/架构/数据流/风险）            │
│     ├── 审计不通过：返回第 1 步修复问题，直至 auditor 通过              │
│     └── 审计通过：项目开发完成                                    │
└─────────────────────────────────────────────────────────────┘
```

**关键约束**：

- `reviewer` 不通过时，**禁止进入下一功能点**。
- `reviewer` 不通过时，必须返回第 1 步继续开发，修复问题后重新跑完整流程（2-6）。
- 主 agent 不能自己判定"差不多可以了"来跳过 reviewer。
- 主 agent 必须为每个功能点提供**变更清单**：
  - 新增文件
  - 修改文件（注明修改的函数/逻辑）
  - 新增/修改的测试
  - 与 design doc 的偏差项及理由
- reviewer 不依赖 `git diff`（多轮开发后才提交时 git diff 不可靠），基于**变更清单 + 验收标准**定位审查范围。
- 缺少变更清单时，reviewer 直接报告"无法评审：缺少功能点变更清单"并终止。
- **所有功能点通过 reviewer 评审后，必须调用 `auditor` 做整体验收审计**，禁止跳过。
- `auditor` 不通过时，必须返回第 1 步修复问题，修复后重新审计，直至 auditor 通过。
- 主 agent 不能自己判定"项目已完成"来跳过 auditor。
- 调用 `auditor` 时，主 agent 必须提供**开发完成状态总结**：
  - 所有功能点清单及各自的 reviewer 评审结论
  - 所有功能点的变更清单
  - 已知的偏差项和遗留问题
- 缺少开发完成状态总结时，auditor 直接报告"无法审计：缺少开发完成状态总结"并终止。

## libmpv 窗口销毁与 OLE 拖放（Windows）

### 问题

反复创建/销毁 libmpv 子窗口后，Tauri 主窗口的拖放（OLE Drag & Drop）失效。

### 根因

`mpv_terminate_destroy` 内部会调用 `OleUninitialize`/`CoUninitialize`，减少进程的 OLE 引用计数。反复销毁后引用计数归零，OLE 被卸载，Tauri 主窗口的拖放系统失效。

### 修复

1. 应用启动时（`lib.rs` 的 `run()`）调用 `OleInitialize`，永不调用 `OleUninitialize`
2. `Player::destroy` 中 `mpv_terminate_destroy` 前后各调用一次 `OleInitialize`，确保引用计数只增不减

### 预防

- 第三方库操作窗口/COM/OLE 时，注意其内部可能调用 `OleInitialize`/`OleUninitialize`/`CoInitialize`/`CoUninitialize`
- 反复创建/销毁原生窗口时，在销毁前后配对调用 `OleInitialize`，不调用 `OleUninitialize`
- "N 次操作后"出现的问题通常指向引用计数或资源泄漏

## 自动更新（Tauri Updater）

### 架构

- 使用 `tauri-plugin-updater` 实现自动更新
- 签名密钥：`~/.tauri/ai-subtrans.key`（私钥）+ `tauri.conf.json` 里的 `pubkey`（公钥）
- 更新清单：`latest.json` 托管在 GitHub Pages（`gh-pages` 分支）
- 安装包存储：GitHub Releases
- 国内加速：`latest.json` 里的 URL 用 `gh-proxy.com` 前缀

### 发布流程

```
node scripts/publish.mjs <版本号> "更新内容"
```

脚本自动完成：改版本号 → 带签名构建 → 创建 GitHub Release → 上传 .exe + .sig → 更新 latest.json

### 环境变量

- `GITHUB_TOKEN`：GitHub Personal Access Token（repo 权限）
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：私钥密码

### 客户端行为

- 启动后 5 秒静默检查更新
- 有新版本时弹窗显示版本信息和更新内容
- 用户确认后下载安装包（显示进度/速度/ETA），验证签名，静默安装
- 安装完成后提示重启
- 设置页"关于"分区有"检查更新"按钮可手动触发

### 注意事项

- 私钥丢了就无法发布更新，务必备份
- `TAURI_SIGNING_PRIVATE_KEY` 需要传私钥内容（不是路径），脚本会自动读取文件
- `--build-only` 参数可只构建不发布（本地测试用）

## 构建与测试命令

### Rust 后端

```bash
# 编译检查
cd src-tauri && cargo check --lib

# 运行单元测试（260 个）
cd src-tauri && cargo test --lib

# Clippy 检查（约 28 个 warning，主要是函数参数过多/复杂类型等风格问题）
cd src-tauri && cargo clippy --lib

# 前端类型检查
npx tsc --noEmit
```

### 翻译质量验证

翻译完字幕后，用 Python 脚本检查质量：

```bash
# 检查 CJK 空格异常（GLM-5.2 常见问题，24-45% 的条目受影响）
# cleanup_cjk_spaces() 函数在后处理中自动修复
python3 -c "
import re
def is_cjk_char(c):
    code = ord(c)
    return (0x4E00 <= code <= 0x9FFF) or (0x3000 <= code <= 0x303F) or (0xFF00 <= code <= 0xFFEF)
def cleanup_cjk_spaces(s):
    chars = list(s); result = []; i = 0
    while i < len(chars):
        result.append(chars[i])
        if is_cjk_char(chars[i]):
            j = i + 1
            while j < len(chars) and chars[j] == ' ': j += 1
            if j < len(chars) and is_cjk_char(chars[j]): i = j
            else: i += 1
        else: i += 1
    return ''.join(result)
# ... 读取 srt 文件，对每条 zh 行调用 cleanup_cjk_spaces 对比
"
```

### 日志位置

- 应用日志：`~/Library/Application Support/com.zimufan.ai-subtrans/logs/`
- API 调试日志（流式响应）：`~/Library/Application Support/com.zimufan.ai-subtrans/api_debug/`

## 已知模型问题与修复

### GLM-5.2 thinking 泄漏

- **问题**：GLM-5.2 在 SiliconFlow 上 `enable_thinking:false` 间歇性失效，thinking 内容泄漏到 `content` 字段
- **修复**：改用 `thinking.type: "disabled"` 参数（所有 GLM 模型），如仍检测到 thinking 则升级为双参数模式
- **文件**：`translate_ai.rs` 的 `ThinkingStyle` 枚举

### CJK 字符间异常空格

- **问题**：GLM-5.2 在英文单词边界处插入空格，即使输出是中文（如 `我 知道 我带了`）
- **修复**：`cleanup_cjk_spaces()` 函数移除 CJK 字符之间的空格，保留 CJK 与非 CJK（拉丁字母、标签）之间的空格
- **文件**：`translate_utils.rs`，在 `post_process_name_tags` 和 `ipc.rs` 中调用

### 音效标记半翻译

- **问题**：`[ All grunting ]` → `[ 所有人发出 grunt 声 ]`，音效标记内的英文单词未翻译
- **修复**：`is_partial_sound_effect()` 函数检测音效标记内的半翻译，触发降级重试
- **文件**：`translate_utils.rs`，在 `translate_ai.rs` 批次处理中调用
