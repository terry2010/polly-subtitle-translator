# E2E 测试报告

**运行时间**: 2026-07-09 05:25:02

**总用时**: 77分42秒

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| clarksons_farm | 1054 | ❌ failed | 16/4/3 |

## clarksons_farm ❌

- ✅ **entry_count** (L1): 条目数 1054，序号唯一递增
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1054
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ✅ **subtitle_shift** (L1): 无平移迹象
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 2 条 (0.19%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 239 条疑似截断: [(16, "句末标点缺失, 长度比 0.29"), (21, "句末标点缺失"), (25, "句子数 2→1"), (34, "长度比 0.29"), (64, "句末标点缺失")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 3 条, 缓存 29 条, token 95784 | 详情: #266: "- and I just tuned into all the birds, so.\\n- You had what?" → "- and I just tuned into all the birds, so.\\n- You had what?"; #826: "\"Harveest\"." → "“Harveest”。"; #873: "- Yeah.\\n- Lisa can drive a tractor." → "- Yeah.\\n- Lisa can drive a tractor."
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 871 pass, 154 fail, 28 shift (共 1053 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=1051, failed=0, missing=3 (翻译时 failed=3, missing=3)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=1051, failed=0, missing=3 (翻译时 failed=3, missing=3)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=1051, failed=0, missing=3 (翻译时 failed=3, missing=3)
- ❌ **repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=3, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=3, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=3, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
