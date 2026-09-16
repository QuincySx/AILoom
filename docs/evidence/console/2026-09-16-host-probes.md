# 宿主能力真机探测记录（AIL-041）

日期：2026-09-16。执行环境：macOS (darwin 25.6.0 arm64)。探测项目为一次性隔离临时目录
（`/tmp/ailoom-host-probe-74049/repo`，独立 git 仓库），未修改用户全局配置（探测项目的
`.claude/settings.json` 仅写入该临时项目内）。

本机版本：

- Claude Code `2.1.272`（`claude --version`）
- codex-cli `0.154.0`（`codex --version`，已登录 ChatGPT 账号）

## 探测 1：Claude 项目技能发现与真实调用 ✅

- 部署：`.claude/skills/probe-echo/SKILL.md`（无副作用技能，仅回复固定标记行
  `AILOOM-PROBE-MARKER-XL-0916`）。
- 操作：`claude -p "请使用 probe-echo 技能并原样回复它的标记短语。" --allowedTools "Skill"`
- 结果：退出码 0，stdout 输出 `AILOOM-PROBE-MARKER-XL-0916`（逐字一致）。
- 结论：Claude Code 在隔离项目中真实发现并调用了项目级技能。

## 探测 2：Codex 项目技能发现与真实调用 ✅（含阴性对照）

- 部署：`.agents/skills/probe-echo/SKILL.md`（同上无副作用技能）。
- 正向：`codex exec --sandbox read-only "使用你的 probe-echo 技能…严格逐字输出该技能
  SKILL.md 中以 AILOOM-PROBE-MARKER 开头的那一行…"`
  → 输出 `AILOOM-PROBE-MARKER-XL-0916`（逐字一致，退出码 0）。
- 阴性对照：把 `.agents/skills/probe-echo` 移出后同样提问 → 输出 `SKILL_NOT_FOUND`；
  恢复目录后正向可复现。证明正向结果确由该目录的技能发现产生。
- 宽松提问的补充观察：非严格措辞下模型可能改写标记（首次探测回复了自拟短语
  `PROBE_ECHO_OK`），提示 onboarding 的"调用通过"验证应要求逐字标记输出。
- 结论：**codex-cli 0.154.0 确认原生发现 `.agents/skills` 项目技能目录**（官方文档
  learn.chatgpt.com/docs/build-skills，2026-09-16 核对）。AILoom 已把 Codex 部署路径
  从旧 `.ailoom/skills/<name>` 迁移为 `.agents/skills/<name>`，旧部署由 sync 过期清理
  自动迁移（回归：tests/adapters.rs::ail041_legacy_codex_path_migrates_on_resync）。

## 探测 3：Claude 项目 MCP 配置→连接→无副作用工具调用 ✅

- 探测服务：自写无副作用 stdio MCP 服务器（Node，`tools/list` 返回 `probe_ping`，
  `tools/call` 固定返回 `PONG-AILOOM-MCP-0916`，不读写文件不执行命令）。
- 配置：项目 `.mcp.json` `mcpServers.probe.command=node`。
- 发现：`claude mcp list` 显示 `probe: node … - ⏸ Pending approval`——项目级 MCP
  需用户批准（与官方文档一致）。
- 连接与调用：探测项目内 `.claude/settings.json` 开启
  `enableAllProjectMcpServers` 后，
  `claude -p "调用 mcp 服务器 probe 的 probe_ping 工具…" --allowedTools "mcp__probe__probe_ping"`
  → 输出 `PONG-AILOOM-MCP-0916`（逐字一致）。
- 结论：Claude 项目 MCP 链路（配置→连接→无副作用工具调用）全部真机通过。

## 探测 4：Codex 项目 MCP 加载 ❌（宿主不加载，非 AILoom 配置缺陷）

- 配置：项目 `.codex/config.toml` `[mcp_servers.probe]`（与官方文档 learn.chatgpt.com/docs/extend/mcp
  的语法一致：command/args）。
- 操作 1：`codex mcp list` → 仅列出用户级/插件服务器（codex_app、computer-use、
  cua_repl、node_repl），无 `probe`。
- 操作 2：`codex exec --sandbox read-only "通过 MCP 服务器 probe 调用 probe_ping…"`
  → 回答 `MCP_UNAVAILABLE`。
- 操作 3：追加 `-c 'projects."<path>".trust_level="trusted"'` 重试 → 仍
  `MCP_UNAVAILABLE`。
- 结论：codex-cli 0.154.0 的 `codex exec` 会话**不加载项目级 `.codex/config.toml`
  的 mcp_servers**（与 2026-09-13 对 0.153.4 的实测一致，见
  [2026-09-13-host-acceptance.md](../2026-09-13-host-acceptance.md)）。官方文档描述的
  "trusted projects only" 项目级 MCP 在本机版本/模式下未生效。
- 处置：AILoom 继续按官方语法写项目级配置（保留宿主修复后的自动生效），能力矩阵该行
  标注"配置写入 supported / 宿主加载 0.154.0 未生效"；不写用户级全局配置充数。AILoom
  状态展示必须区分"已部署"与"宿主已加载"（AIL-050 的验证状态机按此实现）。

## 与能力矩阵/代码的同步

- 代码矩阵：`src/adapters/capability.rs`（查询接口，含本记录引用）。
- 矩阵文档：docs/capabilities/codex.md、docs/capabilities/claude-code.md（2026-09-16 更新）。
- 回归测试：tests/adapters.rs::ail041_codex_native_skill_path_and_capability_query、
  tests/adapters.rs::ail041_legacy_codex_path_migrates_on_resync。
