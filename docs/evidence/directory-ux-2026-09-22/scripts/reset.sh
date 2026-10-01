#!/bin/zsh
# 完全重置沙盒：重建夹具 + 数据区 + 重启 console + 注册项目/合集。
set -e
export PATH="$HOME/.cargo/bin:$PATH:$HOME/.local/share/mise/installs/node/24/bin:$PATH"
pkill -f "ailoom-dirux" 2>/dev/null || true
sleep 0.5
rm -rf /tmp/ailoom-dirux/data
zsh /tmp/ailoom-dirux/setup.sh >/dev/null
nohup <repo>/target/debug/ailoom --data-root /tmp/ailoom-dirux/data console --port 8648 --no-open > /tmp/ailoom-dirux/console.log 2>&1 &
sleep 1.8
curl -s -m 3 http://127.0.0.1:8648/api/server-info >/dev/null || { echo "console failed to start"; exit 1; }
zsh /tmp/ailoom-dirux/register.sh 8648 | tail -1
# 本地个人副本资源（AIL-126 夹具）
curl -s -H "X-AILoom-Session: $(grep -o 'token=[a-f0-9-]*' /tmp/ailoom-dirux/console.log | tail -1 | cut -d= -f2)" -X POST http://127.0.0.1:8648/api/fs/approve -H "Content-Type: application/json" -d '{"path":"/tmp/ailoom-dirux/repos/local-skill"}' >/dev/null
curl -s -H "X-AILoom-Session: $(grep -o 'token=[a-f0-9-]*' /tmp/ailoom-dirux/console.log | tail -1 | cut -d= -f2)" -H "Content-Type: application/json" -X POST http://127.0.0.1:8648/api/library/import -d '{"dir":"/tmp/ailoom-dirux/repos/local-skill/notes-helper","name":"notes-helper","execute":true}' >/dev/null
echo "reset done"
