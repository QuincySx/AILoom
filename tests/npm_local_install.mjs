import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, realpathSync, writeFileSync } from "node:fs";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { PLATFORMS, binaryName } from "../packaging/npm/platforms.js";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const version = JSON.parse(readFileSync(join(repo, "packaging/npm/package.json"), "utf8")).version;
const platform = PLATFORMS.find((p) => p.os === process.platform && p.cpu === process.arch);
const localTest = platform && process.platform !== "win32" ? test : test.skip;
const scratch = join(repo, ".scratch");
mkdirSync(scratch, { recursive: true });

function fixture() {
  const root = mkdtempSync(join(scratch, "npm-local-install-"));
  const tools = join(root, "tools");
  const prefix = join(root, "installation with spaces");
  const binDir = join(root, "bin with spaces");
  const service = join(root, "service-fixture.json");
  const calls = join(root, "cli-calls.log");
  mkdirSync(tools);
  writeFileSync(service, JSON.stringify({ running: false, state: "stopped" }));
  const binary = `#!/usr/bin/env node
import * as fs from "node:fs";
import * as crypto from "node:crypto";
import { fileURLToPath } from "node:url";
const filename = fileURLToPath(import.meta.url);
const path = ${JSON.stringify(service)};
const args = process.argv.slice(2).join(" ");
fs.appendFileSync(${JSON.stringify(calls)}, args + "\\n");
if (args === "--version") { console.log("ailoom ${version}"); process.exit(0); }
const build = { executable: fs.realpathSync(filename), sha256: crypto.createHash("sha256").update(fs.readFileSync(filename)).digest("hex"), version: ${JSON.stringify(version)} };
let state = JSON.parse(fs.readFileSync(path, "utf8"));
if (args === "--json service restart") {
  if (process.env.AILOOM_TEST_RESTART_FAILURE) process.exit(44);
  state = { running: true, state: "running", port: state.port || 47955, build };
  if (process.env.AILOOM_TEST_BAD_SERVICE_SHA) state.build.sha256 = "wrong-build";
  if (process.env.AILOOM_TEST_BAD_SERVICE_PORT) state.port += 1;
  fs.writeFileSync(path, JSON.stringify(state));
} else if (args !== "--json service status") { process.exit(2); }
console.log(JSON.stringify({ result: { ...state, update_required: state.running && state.build?.executable !== build.executable } }));
`;
  const cargo = `#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");
const target = ${JSON.stringify(join(root, "target"))};
if (process.argv[2] === "metadata") { console.log(JSON.stringify({ target_directory: target })); }
else if (process.argv[2] === "build") {
  if (process.env.AILOOM_TEST_BUILD_FAILURE) process.exit(42);
  const file = path.join(target, ${JSON.stringify(platform.triple)}, "release", "ailoom");
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, ${JSON.stringify(binary)} + "// revision " + (process.env.AILOOM_TEST_REVISION || "1") + "\\n", { mode: 0o755 });
} else { process.exit(2); }
`;
  writeFileSync(join(tools, "cargo"), cargo, { mode: 0o755 });
  const npmProxy = join(root, "npm-proxy.cjs");
  writeFileSync(npmProxy, `const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const args = process.argv.slice(2);
if (!args.includes("--ignore-scripts") || !args.includes("--offline")) process.exit(48);
if (args[0] === "pack" && process.env.AILOOM_TEST_PACK_FAILURE) process.exit(43);
const result = spawnSync("npm", args, { stdio: "inherit" });
if (result.status === 0 && args[0] === "install" && process.env.AILOOM_TEST_CORRUPT_INSTALL) {
  const prefix = args[args.indexOf("--prefix") + 1];
  fs.appendFileSync(path.join(prefix, "node_modules/ailoom-cli/bin", ${JSON.stringify(binaryName(platform))}), "// corrupt\\n");
}
process.exit(result.status ?? 1);
`);
  const npmrc = join(root, "npmrc");
  writeFileSync(npmrc, "");
  const entry = join(binDir, "ailoom-test");
  const install = (extra = [], env = {}) => spawnSync(process.execPath, [
    join(repo, "packaging/npm/install-local.mjs"), "--prefix", prefix, "--bin-dir", binDir, ...extra,
  ], { cwd: repo, encoding: "utf8", env: {
    ...process.env, PATH: `${tools}${delimiter}${process.env.PATH}`,
    npm_execpath: npmProxy, npm_config_userconfig: npmrc, npm_config_cache: join(root, "npm-cache"), ...env,
  } });
  return { root, prefix, binDir, entry, service, calls, install };
}

function success(result) {
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
}

localTest("local npm installation runs its wrapper, preserves previous builds and excludes repository scripts", () => {
  const f = fixture();
  success(f.install(["--no-restart"]));
  const old = readlinkSync(f.entry);
  const pkg = JSON.parse(readFileSync(join(dirname(old), "node_modules/ailoom-cli/package.json"), "utf8"));
  assert.equal(pkg.private, true);
  assert.equal(pkg.scripts, undefined);
  assert.deepEqual(pkg.os, [platform.os]);
  assert.deepEqual(pkg.cpu, [platform.cpu]);
  assert.match(pkg.version, new RegExp(`^${version.replaceAll(".", "\\.")}-local\\.sha`));
  const run = spawnSync(f.entry, ["--version"], { encoding: "utf8", env: {
    ...process.env, AILOOM_BIN_DIR: join(f.root, "missing-external-binary"), AILOOM_TRIPLE: "wrong-platform",
  } });
  success(run);
  assert.equal(run.stdout.trim(), `ailoom ${version}`);
  success(f.install(["--no-restart"], { AILOOM_TEST_REVISION: "2" }));
  assert.notEqual(readlinkSync(f.entry), old);
  assert.equal(existsSync(old), true);
  assert.equal(readFileSync(f.calls, "utf8").includes("service"), false);
});

localTest("failed compilation, packing and installed-binary verification preserve the previous entry", () => {
  const f = fixture();
  success(f.install(["--no-restart"]));
  const old = readlinkSync(f.entry);
  const oldBinary = join(dirname(old), "node_modules/ailoom-cli/bin", binaryName(platform));
  const hash = createHash("sha256").update(readFileSync(oldBinary)).digest("hex");
  for (const flag of ["AILOOM_TEST_BUILD_FAILURE", "AILOOM_TEST_PACK_FAILURE", "AILOOM_TEST_CORRUPT_INSTALL"]) {
    const result = f.install(["--no-restart"], { [flag]: "1", AILOOM_TEST_REVISION: "2" });
    assert.notEqual(result.status, 0, flag);
    assert.equal(readlinkSync(f.entry), old, flag);
    assert.equal(createHash("sha256").update(readFileSync(oldBinary)).digest("hex"), hash, flag);
  }
  success(spawnSync(f.entry, ["--version"], { encoding: "utf8" }));
});

localTest("an unrelated entry is never overwritten", () => {
  const f = fixture();
  mkdirSync(f.binDir);
  writeFileSync(f.entry, "user-owned\n");
  const result = f.install(["--no-restart"]);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /不会覆盖/);
  assert.equal(readFileSync(f.entry, "utf8"), "user-owned\n");
});

localTest("a running managed service switches to the installed binary and retains its port", () => {
  const f = fixture();
  writeFileSync(f.service, JSON.stringify({ running: true, state: "running", port: 47956, build: { executable: "old" } }));
  success(f.install());
  const state = JSON.parse(readFileSync(f.service, "utf8"));
  const installedBinary = join(dirname(realpathSync(f.entry)), "node_modules/ailoom-cli/bin", binaryName(platform));
  assert.equal(state.port, 47956);
  assert.equal(state.build.executable, realpathSync(installedBinary));
  assert.equal(state.build.sha256, createHash("sha256").update(readFileSync(installedBinary)).digest("hex"));
  assert.match(readFileSync(f.calls, "utf8"), /service restart/);
});

localTest("an unmanaged service blocks activation; installation-only mode does not contact it", () => {
  const f = fixture();
  success(f.install(["--no-restart"]));
  const old = readlinkSync(f.entry);
  writeFileSync(f.service, JSON.stringify({ running: true, state: "unmanaged", port: 47956 }));
  const result = f.install();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /unmanaged/);
  assert.equal(readlinkSync(f.entry), old);
  success(f.install(["--no-restart"]));
  assert.equal(readFileSync(f.calls, "utf8").includes("service restart"), false);
});

localTest("a restart failure reports the installed entry and a quoted recovery command", () => {
  const f = fixture();
  writeFileSync(f.service, JSON.stringify({ running: true, state: "running", port: 47956, build: { executable: "old" } }));
  const result = f.install([], { AILOOM_TEST_RESTART_FAILURE: "1" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /测试包已安装，网页切换失败/);
  assert.equal(result.stdout.includes("安装完成："), false);
  assert.equal(result.stderr.includes(`'${f.entry}' service restart`), true);
  success(spawnSync(f.entry, ["--version"], { encoding: "utf8" }));
});

localTest("a restart with the wrong service hash or port is never reported as success", () => {
  for (const flag of ["AILOOM_TEST_BAD_SERVICE_SHA", "AILOOM_TEST_BAD_SERVICE_PORT"]) {
    const f = fixture();
    writeFileSync(f.service, JSON.stringify({ running: true, state: "running", port: 47956, build: { executable: "old" } }));
    const result = f.install([], { [flag]: "1" });
    assert.notEqual(result.status, 0, flag);
    assert.match(result.stderr, /网页服务的程序、构建或端口与安装结果不一致/);
    assert.equal(result.stdout.includes("安装完成："), false);
    success(spawnSync(f.entry, ["--version"], { encoding: "utf8" }));
  }
});
