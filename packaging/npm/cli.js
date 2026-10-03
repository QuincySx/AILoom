#!/usr/bin/env node
/**
 * npm wrapper for AILoom: runs the ailoom binary for this platform and passes all arguments through.
 *
 * On first run it downloads the binary from the GitHub Release matching this package version,
 * verifies its sha256 and caches it in ~/.ailoom/npm/<version>/; every later run verifies it again.
 * - AILOOM_DOWNLOAD_BASE: alternative download location (mirror) holding ailoom-<triple> and .sha256
 * - AILOOM_BIN_DIR: use only the binary and .sha256 already in this directory, never download (offline)
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
  // Explicit override (tests, mirrors); anything outside the supported list is rejected
  const override = process.env.AILOOM_TRIPLE;
  if (override) {
    if (SUPPORTED_TRIPLES.includes(override)) return override;
    fail(`unsupported AILOOM_TRIPLE ${override}; see PLATFORMS.md for supported platforms`);
  }
  const p = platform();
  const a = arch();
  if (p === "darwin" && a === "arm64") return "aarch64-apple-darwin";
  if (p === "darwin" && a === "x64") return "x86_64-apple-darwin";
  if (p === "linux" && a === "x64") return "x86_64-unknown-linux-gnu";
  if (p === "linux" && a === "arm64") return "aarch64-unknown-linux-gnu";
  if (p === "win32" && a === "x64") return "x86_64-pc-windows-msvc";
  fail(`unsupported platform ${p}/${a}; see PLATFORMS.md for supported platforms`);
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
  // The checksum ships with the binary; refuse to run when it is missing or does not match
  const sumFile = `${bin}.sha256`;
  if (!existsSync(sumFile)) fail(`missing checksum ${sumFile}; refusing to run an unverified binary`);
  if (sha256(readFileSync(bin)) !== expectedSum(readFileSync(sumFile, "utf8"))) {
    fail(`${bin} does not match its sha256; refusing to run. Delete ${dirname(bin)} to download it again`);
  }
}

async function fetchBytes(url) {
  let res;
  try {
    res = await fetch(url);
  } catch (e) {
    fail(`download failed: ${url} (${e.cause?.code ?? e.message}). Behind a firewall, set AILOOM_DOWNLOAD_BASE to a mirror`);
  }
  if (!res.ok) fail(`download failed: ${url} (HTTP ${res.status})`);
  return Buffer.from(await res.arrayBuffer());
}

async function download(bin) {
  const name = assetName();
  console.error(`[ailoom] first run: downloading ${name} v${VERSION}...`);
  const sum = expectedSum((await fetchBytes(`${BASE_URL}/${name}.sha256`)).toString("utf8"));
  const body = await fetchBytes(`${BASE_URL}/${name}`);
  if (sha256(body) !== sum) fail(`downloaded ${name} does not match its sha256; discarded`);
  mkdirSync(dirname(bin), { recursive: true });
  // Write the checksum first, then rename the binary into place: an interrupted download leaves
  // no binary behind and the next run downloads again
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
    fail(`${bin} not found; AILOOM_BIN_DIR must contain the binary for this platform and its .sha256`);
  }
  await download(bin);
}
verify(bin);
const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (result.error) fail(`cannot start ${bin}: ${result.error.message}`);
process.exit(result.status ?? 1);
