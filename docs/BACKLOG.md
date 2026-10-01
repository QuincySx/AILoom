# AILoom 任务看板

> [cards.json](cards.json) 是卡片状态的唯一来源。下方总表由 `python3 scripts/docs_check.py --write` 生成，禁止手改；改状态时同时改 cards.json 与卡头，再运行该命令。

## 当前执行入口

[2026-10 稳定与收敛迭代](initiatives/iteration-2026-10.md)：AIL-129～147，输入是 [2026-10-01 项目审查报告](reviews/2026-10-01-project-review.md)。本轮不加新功能，先让门禁、关卡与契约可信。

- 待办中的旧卡：AIL-116（MCP 项目参数与凭据）、AIL-117（统一 Markdown 指令）在 AIL-142 宿主注册表就绪后实施。
- 长期 Blocked：AIL-009、012、014、051，解除条件见各卡。
- 审查发现 85 张 Done 卡的必需验收项未勾选，由 AIL-131 逐卡复核；复核完成前，这些卡的 Done 只代表「实现者已交付」。

历史轮次入口（09-16 本地控制台、09-17 实现对齐与 Web 蓝图、09-18 UI 走查、09-21 目录优先）已移入 [归档](archive/README.md)。

## 卡片总表

<!-- cards:begin -->
**149 张主卡：7 Backlog、19 In progress、5 Blocked、106 Done、12 Superseded。**（由 `scripts/docs_check.py --write` 从 cards.json 生成）

| 卡片 | 任务 | 里程碑 | 优先级 | 规模 | 状态 | 前置 |
|---|---|---|---|---|---|---|
| [AIL-001](cards/AIL-001.md) | 冻结初版领域与配置契约 | M0 | — | M | Done | — |
| [AIL-002](cards/AIL-002.md) | 建立 Rust CLI 与工程基础 | M0 | — | S | Done | AIL-001 |
| [AIL-003](cards/AIL-003.md) | 工作区识别与机器数据分区 | M1 | — | L | Done | AIL-001, AIL-002 |
| [AIL-004](cards/AIL-004.md) | Git 资源源、缓存与版本锁定 | M1 | — | M | Done | AIL-002, AIL-003 |
| [AIL-005](cards/AIL-005.md) | 项目绑定与 init/status 命令 | M1 | — | M | Done | AIL-001, AIL-003, AIL-004 |
| [AIL-006](cards/AIL-006.md) | 资源清单与项目角色解析器 | M1 | — | L | Done | AIL-001, AIL-004, AIL-005 |
| [AIL-007](cards/AIL-007.md) | 同步计划、托管清单与差异预览 | M1 | — | L | Done | AIL-006 |
| [AIL-008](cards/AIL-008.md) | 同步执行、锁与失败恢复 | M1 | — | L | Done | AIL-007 |
| [AIL-009](cards/AIL-009.md) | Skills 适配：Claude Code 与 Codex | M1 | — | M | Blocked | AIL-006, AIL-007, AIL-008 |
| [AIL-010](cards/AIL-010.md) | 规则与项目文档适配 | M1 | — | M | Done | AIL-007, AIL-008, AIL-009 |
| [AIL-011](cards/AIL-011.md) | 专用 Agent 统一定义与适配 | M1 | — | M | Done | AIL-006, AIL-007, AIL-008 |
| [AIL-012](cards/AIL-012.md) | MCP 定义、适配与托管更新 | M1 | — | L | Blocked | AIL-006, AIL-007, AIL-008 |
| [AIL-013](cards/AIL-013.md) | doctor 与安全卸载 | M1 | — | M | Done | AIL-009, AIL-010, AIL-011, AIL-012 |
| [AIL-014](cards/AIL-014.md) | 资源贡献与审核发布 | M2 | — | L | Blocked | AIL-004, AIL-006, AIL-007, AIL-008 |
| [AIL-015](cards/AIL-015.md) | 经验文档与项目归属 | M2 | — | M | Done | AIL-001, AIL-005, AIL-006, AIL-014 |
| [AIL-016](cards/AIL-016.md) | 知识索引与隔离召回 | M2 | — | L | Done | AIL-006, AIL-015 |
| [AIL-017](cards/AIL-017.md) | 召回 Agent 与经验总结 Skill | M2 | — | M | Done | AIL-009, AIL-011, AIL-015, AIL-016 |
| [AIL-018](cards/AIL-018.md) | Hook 生命周期与事件协议 | M3 | — | L | Done | AIL-003, AIL-005, AIL-008 |
| [AIL-019](cards/AIL-019.md) | 会话、Token 与人工干预聚合 | M3 | — | L | Done | AIL-018 |
| [AIL-020](cards/AIL-020.md) | 经验总结提示与会话摘要 | M3 | — | M | Done | AIL-015, AIL-017, AIL-019 |
| [AIL-021](cards/AIL-021.md) | 本地实时看板 | M3 | — | M | Done | AIL-019 |
| [AIL-022](cards/AIL-022.md) | 团队统计上报与 digest | M3 | — | L | Done | AIL-014, AIL-019, AIL-020 |
| [AIL-023](cards/AIL-023.md) | 初版跨项目端到端验收与文档 | M3 | — | L | Done | AIL-013, AIL-014, AIL-017, AIL-020, AIL-021, AIL-022 |
| [AIL-024](cards/AIL-024.md) | 成员名册与项目查询 | NEXT | — | M | Done | AIL-005, AIL-014 |
| [AIL-025](cards/AIL-025.md) | 多源订阅、标签和资源版本更新 | NEXT | — | L | Done | AIL-004, AIL-006, AIL-007, AIL-014 |
| [AIL-026](cards/AIL-026.md) | 代码事实与增量知识图谱 | NEXT | — | XL | Done | AIL-016 |
| [AIL-027](cards/AIL-027.md) | 图关系辅助召回 | NEXT | — | L | Done | AIL-016, AIL-026 |
| [AIL-028](cards/AIL-028.md) | 知识反馈、晋升与维护 | NEXT | — | L | Done | AIL-016, AIL-017, AIL-022 |
| [AIL-029](cards/AIL-029.md) | 安装分发与 npm 包装 | NEXT | — | M | Done | AIL-002, AIL-023 |
| [AIL-030](cards/AIL-030.md) | 管理后端、身份与审核模型设计 | LATER | — | XL | Done | AIL-001, AIL-014, AIL-022, AIL-024 |
| [AIL-031](cards/AIL-031.md) | 共享环境配置与团队文化 | NEXT | — | M | Done | AIL-006, AIL-010, AIL-012 |
| [AIL-032](cards/AIL-032.md) | 团队自定义 Hook 分发 | NEXT | — | L | Done | AIL-006, AIL-008, AIL-018 |
| [AIL-033](cards/AIL-033.md) | 团队声明的软件包与插件依赖 | NEXT | — | L | Done | AIL-004, AIL-007, AIL-009, AIL-029 |
| [AIL-034](cards/AIL-034.md) | 目录、仓库与批量知识导入 | NEXT | — | L | Done | AIL-015, AIL-016, AIL-026 |
| [AIL-035](cards/AIL-035.md) | PR/MR 经验与 CI 知识更新 | NEXT | — | L | Done | AIL-014, AIL-015, AIL-026, AIL-034 |
| [AIL-036](cards/AIL-036.md) | 业务仓库同仓资源模式与迁移 | NEXT | — | XL | Done | AIL-003, AIL-004, AIL-008, AIL-014, AIL-022 |
| [AIL-037](cards/AIL-037.md) | 事件留存、数据导出与清理 | NEXT | — | M | Done | AIL-003, AIL-018, AIL-019, AIL-022 |
| [AIL-038](cards/AIL-038.md) | 团队源脚手架与发布链路本地准备 | NEXT | — | M | Done | AIL-001, AIL-005, AIL-029 |
| [AIL-039](cards/AIL-039.md) | Git 仓库身份、工作树与子项目发现 | LOCAL | — | XL | Done | AIL-003 |
| [AIL-040](cards/AIL-040.md) | 仓外个人配置、作用域继承与兼容迁移 | LOCAL | — | XL | Done | AIL-039, AIL-001 |
| [AIL-041](cards/AIL-041.md) | 宿主能力探测与 Codex 项目加载复核 | LOCAL | — | L | Done | AIL-009, AIL-012 |
| [AIL-042](cards/AIL-042.md) | 公司指令文件保护与个人本地调整 | LOCAL | — | XL | Done | AIL-040, AIL-041 |
| [AIL-043](cards/AIL-043.md) | 个人资源库与已有 Skill 导入 | LOCAL | — | L | Done | AIL-038, AIL-040 |
| [AIL-044](cards/AIL-044.md) | 按资源与宿主选择能力及解释有效配置 | LOCAL | — | L | Done | AIL-040, AIL-041 |
| [AIL-045](cards/AIL-045.md) | Grill 到实现的流程包与文档产物关联 | LOCAL | — | L | Done | AIL-043, AIL-044, AIL-015 |
| [AIL-046](cards/AIL-046.md) | 本地 Web 控制服务与受限 API | LOCAL | — | L | Done | AIL-039, AIL-040, AIL-021 |
| [AIL-047](cards/AIL-047.md) | 个人模式 Onboarding 首次成功体验 | LOCAL | — | L | Done | AIL-042, AIL-043, AIL-044, AIL-046, AIL-050 |
| [AIL-048](cards/AIL-048.md) | 仓库工作树与子项目日常配置界面 | LOCAL | — | L | Done | AIL-039, AIL-040, AIL-044, AIL-046, AIL-050, AIL-047 |
| [AIL-049](cards/AIL-049.md) | 源、Skill、MCP 与流程文档可视化编辑 | LOCAL | — | L | Done | AIL-043, AIL-044, AIL-045, AIL-046, AIL-050, AIL-047 |
| [AIL-050](cards/AIL-050.md) | 预览应用任务、宿主验证与可恢复撤销 | LOCAL | — | XL | Done | AIL-042, AIL-044, AIL-046, AIL-008, AIL-013 |
| [AIL-051](cards/AIL-051.md) | Onboarding 与多工作树真实体验验收 | LOCAL | — | XL | Blocked | AIL-039, AIL-040, AIL-041, AIL-042, AIL-043, AIL-044, AIL-045, AIL-046, AIL-047, AIL-048, AIL-049, AIL-050 |
| [AIL-052](cards/AIL-052.md) | 统一写入保护：公司文件、所有权与路径边界 | ALIGN | — | M | Done | — |
| [AIL-053](cards/AIL-053.md) | 撤销校验 after-image 并保留用户后改 | ALIGN | — | M | Done | AIL-052 |
| [AIL-054](cards/AIL-054.md) | 文档导出绑定授权根、预览与目标版本 | ALIGN | — | M | Done | AIL-052 |
| [AIL-055](cards/AIL-055.md) | 配置目标显式贯穿 CLI、API 与任务 | ALIGN | — | M | Done | — |
| [AIL-056](cards/AIL-056.md) | 仓库搬迁、工作树重关联与稳定身份 | ALIGN | — | M | Done | AIL-055 |
| [AIL-057](cards/AIL-057.md) | 非 Git 文件夹完成配置到应用闭环 | ALIGN | — | M | Done | AIL-055 |
| [AIL-058](cards/AIL-058.md) | 三态继承与单项配置合并 | ALIGN | — | M | Done | AIL-055 |
| [AIL-059](cards/AIL-059.md) | 父子作用域统一部署计划与托管归属 | ALIGN | — | L | Done | AIL-052, AIL-055, AIL-058 |
| [AIL-060](cards/AIL-060.md) | 个人配置并发保存、无损编辑与迁移 | ALIGN | — | M | Done | AIL-055, AIL-058 |
| [AIL-061](cards/AIL-061.md) | 最终宿主与资源开关约束实际部署 | ALIGN | — | M | Done | AIL-058, AIL-059 |
| [AIL-062](cards/AIL-062.md) | 个人资源库事务导入、编辑校验与错误恢复 | ALIGN | — | M | In progress | — |
| [AIL-063](cards/AIL-063.md) | Skill 来源身份、版本与旧数据迁移 | ALIGN | — | M | Done | AIL-062 |
| [AIL-064](cards/AIL-064.md) | 从 GitHub 仓库或子目录预览导入 Skill | ALIGN | — | L | Done | AIL-062, AIL-063 |
| [AIL-065](cards/AIL-065.md) | skill.sh 发现入口解析与真实来源导入 | ALIGN | — | M | Done | AIL-063, AIL-064 |
| [AIL-066](cards/AIL-066.md) | Skill 上游检查、差异更新与本地冲突保护 | ALIGN | — | L | Done | AIL-063, AIL-064 |
| [AIL-067](cards/AIL-067.md) | 资源更新到工作树与宿主的同步闭环 | ALIGN | — | M | Done | AIL-059, AIL-061, AIL-066 |
| [AIL-068](cards/AIL-068.md) | 网页来源管理、导入、更新与订阅影响预览 | ALIGN | — | L | Superseded | AIL-064, AIL-065, AIL-066, AIL-067, AIL-060, AIL-095 |
| [AIL-069](cards/AIL-069.md) | MCP 缺失引用诊断、编辑边界与显式探测 | ALIGN | — | M | In progress | AIL-060, AIL-061, AIL-062 |
| [AIL-070](cards/AIL-070.md) | 流程产物稳定 ID、版本冲突与下游复核 | ALIGN | — | M | Done | AIL-060, AIL-062 |
| [AIL-071](cards/AIL-071.md) | 流程包实际 Skill 绑定与文档管理页面 | ALIGN | — | M | Superseded | AIL-054, AIL-068, AIL-070, AIL-096 |
| [AIL-072](cards/AIL-072.md) | 持久任务恢复、幂等与计划失效检查 | ALIGN | — | M | Done | AIL-052, AIL-053, AIL-055, AIL-060 |
| [AIL-073](cards/AIL-073.md) | 真实仓库工作树选择器与目标切换失效 | ALIGN | — | M | Superseded | AIL-055, AIL-056, AIL-057, AIL-058, AIL-059, AIL-094 |
| [AIL-074](cards/AIL-074.md) | 六步 Onboarding 与能力保存真实闭环 | ALIGN | — | L | In progress | AIL-061, AIL-062, AIL-069, AIL-072, AIL-073, AIL-075, AIL-093 |
| [AIL-075](cards/AIL-075.md) | 草稿冲突保留与页面错误恢复 | ALIGN | — | M | Done | AIL-060, AIL-073 |
| [AIL-076](cards/AIL-076.md) | 个人指令补充与替代视图的基线更新 | ALIGN | — | M | Done | AIL-052, AIL-053, AIL-059, AIL-061 |
| [AIL-077](cards/AIL-077.md) | 真实宿主 Skills、MCP 与个人指令验收 | ALIGN | — | M | Blocked | AIL-067, AIL-069, AIL-076 |
| [AIL-078](cards/AIL-078.md) | 真实浏览器与破坏性反例端到端回归 | ALIGN | — | L | Done | AIL-052, AIL-053, AIL-054, AIL-055, AIL-056, AIL-057, AIL-058, AIL-059, AIL-060, AIL-061, AIL-062, AIL-063, AIL-064, AIL-065, AIL-066, AIL-067, AIL-068, AIL-069, AIL-070, AIL-071, AIL-072, AIL-073, AIL-074, AIL-075, AIL-076, AIL-098 |
| [AIL-079](cards/AIL-079.md) | 需求覆盖复审、文档与最终验收交接 | ALIGN | — | M | In progress | AIL-077, AIL-078 |
| [AIL-080](cards/AIL-080.md) | Web 工程拆分与页面/API 契约清单 | WEB | — | M | Done | — |
| [AIL-081](cards/AIL-081.md) | 设计 Token、排版与基础布局规范 | WEB | — | M | Done | AIL-080 |
| [AIL-082](cards/AIL-082.md) | 基础交互组件：按钮、表单与三态开关 | WEB | — | M | Done | AIL-081 |
| [AIL-083](cards/AIL-083.md) | 展示组件：状态、列表、表格与页面反馈 | WEB | — | M | Done | AIL-081, AIL-082 |
| [AIL-084](cards/AIL-084.md) | 对话框、差异预览与冲突编辑组件 | WEB | — | M | Done | AIL-082, AIL-083 |
| [AIL-085](cards/AIL-085.md) | 统一 API 客户端与错误/取消协议 | WEB | — | M | Done | AIL-080 |
| [AIL-086](cards/AIL-086.md) | 页面状态容器、目标版本与草稿状态机 | WEB | — | M | Done | AIL-085 |
| [AIL-087](cards/AIL-087.md) | 应用壳、路由与跨页恢复 | WEB | — | M | Done | AIL-081, AIL-083, AIL-086 |
| [AIL-088](cards/AIL-088.md) | 业务组件：仓库/工作树/子项目选择器 | WEB | — | M | Superseded | AIL-082, AIL-083, AIL-085, AIL-086, AIL-055, AIL-056, AIL-057 |
| [AIL-089](cards/AIL-089.md) | 业务组件：宿主能力矩阵与继承编辑 | WEB | — | M | Superseded | AIL-082, AIL-083, AIL-085, AIL-086, AIL-058, AIL-061, AIL-069 |
| [AIL-090](cards/AIL-090.md) | 业务组件：来源输入、导入预览与更新冲突 | WEB | — | M | Superseded | AIL-082, AIL-083, AIL-084, AIL-085, AIL-086, AIL-064, AIL-065, AIL-066, AIL-067 |
| [AIL-091](cards/AIL-091.md) | 业务组件：流程阶段与文档关联编辑 | WEB | — | M | Superseded | AIL-082, AIL-083, AIL-084, AIL-085, AIL-086, AIL-070 |
| [AIL-092](cards/AIL-092.md) | 业务组件：计划、任务进度与验证结果 | WEB | — | M | Superseded | AIL-083, AIL-084, AIL-085, AIL-086, AIL-053, AIL-054, AIL-067, AIL-072 |
| [AIL-093](cards/AIL-093.md) | 页面组装：六步首次设置向导 | WEB | — | M | Done | AIL-087, AIL-088, AIL-089, AIL-090, AIL-092, AIL-075 |
| [AIL-094](cards/AIL-094.md) | 页面组装：仓库与作用域日常配置 | WEB | — | M | Superseded | AIL-087, AIL-088, AIL-089, AIL-092, AIL-060 |
| [AIL-095](cards/AIL-095.md) | 页面组装：资源库、来源订阅与更新部署 | WEB | — | M | Superseded | AIL-087, AIL-090, AIL-092 |
| [AIL-096](cards/AIL-096.md) | 页面组装：流程与文档工作台 | WEB | — | M | Superseded | AIL-087, AIL-091, AIL-092, AIL-054 |
| [AIL-097](cards/AIL-097.md) | 页面组装：个人指令与宿主验证设置 | WEB | — | M | Superseded | AIL-087, AIL-088, AIL-084, AIL-092, AIL-076 |
| [AIL-098](cards/AIL-098.md) | 组件样例与所有页面浏览器质量关卡 | WEB | — | M | Done | AIL-093, AIL-094, AIL-095, AIL-096, AIL-097 |
| [AIL-099](cards/AIL-099.md) | 主导航收敛、操作记录与流程工作台定位 | WEB | — | M | Done | AIL-087, AIL-096 |
| [AIL-100](cards/AIL-100.md) | 项目入口、配置范围与状态表达 | WEB-FLOW | — | M | Done | — |
| [AIL-101](cards/AIL-101.md) | 项目 Skill 添加、移除与继承闭环 | WEB-FLOW | — | M | Done | AIL-100 |
| [AIL-102](cards/AIL-102.md) | 全局来源管理与项目引用双向走查 | WEB-FLOW | — | M | Done | AIL-100, AIL-101 |
| [AIL-103](cards/AIL-103.md) | 项目宿主配置与能力限制走查 | WEB-FLOW | — | M | Done | AIL-100 |
| [AIL-104](cards/AIL-104.md) | 项目 MCP 引用、配置与安全闭环 | WEB-FLOW | — | M | In progress | AIL-100, AIL-103 |
| [AIL-105](cards/AIL-105.md) | 项目 Agent 与其他资源能力走查 | WEB-FLOW | — | M | In progress | AIL-100, AIL-103 |
| [AIL-106](cards/AIL-106.md) | 项目 Markdown 指令编辑与继承走查 | WEB-FLOW | — | M | In progress | AIL-100, AIL-103 |
| [AIL-107](cards/AIL-107.md) | 预览、应用、冲突与撤销真实闭环 | WEB-FLOW | — | M | Done | AIL-101, AIL-103, AIL-104, AIL-105, AIL-106 |
| [AIL-108](cards/AIL-108.md) | Dialog、跳转、表单与可访问性统一走查 | WEB-FLOW | — | M | In progress | AIL-100, AIL-101, AIL-102, AIL-104, AIL-106 |
| [AIL-109](cards/AIL-109.md) | 完整业务旅程端到端验收与证据归档 | WEB-FLOW | — | M | Done | AIL-099, AIL-100, AIL-101, AIL-102, AIL-103, AIL-104, AIL-105, AIL-106, AIL-107, AIL-108 |
| [AIL-110](cards/AIL-110.md) | 非 Git 知识库作为主项目的急用闭环 | USABLE-LOCAL | P0 | M | Done | — |
| [AIL-111](cards/AIL-111.md) | 扫描项目已有未托管 Skill 并与托管资源区分 | USABLE-LOCAL | P0 | M | Done | AIL-110 |
| [AIL-112](cards/AIL-112.md) | 未托管 Skill 双重确认删除与接管帮助 | USABLE-LOCAL | P1 | M | Done | AIL-111 |
| [AIL-113](cards/AIL-113.md) | 项目内搜索选择与加号导入连续流程 | USABLE-LOCAL | P0 | M | Done | AIL-110 |
| [AIL-114](cards/AIL-114.md) | 引用写入、移除、范围与应用结果正确性修复 | USABLE-LOCAL | P0 | M | Done | AIL-110, AIL-113 |
| [AIL-115](cards/AIL-115.md) | 急用版交付门禁：两个非 Git 知识库的完整演示 | USABLE-LOCAL | P0 | M | In progress | AIL-110, AIL-111, AIL-113, AIL-114 |
| [AIL-116](cards/AIL-116.md) | MCP 项目参数与安全凭据管理 | USABLE-LOCAL | P1 | M | Backlog | AIL-114 |
| [AIL-117](cards/AIL-117.md) | 统一 Markdown 指令与最新宿主能力规则 | USABLE-LOCAL | P1 | M | Backlog | AIL-114 |
| [AIL-118](cards/AIL-118.md) | 其余页面逐动作补漏与旧入口一致性 | USABLE-LOCAL | P1 | M | Done | AIL-114 |
| [AIL-119](cards/AIL-119.md) | 来源故障、符号链接与失效引用恢复 | USABLE-LOCAL | P1 | M | In progress | AIL-111, AIL-113 |
| [AIL-120](cards/AIL-120.md) | CLI 能力对齐与专项走查 | USABLE-LOCAL | — | M | Done | AIL-114, AIL-115 |
| [AIL-121](cards/AIL-121.md) | 统一目录目标与有效配置契约 | DIRECTORY-UX | P0 | M | Done | — |
| [AIL-122](cards/AIL-122.md) | 项目目录导航与选择 Dialog | DIRECTORY-UX | P0 | M | Done | AIL-121 |
| [AIL-123](cards/AIL-123.md) | 目录资源列表与添加移除 Dialog | DIRECTORY-UX | P0 | M | Done | AIL-121, AIL-122 |
| [AIL-124](cards/AIL-124.md) | 项目共享设置与 Markdown 指令范围分离 | DIRECTORY-UX | P0 | M | Done | AIL-121, AIL-122 |
| [AIL-125](cards/AIL-125.md) | 当前目标预览应用与结果闭环 | DIRECTORY-UX | P0 | M | In progress | AIL-121, AIL-123, AIL-124 |
| [AIL-126](cards/AIL-126.md) | 资源中心按真实来源分组与引用 Dialog | DIRECTORY-UX | P1 | M | In progress | AIL-123 |
| [AIL-127](cards/AIL-127.md) | 旧入口衔接与统一交互收口 | DIRECTORY-UX | P1 | M | In progress | AIL-122, AIL-123, AIL-124, AIL-125, AIL-126 |
| [AIL-128](cards/AIL-128.md) | 按线稿完成真实浏览器与文件验收 | DIRECTORY-UX | P0 | M | In progress | AIL-121, AIL-122, AIL-123, AIL-124, AIL-125, AIL-126, AIL-127 |
| [AIL-129](cards/AIL-129.md) | 全量审查首批修复：删除加固、同步互删与契约门禁 | IT-0 首批修复 | P0 | L | Done | — |
| [AIL-130](cards/AIL-130.md) | CI 门禁补齐：MSRV、前端测试与文档检查 | IT-1 门禁与关卡 | P0 | M | In progress | AIL-129 |
| [AIL-131](cards/AIL-131.md) | 关卡纪律复核：Done 卡验收项与证据对齐 | IT-1 门禁与关卡 | P0 | L | In progress | — |
| [AIL-132](cards/AIL-132.md) | 控制台 API 健壮性与统一错误结构 | IT-1 门禁与关卡 | P1 | M | Done | AIL-129 |
| [AIL-133](cards/AIL-133.md) | 未覆盖 API 路由与跨模块组合场景测试 | IT-1 门禁与关卡 | P1 | M | Done | AIL-132 |
| [AIL-134](cards/AIL-134.md) | CLI 输出契约统一 | IT-2 CLI 一致性 | P1 | M | Done | AIL-129 |
| [AIL-135](cards/AIL-135.md) | CLI 动作与参数枚举化 | IT-2 CLI 一致性 | P1 | M | In progress | AIL-134 |
| [AIL-136](cards/AIL-136.md) | 错误码治理与码表一致性测试 | IT-2 CLI 一致性 | P2 | M | Done | AIL-134 |
| [AIL-137](cards/AIL-137.md) | Web 入口收敛：console、web 与 dashboard | IT-2 CLI 一致性 | P2 | S | Done | AIL-135 |
| [AIL-138](cards/AIL-138.md) | 前端死代码与孤儿路由清理 | IT-3 UI 收敛 | P1 | M | Done | AIL-130 |
| [AIL-139](cards/AIL-139.md) | 设计系统对齐与窄屏导航 | IT-3 UI 收敛 | P1 | M | In progress | AIL-138 |
| [AIL-140](cards/AIL-140.md) | 术语统一与空状态、错误状态补齐 | IT-3 UI 收敛 | P2 | M | Done | AIL-138 |
| [AIL-141](cards/AIL-141.md) | 范围追认：原生文件、网页服务、知识库迁移、附加宿主与 ORCA | IT-4 架构与范围 | P0 | L | Backlog | — |
| [AIL-142](cards/AIL-142.md) | 宿主能力事实统一与单一宿主注册表 | IT-4 架构与范围 | P1 | L | Backlog | AIL-141 |
| [AIL-143](cards/AIL-143.md) | 控制台后端按领域拆分 | IT-4 架构与范围 | P2 | L | Done | AIL-133 |
| [AIL-144](cards/AIL-144.md) | 领域类型收紧 | IT-4 架构与范围 | P2 | M | Backlog | AIL-143 |
| [AIL-145](cards/AIL-145.md) | 产品文档按现状重写 | IT-5 文档与发布 | P1 | M | In progress | AIL-137, AIL-142 |
| [AIL-146](cards/AIL-146.md) | 文档结构第二阶段与证据瘦身 | IT-5 文档与发布 | P2 | M | Backlog | AIL-145 |
| [AIL-147](cards/AIL-147.md) | 发布准备：安装脚本、分发地址与发布清单复核 | IT-5 文档与发布 | P2 | M | Backlog | AIL-130, AIL-145 |
| [AIL-148](cards/AIL-148.md) | 并发 sync 误把进行中的 journal 当作崩溃遗留 | IT-1 门禁与关卡 | P1 | S | Done | — |
| [AIL-149](cards/AIL-149.md) | 知识库默认托管位置与迁移：范围追认与验收 | IT-4 架构与范围 | P1 | M | Done | AIL-141 |
<!-- cards:end -->

## 后续范围：统一插件管理

用户于 2026-09-24 确认，本阶段只适配官方 Agent / Subagent 配置，Pi 使用官方 subagent 示例。插件的安装、升级、版本兼容、加载验证、卸载以及第三方实现，留给独立的插件管理功能，届时适用于所有 AI 工具；当前不实施。详见 [范围记录](reviews/2026-09-24-official-agent-scope.md)。
