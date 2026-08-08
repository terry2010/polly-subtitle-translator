# E2E 测试报告

**运行时间**: 2026-07-07 03:24:50

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Clarksons Farm S05E07 Sickening 2160p AMZN WEB-DL DDP5 1 H 265-FLUX.eng | 1054 | ❌ failed | 15/4/2 |

## Clarksons Farm S05E07 Sickening 2160p AMZN WEB-DL DDP5 1 H 265-FLUX.eng ❌

- ✅ **entry_count** (L1): 条目数 1054，序号唯一递增
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1054
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 9 条空译文（可能是平移或降级失败）: [200, 307, 409, 615, 779]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 9 条空译文: [200, 307, 409, 615, 779]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ⚠️ **fake_translations** (L2): 假翻译 1 条 (0.09%)
  - 相关代码: translate.rs prompt 模板
- ❌ **cjk_check** (L2): 1 条译文无 CJK 字符: [826]
  - 相关代码: translate.rs prompt 或模型不支持中文
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ⚠️ **translate_failures** (L2): 失败 22 条, 缓存 947 条, token 87706
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 824 pass, 164 fail, 10 shift (共 998 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 33, 34, 35, 36])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=1043, failed=0, missing=11 (翻译时 failed=10, missing=11)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=1043, failed=0, missing=11 (翻译时 failed=10, missing=11)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=1043, failed=0, missing=11 (翻译时 failed=10, missing=11)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 1045 条命中, failed=1, missing=11
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 1045 条命中, failed=1, missing=11
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 1045 条命中, failed=1, missing=11
