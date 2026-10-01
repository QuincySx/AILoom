# Grok、Pi、OpenCode、Cursor 适配

适配格式核实日期：2026-09-24。2026-09-26 已补充本机 Grok、OpenCode 检查，Cursor 操作受阻；各层级结果见[实测记录](../evidence/console/2026-09-26-extra-host-probes.md)。下表描述配置入口，不代表所有能力均已实际调用通过。

| 工具 | Skill | 子代理 | MCP | 项目补充说明 |
| --- | --- | --- | --- | --- |
| Grok（xai-org/grok-build） | `.grok/skills/<name>` | `.grok/agents/<name>.md` | `.grok/config.toml` 的 `mcp_servers` | `.grok/rules/ailoom-personal.md` |
| Pi | `.pi/skills/<name>` | `.pi/agents/<name>.md`（官方 subagent 扩展） | 原生无配置入口，需要扩展 | `.pi/APPEND_SYSTEM.md` 受管片段 |
| OpenCode | `.opencode/skills/<name>` | `.opencode/agents/<name>.md`，`mode: subagent` | `.opencode/opencode.json` 的 `mcp` | `AGENTS.md` 受管片段 |
| Cursor | `.cursor/skills/<name>` | `.cursor/agents/<name>.md` | `.cursor/mcp.json` 的 `mcpServers` | `.cursor/rules/ailoom-personal.mdc` |

## 适配约束

- Skill 仍由共享 Store 保存，项目目录仅挂载整目录；取消选择、更新与撤销复用已有托管流程。
- 子代理统一输入为 AgentSpec TOML。描述和正文共用；模型 ID、权限使用 `tool_extras.<工具>`。共享配置里的模型不会被擅自转成其他工具的模型；单工具 `targets` 可使用该工具的模型 ID。Cursor 的 `readonly`、OpenCode 的 `permission` 等原生字段可通过 extras 设置。不能转换的工具限制在计划中明确列出。
- MCP 使用现有 JSON pointer / TOML table 合并机制；保留其他服务和字段。`$ENV:NAME` 转成 Cursor `${env:NAME}`、OpenCode `{env:NAME}`、Grok `${NAME}`，不读取或落盘环境变量值。
- OpenCode 原生合并 `.opencode/opencode.json`，因此无需改写仓库根现有 `opencode.jsonc`。若 `.opencode/opencode.jsonc` 已存在，暂不写入可能被它覆盖的同目录 JSON；明确报告未适配该情况。
- Grok 对规则遵循 Git ignore，因此个人模式不会将生成的 Grok 规则加入 `info/exclude`。现有项目 ignore 仍可能阻止工具读取。技能扫描源码与 README 有差异：当前固定源码明确不按 Git ignore 过滤技能。
- Pi 的项目 APPEND_SYSTEM 文件优先于用户级同名文件，不是两个文件合并；项目原有文件中的非托管内容保留。
- 个人模式仍不改 Git 已跟踪文件。OpenCode 的 AGENTS.md 若已跟踪，补充说明会被保护守卫跳过；不会假装已生效。若只有本地 CLAUDE.md，首次创建 AGENTS.md 时保留该基线，避免遮蔽。
- Pi 子代理按用户指定的官方 subagent 示例适配，生成 `.pi/agents/*.md` 的 name、description、model、tools 与正文。界面注明扩展要求和 `agentScope: project/both`；官方示例默认只读用户级代理。AILoom 不修改扩展的信任确认，也不自动安装扩展。MCP 是独立扩展能力。
- 能力矩阵、可选工具、可选资源及部署状态同步更新；某工具不支持时不会把该资源整项标成“已应用”。

## 证据

研究问题：原生发现路径是什么、写入格式是什么、怎样保留原配置和加载优先级？

- Grok 固定 revision `f0e3be1100ef5252488e3be8bb0e91cf68d8c305`：
  [CLI README](https://github.com/xai-org/grok-build/blob/f0e3be1100ef5252488e3be8bb0e91cf68d8c305/crates/codegen/xai-grok-shell/README.md)、
  [指令发现](https://github.com/xai-org/grok-build/blob/f0e3be1100ef5252488e3be8bb0e91cf68d8c305/crates/codegen/xai-grok-agent/src/prompt/agents_md.rs)、
  [技能发现](https://github.com/xai-org/grok-build/blob/f0e3be1100ef5252488e3be8bb0e91cf68d8c305/crates/codegen/xai-grok-agent/src/prompt/skills.rs)、
  [MCP 配置与变量展开](https://github.com/xai-org/grok-build/blob/f0e3be1100ef5252488e3be8bb0e91cf68d8c305/crates/codegen/xai-grok-shell/src/util/config/mcp.rs)。
- Pi 固定 revision `7c696c00f34cf773c86d33093de8d5711994c5e4`（badlogic/pi-mono 已迁移至 earendil-works/pi）：
  [配置](https://github.com/earendil-works/pi/blob/7c696c00f34cf773c86d33093de8d5711994c5e4/packages/coding-agent/docs/configuration.md)、
  [资源加载](https://github.com/earendil-works/pi/blob/7c696c00f34cf773c86d33093de8d5711994c5e4/packages/coding-agent/src/core/resource-loader.ts)、
  [官方 subagent 扩展示例](https://github.com/earendil-works/pi/blob/7c696c00f34cf773c86d33093de8d5711994c5e4/packages/coding-agent/examples/extensions/subagent/README.md)。
- OpenCode：[技能](https://opencode.ai/docs/skills/)、[代理](https://opencode.ai/docs/agents/)、[MCP](https://opencode.ai/docs/mcp-servers/)、[规则](https://opencode.ai/docs/rules/)。
  [配置合并源码](https://github.com/anomalyco/opencode/blob/0f549842ee746e400b1f72516b0b2e292e267e2c/packages/opencode/src/config/config.ts) 固定 revision `0f549842ee746e400b1f72516b0b2e292e267e2c`，确认 `.opencode/opencode.json` 被读取，根目录 JSONC 可保留。
- Cursor：[技能](https://cursor.com/docs/skills)、[子代理](https://cursor.com/docs/subagents)、[MCP](https://cursor.com/docs/mcp)。

验证：适配器测试覆盖四工具路径、MCP 两种传输及变量引用、YAML 特殊字符、模型/权限不兼容、Pi 扩展缺失、条件规则、Claude 回退；CLI 集成测试执行首次与重复 sync，并验证用户 MCP 与根 JSONC 保留。没有向模型发送付费请求；未将生成文件测试标成各工具真实会话验证。

## 本轮验证结果

- `cargo test --lib`：106 项通过。
- `tests/host_adapters.rs`：11 项通过；`tests/adapters.rs` 包括跨四工具 sync 用例。
- `personal_instructions` 4 项、`profile` 6 项、`personal` 4 项通过。旧个人 sync 测试补齐显式 `--root`，实现保留强制目标要求。
- 前端组件 6 项通过，workspace 模块解析与 `git diff --check` 通过。
- Orca 内置浏览器：在测试项目 `知识库甲/notes` 勾选四工具，预览显示六个宿主入口，应用成功；逐个验证四个新工具的 Skill symlink 和 SKILL.md 可读。该测试验证 AILoom 部署流程，不等同于向四个工具发起模型调用。
