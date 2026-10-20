#!/usr/bin/env node
/**
 * Assembles the ailoom-cli npm package for one release:
 *
 *   node packaging/npm/build.mjs <dist-dir> <version> <out-dir> [local-triple]
 *
 * <dist-dir> holds the release binaries ailoom-<triple>[.exe] built by release.yml; all of them are
 * copied into <out-dir>/bin/ next to the wrapper. A local-triple assembles a private,
 * single-platform test package instead. Prints <out-dir> when done.
 */
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PLATFORMS, binaryName } from "./platforms.js";

const [dist, version, out, localTriple, ...extra] = process.argv.slice(2);
const platforms = localTriple ? PLATFORMS.filter((p) => p.triple === localTriple) : PLATFORMS;
if (!dist || !version || !out || extra.length || !platforms.length) {
  console.error("usage: node build.mjs <dist-dir> <version> <out-dir> [local-triple]");
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(here, "package.json"), "utf8"));

mkdirSync(join(out, "bin"), { recursive: true });
for (const p of platforms) {
  const src = join(dist, binaryName(p));
  if (!existsSync(src)) {
    console.error(`missing binary ${src}`);
    process.exit(1);
  }
  const bin = join(out, "bin", binaryName(p));
  copyFileSync(src, bin);
  chmodSync(bin, 0o755);
}
for (const file of manifest.files.filter((f) => f !== "bin")) copyFileSync(join(here, file), join(out, file));
copyFileSync(join(here, "README.md"), join(out, "README.md"));
copyFileSync(resolve(here, "../../LICENSE"), join(out, "LICENSE"));
// Repository-only scripts are not shipped with the npm package.
delete manifest.scripts;
const local = localTriple ? { private: true, os: [platforms[0].os], cpu: [platforms[0].cpu] } : {};
writeFileSync(join(out, "package.json"), `${JSON.stringify({ ...manifest, version, ...local }, null, 2)}\n`);
console.log(out);
