#!/bin/bash
# 浏览器验收：在隔离目录中启动控制台与调试 Chrome，依次运行 tests/ui_browser.mjs 的全部模式。
# 用法：scripts/ui-browser.sh [输出目录]    （默认 target/ui-browser）
# 需要：已构建的 target/debug/ailoom、Node 22+、本机 Chrome（CHROME 可覆盖路径）。
# 不触碰真实 HOME / ~/.claude / ~/.codex；结束时只停止本脚本启动的进程。
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/target/ui-browser}
CHROME=${CHROME:-"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"}
CONSOLE_PORT=${CONSOLE_PORT:-47971}
CDP_PORT=${CDP_PORT:-9251}
WORK=$(mktemp -d "${TMPDIR:-/tmp}/ailoom-ui.XXXXXX")
mkdir -p "$OUT"

REAL_HOME=$HOME
export HOME="$WORK/home" XDG_STATE_HOME="$WORK/home/state" XDG_DATA_HOME="$WORK/home/data" \
  XDG_CONFIG_HOME="$WORK/home/config" CLAUDE_CONFIG_DIR="$WORK/home/claude" CODEX_HOME="$WORK/home/codex" \
  PI_CODING_AGENT_DIR="$WORK/home/pi" AILOOM_REPORTING=off AILOOM_AUTO_SYNC=off
mkdir -p "$HOME"

"$ROOT/target/debug/ailoom" console --port "$CONSOLE_PORT" --no-open >"$OUT/console.log" 2>&1 &
CONSOLE_PID=$!
start_chrome() {
  # 关闭后台网络、组件更新与同步，避免 Chrome 自身的后台任务（含自更新）干扰无头实例。
  # Chrome 用真实 HOME 启动并使用模拟钥匙串：HOME 指向临时目录时，macOS 上的网络服务会卡在钥匙串初始化，
  # 表现为连接已建立却不发出请求（Page.navigate 无响应）。隔离数据只作用于控制台进程。
  HOME="$REAL_HOME" "$CHROME" --headless=new --remote-debugging-port="$CDP_PORT" --user-data-dir="$WORK/chrome-$1" \
    --no-first-run --no-default-browser-check --disable-background-networking --disable-component-update \
    --disable-sync --disable-default-apps --metrics-recording-only --use-mock-keychain --password-store=basic \
    about:blank >>"$OUT/chrome.log" 2>&1 &
  CHROME_PID=$!
  for _ in $(seq 1 60); do
    curl -sf "http://127.0.0.1:$CDP_PORT/json/version" >/dev/null && break
    sleep 0.5
  done
}
start_chrome 1
cleanup() { kill "$CONSOLE_PID" "$CHROME_PID" 2>/dev/null; wait 2>/dev/null; }
trap cleanup EXIT

for _ in $(seq 1 60); do
  curl -sf "http://127.0.0.1:$CONSOLE_PORT/" >/dev/null && break
  sleep 0.5
done

mkdir -p "$WORK/project-a" "$WORK/project-b" "$WORK/project-c" "$WORK/skills/browser-skill"
git -C "$WORK/project-a" init -q && git -C "$WORK/project-b" init -q && git -C "$WORK/project-c" init -q
printf -- "---\nname: browser-skill\ndescription: 浏览器验收用\n---\n正文\n" >"$WORK/skills/browser-skill/SKILL.md"

failed=0
attempt() {
  local mode=$1 log=$2; shift 2
  (cd "$ROOT" && CDP_PORT=$CDP_PORT timeout 150 node tests/ui_browser.mjs "http://127.0.0.1:$CONSOLE_PORT/" "$OUT/$mode" "$mode" "$@") >"$log" 2>&1 \
    && tail -1 "$log" | grep -q '"errors":\[\]'
}
restarts=1
run() {
  local mode=$1; shift
  local log="$OUT/$mode.log"
  if attempt "$mode" "$log" "$@"; then
    echo "${mode}: PASS"
  elif grep -q "CDP Page.navigate 30 秒无响应" "$log"; then
    # 无头 Chrome 偶尔整体卡住（不发出任何请求）：只针对这类失败重启 Chrome 并重试一次，结果如实标注。
    kill "$CHROME_PID" 2>/dev/null; wait "$CHROME_PID" 2>/dev/null
    restarts=$((restarts + 1)); start_chrome "$restarts"
    if attempt "$mode" "$log" "$@"; then
      echo "${mode}: PASS（Chrome 卡住，重启后重试通过）"
    else
      echo "${mode}: FAIL（重启 Chrome 后仍失败，见 ${log}）"; failed=1
    fi
  else
    echo "${mode}: FAIL（见 ${log}）"; failed=1
  fi
}
run current
run grouped
run cc-switch
run projects "$WORK/project-a"
run onboarding
run instructions "$WORK/project-c"
run design "$WORK/project-b" "$WORK/skills/browser-skill"
exit $failed
