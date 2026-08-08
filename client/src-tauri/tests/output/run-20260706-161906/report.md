# E2E 测试报告

**运行时间**: 2026-07-06 16:19:06

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| rick_and_morty | 560 | ⚠️ warned | 12/3/0 |

## rick_and_morty ⚠️

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 11 条译文长度比值异常（可能合并/截断）: [(47, 0.11428571428571428), (135, 0.11904761904761904), (149, 0.14705882352941177)]
  - 相关代码: translate.rs batch 翻译逻辑
- ✅ **empty_translations** (L2): 无空译文
- ✅ **fake_translations** (L2): 无假翻译
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **length_ratio** (L2): 11 条译文长度异常: [(47, 35, 4, 0.11428571428571428), (135, 42, 5, 0.11904761904761904), (149, 34, 5, 0.14705882352941177)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ✅ **translate_failures** (L2): 失败 0 条, 缓存 1990 条, token 0
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 417 pass, 86 fail, 27 shift (共 530 条判定, 问题批次: [5, 6, 8, 9, 10, 13, 17])
  - 相关代码: 27b judge 翻译质量问题
