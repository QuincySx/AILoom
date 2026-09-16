# Claude Code 能力矩阵（AILoom）

核实日期：2026-09-16（AIL-041 真机复核；初核 2026-09-09）。来源：code.claude.com 官方文档（memory / skills / sub-agents / mcp 页面）。
本机实测版本：Claude Code 2.1.272（`claude --version`）。真机探测证据：[console/2026-09-16-host-probes.md](../evidence/console/2026-09-16-host-probes.md)。

| resource_kind | project/user scope | 发现路径 | 格式 | 热重载/重启 | 支持级别 | 来源 | 实际验证 |
|---|---|---|---|---|---|---|---|
| skill | project | `.claude/skills/<name>/SKILL.md`（子目录 `.claude/skills` 亦发现） | Markdown + YAML frontmatter | 按需加载，无需重启 | supported | code.claude.com/docs/en/skills | **真机 2.1.272 发现+调用通过**（隔离项目 `claude -p` 逐字返回技能标记，2026-09-16） |
| rule（常驻） | project | `.claude/rules/*.md`（递归） | Markdown + YAML frontmatter | 会话加载 | supported | code.claude.com/docs/en/memory | 文件落盘自动化验证；真实宿主加载：未验证 |
| rule（条件 paths） | project | 同上，frontmatter `paths` | 同上 | 同上 | supported（透传 frontmatter） | 同上 | 渲染断言；宿主行为：未验证 |
| rule（条件）→ Codex 片段 | — | — | — | — | unsupported（显式报错） | AIL-010 | 自动化断言 Unsupported |
| agent | project | `.claude/agents/*.md`；frontmatter 必填 name/description，可选 tools/model（inherit）/permissionMode 等 | Markdown + YAML frontmatter | 会话加载 | supported | code.claude.com/docs/en/sub-agents | 渲染格式断言；宿主发现：未验证 |
| mcp（stdio） | project | 项目根 `.mcp.json`，`mcpServers.<name>.{command,args,env}` | JSON | 会话加载，首次需用户批准（`claude mcp list` 显示 Pending approval） | supported | code.claude.com/docs/en/mcp | **真机 2.1.272 配置→连接→无副作用工具调用全链路通过**（2026-09-16；批准后 `mcp__probe__probe_ping` 返回逐字标记） |
| mcp（http） | project | 同上，`{type:"http", url, headers}` | JSON | 同上 | supported | 同上 | 渲染断言；真实连接：未验证 |
| 环境变量插值 | project | `${VAR}` / `${VAR:-default}` 在 command/args/env/url/headers 中展开 | — | — | supported（秘密引用 `$ENV:NAME` → `${NAME}`） | 同上 | 渲染断言 |
| Hook 事件 | project | `.claude/settings.json` | JSON | — | supported（结构）| code.claude.com/docs/en/memory（hooks 键名提及） | AIL-018 实施时再核实事件名 |
| MCP/Agent 的项目筛选（projects/roles/shared 字段） | — | — | — | — | **ailoom-design**（非参考实现既有能力） | AIL-001 §2 | — |

## 明确不做

- 不写用户级（user scope）配置充数项目级能力。
- 不把“文件存在”当作“宿主已加载”。真实宿主发现/调用验收需要交互环境，本卡以官方文档路径 + 落盘断言为验收，宿主内行为标记未验证。

## 追加（2026-09-11，alva 接入实测）

| 项 | 说明 |
|---|---|
| 实际部署验证 | alva-agent 仓库实测：`ailoom sync` 后 `.claude/skills/` 出现技能，与用户既有技能共存；settings.json hooks 注册保留用户条目 |
| 已知默认行为 | 内置召回提示片段默认写 AGENTS.md——对 AGENTS.md 为 single source of truth 的项目应 `init --no-builtin`（alva 接入实测发现并已按此配置） |
