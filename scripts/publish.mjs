// AI-SubTrans 发布脚本
// 用法：
//   node scripts/publish.mjs 1.0.1 "更新内容"     → 改版本号 + 构建 + 发布
//   node scripts/publish.mjs                       → 交互式输入
//   node scripts/publish.mjs --build-only          → 只构建不发布（本地测试）
//
// 环境变量：
//   GITHUB_TOKEN                          → GitHub Personal Access Token（repo 权限）
//   TAURI_SIGNING_PRIVATE_KEY_PATH        → 私钥文件路径（默认 ~/.tauri/ai-subtrans.key）
//   TAURI_SIGNING_PRIVATE_KEY_PASSWORD    → 私钥密码
//
// 前提：
//   1. 已生成签名密钥：npx tauri signer generate -w ~/.tauri/ai-subtrans.key
//   2. GitHub Token 有 repo 权限
//   3. 已创建 gh-pages 分支（首次需要手动创建）
import { readFileSync, writeFileSync, existsSync, readdirSync, statSync } from "fs";
import { join } from "path";
import { execSync } from "child_process";
import { homedir } from "os";

const ROOT = process.cwd();
const TARGET_DIR = "C:\\Users\\terry\\.cargo-target\\zimufan\\release";
const NSIS_DIR = join(TARGET_DIR, "bundle", "nsis");

// === 解析参数 ===
const args = process.argv.slice(2);
const buildOnly = args.includes("--build-only");
const versionArg = args.find(a => !a.startsWith("--") && /^\d+\.\d+\.\d+$/.test(a));
const notesArg = args.find(a => !a.startsWith("--") && a !== versionArg);

// === 配置 ===
const GITHUB_TOKEN = process.env.GITHUB_TOKEN;
const PRIVATE_KEY_PATH = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH || join(homedir(), ".tauri", "ai-subtrans.key");
const PRIVATE_KEY_PASSWORD = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;

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
    throw new Error("请设置环境变量 TAURI_SIGNING_PRIVATE_KEY_PASSWORD");
  }

  // 读取私钥内容（Tauri 需要 TAURI_SIGNING_PRIVATE_KEY 而非 PATH）
  const privateKeyContent = readFileSync(PRIVATE_KEY_PATH, "utf-8").trim();

  const env = {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: privateKeyContent,
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: PRIVATE_KEY_PASSWORD,
  };
  console.log(`  私钥: ${PRIVATE_KEY_PATH}`);
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
async function createRelease(version, notes, artifacts, owner, repo) {
  console.log("\n>>> 创建 GitHub Release ...");
  const tag = `v${version}`;

  // 创建 Release
  const releaseRes = await githubAPI("POST", `/repos/${owner}/${repo}/releases`, {
    tag_name: tag,
    name: tag,
    body: notes,
    draft: false,
    prerelease: false,
  });
  const release = await releaseRes.json();
  console.log(`  ✓ Release ${tag}: ${release.html_url}`);

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

// === 生成并推送 latest.json ===
async function publishLatestJson(version, notes, artifacts, owner, repo) {
  console.log("\n>>> 更新 latest.json ...");

  const downloadUrl = `https://github.com/${owner}/${repo}/releases/download/v${version}/${artifacts.exeName}`;
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
  console.log("  latest.json:");
  console.log("  " + jsonContent.replace(/\n/g, "\n  "));

  // 获取 gh-pages 分支上现有的 latest.json（获取 sha 用于更新）
  let sha = null;
  try {
    const res = await githubAPI("GET", `/repos/${owner}/${repo}/contents/latest.json?ref=gh-pages`);
    const data = await res.json();
    sha = data.sha;
    console.log(`  ✓ 现有 latest.json sha: ${sha}`);
  } catch {
    console.log("  (gh-pages 分支上无现有 latest.json，将创建新文件)");
  }

  // 更新/创建 latest.json
  const content = Buffer.from(jsonContent).toString("base64");
  await githubAPI("PUT", `/repos/${owner}/${repo}/contents/latest.json`, {
    message: `chore: update latest.json for v${version}`,
    content,
    sha,
    branch: "gh-pages",
  });
  console.log("  ✓ latest.json 已推送到 gh-pages 分支");
}

// === 主流程 ===
async function main() {
  let version = versionArg;
  let notes = notesArg || "";

  // 交互式输入
  if (!version) {
    const currentPkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf-8"));
    console.log(`\n当前版本: ${currentPkg.version}`);
    console.log('用法: node scripts/publish.mjs <版本号> "更新内容"');
    console.log('示例: node scripts/publish.mjs 1.0.1 "修复字幕提取进度条\\n新增自动更新"');
    process.exit(1);
  }

  console.log(`\n========================================`);
  console.log(`  AI-SubTrans v${version} 发布`);
  console.log(`========================================`);

  if (!buildOnly) {
    if (!GITHUB_TOKEN) {
      throw new Error("请设置环境变量 GITHUB_TOKEN（GitHub Personal Access Token，需要 repo 权限）");
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

  // 5. 创建 Release + 上传
  await createRelease(version, notes, artifacts, owner, repo);

  // 6. 更新 latest.json
  await publishLatestJson(version, notes, artifacts, owner, repo);

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
