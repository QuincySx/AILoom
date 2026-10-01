# 原生规则、Agent 与本地 Skill

入口：全局「规则与 Agent」；项目设置「Rules 与 Agent」；目录能力页的「本地已有」。

原生编辑保存完整文件，不做格式转换或规则继承合成。Git 项目可选择直接修改项目文件或仅本机生效。保留 YAML frontmatter、条件、权限和正文。新建拒绝覆盖同名文件；保存/删除核对读取时的内容哈希，原文备份到数据目录 native-files/backups。操作不提交 Git，也不安装扩展。AILoom 创建与 AILoom 部署管理是不同标记。

## 已接入范围

| 工具 | 项目 | 全局 |
| --- | --- | --- |
| Claude Code | .claude/rules/*.md、.claude/agents/*.md、CLAUDE.md、.claude/CLAUDE.md | ~/.claude/rules、agents、CLAUDE.md；尊重 CLAUDE_CONFIG_DIR |
| Codex / 通用 | AGENTS.md、.codex/agents/*.toml | $CODEX_HOME/AGENTS.md、agents/*.toml（默认 ~/.codex） |
| Cursor | .cursor/rules/*.mdc、.cursor/agents/*.md | ~/.cursor/agents/*.md；User Rules 留在 Cursor 设置 |
| Pi | .pi/agents/*.md、.pi/APPEND_SYSTEM.md | ~/.pi/agent/agents、APPEND_SYSTEM.md；尊重 PI_CODING_AGENT_DIR |
| OpenCode | .opencode/agents/*.md；根 AGENTS.md 共用通用入口 | $XDG_CONFIG_HOME/opencode/agents、AGENTS.md（默认 ~/.config） |
| Grok | .grok/rules/*.md、.grok/agents/*.md | 尚未接入 |

Codex Agent 路径按 [官方子代理文档](https://learn.chatgpt.com/docs/agent-configuration/subagents) 核实；只编辑独立 TOML，不触碰主配置。Pi Agent 依赖官方 subagent 扩展及其 agentScope 设置；这里只维护文件，不声称扩展已经安装或 Agent 已被运行时加载。原生文件的符号链接显示扫描提示，不通过编辑入口跨链接读写。

路径依据：[Claude Rules](https://code.claude.com/docs/en/memory)、[Claude Subagents](https://code.claude.com/docs/en/sub-agents)、[Cursor Rules](https://cursor.com/docs/rules)、[Cursor Subagents](https://prod.cursor.com/docs/subagents)、[OpenCode Agents](https://opencode.ai/docs/agents/)。Pi 与 Grok 沿用 [extra-hosts.md](../capabilities/extra-hosts.md) 的固定版本证据。

## 本地 Skill

扫描 .claude、.agents、.codex、.cursor、.grok、.pi、.opencode 的 skills 目录，包含一层分类目录。项目内的链接可展开，同一目标去重；项目外链接只列出位置，不读取内容。链接到 AILoom Store 的 Skill 归类为已管理，其余为本地。

项目根节点优先使用主 Worktree，具体 Worktree/子环境按选中的实际路径扫描。不会为了扫描而注册、导入或修改项目资源。

本次验证：原生文件 CRUD、全局/项目隔离、旧版本拒绝、备份、越界与符号链接保护；七种 Skill 目录和项目内 Skillshare 双链接去重；HTTP 会话及目录授权。Orca 浏览器在测试项目完成 Rule 新建/编辑/删除和 Agent 新建/删除。真实 app-monorepo 只读识别 36 个 Skill。

## Git 项目中的保存方式

- **修改项目文件**：像文本编辑器一样保存；Git 正常记录修改。
- **仅本机生效**：个人内容仍写入原路径，AI 工具照常读取。未跟踪文件在 `git rev-parse --git-path info/exclude` 指向的本地排除文件内增加精确路径；已跟踪文件要求工作副本和暂存区干净，再设置当前 Worktree 索引的 `skip-worktree` 标记。
- **恢复项目版本**：先保留最新个人内容，恢复启用前的文件，移除 AILoom 添加的标记；新增文件则移除本地入口。再次编辑可载入个人副本。已处于本机模式时选择“修改项目文件”并保存，会明确把编辑内容变成普通项目改动。

恢复记录和个人副本保存在 AILoom 数据目录 `native-files/local`，当前不自动同步到知识库。记录在磁盘/索引写入前持久化，包含原文、个人内容、索引基准和阶段；中断后可以继续恢复。应用时占用 Git 的索引锁，避免与正常 Git 写操作同时替换工作文件。Git 原文件版本改变、暂存冲突、已有索引标记、稀疏检出、符号链接以及资源库管理的生成文件不自动接管。数据目录位于业务仓库内时拒绝保存本机副本，防止个人内容混入业务提交。

已跟踪文件的标记与替换记录按 Worktree 隔离；`info/exclude` 是同一 Git 仓库的 Worktree 共用的排除规则。只移除本功能拥有的规则块，保留其他排除项。不修改用户全局 Git 配置、项目 `.gitignore` 或 Git hooks，不暂存和提交文件。

`skip-worktree` 不是“永不提交”的安全边界，外部 Git 操作可能改变标记或基准。拉取、切分支或变基前应先恢复项目版本；基准已变时停止写入、保留个人副本供核对，不自动把旧原文覆盖到新版。Git 语义依据：[update-index](https://git-scm.com/docs/git-update-index#_skip_worktree_bit)、[gitignore](https://git-scm.com/docs/gitignore)。
