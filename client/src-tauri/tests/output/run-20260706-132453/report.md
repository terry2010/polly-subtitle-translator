# E2E 测试报告

**运行时间**: 2026-07-06 13:24:53

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| rick_and_morty | 560 | ⚠️ warned | 10/4/0 |

## rick_and_morty ⚠️

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 30 条译文长度比值异常（可能合并/截断）: [(30, 7.0), (31, 7.0), (32, 7.0)]
  - 相关代码: translate.rs batch 翻译逻辑
- ✅ **empty_translations** (L2): 无空译文
- ✅ **fake_translations** (L2): 无假翻译
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ⚠️ **sound_effect_consistency** (L2): 1 条音效标记不一致: [(20, false, true)]
  - 相关代码: translate.rs prompt 音效标记规则
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **length_ratio** (L2): 30 条译文长度异常: [(30, 2, 14, 7.0), (31, 2, 14, 7.0), (32, 2, 14, 7.0)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ⚠️ **translate_failures** (L2): 失败 1 条, 缓存 557 条, token 299
  - 相关代码: translate.rs translate_batch_with_fallback
