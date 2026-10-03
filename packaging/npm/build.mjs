#!/usr/bin/env node
/**
 * Assembles the ailoom-cli npm package for one release:
 *
 *   node packaging/npm/build.mjs <dist-dir> <version> <out-dir>
 *
 * <dist-dir> holds the release binaries ailoom-<triple>[.exe] built by release.yml; all of them are
 * copied into <out-dir>/bin/ next to the wrapper. Prints <out-dir> when done.
 */
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PLATFORMS, binaryName } from "./platforms.js";

const [dist, version, out] = process.argv.slice(2);
if (!dist || !version || !out) {
  console.error("usage: node build.mjs <dist-dir> <version> <out-dir>");
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(here, "package.json"), "utf8"));

mkdirSync(join(out, "bin"), { recursive: true });
for (const p of PLATFORMS) {
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
writeFileSync(join(out, "package.json"), `${JSON.stringify({ ...manifest, version }, null, 2)}\n`);
console.log(out);
