# Supported platforms

| OS | Architecture | Binary in `bin/` |
|---|---|---|
| macOS | Apple silicon / Intel | `ailoom-aarch64-apple-darwin` / `ailoom-x86_64-apple-darwin` |
| Linux (glibc) | x64 / arm64 | `ailoom-x86_64-unknown-linux-gnu` / `ailoom-aarch64-unknown-linux-gnu` |
| Windows | x64 | `ailoom-x86_64-pc-windows-msvc.exe` |

`ailoom-cli` ships all of them and runs the one matching your system.

## Offline or custom builds

Put a binary with one of the names above and its `.sha256` file in a directory, and set `AILOOM_BIN_DIR` to it. The wrapper verifies the checksum before every run and refuses to run on a mismatch.

## Versions

The package version equals the `Cargo.toml` version; the release workflow writes it.
