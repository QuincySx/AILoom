#!/usr/bin/env node
/**
 * AILoom 的 npm 包装：运行当前平台的 ailoom 二进制，参数原样转发。
 *
 * 首次运行从 GitHub Release 下载与本包版本一致的二进制，sha256 校验通过后缓存到
 * ~/.ailoom/npm/<版本>/；之后每次运行前都重新校验。
 * - AILOOM_DOWNLOAD_BASE：换下载源（内网镜像），指向放着 ailoom-<三元组> 与 .sha256 的目录
 * - AILOOM_BIN_DIR：只用该目录里已放好的二进制与 .sha256，不下载（离线环境）
 */
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir, platform, arch } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";

const VERSION = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")).version;
const BASE_URL =
  process.env.AILOOM_DOWNLOAD_BASE ?? `https://github.com/QuincySx/AILoom/releases/download/v${VERSION}`;

const SUPPORTED_TRIPLES = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-pc-windows-msvc",
];

function fail(message) {
  console.error(`[ailoom] ${message}`);
  process.exit(1);
}

function triple() {
  // 显式覆写（受控测试/镜像环境用）；不在支持矩阵内即拒绝
  const override = process.env.AILOOM_TRIPLE;
  if (override) {
    if (SUPPORTED_TRIPLES.includes(override)) return override;
    fail(`不支持的 AILOOM_TRIPLE ${override}；支持的平台见 PLATFORMS.md`);
  }
  const p = platform();
  const a = arch();
  if (p === "darwin" && a === "arm64") return "aarch64-apple-darwin";
  if (p === "darwin" && a === "x64") return "x86_64-apple-darwin";
  if (p === "linux" && a === "x64") return "x86_64-unknown-linux-gnu";
  if (p === "linux" && a === "arm64") return "aarch64-unknown-linux-gnu";
  if (p === "win32" && a === "x64") return "x86_64-pc-windows-msvc";
  fail(`不支持的平台 ${p}/${a}；支持的平台见 PLATFORMS.md`);
}

function assetName() {
  return `ailoom-${triple()}${platform() === "win32" ? ".exe" : ""}`;
}

function binaryPath() {
  const dir = process.env.AILOOM_BIN_DIR ?? join(homedir(), ".ailoom", "npm", VERSION);
  return join(dir, assetName());
}

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const expectedSum = (text) => text.trim().split(/\s+/)[0];

function verify(bin) {
  // 校验文件与二进制同源发布；缺失或不匹配即拒绝运行
  const sumFile = `${bin}.sha256`;
  if (!existsSync(sumFile)) fail(`缺少校验文件 ${sumFile}，拒绝运行未校验的二进制`);
  if (sha256(readFileSync(bin)) !== expectedSum(readFileSync(sumFile, "utf8"))) {
    fail(`${bin} 的 sha256 与校验文件不一致，拒绝运行；删除 ${dirname(bin)} 后重试会重新下载`);
  }
}

async function fetchBytes(url) {
  let res;
  try {
    res = await fetch(url);
  } catch (e) {
    fail(`下载失败：${url}（${e.cause?.code ?? e.message}）。内网环境可设置 AILOOM_DOWNLOAD_BASE 指向镜像`);
  }
  if (!res.ok) fail(`下载失败：${url}（HTTP ${res.status}）`);
  return Buffer.from(await res.arrayBuffer());
}

async function download(bin) {
  const name = assetName();
  console.error(`[ailoom] 首次运行，下载 ${name} v${VERSION} …`);
  const sum = expectedSum((await fetchBytes(`${BASE_URL}/${name}.sha256`)).toString("utf8"));
  const body = await fetchBytes(`${BASE_URL}/${name}`);
  if (sha256(body) !== sum) fail(`下载的 ${name} sha256 不匹配，已丢弃`);
  mkdirSync(dirname(bin), { recursive: true });
  // 先写校验文件、再原子改名二进制：中途中断只会留下没有二进制的目录，下次重新下载
  const tmp = `${bin}.tmp-${process.pid}`;
  try {
    writeFileSync(`${bin}.sha256`, `${sum}  ${name}\n`);
    writeFileSync(tmp, body, { mode: 0o755 });
    renameSync(tmp, bin);
  } finally {
    rmSync(tmp, { force: true });
  }
}

const bin = binaryPath();
if (!existsSync(bin)) {
  if (process.env.AILOOM_BIN_DIR) {
    fail(`未找到 ${bin}；AILOOM_BIN_DIR 目录里需要放好对应平台的二进制与 .sha256`);
  }
  await download(bin);
}
verify(bin);
const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (result.error) fail(`无法启动 ${bin}：${result.error.message}`);
process.exit(result.status ?? 1);
