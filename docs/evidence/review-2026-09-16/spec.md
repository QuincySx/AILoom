# 2026-09-16 Spec 轴审查附件

范围：`git diff c4a56a4...edc48bd`。需求：`docs/cards/AIL-039.md` 至 `AIL-051.md`、`docs/initiatives/local-console.md`、`docs/CONTRACTS.md` §11。业务代码/卡状态未修改；测试使用临时 HOME/XDG/data-root 和隔离 Git 仓库，没有读取真实凭据或写远端。CLI 使用已有 `target/debug/ailoom`；全仓验证由主审负责。

本子审负责 registry/profile/resource selection 与 library/workflow/MCP/source 管理；主审负责 UI/onboarding，Standards 负责 HTTP/个人指令安全/apply-undo。本附件不替另外两轴宣称验收通过。

## 实际入口已复现

证据根 `/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/ailoom-review0916-spec-9jeeb9qh`。`evidence.json` 保存 cwd、完整 CLI args、退出码、输出与错误；`relink-evidence.json` 保存 HTTP 结果；`team-host/evidence-successful-init.json` 保存团队宿主选择反例。测试目录路径在 macOS canonicalize 后可能显示 /private/var 前缀，是同一目录。

### F01 · P1 · AIL-040/044/048：配置选择写错仓库

规范：AIL-044 “同仓默认开启 skill A/MCP X…worktree 可单独覆盖”；AIL-048 “操作目标持续可见”；计划要求 Repository/Worktree/Scope 分开且不改变未选工作树。

源：`src/commands/personal.rs:395-401`；`src/console/mod.rs:792-825`。SelectArgs 没有 repo/root，个人配置非空时直接取 profile.repos.keys().last()；API 也未传选择的仓库。`--worktree` 又基于进程 cwd 发现，不是浏览器当前工作树。

复现：临时 A 执行 personal effective，再 select --host claude --state enable；B 执行 personal effective，再 select --host codex --state enable。B 的操作成功退出0，但返回 repo_id=A (`repo-13f755542ecbedb0`)，不是 B (`repo-aac5917950bfde9d`)。因此第二个仓库根本没有自己的配置，用户对 B 的调整写到 A。

### F02 · P1 · AIL-040/044：单项工作树调整清空本层其他选择

规范：三态“禁用是显式值”，AIL-044 要求按资源与宿主开关及工作树独立覆盖。

源：`src/commands/personal.rs:439-440`：`repo_entry.worktrees.insert(wt, sel.clone())`；sel 只有本次一个 host/resource。仓库默认与子项目采用 merge，此分支整层替换。

复现：仓库默认 claude/codex 均开启；连续 select --host claude --state disable --worktree，然后 codex disable --worktree。两次退出0。effective 最终 claude=true/origin=repo_default，codex=false/origin=worktree_override。第二次选择无声撤销第一次，资源开关亦同。

### F03 · P1 · AIL-044：有效配置关闭宿主，但实际仍部署团队资源

规范：AIL-044 “按资源与宿主选择能力及解释有效配置”，三态宿主选择必须控制期望部署；AIL-040 个人层覆盖团队层。

源：`src/commands/personal.rs:172-193`。enabled_hosts 只用于个人库 render，团队层直接复用按团队原 targets 渲染的 artifacts，仅过滤 resource id。

复现：隔离团队本地源含 shared skill example，init --local-path team-src --project a --target claude --no-builtin（退出0）；个人选择 claude disable。effective 输出 hosts.claude.enabled=false；personal plan 却有 create .claude/skills/example、target_tool=claude；personal sync 退出0且 applied 含该路径，磁盘入口存在。全部本地操作，无远端写入。

### F04 · P2 · AIL-039/048：重关联接受不相关独立仓库

规范：CONTRACTS §11.1 “重关联需新路径仍是本仓库工作树”；AIL-039 独立 clone 不误并。

源：`src/repo_registry.rs:350-370`。list_worktrees 在 new_path 自己的仓库运行；belongs 仅检查 new_path 出现在它自己的 worktree list，未比较 registry.common_dir。

复现：POST /api/repo/relink 用 A 的 repo_id/旧 wt_id，new_path=B（独立 git init）。实际 HTTP200、relinked=true。原 A 工作树登记指向 B，边界校验失效。

### F05 · P2 · AIL-039/040/048：搬迁重关联没有恢复稳定身份/配置

规范：AIL-039 “搬迁/删除后可重关联，旧事件/journal 不丢失或并仓”；AIL-048 “搬迁重关联不会重建丢历史”；CONTRACTS §11.1 WorktreeId 移动/重关联保持不变。

源：`src/repo_registry.rs:152-155,305-344`；`src/commands/personal.rs:93-104`。RepositoryId 每次按当前位置 common-dir 重新计算；refresh_worktrees 每次按当前路径 hash 查找/新增，不查已重关联 entry.path。旧 workspace 数据也没有重关联入口。

复现：A 已保存配置后 rename 至 A-moved；effective 返回新 repo_id 与 hosts={}。实际调用重关联 old repo/old wt→A-moved 返回200，随后 effective 仍选择新 repo_id (`repo-d5354e9145f24e5d`)、新 worktree_id (`ff8c02d6924b`)，旧 worktree_id 为 fcadcac29d3a，配置仍空。对于 common-dir 不变的 linked-worktree 搬迁，refresh 的 path hash 同样会新增新 id，并把已 relink 的旧 id 标 missing（该子例静态确认）。

### F06 · P2 · AIL-039/047：非 Git 路径模式只有分类，没有登记/应用链路

规范：AIL-039 “非 Git 路径显式注册”；计划首次选择目录要求非 Git 文件夹显示路径模式。

源：`src/commands/personal.rs:89-90` 无条件 discover_repo；`src/console/mod.rs:785-787` 对 NonGit 只返回 kind/root，无登记。

复现：临时无 .git 目录运行 personal --action effective，退出10/E1001，message=起点不在 Git 仓库内。不是 Git 错误回退问题，而是合法非 Git 路径模式完全未接通。UI 是否阻断由主审补充。

## 静态链路与现有测试直接证明，未新增 CLI 复现

### F07 · P1 · AIL-040/044/048：父子作用域仍互相移除同一宿主入口

规范：AIL-044 “父子作用域相同物理目标由统一计划器协调，不由两次 sync 互相移除/覆盖”；计划要求缺少目录显示未匹配、不得对子项目禁用冒充全局禁用。

源：`src/commands/personal.rs:111-125` active_rel 只改变过滤选择，render 与 plan 仍用 ctx.workspace.workspace_root 和同一 managed manifest；`tests/personal.rs:259-277` 直接要求 web sync 移除根已部署 skill-a、改为 skill-b。

根默认 A，web 模板禁 A/启 B：根 sync 后再 web sync 会从整个根移除 A；再次根 sync 则移除 B。这个测试将违反“父子不互删”的行为当作通过。所谓测试注释“ADR：worktree 是落点”在 ADR-0002 中没有对应许可，CONTRACTS §11 也未取消该不变式。scope 参数还未验证目录是否存在/是否穿越嵌套 Git，设置不存在 scope 可照样改变根部署。

### F08 · P2 · AIL-040：个人配置写回丢未知字段/注释，预览迁移未交付

规范：AIL-040 “旧声明与工作区数据有预览式迁移/回退，保存未知字段和注释”；并发编辑拒绝丢失更新。

源：`src/profile.rs:122-128`（PersonalProfile::save）使用 serde typed model 重序列化整文件，无未知字段保留；select 采用无 lock/revision 的 read-modify-write。卡片第56行承认注释不保留，却第57行宣称必需项全过；契约§11.2只写机器数据区属性，没有显式变更“保留”验收。Console draft 的 revision 不保护实际 profile select。此项为要求缺口，不以机器配置属于本地为由豁免。

### F09 · P2 · AIL-044/049：MCP 引用缺失诊断未接入实际入口

规范：AIL-044 “MCP 单服务开关、所需依赖/秘密引用缺失、host 不支持有可解释状态”；AIL-049 “MCP 的服务状态、引用缺失和受控连接测试分开”。

源：`src/adapters/mcp.rs:121` missing_env_refs 只有定义，全仓 src 无调用；personal plan/sync 的 notes 无 MCP 检查。卡片完成记录声称 missing_env_refs 进入 notes，实际代码未发生。没有读取真实环境值；仅静态检查调用图。

## Library/workflow/source 专项

详见 [Library/workflow 专项](library-workflow.md)（独立子审）。其确认导入 YAML 拼接失败污染整个库（CLI复现）、编辑未校验会锁死库、导出覆盖 dirty 未跟踪文档、流程产物无版本前置条件且 rename 破坏后续编辑身份、MCP 原始正文泄露字面量、订阅/流程绑定/现有文件导入 UI 未接通。

## 逐卡覆盖与证据缺口

| 卡片 | 本子审结论/证据缺口 |
|---|---|
|039|F04/F05/F06 阻断：身份、重关联、非 Git 链路未兑现。卡自认 prunable 未单构，不能把解析字段当完整场景验收。|
|040|F01/F02/F05/F07/F08：分层纯函数有测试，但目标路由、持久化/迁移与生命周期不闭环。|
|041|宿主能力矩阵与真机证据存在；本子审未复跑宿主，不独立认定通过；主审复核。Codex MCP 未加载应持续显示边界。|
|042|公司文件/个人指令由 Standards 深审，本子审不重复判通过。|
|043|专项发现常见合法 Skill 导入失败后破坏整个库；原始导入校验/安全库要求未过。|
|044|F01/F02/F03/F07/F09；effective 的 enabled/deployed 与真实计划不一致，不能凭纯分层测试关卡。|
|045|专项发现导出覆盖用户 dirty 文件、流程身份/版本冲突未闭环、绑定/输入版本缺入口。|
|046|HTTP/路径边界由 Standards 负责；本子审观察 API profile select 缺真实目标，F01。|
|047|主审负责实际 onboarding；F06 无合法非 Git 路径完整流程；资源导入失败会影响首次上手。|
|048|F01/F04/F05/F07 直接阻断仓库/工作树日常切换、稳定重关联和子项目隔离。|
|049|专项证实编辑校验、秘密边界、dirty 冲突与源/流程 UI 交付缺口；F09 MCP 状态未接线。|
|050|apply/undo/计划绑定由 Standards 与主审负责；本子审不以相关现有测试宣称通过。|
|051|保持 Blocked 的3名独立试用者缺口合理；本轮反例说明自动化“全部通过”不能代替体验链路 correctness。|

以上仅在各自证据范围内报告。没有将所有公开函数存在视为产品入口已经完成，也没有将未复现的静态反例标为 CLI 结果。

## 状态建议（仅按当前证据）

039/040/043/044/045/048/049：In progress，已实现但存在必要功能反例；041：保留 Done 与 Codex 项目 MCP 宿主限制；042/046/050：由 Standards/主审具体反例确定，不在本子审重复给通过结论；047：主审已复现 UI 取消开关与切目录旧计划问题，建议 In progress；051：继续 Blocked。验收缺陷可在本地继续修复，不应仅因当前失败标 Blocked。

主审最终统一验证：352 passed、0 failed、1 ignored，fmt/Clippy 成功；这些结果不消除上列反例。
