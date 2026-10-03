# 支持的平台

| 系统 | 架构 | 二进制 |
|---|---|---|
| macOS | Apple 芯片 / Intel | `ailoom-aarch64-apple-darwin` / `ailoom-x86_64-apple-darwin` |
| Linux（glibc） | x64 / arm64 | `ailoom-x86_64-unknown-linux-gnu` / `ailoom-aarch64-unknown-linux-gnu` |
| Windows | x64 | `ailoom-x86_64-pc-windows-msvc.exe` |

## 下载与离线

- 首次运行从 `https://github.com/QuincySx/AILoom/releases/download/v<版本>/` 下载上表对应的二进制和同名 `.sha256`，校验通过后缓存到 `~/.ailoom/npm/<版本>/`；之后每次运行前都会重新校验。
- 内网镜像：`AILOOM_DOWNLOAD_BASE=<镜像目录>`，目录里放着与 Release 同名的文件。
- 完全离线：把二进制和 `.sha256` 放进一个目录，设置 `AILOOM_BIN_DIR=<该目录>`，不会再联网。
- 校验文件缺失或不一致时拒绝运行。

## 版本对应

npm 包版本与 `Cargo.toml` 版本一致，由 release workflow 发布时写入；包装器下载同版本的 Release 二进制。
