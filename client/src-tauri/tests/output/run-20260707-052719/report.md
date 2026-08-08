# E2E 测试报告

**运行时间**: 2026-07-07 05:27:19

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.eng | 560 | ⚠️ warned | 18/3/0 |

## Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.eng ⚠️

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 5 条空译文（可能是平移或降级失败）: [85, 149, 190, 329, 423]
  - 相关代码: translate.rs translate_batch_with_fallback
- ✅ **empty_translations** (L2): 无空译文
- ✅ **fake_translations** (L2): 无假翻译
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ⚠️ **translate_failures** (L2): 失败 7 条, 缓存 18 条, token 38096
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 439 pass, 91 fail, 0 shift (共 530 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17, 18, 19])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=535, failed=0, missing=25 (翻译时 failed=7, missing=25)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=535, failed=0, missing=25 (翻译时 failed=7, missing=25)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=535, failed=0, missing=25 (翻译时 failed=7, missing=25)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 555 条命中, failed=2, missing=25
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 555 条命中, failed=2, missing=25
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 555 条命中, failed=2, missing=25
