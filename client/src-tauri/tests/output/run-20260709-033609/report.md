# E2E 测试报告

**运行时间**: 2026-07-09 03:36:09

**总用时**: 16分24秒

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| clarksons_farm | 1054 | ❌ failed | 14/5/5 |

## clarksons_farm ❌

- ✅ **entry_count** (L1): 条目数 1054，序号唯一递增
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1054
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 844 条空译文（可能是平移或降级失败）: [210, 211, 212, 213, 214]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 844 条空译文: [210, 211, 212, 213, 214]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ⚠️ **fake_translations** (L2): 假翻译 1 条 (0.09%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 39 条疑似截断: [(16, "句末标点缺失, 长度比 0.29"), (22, "句末标点缺失, 句子数 3→2"), (23, "句子数 2→1"), (24, "句子数 3→2"), (25, "句子数 2→1")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 1 条, 缓存 3 条, token 17462 | 详情: #29: "- That's unbelievable.\\n- So," → "- That's unbelievable.\\n- So,"
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 171 pass, 32 fail, 8 shift (共 211 条判定, 问题批次: [1, 3, 4, 5, 6])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=209, failed=0, missing=792 (翻译时 failed=1, missing=792)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=209, failed=0, missing=792 (翻译时 failed=1, missing=792)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=209, failed=0, missing=792 (翻译时 failed=1, missing=792)
- ❌ **repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=792, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=792, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=792, 恢复后=973 (缓存命中 0 条), 差异条目: [6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26, 27]
  - 相关代码: translate.rs get_cached_entries
- ❌ **code_bug_stopped** (L3): 批次 7 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
