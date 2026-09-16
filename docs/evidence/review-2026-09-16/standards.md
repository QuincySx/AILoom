# Standards 轴审查：c4a56a4...edc48bd

审查日期：2026-09-16。重点为本地控制台 HTTP 写接口、个人指令保护、plan/apply/undo 与授权路径边界。以下四项均有临时目录中的真实 CLI/HTTP 证据；未修改业务代码、卡片或用户实际配置。无需要独立报告的判断性风格气味。

证据脚本与日志：
- `/tmp/ailoom-standards-repro-20260916.py`
- `/tmp/ailoom-standards-repro-20260916.log`
- `/tmp/ailoom-standards-restart-20260916.py`
- `/tmp/ailoom-standards-restart-20260916.log`
- 夹具：`/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/ailoom-standards-20260916-4fzllejs`

环境：现有 `target/debug/ailoom` 真实入口；HOME、AILOOM_DATA_ROOT、AILOOM_STORE_ROOT 指向夹具，两个 XDG 变量置空，Git 全局配置指定 `/dev/null`。所有 Git 操作仅在临时仓库（git init、git add），服务通过 console --no-open 启动并由 /api/shutdown 正常关闭。未运行真实宿主、连接 MCP 或读取实际秘密。

## S01 — P1：Undo 无条件删除或覆盖应用后的用户修改

定位：`src/console/jobs.rs:562–611`，尤其 600–611；相关结构 `UndoEntry` 仅存 before-image，未存部署后的 hash/type。

规范：`docs/cards/AIL-050.md:31` 明确“撤销只恢复本次托管改动，遇到用户后改冲突不覆盖”；`docs/CONTRACTS.md §6` 要求保留用户修改、恢复保护失败后的人工修改；`docs/initiatives/local-console.md` 本地 Web 协议及公司文件保护要求任务撤销可恢复、不误报全部回滚。

触发与复现：
1. 临时 Git 仓登记、保存个人指令，启用 codex。
2. HTTP `/api/jobs/plan` → `/api/jobs/apply` 成功，生成 `AGENTS.override.md`。
3. 手工将该文件改成 `USER POST APPLY EDIT\n`。
4. POST `/api/jobs/undo`，body 为该 apply job id。

实际：HTTP 200，`conflicts: []`、`restored: ["AGENTS.override.md"]`、`note: "本任务写入项已全部回滚"`；用户编辑文件被删除，`target_exists False`。

原因：代码注释声称比较期望，但实际仅 symlink_metadata 判断存在性。对于已有目标，无条件写 previous；对于新建目标，无条件删除。若用户把文件替换成目录，609 行甚至递归删除目录（此扩展场景为静态证据，未另行执行）。Undo 同时绕过统一同步器的 ownership/hash/lock/company guard，不能依赖保存前像证明当前文件仍属于本任务。

建议：持久化每个实际成功动作的 before/after 类型与指纹，撤销前校验当前内容与 after-image 一致，并复用锁/安全路径/公司文件守卫；冲突应保留并报告。

## S02 — P1：公司文件守卫过滤后，旧托管文件反而被当作过时目标删除

定位：`src/commands/personal.rs:253–255,285–295`；`src/sync/plan.rs:351` 构造 obsolete Delete。

规范：`docs/initiatives/local-console.md`“公司文件保护与本地指令”明确公司已跟踪 AGENTS/CLAUDE/宿主配置及 index 不被个人 apply/sync/uninstall 改写，既有 staged/unstaged 内容保持原样。`docs/IMPLEMENTATION-GUIDE.md` 要求真实错误路径验收，不能仅以函数存在为证据。

触发与复现：
1. 个人 sync 正常部署 `AGENTS.override.md`（有 managed manifest）。
2. 在临时仓 `git add -f AGENTS.override.md`，模拟该入口被公司跟踪或分支变更后成为跟踪文件；内容保持原部署字节。
3. 执行 `ailoom personal --action plan`，随后 `--action sync`。

实际 plan 同时返回：
- `skipped: [{path: "AGENTS.override.md", reason: "公司已跟踪文件，个人模式不写入（保持公司内容与暂存区原样）"}]`
- `actions: [{action: "delete", path: "AGENTS.override.md", reason: "源不再需要该目标，计划清理"}]`
- `has_conflicts: false`

sync 退出0、`ok:true`、`applied:["AGENTS.override.md"]`，实际文件消失。Git index 字节未变（证据 `index_unchanged True`），但跟踪文件工作树内容已被删除，产生用户未授权删除修改。

原因：守卫仅过滤 desired artifacts，随后 build_plan 仍消费完整旧 managed 清单，将被保护路径视为不再需要。必须把保护规则用于全部计划动作（包括删除、恢复与撤销），保护文件不能通过“从期望集合移除”实现。

## S03 — P1：Workflow 导出绕过授权根并覆盖现有用户文件

定位：`src/console/mod.rs:552–580`，尤其直接把 `Path::new(target)` 传到 export_execute；`src/workflow.rs:365–379`。

规范：`docs/initiatives/local-console.md`“本地 Web 写入协议”要求请求使用注册 ID/ScopeId、后端重新校验路径边界，目录访问限用户授权根；“Skill、MCP、源和文档”要求明确导出时预览路径和 diff。服务器模块顶部亦声明 canonicalize 后校验边界、拒绝符号链接逃逸。

触发与复现：
1. 只向 `/api/fs/approve` 授权 `<fixture>/repo`。
2. 创建流程及 spec 产物，内容为 `EXPORTED OUTSIDE ROOT`。
3. 在授权根外 sibling `<fixture>/not-approved/victim.txt` 写入 `USER FILE`。
4. POST `/api/workflows/export`，携带合法会话 token，body 中 target 为上述文件绝对路径、execute=true；未进行导出 preview，也未批准 sibling 目录。

实际：HTTP 200，`exported:true`，victim.txt 内容变为 `EXPORTED OUTSIDE ROOT`。

原因：该路由完全遗漏 ensure_within_roots。export_execute 只尝试 git ls-files（失败按未跟踪继续），然后直接 atomic_write；用户未跟踪原内容不受冲突/指纹保护。影响是受限本地 API 实际可写任意用户可写路径，且页面确认没有绑定预览或目标版本。修复应校验规范化目标父路径与授权根，拒绝跨根/别名逃逸；已有文件覆盖须用预览确认的指纹约束。

## S04 — P2：服务重启后任务只剩磁盘详情，无法继续操作

定位：`src/console/mod.rs:58–65` 初始化空 jobs；`src/console/jobs.rs::spawn_apply` 仅查内存 map；undo 同样在 543–548 仅查内存。

规范：`docs/initiatives/local-console.md` 首次流程要求“服务/浏览器重启可恢复未完成状态但不能自动重放执行动作”；本地 Web 协议要求任务有 ID、幂等键及恢复能力。AIL-050 任务链路承担该职责。

真实复现：上个服务已生成并持久化成功 plan，正常 shutdown；用同一 data_root 重启控制台，携新会话 token：
- GET `/api/jobs/<old-plan-id>` → 200（磁盘 job 存在）。
- GET `/api/jobs` → 200 `{jobs: []}`。
- POST `/api/jobs/apply` 引用该 plan → 400 `{error: "计划任务不存在"}`。

原因：启动不载入持久化任务，单条 GET 从磁盘读取，而列表、apply、undo、cancel 和幂等检查依赖空内存 map。成功 apply 的撤销也受相同路径影响（本轮额外命令只复现旧 plan apply；undo 丢失为直接共享原因）。恢复应载入并规范化任务状态，保留成功任务/幂等键；中断任务显式转为可恢复状态，不自动重放。

## 转交 Spec 轴合并的静态线索（不并入上述四项计数）

1. `/api/profile/select` 未传 repo_id/root，`personal::select` 取 `profile.repos.keys().last()`；instructions 路由使用服务器 cwd。多仓/多工作树操作可能写错配置作用域。已通知主审，建议与其发现去重。
2. 代码和测试里大量“plan 纯只读”注释并不等价真正无写：prepare_personal 写 registry，也可能 ensure_library。但这里未作为独立缺陷，避免把合理机器状态记录与产品写入混淆。
