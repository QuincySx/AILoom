#!/usr/bin/env node
/**
 * AILoom npm 包装器（设计稿 v0，AIL-029）。
 * 职责：定位/下载平台匹配的 ailoom 二进制 → sha256 校验 → 透传参数。
 * 状态：设计稿。发布与下载 URL 由 release workflow 落地后接线（docs/RELEASE-CHECKLIST.md）。
 */
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, renameSync } from "node:fs";
import { homedir, platform, arch } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const BASE_URL = process.env.AILOOM_DOWNLOAD_BASE ?? "https://example.invalid/ailoom/releases";
const VERSION = "0.0.0-draft";

const SUPPORTED_TRIPLES = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-pc-windows-msvc",
];

function triple() {
  // 显式覆写（受控测试/镜像环境用）；不在支持矩阵内即拒绝
  const override = process.env.AILOOM_TRIPLE;
  if (override) {
    if (SUPPORTED_TRIPLES.includes(override)) return override;
    fail(`不支持的 AILOOM_TRIPLE ${override}；矩阵见 packaging/npm/PLATFORMS.md`);
  }
  const p = platform();
  const a = arch();
  if (p === "darwin" && a === "arm64") return "aarch64-apple-darwin";
  if (p === "darwin" && a === "x64") return "x86_64-apple-darwin";
  if (p === "linux" && a === "x64") return "x86_64-unknown-linux-gnu";
  if (p === "linux" && a === "arm64") return "aarch64-unknown-linux-gnu";
  if (p === "win32" && a === "x64") return "x86_64-pc-windows-msvc";
  fail(`不支持的平台 ${p}/${a}；矩阵见 packaging/npm/PLATFORMS.md`);
}

function fail(message) {
  console.error(`[ailoom] ${message}`);
  console.error("诊断：代理环境设置 HTTPS_PROXY / AILOOM_DOWNLOAD_BASE（离线镜像）；");
  console.error("或手动下载二进制放入 ~/.ailoom/bin/。详见 packaging/npm/PLATFORMS.md。");
  process.exit(1);
}

function binaryPath() {
  const ext = platform() === "win32" ? ".exe" : "";
  const dir = process.env.AILOOM_BIN_DIR ?? join(homedir(), ".ailoom", "bin");
  return join(dir, `ailoom-${triple()}${ext}`);
}

function verifySha256(file, expectedFile) {
  // 校验清单与制品同源发布；缺失或失败即拒绝运行
  if (!existsSync(expectedFile)) fail(`缺少校验文件，拒绝运行未校验制品`);
  const expected = readFileSync(expectedFile, "utf8").trim().split(/\s+/)[0];
  const actual = createHash("sha256").update(readFileSync(file)).digest("hex");
  if (actual !== expected) fail(`制品校验失败（sha256 不匹配）`);
}

function main() {
  const bin = binaryPath();
  if (!existsSync(bin)) {
    // 设计稿：下载逻辑由 release workflow 落地后接线（download → verify → rename → chmod）
    fail(
      `未找到二进制 ${bin}。\n设计稿尚未接线下载；请在 AILOOM_BIN_DIR 放置对应平台二进制。`
    );
  }
  verifySha256(bin, `${bin}.sha256`);
  const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
  process.exit(result.status ?? 1);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) {
  mkdirSync(join(homedir(), ".ailoom", "bin"), { recursive: true });
  main();
}
