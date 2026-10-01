#!/bin/zsh
# AIL-115 门禁可重复夹具：重置数据区与知识库的托管产物，保留夹具正文。
set -e
S=/tmp/ailoom-usable
T=$(grep '/?token=' "$S/evidence/console.log" | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\1/')
curl -s -X POST http://127.0.0.1:8646/api/shutdown -H "X-AILoom-Session: $T" -d '{}' >/dev/null || true
sleep 1.5
pgrep -f "target/debug/ailoom" | xargs kill 2>/dev/null || true
sleep 0.5
rm -rf "$S/data" "$S/xdg-state" && mkdir -p "$S/data"
rm -rf "$S/xdg-data/ailoom"
rm -rf "$S/知识库 甲/.claude" "$S/知识库 甲/AGENTS.override.md" "$S/知识库 乙/.claude" "$S/知识库 乙/AGENTS.override.md" "$S/项目 丙/.claude" "$S/项目 丙/web/.claude" "$S/项目 丙/AGENTS.override.md" "$S/项目 丙/.agents" "$S/知识库 甲/.agents"
HOME="$S/home" XDG_STATE_HOME="$S/xdg-state" XDG_DATA_HOME="$S/xdg-data" GIT_CONFIG_GLOBAL="$S/home/.gitconfig" GIT_CONFIG_NOSYSTEM=1 nohup <repo>/target/debug/ailoom --data-root "$S/data" console --port 8646 --no-open >> "$S/evidence/console.log" 2>&1 &
for i in $(seq 1 30); do curl -s -o /dev/null http://127.0.0.1:8646/api/state && break; sleep 0.5; done
echo "reset done"
