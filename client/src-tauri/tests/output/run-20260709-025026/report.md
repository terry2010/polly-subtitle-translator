# E2E 测试报告

**运行时间**: 2026-07-09 02:50:26

**总用时**: 15分50秒

**结果**: 0 通过 / 0 警告 / 3 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| clarksons_farm | 1054 | ❌ failed | 16/3/5 |
| rick_and_morty | 560 | ❌ failed | 16/3/5 |
| rick_s09e07 | 501 | ❌ failed | 16/3/5 |

## clarksons_farm ❌

- ✅ **entry_count** (L1): 条目数 1054，序号唯一递增
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致
- ✅ **[NP] translated_entry_count** (L1): 条目数一致: 1054
- ✅ **[NP] translated_timeline** (L1): 时间轴全部对齐
- ✅ **[NP] translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **[NP] subtitle_shift** (L1): 1024 条空译文（可能是平移或降级失败）: [30, 31, 32, 33, 34]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **[NP] empty_translations** (L2): 1024 条空译文: [30, 31, 32, 33, 34]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ✅ **[NP] fake_translations** (L2): 无假翻译
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ✅ **[NP] name_consistency** (L2): 人名一致，无残留标签
- ✅ **[NP] length_ratio** (L2): 译文长度全部在合理范围
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ⚠️ **[NP] truncation_check** (L2): 5 条疑似截断: [(16, "长度比 0.25"), (21, "句末标点缺失"), (23, "句末标点缺失"), (24, "句子数 3→1"), (25, "句子数 2→1")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ✅ **[NP] translate_failures** (L2): 失败 0 条, 缓存 0 条, token 2222
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 26 pass, 4 fail, 0 shift (共 30 条判定, 问题批次: [])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=30, failed=0, missing=951 (翻译时 failed=0, missing=951)
- ✅ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=30, failed=0, missing=951 (翻译时 failed=0, missing=951)
- ✅ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=30, failed=0, missing=951 (翻译时 failed=0, missing=951)
- ❌ **[NP] repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=951, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=951, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=951, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] code_bug_stopped** (L3): 批次 1 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出

## rick_and_morty ❌

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **[NP] translated_entry_count** (L1): 条目数一致: 560
- ✅ **[NP] translated_timeline** (L1): 时间轴全部对齐
- ✅ **[NP] translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **[NP] subtitle_shift** (L1): 530 条空译文（可能是平移或降级失败）: [30, 31, 32, 33, 34]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **[NP] empty_translations** (L2): 530 条空译文: [30, 31, 32, 33, 34]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ✅ **[NP] fake_translations** (L2): 无假翻译
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ✅ **[NP] name_consistency** (L2): 人名一致，无残留标签
- ✅ **[NP] length_ratio** (L2): 译文长度全部在合理范围
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ⚠️ **[NP] truncation_check** (L2): 5 条疑似截断: [(14, "长度比 0.30"), (16, "长度比 0.26"), (20, "长度比 0.30"), (22, "长度比 0.27"), (27, "长度比 0.27")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ✅ **[NP] translate_failures** (L2): 失败 0 条, 缓存 0 条, token 2640
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 25 pass, 5 fail, 0 shift (共 30 条判定, 问题批次: [])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=30, failed=0, missing=451 (翻译时 failed=0, missing=451)
- ✅ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=30, failed=0, missing=451 (翻译时 failed=0, missing=451)
- ✅ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=30, failed=0, missing=451 (翻译时 failed=0, missing=451)
- ❌ **[NP] repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=451, 恢复后=480 (缓存命中 0 条), 差异条目: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=451, 恢复后=480 (缓存命中 0 条), 差异条目: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=451, 恢复后=480 (缓存命中 0 条), 差异条目: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] code_bug_stopped** (L3): 批次 1 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出

## rick_s09e07 ❌

- ✅ **entry_count** (L1): 条目数 501，序号唯一递增
- ✅ **timeline_validity** (L1): 501 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 501 条往返一致
- ✅ **[NP] translated_entry_count** (L1): 条目数一致: 501
- ✅ **[NP] translated_timeline** (L1): 时间轴全部对齐
- ✅ **[NP] translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **[NP] subtitle_shift** (L1): 411 条空译文（可能是平移或降级失败）: [90, 91, 92, 93, 94]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **[NP] empty_translations** (L2): 411 条空译文: [90, 91, 92, 93, 94]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ✅ **[NP] fake_translations** (L2): 无假翻译
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ✅ **[NP] name_consistency** (L2): 人名一致，无残留标签
- ✅ **[NP] length_ratio** (L2): 译文长度全部在合理范围
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ⚠️ **[NP] truncation_check** (L2): 8 条疑似截断: [(1, "长度比 0.21, 句子数 2→1"), (4, "长度比 0.21, 句子数 2→1"), (26, "句末标点缺失"), (42, "长度比 0.23"), (50, "长度比 0.29")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ✅ **[NP] translate_failures** (L2): 失败 0 条, 缓存 4 条, token 6333
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 80 pass, 9 fail, 1 shift (共 90 条判定, 问题批次: [1, 2])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=84, failed=0, missing=310 (翻译时 failed=0, missing=310)
- ✅ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=84, failed=0, missing=310 (翻译时 failed=0, missing=310)
- ✅ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=84, failed=0, missing=310 (翻译时 failed=0, missing=310)
- ❌ **[NP] repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=310, 恢复后=374 (缓存命中 0 条), 差异条目: [1, 4, 5, 6, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 26, 30]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=310, 恢复后=374 (缓存命中 0 条), 差异条目: [1, 4, 5, 6, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 26, 30]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=310, 恢复后=374 (缓存命中 0 条), 差异条目: [1, 4, 5, 6, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 26, 30]
  - 相关代码: translate.rs get_cached_entries
- ❌ **[NP] code_bug_stopped** (L3): 批次 3 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
