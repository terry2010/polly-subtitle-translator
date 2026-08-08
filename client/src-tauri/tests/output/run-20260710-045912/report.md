# E2E 测试报告

**运行时间**: 2026-07-10 04:59:12

**总用时**: 47分7秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng | 543 | ⚠️ warned | 17/6/0 |

## Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng ⚠️

- ✅ **entry_count** (L1): 条目数 543，序号唯一递增
- ✅ **timeline_validity** (L1): 543 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 543 条往返一致
- ✅ **[NP] translated_entry_count** (L1): 条目数一致: 543
- ✅ **[NP] translated_timeline** (L1): 时间轴全部对齐
- ✅ **[NP] translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **[NP] subtitle_shift** (L1): 1 条空译文（可能是平移或降级失败）: [266]
  - 相关代码: translate.rs translate_batch_with_fallback
- ✅ **[NP] empty_translations** (L2): 无空译文
- ⚠️ **[NP] fake_translations** (L2): 假翻译 3 条 (0.55%)
  - 相关代码: translate.rs prompt 模板
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ✅ **[NP] name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **[NP] length_ratio** (L2): 1 条译文长度异常: [(505, 14, 2, 0.14285714285714285)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ⚠️ **[NP] truncation_check** (L2): 69 条疑似截断: [(2, "句子数 2→1"), (15, "句子数 2→1"), (18, "长度比 0.26"), (26, "句子数 2→1"), (31, "长度比 0.27")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **[NP] translate_failures** (L2): 失败 8 条, 缓存 19 条, token 64353 | 详情: #1: "Ah, ugh." → "Ah，ugh。"; #253: "c-cut, cut through\\nthe prison level..." → "c-cut, cut through\\nthe prison level..."; #266: "Unh! Unh!\\nUnh! Unh!" → "\\n"; #355: "Wooooh!\\nOoohahh! Ohh!" → "Wooooh!\\nOoohahh！Ohh!"; #366: "F-u-u-u-u-uck!" → "F-u-u-u-u-uck!"; #387: "Ooh-hoo-ho-hoo!\\nHo-ho-ho!" → "Ooh-hoo-ho-hoo!\\nHa-ha-ha!"; #414: "Aaaah! Aaaaah! Unf!" → "Aaaah!Aaaaah!Unf!"; #541: "Georgie Boy!\\nGeorgie Boy!" → "Georgie Boy!"
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 421 pass, 116 fail, 6 shift (共 543 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=526, failed=0, missing=8 (翻译时 failed=8, missing=8)
- ✅ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=526, failed=0, missing=8 (翻译时 failed=8, missing=8)
- ✅ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=526, failed=0, missing=8 (翻译时 failed=8, missing=8)
- ✅ **[NP] repeated_open_1** (L3): 第 1 次打开一致: 543 条命中, failed=8, missing=8, 排除 44 条重复原文
- ✅ **[NP] repeated_open_2** (L3): 第 2 次打开一致: 543 条命中, failed=8, missing=8, 排除 44 条重复原文
- ✅ **[NP] repeated_open_3** (L3): 第 3 次打开一致: 543 条命中, failed=8, missing=8, 排除 44 条重复原文
