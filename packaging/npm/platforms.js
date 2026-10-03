// Supported platforms: Rust target triple -> npm platform package and its os / cpu fields.
export const PLATFORMS = [
  { triple: "aarch64-apple-darwin", os: "darwin", cpu: "arm64" },
  { triple: "x86_64-apple-darwin", os: "darwin", cpu: "x64" },
  { triple: "x86_64-unknown-linux-gnu", os: "linux", cpu: "x64", libc: "glibc" },
  { triple: "aarch64-unknown-linux-gnu", os: "linux", cpu: "arm64", libc: "glibc" },
  { triple: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64", exe: true },
];

export const packageName = (p) => `ailoom-cli-${p.os}-${p.cpu}`;
export const binaryName = (p) => (p.exe ? "ailoom.exe" : "ailoom");
// Name of the release build artifact (and of binaries placed in AILOOM_BIN_DIR)
export const artifactName = (p) => `ailoom-${p.triple}${p.exe ? ".exe" : ""}`;
