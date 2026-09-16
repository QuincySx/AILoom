# 2026-09-16 Agent 实现复审

结论：本轮不能按“本地控制台主体已完成”验收。已确认存在用户内容丢失、公司跟踪文件被删除、跨授权目录覆盖、配置写错仓库等问题；onboarding 与日常管理也未形成可靠闭环。建议先修数据保护和作用域，再验收页面。此次仅增加审查材料，未修改业务代码或卡片状态。

## 范围与方法

固定基线 `c4a56a4`，审查 HEAD `edc48bd`，212 个文件，25,755 行新增、1,744 行删除。重点核对 AIL-039～051 及其调用的同步与保护代码；旧 RW 修改执行全量回归，不宣称逐项重新完成所有旧卡验收。Standards / Spec 两轴独立审查；CLI/HTTP 反例在隔离临时仓库与数据根执行。未使用用户真实宿主配置、未运行真实 MCP。

独立验证：`cargo test --locked --offline --all-features --no-fail-fast` 共 352 passed、0 failed、1 ignored（真实 npm registry 用例）；`cargo fmt --all -- --check`、`cargo clippy --locked --offline --all-targets -- -D warnings` 通过。测试通过不足以覆盖以下用户操作反例。

## Standards 轴：必须先修

| ID | 严重度 | 触发与实际结果 | 位置 | 关联卡 |
|---|---|---|---|---|
| S01 | P1 | 应用个人指令后手改文件，Undo 返回 conflicts=[] / 全部回滚，实际删除用户新内容 | `src/console/jobs.rs:600` | 050、042 |
| S02 | P1 | 已托管入口后来被 git add 跟踪，个人 plan 一边说公司文件已跳过，一边安排 delete；sync 真正删除跟踪文件 | `src/commands/personal.rs:253`、`:285` | 042、050 |
| S03 | P1 | 只批准 repo，workflow export execute 可写 sibling 未批准路径并覆盖已有用户文件 | `src/console/mod.rs:552`、`src/workflow.rs:365` | 046、045、049 |
| S04 | P2 | 服务重启后旧 plan 的 GET 返回200，但 apply 返回“计划任务不存在”；持久化记录未恢复为可操作任务 | `src/console/mod.rs:58`、`src/console/jobs.rs::spawn_apply` | 050、047 |

S01 只保存 before-image，没有校验当前文件仍匹配本次写入的 after-image。S02 的公司守卫只过滤期望集合，旧 managed manifest 又把被过滤目标转成清理动作。应对 create/update/delete/undo/recover 统一执行所有权、Git 保护、目标指纹和路径检查。

S03 导出路由遗漏 approved_roots 校验，也没有将覆盖与预览目标指纹绑定。S04 必须恢复任务状态和幂等信息，并明确中断状态；仅能展示磁盘 JSON 不等于可继续操作。

详细反例、实际响应、规范出处见 [Standards 报告](../evidence/review-2026-09-16/standards.md)。

## Spec 轴：作用域与能力配置

以下为独立 Spec 审查已复现结果；完整步骤见 [Spec 报告](../evidence/review-2026-09-16/spec.md)。

- **P1：配置会写错仓库。** `src/commands/personal.rs:401` 从 profile 的 key 集合取最后一项，既非当前 cwd，也非页面选择。已有 A 配置时在 B 执行 select，实际修改 A。HTTP select 同样没有明确 repo/scope 参数。需要端到端显式 RepositoryId / WorktreeId / ScopeId，不用排序或服务 cwd 推断用户意图。（040、044、048）
- **P1：连续工作树设置互相擦除。** `src/commands/personal.rs:440` 用新的单项 ScopeSelection 替换整层。先禁用 Claude 再禁用 Codex，前一项消失并恢复继承。应按字段合并、单项移除恢复继承。（040、044）
- **P1：宿主已关闭仍实际部署。** 团队启用 Claude 时，个人 effective 显示 Claude=false，但团队 artifacts 未按最终 enabled_hosts 重新过滤，plan/sync 仍写 `.claude/skills/example`。（044）
- **P2：搬迁身份与重关联失效。** 整体移动仓库会改变 repo_id 并丢失有效个人配置；relink 返回成功仍未恢复配置，且允许把 A 的工作树重关联到独立仓库 B。应验证 Git 身份并迁移注册关系，不能只改路径字符串。（039、040、048）
- **P2：非 Git 首次配置闭环断裂。** 能识别 nongit，但 personal effective 拒绝该目录；页面 runPlan 固定读取 repo.repo_root，而 nongit 返回 root。路径兜底仅展示成功，不能实际继续配置。（039、047）
- **P1：普通技能导入可破坏整个个人库。** 带 frontmatter 的普通 skill 在补元数据时缺少换行，生成非法 YAML；导入退出12，后续 library list 也退出12。应先在暂存位置验证完整源，再原子发布，失败保留旧库。（043、049、047）

此外，静态链路及现有测试直接证明：子项目 sync 与根 sync 使用同一落点与 manifest，互相移除入口（`src/commands/personal.rs:111`，`tests/personal.rs:259`，P1）；实际 profile 保存没有 revision/锁，未知字段和注释重序列化丢失（P2）；MCP missing_env_refs 仅定义未接入 plan/sync（P2）。这些未标为新增 CLI 复现，详见 Spec F07～F09。

资源/流程专项还发现保存前未完整校验导致库无法再次读取、产物更新缺版本前置条件且重命名破坏稳定关联、MCP 正文缺字面量秘密边界，以及订阅/流程绑定/现有文件导入缺少页面入口。详见 [专项报告](../evidence/review-2026-09-16/library-workflow.md)，其中导出覆盖与 S03 合并处理，不重复计数。

## 主审补充：onboarding 与日常界面

这里采用原始页面 JavaScript 处理函数的 Node VM 复现，DOM/API 使用桩；**不是本轮真实浏览器验收**。脚本与输出见 [UI 反例](../evidence/review-2026-09-16/ui-handlers-repro.log) 和 [可运行脚本](../evidence/review-2026-09-16/ui-handlers-repro.cjs)。

### U01 — P1：取消宿主勾选不产生禁用操作

`src/console/web.rs:182` 的 saveCapabilities 只对 checked=true 发 enable。先前启用的宿主取消勾选后，只有 PUT /api/draft，配置仍启用，页面却显示保存成功。这与上面的团队 artifact 问题是两个独立缺陷：即使后端修好，页面仍无法关闭。必须对每项提交明确选择，并以返回的 effective 配置刷新页面。（047、044）

### U02 — P1：切目录不废弃旧目标与计划

`src/console/web.rs:133` approveDir 只改 approvedRoot。复现从 A 切 B 后，draft 同时包含 approvedRoot=B、repo=A、planJob=plan-A、applyJob=apply-A、capSaved=true。用户改变目录后仍可能执行之前 A 的计划。选择变化必须使下游步骤失效，计划/应用处持续显示确切仓库、工作树、作用域；重新识别后才能继续。（047、048、050）

### U03 — P2：草稿冲突被吞掉，下次保存覆盖别人

`src/console/web.rs:49` 收到409后仅更新本地 revision。下一次原草稿使用服务器的新 revision 直接提交，失去并发冲突保护，界面没有冲突提示或合并选择。其他保存错误也被吞掉。应保留用户草稿并展示冲突，读取最新版本后显式重试/合并。（046、047）

### U04 — P2：工作树列表没有成为实际选择器

`src/console/web.rs:243`～`:292` 仅列出登记表，作用域表单要求手输完整资源 ID 和子目录，只有“当前工作树”布尔值，没有选择具体仓库/工作树并贯穿 effective/select/plan 的状态。界面宣称“当前作用域”但没有可靠指向用户所选对象。无法验收用户要求的快速选择任意 worktree 并独立管理能力。（048、049）

另外，detectHosts 只展示版本、不默认勾选已安装宿主（`:154`）；六步同时展示，step 固定为1，缺少约定的返回步骤与明确进度。这些体验缺口应随047返工，不另凑高严重度问题。

## 验收与返工顺序

1. **先保护数据：** S01/S02/S03。补用户后改、文件后来被跟踪、跨根/已有文件导出的真实反例，禁止成功响应掩盖内容丢失。
2. **再统一作用域：** 明确 repo/worktree/subproject 身份贯穿 API、配置、渲染和任务；修错仓、整层替换、搬迁和错误重关联。
3. **再修资源闭环：** 技能导入事务性、最终宿主开关、非 Git 兜底；界面使用返回的 effective 状态。
4. **最后验收 onboarding：** 取消勾选、切目录、双标签冲突、服务重启、任意工作树选择均走真实页面；补实际宿主加载证据。

建议重新打开有明确缺陷的039、040、042、043、044、045、046、047、048、049、050；041 本轮没有独立新增结论，沿用现有宿主支持证据与限制，不能因此认定跨宿主全能力可用。051保持开放：阻塞不只是缺三个外部体验者，上述实现缺陷和浏览器操作闭环也未通过。

本次没有自动修改卡状态。完整实现验收应把“API 测试通过”“页面能点击”“宿主真实加载”“用户体验达标”分别记录；不能互相替代。
