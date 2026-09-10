# 支持范围与能力矩阵

更新日期：2026-09-09。规则：**未知标 unknown；待核实不能标 supported**（契约 §8）。

## 宿主工具

| 工具 | 实测版本 | 项目级支持（初版） | 详细矩阵 |
|---|---|---|---|
| Claude Code | 2.1.266 | skills / rules / agents / MCP / hooks / 文档索引 | [claude-code.md](capabilities/claude-code.md) |
| Codex CLI | 0.153.4 | skills（config 数组）/ rules（AGENTS.md 片段）/ MCP / 文档索引 | [codex.md](capabilities/codex.md) |

宿主内真实加载/调用验收需要交互环境：文件落盘与官方发现路径一致性已自动化断言，
宿主内行为在矩阵中逐行标注"未验证"。这些条目不作为 supported 证据。

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
