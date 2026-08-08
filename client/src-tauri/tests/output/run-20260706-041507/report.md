# E2E 测试报告

**运行时间**: 2026-07-06 04:15:07

**结果**: 0 通过 / 2 警告 / 0 失败

## 概览

| Fixture | 条目数 | 状态 | 检查项 (P/W/F) |
|---------|--------|------|----------------|
| clarksons_farm | 1054 | ⚠️ warned | 2/1/0 |
| rick_and_morty | 560 | ⚠️ warned | 2/1/0 |

## clarksons_farm ⚠️

- ⚠️ **entry_count** (L1): 条目数 1054，1054 条序号不连续
  - 相关代码: subtitle.rs parse_srt index 赋值
- ✅ **timeline_validity** (L1): 1054 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 1054 条往返一致

## rick_and_morty ⚠️

- ⚠️ **entry_count** (L1): 条目数 560，560 条序号不连续
  - 相关代码: subtitle.rs parse_srt index 赋值
- ✅ **timeline_validity** (L1): 560 条时间轴全部有效
- ✅ **format_roundtrip** (L1): 560 条往返一致
