# AILoom 任务看板

> 账本基准：2026-09-13 返工轮次完成。以 [cards.json](cards.json) 为状态唯一真相；卡头与 JSON 必须一致。

**当前：35 张 Done（含 9 张保留核心结论 + 返工/补验收通过 + AIL-038 新增），3 张 Blocked（AIL-009/012/014，均为外部授权或宿主能力阻塞，本地证据完整），0 张 Backlog。**

2026-09-14 追加：契约 v1.1（路径解析改 XDG 优先 + 规范默认 + 旧目录自动迁移）；AIL-038 团队源脚手架（`ailoom source`）与 release workflow 本地准备。

本轮返工执行记录：逐卡「重新关卡记录」见各卡；真机与全量验证证据见 [evidence/](evidence/) 与 [evidence/logs/](evidence/logs/2026-09-13-full-validation.log)。

直接派工：[REWORK.md](REWORK.md)。历史事实：[完成度与代码审查报告](reviews/2026-09-12-completion-review.md)、[2026-09-12 历史档案](reviews/2026-09-12-card-history.md)。

## 领取与关卡

- Blocked 表示存在明确外部阻塞：AIL-009/012=Codex 项目级 skills/MCP 实机不加载（实测 0.153.4，矩阵已降级）；AIL-014=真实 GitHub PR 创建缺少授权测试仓。解除条件逐卡写在卡面「重新关卡记录」。
- 必需验收缺失或依赖不可用时保持开放；不能用 Done + 缺口文字代替。
- 可选能力未实现单列在卡面记录（如 AIL-034 组织枚举分页），不冒充必需项完成。
- 真实发布、对外 PR/评论等动作按该次任务授权执行。

## 本轮未关闭工作（Blocked）

| 卡片 | 优先级 | 阻塞 |
|---|---|---|
| [AIL-009](cards/AIL-009.md) | P1 | 宿主/外部验收阻塞（本地证据完整，解除条件见卡面） |
| [AIL-012](cards/AIL-012.md) | P2 | 宿主/外部验收阻塞（本地证据完整，解除条件见卡面） |
| [AIL-014](cards/AIL-014.md) | P2 | 宿主/外部验收阻塞（本地证据完整，解除条件见卡面） |

## 保留交付与后置边界

- 9 张保留 Done：AIL-001、003、004、005、006、007、013、016、025（受返工影响的接口已在本轮回归）。
- AIL-029 的实际发布与发布时包名复核是独立后置发布事项；本地安装/校验/卸载已验收。
- AIL-030 为设计交付；实施子卡见 [backend/subcards/](backend/subcards/README.md)，全部 Open。
- 本轮真机验收边界：Codex 项目级 skills/MCP 实测不加载（能力矩阵已降级）；Windows 未验证。

## 卡片总表

| 卡片 | 任务 | 里程碑 | 规模 | 状态 | 前置 |
|---|---|---|---|---|---|
| [AIL-001](cards/AIL-001.md) | 冻结初版领域与配置契约 | M0 | M | Done | — |
| [AIL-002](cards/AIL-002.md) | 建立 Rust CLI 与工程基础 | M0 | S | Done | AIL-001 |
| [AIL-003](cards/AIL-003.md) | 工作区识别与机器数据分区 | M1 | L | Done | AIL-001, AIL-002 |
| [AIL-004](cards/AIL-004.md) | Git 资源源、缓存与版本锁定 | M1 | M | Done | AIL-002, AIL-003 |
| [AIL-005](cards/AIL-005.md) | 项目绑定与 init/status 命令 | M1 | M | Done | AIL-001, AIL-003, AIL-004 |
| [AIL-006](cards/AIL-006.md) | 资源清单与项目角色解析器 | M1 | L | Done | AIL-001, AIL-004, AIL-005 |
| [AIL-007](cards/AIL-007.md) | 同步计划、托管清单与差异预览 | M1 | L | Done | AIL-006 |
| [AIL-008](cards/AIL-008.md) | 同步执行、锁与失败恢复 | M1 | L | Done | AIL-007 |
| [AIL-009](cards/AIL-009.md) | Skills 适配：Claude Code 与 Codex | M1 | M | Blocked | AIL-006, AIL-007, AIL-008 |
| [AIL-010](cards/AIL-010.md) | 规则与项目文档适配 | M1 | M | Done | AIL-007, AIL-008, AIL-009 |
| [AIL-011](cards/AIL-011.md) | 专用 Agent 统一定义与适配 | M1 | M | Done | AIL-006, AIL-007, AIL-008 |
| [AIL-012](cards/AIL-012.md) | MCP 定义、适配与托管更新 | M1 | L | Blocked | AIL-006, AIL-007, AIL-008 |
| [AIL-013](cards/AIL-013.md) | doctor 与安全卸载 | M1 | M | Done | AIL-009, AIL-010, AIL-011, AIL-012 |
| [AIL-014](cards/AIL-014.md) | 资源贡献与审核发布 | M2 | L | Blocked | AIL-004, AIL-006, AIL-007, AIL-008 |
| [AIL-015](cards/AIL-015.md) | 经验文档与项目归属 | M2 | M | Done | AIL-001, AIL-005, AIL-006, AIL-014 |
| [AIL-016](cards/AIL-016.md) | 知识索引与隔离召回 | M2 | L | Done | AIL-006, AIL-015 |
| [AIL-017](cards/AIL-017.md) | 召回 Agent 与经验总结 Skill | M2 | M | Done | AIL-009, AIL-011, AIL-015, AIL-016 |
| [AIL-018](cards/AIL-018.md) | Hook 生命周期与事件协议 | M3 | L | Done | AIL-003, AIL-005, AIL-008 |
| [AIL-019](cards/AIL-019.md) | 会话、Token 与人工干预聚合 | M3 | L | Done | AIL-018 |
| [AIL-020](cards/AIL-020.md) | 经验总结提示与会话摘要 | M3 | M | Done | AIL-015, AIL-017, AIL-019 |
| [AIL-021](cards/AIL-021.md) | 本地实时看板 | M3 | M | Done | AIL-019 |
| [AIL-022](cards/AIL-022.md) | 团队统计上报与 digest | M3 | L | Done | AIL-014, AIL-019, AIL-020 |
| [AIL-023](cards/AIL-023.md) | 初版跨项目端到端验收与文档 | M3 | L | Done | AIL-013, AIL-014, AIL-017, AIL-020, AIL-021, AIL-022 |
| [AIL-024](cards/AIL-024.md) | 成员名册与项目查询 | NEXT | M | Done | AIL-005, AIL-014 |
| [AIL-025](cards/AIL-025.md) | 多源订阅、标签和资源版本更新 | NEXT | L | Done | AIL-004, AIL-006, AIL-007, AIL-014 |
| [AIL-026](cards/AIL-026.md) | 代码事实与增量知识图谱 | NEXT | XL | Done | AIL-016 |
| [AIL-027](cards/AIL-027.md) | 图关系辅助召回 | NEXT | L | Done | AIL-016, AIL-026 |
| [AIL-028](cards/AIL-028.md) | 知识反馈、晋升与维护 | NEXT | L | Done | AIL-016, AIL-017, AIL-022 |
| [AIL-029](cards/AIL-029.md) | 安装分发与 npm 包装 | NEXT | M | Done | AIL-002, AIL-023 |
| [AIL-030](cards/AIL-030.md) | 管理后端、身份与审核模型设计 | LATER | XL | Done | AIL-001, AIL-014, AIL-022, AIL-024 |
| [AIL-031](cards/AIL-031.md) | 共享环境配置与团队文化 | NEXT | M | Done | AIL-006, AIL-010, AIL-012 |
| [AIL-032](cards/AIL-032.md) | 团队自定义 Hook 分发 | NEXT | L | Done | AIL-006, AIL-008, AIL-018 |
| [AIL-033](cards/AIL-033.md) | 团队声明的软件包与插件依赖 | NEXT | L | Done | AIL-004, AIL-007, AIL-009, AIL-029 |
| [AIL-034](cards/AIL-034.md) | 目录、仓库与批量知识导入 | NEXT | L | Done | AIL-015, AIL-016, AIL-026 |
| [AIL-035](cards/AIL-035.md) | PR/MR 经验与 CI 知识更新 | NEXT | L | Done | AIL-014, AIL-015, AIL-026, AIL-034 |
| [AIL-036](cards/AIL-036.md) | 业务仓库同仓资源模式与迁移 | NEXT | XL | Done | AIL-003, AIL-004, AIL-008, AIL-014, AIL-022 |
| [AIL-037](cards/AIL-037.md) | 事件留存、数据导出与清理 | NEXT | M | Done | AIL-003, AIL-018, AIL-019, AIL-022 |
| [AIL-038](cards/AIL-038.md) | 团队源脚手架与发布链路本地准备 | NEXT | M | Done | AIL-001, AIL-005, AIL-029 |
