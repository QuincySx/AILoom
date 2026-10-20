#!/usr/bin/env node
// Build and install the current checkout through the same npm wrapper as a release.
import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, realpathSync, renameSync, symlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { PLATFORMS, binaryName } from "./platforms.js";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const childEnv = { ...process.env };
delete childEnv.AILOOM_BIN_DIR;
delete childEnv.AILOOM_TRIPLE;

function run(command, args, { cwd = repo, capture = false } = {}) {
  const result = spawnSync(command, args, {
    cwd, env: childEnv, encoding: "utf8", stdio: capture ? ["ignore", "pipe", "pipe"] : "inherit",
  });
  if (result.error || result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} 失败\n${result.error?.message || result.stderr?.trim() || `退出码 ${result.status}`}`);
  }
  return result.stdout?.trim();
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function checkEntry(entry, builds) {
  let stat;
  try { stat = lstatSync(entry); } catch (error) {
    if (error.code === "ENOENT") return;
    throw error;
  }
  const target = stat.isSymbolicLink() ? resolve(dirname(entry), readlinkSync(entry)) : "";
  const inside = target && relative(builds, target);
  if (!inside || isAbsolute(inside) || inside === ".." || inside.startsWith(`..${sep}`)
      || inside.split(sep).length !== 2 || !inside.endsWith(`${sep}launcher.mjs`)) {
    throw new Error(`不会覆盖已有文件：${entry}\n请用 --bin-dir 指定另一个测试入口目录。`);
  }
}

function main() {
  const { values } = parseArgs({ options: {
    prefix: { type: "string" }, "bin-dir": { type: "string" },
    "no-restart": { type: "boolean", default: false }, help: { type: "boolean", short: "h" },
  } });
  if (values.help) {
    console.log("npm --prefix packaging/npm run install:local -- [--prefix 路径] [--bin-dir 路径] [--no-restart]\n默认安装到 ~/.local/opt/ailoom-test，入口为 ~/.local/bin/ailoom-test；已有网页服务会切换到新包。\n--no-restart 仅安装，不连接或重启服务。");
    return;
  }
  const platform = PLATFORMS.find((p) => p.os === process.platform && p.cpu === process.arch);
  if (!platform || process.platform === "win32") {
    throw new Error("本地测试安装脚本目前支持 macOS / Linux；Windows 请使用发布包。 ");
  }
  if (!process.env.npm_execpath || !existsSync(process.env.npm_execpath)) {
    throw new Error("请通过 npm --prefix packaging/npm run install:local 执行。");
  }
  const npm = (args, options) => run(process.execPath, [process.env.npm_execpath, ...args], options);
  const prefix = resolve(values.prefix || join(homedir(), ".local", "opt", "ailoom-test"));
  const binDir = resolve(values["bin-dir"] || join(homedir(), ".local", "bin"));
  mkdirSync(prefix, { recursive: true });
  const builds = join(realpathSync(prefix), "builds");
  const entry = join(binDir, "ailoom-test");
  const entryCommand = `'${entry.replaceAll("'", "'\\''")}'`;
  checkEntry(entry, builds);

  console.log("[1/4] 编译当前源码（release）…");
  const metadata = JSON.parse(run("cargo", ["metadata", "--locked", "--no-deps", "--format-version", "1"], { capture: true }));
  run("cargo", ["build", "--locked", "--release", "--bin", "ailoom", "--target", platform.triple]);
  const binary = join(metadata.target_directory, platform.triple, "release", "ailoom");
  const hash = sha256(binary);
  const manifest = JSON.parse(readFileSync(join(here, "package.json"), "utf8"));
  const version = `${manifest.version}-local.sha${hash.slice(0, 12)}`;
  mkdirSync(builds, { recursive: true });
  const candidate = mkdtempSync(join(builds, `${version}-`));
  const dist = join(candidate, "dist");
  const packaged = join(candidate, "package");
  const artifacts = join(candidate, "artifacts");
  for (const dir of [dist, artifacts]) mkdirSync(dir);
  copyFileSync(binary, join(dist, binaryName(platform)));

  console.log("[2/4] 生成本机 npm 测试包并安装（离线、禁用安装脚本）…");
  run(process.execPath, [join(here, "build.mjs"), dist, version, packaged, platform.triple]);
  const packed = JSON.parse(npm(["pack", "--ignore-scripts", "--offline", "--json", "--pack-destination", artifacts], { cwd: packaged, capture: true }));
  const tarball = join(artifacts, packed[0].filename);
  writeFileSync(join(candidate, "package.json"), `${JSON.stringify({ private: true }, null, 2)}\n`);
  npm(["install", "--prefix", candidate, "--ignore-scripts", "--offline", "--no-audit", "--no-fund", "--save-exact", tarball]);

  console.log("[3/4] 验证安装后的入口与二进制…");
  const installed = join(candidate, "node_modules", "ailoom-cli");
  const cli = join(installed, "cli.js");
  const launcher = join(candidate, "launcher.mjs");
  writeFileSync(launcher, '#!/usr/bin/env node\ndelete process.env.AILOOM_BIN_DIR;\ndelete process.env.AILOOM_TRIPLE;\nawait import("./node_modules/ailoom-cli/cli.js");\n', { mode: 0o755 });
  const installedBinary = join(installed, "bin", binaryName(platform));
  if (sha256(installedBinary) !== hash) throw new Error("安装后的构建 SHA256 与编译产物不同，未更新入口。");
  const cliVersion = run(process.execPath, [launcher, "--version"], { capture: true });
  if (cliVersion !== `ailoom ${manifest.version}`) throw new Error(`版本不一致：${cliVersion}，未更新入口。`);
  const status = () => JSON.parse(run(process.execPath, [cli, "--json", "service", "status"], { capture: true })).result;
  const before = values["no-restart"] ? null : status();
  if (before?.running && before.state !== "running") {
    throw new Error(`网页服务状态为 ${before.state}，未切换入口。请先处理 service status，或用 --no-restart 仅安装。`);
  }
  mkdirSync(binDir, { recursive: true });
  // Recheck after compilation; switch a verified, immutable installation atomically.
  checkEntry(entry, builds);
  const pending = `${entry}.${randomUUID()}`;
  symlinkSync(launcher, pending);
  renameSync(pending, entry);

  console.log("[4/4] 更新测试入口与网页服务…");
  if (before?.running) {
    try {
      run(process.execPath, [cli, "--json", "service", "restart"], { capture: true });
      const after = status();
      if (!after.running || after.update_required !== false || after.build?.sha256 !== hash
          || after.build?.executable !== realpathSync(installedBinary) || after.port !== before.port) {
        throw new Error("网页服务的程序、构建或端口与安装结果不一致。");
      }
      console.log(`网页已切换：http://127.0.0.1:${after.port}（刷新已打开的页面）`);
    } catch (error) {
      throw new Error(`测试包已安装，网页切换失败：${error.message}\n请运行 ${entryCommand} service restart 后重试。`);
    }
  }
  if (before?.autostart?.enabled) {
    console.log(`登录自启动保留原入口；如需改用测试版，运行：${entryCommand} service enable --port ${before.port || 47831}`);
  }
  console.log(`安装完成：${version}\n入口：${entry}\n程序：${installedBinary}\nSHA256：${hash}\n打开网页：${entryCommand} web`);
  if (!(process.env.PATH || "").split(delimiter).some((part) => resolve(part) === binDir)) {
    console.log(`当前 PATH 没有 ${binDir}；可直接使用上面的完整入口路径。`);
  }
}

try { main(); } catch (error) {
  console.error(`本地测试安装失败：${error.message}`);
  process.exitCode = 1;
}
