# AILoom 任务看板

创建日期：2026-09-09。所有卡片尚未实现；初版是 AIL-001—AIL-023，后续是 AIL-024—AIL-037。

这是项目内 Markdown 任务卡，未在外部看板建卡，也未启动自动实现任务。领取时将状态改为 Ready / In progress / Blocked / Done，并同步 cards.json。

## 里程碑与交付边界

| 里程碑 | 目标 | 完成定义 |
|---|---|---|
| M0 | 契约与 Rust 工程 | 领域区分清楚，CLI 工程可运行 |
| M1 | 项目资源可用 | 项目/角色解析、Git 版本、计划与同步、Skills/Rules/Agent/MCP、诊断卸载 |
| M2 | 团队知识闭环 | 审核贡献、项目经验、隔离召回与内置召回 Agent |
| M3 | 可观测初版 | Hook、统计、摘要、本地看板、团队 digest、完整验收 |
| NEXT | 深化知识与分发 | 成员名册、多源、代码图谱、维护、安装分发 |
| LATER | 可选管理平台 | 身份权限、设备、审核发布后端设计 |

M1 是首个可用演示点，不代表完整初版完成。M2 与 M3 的部分任务可在依赖满足后并行。

建议先做 AIL-001 → AIL-002 → AIL-003 → AIL-004 → AIL-005 → AIL-006 → AIL-007 → AIL-008，再展开资源适配卡。

## 卡片总表

| 卡片 | 任务 | 里程碑 | 规模 | 前置 |
|---|---|---|---|---|
| [AIL-001](cards/AIL-001.md) | 冻结初版领域与配置契约 | M0 | M | — |
| [AIL-002](cards/AIL-002.md) | 建立 Rust CLI 与工程基础 | M0 | S | AIL-001 |
| [AIL-003](cards/AIL-003.md) | 工作区识别与机器数据分区 | M1 | L | AIL-001, AIL-002 |
| [AIL-004](cards/AIL-004.md) | Git 资源源、缓存与版本锁定 | M1 | M | AIL-002, AIL-003 |
| [AIL-005](cards/AIL-005.md) | 项目绑定与 init/status 命令 | M1 | M | AIL-001, AIL-003, AIL-004 |
| [AIL-006](cards/AIL-006.md) | 资源清单与项目角色解析器 | M1 | L | AIL-001, AIL-004, AIL-005 |
| [AIL-007](cards/AIL-007.md) | 同步计划、托管清单与差异预览 | M1 | L | AIL-006 |
| [AIL-008](cards/AIL-008.md) | 同步执行、锁与失败恢复 | M1 | L | AIL-007 |
| [AIL-009](cards/AIL-009.md) | Skills 适配：Claude Code 与 Codex | M1 | M | AIL-006, AIL-007, AIL-008 |
| [AIL-010](cards/AIL-010.md) | 规则与项目文档适配 | M1 | M | AIL-007, AIL-008, AIL-009 |
| [AIL-011](cards/AIL-011.md) | 专用 Agent 统一定义与适配 | M1 | M | AIL-006, AIL-007, AIL-008 |
| [AIL-012](cards/AIL-012.md) | MCP 定义、适配与托管更新 | M1 | L | AIL-006, AIL-007, AIL-008 |
| [AIL-013](cards/AIL-013.md) | doctor 与安全卸载 | M1 | M | AIL-009, AIL-010, AIL-011, AIL-012 |
| [AIL-014](cards/AIL-014.md) | 资源贡献与审核发布 | M2 | L | AIL-004, AIL-006, AIL-007, AIL-008 |
| [AIL-015](cards/AIL-015.md) | 经验文档与项目归属 | M2 | M | AIL-001, AIL-005, AIL-006, AIL-014 |
| [AIL-016](cards/AIL-016.md) | 知识索引与隔离召回 | M2 | L | AIL-006, AIL-015 |
| [AIL-017](cards/AIL-017.md) | 召回 Agent 与经验总结 Skill | M2 | M | AIL-009, AIL-011, AIL-015, AIL-016 |
| [AIL-018](cards/AIL-018.md) | Hook 生命周期与事件协议 | M3 | L | AIL-003, AIL-005, AIL-008 |
| [AIL-019](cards/AIL-019.md) | 会话、Token 与人工干预聚合 | M3 | L | AIL-018 |
| [AIL-020](cards/AIL-020.md) | 经验总结提示与会话摘要 | M3 | M | AIL-015, AIL-017, AIL-019 |
| [AIL-021](cards/AIL-021.md) | 本地实时看板 | M3 | M | AIL-019 |
| [AIL-022](cards/AIL-022.md) | 团队统计上报与 digest | M3 | L | AIL-014, AIL-019, AIL-020 |
| [AIL-023](cards/AIL-023.md) | 初版跨项目端到端验收与文档 | M3 | L | AIL-013, AIL-014, AIL-017, AIL-020, AIL-021, AIL-022 |
| [AIL-024](cards/AIL-024.md) | 成员名册与项目查询 | NEXT | M | AIL-005, AIL-014 |
| [AIL-025](cards/AIL-025.md) | 多源订阅、标签和资源版本更新 | NEXT | L | AIL-004, AIL-006, AIL-007, AIL-014 |
| [AIL-026](cards/AIL-026.md) | 代码事实与增量知识图谱 | NEXT | XL | AIL-016 |
| [AIL-027](cards/AIL-027.md) | 图关系辅助召回 | NEXT | L | AIL-016, AIL-026 |
| [AIL-028](cards/AIL-028.md) | 知识反馈、晋升与维护 | NEXT | L | AIL-016, AIL-017, AIL-022 |
| [AIL-029](cards/AIL-029.md) | 安装分发与 npm 包装 | NEXT | M | AIL-002, AIL-023 |
| [AIL-030](cards/AIL-030.md) | 管理后端、身份与审核模型设计 | LATER | XL | AIL-001, AIL-014, AIL-022, AIL-024 |
| [AIL-031](cards/AIL-031.md) | 共享环境配置与团队文化 | NEXT | M | AIL-006, AIL-010, AIL-012 |
| [AIL-032](cards/AIL-032.md) | 团队自定义 Hook 分发 | NEXT | L | AIL-006, AIL-008, AIL-018 |
| [AIL-033](cards/AIL-033.md) | 团队声明的软件包与插件依赖 | NEXT | L | AIL-004, AIL-007, AIL-009, AIL-029 |
| [AIL-034](cards/AIL-034.md) | 目录、仓库与批量知识导入 | NEXT | L | AIL-015, AIL-016, AIL-026 |
| [AIL-035](cards/AIL-035.md) | PR/MR 经验与 CI 知识更新 | NEXT | L | AIL-014, AIL-015, AIL-026, AIL-034 |
| [AIL-036](cards/AIL-036.md) | 业务仓库同仓资源模式与迁移 | NEXT | XL | AIL-003, AIL-004, AIL-008, AIL-014, AIL-022 |
| [AIL-037](cards/AIL-037.md) | 事件留存、数据导出与清理 | NEXT | M | AIL-003, AIL-018, AIL-019, AIL-022 |
