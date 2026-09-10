# alva 宿主能力矩阵（AIL-011 扩展 / T11）

核实日期：2026-09-11。来源：alva-agent 源码（`crates/alva-app-core/src/paths.rs`、`.alva/agents.toml` 实例、`alva-protocol-skill`）——用户自研项目，源码即权威。
本机版本：alva-agent main（refactor/kernel-boundary 活跃开发中）。

| resource_kind | scope | 发现路径 | 格式 | 支持级别 | 实际验证 |
|---|---|---|---|---|---|
| skill | project/user | `.claude/skills/`（**co-load**）、`.agents/skills/`、`.alva/skills/` | SKILL.md（Anthropic 格式兼容，三级渐进加载） | supported（**co-load 免费覆盖**：AILoom Claude 适配部署即生效，无需单独适配） | 文件落盘断言 + alva 源码路径核实 |
| mcp | project/user | `.mcp.json`、`.alva/mcp.json`（**co-load**） | JSON | supported（同上，Claude 适配产物直接复用） | 同上 |
| agent | project | `.alva/agents.toml` `[[agent]]`：name/description/system_prompt_base (req)、allowed_tools/max_iterations (opt)；按 name overlay | TOML | supported（`TomlArrayEntry` 条目粒度管理；model 字段 alva 未支持 → 显式降级） | T11 渲染+托管断言 |
| rule | — | AGENTS.md（项目手工维护的 single source of truth + 分形级联） | Markdown | unsupported（AILoom 不注入；skills-first 决策） | 渲染路径断言 unsupported |
| env | — | `.alva/config.json` | JSON | unknown（环境注入能力未核实） | 显式 unsupported |
| hook | — | 未核实 | — | unknown | — |

## 明确不做

- 不写 alva 的 AGENTS.md（项目 single source of truth）
- model 字段：alva 无 per-agent model → AIL-011 model ≠ inherit 时显式降级报告
