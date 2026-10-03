# Supported platforms

| OS | Architecture | Binary |
|---|---|---|
| macOS | Apple silicon / Intel | `ailoom-aarch64-apple-darwin` / `ailoom-x86_64-apple-darwin` |
| Linux (glibc) | x64 / arm64 | `ailoom-x86_64-unknown-linux-gnu` / `ailoom-aarch64-unknown-linux-gnu` |
| Windows | x64 | `ailoom-x86_64-pc-windows-msvc.exe` |

## Download and offline use

- On first run the binary for your platform and its `.sha256` are downloaded from `https://github.com/QuincySx/AILoom/releases/download/v<version>/`, verified, and cached in `~/.ailoom/npm/<version>/`. The cached binary is verified again before every run.
- Mirror: set `AILOOM_DOWNLOAD_BASE=<directory URL>` to a location holding files with the same names as the Release assets.
- Fully offline: put the binary and its `.sha256` in a directory and set `AILOOM_BIN_DIR=<that directory>`; nothing is downloaded.
- A missing or mismatching checksum stops the wrapper from running the binary.

## Versions

The npm package version equals the `Cargo.toml` version (written by the release workflow), and the wrapper downloads the Release binary of that same version.
