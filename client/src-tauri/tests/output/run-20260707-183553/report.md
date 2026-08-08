# E2E 测试报告

**运行时间**: 2026-07-07 18:35:53

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| 1782294137861 | 1984 | ❌ failed | 18/4/1 |

## 1782294137861 ❌

- ✅ **entry_count** (L1): 条目数 1984，序号唯一递增
- ✅ **timeline_validity** (L1): 1984 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1984 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1984
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ✅ **subtitle_shift** (L1): 无平移迹象
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 20 条 (1.01%)
  - 相关代码: translate.rs prompt 模板
- ❌ **cjk_check** (L2): 12 条译文无 CJK 字符: [1944, 1945, 1947, 1948, 1967]
  - 相关代码: translate.rs prompt 或模型不支持中文
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 404 条疑似截断: [(2, "长度比 0.27, 句子数 2→1"), (7, "句子数 2→1"), (20, "句子数 2→1"), (22, "句子数 2→1"), (31, "句子数 2→1")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 17 条, 缓存 97 条, token 125258
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 1779 pass, 155 fail, 50 shift (共 1984 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 18, 19, 20, 21, 22, 23, 24, 25, 27, 28, 29, 30, 31, 32, 34, 35, 36, 37, 38, 40, 41, 42, 43, 44, 46, 47, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=1955, failed=0, missing=29 (翻译时 failed=17, missing=29)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=1955, failed=0, missing=29 (翻译时 failed=17, missing=29)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=1955, failed=0, missing=29 (翻译时 failed=17, missing=29)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 1984 条命中, failed=17, missing=29
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 1984 条命中, failed=17, missing=29
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 1984 条命中, failed=17, missing=29
