#!/usr/bin/env python3
"""文档门禁：断链、卡片状态一致性、BACKLOG 总表生成。

用法：
  python3 scripts/docs_check.py            # 检查（CI 用；有问题退出 1）
  python3 scripts/docs_check.py --write    # 从 docs/cards.json 重新生成 BACKLOG 总表与统计行

规则：
- cards.json 是卡片状态的唯一来源；卡片 md 的「- 状态：」必须与之相同。
- BACKLOG.md 中 `<!-- cards:begin -->` 与 `<!-- cards:end -->` 之间由脚本生成，禁止手改。
- 断链检查覆盖 README.md、CONTEXT.md 与 docs/ 下的 md；docs/archive/ 与 docs/evidence/
  是历史快照，只统计不判失败。
"""
import collections
import json
import os
import re
import sys
import urllib.parse

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(ROOT, "docs")
HISTORICAL = ("docs/archive/", "docs/evidence/")
BEGIN, END = "<!-- cards:begin -->", "<!-- cards:end -->"
STATUS_ORDER = ["Backlog", "In progress", "Blocked", "Done", "Superseded"]


def rel(path):
    return os.path.relpath(path, ROOT)


def md_files():
    files = [os.path.join(ROOT, "README.md"), os.path.join(ROOT, "CONTEXT.md")]
    for base, _, names in os.walk(DOCS):
        files += [os.path.join(base, n) for n in names if n.endswith(".md")]
    return sorted(f for f in files if os.path.exists(f))


LINK = re.compile(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")


def check_links():
    broken, historical = [], 0
    for f in md_files():
        text = open(f, encoding="utf-8").read()
        text = re.sub(r"```.*?```", lambda m: "\n" * m.group(0).count("\n"), text, flags=re.S)
        for ln, line in enumerate(text.split("\n"), 1):
            for m in LINK.finditer(line):
                target = m.group(1)
                if re.match(r"^(https?:|mailto:|data:|#)", target):
                    continue
                path = urllib.parse.unquote(target.partition("#")[0])
                resolved = os.path.normpath(os.path.join(os.path.dirname(f), path))
                if os.path.exists(resolved):
                    continue
                if rel(f).startswith(HISTORICAL):
                    historical += 1
                else:
                    broken.append(f"{rel(f)}:{ln}: {target}")
    return broken, historical


def load_cards():
    return json.load(open(os.path.join(DOCS, "cards.json"), encoding="utf-8"))


def check_cards(cards):
    problems = []
    ids = [c["id"] for c in cards]
    for dup in [i for i, n in collections.Counter(ids).items() if n > 1]:
        problems.append(f"cards.json 重复 id：{dup}")
    for c in cards:
        path = os.path.join(DOCS, c["file"])
        if not os.path.exists(path):
            problems.append(f"{c['id']}：缺少卡片文件 {c['file']}")
            continue
        text = open(path, encoding="utf-8").read()
        m = re.search(r"^- 状态：\s*(.+?)\s*$", text, re.M)
        status = m.group(1) if m else None
        if status != c["status"]:
            problems.append(f"{c['id']}：cards.json={c['status']}，卡头={status}")
        head = re.match(r"# (AIL-\d+) · (.*)", text)
        if not head or head.group(1) != c["id"]:
            problems.append(f"{c['id']}：卡片标题行与 id 不一致")
    on_disk = {n[:-3] for n in os.listdir(os.path.join(DOCS, "cards")) if n.endswith(".md")}
    for extra in sorted(on_disk - set(ids)):
        problems.append(f"{extra}：卡片文件未登记在 cards.json")
    return problems


def done_with_unchecked(cards):
    out = []
    for c in cards:
        if c["status"] != "Done":
            continue
        path = os.path.join(DOCS, c["file"])
        if os.path.exists(path) and re.search(r"^\s*- \[ \] ", open(path, encoding="utf-8").read(), re.M):
            out.append(c["id"])
    return out


def stats_line(cards):
    counts = collections.Counter(c["status"] for c in cards)
    parts = [f"{counts[s]} {s}" for s in STATUS_ORDER if counts[s]]
    return f"**{len(cards)} 张主卡：{'、'.join(parts)}。**（由 `scripts/docs_check.py --write` 从 cards.json 生成）"


def table(cards):
    rows = ["| 卡片 | 任务 | 里程碑 | 优先级 | 规模 | 状态 | 前置 |", "|---|---|---|---|---|---|---|"]
    for c in cards:
        deps = ", ".join(c.get("depends_on") or []) or "—"
        rows.append(
            f"| [{c['id']}]({c['file']}) | {c['title']} | {c.get('milestone', '—')} | "
            f"{c.get('priority', '—')} | {c.get('size', '—')} | {c['status']} | {deps} |"
        )
    return "\n".join(rows)


def render_backlog_block(cards):
    return f"{BEGIN}\n{stats_line(cards)}\n\n{table(cards)}\n{END}"


def backlog_problems(cards, write):
    path = os.path.join(DOCS, "BACKLOG.md")
    text = open(path, encoding="utf-8").read()
    if BEGIN not in text or END not in text:
        return [f"BACKLOG.md 缺少 {BEGIN} / {END} 生成区"]
    start, end = text.index(BEGIN), text.index(END) + len(END)
    block = render_backlog_block(cards)
    if text[start:end] == block:
        return []
    if write:
        open(path, "w", encoding="utf-8").write(text[:start] + block + text[end:])
        return []
    return ["BACKLOG.md 生成区与 cards.json 不一致：运行 python3 scripts/docs_check.py --write"]


def main():
    write = "--write" in sys.argv
    cards = load_cards()
    problems = check_cards(cards) + backlog_problems(cards, write)
    broken, historical = check_links()
    problems += [f"断链 {b}" for b in broken]
    for p in problems:
        print(p)
    gaps = done_with_unchecked(cards)
    if gaps:
        # 只提示不判失败：这些卡经用户确认保留 Done，缺口在卡内「验收项核对」中列明，后续补证据。
        print(f"提示：{len(gaps)} 张 Done 卡仍有未勾验收项：{', '.join(gaps)}")
    print(f"cards={len(cards)} 问题={len(problems)} 历史快照内断链（不判失败）={historical}")
    sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
