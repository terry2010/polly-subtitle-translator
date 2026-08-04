#!/usr/bin/env node
// kill-ports.js — 检查指定端口是否被占用，如果被占用则 kill 掉占用进程
// 用法: node scripts/kill-ports.js <port1> [port2] [port3] ...
import { execSync } from "child_process";

const ports = process.argv.slice(2);
if (ports.length === 0) {
  console.error("用法: node scripts/kill-ports.js <port1> [port2] ...");
  process.exit(1);
}

for (const port of ports) {
  try {
    // macOS/Linux: 用 lsof 查找占用端口的进程
    const pids = execSync(`lsof -i :${port} -sTCP:LISTEN -t 2>/dev/null`, { encoding: "utf8" }).trim();
    if (!pids) {
      console.log(`✅ 端口 ${port} 空闲`);
      continue;
    }
    const pidList = pids.split("\n").filter(Boolean);
    console.log(`⚠ 端口 ${port} 被占用，正在 kill 进程: ${pidList.join(" ")}`);
    for (const pid of pidList) {
      try {
        execSync(`kill -9 ${pid} 2>/dev/null`);
      } catch {
        // 忽略 kill 失败（进程可能已退出）
      }
    }
    // 等待端口释放
    execSync("sleep 1");
    // 确认已释放
    const remaining = execSync(`lsof -i :${port} -sTCP:LISTEN -t 2>/dev/null`, { encoding: "utf8" }).trim();
    if (remaining) {
      console.error(`❌ 端口 ${port} 仍被占用，无法释放`);
      process.exit(1);
    }
    console.log(`✅ 端口 ${port} 已释放`);
  } catch {
    // lsof 无输出时 exit code 非 0，端口空闲
    console.log(`✅ 端口 ${port} 空闲`);
  }
}
