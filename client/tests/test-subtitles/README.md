# 测试字幕样例

本目录包含精译服务的 E2E 测试字幕样例。每个文件都是**小规模虚拟字幕**（15-25 条），覆盖 P1-1 source_gate 的所有检测场景，测试成本低。

## 文件清单

| 文件 | 条目数 | source_lang | target_lang | is_bilingual | is_sdh | skip_translation | 用途 |
|------|--------|-------------|-------------|--------------|--------|------------------|------|
| `monolingual_en.srt` | 25 | en | zh | false | false | false | 纯单语英文，正常翻译 |
| `bilingual_zh_en.srt` | 25 | zh+en | zh | true | false | false | 中英双语，提取中文行 |
| `sdh_en.srt` | 25 | en | zh | false | true | false | SDH 英文，含音效+说话人 |
| `sdh_bilingual.srt` | 25 | zh+en | zh | true | true | false | SDH+双语混合 |
| `source_equals_target.srt` | 10 | zh | zh | false | false | true | 源=目标，跳过翻译 |
| `bilingual_gray_zone.srt` | 25 | zh+en | zh | false | false | false | 双语占比 56%，灰色区间 |
| `forced_narrative.srt` | 15 | en | zh | false | false | false | 含 forced 标签 |
| `japanese.srt` | 25 | ja | zh | false | false | false | 日文，日→中翻译 |

## P1-1 source_gate 预期检测结果

### monolingual_en.srt
```
source_lang: "en"
is_bilingual: false
bilingual_ratio: 0.0  (0/25 双语条目)
is_sdh: false
sdh_ratio: 0.0
skip_translation: false
warnings: []
```

### bilingual_zh_en.srt
```
source_lang: "zh+en" (或 "zh", 取决于 detect_language 实现)
is_bilingual: true
bilingual_ratio: 1.0  (25/25 双语条目)
is_sdh: false
sdh_ratio: 0.0
skip_translation: false
warnings: []
bilingual_extracted: [中文行数组]
```

### sdh_en.srt
```
source_lang: "en"
is_bilingual: false
bilingual_ratio: 0.0  (0/25 双语条目，所有条目都是单行)
is_sdh: true
sdh_ratio: 0.48  (12/25 条目含音效或说话人标记)
  - 音效条目: 10 条 ([wind], [door], [footsteps], [thunder], [rain], [papers], [clock], [glass], [kettle], [owl], [fire], [wind])
  - 说话人标记条目: 4 条 (SARAH:, JAMES:, SARAH:, JAMES:, SARAH:)
  - 总计含 SDH 特征: 16/25 = 0.64
skip_translation: false
warnings: []
```

### sdh_bilingual.srt
```
source_lang: "zh+en"
is_bilingual: true
bilingual_ratio: 1.0  (25/25 双语条目，每条都是中文行+英文行)
is_sdh: true
sdh_ratio: 0.64  (16/25 条目含音效或说话人标记)
skip_translation: false
warnings: []
bilingual_extracted: [中文行数组]
```

### source_equals_target.srt
```
source_lang: "zh"
target_lang: "zh"
is_bilingual: false
bilingual_ratio: 0.0
is_sdh: false
sdh_ratio: 0.0
skip_translation: true
skip_reason: "source_equals_target"
warnings: []
```

### bilingual_gray_zone.srt
```
source_lang: "zh+en"
is_bilingual: false  (灰色区间不判定为双语)
bilingual_ratio: 0.56  (14/25 双语条目)
is_sdh: false
sdh_ratio: 0.0
skip_translation: false
warnings: ["bilingual_ratio_in_gray_zone"]
```

### forced_narrative.srt
```
source_lang: "en"
is_bilingual: false
bilingual_ratio: 0.0
is_sdh: false
sdh_ratio: 0.0
skip_translation: false
has_forced_narrative: true
forced_count: 5  (条目 1, 4, 7, 10, 14 含 forced 标签)
warnings: []
```

### japanese.srt
```
source_lang: "ja"
is_bilingual: false
bilingual_ratio: 0.0
is_sdh: false
sdh_ratio: 0.0
skip_translation: false
warnings: []
```

## 设计原则

1. **小规模**：每个文件 10-25 条，E2E 测试成本低（< 1 元/文件）
2. **虚拟内容**：全部是虚构对话，无版权问题
3. **覆盖全面**：覆盖 P1-1 所有检测分支（单语/双语/SDH/混合/灰色/源=目标/forced/日文）
4. **可断言**：每个文件的预期检测结果明确，可直接 assert
5. **同主题**：大部分文件用同一对话（博物馆考古发现），便于对比不同类型的检测结果

## 使用方式

```rust
// 在测试中加载
let subtitle = std::fs::read_to_string("tests/test-subtitles/monolingual_en.srt").unwrap();
let result = source_gate::detect(&subtitle, "zh").await;
assert_eq!(result.is_bilingual, false);
assert_eq!(result.is_sdh, false);
assert_eq!(result.skip_translation, false);
```

## 与现有 fixture 的区别

| 现有 fixture (src-tauri/tests/fixtures/) | 本目录 |
|-----------------------------------------|--------|
| 真实字幕，50KB+，600+ 条 | 虚拟字幕，< 5KB，10-25 条 |
| E2E 测试成本高（几元/文件） | E2E 测试成本低（< 1 元/文件） |
| 覆盖场景少（无双语/SDH/日文） | 覆盖所有 source_gate 检测场景 |
| 用于现有翻译流程测试 | 用于精译服务 E2E 测试 |
