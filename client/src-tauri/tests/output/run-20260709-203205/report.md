# E2E 测试报告

**运行时间**: 2026-07-09 20:32:05

**总用时**: 45分6秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| rick_and_morty | 560 | ⚠️ warned | 17/6/0 |

## rick_and_morty ⚠️

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 560
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **subtitle_shift** (L1): 1 条译文长度比值异常（可能合并/截断）: [(2, 0.13793103448275862)]
  - 相关代码: translate.rs batch 翻译逻辑
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 5 条 (0.89%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ⚠️ **length_ratio** (L2): 1 条译文长度异常: [(2, 29, 4, 0.13793103448275862)]
  - 相关代码: translate.rs prompt 或 batch 逻辑
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 76 条疑似截断: [(2, "长度比 0.10"), (3, "句子数 2→1"), (8, "长度比 0.29"), (10, "句子数 2→1"), (16, "长度比 0.28")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 24 条, 缓存 18 条, token 43701 | 详情: #39: "UGG! Glugg UGG!" → "UGG！Glugg UGG！"; #60: "I know you're probably\\ndistracted by the mups." → "I know you're probably\\ndistracted by the mups."; #63: "You know?\\nCh-Chill? Hang?" → "You know?\\nCh-Chill? Hang?"; #272: "from our mothers!\\n[ Mup crying ]" → "from our mothers!\\n[ Mup crying ]"; #273: "I-I probably know\\nsome of this." → "\"I-I probably know\\nsome of this.\""; #274: "They made me watch the\\nmup video. Three times." → "\"They made me watch the\\nmup video. Three times.\""; #275: "\"Mup\" is the language\\nof the oppressor!" → "\"Mup\" is the language\\nof the oppressor!\""; #276: "You're still trapped\\nin their way of thinking!" → "\"You're still trapped\\nin their way of thinking!\""; #277: "Again, just getting out\\nin front of this," → "\"Again, just getting out\\nin front of this,\""; #278: "I'm not looking\\nto get caught up" → "\"I'm not looking\\nto get caught up\""; #279: "in all the colonization,\\neugenics, hierarchies." → "\"in all the colonization,\\neugenics, hierarchies.\""; #281: "Humans kind of started this\\ncycle a billion years ago." → "\"Humans kind of started this\\ncycle a billion years ago.\""; #282: "You don't want me\\ninvolved." → "\"You don't want me\\ninvolved.\""; #283: "So this is <i>your </i>fault?" → "\"So this is <i>your </i>fault?\""; #284: "Not directly enough\\nthat I need to fix it." → "\"Not directly enough\\nthat I need to fix it.\""; #285: "I just come from a world where\\nthe humans are in charge" → "\"I just come from a world where\\nthe humans are in charge\""; #287: "[ Gasps ]\\nHe will help us!" → "[ Gasps ]\\nHe will help us!\""; #290: "- Scared! Scared!\\n- Scared! Scared!" → "\"- Scared! Scared!\\n- Scared! Scared!\""; #293: "Dog commander:\\nSearch everywhere!" → "\"Dog commander:\\nSearch everywhere!\""; #294: "<i>Do you have eyes</i>\\n<i>on Morty?</i>" → "<i>Do you have eyes</i>\\n<i>on Morty?</i>\""; #295: "Commander:\\nNot yet. <i>Find him.</i>" → "\"Commander:\\nNot yet. <i>Find him.</i>\""; #296: "<i>He said we were asshole,</i>\\n<i>racist piece-of-shit bigots!</i>" → "<i>He said we were asshole,</i>\\n<i>racist piece-of-shit bigots!</i>\""; #315: "<i>Yes, General?</i>\\n<i>Awww! Morty!</i>" → "<i>Yes, General?</i>\\n<i>Awww! Morty!</i>"; #323: "Guest house is here." → "Guest house is here."
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 449 pass, 109 fail, 2 shift (共 560 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=518, failed=0, missing=24 (翻译时 failed=24, missing=24)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=518, failed=0, missing=24 (翻译时 failed=24, missing=24)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=518, failed=0, missing=24 (翻译时 failed=24, missing=24)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 560 条命中, failed=24, missing=24
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 560 条命中, failed=24, missing=24
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 560 条命中, failed=24, missing=24
