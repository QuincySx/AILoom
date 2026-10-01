#!/bin/zsh
# 沙盒注册：批准根 + 三个项目 + 合集来源导入（幂等：重复执行跳过已存在）。
PORT=${1:-8648}
TOKEN=$(grep -o "token=[a-f0-9-]*" /tmp/ailoom-dirux/console.log | tail -1 | cut -d= -f2)
API="http://127.0.0.1:$PORT/api"
H=(-H "X-AILoom-Session: $TOKEN" -H "Content-Type: application/json")
approve() { curl -s -m 5 -X POST "$API/fs/approve" "${H[@]}" -d "{\"path\":\"$1\"}" >/dev/null; }

approve /tmp/ailoom-dirux/repos/src-team-lib
approve /tmp/ailoom-dirux/repos/proj-bing
approve /tmp/ailoom-dirux/repos/kb-jia
approve /tmp/ailoom-dirux/repos/kb-yi

# 合集来源（已存在则 state=existing）
PV=$(curl -s -m 20 -X POST "$API/collections/preview" "${H[@]}" -d '{"name":"team-lib","url":"/tmp/ailoom-dirux/repos/src-team-lib"}')
STATE=$(echo "$PV" | /usr/bin/python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("state",""))')
if [ "$STATE" != "existing" ]; then
  PID=$(echo "$PV" | /usr/bin/python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["preview_id"])')
  curl -s -m 60 -X POST "$API/collections/apply" "${H[@]}" -d "{\"preview_id\":\"$PID\"}" | head -c 300; echo
else
  echo "collection source already imported"
fi

# 项目登记（项目资料 API 会创建 registry；repo_discover 先行）
for P in proj-bing kb-jia kb-yi; do
  approve /tmp/ailoom-dirux/repos/$P
  RID=$(curl -s -m 10 -X POST "$API/repo/discover" "${H[@]}" -d "{\"path\":\"/tmp/ailoom-dirux/repos/$P\"}" | /usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["repo_id"])')
  N=$( [ $P = proj-bing ] && echo "项目丙" || ([ $P = kb-jia ] && echo "知识库甲" || echo "知识库乙") )
  curl -s -m 10 -X POST "$API/projects/metadata" "${H[@]}" -d "{\"repo_id\":\"$RID\",\"name\":\"$N\",\"category\":\"沙盒\"}" >/dev/null
  echo "$P -> $RID ($N)"
done
curl -s "$API/state" "${H[@]}" | /usr/bin/python3 -c 'import json,sys; d=json.load(sys.stdin); [print(r["repo_id"], r["project"]["name"] if r.get("project") else "", list((r.get("worktrees") or {}).keys())) for r in d["repos"]]'
