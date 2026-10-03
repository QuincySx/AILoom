# Codex CLI 能力矩阵（AILoom）

核实日期：2026-09-16（AIL-041 复核；历史：2026-09-09 初核、2026-09-13/09-16 mcp 实机）。来源：learn.chatgpt.com/docs/build-skills（skills）、learn.chatgpt.com/docs/extend/mcp（MCP）、config-reference 与 agents.md 规范。
本机实测版本：codex-cli 0.154.0（`codex --version`）。真机探测证据：[console/2026-09-16-host-probes.md](../evidence/console/2026-09-16-host-probes.md)。

| resource_kind | project/user scope | 发现路径 | 格式 | 支持级别 | 来源 | 实际验证 |
|---|---|---|---|---|---|---|
| skill | project | **`.agents/skills/<name>`（原生发现：CWD 向上扫描至仓库根，支持 symlink）**；`.codex/config.toml` `skills.config[].{path,enabled}` 为显式条目（path 指向 SKILL.md） | 目录 symlink + TOML | supported（AIL-041 起部署到原生目录；旧 `.ailoom/skills/` 由 sync 过期清理自动迁移） | build-skills（2026-09-16） | **真机 0.154.0 发现+调用通过（正向逐字标记 + 阴性对照 SKILL_NOT_FOUND）**；旧路径迁移回归 tests/adapters.rs::ail041_legacy_* |
| skill | user | `~/.agents/skills/<name>`（`.agents` 规范的用户级目录，AILoom 全局部署与其他遵循该规范的 agent 共用） | 目录 symlink | 部署 supported（AIL-152 全局 Skill，不写 `~/.codex/config.toml`）；宿主加载待核实 | build-skills（用户级目录；2026-10-02 未重新核对） | 文件落盘自动化验证（tests/global_skills.rs）；真机发现与调用：**未验证** |
| rule | project | 仓库根 `AGENTS.md`（就近优先） | Markdown | supported（受管片段） | agents.md | 片段落盘自动化验证；宿主加载：未验证 |
| rule（条件 paths） | project | — | — | unsupported（片段无条件语义，显式报错） | AIL-010 | 自动化断言 Unsupported |
| agent（自定义子代理） | project | — | — | **unknown**（官方文档未确认项目级自定义 Agent）→ AILoom 显式 Unsupported，不写用户级配置充数 | 未能核实 | 自动化断言 Unsupported |
| mcp（stdio） | project | `.codex/config.toml` `mcp_servers.<id>`（官方：trusted projects only） | TOML | 配置写入 supported；**宿主加载在 0.154.0 `codex exec` 实测不生效（含 trust_level 覆盖重试）**，AILoom 状态展示区分「已部署/宿主已加载」，不写用户级配置充数 | extend/mcp + 2026-09-13（0.153.4）与 2026-09-16（0.154.0）实机 | TOML 合并自动化验证 ✔；`codex mcp list` 与 exec 会话均不加载项目级服务器（证据：docs/evidence/2026-09-13-host-acceptance.md、console/2026-09-16-host-probes.md） |
| mcp（http） | project | 同上 `mcp_servers.<id>.url` | TOML | supported | config-reference | 渲染断言；真实连接：未验证 |
| 环境变量插值 | — | — | — | **unknown**（官方文档未确认 config 中 `${VAR}` 插值）→ 含秘密引用（`$ENV:NAME`）的服务拒绝写入 Codex 配置，绝不落明文 | 未能核实 | 自动化断言 Unsupported |
| 全局配置 | user | `~/.codex/config.toml` | TOML | AILoom 初版不写用户级配置 | config-reference | — |

## 明确不做

- 不把项目级能力降级写进 `~/.codex/config.toml`。
- 不在未核实插值能力时把秘密值以明文写进配置。
- 不把“项目受信”状态当作 AILoom 职责（trust_level 由用户/宿主管理）。
