#!/bin/zsh
# AIL-121~128 隔离夹具搭建：两个非 Git 知识库、Git 双工作树、嵌套目录、托管+未托管 Skill。
# 仅在 /tmp/ailoom-dirux 内操作；不触碰用户真实知识库。可重复执行（强制重建）。
set -e
ROOT=/tmp/ailoom-dirux
REPOS=$ROOT/repos
export GIT_CONFIG_GLOBAL=$ROOT/gitconfig
export GIT_CONFIG_NOSYSTEM=1
git() { command git -c commit.gpgsign=false -c user.name=Fixture -c user.email=fixture@local "$@"; }

rm -rf "$REPOS"
mkdir -p "$REPOS" "$ROOT/evidence"
cat > "$ROOT/gitconfig" <<'EOF'
[init]
	defaultBranch = main
[commit]
	gpgsign = false
[safe]
	directory = *
EOF

mk_skill() { # dir name desc
  mkdir -p "$1"
  cat > "$1/SKILL.md" <<EOF
---
name: $2
description: $3
shared: true
namespace: common
---

# $2

沙盒夹具 Skill：$3。仅用于 AILoom 目录优先轮次验收。
EOF
}

# ── 合集来源仓库（Git，含两个 Skill）──────────────────────────────
SRC=$REPOS/src-team-lib
mk_skill "$SRC/resources/skills/meeting-notes" "meeting-notes" "会议记录整理与待办提取"
mk_skill "$SRC/resources/skills/doc-search" "doc-search" "文档检索入口与索引说明"
cat > "$SRC/ailoom.toml" <<'EOF'
schema_version = 1
team_id = "fixture"

[namespaces]
known = ["common"]
EOF
git -C "$SRC" init -q && git -C "$SRC" add -A && git -C "$SRC" commit -qm "fixture: team source"

# ── Git 项目「项目丙」：main 工作树 + 链接工作树，含 web/docs 嵌套 ──
P1=$REPOS/proj-bing
mkdir -p "$P1/web/css" "$P1/docs/guide"
echo "# 项目丙" > "$P1/README.md"
echo "body{}" > "$P1/web/css/site.css"
echo "# 指南" > "$P1/docs/guide/intro.md"
git -C "$P1" init -q && git -C "$P1" add -A && git -C "$P1" commit -qm "fixture: proj-bing main"
# 未托管 Skill：直接放在 web/.claude/skills（宿主自有，AILoom 不托管）
mkdir -p "$P1/web/.claude/skills/old-notes"
cat > "$P1/web/.claude/skills/old-notes/SKILL.md" <<'EOF'
---
name: old-notes
description: 项目里自带的旧笔记 Skill（未托管）
---
手写目录，未经 AILoom 托管。
EOF
git -C "$P1" add -A && git -C "$P1" commit -qm "fixture: local unmanaged skill"
# 链接工作树（分支 feature）
git -C "$P1" worktree add -b feature "$REPOS/proj-bing-wt" -q
mkdir -p "$REPOS/proj-bing-wt/web"
echo "h1{}" > "$REPOS/proj-bing-wt/web/style.css"

# ── 非 Git 知识库甲（含 web/docs 嵌套 + 顶层笔记目录）────────────
KB1=$REPOS/kb-jia
mkdir -p "$KB1/web/assets" "$KB1/docs/meetings" "$KB1/notes"
echo "甲库首页" > "$KB1/index.md"
echo "body{}" > "$KB1/web/assets/main.css"
echo "# 周会" > "$KB1/docs/meetings/week1.md"
echo "# 随记" > "$KB1/notes/scratch.md"

# ── 非 Git 知识库乙（简单根目录，用于跨库隔离断言）───────────────
KB2=$REPOS/kb-yi
mkdir -p "$KB2"
echo "乙库首页" > "$KB2/index.md"

echo "fixtures ready at $REPOS"

# ── 本地个人副本 Skill 源目录（AIL-126：导入个人库用）────────────
mkdir -p "$REPOS/local-skill/notes-helper"
cat > "$REPOS/local-skill/notes-helper/SKILL.md" <<'EOF2'
---
name: notes-helper
description: 本地笔记整理助手（个人副本）
namespace: personal
shared: true
---
本地个人副本 Skill，用于 AIL-126 验收。
EOF2
