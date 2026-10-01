# Web 控制台详细实施蓝图（2026-09-17）

这是可执行的前端拆分规格，不是另建产品。用户要求先做好 Token、封装组件，再用清楚的方式组装。现状是 src/console/web.rs 内一个 Rust format! 字符串包含 CSS、HTML、全局 draft、fetch、三页函数和 inline onclick。复审已发现 U01取消勾选无效、U02切目录仍沿用旧计划、U03吞409、U04列表不是真实选择器。

## 实施方式与目录边界

沿用 Rust 本地服务与单二进制离线分发；采用原生 ES modules、CSS variables 和小型 DOM 组件，不为这轮默认引入 React/Vue、Node运行时、CDN或大型组件库。组件化并不要求换框架。080负责将静态资源按固定映射 include_str!/等价嵌入并正确返回 MIME；禁止把任意磁盘路径映射成静态文件。API、认证和受限写入口继续走现有 Rust 服务。

目标目录（职责约定，具体文件后缀/分组可在080核对后明确；不能换职责）：

- src/console/web.rs：HTML入口与安全 bootstrap，移除具体业务页面。
- src/console/ui/tokens.css、base.css：设计变量与全局排版/布局。
- src/console/ui/components/：纯展示/交互组件，无 fetch、无业务全局变量。
- src/console/ui/services/：唯一网络入口与领域API方法。
- src/console/ui/state/：目标、草稿、选择、计划、任务状态及转换。
- src/console/ui/features/：业务组件，接领域数据/服务，发明确业务事件。
- src/console/ui/pages/：组装业务组件、绑定状态与路由。
- src/console/ui/app.js：bootstrap、路由、生命周期和应用壳。

080先移出样式和最小模块启动，再按基础→业务→页面替换旧函数。迁移期间旧路由可保持工作，但每个动作只能有一条生效链路；替换后移除对应旧 handler 与全局状态，禁止两个实现各维护 draft。新增第三方依赖必须证明本卡需要并遵守依赖审查，不因搭组件而批量安装框架。

## 每一层的交付与禁区

| 层 | 卡 | 交付 | 不允许 |
|---|---|---|---|
| 模块与契约 | 080 | 可加载模块、迁移/API动作表 | 只有目录和 TODO |
| 设计 Token | 081 | semantic CSS variables、排版布局样例 | 每页自行配色/间距 |
| 基础组件 | 082–084 | 控件、表格、反馈、弹层、编辑/diff | 组件直接改profile或fetch |
| 数据/状态 | 085–086 | API错误协议、显式目标、草稿/任务转换 | 多份全局draft、静默吞错 |
| 壳与导航 | 087 | 路由、TargetBar、连接状态、恢复 | 换页冒充切业务目标 |
| 业务组件 | 088–092 | 选择器、能力、来源、流程、任务 | 另造通用控件/同步器 |
| 页面组装 | 093–097 | 真实可用的五类页面流程 | 假成功按钮、生产mock回退 |
| 浏览器质量 | 098 | 状态样例、响应式、键盘与真实操作证据 | 静态截图代替交互 |

## 组件统一接口

统一组件工厂接收 container 与 props，返回 update(nextProps)、destroy()。props 是可渲染的数据快照，onXxx 是动作回调；业务数据通过 state/services 提供，基础组件不知会话 token 或 API URL。destroy 必须注销监听、取消自身请求/订阅、清理计时器；服务端正在执行的任务不因页面销毁而假称取消。

| 组件 | 关键输入 | 输出事件 | 必须状态/行为 |
|---|---|---|---|
| Button | label,variant,disabled,pending | onPress | 防重复、可聚焦、pending文字 |
| Field/Input | label,value,error,hint | onChange,onBlur | 保光标、错误关联、长值 |
| TriStateSelect | value,inheritedValue | onChange(inherit/enable/disable) | 当前层与effective分开 |
| DataTable | rows,rowKey,columns,selection | onSelect,onSort | 空/加载/失败、稳定键 |
| StatusBadge | domain,status,label | 无 | 部署不等于调用成功 |
| ScopeSummary | target,displayNames,status | onInspect | 长路径、失联、非Git |
| Dialog/Drawer | open,title,actions,dirty | onClose/onAction | 焦点约束与恢复 |
| Editor | documentId,content,baseRevision,validation | onChange,onSave | 不丢输入/版本 |
| DiffView | before,after,path,operation | onExpand | 路径、冲突、长文 |
| ConflictPanel | local,base,server,currentRevision | onReload,onMerge | 不自动覆盖 |
| Repository/Worktree/ScopePicker | registry,target,loading,error | onTargetSelected | 不自行保存或apply |
| CapabilityMatrix | target,selections,effective,support | onPatch,onSave | 单项三态与来源 |
| ImportPreview | source,candidates,conflicts,revision | onSelect,onImport | 导入不执行脚本 |
| UpdatePanel | base,local,upstream,affectedTargets | onCheck,onResolve,onUpdate,onPlan | 库与部署分别显示 |
| WorkflowStageList | workflow,bindings,artifacts | onBind,onOpenArtifact | 缺项、需复核 |
| PlanPreview | plan,target,versions,stale | onRegenerate,onApply | 无变化禁执行 |
| JobPanel | job,status,progress,connection | onRetryRead,onCancel,onUndo | 中断/部分失败/恢复 |
| VerificationPanel | saved,deployed,discovered,invoked | onExplicitProbe | 需新会话/未知/不支持 |

实现时各模块用 JSDoc或等价明确定义字段类型与可选值，并在080契约表列后端实际对应字段。上述是前端 view model，不声称现有 API 已提供所有字段；缺字段回到责任后端卡补接口，不伪造。

## 数据流与状态转换

用户操作 → 组件 onXxx → 页面/feature action → 统一 services → Rust API → 结果归一化 → 状态更新 → 组件 update。基础组件不能越过这条路径直接发网络请求；页面禁止直接 fetch。后端仍是文件写入、权限和版本校验的最终执行者，前端禁用按钮不是保护边界。

TargetContext 包含 repoId/worktreeId/scopeId/kind/root/generation。所有修改动作带目标，所有异步结果带发起时generation。选A后立刻选B，即使A响应更晚也必须丢弃A响应。

| 事件 | 保留 | 失效/清除 | 后续允许动作 |
|---|---|---|---|
| 改目录/目标 | 原目标独立草稿、已运行任务历史 | 当前effective、能力已保存标记、plan/apply展示关联 | 重新发现并加载 |
| 修改能力 | 当前编辑值 | 旧plan有效性 | 保存配置 |
| 保存成功 | 服务端revision/effective | 旧plan | 生成预览 |
| 保存409 | 本地草稿与base、服务端版本 | 成功提示、盲目重试资格 | 显式比较/合并/重载 |
| 上游/库版本变化 | 用户选择与旧任务证据 | 旧plan | 重新预览 |
| 服务断线 | 草稿、jobId、target | 连接状态 | 重连查询，禁止自动POST |
| 切页 | 按目标隔离草稿、服务端任务 | 本页监听/轮询/请求 | mount新页 |
| apply部分失败 | journal、已完成/未完成列表 | 全部成功状态 | 恢复/冲突处理 |
| undo遇后改 | 用户内容与冲突列表 | 全部回滚声明 | 展示部分结果 |

ApiError分别处理冲突、校验、鉴权、断线、超时、取消；所有错误必须有可见反馈，不能 catch 后空处理。会话 token 只存在必要的内存/bootstrap边界，不写 localStorage、业务日志、资源文档或证据。

## 页面装配清单（不允许一张“做后台”包办）

| 页面与路由建议 | 先装哪些组件 | 读请求/状态 | 写动作 | 本轮卡 |
|---|---|---|---|---|
| #/overview | AppShell、ScopeSummary、任务摘要、EmptyState | 最近合法目标、现有任务/验证状态 | 仅导航到具体动作 | 087 |
| #/onboarding | Stepper、ScopePicker、CapabilityMatrix、ImportPreview、PlanPreview、JobPanel、VerificationPanel | draft/server-info/registry/effective/job | 授权、选择保存、计划、应用、显式探针 | 093+074 |
| #/scopes | ScopePicker、InheritanceView、CapabilityMatrix、PlanPreview | 目标/effective/revision | 单项patch、保存、恢复继承、重关联 | 094+073 |
| #/library 与资源详情 | DataTable、SourceBadge、Editor、UpdatePanel | 资源、来源、版本、引用目标 | 导入、保存、更新库、选择部署 | 095+068 |
| #/sources | SourceInput、订阅列表、ImpactPreview、ConfirmAction | 订阅/ref/cache/影响 | 启停、固定版本、个人副本、明确删除 | 095+068 |
| #/workflows 与具体流程 | StageList、SkillBindingPicker、ArtifactList/Editor、Diff/Conflict | 流程ID、绑定、产物revision | 导入、绑定、保存、重命名、复核、导出 | 096+071 |
| #/tasks 与具体任务 | JobPanel、ScopeSummary、UndoPanel、VerificationPanel | jobId/原始target/恢复状态 | 安全取消、明确恢复/撤销/探针 | 087+092 |
| #/instructions | ScopeSummary、模式选择、只读基线、Editor、Diff/Conflict | 基线摘要、个人patch、支持方式 | 个人保存、预览应用、显式验证 | 097+076 |

路由方案在080落实为精确路径表，不强制此处字符串完全不变。目标ID、资源ID必须编码，不能直接拼未转义路径。不存在资源显示可恢复404，不回退其他资源。内部实现ID放详情，用户主流程显示名称/分支/路径。

首次向导逐步装配：选择目录→确认归属→选择宿主/资源→预览→应用→验证。只有当前步骤展开编辑，已完成步骤为摘要可返回。第3步保存不调用apply；第4步无变更无需创建空任务；第5步只执行当前绑定的有效计划；第6步未真实调用不能标为全部可用。

## API 动作冻结方式

080列出现有路由与调用点，085按实际 Rust返回归一化为前端模型；055/060/063/070/072各自负责目标、revision、来源、产物、任务契约。每个动作表必须填写：HTTP方法/路径、参数、目标字段、版本前提、返回、错误码、写文件/进程副作用、负责卡、实际测试。

当前已知入口包括 /api/draft、/api/fs/approve、/api/repo/discover、/api/hosts/detect、/api/profile/select、/api/library/import、/api/library/list、/api/jobs/plan、/api/jobs/apply、/api/jobs/:id。新增更新/来源/流程动作按现有Rust实现核对再冻结，不能把这里的页面名称当作存在的API。

同一个端到端动作的分工：后端卡提供真实约束；业务组件卡提供输入/状态/事件；页面卡连接真实服务；对应业务完整性卡复跑旧反例；078/098验证组合结果。不要五张卡各复制一套处理函数。

## 状态样例与验收

每个可复用组件至少有正常、加载、空、失败、禁用、长内容样例；涉及版本和更新的增加冲突、离线、过期、部分失败。组件样例可用fixture，生产页面禁止假数据回退。先验收组件，再页面，最后真实业务链路。

设计Token和基础组件需1440/1024/768/390宽度截图及键盘行为；页面需真实API成功与失败链路。涉及409至少两个浏览器上下文；切目标需要故意乱序返回；服务恢复需要真实进程重启；宿主加载必须交077。截图、API断言、真实宿主调用各证明不同层，不能相互替代。

每张 Web 卡交接必须包含：导出组件/方法、props/事件定义、状态样例路径、真实服务连接点、对应验收步骤、未完成API依赖。没有这些材料，下游卡不能以“组件大致做完了”为依据关卡。

