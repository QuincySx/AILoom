#!/usr/bin/env node
/**
 * npm wrapper for AILoom: runs the ailoom binary from the platform package npm installed next to
 * this one (ailoom-cli-<os>-<cpu>, an optional dependency) and passes all arguments through.
 *
 * AILOOM_BIN_DIR: run ailoom-<triple> from this directory instead (offline or custom builds);
 * it must come with ailoom-<triple>.sha256 and is verified before every run.
 */
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { arch, platform } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { PLATFORMS, artifactName, binaryName, packageName } from "./platforms.js";

const VERSION = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")).version;

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
  const bin = join(dir, artifactName(p));
  if (!existsSync(bin)) fail(`${bin} not found; AILOOM_BIN_DIR must contain the binary for this platform`);
  const sumFile = `${bin}.sha256`;
  if (!existsSync(sumFile)) fail(`missing checksum ${sumFile}; refusing to run an unverified binary`);
  const expected = readFileSync(sumFile, "utf8").trim().split(/\s+/)[0];
  const actual = createHash("sha256").update(readFileSync(bin)).digest("hex");
  if (actual !== expected) fail(`${bin} does not match ${sumFile}; refusing to run`);
  return bin;
}

function fromPackage(p) {
  const name = packageName(p);
  let manifest;
  try {
    manifest = createRequire(import.meta.url).resolve(`${name}/package.json`);
  } catch {
    fail(`${name} is not installed. It is an optional dependency of ailoom-cli; reinstall without --omit=optional: npm install -g ailoom-cli`);
  }
  const { version } = JSON.parse(readFileSync(manifest, "utf8"));
  if (version !== VERSION) {
    fail(`${name} ${version} does not match ailoom-cli ${VERSION}; reinstall: npm install -g ailoom-cli@${VERSION}`);
  }
  return join(dirname(manifest), "bin", binaryName(p));
}

const p = currentPlatform();
const bin = process.env.AILOOM_BIN_DIR ? fromBinDir(process.env.AILOOM_BIN_DIR, p) : fromPackage(p);
const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (result.error) fail(`cannot start ${bin}: ${result.error.message}`);
process.exit(result.status ?? 1);
