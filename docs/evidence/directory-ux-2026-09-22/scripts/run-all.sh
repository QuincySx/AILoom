#!/bin/zsh
# AIL-128 全量门禁：每张卡前重置沙盒（脚本各自建立场景基线），依 121→128 顺序执行。
set -e
export PATH="$HOME/.cargo/bin:$PATH:$HOME/.local/share/mise/installs/node/24/bin:$PATH"
cd /tmp/ailoom-dirux/evidence
for S in ail121-verify ail123-verify ail124-verify ail125-verify ail126-verify ail127-verify ail128-nongit ail128-focus; do
  echo "== reset for $S =="
  zsh /tmp/ailoom-dirux/reset.sh | tail -1
  echo "== $S =="
  if ! node $S.mjs > /tmp/ailoom-dirux/evidence/$S.log 2>&1; then
    echo "== $S FAILED (see $S.log) =="
    grep -E "FAIL|Error" $S.log | head -5
    exit 1
  fi
  grep -E "^==" $S.log | tail -1
done
echo "== ALL GATES DONE =="
