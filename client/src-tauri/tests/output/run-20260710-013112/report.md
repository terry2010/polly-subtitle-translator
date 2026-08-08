# E2E 测试报告

**运行时间**: 2026-07-10 01:31:12

**总用时**: 43分0秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng | 543 | ⚠️ warned | 20/3/0 |

## Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng ⚠️

- ✅ **entry_count** (L1): 条目数 543，序号唯一递增
- ✅ **timeline_validity** (L1): 543 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 543 条往返一致
- ✅ **[NP] translated_entry_count** (L1): 条目数一致: 543
- ✅ **[NP] translated_timeline** (L1): 时间轴全部对齐
- ✅ **[NP] translated_format_roundtrip** (L1): 翻译后格式往返一致
- ✅ **[NP] subtitle_shift** (L1): 无平移迹象
- ✅ **[NP] empty_translations** (L2): 无空译文
- ✅ **[NP] fake_translations** (L2): 无假翻译
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ⚠️ **[NP] name_consistency** (L2): 3 条译文残留 <name> 标签: [428, 443, 541]
  - 相关代码: translate.rs post_process_name_tags / extract_name_tags
- ✅ **[NP] length_ratio** (L2): 译文长度全部在合理范围
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ⚠️ **[NP] truncation_check** (L2): 53 条疑似截断: [(15, "句子数 2→1"), (31, "长度比 0.27"), (34, "长度比 0.28"), (39, "长度比 0.15"), (43, "句末标点缺失")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ✅ **[NP] translate_failures** (L2): 失败 0 条, 缓存 19 条, token 79898
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 501 pass, 42 fail, 0 shift (共 543 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 13, 14, 15, 16, 17, 18])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=535, failed=0, missing=0 (翻译时 failed=0, missing=0)
- ✅ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=535, failed=0, missing=0 (翻译时 failed=0, missing=0)
- ✅ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=535, failed=0, missing=0 (翻译时 failed=0, missing=0)
- ✅ **[NP] repeated_open_1** (L3): 第 1 次打开一致: 543 条命中, failed=0, missing=0
- ✅ **[NP] repeated_open_2** (L3): 第 2 次打开一致: 543 条命中, failed=0, missing=0
- ✅ **[NP] repeated_open_3** (L3): 第 3 次打开一致: 543 条命中, failed=0, missing=0
