// AI-SubTrans 发布脚本
// 用法：
//   node scripts/publish.mjs                        → 交互式输入版本号、说明、密码，确认后发布
//   node scripts/publish.mjs 1.0.1 "更新内容"        → 改版本号 + 构建 + 发布（全流程）
//   node scripts/publish.mjs 1.0.1 "更新内容" --password xxx → 带密码直接发布
//   node scripts/publish.mjs --build-only           → 只构建不发布（本地测试）
//   node scripts/publish.mjs --set-version 1.0.1    → 只改版本号（CI 构建前用）
//   node scripts/publish.mjs --update-latest 1.0.1 "更新内容" → 只从 Release assets 合并生成 latest.json（CI 构建后用）
//
// 环境变量（交互式模式会自动获取/输入，无需手动设置）：
//   GITHUB_TOKEN                          → GitHub Token（自动从 gh CLI 获取）
//   TAURI_SIGNING_PRIVATE_KEY_PATH        → 私钥文件路径（默认 ~/.tauri/ai-subtrans.key）
//   TAURI_SIGNING_PRIVATE_KEY_PASSWORD    → 私钥密码（交互式输入或 --password 参数）
//   TAURI_SIGNING_PRIVATE_KEY             → 私钥内容（CI 中用，优先于 PATH）
//
// 前提：
//   1. 已生成签名密钥：npx tauri signer generate -w ~/.tauri/ai-subtrans.key
//   2. gh CLI 已登录（gh auth login）或已设置 GITHUB_TOKEN
//   3. 已创建 gh-pages 分支（首次需要手动创建）
import { readFileSync, writeFileSync, existsSync, readdirSync, statSync } from "fs";
import { join } from "path";
import { execSync } from "child_process";
import { homedir } from "os";
import * as readline from "readline";

const ROOT = process.cwd();
const TARGET_DIR = "C:\\Users\\terry\\.cargo-target\\zimufan\\release";
const NSIS_DIR = join(TARGET_DIR, "bundle", "nsis");

// === 解析参数 ===
const args = process.argv.slice(2);
const buildOnly = args.includes("--build-only");
const setVersionOnly = args.includes("--set-version");
const updateLatestOnly = args.includes("--update-latest");
const isNightly = args.includes("--nightly");
const isPrerelease = args.includes("--prerelease");
const manifestArg = args.find(a => a.startsWith("--manifest="))?.split("=")[1] || "latest.json";
const passwordArg = args.find(a => a.startsWith("--password="))?.split("=")[1];
// 支持正式版(1.0.1)和nightly版本(1.0.1-nightly.20260706-143025)
const versionArg = args.find(a => !a.startsWith("--") && /^\d+\.\d+\.\d+(-[\w.-]+)?$/.test(a));
const notesArg = args.find(a => !a.startsWith("--") && a !== versionArg);

// === 配置 ===
let GITHUB_TOKEN = process.env.GITHUB_TOKEN;
const PRIVATE_KEY_PATH = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH || join(homedir(), ".tauri", "ai-subtrans.key");
const PRIVATE_KEY_PASSWORD_FILE = join(homedir(), ".tauri", "ai-subtrans.key.password");
// 密码优先级：环境变量 > --password 参数 > 密码文件 > 交互式输入
let PRIVATE_KEY_PASSWORD = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD || passwordArg || null;

// 尝试从密码文件读取
function tryReadPasswordFile() {
  try {
    if (existsSync(PRIVATE_KEY_PASSWORD_FILE)) {
      return readFileSync(PRIVATE_KEY_PASSWORD_FILE, "utf-8").trim();
    }
  } catch {}
  return null;
}

// === 交互式输入 ===
function ask(question, { hidden = false } = {}) {
  return new Promise((resolve) => {
    const rl = readline.createInterface({
      input: process.stdin,
      output: process.stdout,
    });
    if (hidden) {
      // 隐藏密码输入
      const stdin = process.stdin;
      const isRaw = stdin.isTTY ? stdin.isRaw : false;
      if (stdin.isTTY) stdin.setRawMode(true);
      rl.question(question, (answer) => {
        if (stdin.isTTY) stdin.setRawMode(isRaw);
        console.log(); // 换行
        rl.close();
        resolve(answer);
      });
      // 隐藏输入字符
      if (stdin.isTTY) {
        stdin.on("data", (char) => {
          if (char.toString() === "\r" || char.toString() === "\n") return;
          process.stdout.write("\r\x1b[K" + question);
        });
      }
    } else {
      rl.question(question, (answer) => {
        rl.close();
        resolve(answer);
      });
    }
  });
}

function askYesNo(question, defaultValue = true) {
  return new Promise((resolve) => {
    const rl = readline.createInterface({
      input: process.stdin,
      output: process.stdout,
    });
    const hint = defaultValue ? "[Y/n]" : "[y/N]";
    rl.question(`${question} ${hint} `, (answer) => {
      rl.close();
      const a = answer.trim().toLowerCase();
      if (a === "") resolve(defaultValue);
      else resolve(a === "y" || a === "yes");
    });
  });
}

// === 自动获取 GitHub Token（从 gh CLI）===
function tryGetGhToken() {
  try {
    const token = execSync("gh auth token", { encoding: "utf-8", stdio: ["pipe", "pipe", "pipe"] }).trim();
    if (token && token.length > 0) return token;
  } catch {}
  return null;
}

// 从 git remote 获取 owner/repo
function getRepoInfo() {
  const remoteUrl = execSync("git remote get-url origin", { cwd: ROOT, encoding: "utf-8" }).trim();
  // git@github.com:owner/repo.git 或 https://github.com/owner/repo.git
  const match = remoteUrl.match(/github\.com[:/]([^/]+)\/([^/]+?)(\.git)?$/);
  if (!match) throw new Error(`无法从 git remote 解析 owner/repo: ${remoteUrl}`);
  return { owner: match[1], repo: match[2] };
}

// === 修改版本号 ===
function updateVersion(version) {
  console.log(`\n>>> 更新版本号到 ${version} ...`);
  // package.json
  const pkgPath = join(ROOT, "package.json");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
  pkg.version = version;
  writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
  console.log("  ✓ package.json");

  // tauri.conf.json
  const tauriConfPath = join(ROOT, "src-tauri", "tauri.conf.json");
  const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf-8"));
  tauriConf.version = version;
  writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + "\n");
  console.log("  ✓ tauri.conf.json");

  // Cargo.toml
  const cargoPath = join(ROOT, "src-tauri", "Cargo.toml");
  let cargoContent = readFileSync(cargoPath, "utf-8");
  cargoContent = cargoContent.replace(/^version = "[\d.]+"/m, `version = "${version}"`);
  writeFileSync(cargoPath, cargoContent);
  console.log("  ✓ Cargo.toml");
}

// === 构建 ===
function build() {
  console.log("\n>>> 构建（带签名）...");
  if (!existsSync(PRIVATE_KEY_PATH)) {
    throw new Error(`私钥文件不存在: ${PRIVATE_KEY_PATH}\n请先运行: npx tauri signer generate -w ${PRIVATE_KEY_PATH}`);
  }
  if (!PRIVATE_KEY_PASSWORD) {
    throw new Error("未提供私钥密码");
  }

  // 读取私钥内容（Tauri 需要 TAURI_SIGNING_PRIVATE_KEY 而非 PATH）
  const privateKeyContent = readFileSync(PRIVATE_KEY_PATH, "utf-8").trim();

  // 自动设置 PATH 和 CARGO_TARGET_DIR（Windows 本地开发环境）
  const cargoBin = join(homedir(), ".cargo", "bin");
  const cargoTargetDir = "C:\\Users\\terry\\.cargo-target\\zimufan";
  const env = {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: privateKeyContent,
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: PRIVATE_KEY_PASSWORD,
    PATH: `${cargoBin};${process.env.PATH || ""}`,
    CARGO_TARGET_DIR: cargoTargetDir,
  };
  console.log(`  私钥: ${PRIVATE_KEY_PATH}`);
  console.log(`  CARGO_TARGET_DIR: ${cargoTargetDir}`);
  execSync("npm run tauri build -- --bundles nsis", { cwd: ROOT, stdio: "inherit", env, shell: "cmd.exe" });
  console.log("  ✓ 构建完成");
}

// === 查找构建产物 ===
function findArtifacts(version) {
  console.log("\n>>> 查找构建产物 ...");
  if (!existsSync(NSIS_DIR)) {
    throw new Error(`NSIS 输出目录不存在: ${NSIS_DIR}`);
  }
  // 优先查找当前版本的安装包
  const allFiles = readdirSync(NSIS_DIR).filter(f => f.endsWith("-setup.exe"));
  const exeFile = allFiles.find(f => f.includes(`_${version}_`)) || allFiles[0];
  if (!exeFile) throw new Error("未找到 -setup.exe 安装包");
  const exePath = join(NSIS_DIR, exeFile);
  const sigPath = exePath + ".sig";
  if (!existsSync(sigPath)) throw new Error(`未找到签名文件: ${sigPath}`);
  const sizeMB = (statSync(exePath).size / 1024 / 1024).toFixed(1);
  console.log(`  ✓ ${exeFile} (${sizeMB} MB)`);
  console.log(`  ✓ ${exeFile}.sig`);
  return { exePath, exeName: exeFile, sigPath, signature: readFileSync(sigPath, "utf-8").trim() };
}

// === GitHub API ===
async function githubAPI(method, path, body, contentType = "application/json") {
  const url = path.startsWith("http") ? path : `https://api.github.com${path}`;
  const res = await fetch(url, {
    method,
    headers: {
      "Authorization": `Bearer ${GITHUB_TOKEN}`,
      "Accept": "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      "Content-Type": contentType,
    },
    body: body ? (contentType === "application/json" ? JSON.stringify(body) : body) : undefined,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`GitHub API ${method} ${url} 失败: ${res.status} ${text}`);
  }
  return res;
}

// === 创建 Release + 上传 assets ===
async function createRelease(version, notes, artifacts, owner, repo, prerelease = false) {
  console.log("\n>>> 创建 GitHub Release ...");
  // nightly tag 格式：nightly-基础版本号-时间戳（如 nightly-1.0.1-20260706.081028）
  // 正式版 tag 格式：v版本号（如 v1.0.1）
  const tag = prerelease && version.includes("-nightly.")
    ? `nightly-${version.replace("-nightly.", "-")}`
    : `v${version}`;

  // 创建 Release
  const releaseRes = await githubAPI("POST", `/repos/${owner}/${repo}/releases`, {
    tag_name: tag,
    name: tag,
    body: notes,
    draft: false,
    prerelease,
  });
  const release = await releaseRes.json();
  console.log(`  ✓ Release ${tag}${prerelease ? " (prerelease)" : ""}: ${release.html_url}`);

  // 上传 .exe
  console.log("  >>> 上传安装包 ...");
  const exeData = readFileSync(artifacts.exePath);
  const uploadUrl = release.upload_url.replace("{?name,label}", `?name=${encodeURIComponent(artifacts.exeName)}`);
  await githubAPI("POST", uploadUrl, exeData, "application/octet-stream");
  console.log(`  ✓ ${artifacts.exeName}`);

  // 上传 .sig
  console.log("  >>> 上传签名文件 ...");
  const sigData = readFileSync(artifacts.sigPath);
  const sigUploadUrl = release.upload_url.replace("{?name,label}", `?name=${encodeURIComponent(artifacts.exeName + ".sig")}`);
  await githubAPI("POST", sigUploadUrl, sigData, "application/octet-stream");
  console.log(`  ✓ ${artifacts.exeName}.sig`);

  return release;
}

// === 生成并推送清单文件（latest.json 或 nightly.json）===
async function publishLatestJson(version, notes, artifacts, owner, repo, manifestFile = "latest.json") {
  console.log(`\n>>> 更新 ${manifestFile} ...`);

  // nightly tag 格式：nightly-基础版本号-时间戳（如 nightly-1.0.1-20260706.081028）
  // 正式版 tag 格式：v版本号（如 v1.0.1）
  const tag = version.includes("-nightly.")
    ? `nightly-${version.replace("-nightly.", "-")}`
    : `v${version}`;
  const downloadUrl = `https://github.com/${owner}/${repo}/releases/download/${tag}/${artifacts.exeName}`;
  // 国内加速：用 gh-proxy 前缀
  const acceleratedUrl = `https://gh-proxy.com/${downloadUrl}`;

  const latestJson = {
    version,
    notes: notes.replace(/\\n/g, "\n"),
    pub_date: new Date().toISOString(),
    platforms: {
      "windows-x86_64": {
        signature: artifacts.signature,
        url: acceleratedUrl,
      },
    },
  };

  const jsonContent = JSON.stringify(latestJson, null, 2);
  console.log(`  ${manifestFile}:`);
  console.log("  " + jsonContent.replace(/\n/g, "\n  "));

  // 获取 gh-pages 分支上现有的清单文件（获取 sha 用于更新）
  let sha = null;
  try {
    const res = await githubAPI("GET", `/repos/${owner}/${repo}/contents/${manifestFile}?ref=gh-pages`);
    const data = await res.json();
    sha = data.sha;
    console.log(`  ✓ 现有 ${manifestFile} sha: ${sha}`);
  } catch {
    console.log(`  (gh-pages 分支上无现有 ${manifestFile}，将创建新文件)`);
  }

  // 更新/创建清单文件
  const content = Buffer.from(jsonContent).toString("base64");
  await githubAPI("PUT", `/repos/${owner}/${repo}/contents/${manifestFile}`, {
    message: `chore: update ${manifestFile} for ${tag}`,
    content,
    sha,
    branch: "gh-pages",
  });
  console.log(`  ✓ ${manifestFile} 已推送到 gh-pages 分支`);
}

// === 从 Release assets 合并生成多平台 latest.json（CI 用） ===
// 根据 .sig 文件名判断平台，找到对应的下载 URL
function detectPlatform(assetName) {
  // Windows NSIS: AI-SubTrans_1.0.1_x64-setup.exe.sig
  if (assetName.endsWith("-setup.exe.sig")) return "windows-x86_64";
  // macOS arm64: AI-SubTrans_1.0.1_aarch64.app.tar.gz.sig
  if (assetName.endsWith("aarch64.app.tar.gz.sig")) return "darwin-aarch64";
  // macOS x86_64: AI-SubTrans_1.0.1_x86_64.app.tar.gz.sig
  if (assetName.endsWith("x86_64.app.tar.gz.sig")) return "darwin-x86_64";
  // macOS universal: AI-SubTrans_1.0.1_universal.app.tar.gz.sig
  if (assetName.endsWith("universal.app.tar.gz.sig")) return "darwin-universal";
  return null;
}

async function updateLatestFromRelease(version, notes, manifestFile = "latest.json") {
  // nightly tag 格式：nightly-基础版本号-时间戳（如 nightly-1.0.1-20260706.081028）
  // 正式版 tag 格式：v版本号（如 v1.0.1）
  const tag = version.includes("-nightly.")
    ? `nightly-${version.replace("-nightly.", "-")}`
    : `v${version}`;
  console.log(`\n>>> 从 Release assets 合并生成 ${manifestFile} (${tag}) ...`);

  const { owner, repo } = getRepoInfo();
  console.log(`  仓库: ${owner}/${repo}`);

  // 1. 获取 Release 信息和 assets 列表
  const releaseRes = await githubAPI("GET", `/repos/${owner}/${repo}/releases/tags/${tag}`);
  const release = await releaseRes.json();
  console.log(`  ✓ Release: ${release.html_url}`);
  console.log(`  ✓ Assets: ${release.assets.length} 个`);

  // 2. 遍历 assets，找 .sig 文件并匹配平台
  const platforms = {};
  for (const asset of release.assets) {
    const platform = detectPlatform(asset.name);
    if (!platform) continue;

    // 下载 .sig 文件内容
    console.log(`  >>> 下载签名: ${asset.name}`);
    const sigRes = await fetch(asset.browser_download_url);
    if (!sigRes.ok) {
      throw new Error(`下载签名文件失败: ${asset.name} (${sigRes.status})`);
    }
    const signature = (await sigRes.text()).trim();

    // 找对应的安装包 asset（去掉 .sig 后缀）
    const baseName = asset.name.replace(/\.sig$/, "");
    const baseAsset = release.assets.find(a => a.name === baseName);
    if (!baseAsset) {
      console.log(`  ⚠ 未找到对应安装包: ${baseName}，跳过 ${platform}`);
      continue;
    }

    // 构建 URL（加 gh-proxy 加速前缀）
    const downloadUrl = `https://github.com/${owner}/${repo}/releases/download/${tag}/${baseName}`;
    const acceleratedUrl = `https://gh-proxy.com/${downloadUrl}`;

    platforms[platform] = {
      signature,
      url: acceleratedUrl,
    };
    console.log(`  ✓ ${platform}: ${baseName}`);
  }

  if (Object.keys(platforms).length === 0) {
    throw new Error("未从 Release assets 中找到任何平台的签名文件");
  }

  // 3. 生成清单文件
  const latestJson = {
    version,
    notes: notes.replace(/\\n/g, "\n"),
    pub_date: new Date().toISOString(),
    platforms,
  };

  const jsonContent = JSON.stringify(latestJson, null, 2);
  console.log(`\n  ${manifestFile}:`);
  console.log("  " + jsonContent.replace(/\n/g, "\n  "));

  // 4. 推送到 gh-pages
  let sha = null;
  try {
    const res = await githubAPI("GET", `/repos/${owner}/${repo}/contents/${manifestFile}?ref=gh-pages`);
    const data = await res.json();
    sha = data.sha;
    console.log(`  ✓ 现有 ${manifestFile} sha: ${sha}`);
  } catch {
    console.log(`  (gh-pages 分支上无现有 ${manifestFile}，将创建新文件)`);
  }

  const content = Buffer.from(jsonContent).toString("base64");
  await githubAPI("PUT", `/repos/${owner}/${repo}/contents/${manifestFile}`, {
    message: `chore: update ${manifestFile} for ${tag}`,
    content,
    sha,
    branch: "gh-pages",
  });
  console.log(`  ✓ ${manifestFile} 已推送到 gh-pages 分支`);
  console.log(`\n  验证: https://${owner}.github.io/${repo}/${manifestFile}`);
}

// === 主流程 ===
async function main() {
  let version = versionArg;
  let notes = notesArg || "";

  // ── 模式 1：--set-version（CI 构建前改版本号）──
  if (setVersionOnly) {
    if (!version) {
      console.error('用法: node scripts/publish.mjs --set-version <版本号>');
      process.exit(1);
    }
    updateVersion(version);
    console.log(`\n✅ 版本号已更新到 ${version}`);
    return;
  }

  // ── 模式 2：--update-latest（CI 构建后合并清单文件）──
  if (updateLatestOnly) {
    if (!version) {
      console.error('用法: node scripts/publish.mjs --update-latest [--nightly] [--manifest=nightly.json] <版本号> "更新内容"');
      process.exit(1);
    }
    if (!GITHUB_TOKEN) {
      throw new Error("请设置环境变量 GITHUB_TOKEN");
    }
    const manifest = isNightly ? "nightly.json" : manifestArg;
    await updateLatestFromRelease(version, notes, manifest);
    console.log(`\n========================================`);
    console.log(`  ✅ ${manifest} 更新完成！`);
    console.log(`========================================`);
    return;
  }

  // ── 模式 3：本地全流程发布 ──

  // 交互式输入（无版本号参数时）
  if (!version) {
    const currentPkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf-8"));
    console.log(`\n========================================`);
    console.log(`  AI-SubTrans 发布脚本（交互式）`);
    console.log(`========================================`);
    console.log(`\n当前版本: ${currentPkg.version}`);

    version = await ask("\n请输入发布版本号: ");
    if (!version || !/^\d+\.\d+\.\d+(-[\w.-]+)?$/.test(version)) {
      console.error("❌ 版本号格式无效，应为 x.y.z 或 x.y.z-prerelease");
      process.exit(1);
    }

    notes = await ask("请输入更新说明: ");
    if (!notes) {
      console.error("❌ 更新说明不能为空");
      process.exit(1);
    }

    // 自动获取 GitHub Token
    if (!GITHUB_TOKEN) {
      GITHUB_TOKEN = tryGetGhToken();
      if (GITHUB_TOKEN) {
        console.log("  ✓ 自动获取 GitHub Token（gh auth token）");
      } else {
        console.error("❌ 未找到 GitHub Token，请先运行 gh auth login 或设置 GITHUB_TOKEN 环境变量");
        process.exit(1);
      }
    }

    // 私钥密码：优先环境变量/参数，其次密码文件，最后交互输入
    if (!PRIVATE_KEY_PASSWORD) {
      PRIVATE_KEY_PASSWORD = tryReadPasswordFile();
      if (PRIVATE_KEY_PASSWORD) {
        console.log(`  ✓ 从密码文件读取: ${PRIVATE_KEY_PASSWORD_FILE}`);
      } else {
        PRIVATE_KEY_PASSWORD = await ask("请输入私钥密码: ", { hidden: true });
        if (!PRIVATE_KEY_PASSWORD) {
          console.error("❌ 私钥密码不能为空");
          process.exit(1);
        }
      }
    }

    // 确认
    console.log(`\n========================================`);
    console.log(`  发布确认`);
    console.log(`========================================`);
    console.log(`  版本号: ${version}`);
    console.log(`  说明:   ${notes}`);
    console.log(`  仓库:   ${getRepoInfo().owner}/${getRepoInfo().repo}`);
    console.log(`  预发布: ${isNightly || isPrerelease ? "是" : "否"}`);
    console.log(`  构建:   ${buildOnly ? "仅构建" : "构建 + 发布"}`);
    const confirmed = await askYesNo("\n确认发布？");
    if (!confirmed) {
      console.log("已取消发布。");
      process.exit(0);
    }
  }

  console.log(`\n========================================`);
  console.log(`  AI-SubTrans v${version} 发布`);
  console.log(`========================================`);

  if (!buildOnly) {
    if (!GITHUB_TOKEN) {
      // 尝试自动获取
      GITHUB_TOKEN = tryGetGhToken();
      if (!GITHUB_TOKEN) {
        throw new Error("未找到 GitHub Token，请先运行 gh auth login 或设置 GITHUB_TOKEN 环境变量");
      }
      console.log("  ✓ 自动获取 GitHub Token");
    }
  }

  // 1. 更新版本号
  updateVersion(version);

  // 2. 构建
  build();

  // 3. 查找产物
  const artifacts = findArtifacts(version);

  if (buildOnly) {
    console.log("\n=== --build-only 模式，跳过发布 ===");
    console.log(`安装包: ${artifacts.exePath}`);
    console.log(`签名文件: ${artifacts.sigPath}`);
    return;
  }

  // 4. 获取仓库信息
  const { owner, repo } = getRepoInfo();
  console.log(`\n>>> 仓库: ${owner}/${repo}`);

  // 5. 创建 Release + 上传（nightly 标记为 prerelease）
  await createRelease(version, notes, artifacts, owner, repo, isNightly || isPrerelease);

  // 6. 更新清单文件（nightly 用 nightly.json）
  const manifest = isNightly ? "nightly.json" : manifestArg;
  await publishLatestJson(version, notes, artifacts, owner, repo, manifest);

  console.log(`\n========================================`);
  console.log(`  ✅ 发布完成！`);
  console.log(`========================================`);
  console.log(`\n客户端启动后 5 秒会自动检查更新。`);
  console.log(`验证: https://${owner}.github.io/${repo}/latest.json`);
}

main().catch(err => {
  console.error("\n❌ 发布失败:", err.message);
  process.exit(1);
});
