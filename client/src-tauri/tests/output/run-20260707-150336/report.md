# E2E 测试报告

**运行时间**: 2026-07-07 15:03:36

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Clarksons Farm S05E07 Sickening 2160p AMZN WEB-DL DDP5 1 H 265-FLUX.eng | 1054 | ⚠️ warned | 16/7/0 |

## Clarksons Farm S05E07 Sickening 2160p AMZN WEB-DL DDP5 1 H 265-FLUX.eng ⚠️

- ✅ **entry_count** (L1): 条目数 1054，序号唯一递增
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1054
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 2 条空译文（可能是平移或降级失败）: [200, 809]
  - 相关代码: translate.rs translate_batch_with_fallback
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 1 条 (0.09%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ⚠️ **sound_effect_consistency** (L2): 3 条音效标记不一致: [(173, true, false), (208, false, true), (717, true, false)]
  - 相关代码: translate.rs prompt 音效标记规则
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **length_ratio** (L2): 1 条译文长度异常: [(805, 62, 9, 0.14516129032258066)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 266 条疑似截断: [(7, "长度比 0.26, 句子数 2→1"), (8, "长度比 0.24"), (9, "句子数 2→1"), (15, "长度比 0.16"), (16, "长度比 0.18")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 7 条, 缓存 29 条, token 65270
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 774 pass, 167 fail, 23 shift (共 964 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=1047, failed=0, missing=7 (翻译时 failed=7, missing=7)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=1047, failed=0, missing=7 (翻译时 failed=7, missing=7)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=1047, failed=0, missing=7 (翻译时 failed=7, missing=7)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 1052 条命中, failed=5, missing=7
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 1052 条命中, failed=5, missing=7
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 1052 条命中, failed=5, missing=7
