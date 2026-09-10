# 平台矩阵（npm 包装器）

| OS | Arch | 三元组 | 状态 |
|---|---|---|---|
| macOS (darwin) | arm64 / x64 | aarch64-apple-darwin | 设计支持（发布前验证） |
| Linux | x64 / arm64 | x86_64 / aarch64-unknown-linux-gnu | 设计支持（发布前验证） |
| Windows | x64 | x86_64-pc-windows-msvc | 设计支持（发布前验证） |

## 下载与离线

- 制品：`{BASE}/{version}/ailoom-{version}-{三元组}[.exe]` + 同名 `.sha256`
- `AILOOM_DOWNLOAD_BASE` 可指向内网镜像；完全离线环境手动放置二进制到 `~/.ailoom/bin/`
- 校验：安装前 sha256 必须匹配 `.sha256`；失败拒绝运行

## 版本对应

- npm 包版本与 Rust crate 版本一一对应（release workflow 由 Cargo.toml tag 触发并注入 VERSION 文件）
