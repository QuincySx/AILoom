# AILoom 任务看板

> 状态基准：2026-09-17 需求实现对齐；[cards.json](cards.json) 是主卡状态真相。

**98 张主卡：58 Backlog、4 Blocked、36 Done。另有旧轮次 18 张 RW 子卡，单独统计。**

## 当前执行入口

[整晚逐卡执行与16项需求覆盖](initiatives/implementation-alignment-2026-09-17.md) · [Web分层实施蓝图](initiatives/web-console-blueprint-2026-09-17.md) · [本轮复审](reviews/2026-09-16-local-console-review.md)

- 新增 AIL-052～079 共28张修复/补齐/验收卡；AIL-080～098共19张Web模块、Token、组件、状态和页面卡。
- 原 AIL-039、040、042～050共11张重新打开；原历史完成记录保留但不再代表当前验收通过。
- 以052～098为执行领取单位，依赖按账本核实。原主卡关联任务见 follow_up_cards；关联任务完成后还需原始必需验收。
- 051仍Blocked：本轮实现与浏览器闭环、真实宿主必需项和三名独立试用者都须有证据；不再表述为只缺人工试用。
- 009/012/014保持既有Blocked；041暂保留Done，由077复核当前宿主证据。030仍仅设计完成，后端B卡未并入本轮。
- 本次只建卡及修正文档状态，未实施新卡或重新运行产品测试；不以旧352/286测试次数作为本轮完成证据。

旧[返工派工单](REWORK.md)、[18张RW记录](rework/2026-09-15/README.md)及[历史快照](reviews/2026-09-15-before-rework/README.md)保留供追溯。

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
| [AIL-039](cards/AIL-039.md) | Git 仓库身份、工作树与子项目发现 | LOCAL | XL | Backlog | AIL-003 |
| [AIL-040](cards/AIL-040.md) | 仓外个人配置、作用域继承与兼容迁移 | LOCAL | XL | Backlog | AIL-039, AIL-001 |
| [AIL-041](cards/AIL-041.md) | 宿主能力探测与 Codex 项目加载复核 | LOCAL | L | Done | AIL-009, AIL-012 |
| [AIL-042](cards/AIL-042.md) | 公司指令文件保护与个人本地调整 | LOCAL | XL | Backlog | AIL-040, AIL-041 |
| [AIL-043](cards/AIL-043.md) | 个人资源库与已有 Skill 导入 | LOCAL | L | Backlog | AIL-038, AIL-040 |
| [AIL-044](cards/AIL-044.md) | 按资源与宿主选择能力及解释有效配置 | LOCAL | L | Backlog | AIL-040, AIL-041 |
| [AIL-045](cards/AIL-045.md) | Grill 到实现的流程包与文档产物关联 | LOCAL | L | Backlog | AIL-043, AIL-044, AIL-015 |
| [AIL-046](cards/AIL-046.md) | 本地 Web 控制服务与受限 API | LOCAL | L | Backlog | AIL-039, AIL-040, AIL-021 |
| [AIL-047](cards/AIL-047.md) | 个人模式 Onboarding 首次成功体验 | LOCAL | L | Backlog | AIL-042, AIL-043, AIL-044, AIL-046, AIL-050 |
| [AIL-048](cards/AIL-048.md) | 仓库工作树与子项目日常配置界面 | LOCAL | L | Backlog | AIL-039, AIL-040, AIL-044, AIL-046, AIL-050, AIL-047 |
| [AIL-049](cards/AIL-049.md) | 源、Skill、MCP 与流程文档可视化编辑 | LOCAL | L | Backlog | AIL-043, AIL-044, AIL-045, AIL-046, AIL-050, AIL-047 |
| [AIL-050](cards/AIL-050.md) | 预览应用任务、宿主验证与可恢复撤销 | LOCAL | XL | Backlog | AIL-042, AIL-044, AIL-046, AIL-008, AIL-013 |
| [AIL-051](cards/AIL-051.md) | Onboarding 与多工作树真实体验验收 | LOCAL | XL | Blocked | AIL-039, AIL-040, AIL-041, AIL-042, AIL-043, AIL-044, AIL-045, AIL-046, AIL-047, AIL-048, AIL-049, AIL-050 |
| [AIL-052](cards/AIL-052.md) | 统一写入保护：公司文件、所有权与路径边界 | ALIGN | M | Backlog | — |
| [AIL-053](cards/AIL-053.md) | 撤销校验 after-image 并保留用户后改 | ALIGN | M | Backlog | AIL-052 |
| [AIL-054](cards/AIL-054.md) | 文档导出绑定授权根、预览与目标版本 | ALIGN | M | Backlog | AIL-052 |
| [AIL-055](cards/AIL-055.md) | 配置目标显式贯穿 CLI、API 与任务 | ALIGN | M | Backlog | — |
| [AIL-056](cards/AIL-056.md) | 仓库搬迁、工作树重关联与稳定身份 | ALIGN | M | Backlog | AIL-055 |
| [AIL-057](cards/AIL-057.md) | 非 Git 文件夹完成配置到应用闭环 | ALIGN | M | Backlog | AIL-055 |
| [AIL-058](cards/AIL-058.md) | 三态继承与单项配置合并 | ALIGN | M | Backlog | AIL-055 |
| [AIL-059](cards/AIL-059.md) | 父子作用域统一部署计划与托管归属 | ALIGN | L | Backlog | AIL-052, AIL-055, AIL-058 |
| [AIL-060](cards/AIL-060.md) | 个人配置并发保存、无损编辑与迁移 | ALIGN | M | Backlog | AIL-055, AIL-058 |
| [AIL-061](cards/AIL-061.md) | 最终宿主与资源开关约束实际部署 | ALIGN | M | Backlog | AIL-058, AIL-059 |
| [AIL-062](cards/AIL-062.md) | 个人资源库事务导入、编辑校验与错误恢复 | ALIGN | M | Backlog | — |
| [AIL-063](cards/AIL-063.md) | Skill 来源身份、版本与旧数据迁移 | ALIGN | M | Backlog | AIL-062 |
| [AIL-064](cards/AIL-064.md) | 从 GitHub 仓库或子目录预览导入 Skill | ALIGN | L | Backlog | AIL-062, AIL-063 |
| [AIL-065](cards/AIL-065.md) | skill.sh 发现入口解析与真实来源导入 | ALIGN | M | Backlog | AIL-063, AIL-064 |
| [AIL-066](cards/AIL-066.md) | Skill 上游检查、差异更新与本地冲突保护 | ALIGN | L | Backlog | AIL-063, AIL-064 |
| [AIL-067](cards/AIL-067.md) | 资源更新到工作树与宿主的同步闭环 | ALIGN | M | Backlog | AIL-059, AIL-061, AIL-066 |
| [AIL-068](cards/AIL-068.md) | 网页来源管理、导入、更新与订阅影响预览 | ALIGN | L | Backlog | AIL-064, AIL-065, AIL-066, AIL-067, AIL-060, AIL-095 |
| [AIL-069](cards/AIL-069.md) | MCP 缺失引用诊断、编辑边界与显式探测 | ALIGN | M | Backlog | AIL-060, AIL-061, AIL-062 |
| [AIL-070](cards/AIL-070.md) | 流程产物稳定 ID、版本冲突与下游复核 | ALIGN | M | Backlog | AIL-060, AIL-062 |
| [AIL-071](cards/AIL-071.md) | 流程包实际 Skill 绑定与文档管理页面 | ALIGN | M | Backlog | AIL-054, AIL-068, AIL-070, AIL-096 |
| [AIL-072](cards/AIL-072.md) | 持久任务恢复、幂等与计划失效检查 | ALIGN | M | Backlog | AIL-052, AIL-053, AIL-055, AIL-060 |
| [AIL-073](cards/AIL-073.md) | 真实仓库工作树选择器与目标切换失效 | ALIGN | M | Backlog | AIL-055, AIL-056, AIL-057, AIL-058, AIL-059, AIL-094 |
| [AIL-074](cards/AIL-074.md) | 六步 Onboarding 与能力保存真实闭环 | ALIGN | L | Backlog | AIL-061, AIL-062, AIL-069, AIL-072, AIL-073, AIL-075, AIL-093 |
| [AIL-075](cards/AIL-075.md) | 草稿冲突保留与页面错误恢复 | ALIGN | M | Backlog | AIL-060, AIL-073 |
| [AIL-076](cards/AIL-076.md) | 个人指令补充与替代视图的基线更新 | ALIGN | M | Backlog | AIL-052, AIL-053, AIL-059, AIL-061 |
| [AIL-077](cards/AIL-077.md) | 真实宿主 Skills、MCP 与个人指令验收 | ALIGN | M | Backlog | AIL-067, AIL-069, AIL-076 |
| [AIL-078](cards/AIL-078.md) | 真实浏览器与破坏性反例端到端回归 | ALIGN | L | Backlog | AIL-052, AIL-053, AIL-054, AIL-055, AIL-056, AIL-057, AIL-058, AIL-059, AIL-060, AIL-061, AIL-062, AIL-063, AIL-064, AIL-065, AIL-066, AIL-067, AIL-068, AIL-069, AIL-070, AIL-071, AIL-072, AIL-073, AIL-074, AIL-075, AIL-076, AIL-098 |
| [AIL-079](cards/AIL-079.md) | 需求覆盖复审、文档与最终验收交接 | ALIGN | M | Backlog | AIL-077, AIL-078 |
| [AIL-080](cards/AIL-080.md) | Web 工程拆分与页面/API 契约清单 | WEB | M | Backlog | — |
| [AIL-081](cards/AIL-081.md) | 设计 Token、排版与基础布局规范 | WEB | M | Backlog | AIL-080 |
| [AIL-082](cards/AIL-082.md) | 基础交互组件：按钮、表单与三态开关 | WEB | M | Backlog | AIL-081 |
| [AIL-083](cards/AIL-083.md) | 展示组件：状态、列表、表格与页面反馈 | WEB | M | Backlog | AIL-081, AIL-082 |
| [AIL-084](cards/AIL-084.md) | 对话框、差异预览与冲突编辑组件 | WEB | M | Backlog | AIL-082, AIL-083 |
| [AIL-085](cards/AIL-085.md) | 统一 API 客户端与错误/取消协议 | WEB | M | Backlog | AIL-080 |
| [AIL-086](cards/AIL-086.md) | 页面状态容器、目标版本与草稿状态机 | WEB | M | Backlog | AIL-085 |
| [AIL-087](cards/AIL-087.md) | 应用壳、路由与跨页恢复 | WEB | M | Backlog | AIL-081, AIL-083, AIL-086 |
| [AIL-088](cards/AIL-088.md) | 业务组件：仓库/工作树/子项目选择器 | WEB | M | Backlog | AIL-082, AIL-083, AIL-085, AIL-086, AIL-055, AIL-056, AIL-057 |
| [AIL-089](cards/AIL-089.md) | 业务组件：宿主能力矩阵与继承编辑 | WEB | M | Backlog | AIL-082, AIL-083, AIL-085, AIL-086, AIL-058, AIL-061, AIL-069 |
| [AIL-090](cards/AIL-090.md) | 业务组件：来源输入、导入预览与更新冲突 | WEB | M | Backlog | AIL-082, AIL-083, AIL-084, AIL-085, AIL-086, AIL-064, AIL-065, AIL-066, AIL-067 |
| [AIL-091](cards/AIL-091.md) | 业务组件：流程阶段与文档关联编辑 | WEB | M | Backlog | AIL-082, AIL-083, AIL-084, AIL-085, AIL-086, AIL-070 |
| [AIL-092](cards/AIL-092.md) | 业务组件：计划、任务进度与验证结果 | WEB | M | Backlog | AIL-083, AIL-084, AIL-085, AIL-086, AIL-053, AIL-054, AIL-067, AIL-072 |
| [AIL-093](cards/AIL-093.md) | 页面组装：六步首次设置向导 | WEB | M | Backlog | AIL-087, AIL-088, AIL-089, AIL-090, AIL-092, AIL-075 |
| [AIL-094](cards/AIL-094.md) | 页面组装：仓库与作用域日常配置 | WEB | M | Backlog | AIL-087, AIL-088, AIL-089, AIL-092, AIL-060 |
| [AIL-095](cards/AIL-095.md) | 页面组装：资源库、来源订阅与更新部署 | WEB | M | Backlog | AIL-087, AIL-090, AIL-092 |
| [AIL-096](cards/AIL-096.md) | 页面组装：流程与文档工作台 | WEB | M | Backlog | AIL-087, AIL-091, AIL-092, AIL-054 |
| [AIL-097](cards/AIL-097.md) | 页面组装：个人指令与宿主验证设置 | WEB | M | Backlog | AIL-087, AIL-088, AIL-084, AIL-092, AIL-076 |
| [AIL-098](cards/AIL-098.md) | 组件样例与所有页面浏览器质量关卡 | WEB | M | Backlog | AIL-093, AIL-094, AIL-095, AIL-096, AIL-097 |
