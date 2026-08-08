# E2E 测试报告

**运行时间**: 2026-07-06 19:01:02

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| rick_s09e07 | 342 | ❌ failed | 9/4/2 |

## rick_s09e07 ❌

- ✅ **entry_count** (L1): 条目数 342，序号唯一递增
- ✅ **timeline_validity** (L1): 342 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 342 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 342
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 7 条空译文（可能是平移或降级失败）: [24, 121, 130, 242, 278]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 7 条空译文: [24, 121, 130, 242, 278]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ⚠️ **fake_translations** (L2): 假翻译 2 条 (0.58%)
  - 相关代码: translate.rs prompt 模板
- ❌ **cjk_check** (L2): 2 条译文无 CJK 字符: [137, 234]
  - 相关代码: translate.rs prompt 或模型不支持中文
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ⚠️ **translate_failures** (L2): 失败 9 条, 缓存 0 条, token 38218
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 300 pass, 31 fail, 11 shift (共 342 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
  - 相关代码: 27b judge 翻译质量问题
