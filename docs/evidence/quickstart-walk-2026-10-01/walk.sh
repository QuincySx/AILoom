#!/bin/bash
# 按 docs/guide/QUICKSTART.md 的 CLI 步骤在隔离 HOME 中走一遍（个人模式 + 本地合集 + 团队源 + 卸载）
set -u
B=<repo>/target/debug/ailoom
W=$(mktemp -d "${TMPDIR:-/tmp}/ailoom-qs.XXXXXX")
export HOME=$W/home XDG_STATE_HOME=$W/home/state XDG_DATA_HOME=$W/home/data XDG_CONFIG_HOME=$W/home/config \
  CLAUDE_CONFIG_DIR=$W/home/claude CODEX_HOME=$W/home/codex AILOOM_REPORTING=off AILOOM_AUTO_SYNC=off
mkdir -p "$HOME"
step() { local desc=$1; shift; local out; out=$("$@" 2>&1); local c=$?; printf '%-48s exit=%s  %s\n' "$desc" "$c" "$(echo "$out" | tail -1 | cut -c1-90)"; return 0; }
# 个人模式
mkdir -p "$W/repo" "$W/my-skill" && git -C "$W/repo" init -q && echo demo > "$W/repo/README.md"
printf -- "---\nname: my-skill\ndescription: demo\n---\n正文\n" > "$W/my-skill/SKILL.md"
cd "$W/repo"
step "library import --execute"        $B library --action import --dir "$W/my-skill" --execute
step "personal effective"              $B personal --action effective
step "personal select --host claude"   $B personal --action select --host claude --state enable
step "personal select --resource"      $B personal --action select --resource personal/skill/personal/my-skill --state enable
step "personal plan"                   $B personal --action plan
step "personal sync"                   $B personal --action sync --root "$W/repo"
[ -e "$W/repo/.claude/skills/my-skill/SKILL.md" ] && echo "  ✓ .claude/skills/my-skill 已部署" || echo "  ✗ my-skill 未部署"
printf "个人偏好\n" > "$W/my.md"
step "personal instructions --file"    $B personal --action instructions --file "$W/my.md"
# 本地合集
mkdir -p "$W/col/skills/chosen" && printf -- "---\nname: chosen\ndescription: c\nnamespace: common\nshared: true\n---\nx\n" > "$W/col/skills/chosen/SKILL.md"
git -C "$W/col" init -q && git -C "$W/col" add . && git -C "$W/col" -c user.name=t -c user.email=t@e.invalid -c commit.gpgsign=false commit -qm init
PID=$($B --json collection --action preview --name my-tools --url "$W/col" | python3 -c "import json,sys;print(json.load(sys.stdin)['result']['preview_id'])")
step "collection apply"                $B collection --action apply --preview-id "$PID"
CID=$($B --json collection --action list | python3 -c "import json,sys;print(json.load(sys.stdin)['result']['sources'][0]['id'])")
step "personal select collection skill" $B personal --action select --resource "$CID/skill/common/chosen" --state enable
step "personal sync (collection)"      $B personal --action sync --root "$W/repo"
[ -e "$W/repo/.claude/skills/chosen" ] && echo "  ✓ 合集 Skill 已部署" || echo "  ✗ 合集 Skill 未部署"
step "collection check"                $B collection --action check
# 团队源
mkdir -p "$W/team/resources/skills/common-greet" "$W/biz"
cat > "$W/team/ailoom.toml" <<T
schema_version = 1
team_id = "example-team"
[projects.a]
name = "Project A"
[roles.dev]
description = "Developer"
[namespaces]
known = ["common"]
shared = ["common"]
T
printf -- "---\nname: common-greet\ndescription: g\nnamespace: common\nshared: true\n---\nhi\n" > "$W/team/resources/skills/common-greet/SKILL.md"
git -C "$W/team" init -q && git -C "$W/team" add . && git -C "$W/team" -c user.name=t -c user.email=t@e.invalid -c commit.gpgsign=false commit -qm init
git -C "$W/biz" init -q && echo b > "$W/biz/README.md"
cd "$W/biz"
step "init --url (local git) --project a" $B init --url "$W/team" --project a --role dev
step "plan"                            $B plan
step "sync"                            $B sync
step "recall --query"                  $B recall --query greet
step "doctor"                          $B doctor
step "doctor --strict"                 $B doctor --strict
step "uninstall (preview)"             $B uninstall
step "uninstall --execute"             $B uninstall --execute
[ -e "$W/biz/.claude/skills/common-greet" ] && echo "  ✗ 卸载后仍存在" || echo "  ✓ 卸载移除了托管 Skill"
echo "work dir: $W"
