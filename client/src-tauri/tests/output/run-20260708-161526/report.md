# E2E 测试报告

**运行时间**: 2026-07-08 16:15:26

**总用时**: 20分48秒

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng | 543 | ❌ failed | 15/4/5 |

## Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng ❌

- ✅ **entry_count** (L1): 条目数 543，序号唯一递增
- ✅ **timeline_validity** (L1): 543 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 543 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 543
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 273 条空译文（可能是平移或降级失败）: [270, 271, 272, 273, 274]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 273 条空译文: [270, 271, 272, 273, 274]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ✅ **fake_translations** (L2): 无假翻译
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 19 条疑似截断: [(18, "长度比 0.29"), (26, "句子数 2→1"), (31, "长度比 0.27"), (61, "长度比 0.26"), (80, "长度比 0.26")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 1 条, 缓存 5 条, token 18458 | 详情: #165: "Ugh!" → "ugh!"
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 228 pass, 42 fail, 0 shift (共 270 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=264, failed=0, missing=279 (翻译时 failed=1, missing=279)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=264, failed=0, missing=279 (翻译时 failed=1, missing=279)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=264, failed=0, missing=279 (翻译时 failed=1, missing=279)
- ❌ **repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=279, 恢复后=509 (缓存命中 34 条), 差异条目: [30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=279, 恢复后=509 (缓存命中 34 条), 差异条目: [30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=279, 恢复后=509 (缓存命中 34 条), 差异条目: [30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49]
  - 相关代码: translate.rs get_cached_entries
- ❌ **code_bug_stopped** (L3): 批次 9 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
