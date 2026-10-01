# 支持范围

更新日期：2026-10-01。规则：**未知标 unknown；待核实不能标 supported**（契约 §8）。宿主能力的逐项结论、官方来源与核实日期以 [能力矩阵](../capabilities/) 为准，本页只做汇总。

## 宿主工具

| 工具 | 定位 | 项目级能力 | 详细矩阵 |
|---|---|---|---|
| Claude Code | 主要宿主 | Skills（真机发现与调用已验证）、Rules、Agents、MCP（真机连接与工具调用已验证）、Hooks、文档索引 | [claude-code.md](../capabilities/claude-code.md) |
| Codex CLI | 主要宿主 | Skills：`.agents/skills` 真机发现与调用已验证；Rules：`AGENTS.md` 受管片段；MCP：项目级配置可写入，但实测版本不加载 | [codex.md](../capabilities/codex.md) |
| Grok、Pi、OpenCode、Cursor | 附加宿主 | 按官方文档的路径部署；各项的验证程度不同，见矩阵 | [extra-hosts.md](../capabilities/extra-hosts.md) |
| alva、Antigravity | 实验 | 仅文件落盘 | [alva.md](../capabilities/alva.md)、[cursor-antigravity.md](../capabilities/cursor-antigravity.md) |

Codex 项目级 Agent 的支持结论在文档与适配器之间尚不一致，统一核实见 AIL-142。宿主内行为逐项标注实测状态；unknown 条目不作为 supported 证据。

## 平台

| 平台 | CLI 与网页 | 登录自启动（`ailoom service enable`） |
|---|---|---|
| macOS | 已验证 | LaunchAgent（写入 `~/Library/LaunchAgents`，属于系统级副作用） |
| Linux | CI（ubuntu-latest）运行全量测试，本地未人工验证 | systemd 用户单元（`~/.config/systemd/user`） |
| Windows | 未验证 | 不支持，明确报错；可手动 `ailoom service start` |

构建要求：Rust 1.85+（CI 固定检查 MSRV）、git 2.x。

## 功能支持级别

| 能力 | 级别 | 说明与证据 |
|---|---|---|
| 团队源：项目/角色并集选择、shared 显式、Git 源锁与离线 | supported | tests/resolver.rs、tests/source.rs |
| 计划 / 同步 / 冲突保留 / journal 恢复 / 并发锁 | supported | tests/sync_plan.rs、tests/sync_apply.rs、tests/e2e.rs |
| 个人层：仓外配置、仓库 / Worktree / 子目录三态选择、个人指令 | supported | tests/personal.rs、tests/personal_instructions.rs |
| 团队同步与个人同步交替执行不互删 | supported | tests/personal.rs、tests/collections.rs（组合场景） |
| 资源库、资源合集（Git / 本地 / 外部目录）、CC Switch 迁移 | supported | tests/collections.rs、tests/skill_sources.rs |
| 网页控制台（loopback、会话令牌、目录授权） | supported | tests/console_*.rs；浏览器验收 `scripts/ui-browser.sh` |
| 网页服务与登录自启动 | supported（macOS / Linux） | tests/service.rs |
| 原生 Rules / Agent 文件编辑（含「仅本机生效」） | supported | tests/native_files.rs、tests/native_local.rs；见 [原生文件](NATIVE-FILES.md) |
| 项目知识库：保存 / 召回 / 迁移 / 换机恢复 | supported | tests/knowledge_location.rs、tests/knowledge_portable.rs |
| 项目内未托管 Skill 扫描与双重确认删除 | supported | tests/console_delete_skill.rs；evidence/ail112-ui-2026-10-01 |
| 贡献变更集 + PR / 手动审核 | supported（PR 自动创建仅 GitHub + gh；未在真实 GitHub 验证） | tests/contribution.rs |
| Hook 事件采集 / 会话与 Token 聚合 / 摩擦提示 / 本地看板 | supported（Hook 采集仅 Claude；Codex unknown） | tests/hook_events.rs、tests/session_metrics.rs、tests/dashboard_reporting.rs |
| 统计上报（Git 报告分支，默认关闭） | supported（本地裸仓验证；无实时总线宣称） | tests/dashboard_reporting.rs |
| 管理后端 / SSO / RBAC | 不在范围（Git namespace 不是 RBAC） | [设计稿](../design/backend/architecture.md) |

## 公司文件与 Git 工作区

两个功能对已跟踪文件的处理方式不同，按功能区分：

- **个人指令与个人同步**：公司已跟踪的 AGENTS.md / CLAUDE.md / 宿主配置及暂存区保持原样。个人内容写到独立的本地文件（如 `.claude/rules/ailoom-personal.md`、Codex 个人视图），未跟踪的文件通过 Git 本地 exclude 避免被普通 `git add` 收入。**不使用** skip-worktree / assume-unchanged / rm --cached。「严格零新增文件」模式不支持。
- **原生文件「仅本机生效」**：在原路径写入个人版本。未跟踪的文件同样用本地 exclude；**已跟踪的文件会设置当前 Worktree 索引的 skip-worktree 标记**，并保留原文以便恢复。skip-worktree 不是「永不提交」的安全边界，拉取、切分支或变基前应先恢复项目版本。详见 [原生文件](NATIVE-FILES.md)。

其他已知限制：AILoom 不部署某项能力，不等于宿主已禁用它（宿主仍可能从全局或祖先目录加载，界面如实区分）。

## 明确不做

- 不写用户级宿主配置来冒充项目级能力（「全局规则与 Agent」页面是用户显式编辑用户级文件，不在此列）。
- 不宣称「同步成功 = 宿主发现成功 = Agent 使用成功」，三层验收分开。
- 不把启发式纠正计数称为真实错误率；不把干预次数当绩效。
