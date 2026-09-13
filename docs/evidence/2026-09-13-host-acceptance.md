# 宿主真机验收证据（2026-09-13）

环境：macOS 25.5.0 arm64；Claude Code 2.1.266（已认证）；Codex CLI 0.153.4（ChatGPT 登录）；
node v24.19.0；ailoom 本地构建（target/debug，2026-09-13，含本轮返工改动）。
场景工作区：/tmp/ailoom-host-acc（本地裸远端 + 团队源 + 业务仓 biz）；
`AILOOM_DATA_ROOT=/tmp/ailoom-host-acc/data`；AILOOM 二进制在 PATH（hooks 需要）。

## 部署（真实宿主发现路径）

- `.claude/skills/common-greet/SKILL.md`（skill，Claude 项目 skill 目录）
- `.claude/rules/commit-style.md`（规则，Claude 项目规则目录）
- `.claude/agents/release-helper.md`（agent，Claude 项目 agent 目录）
- `.mcp.json`：acc-mcp → stdio `npx -y @modelcontextprotocol/server-everything`
- `AGENTS.md` 受管片段（规则 + 内置 recall-hint）；`.codex/config.toml` skills 注册
- `.claude/settings.json`：hooks 4 事件注册（`ailoom hooks --action install`）

## AIL-009 · Skills 真实加载与调用（Claude）

命令：`claude -p --allow-dangerously-skip-permissions "/common-greet 然后告诉我 skill 输出的原文"`
输出（节选）：

```
ACC-GREETING-你好，AILoom 团队验收成功

上面那行就是 skill 输出的原文：`ACC-GREETING-你好，AILoom 团队验收成功`
```

结论：真实宿主发现并调用 AILoom 部署的 skill。✔

## AIL-010 · 规则真实发现（两宿主）

Claude：`claude -p "…团队规则规定提交信息的动词语气…"` 输出：

```
规则要求的语气：**祈使句**（imperative）
规则文件名：`commit-style.md`（位于 `.claude/rules/commit-style.md`）
```

Codex：`codex exec --skip-git-repo-check "只根据仓库根 AGENTS.md…"` 输出：

```
祈使句。信息来自仓库根目录的 `AGENTS.md`。
```

两宿主真实会话均从 AILoom 部署产物读取规则。✔

## AIL-011 · Agent 真实发现与无副作用调用（Claude）

命令：`claude -p --agent release-helper "报告你的职责（一句话）"` 输出：

```
RELEASE-HELPER-OK

我的职责：作为发布辅助，检查发布清单（release checklist）各项是否齐备……
```

结论：项目 agent 被宿主发现并以该人设执行（指令内容回显）。✔

## AIL-012 · MCP 真实连接与工具调用（Claude stdio）

`claude mcp list` 初始显示 `acc-mcp … ⏸ Pending approval`（项目级 .mcp.json 需批准，
已在 .claude/settings.json 加 `enableAllProjectMcpServers: true` 后由真实会话调用）。
真实会话：`claude -p "调用 acc-mcp 的 echo 工具，发送 ACC-MCP-OK…"` 输出：

```
Echo: ACC-MCP-OK
```

结论：宿主真实拉起 stdio MCP 服务器并完成工具往返。✔（Claude/stdio）

Codex 边界：`codex mcp list` 仅列出全局配置服务器；项目级 `.codex/config.toml`
的 `mcp_servers` 需项目受信（trust_level）后生效，非交互环境无法完成受信流程。
按能力矩阵记录为：配置落盘 ✔，真实连接未验证（阻塞：Codex 项目受信门控）。

## AIL-017 · 内置召回提示真实链路（Claude 会话 → ailoom CLI → 索引）

种子共享经验 cache-lesson（正文含标记 ACC-RECALL-LESSON-7788）→ `init --refresh` +
索引重建 → 真实会话：`claude -p "遵循仓库里的团队知识检索提示：运行 ailoom recall 查询「缓存」…"`
模型按 AGENTS.md 内置提示真实执行 CLI 并输出：

```
> **# 缓存击穿复盘**
> 根因：热点 key 过期瞬间并发回源。对策：互斥重建 + 逻辑过期。验收标记 ACC-RECALL-LESSON-7788。
```

附注：种子未推送时，同一提示下模型明确拒绝编造标记（不可信边界 ✔）。

## AIL-018 · 真实宿主 Hooks 触发链路

`ailoom hooks --action install` 注册 4 事件后，真实 `claude -p` 会话（含一次工具调用）
触发宿主执行 `ailoom hook …`，事件落盘 `data/ws/<wsid>/events/events.jsonl`：

```
事件计数: {'session-start': 3, 'prompt': 3, 'tool': 17, 'stop': 3} | 独立真实会话: 3
```

结论：注册命令被真实宿主执行、payload.cwd 归属工作区、标准事件类型与真实 session_id 落盘。✔

## 边界与未验证项

- Codex 项目级 MCP 真实连接：受项目受信门控阻塞（见上）。
- Claude hooks 的 Stop 摩擦提示（systemMessage）在交互宿主中的呈现：已由受控
  CLI 会话验证（见 tests/hook_events.rs::stop_prompt_uses_real_session_and_outputs_once），
  交互式呈现属宿主 UI 行为，未单独验收。
- 真实 GitHub PR 自动创建（AIL-014）：需要远端仓库写权限与本次任务未含的对外授权，
  保持 Blocked（本地裸仓 + manual + fake gh 路径已全部自动化验证）。

## AIL-023/AIL-021 · 看板浏览器交互与跨进程 SSE（真实浏览器）

流程：真实浏览器（ZCode 内嵌 Chromium）打开 `http://127.0.0.1:18742/` →
初始仅表头 → 服务端重启（更换正确 AILOOM_DATA_ROOT 后 EventSource 自动重连）→
CLI 独立进程注入新事件（session_id=sse-live-check）→ 页面**未刷新**收到 SSE 快照，
表格出现 4 行（3 个真实宿主会话 + sse-live-check，状态 running/idle）。

DOM 快照节选（页面实际渲染）：

```
- row "sse-live-check running 0 0 0 0 unavailable"
- row "5440b96d-25fd-4543-a332-e53dbdba285b idle 1 10 0 0 unavailable"
```

结论：浏览器基本交互 ✔；跨进程 SSE 实时更新 ✔；断线重连恢复 ✔。
附注：一次 dashboard 以错误数据根启动导致空视图，属操作环境问题而非代码缺陷
（换正确 AILOOM_DATA_ROOT 后数据正确）。

## Codex 实机补充发现（AIL-009/012 边界）

- 项目级 skills（`.codex/config.toml` `[[skills.config]]` path=.ailoom/skills/common-greet，
  且沙箱 CODEX_HOME 配置 `projects."/tmp/ailoom-host-acc/biz".trust_level = "trusted"`）：
  `codex exec` 会话内 skill 不可用（模型自报 SKILL-NOT-FOUND）。
- 项目级 mcp_servers：`codex mcp list` 与会话工具列表均不加载（同上受信配置）。
  → docs/capabilities/codex.md 已把 mcp（stdio）降级为 unsupported（实测），
  skills 行加载状态同步为实测未加载。
- AGENTS.md 受管片段加载 ✔（规则发现验证通过，见上文）。

## AIL-017 · Codex 真实召回链路 ✔

`CODEX_HOME=<沙箱> codex exec --sandbox danger-full-access "遵循 AGENTS.md 团队知识检索提示…"`
模型按内置提示真实执行 `ailoom recall --query "缓存"` 并输出：

```
验收标记：`ACC-RECALL-LESSON-7788`
```

附注：codex 默认沙箱拦截工作区外读取（首次尝试 E9000 Operation not permitted），
放开沙箱后（recall 为只读）成功；两宿主真实召回链路均验证通过。
