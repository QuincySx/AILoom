// Supported platforms: Rust target triple and the Node.js platform / arch it runs on.
export const PLATFORMS = [
  { triple: "aarch64-apple-darwin", os: "darwin", cpu: "arm64" },
  { triple: "x86_64-apple-darwin", os: "darwin", cpu: "x64" },
  { triple: "x86_64-unknown-linux-gnu", os: "linux", cpu: "x64" },
  { triple: "aarch64-unknown-linux-gnu", os: "linux", cpu: "arm64" },
  { triple: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64", exe: true },
];

// Binary name in the package's bin/ directory, in the release build artifacts and in AILOOM_BIN_DIR
export const binaryName = (p) => `ailoom-${p.triple}${p.exe ? ".exe" : ""}`;
