# E2E 测试报告

**运行时间**: 2026-07-09 18:13:08

**总用时**: 105分57秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| clarksons_farm | 1054 | ⚠️ warned | 17/6/0 |

## clarksons_farm ⚠️

- ✅ **entry_count** (L1): 条目数 1054，序号唯一递增
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1054
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 1 条译文长度比值异常（可能合并/截断）: [(618, 0.125)]
  - 相关代码: translate.rs batch 翻译逻辑
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 11 条 (1.04%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **length_ratio** (L2): 1 条译文长度异常: [(618, 8, 1, 0.125)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 237 条疑似截断: [(16, "句末标点缺失, 长度比 0.29"), (21, "句末标点缺失"), (24, "句子数 3→1"), (25, "句子数 2→1"), (64, "句末标点缺失")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 11 条, 缓存 29 条, token 90231 | 详情: #140: "<i>♪ Rain on me ♪</i>" → "<i>♪ Rain on me ♪</i>"; #144: "\"Cool, cool rain\"!" → "\"Cool, cool rain\"!"; #147: "[Jeremy] Cool, cool rain!" → "[Jeremy] Cool, cool rain!"; #150: "<i>we decided to release Endgame\\nback into the fields</i>" → "<i>we decided to release Endgame\\nback into the fields</i>"; #151: "<i>to join the rest of the herd.</i>" → "<i>to join the rest of the herd.</i>"; #153: "- Endgame, give us a hand.\\n- [Kaleb panting]" → "- Endgame, give us a hand.\\n- [Kaleb panting]"; #154: "- [Kaleb] Yeah, push, buddy.\\n- Come on!" → "- [Kaleb] Yeah, push, buddy.\\n- Come on!"; #155: "- [Kaleb] Push!\\n- You can do it!" → "- [Kaleb] Push!\\n- You can do it!"; #156: "- [mooing loudly]\\n- [Kaleb] Go on, buddy." → "- [mooing loudly]\\n- [Kaleb] Go on, buddy."; #157: "- [Jeremy whispering] Look at him.\\n- Looks fucking good. Watch out..." → "- [Jeremy whispering] Look at him.\\n- Looks fucking good. Watch out..."; #158: "He looks smart, doesn't he?\\nHe's done well over the winter." → "He looks smart, doesn't he?\\nHe's done well over the winter."
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 851 pass, 170 fail, 7 shift (共 1028 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 33, 34, 35])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=1043, failed=0, missing=11 (翻译时 failed=11, missing=10)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=1043, failed=0, missing=11 (翻译时 failed=11, missing=10)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=1043, failed=0, missing=11 (翻译时 failed=11, missing=10)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 1054 条命中, failed=11, missing=10
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 1054 条命中, failed=11, missing=10
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 1054 条命中, failed=11, missing=10
