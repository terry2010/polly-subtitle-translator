# E2E 测试报告

**运行时间**: 2026-07-06 22:36:04

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| rick_s09e07 | 501 | ❌ failed | 9/4/9 |

## rick_s09e07 ❌

- ✅ **entry_count** (L1): 条目数 501，序号唯一递增
- ✅ **timeline_validity** (L1): 501 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 501 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 501
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 262 条空译文（可能是平移或降级失败）: [101, 240, 241, 242, 243]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 262 条空译文: [101, 240, 241, 242, 243]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ⚠️ **fake_translations** (L2): 假翻译 1 条 (0.20%)
  - 相关代码: translate.rs prompt 模板
- ❌ **cjk_check** (L2): 1 条译文无 CJK 字符: [212]
  - 相关代码: translate.rs prompt 或模型不支持中文
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ⚠️ **translate_failures** (L2): 失败 1 条, 缓存 227 条, token 1481
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 199 pass, 38 fail, 3 shift (共 240 条判定, 问题批次: [1, 2, 3, 4, 5, 6])
  - 相关代码: 27b judge 翻译质量问题
- ❌ **bilingual_roundtrip_srt** (L3): SRT 双语字幕问题数不一致: 翻译时 failed=1+missing=274=275, 重新加载 failed=0+missing=274=274, 差异条目: []
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ❌ **bilingual_roundtrip_ass** (L3): ASS 双语字幕问题数不一致: 翻译时 failed=1+missing=274=275, 重新加载 failed=0+missing=274=274, 差异条目: []
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ❌ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕问题数不一致: 翻译时 failed=1+missing=274=275, 重新加载 failed=0+missing=274=274, 差异条目: []
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ❌ **repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=275, 恢复后=54 (缓存命中 497 条)
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=275, 恢复后=54 (缓存命中 497 条)
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=275, 恢复后=54 (缓存命中 497 条)
  - 相关代码: translate.rs get_cached_entries
- ❌ **code_bug_stopped** (L3): 批次 8 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
