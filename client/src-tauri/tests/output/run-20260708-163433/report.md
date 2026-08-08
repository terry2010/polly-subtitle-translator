# E2E 测试报告

**运行时间**: 2026-07-08 16:34:33

**总用时**: 97分34秒

**结果**: 0 通过 / 1 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng | 543 | ⚠️ warned | 19/4/0 |

## Rick and Morty S09E04 A Ricker Runs Through It 1080p AMZN WEB-DL DDP5 1 H 264-Kitsune.eng ⚠️

- ✅ **entry_count** (L1): 条目数 543，序号唯一递增
- ✅ **timeline_validity** (L1): 543 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 543 条往返一致
- ✅ **translated_entry_count** (L1): 条目数一致: 543
- ✅ **translated_timeline** (L1): 时间轴全部对齐
- ✅ **translated_format_roundtrip** (L1): 翻译后格式往返一致
- ✅ **subtitle_shift** (L1): 无平移迹象
- ✅ **empty_translations** (L2): 无空译文
- ⚠️ **fake_translations** (L2): 假翻译 20 条 (3.68%)
  - 相关代码: translate.rs prompt 模板
- ✅ **cjk_check** (L2): 译文均含 CJK 字符
- ✅ **sound_effect_consistency** (L2): 音效标记一致
- ✅ **name_consistency** (L2): 人名一致，无残留标签
- ✅ **length_ratio** (L2): 译文长度全部在合理范围
- ✅ **alignment_check** (L2): 无错位迹象
- ⚠️ **truncation_check** (L2): 49 条疑似截断: [(18, "长度比 0.26"), (26, "句子数 2→1"), (31, "长度比 0.27"), (61, "长度比 0.26"), (90, "长度比 0.17, 句子数 2→1")]
  - 相关代码: translate.rs prompt 或 batch 翻译逻辑
- ⚠️ **translate_failures** (L2): 失败 22 条, 缓存 19 条, token 38082 | 详情: #165: "Ugh!" → "ugh!"; #166: "Rick?" → "Rick?"; #182: "'cause I was never\\nreally living." → "'cause I was never\\nreally living."; #183: "[ Slurring ] He's got all\\nmy passwords, Morty." → "[ Slurring ] He's got all\\nmy passwords, Morty."; #184: "You can't just -- he's not\\na guy, he's -- he's a wallet." → "You can't just -- he's not\\na guy, he's -- he's a wallet."; #196: "[ Intercom static ]\\nReese is my <i>friend!</i>" → "[ Intercom static ]\\nReese is my <i>friend!</i>"; #197: "Rick:\\n<i>He's not </i>alive, <i>Morty!</i>" → "Rick:\\n<i>He's not </i>alive, <i>Morty!</i>"; #198: "He's-- he's a program\\nthat unlocks my computer" → "He's-- he's a program\\nthat unlocks my computer"; #200: "<i>You better open</i>\\n<i>that hatch, Morty!</i>" → "<i>You better open</i>\\n<i>that hatch, Morty!</i>"; #205: "I mean, what happens\\nwhen Rick reels us in?" → "I mean, what happens\\nwhen Rick reels us in?"; #206: "Guess I can drop\\nthe folksy act." → "Guess I can drop\\nthe folksy act."; #207: "No, I-I like it.\\nIt's part of the charm." → "No, I-I like it.\\nIt's part of the charm."; #208: "Well, then where are\\nwe casting off?" → "Well, then where are\\nwe casting off?"; #209: "There's an old\\nlandline portal down here." → "There's an old\\nlandline portal down here."; #248: "Noooo..." → "Noooo..."; #355: "Wooooh!\\nOoohahh! Ohh!" → "Wooooh!\\nOoohahh! Ohh!"; #366: "F-u-u-u-u-uck!" → "F-u-u-u-u-uck!"; #368: "- Wh-o-o-o-oa!\\n- Aaaah!" → "- Wh-o-o-o-oa!\\n- Aaaah!"; #378: "Badass." → "badass。"; #387: "Ooh-hoo-ho-hoo!\\nHo-ho-ho!" → "Ooh-hoo-ho-hoo!\\nHa-ha-ha!"; #397: "Ooh-ugh-ah!" → "Ooh-ugh-ah!"; #398: "Whoa-ho-ho-ho!" → "Whoa-ho-ho-ho!"
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **judge_27b** (L5): 27b judge: 468 pass, 73 fail, 2 shift (共 543 条判定, 问题批次: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19])
  - 相关代码: 27b judge 翻译质量问题
- ✅ **bilingual_roundtrip_srt** (L3): SRT 双语字幕往返一致: translated=511, failed=0, missing=32 (翻译时 failed=22, missing=32)
- ✅ **bilingual_roundtrip_ass** (L3): ASS 双语字幕往返一致: translated=511, failed=0, missing=32 (翻译时 failed=22, missing=32)
- ✅ **bilingual_roundtrip_vtt** (L3): VTT 双语字幕往返一致: translated=511, failed=0, missing=32 (翻译时 failed=22, missing=32)
- ✅ **repeated_open_1** (L3): 第 1 次打开一致: 543 条命中, failed=22, missing=32
- ✅ **repeated_open_2** (L3): 第 2 次打开一致: 543 条命中, failed=22, missing=32
- ✅ **repeated_open_3** (L3): 第 3 次打开一致: 543 条命中, failed=22, missing=32
