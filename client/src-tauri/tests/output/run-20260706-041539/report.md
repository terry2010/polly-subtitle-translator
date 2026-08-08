# E2E 测试报告

**运行时间**: 2026-07-06 04:15:39

**结果**: 0 通过 / 0 警告 / 1 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| rick_and_morty | 560 | ❌ failed | 9/0/3 |

## rick_and_morty ❌

- ✅ **entry_count** (L1): 条目数 560，序号唯一递增
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
- ✅ **format_srt_to_srt** (L1): 条目数一致: 560
- ❌ **format_srt_to_ass** (L1): 条目数: 560 → 20
  - 相关代码: subtitle.rs render_ass / parse_ass
- ✅ **format_srt_to_vtt** (L1): 条目数一致: 560
- ✅ **format_ass_to_srt** (L1): 条目数一致: 560
- ❌ **format_ass_to_ass** (L1): 条目数: 560 → 20
  - 相关代码: subtitle.rs render_ass / parse_ass
- ✅ **format_ass_to_vtt** (L1): 条目数一致: 560
- ✅ **format_vtt_to_srt** (L1): 条目数一致: 560
- ❌ **format_vtt_to_ass** (L1): 条目数: 560 → 20
  - 相关代码: subtitle.rs render_ass / parse_ass
- ✅ **format_vtt_to_vtt** (L1): 条目数一致: 560
