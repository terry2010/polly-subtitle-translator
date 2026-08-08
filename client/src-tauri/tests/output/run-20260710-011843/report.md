# E2E 测试报告

**运行时间**: 2026-07-10 01:18:43

**总用时**: 9分57秒

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng | 543 | ❌ failed | 16/3/5 |

## Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng ❌

- ✅ **entry_count** (L1): 条目数 543，序号唯一递增
- ✅ **timeline_validity** (L1): 543 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 543 条往返一致
- ✅ **[NP] translated_entry_count** (L1): 条目数一致: 543
- ✅ **[NP] translated_timeline** (L1): 时间轴全部对齐
- ✅ **[NP] translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **[NP] subtitle_shift** (L1): 423 条空译文（可能是平移或降级失败）: [120, 121, 122, 123, 124]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **[NP] empty_translations** (L2): 423 条空译文: [120, 121, 122, 123, 124]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ✅ **[NP] fake_translations** (L2): 无假翻译
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ✅ **[NP] name_consistency** (L2): 人名一致，无残留标签
- ✅ **[NP] length_ratio** (L2): 译文长度全部在合理范围
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ⚠️ **[NP] truncation_check** (L2): 7 条疑似截断: [(15, "句子数 2→1"), (18, "长度比 0.26"), (31, "长度比 0.27"), (43, "句末标点缺失"), (61, "长度比 0.26")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ✅ **[NP] translate_failures** (L2): 失败 0 条, 缓存 0 条, token 19141
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 111 pass, 9 fail, 0 shift (共 120 条判定, 问题批次: [2, 3])
  - 相关代码: 27b judge 翻译质量问题
- ❌ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕问题数不一致: 翻译时 failed=0+missing=357=357, 重新加载 failed=0+missing=356=356, 差异条目: [401]
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ❌ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕问题数不一致: 翻译时 failed=0+missing=357=357, 重新加载 failed=0+missing=356=356, 差异条目: [401]
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ❌ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕问题数不一致: 翻译时 failed=0+missing=357=357, 重新加载 failed=0+missing=356=356, 差异条目: [401]
  - 相关代码: subtitle.rs export_subtitle / split_bilingual
- ✅ **[NP] repeated_open_1** (L3): 第 1 次打开一致: 132 条命中, failed=0, missing=355
- ✅ **[NP] repeated_open_2** (L3): 第 2 次打开一致: 132 条命中, failed=0, missing=355
- ✅ **[NP] repeated_open_3** (L3): 第 3 次打开一致: 132 条命中, failed=0, missing=355
- ❌ **[NP] code_bug_stopped** (L3): 批次 4 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
