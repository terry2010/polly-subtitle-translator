# E2E 测试报告

**运行时间**: 2026-07-09 14:13:45

**总用时**: 86分8秒

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| clarksons_farm | 1054 | ❌ failed | 13/6/5 |

## clarksons_farm ❌

- ✅ **entry_count** (L1): 条目数 1054，序号唯一递增
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 1054
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 274 条空译文（可能是平移或降级失败）: [780, 781, 782, 783, 784]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **empty_translations** (L2): 274 条空译文: [780, 781, 782, 783, 784]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ⚠️ **fake_translations** (L2): 假翻译 29 条 (2.75%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **length_ratio** (L2): 1 条译文长度异常: [(618, 8, 1, 0.125)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 156 条疑似截断: [(16, "句末标点缺失, 长度比 0.29"), (21, "句末标点缺失"), (24, "句子数 3→1"), (25, "句子数 2→1"), (39, "长度比 0.27")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 29 条, 缓存 19 条, token 64952 | 详情: #750: "therefore the nutrition\\nin the grass is much higher," → "therefore the nutrition\\nin the grass is much higher,"; #751: "for example like the sugars and so on." → "for example like the sugars and so on."; #752: "So therefore,\\nactually the cows might actually like it," → "So therefore,\\nactually the cows might actually like it,"; #753: "'cause it's more palatable for them." → "'cause it's more palatable for them."; #754: "Where I think now it's gonna be\\nlike eating a bit of cardboard." → "Where I think now it's gonna be\\nlike eating a bit of cardboard."; #756: "[Jeremy] <i>In the grand scheme\\nof things, though,</i>" → "[Jeremy] <i>In the grand scheme\\nof things, though,</i>"; #757: "<i>Kaleb's silage problems were quite small.</i>" → "<i>Kaleb's silage problems were quite small.</i>"; #758: "<i>Because harvest was now approaching</i>" → "<i>Because harvest was now approaching</i>"; #759: "<i>and I was seriously worried about it.</i>" → "<i>and I was seriously worried about it.</i>"; #760: "<i>We'd had the driest spring\\nfor over a hundred years.</i>" → "<i>We'd had the driest spring\\nfor over a hundred years.</i>"; #761: "<i>In early summer,\\na drought had been formally declared.</i>" → "<i>In early summer,\\na drought had been formally declared.</i>"; #762: "<i>And in the five months\\nsince we'd planted the spring crops</i>" → "<i>And in the five months\\nsince we'd planted the spring crops</i>"; #763: "<i>we'd had 70% less rain than average.</i>" → "<i>we'd had 70% less rain than average.</i>"; #764: "<i>Consequently,\\nmy pre-harvest crop walk with Charlie</i>" → "<i>Consequently,\\nmy pre-harvest crop walk with Charlie</i>"; #765: "<i>was a grim affair.</i>" → "<i>was a grim affair.</i>"; #766: "- [Jeremy] Onions and beetroots.\\n- [Charlie] Yeah." → "- [Jeremy] Onions and beetroots.\\n- [Charlie] Yeah."; #767: "[Jeremy] Or, as I like to call it,\\nno onions or beetroots." → "[Jeremy] Or, as I like to call it,\\nno onions or beetroots."; #768: "What the bloody hell's gone wrong?" → "What the bloody hell's gone wrong?"; #769: "- [Charlie] Well...\\n- We planted this twice, remember." → "- [Charlie] Well...\\n- We planted this twice, remember."; #770: "We've given it 1.6 million onion seeds" → "We've given it 1.6 million onion seeds"; #771: "- and five have grown.\\n- Yeah." → "- and five have grown.\\n- Yeah."; #772: "[Jeremy] Sorry, sorry, sorry, I'm wrong." → "[Jeremy] Sorry, sorry, sorry, I'm wrong."; #773: "Seven have grown, there's two there." → "Seven have grown, there's two there."; #774: "It is very disappointing,\\n'cause you put all that effort in." → "It is very disappointing,\\n'cause you put all that effort in."; #775: "- [Jeremy] I know.\\n- Well, the machine did." → "- [Jeremy] I know.\\n- Well, the machine did."; #776: "And I feel sad actually\\nbecause the RoboDroid" → "And I feel sad actually\\nbecause the RoboDroid"; #777: "is a fascinating piece of equipment." → "is a fascinating piece of equipment."; #778: "And everyone will just go:\\n\"Well, that's rubbish.\"" → "And everyone will just go:\\n\"Well, that's rubbish.\""; #779: "But the truth of the matter is\\nit just hasn't rained." → "But the truth of the matter is\\nit just hasn't rained."
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 638 pass, 115 fail, 26 shift (共 779 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=751, failed=0, missing=290 (翻译时 failed=29, missing=290)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=751, failed=0, missing=290 (翻译时 failed=29, missing=290)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=751, failed=0, missing=290 (翻译时 failed=29, missing=290)
- ❌ **repeated_open_1** (L3): 第 1 次打开问题数不一致: 翻译时=290, 恢复后=285 (缓存命中 788 条), 差异条目: [841, 975, 990, 994, 1049]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_2** (L3): 第 2 次打开问题数不一致: 翻译时=290, 恢复后=285 (缓存命中 788 条), 差异条目: [841, 975, 990, 994, 1049]
  - 相关代码: translate.rs get_cached_entries
- ❌ **repeated_open_3** (L3): 第 3 次打开问题数不一致: 翻译时=290, 恢复后=285 (缓存命中 788 条), 差异条目: [841, 975, 990, 994, 1049]
  - 相关代码: translate.rs get_cached_entries
- ❌ **code_bug_stopped** (L3): 批次 26 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
