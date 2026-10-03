#!/usr/bin/env node
/**
 * npm wrapper for AILoom: the package ships the ailoom binary for every supported platform in bin/;
 * this picks the one for the current system and passes all arguments through.
 *
 * AILOOM_BIN_DIR: run the binary from this directory instead (offline or custom builds); it must
 * come with a matching .sha256 file and is verified before every run.
 */
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { arch, platform } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { PLATFORMS, binaryName } from "./platforms.js";

function fail(message) {
  console.error(`[ailoom] ${message}`);
  process.exit(1);
}

function currentPlatform() {
  // Explicit override (tests); anything outside the supported list is rejected
  const override = process.env.AILOOM_TRIPLE;
  const found = override
    ? PLATFORMS.find((p) => p.triple === override)
    : PLATFORMS.find((p) => p.os === platform() && p.cpu === arch());
  if (!found) {
    fail(`unsupported platform ${override ?? `${platform()}/${arch()}`}; see PLATFORMS.md for supported platforms`);
  }
  return found;
}

function fromBinDir(dir, p) {
  const bin = join(dir, binaryName(p));
  if (!existsSync(bin)) fail(`${bin} not found; AILOOM_BIN_DIR must contain the binary for this platform`);
  const sumFile = `${bin}.sha256`;
  if (!existsSync(sumFile)) fail(`missing checksum ${sumFile}; refusing to run an unverified binary`);
  const expected = readFileSync(sumFile, "utf8").trim().split(/\s+/)[0];
  const actual = createHash("sha256").update(readFileSync(bin)).digest("hex");
  if (actual !== expected) fail(`${bin} does not match ${sumFile}; refusing to run`);
  return bin;
}

function fromPackage(p) {
  const bin = join(fileURLToPath(new URL("./bin/", import.meta.url)), binaryName(p));
  if (!existsSync(bin)) fail(`this ailoom-cli package has no binary for ${p.triple}; reinstall: npm install -g ailoom-cli`);
  return bin;
}

const p = currentPlatform();
const bin = process.env.AILOOM_BIN_DIR ? fromBinDir(process.env.AILOOM_BIN_DIR, p) : fromPackage(p);
const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (result.error) fail(`cannot start ${bin}: ${result.error.message}`);
process.exit(result.status ?? 1);
