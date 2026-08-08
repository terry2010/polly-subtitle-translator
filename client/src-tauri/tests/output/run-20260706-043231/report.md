# E2E 测试报告

**运行时间**: 2026-07-06 04:32:31

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| rick_and_morty | 560 | ❌ failed | 7/5/2 |

## rick_and_morty ❌

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 4 条空译文（可能是平移或降级失败）: [19, 20, 60, 61]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 4 条空译文: [19, 20, 60, 61]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ⚠️ **fake_translations** (L2): 假翻译 17 条 (3.04%)
  - 相关代码: translate.rs prompt 模板
- ❌ **cjk_check** (L2): 24 条译文无 CJK 字符: [30, 31, 32, 33, 39]
  - 相关代码: translate.rs prompt 或模型不支持中文
- ⚠️ **sound_effect_consistency** (L2): 5 条音效标记不一致: [(192, false, true), (327, false, true), (437, false, true)]
  - 相关代码: translate.rs prompt 音效标记规则
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **length_ratio** (L2): 11 条译文长度异常: [(109, 30, 4, 0.13333333333333333), (149, 34, 5, 0.14705882352941177), (155, 60, 3, 0.05)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ⚠️ **translate_failures** (L2): 失败 33 条, 缓存 0 条, token 52142
  - 相关代码: translate.rs translate_batch_with_fallback
