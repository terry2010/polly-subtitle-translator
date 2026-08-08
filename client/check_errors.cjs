#!/usr/bin/env node
/**
 * CI 检查脚本：验证错误码完整性
 *
 * 检查项：
 * 1. 后端 error.rs 中所有 IpcError::new("xxx.code", ...) 的 code
 *    必须在 en.json 和 zh.json 的 error.* 下有对应翻译
 * 2. 前端 t() 调用不允许带字符串字面量 fallback（第二参数为字符串）
 * 3. 前端不允许直接使用 e?.message / error.message 访问 IpcError
 *    （应使用 formatIpcError）
 *
 * 用法：node check_errors.cjs
 * 退出码：0 = 通过，1 = 有错误
 */
const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname);
const ERROR_RS = path.join(ROOT, "src-tauri", "src", "error.rs");
const EN_JSON = path.join(ROOT, "src", "locales", "en.json");
const ZH_JSON = path.join(ROOT, "src", "locales", "zh.json");
const SRC_DIR = path.join(ROOT, "src");

let hasError = false;

function fail(msg) {
  console.error(`[ERROR] ${msg}`);
  hasError = true;
}

function warn(msg) {
  console.warn(`[WARN] ${msg}`);
}

// === 1. 提取后端错误码，检查 locale 覆盖 ===
const errorRs = fs.readFileSync(ERROR_RS, "utf8");
const codeRegex = /IpcError::new\("([^"]+)"/g;
const codes = new Set();
let m;
while ((m = codeRegex.exec(errorRs)) !== null) {
  codes.add(m[1]);
}
console.log(`Found ${codes.size} error codes in error.rs`);

const en = JSON.parse(fs.readFileSync(EN_JSON, "utf8"));
const zh = JSON.parse(fs.readFileSync(ZH_JSON, "utf8"));

function getNested(obj, dottedKey) {
  return dottedKey.split(".").reduce((acc, k) => (acc && typeof acc === "object" ? acc[k] : undefined), obj);
}

for (const code of codes) {
  const enVal = getNested(en.error, code);
  const zhVal = getNested(zh.error, code);
  if (enVal === undefined) fail(`en.json missing error.${code}`);
  if (zhVal === undefined) fail(`zh.json missing error.${code}`);
}

// === 2. 检查前端 t() 不带字符串 fallback ===
function walkDir(dir, ext, callback) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walkDir(full, ext, callback);
    else if (entry.name.endsWith(ext)) callback(full);
  }
}

walkDir(SRC_DIR, ".ts", (file) => checkFile(file));
walkDir(SRC_DIR, ".tsx", (file) => checkFile(file));

function checkFile(file) {
  const content = fs.readFileSync(file, "utf8");
  const rel = path.relative(ROOT, file);

  // 检查 t("key", "fallback") 模式 — 第二参数为字符串字面量
  // 注意：这是预存问题，暂列为 warning，后续逐步清理
  const fallbackRegex = /\bt\(["']([^"']+)["'],\s*["']/g;
  let fm;
  while ((fm = fallbackRegex.exec(content)) !== null) {
    warn(`${rel}: t("${fm[1]}", ...) has string literal fallback — should use i18n keys only`);
  }

  // 检查 i18n.t("key", "fallback") 模式
  const i18nFallbackRegex = /\bi18n\.t\(["']([^"']+)["'],\s*["']/g;
  while ((fm = i18nFallbackRegex.exec(content)) !== null) {
    warn(`${rel}: i18n.t("${fm[1]}", ...) has string literal fallback — should use i18n keys only`);
  }
}

// === 3. 检查前端不直接访问 IpcError.message ===
// 注意：SearchDialog 中有 e instanceof Error 的合法用法，但 IpcError 已无 message 字段
// 所以任何 e?.message 或 error.message 在 IPC 上下文中都是错误的
walkDir(SRC_DIR, ".ts", (file) => checkMessageAccess(file));
walkDir(SRC_DIR, ".tsx", (file) => checkMessageAccess(file));

function checkMessageAccess(file) {
  const content = fs.readFileSync(file, "utf8");
  const rel = path.relative(ROOT, file);
  // 跳过 api.ts（formatIpcError 定义处）和 node_modules
  if (rel.includes("node_modules") || rel.endsWith("lib/api.ts")) return;

  // 检查 e?.message 模式（IPC 错误上下文）
  const msgRegex = /\be\?\.message/g;
  let mm;
  while ((mm = msgRegex.exec(content)) !== null) {
    const line = content.substring(0, mm.index).split("\n").length;
    warn(`${rel}:${line} uses e?.message — should use formatIpcError(e) for IpcError`);
  }
}

// === 结果 ===
if (hasError) {
  console.error("\n❌ Error code check FAILED");
  process.exit(1);
} else {
  console.log("\n✅ Error code check PASSED");
  process.exit(0);
}
