# 支持范围与能力矩阵

更新日期：2026-09-16。规则：**未知标 unknown；待核实不能标 supported**（契约 §8）。

## 宿主工具

| 工具 | 实测版本 | 项目级支持 | 详细矩阵 |
|---|---|---|---|
| Claude Code | 2.1.272 | skills（真机发现+调用 ✔）/ rules / agents / MCP（真机连接+工具调用 ✔）/ hooks / 文档索引 | [claude-code.md](capabilities/claude-code.md) |
| Codex CLI | 0.154.0 | skills：`.agents/skills` **真机发现+调用 ✔（2026-09-16）**；rules（AGENTS.md 片段）；MCP 项目级配置落盘 ✔ 但宿主**仍不加载**（0.154.0 复测，见矩阵） | [codex.md](capabilities/codex.md) |

真机探测证据：[evidence/console/2026-09-16-host-probes.md](evidence/console/2026-09-16-host-probes.md)。
宿主内行为逐行标注实测状态；unknown 条目不作为 supported 证据。

## 个人模式（2026-09-16 起）

- 个人配置/资源库/流程产物存放在机器数据区（仓外）；项目根无需 `.ailoom/project.toml`。
- 公司已跟踪的 AGENTS.md/CLAUDE.md/宿主配置与暂存区在个人 apply/sync/uninstall 中保持原样；
  不使用 skip-worktree/assume-unchanged/rm --cached。
- 已知限制：AILoom 不部署 ≠ 宿主已禁用（宿主仍可能从全局/祖先目录加载能力，界面如实区分）；
  Codex 项目级 MCP 在实测版本不加载。
- 个人指令入口需要在工作树放置独立本地文件（如 `.claude/rules/ailoom-personal.md`、
  Codex 个人视图），经 Git 本地 exclude 避免普通 add 收入；「严格零新增文件」模式
  明确不支持（需宿主外部/启动注入能力，AILoom 不假装支持，也不使用
  skip-worktree/assume-unchanged/rm --cached 隐藏修改）。exclude 对已跟踪文件无效，
  强制 `git add -f` 不在产品承诺内。

## 平台

| 平台 | 状态 |
|---|---|
| macOS（开发/验证机） | 已验证（Apple git 2.50.1，rustc 1.93） |
| Linux | CI 矩阵目标（ubuntu-latest），本地未人工验证 |
| Windows | 未验证；分发支持待 AIL-029 确定 |

## 功能支持级别

| 能力 | 级别 | 说明 |
|---|---|---|
| 项目/角色并集选择、shared 显式 | supported | 自动化断言（tests/resolver.rs） |
| Git 源锁/离线/并发 fetch | supported | tests/source.rs |
| 计划/同步/冲突保留/journal 恢复 | supported | tests/sync_plan.rs、tests/sync_apply.rs |
| 贡献变更集 + PR/手动审核 | supported（PR 自动创建仅 GitHub+gh；未在真实 GitHub 验证） | tests/contribution.rs |
| 知识索引/中文检索/项目隔离召回 | supported | tests/recall.rs |
| Hook 事件采集/去重/注册 | supported（Claude）；Codex unknown | tests/hook_events.rs |
| 会话/Token/干预聚合（缺失=unavailable） | supported | tests/session_metrics.rs |
| 摩擦提示（每会话一次，可关闭） | supported | tests/session_metrics.rs |
| 本地看板（loopback SSE） | supported | tests/dashboard_reporting.rs |
| 统计上报（Git 报告分支，默认关闭） | supported（本地裸仓验证；无实时总线宣称） | tests/dashboard_reporting.rs |
| 学习/导入/图谱/多源等 NEXT 能力 | 见 docs/cards/AIL-024—037 | 未实现或部分实现 |
| 管理后端/SSO/RBAC | 不在初版范围（Git namespace 不是 RBAC） | AIL-030（设计） |

## 明确不做（初版）

- 不写用户级宿主配置充数项目级能力。
- 不宣称同步成功 = 宿主发现成功 = Agent 使用成功（三层验收分离）。
- 不把启发式纠正计数称为真实错误率；不把干预次数当绩效。
