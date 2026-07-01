// 分平台测试示例
// 通过 VITEST_PLATFORM 环境变量控制只运行特定平台的测试
//
// 用法：
//   VITEST_PLATFORM=macos npm test
//   VITEST_PLATFORM=windows npm test
//   VITEST_PLATFORM=linux npm test
//   npm test                       （运行所有平台测试）

import { describe, it, expect } from "vitest";
import { skipUnless } from "./platform";

// === SECTION 1 END ===

// macOS 平台专属测试
describe.skipIf(skipUnless("macos"))("platform - macOS 专用", () => {
  it("macOS 平台标志", () => {
    // 实际测试可以验证 macOS 特有的文件路径或应用列表行为
    expect(process.platform).toMatch(/^(darwin|win32|linux)$/);
  });
});

// === SECTION 2 END ===

// Windows 平台专属测试
describe.skipIf(skipUnless("windows"))("platform - Windows 专用", () => {
  it("Windows 路径分隔符", () => {
    expect("C:\\Users\\test\\video.mkv").toContain("\\");
  });
});

// === SECTION 3 END ===

// Linux 平台专属测试
describe.skipIf(skipUnless("linux"))("platform - Linux 专用", () => {
  it("Unix 路径分隔符", () => {
    expect("/home/user/video.mkv").toContain("/");
  });
});

// === SECTION 4 END ===
