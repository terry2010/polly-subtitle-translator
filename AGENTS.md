# AI 助手开发规范

## 文件写入规范（防止 GUI OOM）

写入文件时必须**一段一段地写入**，不能一次性写入整个文件的全部内容。

一次性写入大文件会导致 GUI OOM（内存溢出），工具会崩溃或卡死。

不要使用 subagent ！ 使用subagent 会导致 任务失败！ GUI OOM 崩溃！


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

