# E2E 测试报告

**运行时间**: 2026-07-08 13:18:56

**总用时**: 41分29秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.eng | 560 | ⚠️ warned | 16/7/0 |

## Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.eng ⚠️

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 1 条译文长度比值异常（可能合并/截断）: [(502, 0.13793103448275862)]
  - 相关代码: translate.rs batch 翻译逻辑
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 1 条 (0.18%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ⚠️ **sound_effect_consistency** (L2): 2 条音效标记不一致: [(272, false, true), (502, false, true)]
  - 相关代码: translate.rs prompt 音效标记规则
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **length_ratio** (L2): 1 条译文长度异常: [(502, 29, 4, 0.13793103448275862)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 87 条疑似截断: [(2, "长度比 0.21"), (3, "句子数 2→1"), (4, "长度比 0.27"), (7, "句子数 2→1"), (8, "长度比 0.29")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 5 条, 缓存 18 条, token 31148 | 详情: #39: "UGG! Glugg UGG!" → "UGG！Glugg UGG！"; #105: "Nipslip Vodka." → "Nipslip Vodka。"; #272: "from our mothers!\\n[ Mup crying ]" → "[穆普哭泣]"; #310: "'cause I'm pissed\\nat Snowball," → "'cause I'm pissed\\nat Snowball,"; #502: "to go around, huh?\\n[ Laughs ]" → "[笑声]"
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 469 pass, 82 fail, 9 shift (共 560 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=537, failed=0, missing=23 (翻译时 failed=5, missing=23)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=537, failed=0, missing=23 (翻译时 failed=5, missing=23)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=537, failed=0, missing=23 (翻译时 failed=5, missing=23)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 560 条命中, failed=5, missing=23
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 560 条命中, failed=5, missing=23
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 560 条命中, failed=5, missing=23
