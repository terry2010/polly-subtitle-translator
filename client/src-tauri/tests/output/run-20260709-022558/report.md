# E2E 测试报告

**运行时间**: 2026-07-09 02:25:58

**总用时**: 33分3秒

**结果**: 0 通过 / 0 警告 / 3 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| clarksons_farm | 1054 | ❌ failed | 15/4/5 |
| rick_and_morty | 560 | ❌ failed | 15/3/6 |
| rick_s09e07 | 501 | ❌ failed | 15/4/5 |

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
- ⚠️ **[NP] fake_translations** (L2): 假翻译 22 条 (2.09%)
  - 相关代码: translate.rs prompt 模板
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ✅ **[NP] name_consistency** (L2): 人名一致，无残留标签
- ✅ **[NP] length_ratio** (L2): 译文长度全部在合理范围
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ✅ **[NP] truncation_check** (L2): 无截断迹象
- ⚠️ **[NP] translate_failures** (L2): 失败 22 条, 缓存 0 条, token 0 | 详情: #6: "[Jeremy] <i>The FarmDroid\\nhad now been set to work</i>" → "[Jeremy] <i>The FarmDroid\\nhad now been set to work</i>"; #7: "<i>replanting the onion and beetroot field.</i>" → "<i>replanting the onion and beetroot field.</i>"; #8: "<i>But with the sky still stubbornly blue,</i>" → "<i>But with the sky still stubbornly blue,</i>"; #9: "<i>I wasn't sure I could see the point.</i>" → "<i>I wasn't sure I could see the point.</i>"; #11: "[Jeremy] So this planting\\nis all very well," → "[Jeremy] So this planting\\nis all very well,"; #12: "but pointless if it doesn't rain." → "but pointless if it doesn't rain."; #13: "Yeah." → "Yeah."; #14: "[Jeremy] So, it's reckoned" → "[Jeremy] So, it's reckoned"; #15: "ten thousand litres of water\\nover a hectare" → "ten thousand litres of water\\nover a hectare"; #16: "gives you the equivalent\\nof one millimetre of rain." → "gives you the equivalent\\nof one millimetre of rain."; #17: "And it's 24 millimetres" → "And it's 24 millimetres"; #18: "you need every week." → "you need every week."; #20: "You've never seen a James Bond film," → "You've never seen a James Bond film,"; #21: "but I've never seen anyone\\nas captivated..." → "but I've never seen anyone\\nas captivated..."; #22: "- [Kaleb] It's fascinating.\\n- ...as you are by that." → "- [Kaleb] It's fascinating.\\n- ...as you are by that."; #23: "- Could you not just watch that all day?\\n- No..." → "- Could you not just watch that all day?\\n- No..."; #24: "I mean, yeah, five minutes and...\\nBut you... Look at his face!" → "I mean, yeah, five minutes and...\\nBut you... Look at his face!"; #25: "It's really... [chuckling]" → "It's really... [chuckling]"; #26: "- You love that machine, don't you?\\n- Yeah, I do." → "- You love that machine, don't you?\\n- Yeah, I do."; #27: "What gets me is\\nit put 200,000 seeds in yesterday" → "What gets me is\\nit put 200,000 seeds in yesterday"; #28: "and it knows\\nwhere every single one of them is." → "and it knows\\nwhere every single one of them is."; #29: "- That's unbelievable.\\n- So," → "- That's unbelievable.\\n- So,"
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 0 pass, 30 fail, 0 shift (共 30 条判定, 问题批次: [])
  - 相关代码: 27b judge 翻译质量问题
- ❌ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕检测失败: is_bilingual=false, matched=0, total=1054
  - 相关代码: subtitle.rs detect_bilingual
- ❌ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕检测失败: is_bilingual=false, matched=0, total=1054
  - 相关代码: subtitle.rs detect_bilingual
- ❌ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕检测失败: is_bilingual=false, matched=0, total=1054
  - 相关代码: subtitle.rs detect_bilingual
- ✅ **[NP] repeated_open_1** (L3): 第 1 次打开一致: 0 条命中, failed=0, missing=973
- ✅ **[NP] repeated_open_2** (L3): 第 2 次打开一致: 0 条命中, failed=0, missing=973
- ✅ **[NP] repeated_open_3** (L3): 第 3 次打开一致: 0 条命中, failed=0, missing=973
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
- ❌ **[NP] fake_translations** (L2): 假翻译 29 条 (5.2%): [0, 1, 2, 3, 4]
  - 相关代码: translate.rs prompt 模板（强化必须翻译）
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ✅ **[NP] name_consistency** (L2): 人名一致，无残留标签
- ✅ **[NP] length_ratio** (L2): 译文长度全部在合理范围
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ✅ **[NP] truncation_check** (L2): 无截断迹象
- ⚠️ **[NP] translate_failures** (L2): 失败 29 条, 缓存 0 条, token 0 | 详情: #0: "Jerry:\\nUhh, I'm so nervous!" → "Jerry:\\nUhh, I'm so nervous!"; #1: "It's been so long, no one\\neven makes fun of me" → "It's been so long, no one\\neven makes fun of me"; #2: "for being unemployed\\nanymore." → "for being unemployed\\nanymore."; #3: "Aw. We can make fun of you\\nif you want, sweetie." → "Aw. We can make fun of you\\nif you want, sweetie."; #4: "You could also\\ntake one of these." → "You could also\\ntake one of these."; #5: "It's a worry worm." → "It's a worry worm."; #6: "It eats your worries\\nand dies out in a couple hours." → "It eats your worries\\nand dies out in a couple hours."; #7: "I'm on one right now.\\nSuper chill." → "I'm on one right now.\\nSuper chill."; #8: "I'm not taking\\nbreakfast drugs!" → "I'm not taking\\nbreakfast drugs!"; #9: "We doing\\nbreakfast drugs?" → "We doing\\nbreakfast drugs?"; #10: "I'm just dropping\\nMorty off. Deal me in." → "I'm just dropping\\nMorty off. Deal me in."; #11: "Nobody's dealer-ing\\nanyone." → "Nobody's dealer-ing\\nanyone."; #12: "Seriously, guys, this\\ninterview is important to me!" → "Seriously, guys, this\\ninterview is important to me!"; #13: "Wow! Good luck, Dad!" → "Wow! Good luck, Dad!"; #14: "Didn't -- Didn't realize\\nyou were still trying." → "Didn't -- Didn't realize\\nyou were still trying."; #15: "Yeah. Why?" → "Yeah. Why?"; #16: "No one's on your ass\\nto contribute a thing." → "No one's on your ass\\nto contribute a thing."; #17: "Ah, if you're trying to\\nneg me into taking a worm," → "Ah, if you're trying to\\nneg me into taking a worm,"; #18: "you've succeeded." → "you've succeeded."; #19: "Negging works.\\n[ Gulps ]" → "Negging works.\\n[ Gulps ]"; #20: "See you all in a week!\\nMnh." → "See you all in a week!\\nMnh."; #21: "I'm so excited!" → "I'm so excited!"; #22: "Guh-uh, this is what it feels\\nlike to have friends?" → "Guh-uh, this is what it feels\\nlike to have friends?"; #23: "Wouldn't know.\\nPick you up on Sunday?" → "Wouldn't know.\\nPick you up on Sunday?"; #24: "See you then!" → "See you then!"; #25: "- Snowball!\\n- Morty!" → "- Snowball!\\n- Morty!"; #26: "- [ Panting ]\\n- [ Laughs ]" → "- [ Panting ]\\n- [ Laughs ]"; #27: "Oh, my God!\\nI missed you so much!" → "Oh, my God!\\nI missed you so much!"; #28: "Me too, Morty. Me too." → "Me too, Morty. Me too."
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 0 pass, 30 fail, 0 shift (共 30 条判定, 问题批次: [])
  - 相关代码: 27b judge 翻译质量问题
- ❌ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕检测失败: is_bilingual=false, matched=0, total=560
  - 相关代码: subtitle.rs detect_bilingual
- ❌ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕检测失败: is_bilingual=false, matched=0, total=560
  - 相关代码: subtitle.rs detect_bilingual
- ❌ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕检测失败: is_bilingual=false, matched=0, total=560
  - 相关代码: subtitle.rs detect_bilingual
- ✅ **[NP] repeated_open_1** (L3): 第 1 次打开一致: 0 条命中, failed=0, missing=480
- ✅ **[NP] repeated_open_2** (L3): 第 2 次打开一致: 0 条命中, failed=0, missing=480
- ✅ **[NP] repeated_open_3** (L3): 第 3 次打开一致: 0 条命中, failed=0, missing=480
- ❌ **[NP] code_bug_stopped** (L3): 批次 1 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出

## rick_s09e07 ❌

- ✅ **entry_count** (L1): 条目数 501，序号唯一递增
- ✅ **timeline_validity** (L1): 501 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 501 条往返一致
- ✅ **[NP] translated_entry_count** (L1): 条目数一致: 501
- ✅ **[NP] translated_timeline** (L1): 时间轴全部对齐
- ✅ **[NP] translated_format_roundtrip** (L1): 翻译后格式往返一致
- ⚠️ **[NP] subtitle_shift** (L1): 471 条空译文（可能是平移或降级失败）: [30, 31, 32, 33, 34]
  - 相关代码: translate.rs translate_batch_with_fallback
- ❌ **[NP] empty_translations** (L2): 471 条空译文: [30, 31, 32, 33, 34]
  - 相关代码: translate.rs translate_batch_with_fallback 降级重试
- ⚠️ **[NP] fake_translations** (L2): 假翻译 19 条 (3.79%)
  - 相关代码: translate.rs prompt 模板
- ✅ **[NP] cjk_check** (L2): 译文均含 CJK 字符
- ✅ **[NP] sound_effect_consistency** (L2): 音效标记一致
- ✅ **[NP] name_consistency** (L2): 人名一致，无残留标签
- ✅ **[NP] length_ratio** (L2): 译文长度全部在合理范围
- ✅ **[NP] alignment_check** (L2): 无错位迹象
- ✅ **[NP] truncation_check** (L2): 无截断迹象
- ⚠️ **[NP] translate_failures** (L2): 失败 19 条, 缓存 0 条, token 0 | 详情: #1: "-This the place?\\n-This the place." → "-This the place?\\n-This the place."; #4: "-This the stuff?\\n-This the stuff." → "-This the stuff?\\n-This the stuff."; #5: "And this is no ordinary stuff,\\nMorty." → "And this is no ordinary stuff,\\nMorty."; #6: "This sap is primo ." → "This sap is primo ."; #10: "-Argh!\\n-Ah!" → "-Argh!\\n-Ah!"; #11: "-What the hell?\\n-Shit! Vines!" → "-What the hell?\\n-Shit! Vines!"; #13: "Locusts come\\nto harvest my bounty." → "Locusts come\\nto harvest my bounty."; #14: "Hey, giant ancient tree guy." → "Hey, giant ancient tree guy."; #15: "Sorry. We thought\\nthe sap was accountability free." → "Sorry. We thought\\nthe sap was accountability free."; #16: "Yeah, well, we're not locusts,\\njust a couple adventurers." → "Yeah, well, we're not locusts,\\njust a couple adventurers."; #17: "Spacefarers." → "Spacefarers."; #18: "You consume your planets," → "You consume your planets,"; #19: "then spread your sickness\\nto the stars." → "then spread your sickness\\nto the stars."; #20: "Nah. Nah, man.\\nWe're clean, I promise." → "Nah. Nah, man.\\nWe're clean, I promise."; #21: "Yeah, yeah.\\nWe just got tested," → "Yeah, yeah.\\nWe just got tested,"; #22: "and I got a bit of\\na soy allergy." → "and I got a bit of\\na soy allergy."; #23: "I have a place\\nfor animals like you." → "I have a place\\nfor animals like you."; #24: "You will be reformed." → "You will be reformed."; #26: "What is..." → "What is..."
  - 相关代码: translate.rs translate_batch_with_fallback
- ⚠️ **[NP] judge_27b** (L5): 27b judge: 0 pass, 30 fail, 0 shift (共 30 条判定, 问题批次: [])
  - 相关代码: 27b judge 翻译质量问题
- ❌ **[NP] bilingual_roundtrip_srt** (L3): SRT 双语字幕检测失败: is_bilingual=false, matched=0, total=501
  - 相关代码: subtitle.rs detect_bilingual
- ❌ **[NP] bilingual_roundtrip_ass** (L3): ASS 双语字幕检测失败: is_bilingual=false, matched=0, total=501
  - 相关代码: subtitle.rs detect_bilingual
- ❌ **[NP] bilingual_roundtrip_vtt** (L3): VTT 双语字幕检测失败: is_bilingual=false, matched=0, total=501
  - 相关代码: subtitle.rs detect_bilingual
- ✅ **[NP] repeated_open_1** (L3): 第 1 次打开一致: 0 条命中, failed=0, missing=374
- ✅ **[NP] repeated_open_2** (L3): 第 2 次打开一致: 0 条命中, failed=0, missing=374
- ✅ **[NP] repeated_open_3** (L3): 第 3 次打开一致: 0 条命中, failed=0, missing=374
- ❌ **[NP] code_bug_stopped** (L3): 批次 1 L3 持久化验证发现代码 bug，测试已停止。修复代码后用 E2E_RESET=1 重跑
  - 相关代码: translate.rs 缓存质量校验 / subtitle.rs 双语导出
