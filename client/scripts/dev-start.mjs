#!/usr/bin/env node
// dev-start.mjs — macOS 开发启动（npm run start）
//
// 为什么不能直接 `tauri dev`：从 IDE（Devin/Windsurf/VSCode 集成终端）spawn
// 的裸二进制，macOS TCC 会把语音识别授权请求归因到父应用；父应用没声明
// NSSpeechRecognitionUsageDescription 时，系统在弹窗前直接 SIGABRT 杀进程。
//
// 因此 macOS 下改为：vite + cargo build + `open` 一个 .app 外壳。
// 外壳经 LaunchServices/launchd 拉起，进程自己成为 responsible，读外壳的
// Info.plist（含 NSSpeechRecognitionUsageDescription）。需要 cargo 编译产物
// 热重载时可用 `npm run start:tauri`（在真实 Terminal 里跑不会触发该问题）。
//
// 非 macOS 平台无 TCC 问题，直接回退 `tauri dev`。
import { spawn, spawnSync, execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const clientDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const srcTauriDir = path.join(clientDir, "src-tauri");
const debugDir = path.join(srcTauriDir, "target", "debug");
const appDir = path.join(debugDir, "zimufan.app");
const DEV_URL = "http://localhost:5173";

if (process.platform !== "darwin") {
  const r = spawnSync("npm", ["run", "tauri", "dev"], { cwd: clientDir, stdio: "inherit" });
  process.exit(r.status ?? 1);
}

// .app 外壳：Contents/MacOS/zimufan 符号链接到 cargo 产物；
// Info.plist 复用 src-tauri/Info.plist（含 speech key），补 CFBundleExecutable。
function ensureDevApp() {
  const macosDir = path.join(appDir, "Contents", "MacOS");
  fs.mkdirSync(macosDir, { recursive: true });
  const link = path.join(macosDir, "zimufan");
  try {
    fs.rmSync(link);
  } catch {}
  fs.symlinkSync("../../../zimufan", link);

  let plist = fs.readFileSync(path.join(srcTauriDir, "Info.plist"), "utf8");
  if (!plist.includes("CFBundleExecutable")) {
    plist = plist.replace(
      "</dict>",
      "  <key>CFBundleExecutable</key>\n  <string>zimufan</string>\n" +
        "  <key>CFBundlePackageType</key>\n  <string>APPL</string>\n</dict>",
    );
  }
  fs.writeFileSync(path.join(appDir, "Contents", "Info.plist"), plist);
}

function pkillDevInstance() {
  // 单实例插件会让第二次 open 把参数转发给旧实例——先杀掉残留进程。
  // 裸二进制与外壳启动的进程，可执行路径都解析为 target/debug/zimufan。
  execSync('pkill -f "target/debug/zimufan" 2>/dev/null || true', { stdio: "ignore" });
}

async function waitForVite(viteProc, timeoutMs = 60000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (viteProc.exitCode !== null) return false;
    try {
      const res = await fetch(DEV_URL);
      if (res.ok) return true;
    } catch {}
    await new Promise((r) => setTimeout(r, 500));
  }
  return false;
}

function cleanup(viteProc) {
  try {
    if (viteProc && viteProc.exitCode === null) viteProc.kill("SIGINT");
  } catch {}
  try {
    pkillDevInstance();
  } catch {}
}

function run(cmd, opts = {}) {
  const r = spawnSync(cmd, { shell: true, stdio: "inherit", ...opts });
  return r.status ?? 1;
}

// ---- main ----
ensureDevApp();
pkillDevInstance();

console.log("== cargo build (src-tauri) ==");
if (run("cargo build", { cwd: srcTauriDir }) !== 0) {
  console.error("cargo build failed");
  process.exit(1);
}

console.log("== vite dev server ==");
const vite = spawn("npm", ["run", "dev"], { cwd: clientDir, stdio: "inherit" });

let exiting = false;
const shutdown = (code) => {
  if (exiting) return;
  exiting = true;
  cleanup(vite);
  setTimeout(() => process.exit(code), 300);
};
process.on("SIGINT", () => shutdown(130));
process.on("SIGTERM", () => shutdown(143));

if (!(await waitForVite(vite))) {
  console.error(`vite did not come up at ${DEV_URL}`);
  shutdown(1);
}

console.log(`== open ${appDir} ==`);
const opened = spawnSync("open", [appDir], { stdio: "inherit" });
if (opened.status !== 0) {
  console.error("`open` failed — 可直接到 Finder 双击该 .app");
  shutdown(1);
}

console.log("✅ zimufan.app 已启动（进程自身即 TCC responsible，可正常请求语音识别权限）");
console.log("   Ctrl+C 会同时关闭 vite 和 dev 实例");

await new Promise((resolve) => vite.on("exit", resolve));
shutdown(vite.exitCode ?? 0);
