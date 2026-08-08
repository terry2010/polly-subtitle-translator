# E2E 测试报告

**运行时间**: 2026-07-07 06:07:24

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick.and.Morty.S09E07.1080p.WEB.h264-EDITH[EZTVx.to].eng | 501 | ❌ failed | 16/4/1 |

## Rick.and.Morty.S09E07.1080p.WEB.h264-EDITH[EZTVx.to].eng ❌

- ✅ **entry_count** (L1): 条目数 501，序号唯一递增
- ✅ **timeline_validity** (L1): 501 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 501 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 501
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 3 条空译文（可能是平移或降级失败）: [101, 284, 458]
  - 相关代码: translate.rs translate_batch_with_fallback
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 1 条 (0.20%)
  - 相关代码: translate.rs prompt 模板
- ❌ **cjk_check** (L2): 1 条译文无 CJK 字符: [212]
  - 相关代码: translate.rs prompt 或模型不支持中文
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ⚠️ **translate_failures** (L2): 失败 4 条, 缓存 20 条, token 30281
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 435 pass, 62 fail, 4 shift (共 501 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 17])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=446, failed=0, missing=55 (翻译时 failed=4, missing=55)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=446, failed=0, missing=55 (翻译时 failed=4, missing=55)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=446, failed=0, missing=55 (翻译时 failed=4, missing=55)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 498 条命中, failed=1, missing=55
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 498 条命中, failed=1, missing=55
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 498 条命中, failed=1, missing=55
