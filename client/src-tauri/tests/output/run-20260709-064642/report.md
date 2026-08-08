# E2E 测试报告

**运行时间**: 2026-07-09 06:46:42

**总用时**: 42分47秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| rick_and_morty | 560 | ⚠️ warned | 19/4/0 |

## rick_and_morty ⚠️

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ✅ **subtitle_shift** (L1): 无平移迹象
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 2 条 (0.36%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 73 条疑似截断: [(2, "长度比 0.21"), (14, "长度比 0.28"), (16, "长度比 0.21"), (18, "长度比 0.29"), (20, "长度比 0.30")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 2 条, 缓存 18 条, token 47929 | 详情: #389: "Help? Help?" → "Help? Help?"; #448: "- Leader?\\n- No." → "- Leader?\\n- No."
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 478 pass, 51 fail, 9 shift (共 538 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=540, failed=0, missing=2 (翻译时 failed=2, missing=2)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=540, failed=0, missing=2 (翻译时 failed=2, missing=2)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=540, failed=0, missing=2 (翻译时 failed=2, missing=2)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 560 条命中, failed=2, missing=2
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 560 条命中, failed=2, missing=2
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 560 条命中, failed=2, missing=2
