# 2026-10-02 新用户盲测

- 方式：一个 subagent 扮演第一次使用的用户，只读 README、docs/guide 与 `--help`，不读源码；隔离 HOME 与 `--data-root`，本地 Git 仓库模拟远端，不联网。
- 被测版本：提交 49d7f3a 之后、AIL-062 修复之前的工作区构建（部分问题在盲测进行时已另行修复，下表注明）。
- 覆盖面：个人资源导入/选择/同步、团队源、合集、知识库、hooks、doctor、uninstall、网页服务启停与鉴权。

## 确定的 bug

| # | 严重度 | 现象 | 处理 | 回归测试 |
|---|---|---|---|---|
| 1 | 高 | `update --preview-id P`（不带 `--execute`）重新检查并换新 ID，随后按提示加 `--execute` 必报 E4001 | 已修：带 preview_id 的预览只展示那次检查；提示给出完整命令 | tests/skill_sources.rs::standard_upstream_skill_checks_and_updates_with_the_previewed_id |
| 2 | 高 | Git 导入的标准 Skill（不带 AILoom 专有字段）刚导入就报 upstream-new；更新报「缺少 namespace」 | 已修：上游先按导入规则规范化再比较/发布；更新流程不再自锁 | 同上；failed_skill_validation_restores_entire_old_directory 改用规范化后仍无效的上游 |
| 3 | 高 | `.ailoom/project.toml` 与源锁写成 `ref_`，按契约手写 `ref` 被静默忽略 | 已修：写出 `ref`，读取兼容 `ref_` | tests/e2e.rs::project_declaration_uses_contract_ref_field |
| 4 | 中 | CLI sync 不输出任务 ID；删除型 sync 的撤销清单为空，undo 报 E9000 | 已修：输出 `job_id` 与撤销命令；删除动作可撤销（链接按原样重建，路径被重新占用时冲突保留）；不可撤销改报 E4001 | tests/personal.rs::cli_sync_reports_job_id_and_deletions_can_be_undone |
| 5 | 中 | doctor 遇到损坏的托管清单/源锁直接报错退出；纯个人模式被判为问题并建议 init | 已修：损坏文件作为失败检查项报告；无团队声明标为个人模式（健康） | tests/doctor_uninstall.rs::doctor_treats_personal_only_repo_as_healthy、doctor_reports_corrupt_files_instead_of_failing |
| 6 | 低 | `select --host X --resource Y` 静默丢掉 `--host` | 已修：同时给出时报 E0001 并提示分两次执行 | tests/personal.rs::team_and_personal_sync_do_not_undo_each_other（拆分调用） |
| 7 | 低 | 保存个人指令后提示 `ailoom personal sync`（exit 2） | 已修；同类错误命令另有 2 处，其中团队 Hook 注册命令 `ailoom hooks exec --id` **CLI 不接受，部署出去的团队 Hook 每次触发都失败** | tests/adapters_next.rs::deployed_team_hook_command_actually_runs_and_legacy_entries_migrate（真实执行注册命令 + 旧条目迁移）；tests/hook_events.rs 断言改为可执行写法 |

## 文档与体验问题

| 严重度 | 现象 | 处理 |
|---|---|---|
| 中 | `collection preview` / `list` 普通输出不显示 preview_id 与资源 ID | 已修：preview 打印资源与 apply 命令，list 打印完整资源 ID（collections.rs::collection_flow_works_without_json_and_update_needs_only_source） |
| 中 | 契约 MCP 示例把顶层字段写在 `[mcp.env]` 之后，照抄即无效 | 已修文档；新增 skill_sources.rs::contract_mcp_example_is_a_valid_resource 防回退。「一个坏资源让整个 plan 失败」对资源库已由 AIL-062 修复，团队源按契约仍整体校验 |
| 中 | 冲突提示只说「处理后重新 sync」 | 已修：plan / sync 列出冲突文件并说明两种处理办法 |
| 中 | 本地文件夹导入的 Skill 源目录改动后无法更新（重导 E2006 → 删除被拒） | 已修：本地来源支持 check-update / update；E2006 提示改指向更新流程（skill_sources.rs::local_folder_skill_can_check_and_apply_updates） |
| 低 | 合集更新必须带 `--name` | 已修：带 `--source` 时名称与地址沿用登记值；参数补了帮助说明 |
| 低 | 快速上手说 MCP 写入 `.codex/config.toml`，实际被标 unsupported；plan 输出有空 unsupported 行 | 文档补充 `$ENV:` 秘密引用不写入 Codex 的例外与实测加载状态；unsupported 行改为打印资源、宿主与原因 |
| 低 | library 各动作、`instructions --clear` 普通模式打印裸 JSON | 已修：library 列表/导入/检查更新/更新/删除有专门输出 |
| 低 | 导入不存在目录、`--file` 不存在、`--data-root` 指向文件的报错不带路径 | 已修：带路径与 fix；`--file` 改为先读文件再登记仓库 |
| 低 | 未知宿主 `select` exit 12、`init --target` exit 2 | 已统一为 E3004 |
| 低 | `library --skill` update 写名字、delete 要完整 ID | 已修（AIL-062 一并处理） |
| 低 | 停用宿主后 `.git/info/exclude` 残留 | 已修：根因是计数每次同步 +1 且移除函数无人调用；改为按 Worktree+作用域对账（personal.rs::git_exclude_entries_follow_what_is_deployed） |
| 低 | `hooks remove` 留下空 `{}` | 未处理：不影响宿主，文件是否由 AILoom 创建无法可靠判断 |
| 低 | `console` 不在顶层帮助 | 不处理：有意隐藏的调试命令，日常入口是 `ailoom web` |

## 网页盲测（另一轮，证据见 docs/evidence/blind-test-2026-10-02/）

34 个用例，31 通过。与本轮修复相关的三项：

| 用例 | 现象 | 处理 |
|---|---|---|
| T24 | 照抄 `ailoom source` 给出的接入命令失败：绝对 `--local-path` 被 init 以 E3007 拒绝；普通输出也不打印下一步 | 已修：同仓源给相对仓库根的路径，独立 Git 仓库给 `--url`，普通目录提示先 git init；普通输出打印下一步（source_init.rs::scaffold_next_step_command_works_verbatim 原样执行打印的命令） |
| T33 | 网页执行上游更新两次 E3002 回滚 | 即上表第 2 项；冻结新构建复测网页与 CLI 均通过 |
| T04（观察） | 网页导入预览只显示路径与 Skill 名，不展示正文 | 未处理 |

同时发现：`/api/effective` 不带 root 时退回服务进程 cwd，绕过目录授权且结果取决于后台服务从哪启动。已改为必须显式 root（console_routes_smoke 原来的「缺参数 4xx」断言此前只是碰巧成立）。

## 盲测确认正常的流程

个人资源导入→选择→plan/sync→symlink 与 exclude；关闭宿主后清理与幂等；个人指令（Claude 规则、Codex 视图随公司 AGENTS.md 重合成、清除）；Worktree / 子目录选择与越界拒绝；公司已跟踪文件跳过；团队源脚手架、init/status/plan/sync --refresh、冲突保留、团队与个人同步互不删除；合集删除保护；资源库同名冲突与 `--name` 另存；profile 损坏报错；`--json` 输出与错误格式；并发 sync 加锁；recall（含中文）；contribute 两种方式；知识库初始化与迁移；hooks 安装/移除；uninstall；dashboard；网页服务启动/复用/鉴权/Origin 校验/权限 0700/0600/单实例/停止。

## 测试隔离提醒

本机 shell 设置了 `XDG_DATA_HOME`，它优先于 HOME 决定 SkillStore 位置。只隔离 HOME 与 `--data-root` 的手工试验仍会把 Skill 实体写进真实的 `~/.local/share/ailoom/store`。子进程测试经 `tests/common::isolated_child_env` 同时设置了 `XDG_DATA_HOME` / `XDG_STATE_HOME`，不受影响；但进程内启动 ConsoleServer 的 console_server / console_onboarding / console_e2e 直接继承了 shell 的变量，每次 `cargo test` 都会往真实 store 写条目。

- 已修：新增 `tests/common::isolate_in_process_roots()`（每个测试进程一次，把 XDG 根指向专属临时目录），三个测试文件在启动服务前调用；修复后完整跑一遍测试，真实 store 目录列表前后一致。
- 已清理：真实 store 中来源为测试临时目录且已不存在的条目共 671 项移入废纸篓；保留 `/tmp/ailoom-dirux`（正在运行的服务）与 `/tmp/ailoom-host-acc`、`ailoom-host-check`（来源仍在的宿主核实夹具）。
- 手工复现与盲测需要同样设置 `XDG_DATA_HOME` / `XDG_STATE_HOME`。
