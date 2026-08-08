# E2E 测试报告

**运行时间**: 2026-07-08 20:16:50

**总用时**: 86分44秒

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Clarksons Farm S05E01 Recovering 2160p AMZN WEB-DL DDP5 1 H 265-RAWR.eng | 992 | ❌ failed | 21/2/1 |

## Clarksons Farm S05E01 Recovering 2160p AMZN WEB-DL DDP5 1 H 265-RAWR.eng ❌

- ✅ **entry_count** (L1): 条目数 992，序号唯一递增
- ✅ **timeline_validity** (L1): 992 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 992 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 992
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ✅ **subtitle_shift** (L1): 无平移迹象
- ✅ **empty_translations** (L2): 无空译文
- ✅ **fake_translations** (L2): 无假翻译
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 183 条疑似截断: [(17, "长度比 0.30"), (19, "句末标点缺失"), (26, "句末标点缺失"), (37, "长度比 0.29"), (40, "长度比 0.26")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ✅ **translate_failures** (L2): 失败 0 条, 缓存 29 条, token 63449
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 760 pass, 165 fail, 7 shift (共 932 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=991, failed=0, missing=0 (翻译时 failed=0, missing=0)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=992, failed=0, missing=0 (翻译时 failed=0, missing=0)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=991, failed=0, missing=0 (翻译时 failed=0, missing=0)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 992 条命中, failed=0, missing=0
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 992 条命中, failed=0, missing=0
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 992 条命中, failed=0, missing=0
- ❌ **code_bug_stopped** (L3): 批次 34 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
