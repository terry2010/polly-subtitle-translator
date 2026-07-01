// 测试平台过滤辅助函数
// 通过环境变量 VITEST_PLATFORM 控制只运行特定平台的测试
// 例如：VITEST_PLATFORM=macos npm test

export const currentPlatform = process.env.VITEST_PLATFORM ?? "all";

export function isPlatform(platform: "macos" | "windows" | "linux"): boolean {
  if (currentPlatform === "all") return true;
  return currentPlatform === platform;
}

export function skipUnless(platform: "macos" | "windows" | "linux"): boolean {
  return !isPlatform(platform);
}
