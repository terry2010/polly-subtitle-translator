# E2E 测试报告

**运行时间**: 2026-07-08 14:00:45

**总用时**: 38分42秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.eng | 560 | ⚠️ warned | 19/4/0 |

## Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.eng ⚠️

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ✅ **subtitle_shift** (L1): 无平移迹象
- ✅ **empty_translations** (L2): 无空译文
- ✅ **fake_translations** (L2): 无假翻译
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ⚠️ **sound_effect_consistency** (L2): 1 条音效标记不一致: [(272, false, true)]
  - 相关代码: translate.rs prompt 音效标记规则
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 83 条疑似截断: [(4, "长度比 0.27"), (8, "长度比 0.23"), (23, "长度比 0.24"), (35, "句子数 2→1"), (41, "长度比 0.28, 句子数 4→3")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 2 条, 缓存 18 条, token 30675 | 详情: #39: "UGG! Glugg UGG!" → "UGG！Glugg UGG！"; #272: "from our mothers!\\n[ Mup crying ]" → "[Mup 哭泣声]"
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 467 pass, 56 fail, 7 shift (共 530 条判定, 问题批次: [1, 2, 3, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 17, 18, 19])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=540, failed=0, missing=20 (翻译时 failed=2, missing=20)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=540, failed=0, missing=20 (翻译时 failed=2, missing=20)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=540, failed=0, missing=20 (翻译时 failed=2, missing=20)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 560 条命中, failed=2, missing=20
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 560 条命中, failed=2, missing=20
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 560 条命中, failed=2, missing=20
