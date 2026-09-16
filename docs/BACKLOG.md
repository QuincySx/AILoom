# AILoom 任务看板

> 状态基准：2026-09-15 第二轮 review 退回；[cards.json](cards.json) 是主卡状态真相。

**51 张主卡：0 Backlog、4 Blocked、47 Done。另有 18 张 RW 返工子卡，单独统计。**

[返工派工单](REWORK.md) · [18 张可领取子卡](rework/2026-09-15/README.md) · [本轮审查](reviews/2026-09-15-agent-implementation-review.md) · [退回前完整快照](reviews/2026-09-15-before-rework/README.md)

## 领取与关卡

- 本轮 18 张 RW 子卡全部完成；子卡完成且原始必需验收核对后主卡关卡。
- 子卡完成不自动关闭主卡；必须同时满足原始必需验收。状态变化同步卡头、对应账本和看板/索引。
- AIL-009/012 保留宿主必需验收阻塞；AIL-014 保留真实 GitHub PR 创建验收阻塞，解除条件见各卡。009 另需 RW-01 本地迁移回归。
- 当前测试 286 passed / 0 failed / 1 ignored（真实 npm registry 用例按设计 ignored）+ fmt/Clippy 干净（[全量日志](evidence/logs/2026-09-15-rework-full-validation.log)）。新关卡不得引用旧全绿次数。
- AIL-030 是设计完成，后端子卡仍 Open；真实发布是独立后置事项。

## 本地控制台与 Onboarding 新需求

[需求/回复审查](reviews/2026-09-15-onboarding-requirements-review.md) · [规格、阶段与派工](initiatives/local-console.md)。AIL-039～051（本地控制台与个人配置管理）本轮完成：**12 Done + 1 Blocked**（AIL-051 等待三名试用者人工验收，解除条件见卡）。契约新增 §11（v1.3）；真机/真浏览器证据见 [evidence/console/](evidence/console/)；当前测试 352 passed / 0 failed（[全量日志](evidence/logs/2026-09-16-local-console-final.log)）。

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
| [AIL-039](cards/AIL-039.md) | Git 仓库身份、工作树与子项目发现 | LOCAL | XL | Done | AIL-003 |
| [AIL-040](cards/AIL-040.md) | 仓外个人配置、作用域继承与兼容迁移 | LOCAL | XL | Done | AIL-039, AIL-001 |
| [AIL-041](cards/AIL-041.md) | 宿主能力探测与 Codex 项目加载复核 | LOCAL | L | Done | AIL-009, AIL-012 |
| [AIL-042](cards/AIL-042.md) | 公司指令文件保护与个人本地调整 | LOCAL | XL | Done | AIL-040, AIL-041 |
| [AIL-043](cards/AIL-043.md) | 个人资源库与已有 Skill 导入 | LOCAL | L | Done | AIL-038, AIL-040 |
| [AIL-044](cards/AIL-044.md) | 按资源与宿主选择能力及解释有效配置 | LOCAL | L | Done | AIL-040, AIL-041 |
| [AIL-045](cards/AIL-045.md) | Grill 到实现的流程包与文档产物关联 | LOCAL | L | Done | AIL-043, AIL-044, AIL-015 |
| [AIL-046](cards/AIL-046.md) | 本地 Web 控制服务与受限 API | LOCAL | L | Done | AIL-039, AIL-040, AIL-021 |
| [AIL-047](cards/AIL-047.md) | 个人模式 Onboarding 首次成功体验 | LOCAL | L | Done | AIL-042, AIL-043, AIL-044, AIL-046, AIL-050 |
| [AIL-048](cards/AIL-048.md) | 仓库工作树与子项目日常配置界面 | LOCAL | L | Done | AIL-039, AIL-040, AIL-044, AIL-046, AIL-050, AIL-047 |
| [AIL-049](cards/AIL-049.md) | 源、Skill、MCP 与流程文档可视化编辑 | LOCAL | L | Done | AIL-043, AIL-044, AIL-045, AIL-046, AIL-050, AIL-047 |
| [AIL-050](cards/AIL-050.md) | 预览应用任务、宿主验证与可恢复撤销 | LOCAL | XL | Done | AIL-042, AIL-044, AIL-046, AIL-008, AIL-013 |
| [AIL-051](cards/AIL-051.md) | Onboarding 与多工作树真实体验验收 | LOCAL | XL | Blocked | AIL-039, AIL-040, AIL-041, AIL-042, AIL-043, AIL-044, AIL-045, AIL-046, AIL-047, AIL-048, AIL-049, AIL-050 |
