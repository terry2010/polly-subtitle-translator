# E2E 测试报告

**运行时间**: 2026-07-07 20:56:24

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| 1782294137861 | 1984 | ❌ failed | 14/5/5 |

## 1782294137861 ❌

- ✅ **entry_count** (L1): 条目数 1984，序号唯一递增
- ✅ **timeline_validity** (L1): 1984 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1984 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1984
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 784 条空译文（可能是平移或降级失败）: [1200, 1201, 1202, 1203, 1204]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 784 条空译文: [1200, 1201, 1202, 1203, 1204]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ⚠️ **fake_translations** (L2): 假翻译 4 条 (0.20%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 283 条疑似截断: [(2, "长度比 0.27, 句子数 2→1"), (7, "句子数 2→1"), (10, "长度比 0.23, 句子数 2→1"), (20, "句子数 2→1"), (22, "句子数 2→1")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 5 条, 缓存 32 条, token 77696
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 1030 pass, 85 fail, 55 shift (共 1170 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 9, 10, 12, 13, 14, 15, 17, 18, 19, 20, 21, 22, 23, 24, 25, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=1195, failed=0, missing=789 (翻译时 failed=5, missing=789)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=1195, failed=0, missing=789 (翻译时 failed=5, missing=789)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=1195, failed=0, missing=789 (翻译时 failed=5, missing=789)
- ❌ **repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=789, 恢复后=1983 (缓存命中 2 条), 差异条目: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=789, 恢复后=1983 (缓存命中 2 条), 差异条目: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=789, 恢复后=1983 (缓存命中 2 条), 差异条目: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]
  - 相关代码: translate.rs get_cached_entries
- ❌ **code_bug_stopped** (L3): 批次 40 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
