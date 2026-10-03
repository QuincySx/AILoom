# Supported platforms

| OS | Architecture | npm package |
|---|---|---|
| macOS | Apple silicon / Intel | `ailoom-cli-darwin-arm64` / `ailoom-cli-darwin-x64` |
| Linux (glibc) | x64 / arm64 | `ailoom-cli-linux-x64` / `ailoom-cli-linux-arm64` |
| Windows | x64 | `ailoom-cli-win32-x64` |

`ailoom-cli` lists all of them as optional dependencies, and npm installs only the one matching your system. Installing with `--omit=optional` (or `--no-optional`) skips it, and `ailoom` then reports the missing package.

## Offline or custom builds

Put a binary named `ailoom-<target triple>` (for example `ailoom-aarch64-apple-darwin`, or `ailoom-x86_64-pc-windows-msvc.exe`) and its `.sha256` file in a directory, and set `AILOOM_BIN_DIR` to it. The wrapper verifies the checksum before every run and refuses to run on a mismatch.

## Versions

All packages share the `Cargo.toml` version; the release workflow writes it and pins the platform packages to exactly that version.
