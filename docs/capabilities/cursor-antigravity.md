# Cursor / Antigravity 能力矩阵（T07/T08，rules-only）

核实日期：2026-09-11。级别：file-placed（文件落盘+格式核实；**真实宿主加载未验证**）。

| 宿主 | 发现路径 | 格式 | 验证级别 | 来源 |
|---|---|---|---|---|
| Cursor | `.cursor/rules/*.mdc`，frontmatter description + alwaysApply/globs | Markdown (mdc) | file-placed | Cursor 官方文档（项目规则） |
| Antigravity | `.antigravity/rules/*.md`，frontmatter trigger/description（Windsurf 血统推断） | Markdown | **file-placed（发现路径待官方核实）** | 推断自 Windsurf 约定 |

## 明确不做

- 不写全局（~）目录
- 真实宿主加载验证由用户在试用中完成，完成后升级为 supported

## zcode / pi（T09 核实记录，2026-09-11）

| 宿主 | 核实结果 | 级别 |
|---|---|---|
| pi | 项目用 AGENTS.md 约定（pi 自身仓库实践）；`.pi/` 目录存在但 rules 专用目录未确认 | unknown（不入注册表） |
| zcode | 公开渠道无法核实项目级 rules 发现路径 | unknown（不入注册表） |

两宿主在发现路径核实后，以纯配置加入 `src/adapters/registry.rs::RULES_HOSTS` 即可接入。
