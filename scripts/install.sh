#!/bin/sh
# AILoom 一键安装（T10 设计稿 + 本机路径可用）
# 优先级：1) 已有 cargo → cargo install --path .（源码构建，最可信）
#         2) 配置了 AILOOM_DOWNLOAD_BASE → 下载 release 二进制 + sha256 校验
set -eu

cd "$(dirname "$0")/.."

say() { printf '[ailoom-install] %s\n' "$*"; }

# 1) 源码构建路径（当前主推：仓库即源）
if command -v cargo >/dev/null 2>&1; then
  say "使用 cargo 从源码构建并安装（cargo install --path .）…"
  cargo install --path . --locked
  say "安装完成：$(command -v aloom 2>/dev/null || echo '~/.cargo/bin/ailoom')"
  say "验证：ailoom version"
  exit 0
fi

# 2) 二进制下载路径（release base 由 AILOOM_DOWNLOAD_BASE 提供）
BASE="${AILOOM_DOWNLOAD_BASE:-}"
if [ -z "$BASE" ]; then
  say "未找到 cargo，且未设置 AILOOM_DOWNLOAD_BASE（release 下载源）。"
  say "两条路："
  say "  a) 安装 Rust 工具链（https://rustup.rs）后重跑本脚本"
  say "  b) 设置 AILOOM_DOWNLOAD_BASE 指向内网镜像后重跑"
  exit 1
fi

TRIPLE=""
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64)  TRIPLE="aarch64-apple-darwin" ;;
  Darwin/x86_64) TRIPLE="x86_64-apple-darwin" ;;
  Linux/x86_64)  TRIPLE="x86_64-unknown-linux-gnu" ;;
  Linux/aarch64) TRIPLE="aarch64-unknown-linux-gnu" ;;
  *) say "不支持的平台：$(uname -s)/$(uname -m)"; exit 1 ;;
esac

BIN_DIR="${AILOOM_BIN_DIR:-$HOME/.ailoom/bin}"
mkdir -p "$BIN_DIR"
ARCHIVE="$BIN_DIR/ailoom-$TRIPLE"
URL="$BASE/ailoom-$TRIPLE"

say "下载 $URL …"
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$URL" -o "$ARCHIVE" || { say "下载失败：检查网络/代理（HTTPS_PROXY）或 AILOOM_DOWNLOAD_BASE"; exit 1; }
  curl -fsSL "$URL.sha256" -o "$ARCHIVE.sha256" || say "（无 .sha256，跳过校验——不推荐）"
else
  say "需要 curl 下载"; exit 1
fi

if [ -f "$ARCHIVE.sha256" ]; then
  expected=$(cut -d' ' -f1 "$ARCHIVE.sha256")
  actual=$(/usr/bin/shasum -a 256 "$ARCHIVE" 2>/dev/null | cut -d' ' -f1 || sha256sum "$ARCHIVE" | cut -d' ' -f1)
  [ "$actual" = "$expected" ] || { say "sha256 校验失败，拒绝安装"; exit 1; }
fi

chmod +x "$ARCHIVE"
ln -sf "$ARCHIVE" "$BIN_DIR/ailoom"
say "安装完成：$BIN_DIR/ailoom（确保 $BIN_DIR 在 PATH 中）"
say "验证：ailoom version"
