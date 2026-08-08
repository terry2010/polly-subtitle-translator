# E2E 测试报告

**运行时间**: 2026-07-10 02:56:50

**总用时**: 48分43秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng | 543 | ⚠️ warned | 15/8/0 |

## Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng ⚠️

- ✅ **entry_count** (L1): 条目数 543，序号唯一递增
- ✅ **timeline_validity** (L1): 543 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 543 条往返一致
- ✅ **[NP] translated_entry_count** (L1): 条目数一致: 543
- ✅ **[NP] translated_timeline** (L1): 时间轴全部对齐
- ✅ **[NP] translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **[NP] subtitle_shift** (L1): 1 条译文长度比值异常（可能合并/截断）: [(453, 0.125)]
  - 相关代码: translate.rs batch 翻译逻辑
- ✅ **[NP] empty_translations** (L2): 无空译文
- ⚠️ **[NP] fake_translations** (L2): 假翻译 4 条 (0.74%)
  - 相关代码: translate.rs prompt 模板
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ⚠️ **[NP] sound_effect_consistency** (L2): 1 条音效标记不一致: [(461, false, true)]
  - 相关代码: translate.rs prompt 音效标记规则
- ⚠️ **[NP] name_consistency** (L2): 11 条译文残留 <name> 标签: [116, 335, 345]
  - 相关代码: translate.rs post_process_name_tags / extract_name_tags
- ⚠️ **[NP] length_ratio** (L2): 1 条译文长度异常: [(453, 16, 2, 0.125)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ⚠️ **[NP] truncation_check** (L2): 61 条疑似截断: [(15, "句子数 2→1"), (31, "长度比 0.27"), (35, "句子数 2→1"), (61, "长度比 0.26"), (80, "长度比 0.29")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **[NP] translate_failures** (L2): 失败 5 条, 缓存 19 条, token 63221 | 详情: #165: "Ugh!" → "ugh!"; #366: "F-u-u-u-u-uck!" → "F-u-u-u-u-uck!"; #397: "Ooh-ugh-ah!" → "Ooh-ugh-ah!"; #461: "but you're the real d-d-deal." → "[机器轰鸣声渐弱]"; #475: "I like him.\\nMe too." → "I like him.\\nMe too."
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 445 pass, 121 fail, 7 shift (共 573 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=528, failed=0, missing=5 (翻译时 failed=5, missing=5)
- ✅ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=528, failed=0, missing=5 (翻译时 failed=5, missing=5)
- ✅ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=528, failed=0, missing=5 (翻译时 failed=5, missing=5)
- ✅ **[NP] repeated_open_1** (L3): 第 1 次打开一致: 543 条命中, failed=5, missing=5
- ✅ **[NP] repeated_open_2** (L3): 第 2 次打开一致: 543 条命中, failed=5, missing=5
- ✅ **[NP] repeated_open_3** (L3): 第 3 次打开一致: 543 条命中, failed=5, missing=5
