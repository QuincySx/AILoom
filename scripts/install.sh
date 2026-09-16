#!/bin/sh
# AILoom 一键安装（AIL-029 返工：临时制品 → 校验 → 原子替换；所有失败分支保留原安装）
#
# 优先级：AILOOM_INSTALL_MODE=auto（缺省，本机行为：有 cargo 用源码构建，否则下载）
#         AILOOM_INSTALL_MODE=cargo    强制源码构建
#         AILOOM_INSTALL_MODE=download 强制二进制下载（需 AILOOM_DOWNLOAD_BASE）
# 卸载：  scripts/install.sh uninstall   （只删除 $AILOOM_BIN_DIR 内 AILoom 自身文件，不碰用户配置）
set -eu

cd "$(dirname "$0")/.."

say() { printf '[ailoom-install] %s\n' "$*"; }
die() { printf '[ailoom-install] 错误：%s\n' "$*" >&2; exit 1; }

MODE="${AILOOM_INSTALL_MODE:-auto}"

# ---------------------------------------------------------------------------
# 卸载：仅移除 AILoom 自身安装物（二进制/校验文件/链接），不删除任何用户配置
# ---------------------------------------------------------------------------
if [ "${1:-}" = "uninstall" ]; then
  BIN_DIR="${AILOOM_BIN_DIR:-$HOME/.ailoom/bin}"
  if [ ! -d "$BIN_DIR" ]; then
    say "未发现安装目录 ${BIN_DIR}，无需卸载"
    exit 0
  fi
  removed=0
  for f in "$BIN_DIR/ailoom" "$BIN_DIR"/ailoom-* "$BIN_DIR"/ailoom-*.sha256; do
    if [ -e "$f" ] || [ -L "$f" ]; then
      rm -f "$f"
      removed=$((removed + 1))
    fi
  done
  say "已卸载：删除 $BIN_DIR 内 $removed 个 AILoom 安装文件（用户数据与配置未触碰）"
  exit 0
fi

# ---------------------------------------------------------------------------
# 1) 源码构建路径
# ---------------------------------------------------------------------------
if [ "$MODE" = "cargo" ] || { [ "$MODE" = "auto" ] && command -v cargo >/dev/null 2>&1; }; then
  [ "$MODE" = "cargo" ] || command -v cargo >/dev/null 2>&1 || die "cargo 不可用"
  say "使用 cargo 从源码构建并安装（cargo install --path .）…"
  cargo install --path . --locked
  say "安装完成：$(command -v ailoom 2>/dev/null || echo '~/.cargo/bin/ailoom')"
  say "验证：ailoom version"
  exit 0
fi

# ---------------------------------------------------------------------------
# 2) 二进制下载路径：下载到临时文件 → sha256 校验 → 原子替换 → 软链
#    任何失败分支都保留既有 binary/软链，且不执行未校验文件
# ---------------------------------------------------------------------------
BASE="${AILOOM_DOWNLOAD_BASE:-}"
[ -n "$BASE" ] || die "未设置 AILOOM_DOWNLOAD_BASE（release 下载源）；或安装 Rust 工具链后重跑（https://rustup.rs）"

SUPPORTED_TRIPLES=" aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu "
TRIPLE="${AILOOM_TRIPLE:-}"
if [ -z "$TRIPLE" ]; then
  case "$(uname -s)/$(uname -m)" in
    Darwin/arm64)  TRIPLE="aarch64-apple-darwin" ;;
    Darwin/x86_64) TRIPLE="x86_64-apple-darwin" ;;
    Linux/x86_64)  TRIPLE="x86_64-unknown-linux-gnu" ;;
    Linux/aarch64) TRIPLE="aarch64-unknown-linux-gnu" ;;
    *) die "不支持的平台：$(uname -s)/$(uname -m)（支持矩阵见 packaging/npm/PLATFORMS.md）" ;;
  esac
else
  case "$SUPPORTED_TRIPLES" in
    *" $TRIPLE "*) ;;
    *) die "不支持的 AILOOM_TRIPLE：${TRIPLE}（支持矩阵见 packaging/npm/PLATFORMS.md）" ;;
  esac
fi

BIN_DIR="${AILOOM_BIN_DIR:-$HOME/.ailoom/bin}"
mkdir -p "$BIN_DIR"
FINAL="$BIN_DIR/ailoom-$TRIPLE"
SHA_FILE="$BIN_DIR/ailoom-$TRIPLE.sha256"
LINK="$BIN_DIR/ailoom"
URL="$BASE/ailoom-$TRIPLE"

# 临时文件与 FINAL 同目录（保证 rename 原子性）；带 $$ 避免并发冲突
TMP="$BIN_DIR/.ailoom-$TRIPLE.tmp.$$"
TMP_SHA="$BIN_DIR/.ailoom-$TRIPLE.sha256.tmp.$$"
cleanup_tmp() { rm -f "$TMP" "$TMP_SHA"; }
trap cleanup_tmp EXIT INT TERM

command -v curl >/dev/null 2>&1 || die "需要 curl 下载"

say "下载 $URL …"
curl -fsSL --connect-timeout 10 "$URL" -o "$TMP" \
  || die "下载失败：检查网络/代理（HTTPS_PROXY）或 AILOOM_DOWNLOAD_BASE"
curl -fsSL --connect-timeout 10 "$URL.sha256" -o "$TMP_SHA" \
  || die "缺少 $URL.sha256，拒绝安装（必须校验）"

# 显式探测可用 sha256 工具；不可用/计算失败一律 fail closed
expected=$(cut -d' ' -f1 "$TMP_SHA")
[ -n "$expected" ] || die "校验文件为空或格式非法，拒绝安装"
actual=""
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$TMP" 2>/dev/null | cut -d' ' -f1) \
    || die "sha256sum 计算失败，拒绝安装"
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$TMP" 2>/dev/null | cut -d' ' -f1) \
    || die "shasum 计算失败，拒绝安装"
else
  die "未找到 sha256sum 或 shasum，无法校验制品完整性，拒绝安装"
fi
[ -n "$actual" ] || die "sha256 计算结果为空，拒绝安装"
if [ "$actual" != "$expected" ]; then
  die "sha256 校验失败：expected=$expected actual=${actual}，拒绝安装"
fi

# ---------------------------------------------------------------------------
# 校验通过后事务化切换（RW-04/S04）：binary → 摘要 → 软链；
# 任一步失败整体恢复旧安装：旧 binary/摘要回位、链接重新指向旧安装、退出非零。
# 备份用 cp（不移动原文件），切换前先备份，失败时才能回滚。
# AILOOM_INJECT_FAILURE=move-binary|move-sha|link 仅为安装器测试注入点，缺省不生效。
# ---------------------------------------------------------------------------
chmod +x "$TMP"

FINAL_BAK="$BIN_DIR/.ailoom-$TRIPLE.bak.$$"
SHA_BAK="$BIN_DIR/.ailoom-$TRIPLE.sha256.bak.$$"
had_old=0
if [ -f "$FINAL" ]; then
  cp "$FINAL" "$FINAL_BAK" || die "备份旧安装失败，已保留原安装（未做任何修改）"
  had_old=1
fi
if [ -f "$SHA_FILE" ]; then
  cp "$SHA_FILE" "$SHA_BAK" || {
    rm -f "$FINAL_BAK"
    die "备份旧校验文件失败，已保留原安装（未做任何修改）"
  }
fi

restore_old() {
  if [ "$had_old" = "1" ]; then
    mv -f "$FINAL_BAK" "$FINAL"
    [ -f "$SHA_BAK" ] && mv -f "$SHA_BAK" "$SHA_FILE"
    ln -sfn "$FINAL" "$LINK" 2>/dev/null || true
  else
    rm -f "$FINAL" "$SHA_FILE"
    [ -L "$LINK" ] && rm -f "$LINK"
  fi
}

if [ "${AILOOM_INJECT_FAILURE:-}" = "move-binary" ]; then
  restore_old
  die "注入失败（move-binary）：已恢复旧安装"
fi
mv -f "$TMP" "$FINAL" || {
  restore_old
  die "binary 替换失败，已恢复旧安装"
}
if [ "${AILOOM_INJECT_FAILURE:-}" = "move-sha" ]; then
  restore_old
  die "注入失败（move-sha）：已恢复旧安装"
fi
mv -f "$TMP_SHA" "$SHA_FILE" || {
  restore_old
  die "摘要切换失败，已恢复旧安装"
}
if [ "${AILOOM_INJECT_FAILURE:-}" = "link" ]; then
  restore_old
  die "注入失败（link）：已恢复旧安装"
fi
ln -sfn "$FINAL" "$LINK" || {
  restore_old
  die "链接切换失败，已恢复旧安装"
}
rm -f "$FINAL_BAK" "$SHA_BAK"
say "安装完成：${LINK} → ${FINAL}（确保 $BIN_DIR 在 PATH 中）"
say "验证：ailoom version"
