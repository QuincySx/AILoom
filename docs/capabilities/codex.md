# Codex CLI 能力矩阵（AILoom）

核实日期：2026-09-09。来源：OpenAI Codex 官方文档 config-reference（learn.chatgpt.com/docs/config-file/config-reference，developers.openai.com/codex 重定向目标）与 agents.md 规范。
本机实测版本：codex-cli 0.153.4（`codex --version`）。

| resource_kind | project/user scope | 发现路径 | 格式 | 支持级别 | 来源 | 实际验证 |
|---|---|---|---|---|---|---|
| skill | project | `.codex/config.toml` `skills.config[].{path,enabled}`，path 指向含 SKILL.md 的目录 | TOML | supported（AILoom 以数组条目粒度管理，托管条目 path 前缀 `.ailoom/skills/`） | config-reference | 文件+配置落盘自动化验证；宿主加载：未验证 |
| rule | project | 仓库根 `AGENTS.md`（就近优先） | Markdown | supported（受管片段） | agents.md | 片段落盘自动化验证；宿主加载：未验证 |
| rule（条件 paths） | project | — | — | unsupported（片段无条件语义，显式报错） | AIL-010 | 自动化断言 Unsupported |
| agent（自定义子代理） | project | — | — | **unknown**（官方文档未确认项目级自定义 Agent）→ AILoom 显式 Unsupported，不写用户级配置充数 | 未能核实 | 自动化断言 Unsupported |
| mcp（stdio） | project | `.codex/config.toml` `mcp_servers.<id>.{command,args,env}`（项目级配置需项目处于受信状态，由用户自行 `projects.<path>.trust_level` 控制） | TOML | supported | config-reference | TOML 合并自动化验证；真实连接：未验证 |
| mcp（http） | project | 同上 `mcp_servers.<id>.url` | TOML | supported | config-reference | 渲染断言；真实连接：未验证 |
| 环境变量插值 | — | — | — | **unknown**（官方文档未确认 config 中 `${VAR}` 插值）→ 含秘密引用（`$ENV:NAME`）的服务拒绝写入 Codex 配置，绝不落明文 | 未能核实 | 自动化断言 Unsupported |
| 全局配置 | user | `~/.codex/config.toml` | TOML | AILoom 初版不写用户级配置 | config-reference | — |

## 明确不做

- 不把项目级能力降级写进 `~/.codex/config.toml`。
- 不在未核实插值能力时把秘密值以明文写进配置。
- 不把“项目受信”状态当作 AILoom 职责（trust_level 由用户/宿主管理）。
