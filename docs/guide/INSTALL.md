# 安装 AILoom

> 当前尚未发布预编译制品与 npm 包（未经明确发布指令不发布）。可用的安装方式是源码构建，或用 `scripts/install.sh` 自动完成构建与安装。

## 源码构建

```bash
git clone <本仓库>
cargo build --release
# 二进制：target/release/ailoom（加入 PATH 即可）
cargo test   # 可选：完整验收
```

要求：Rust 1.85+（MSRV，CI 固定检查）；无运行时 Rust 编译要求（未来二进制分发后）。

## 安装脚本 `scripts/install.sh`

```bash
scripts/install.sh                                  # 默认 auto：有 cargo 就源码构建，否则下载
AILOOM_INSTALL_MODE=cargo scripts/install.sh        # 强制源码构建
AILOOM_INSTALL_MODE=download AILOOM_DOWNLOAD_BASE=<制品地址> scripts/install.sh   # 强制下载（需制品地址）
scripts/install.sh uninstall                        # 只删除安装目录中的 AILoom 文件，不碰用户数据与配置
```

安装流程为：临时制品 → 校验 → 原子替换，任何失败都保留原有安装。安装目录由 `AILOOM_BIN_DIR` 指定，默认 `~/.ailoom/bin/`。注意：这个默认值仍沿用旧的数据根位置，与 XDG 迁移后的数据目录不一致，见 AIL-147。

## 设计：分平台二进制 + npm 包装（尚未发布）


### 平台矩阵

| OS | Arch | 三元组 |
|---|---|---|
| macOS | arm64 / x64 | aarch64-apple-darwin |
| Linux | x64 / arm64 | x86_64 / aarch64-unknown-linux-gnu |
| Windows | x64 | x86_64-pc-windows-msvc |

### 安装（npm 包装器）

```bash
npm install -g ailoom-cli
ailoom version   # 校验版本一致
```

包装器职责（且仅限）：定位平台二进制 → sha256 校验 → 透传参数。

### 升级

```bash
npm update -g ailoom-cli && ailoom version
# 各工作区数据（缓存/绑定/事件）向后兼容；schema 变更见 docs/CONTRACTS.md §0
```

### 卸载

```bash
npm uninstall -g ailoom-cli
# 不删除用户配置与业务仓库内容；名册/声明是团队共享数据，卸载只影响本机
```

### 下载失败诊断

- 代理环境：设置 `HTTPS_PROXY` 或 `AILOOM_DOWNLOAD_BASE`（内网镜像）
- 校验失败：删除半成品重新下载；`.sha256` 与制品必须同源
- 完全离线：手动放置二进制至 `AILOOM_BIN_DIR`（默认 `~/.ailoom/bin/`）

## 发布前必查（引用 docs/RELEASE-CHECKLIST.md）

- 包名 `ailoom-cli`（`packaging/npm/package.json`）的可用性在发布时重新核实（不视作商标授权）；`packaging/npm/cli.js` 的下载地址仍是占位值，发布前必须替换（AIL-147）
- 干净环境安装→升级→卸载全流程测试
- 未经明确发布指令不实际发布
