# E2E 测试报告

**运行时间**: 2026-07-07 05:20:24

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.eng | 560 | ❌ failed | 17/3/2 |

## Rick and Morty S09E05 1080p AMZN WEB-DL DUAL DDP5 1 H 264-TURG.eng ❌

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 501 条空译文（可能是平移或降级失败）: [2, 60, 61, 62, 63]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 500 条空译文: [60, 61, 62, 63, 64]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ✅ **fake_translations** (L2): 无假翻译
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ⚠️ **translate_failures** (L2): 失败 2 条, 缓存 0 条, token 4360
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 51 pass, 9 fail, 0 shift (共 60 条判定, 问题批次: [1])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=54, failed=0, missing=506 (翻译时 failed=2, missing=506)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=54, failed=0, missing=506 (翻译时 failed=2, missing=506)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=54, failed=0, missing=506 (翻译时 failed=2, missing=506)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 72 条命中, failed=0, missing=506
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 72 条命中, failed=0, missing=506
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 72 条命中, failed=0, missing=506
- ❌ **code_bug_stopped** (L3): 批次 2 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
