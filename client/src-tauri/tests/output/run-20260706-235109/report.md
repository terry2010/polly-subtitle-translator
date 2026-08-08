# E2E 测试报告

**运行时间**: 2026-07-06 23:51:09

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Clarksons Farm S05E07 Sickening 2160p AMZN WEB-DL DDP5 1 H 265-FLUX.eng | 1054 | ❌ failed | 10/4/8 |

## Clarksons Farm S05E07 Sickening 2160p AMZN WEB-DL DDP5 1 H 265-FLUX.eng ❌

- ✅ **entry_count** (L1): 条目数 1054，序号唯一递增
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1054
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 787 条空译文（可能是平移或降级失败）: [188, 198, 200, 270, 271]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 787 条空译文: [188, 198, 200, 270, 271]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ✅ **fake_translations** (L2): 无假翻译
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ⚠️ **sound_effect_consistency** (L2): 2 条音效标记不一致: [(246, false, true), (253, false, true)]
  - 相关代码: translate.rs prompt 音效标记规则
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ⚠️ **translate_failures** (L2): 失败 5 条, 缓存 5 条, token 20461
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 213 pass, 50 fail, 7 shift (共 270 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8])
  - 相关代码: 27b judge 翻译质量问题
- ❌ **bilingual_roundtrip_srt** (L3): SRT 双语字幕问题数不一致: 翻译时 failed=5+missing=789=794, 重新加载 failed=0+missing=789=789, 差异条目: []
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ❌ **bilingual_roundtrip_ass** (L3): ASS 双语字幕问题数不一致: 翻译时 failed=5+missing=789=794, 重新加载 failed=0+missing=789=789, 差异条目: []
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ❌ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕问题数不一致: 翻译时 failed=5+missing=789=794, 重新加载 failed=0+missing=789=789, 差异条目: []
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ❌ **repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=794, 恢复后=776 (缓存命中 278 条)
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=794, 恢复后=776 (缓存命中 278 条)
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=794, 恢复后=776 (缓存命中 278 条)
  - 相关代码: translate.rs get_cached_entries
- ❌ **code_bug_stopped** (L3): 批次 9 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
