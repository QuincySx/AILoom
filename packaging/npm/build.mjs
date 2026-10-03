#!/usr/bin/env node
/**
 * Assembles the npm packages for one release:
 *
 *   node packaging/npm/build.mjs <dist-dir> <version> <out-dir>
 *
 * <dist-dir> holds the release binaries ailoom-<triple>[.exe] built by release.yml. Writes
 * <out-dir>/ailoom-cli-<os>-<cpu>/ for every platform and <out-dir>/ailoom-cli/ (the wrapper, which
 * pins every platform package as an optional dependency), then prints the package directories in
 * publish order: platform packages first, wrapper last.
 */
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PLATFORMS, artifactName, binaryName, packageName } from "./platforms.js";

const [dist, version, out] = process.argv.slice(2);
if (!dist || !version || !out) {
  console.error("usage: node build.mjs <dist-dir> <version> <out-dir>");
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const license = resolve(here, "../../LICENSE");
const wrapper = JSON.parse(readFileSync(join(here, "package.json"), "utf8"));
const shared = {
  license: wrapper.license,
  repository: wrapper.repository,
  homepage: wrapper.homepage,
  bugs: wrapper.bugs,
};
const writeJson = (file, value) => writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);

const order = [];
for (const p of PLATFORMS) {
  const src = join(dist, artifactName(p));
  if (!existsSync(src)) {
    console.error(`missing binary ${src}`);
    process.exit(1);
  }
  const dir = join(out, packageName(p));
  mkdirSync(join(dir, "bin"), { recursive: true });
  const bin = join(dir, "bin", binaryName(p));
  copyFileSync(src, bin);
  chmodSync(bin, 0o755);
  copyFileSync(license, join(dir, "LICENSE"));
  writeFileSync(
    join(dir, "README.md"),
    `# ${packageName(p)}\n\nThe ailoom binary for ${p.os} ${p.cpu}. Install [ailoom-cli](https://www.npmjs.com/package/ailoom-cli) instead; npm picks this package automatically.\n`,
  );
  writeJson(join(dir, "package.json"), {
    name: packageName(p),
    version,
    description: `ailoom binary for ${p.os} ${p.cpu}`,
    ...shared,
    os: [p.os],
    cpu: [p.cpu],
    ...(p.libc ? { libc: [p.libc] } : {}),
    files: ["bin"],
    preferUnplugged: true,
  });
  order.push(dir);
}

const main = join(out, wrapper.name);
mkdirSync(main, { recursive: true });
for (const file of [...wrapper.files, "README.md"]) copyFileSync(join(here, file), join(main, file));
copyFileSync(license, join(main, "LICENSE"));
writeJson(join(main, "package.json"), {
  ...wrapper,
  version,
  optionalDependencies: Object.fromEntries(PLATFORMS.map((p) => [packageName(p), version])),
});
order.push(main);
console.log(order.join("\n"));
