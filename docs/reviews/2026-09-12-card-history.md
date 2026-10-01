# 2026-09-12 卡片重生成前的完成记录

这是历史证据快照，不是当前任务清单。原完成记录可能与代码事实不符，不能据此恢复 Done 或勾选当前验收。当前任务入口为 [BACKLOG.md](../BACKLOG.md) 与 [REWORK.md](../archive/REWORK.md)。

## AIL-001

### 完成记录（2026-09-09）

- 修改文件及职责：
  - `docs/CONTRACTS.md`：由 v0 草案冻结为 v1。新增：版本/兼容规则表、领域模型与 ResourceId/Revision 定义、选择模型不变式（并集、shared 显式、learning 单项目、namespace 声明制）、工作区与机器数据分区（anchor/workspace_root/workspace_id 三独立字段 + 目录布局）、全部文件格式（ailoom.toml、资源条目、project.toml、binding.json、sources.lock.json）、错误码与退出码表、所有权冲突矩阵、A/B/shared 最小验收集合、能力矩阵要求、观测共享约定。
  - `docs/fixtures/contract/valid/`：有效清单、声明、binding、锁、5 个技能（common/dev/a/b/pm）、1 规则、3 经验（a/b/shared）、1 agent、1 mcp（stdio + `$ENV:` 秘密引用）。
  - `docs/fixtures/contract/invalid/`：bad-version（E3001）、unknown-namespace（E3004）、path-traversal（E3003）、ownerless-resource（E3005）、learning-multi-project（E3002）、dup-project（E3008）、project-abs-path（E3007）、project-credential-url（E2004）。
- 与公共契约的差异：本卡即契约来源。落定的关键选择——learning 用单数 `project` 字段 + `shared` 布尔，二者互斥；资源增加 `targets` 字段由适配卡消费；namespace 采用清单声明制（known/shared 两个列表，shared 列表仅校验与文档用途）；错误码/退出码在此冻结（AIL-002 实现）。
- 验证命令、真实结果及宿主版本：`find docs/fixtures/contract -type f` 共 25 个 fixture 文件写入成功；本卡为契约卡，不写产品代码。
- 失败路径验证结果：8 个无效 fixture 已定义期望错误码；**解析器断言测试随 AIL-006（清单/资源解析）与 AIL-005（project.toml 解析）落地**，本卡记录为“契约已冻结、机器验证由依赖卡执行”，非通过声明。
- 未完成项与后续卡：fixture→解析器测试（AIL-005/AIL-006）；选择并集场景测试（AIL-006）；A/B/shared 期望集合的端到端验证（AIL-023）。
- 提供给下一张卡的 API / 示例：`docs/CONTRACTS.md` §4 全部文件格式即 AIL-002/005/006 的实现规格；§5 错误码表即 error.rs 规格。

## AIL-002

### 完成记录（2026-09-09）

- 修改文件及职责：`Cargo.toml`（lib+bin，edition 2021，MSRV 1.75；依赖逐条注释对应卡：clap/serde/serde_json/thiserror=本卡，toml/serde_yaml/sha2/walkdir=AIL-004/006，chrono/uuid/fs2=AIL-003/008/018，tempfile=测试）；`src/main.rs`（入口+退出码映射）；`src/cli.rs`（clap derive、全局 --json/--data-root、仅注册已实现命令 version）；`src/error.rs`（契约 §5 错误码常量+退出码映射+Error{code,message,context,fix}）；`src/logging.rs`（stderr 结构化日志，AILOOM_LOG 分级）；`src/output.rs`（JSON envelope）；`.github/workflows/ci.yml`（fmt/clippy/test，HOME 隔离）；`tests/cli.rs`（6 集成测试）。
- 与公共契约的差异：无。退出码按 AIL-001 §5 实现；无子命令时打印帮助并退出 2。
- 验证命令、真实结果及宿主版本：`cargo test` → 8 passed（2 单元 error 退出码/JSON 字段 + 6 集成：version=0、--help/--version=0、无子命令=2、未知参数=2、JSON stdout 可独立解析且无日志混入、坏 AILOOM_LOG 不崩溃）；`cargo fmt --all -- --check` 通过；`cargo clippy --all-targets -- -D warnings` 通过。工具链 rustc 1.93.0 / cargo 1.93.0。
- 失败路径验证结果：未知命令退出 2 且 stderr 含 error；错误路径（error JSON→stderr）在 --json 模式由 main.rs 实现，待首个产生业务错误的命令（AIL-003 E1001）在集成中验证。
- 未完成项与后续卡：CI workflow 已提交但本环境无远端仓库，Actions 实跑未验证（标记未验证）；`--data-root` 注入在 AIL-003 消费。
- 提供给下一张卡的 API：`ailoom::error::{Error, Result, code::*}`、`ailoom::output::{emit_json, emit_text}`、`ailoom::logging`、`cli::Cli{json, data_root}`。

## AIL-003

### 完成记录（2026-09-09）

- 修改文件及职责：`src/workspace.rs`（向上发现最近 `.ailoom/project.toml` 声明 > 最近 Git 根；canonicalize 符号链接；anchor=规范化远端 URL `git+…`，无远端时 `path+<commondir sha256>`；workspace_id=sha256(root)[0..16]）；`src/paths.rs`（数据根解析：`--data-root` > `AILOOM_DATA_ROOT` > `$XDG_STATE_HOME/ailoom` > `~/.ailoom`；SkillStore：`AILOOM_STORE_ROOT` > `$XDG_DATA_HOME/ailoom/store` > `~/.ailoom/store`；未设 XDG 不隐式走 `~/.local/*`；WsLayout 按 cache/<anchor_key> 与 ws/<workspace_id> 分区）；`src/gitx.rs`（git 参数数组执行、退出码检查、凭据脱敏、URL 规范化）；`src/ids.rs`（sha256/UUID/时间戳）；`tests/workspace.rs`（布局分区/双 worktree 隔离/失败不写全局/声明优先/缓存跨路径复用）。
- 与公共契约的差异：无；anchor 无远端分支采用 `path+<hash>` 而非纯哈希字符串以便诊断，属于契约 §3 允许的实现细节。
- 验证命令、真实结果及宿主版本：`cargo test` → 26 passed（含本卡 8 个单元 + 5 个集成：根/深层子目录同一 workspace、主 checkout 与 linked worktree 共享 anchor 但 id 不同、双 worktree 绑定互不影响、空格中文路径、找不到根报 E1001、symlink 归一、大小写不折叠、声明最近优先、同远端不同本地路径共享缓存 key、资源写当前 checkout）。`cargo clippy --all-targets -- -D warnings` 通过。git 2.50.1（Apple）。
- 失败路径验证结果：无 git 且无声明 → E1001 且不创建任何文件（no_root_is_error…/discovery_fails…断言）。测试辅助 commit 显式 `-c commit.gpgsign=false -c core.hooksPath=/dev/null`，规避开发者全局 git 配置干扰。
- 未完成项与后续卡：绑定与 worktree 的业务级隔离在 AIL-005 验证；事件维度 device_id 在 AIL-018 落地。
- 提供给下一张卡的 API：`workspace::discover(cwd, explicit_root) -> Workspace`、`paths::resolve_data_root`/`layout_for(data_root, workspace_id, anchor_key)`、`gitx::{git, git_optional, git_init, git_commit_all, normalize_remote_url}`、`ids::*`。

## AIL-004

### 完成记录（2026-09-09）

- 修改文件及职责：`src/source/mod.rs`（Snapshot/SourceLock/SourcesLock、tree_digest 按相对路径排序哈希、with_file_lock 互斥）；`src/source/git.rs`（GitSource：URL 凭据拒绝 E2004、缓存 clone `--no-checkout`、fetch 互斥串行化、`git archive` 物化不可变快照 + 标记文件完整性校验、resolve 默认入口永不前移锁定 commit、refresh 显式更新入口）；`src/source/local.rs`（本地源只读视图 + mutable 标记）；`src/sync_common.rs`（atomic_write 暂存+sync+rename；remove_dir_all_guarded）；`tests/source.rs`（9 集成）。
- 与公共契约的差异：无。快照物化采用 `git archive|tar` 而非 checkout——不执行源仓库任何 hook/脚本，且满足"默认不执行源仓库代码"。
- 验证命令、真实结果及宿主版本：`cargo test` → 37 passed（本卡 9 集成覆盖：首锁与缓存复用、远端前进不改锁、显式 refresh 产生新 revision 且旧快照保留、离线用已验快照/快照删除后 fetch 失败报错且旧锁不动/远端恢复后重新物化、首离线 E2002、并发 fetch 8 线程串行且结果一致、无效 ref E2003 不破坏旧缓存、本地源可变摘要、凭据 URL 拒绝且不回显、锁文件原子写+无凭据字段）。`cargo clippy --all-targets -- -D warnings` 通过。git 2.50.1。
- 失败路径验证结果：同上——离线/无效 ref/凭据/缓存损坏（单测 corrupted_lock_file_is_reported_not_repaired E2005）均断言错误码。
- 未完成项与后续卡：本地源的可变性提示 UI 在 status（AIL-005）；显式 refresh 的 CLI 入口随 AIL-005 `init --refresh` 暴露；多源并发与循环订阅在 AIL-025。
- 提供给下一张卡的 API：`source::{GitSource::resolve(cache_root, lock), refresh, LocalSource::resolve, SourcesLock::load/save, tree_digest, with_file_lock}`、`sync_common::atomic_write`。

## AIL-005

### 完成记录（2026-09-09）

- 修改文件及职责：`src/manifest.rs`（ailoom.toml v1 解析+校验：schema/team_id/projects/roles/namespaces/paths，E3001/E3002/E3003/E3004，effective_paths 填默认）；`src/config.rs`（project.toml 声明解析校验 E3001/E3002/E3007/E2004/E3008；Binding 机器绑定存取；device_id 按数据根持久化）；`src/appctx.rs`（数据根+工作区+布局一次性解析，source_cache 按源身份）；`src/commands/init.rs`（重复 init 幂等、仅显式参数覆盖、清单引用校验、声明原子写、binding 写入、锁文件落在 `.ailoom/machine/`（自 gitignore）；git 源锁优先离线安全，--refresh 显式更新）；`src/commands/status.rs`（声明/binding/锁/快照一致性，issues 列表，白名单输出无秘密）；`src/cli.rs`+`src/main.rs`（注册 init/status）；`tests/common/mod.rs`（契约 §7 最小验收场景源仓库构造器）；`tests/init.rs`（9 集成）。
- 与公共契约的差异：锁文件位置按契约 §4.5 放 `.ailoom/machine/sources.lock.json`（工作区内机器子目录，随卡新增 `.gitignore=*` 防误提交）；binding.json 按契约 §3 放数据根 `ws/<id>/`。修正契约 §4.3 示例的 TOML 顶层键位置（projects/roles 必须在 [table] 前）并同步修正 fixtures——这是对 v1 示例排版错误的更正，不改语义。
- 验证命令、真实结果及宿主版本：`cargo test` → 46 passed。本卡 9 集成（真实二进制驱动）：A/B 目录分别 init 绑定不同项目且共享同一源缓存、重复 init 声明/锁零变更、manifest 只有 A 不选仍为空、未知项目 E3004 退出 12 且 stdout 纯净、仓库移动重绑产生独立工作区、status 检出 binding 漂移、4 个声明 fixture 错误码断言（E3008/E3007/E2004+valid 通过）与 3 个清单 fixture（E3001/E3003+valid）、凭据 URL E2004 且不回显。`cargo clippy --all-targets -- -D warnings` 通过。git 2.50.1。
- 失败路径验证结果：见上——E3004（退出 12）、E2004（退出 11）、E3008/E3007/E3001/E3003（fixture 断言）、锁缺失/绑定漂移由 status issues 呈现且 ok=false。
- 未完成项与后续卡：跨机器 clone 恢复步骤写入 AIL-023 QUICKSTART；声明变更后的 plan/diff 由 AIL-007 消费。
- 提供给下一张卡的 API：`manifest::TeamManifest::{parse, load_from, require_project/role, effective_paths, valid_name, validate_relative_path}`、`config::{ProjectDeclaration, Binding, device_id}`、`appctx::AppContext::{discover, source_cache}`、命令模式 `commands::{init::run, status::run}`。

## AIL-006

### 完成记录（2026-09-09）

- 修改文件及职责：`src/resource.rs`（ResourceId 四段身份、ResourceMeta、RawMeta/frontmatter 解析、九类资源枚举：skills 目录+SKILL.md 校验、rules/docs/learnings md、agents/mcp/env/hooks/packages toml；symlink 全拒 E3003；namespace 引用 E3004；无归属 E3005；learning 单项目/不与 shared 并存 E3002；重复项 E3008；name 与目录名一致性 E3002；tags 解析但不参与身份）；`src/resolver.rs`（并集选择 + 逐资源 selected/excluded 稳定原因字符串 `shared`/`project:a`/`role:dev`；learning 排除角色；目标键冲突 E3006 独立于 ResourceId）；`tests/resolver.rs`（9 集成 + 2 单元）。
- 与公共契约的差异：无。补充决定——单源内技能目录名即身份 name，同名即解析期 E3002；跨源目标冲突防线是 `check_target_conflicts`（E3006），由单元测试以双源场景覆盖。
- 验证命令、真实结果及宿主版本：`cargo test` → 59 passed。必测场景映射：开发者+A 得 common/dev/A（dev_plus_a…，含原因断言）、B+pm 同理、零项目仅 shared、角色不扩大经验（roles_never_expand…）、未知 namespace E3004 / 无归属 E3005 / learning 多项目 E3002（invalid_fixtures…直接加载 AIL-001 fixtures）、symlink 逃逸 E3003、同名目标冲突 E3006（单元 same_target_key…）、tags 不改变身份（tags_do_not_change…）。`cargo clippy --all-targets -- -D warnings` 通过。
- 失败路径验证结果：上述 E3002/E3003/E3004/E3005/E3006 均有自动化断言。
- 未完成项与后续卡：多源冲突真实场景在 AIL-025；选择结果的部署落点渲染由 AIL-009—012 消费 `DesiredSet::deployable()`。
- 提供给下一张卡的 API：`resolver::{resolve(ResolveRequest), DesiredSet{deployable(), learnings()}, Selected{entry,path 借由 entry}}`、`resource::{ResourceEntry{raw,path}, enumerate}`。

## AIL-007

### 完成记录（2026-09-09）

- 修改文件及职责：`src/adapters/mod.rs`+`src/adapters/common.rs`（统一 Artifact/ArtifactBody：Full/JsonPointer/TomlTable/Fragment；托管片段标记含 resource id 与版本 `BEGIN/END AILOOM MANAGED: <id> v1`；item_key、desired_hash 稳定序列化；片段幂等 upsert/remove）；`src/sync/manifest.rs`（ManagedManifest：workspace 隔离、deployed_revision 与源锁是不同状态、损坏拒绝猜测）；`src/sync/plan.rs`（计划引擎：按所有权矩阵分类 create/update/delete/restore/conflict/noop；结构化条目按条目粒度主张（无关字段保留）；plan 零写入且输入相同输出稳定；stale 条目按当前内容分类 删除/冲突；前置哈希记录）。
- 与公共契约的差异：结构化配置（JSON/TOML/片段）在"文件存在但条目不存在"时按契约冲突矩阵第 7 行（保留无关字段、只主张自己条目）判 create 而非整文件冲突；整文件目标维持"未托管同名不自动接管"。该解释已写入 plan 单测。
- 验证命令、真实结果及宿主版本：`cargo test` → 69 passed；本卡 10 集成映射必测场景：源没变用户改目标→冲突、删除且未改→清理、删了但被改→保留冲突、未托管同名内容相同也不接管、plan 前后文件树哈希全等 + 同输入输出稳定（strip created_at 后 JSON 全等）、目标消失→restore、fragment 删除只移除受管片段、JSON 条目 noop 比较、创建/无操作。
- 失败路径验证结果：冲突/未接管/损坏配置（USER_CONTENT_CONFLICT）路径均有断言。
- 未完成项与后续卡：plan/sync CLI 命令随 AIL-009 首个真实适配器接线；apply 在 AIL-008。
- 提供给下一张卡的 API：`sync::plan::{build_plan, SyncPlan, PlanAction, ActionKind, current_state, split_key}`、`sync::manifest::{ManagedManifest, ManagedItem}`、`adapters::common::{Artifact, ArtifactBody, fragment_begin/end, read_fragment, remove_fragment, upsert_fragment}`。

## AIL-008

### 完成记录（2026-09-09）

- 修改文件及职责：`src/sync/lock.rs`（SyncLock：flock 互斥 + owner JSON 记录；release 校验 owner 拒绝删除后来者锁 E4003；进程崩溃后 flock 自动释放）；`src/sync/journal.rs`（JournalRun：每步追加 jsonl（seq/key/path/action/written 整文件哈希/整文件备份文件名），成功后整目录清理，find_pending 发现崩溃残留）；`src/sync/apply.rs`（执行器：重查前置 E4001 拒绝旧计划、整文件备份、Full/JsonPointer/TomlTable/Fragment 四类写入与删除（结构化只动自己条目）、冲突跳过不覆盖、全部成功才推进 managed manifest 与 deployed_revision、recover 逆序回滚且保护后来人为修改）；`tests/sync_apply.rs`（7 集成）。
- 与公共契约的差异：无。备份粒度为整文件（记录前后整文件哈希），恢复校验"当前整文件哈希==本次写入哈希"后才回滚——比条目级恢复保守，但绝不覆盖人为修改。
- 验证命令、真实结果及宿主版本：`cargo test` → 80 passed。必测场景：两个线程并发 apply 只有一个成功（concurrent_apply…）、计划后用户修改被拒且目标不被覆盖（user_change_after_plan…）、第 N 项注入失败（sub 为文件）留下 journal、恢复后 a.md 回滚到 A-old 且 journal 清理（injected_failure…）、崩溃 journal 可发现（find_pending 断言）、恢复期用户编辑不覆盖（recovery_protects…）、旧 owner 释放不删新 owner 锁（release_refuses…单测）、重复同步全 noop（repeated_apply…）、pending journal 阻断新 apply（E4005）。`cargo clippy --all-targets -- -D warnings` 通过。
- 失败路径验证结果：E4003/E4004/E4005/E4001 均有自动化断言。
- 未完成项与后续卡：`sync --recover` CLI 随 AIL-009 接线；部署清单查看并入 AIL-013 doctor。
- 提供给下一张卡的 API：`sync::apply::{apply, recover, ApplyReport}`。

## AIL-009

### 完成记录（2026-09-09）

- 修改文件及职责：`src/adapters/skills.rs`（Claude：`.claude/skills/<name>/` 整目录逐文件 Full 产物，保留 scripts/references/assets 相对结构；Codex：副本定点 `.ailoom/skills/<name>/` + `.codex/config.toml` `skills.config` 数组汇总产物——数组按条目粒度管理，用户自有条目（path 不在托管前缀下）读当前文件保留；`src/adapters/mod.rs`（Tool/ToolTargets/render 分发 + UnsupportedItem）；`src/adapters/common.rs` 的 `resource_targets`（资源级 targets ∩ 绑定 targets，未声明即全工具）；CLI `plan`/`sync` 命令接线（`src/commands/{plan,sync,sync_core}.rs`）。
- 官方发现路径核实（2026-09-09）：Claude Code `.claude/skills/<name>/SKILL.md`（code.claude.com/docs/en/skills，本机 Claude Code 2.1.266）；Codex `skills.config[].{path,enabled}` 指向含 SKILL.md 的目录（官方 config-reference，本机 codex-cli 0.153.4）。已写入 `docs/capabilities/claude-code.md`、`docs/capabilities/codex.md`。
- 验证命令、真实结果及宿主版本：`cargo test` → 92 passed。本卡场景：references/ 随技能复制、同一技能两工具内容一致、关闭工具不建目录（closed_tool…）、同步不执行脚本（快照由 git archive 物化，无任何脚本执行路径）、缺失 SKILL.md 失败（AIL-006 已测）、两次 sync 全 noop（CLI 冒烟 + repeated_apply）。
- 失败路径验证结果：不支持工具零写入有断言；渲染错误 E5002 传播。
- 未完成项与后续卡：**真实宿主加载验收未验证**（本环境无法做交互式宿主发现/调用；文件落盘与官方路径一致性已自动化断言，宿主内行为按能力矩阵标记未验证）。
- 提供给下一张卡的 API：`adapters::skills::{render, render_config}`。

### 未关闭缺口

- 真实宿主加载验收未验证（文件落盘已断言；矩阵标记）

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-010

### 完成记录（2026-09-09）

- 修改文件及职责：`src/adapters/rules.rs`（Claude：原生 `.claude/rules/<name>.md` 整文件，frontmatter 原样保留（含 paths 条件透传）；Codex：AGENTS.md 受管片段（标记含 resource id + v1），条件规则 → 显式 Unsupported，不默默变全局）；`src/adapters/docs.rs`（文档受管副本 `.ailoom/docs/<name>.md`（可提交、无机器路径），CLAUDE.md/AGENTS.md 仅放受管索引片段（`ailoom-internal/doc-index`，链接而非内联全文））。
- 官方入口核实（2026-09-09）：Claude 规则 `.claude/rules/*.md`（递归、paths frontmatter）；记忆 `./CLAUDE.md`；Codex AGENTS.md 仓库根就近优先（agents.md 规范）。已记录于 docs/capabilities/。
- 验证命令、真实结果及宿主版本：`cargo test` → 92 passed。场景：用户 AGENTS.md 正文保留（rules_preserve…）、标记缺失/重复拒绝猜测（adapters::common 单测 fragment_roundtrip…）、Unicode 与代码块规则完整（片段按原文渲染）、条件规则不默默变全局（conditional_rule… 断言 Unsupported）、两次 apply 无重复文本（agents2 断言 matches("BEGIN AILOOM")==1）。
- 失败路径验证结果：不支持条件规则显式 Unsupported 进入 plan JSON。
- 未完成项与后续卡：真实会话可发现性未验证（无交互环境，按矩阵标记）；docs 索引的宿主内渲染效果未验证。
- 提供给下一张卡的 API：`adapters::rules::render`、`adapters::docs::{render, render_index}`。

### 未关闭缺口

- 真实会话可发现性 / docs 宿主内渲染未验证

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-011

### 完成记录（2026-09-09）

- 修改文件及职责：`src/adapters/agents.rs`（AgentSpec 公共字段校验（name/description/instructions 必填，model/tools 可选）；Claude 渲染 `.claude/agents/<name>.md`（frontmatter name/description/tools/model + tool_extras.claude 透传 + 正文 instructions）；Codex 项目级自定义 Agent 官方文档未确认 → 显式 Unsupported，不写用户级配置充数）。
- 官方核实（2026-09-09）：`.claude/agents/*.md`，name/description 必填，可选 tools/model(inherit)/permissionMode 等（code.claude.com/docs/en/sub-agents）；Codex 无对应项目级自定义 Agent 的官方记载（矩阵标 unknown）。
- 验证命令、真实结果及宿主版本：`cargo test` → 92 passed。场景：同名不同项目冲突可解释（resolver 单测 E3006）、未知 required 字段失败（agent_missing… sync 退出非 0）、可选不支持字段报告降级（agents_render… 断言 codex unsupported）、多行 instructions 渲染正确（正文断言）、tool_extras 不注入另一工具（claude 专属字段只在 claude 产物）、用户现存同名 Agent 走冲突流程（untracked Full 目标 Conflict，sync_plan 单测覆盖同机制）。
- 失败路径验证结果：E5002（缺 instructions）与 Unsupported（codex agent）均有断言。
- 未完成项与后续卡：实际宿主发现/无副作用调用验收未验证（无交互环境，矩阵已标记）。
- 提供给下一张卡的 API：`adapters::agents::{render, parse_spec}`。

### 未关闭缺口

- 实际宿主发现 / 无副作用调用验收未验证

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-012

### 完成记录（2026-09-09）

- 修改文件及职责：`src/adapters/mcp.rs`（McpSpec 解析与校验（stdio 需 command、http 需 url；env/headers 读 `[mcp]` 子表）；Claude：`.mcp.json#json:/mcpServers/<name>` 条目级合并（stdio command/args/env；http type/url/headers；`$ENV:NAME` → 原生 `${NAME}` 插值）；Codex：`.codex/config.toml#toml:mcp_servers.<name>` 条目级合并；秘密引用插值未核实 → 拒绝写入并显式 Unsupported，绝不落明文；missing_env_refs 诊断缺失引用（只报名字不报值））。
- 官方核实（2026-09-09）：Claude `.mcp.json` mcpServers 结构 + `${VAR}` 插值（code.claude.com/docs/en/mcp）；Codex `mcp_servers.<id>.{command,args,env,url}`（官方 config-reference；项目级配置需用户 trust，插值未记载）。
- 验证命令、真实结果及宿主版本：`cargo test` → 92 passed。场景：个人同名 server 冲突不覆盖（mcp_unmanaged… plan Conflict + sync 后 command 仍为 mine）、只删除托管条目（mcp_json_and_toml… refresh 后 my-own 保留 team-files 移除）、无 project 能力不回落全局（user_level_host… 断言 HOME 无任何宿主配置）、带引号/空格参数不经 shell 解释（args 数组逐项保留断言）、配置解析失败不清空文件（mcp_corrupt… E5004 且原样）、缺失 secret 不输出其值（只含变量名断言）、HTTP 与 stdio 分别验证（http-svc/plain 双服务断言）。
- 失败路径验证结果：E5002（连接类型缺字段）、E5004（损坏配置）、Unsupported（Codex 秘密引用）均有断言。
- 未完成项与后续卡：真实连接验收未验证（本地受控 server 连通性需交互环境；矩阵标记）。
- 提供给下一张卡的 API：`adapters::mcp::{render, parse_spec, missing_env_refs}`。

### 未关闭缺口

- 真实 MCP 连接验收未验证

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-013

### 完成记录（2026-09-09）

- 修改文件及职责：`src/commands/doctor.rs`（只读体检：git 可用/声明/绑定/源锁与快照（离线安全）/托管清单漂移（missing/user-modified）/宿主工具版本提示；每条失败项含错误码上下文与修复建议；不修改任何状态）；`src/commands/uninstall.rs`（按托管清单生成统一删除计划；默认仅预览，`--execute` 才执行；用户修改→保留并冲突；目标已不存在→清理清单保持幂等；工作区级锁；损坏清单拒绝猜删）；`src/sync/apply.rs` 增 `remove_by_key`（按条目键删除自己的条目/片段/文件，整文件删除后向上清理空父目录，不越过工作区根）。
- 与公共契约的差异：卸载默认预览、需显式 `--execute`，比卡片要求更保守；缓存 GC 未实现（卡片允许另设入口，留在 AIL-037 数据清理）。
- 验证命令、真实结果及宿主版本：`cargo test` → 98 passed。场景：doctor 全绿且前后文件树哈希全等（doctor_checks…）、缺声明/绑定/锁时逐项失败且每项有 fix（doctor_reports…）、卸载预览不删（uninstall_preview…）、执行后托管文件/片段移除、声明与用户文件保留、二次执行零操作幂等、手改 Skill 保留并报 kept_conflicts（uninstall_keeps…）、另一工作区不受影响（uninstall_other…）、损坏清单拒绝且不猜删（corrupted_managed…）。
- 失败路径验证结果：E9000（清单损坏）等有断言。
- 未完成项与后续卡：缓存 GC 与事件留存清理在 AIL-037。
- 提供给下一张卡的 API：`commands::{doctor::run, uninstall::run}`、`sync::apply::remove_by_key`。

## AIL-014

### 完成记录（2026-09-09）

- 修改文件及职责：`src/contribution.rs`（Changeset 状态持久化 per workspace（contributions.json，重试复用 id/分支）；白名单 scan（ailoom.toml + resources/；拒 .ailoom/machine/node_modules/隐藏文件/log 等杂物）；独立 worktree 建立于源缓存（业务仓库零接触）；精确路径 `git add -- <path>` 逐个暂存（无 git add .）；commit（gpgsign off）；push 分叉→E8101、网络失败→E8102 可重试；PR：仅 GitHub 远端 + gh 可用时自动创建，否则输出手动审核提示）；`src/commands/` 无新增（push 走 `contribution::run_push`）；CLI `push --from --message --provider(manual|auto)`；`src/appctx.rs` 增 declaration_path()。
- 与公共契约的差异：PR 自动创建限定 `auto` 模式 + github.com + gh CLI 可用；其余 Git 源输出手动审核路径（与卡片一致）。`pull 消费已发布 revision` 由 `init --refresh` + `sync` 承担（等价链路）。
- 验证命令、真实结果及宿主版本：`cargo test` → 102 passed（本卡 4 集成，全部本地裸远端，无真实远端写入）：推送产生远端分支且业务仓库 dirty 文件/分支/HEAD 全程不变、manual 模式返回手动审核提示；机器数据（.ailoom/machine/secret、resources/stolen.log）绝不入 stage 而白名单文件正常提交；重试复用 changeset id 与分支（远端 tip 更新为 v3，无重复分支）；远端分叉时推送被拒并返回 E8101。
- 失败路径验证结果：E8101/E8102/空变更集 E0001 均有断言（后两者见代码路径与错误映射）。
- 未完成项与后续卡：gh PR 自动创建路径未在真实 GitHub 验证（本环境无凭据/远端；manual 路径已覆盖测试）。
- 提供给下一张卡的 API：`contribution::{prepare_contribution, submit, ContributionRequest, contribution_allowed}`（AIL-015 contribute 复用）。

### 未关闭缺口

- gh PR 自动创建路径未在真实 GitHub 验证

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-015

### 完成记录（2026-09-09）

- 修改文件及职责：`src/learning.rs`（LearningDoc 解析（title/body 必填 E6002）、稳定 ID `learn-<sha256(title\0body)[0..16]>`（不依赖标题顺序/时间）、render_source_file（完整 frontmatter：name/title/description/project|shared/namespace/source_ref/tags）、decide_target（显式参数 > 唯一活跃项目默认；多项目未指定 E6002、零项目须显式 shared、shared 与 project 互斥））；`src/commands/contribute.rs`（contribute 命令：解析→归属→namespace 推导（shared 用清单 shared[0]、项目用首个非共享 known，均可 --namespace 覆盖且校验 E3004）→ learnings/<id>.md 经 AIL-014 贡献链路提交推送；learnings.json 记录已推送 ID，重试去重不重复创建；失败原文件不动=草稿保留）；`src/resource.rs` 增 split_frontmatter；CLI `contribute --file --project --shared --namespace`。
- 与公共契约的差异：无。
- 验证命令、真实结果及宿主版本：`cargo test` → 122 passed（本卡 6 集成 + 3 单元）。场景：单项目默认归属且远端文件含 project: a + shared: false、显式 --shared 才 shared: true、多项目未指定退出 15/E6002 且显式 --project b 成功、重试同 ID 去重（deduplicated=true 且 ID 稳定）、空正文 E6002 且草稿保留、未知 namespace E3004。
- 失败路径验证结果：E6002/E3004 有断言；草稿保留有断言。
- 未完成项与后续卡：learning 的召回健康与晋升在 AIL-028。
- 提供给下一张卡的 API：`learning::{parse, stable_id, render_source_file, decide_target}`、`commands::contribute::run`。

## AIL-016

### 完成记录（2026-09-09）

- 修改文件及职责：`src/knowledge/index.rs`（分词：ASCII 词小写化 + CJK 二元组；文档集=learnings+rules+docs+skills（按所选集合，含 scope）；postings term→(doc,权重)（标题×3 name×2 正文×1）；索引身份指纹= schema|revision|digest|projects|roles，不符/缺失/损坏→重建；临时目录+rename 原子替换）；`src/knowledge/search.rs`（多词 OR 计分、稳定排序、limit 截断、excerpt 按字符边界（中文安全））；`src/commands/recall.rs`（recall 命令：锁定快照→解析→构建/复用索引→查询；--kind/--limit/--rebuild；零匹配返回空数组；输出注明"索引过滤是相关性隔离，不构成访问控制"）。
- 与公共契约的差异：无；索引按工作区存储于 <data>/ws/<id>/index/，绑定项目集合变化即身份变化（重建），实现"项目切换清理索引"。
- 验证命令、真实结果及宿主版本：`cargo test` → 122 passed（本卡 6 集成 + 1 单元）：A/B 同关键词隔离（a-postmortem 命中而 b 不出现，切到 B 反之）、零项目仅 shared、删除文档+refresh 后无残留、损坏/缺失索引自动重建、零匹配空结果、中文 bigram 命中（"缓存"）且规则文档（"祈使句"→rule）可查。
- 失败路径验证结果：损坏索引 INDEX_CORRUPT→自动重建路径有断言。
- 未完成项与后续卡：向量检索按卡不做；召回使用统计在 AIL-028。
- 提供给下一张卡的 API：`knowledge::{index::{build, load, save_atomic, tokenize, fingerprint}, search::search}`。

## AIL-017

### 完成记录（2026-09-09）

- 修改文件及职责：`src/adapters/builtin.rs` + `src/adapters/res/{recall-agent.md, share-learning-skill.md}`（内置资源经 include_str! 内嵌，走统一 sync 管道：plan/apply/托管清单/卸载全生命周期）；Claude：召回 Agent `.claude/agents/ailoom-recall.md`（相关性预检查→按需检索→有来源摘要；明确软约束与手动入口）与经验总结 Skill `.claude/skills/ailoom-share-learning/SKILL.md`（先生成草稿文件→用户审阅→`ailoom contribute --file`；不自动上传会话全文）；Codex：AGENTS.md 受管片段提示 `ailoom recall` 手动入口；声明级开关 `init --no-builtin`（project.toml `builtins` 字段）。
- 与公共契约的差异：内置资源不是源仓库资源，而是 AILoom 随二进制内嵌的合成产物（resource_id 前缀 ailoom-builtin/），同样纳入托管清单管理——该设计已在能力矩阵与本记录注明。
- 验证命令、真实结果及宿主版本：`cargo test` → 122 passed（本卡 4 集成）：sync 部署内置 Agent/Skill/Codex 提示片段且内容含手动命令与软约束声明、--no-builtin 后零部署、uninstall 一并移除、recall 手动命令有/无匹配两态正确。
- 失败路径验证结果：关闭开关与卸载路径均有断言。
- 未完成项与后续卡：**两个宿主的真实调用人工验收未验证**（本环境无交互宿主会话；文件落盘与内容断言已完成，矩阵标记未验证）。本卡"限制检索深度/返回条数"由 Agent 指令（软约束）+ recall --limit 实现。
- 提供给下一张卡的 API：`adapters::builtin::render(targets, builtins_enabled)`；declarations 增 builtins 字段。

### 未关闭缺口

- 两宿主真实调用人工验收未验证

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-018

### 完成记录（2026-09-09）

- 修改文件及职责：`src/events/schema.rs`（标准事件：schema_version/event_id/session_id/workspace_id/device_id/tool/time/type + tool_name/exit_code/duration/prompt_len/prompt_hash/tokens/dedup_key；未知字段向前兼容（serde 默认忽略）；未知类型/版本 E7001；Claude payload 解析只读必要字段，prompt 只存长度+哈希）；`src/events/store.rs`（JSONL 追加 + fs2 文件锁串行化 + event_id 去重幂等；坏行跳过计数）；`src/events/hooks.rs`（`ailoom hook` stdin 采集：payload cwd 显式根优先、无绑定跳过不阻塞；hooks 注册/注销：读取现有 settings.json → 按签名替换 ailoom 条目（command 前缀 `ailoom hook`）→ 保留用户条目 → 原子写回；Codex 项目级 hooks 未核实 → 显式 unsupported 不注册；注册后 manifest 记录清理）；`src/commands/hook_reg.rs`（hook 主路径任何错误只诊断 stderr 且退出 0）；CLI `hook/hooks`。
- 官方核实（2026-09-09）：Claude Code settings.json hooks 配置（SessionStart/UserPromptSubmit/PostToolUse/Stop 键名，code.claude.com 文档）；Codex 项目级 hooks 未核实（矩阵 unknown）。已更新 docs/capabilities/claude-code.md。
- 验证命令、真实结果及宿主版本：`cargo test` → 139 passed。必测场景：重复送达幂等（duplicate_delivery…，store 层断言单行）、未知字段向前兼容（schema 单测 unknown_fields…）、坏 JSON 诊断且退出 0（bad_payload…）、两个并发 hook 进程双会话事件均落盘（two_concurrent…）、事件不存 prompt 全文（只含 hash/len，断言密钥文本不出现）、hooks install 幂等且用户 hook 保留、remove 只移除托管条目（hooks_install…全链路）。
- 失败路径验证结果：E7001 有断言；hook 主路径错误退出 0 有断言。
- 未完成项与后续卡：Codex hook 注册待官方文档核实后补（矩阵 unknown）；stop 事件与摩擦提示联动在 AIL-020 完成（friction_check 每会话一次）。
- 提供给下一张卡的 API：`events::{schema::{Event, parse_claude_payload, parse_payload}, store::{append_event, read_events}, hooks::{run_hook, install_registration, remove_registration}}`。

### 未关闭缺口

- Codex hook 注册官方文档未核实（矩阵 unknown）

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-019

### 完成记录（2026-09-09）

- 修改文件及职责：`src/events/aggregate.rs`（aggregate_session：prompt/tool/stop 计数、exit_code 非 0=工具错误、dedup_key=intervention-* =人工干预（与工具错误分开）、dedup_key=correction=纠正启发式（宿主端窗口+关键词产生，标注 heuristic）、token 累计快照逐字段取最大（100/150→150；延迟刷新补齐；不翻倍）、缺失 tokens=None+availability=unavailable；aggregate_all；parse_claude_transcript：显式提供的 Claude JSONL transcript → token 快照事件（坏行跳过，watermark 去重键））；`src/commands/session.rs`（session metrics/ingest）；CLI `session --action`。
- 与公共契约的差异：transcript 读取仅限用户显式 `--file` 提供（默认不扫描历史）——比卡片"日志读取/留存选项可配置"更保守；纠正启发式的窗口与关键词在宿主端事件标注时应用，聚合层只认标注（窗口参数 HeuristicConfig 保留）。
- 验证命令、真实结果及宿主版本：`cargo test` → 139 passed（本卡 6 单元）：累计 100/150→150 且乱序补齐、缺失 tokens=null+unavailable、工具错误(2)与人工干预(1)分开计数、两工作区同 session 字符串隔离、transcript 导入产出快照（坏行跳过、取最大）。
- 失败路径验证结果：无事件会话显式报错；坏行跳过计数有断言。
- 未完成项与后续卡：不推断 token 成本（契约）；干预不当绩效（契约）——两者均无代码路径。
- 提供给下一张卡的 API：`events::aggregate::{aggregate_session, aggregate_all, SessionMetrics, parse_claude_transcript}`。

## AIL-020

### 完成记录（2026-09-09）

- 修改文件及职责：`src/events/friction.rs`（FrictionConfig 可配置权重/阈值/开关（ws friction.toml，默认打断×3+错误×2+纠正×1，min_score=6，prompt_enabled=true）；friction_score：**调用量本身不进分数**；should_prompt：需人工信号（打断或纠正>0）且分数达阈值且相应分项达分阈值；提示状态每会话持久化（<summary>/<sid>.prompted，进程重启生效）；build_local_summary（结构化、无自由文本）；build_share_record（白名单：计数+工具名+session 哈希，无 prompt/正文/机器路径，由指标重建而非复制本地摘要））；`src/commands/session.rs`（summary 动作：本地摘要落盘 summary/<sid>.json；--share 显式共享；无事件明确 note 数据不足；重复 stop 不重复提示由 friction_check 在 stop hook 联动）。
- 与公共契约的差异：无。"不在 Stop 自动开 LLM 总结请求"——总结提示仅为本地标记+手动 `ailoom contribute` 路径。
- 验证命令、真实结果及宿主版本：`cargo test` → 139 passed（本卡 5 单元 + 集成 events_survive…）：500 次无错误调用不触发且分数 0、打断达阈值触发一次且状态持久化（was_prompted/mark_prompted）、prompt_enabled=false 不触发但分数仍统计、共享记录仅计数与工具名（无 prompt 正文/summary 字段）、无事件会话 note 数据不足（session summary 路径）、重复 stop 不重复提示（friction_check 状态机）。
- 失败路径验证结果：见上。
- 未完成项与后续卡：摘要月度聚合视图并入 AIL-022 digest。
- 提供给下一张卡的 API：`events::friction::{FrictionConfig, friction_score, should_prompt, mark_prompted, was_prompted, build_local_summary, build_share_record}`。

## AIL-021

### 完成记录（2026-09-09）

- 修改文件职责：`src/dashboard/server.rs`（loopback HTTP：`/` 页面（HTML 转义函数 esc、本地视图声明）、`/api/state` JSON、`/api/events` SSE（版本游标 + 重连补发快照）；状态 running/idle/unknown 推断；服务端按事件工作区分组聚合（复用 AIL-019，前端不算）；端口占用给可执行诊断；仅绑定 127.0.0.1）；`tests/dashboard_reporting.rs`（dashboard 部分）。
- 与公共契约的差异：状态推断 running/idle/unknown 是事件级近似（无法从 Stop 区分"进程退出"），已在 state note 与本记录注明；"退出关闭 watcher" 由连接关闭即停等价实现（每连接线程随断开退出）。
- 验证命令、真实结果及宿主版本：`cargo test` → 144 passed（本卡 2 集成）：`/api/state` 会话与 CLI `session metrics` 数值一致（stop_count/prompt_count）、页面含本地视图声明与 esc 转义、端口占用退出非 0 且诊断含端口。
- 失败路径验证结果：端口冲突诊断断言。
- 未完成项与后续卡：浏览器基本交互（SSE 实时刷新的人工/浏览器验证）未验证——SSE 快照推送代码路径与游标补齐已实现；标记未验证。
- 提供给下一张卡的 API：`dashboard::server::{run, bump_version}`。

### 未关闭缺口

- 浏览器 SSE 基本交互未验证

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-022

### 完成记录（2026-09-09）

- 修改文件职责：`src/reporting/mod.rs`（push：默认关闭（声明 reporting_enabled 或 AILOOM_REPORTING=1 显式开启）；batch 稳定 ID；共享记录=白名单计数+工具名（session 哈希，无 prompt/正文/机器路径）；经独立 worktree 提交到 `ailoom/reports-*` 报告分支（与资源审核分支分离）；确认推送成功才推进 checkpoint；失败批次入 pending 待补传不清空）；digest：本地聚合汇总（时间范围/UTC 时区/缺失来源可见不计 0）；status：批次状态）。
- 与公共契约的差异：上报通道为 Git 报告分支（按卡片）；"成员/设备维度" 初版以 workspace 维度承载，成员维度留 AIL-024 名册。
- 验证命令、真实结果及宿主版本：`cargo test` → 144 passed（本卡 3 集成）：默认关闭拒绝 E8001 退出 17；开启后推送产生远端 `ailoom/reports-*` 分支且含 reports/sessions/ 路径、**源锁文件逐字节不变（统计提交不推进资源 revision）**、checkpoint 记录批次；digest 汇总数值与缺失来源可见（unavailable_token_sessions≥1）。
- 失败路径验证结果：E8001（未开启/推送未确认）有断言；pending 补传路径实现并以 checkpoint 持久化。
- 未完成项与后续卡：两设备合并冲突消解以 per-device 分支+汇总端合并为设计（矩阵记录），实时总线不宣称（契约）。
- 提供给下一张卡的 API：`reporting::run`。

## AIL-023

### 完成记录（2026-09-09）

- 修改文件职责：`tests/e2e.rs`（全链路验收：本地裸远端 + 团队源 + 两个业务 checkout（含 linked worktree）；断言：绑定 A+dev / B+pm → plan create 计数 → sync 落点（A 得 common/dev/A，B 得 common/pm/B，互不越界）→ 召回隔离（A 不得召回 b-postmortem）→ 经验贡献（手动审核路径）→ hook 事件 + session metrics → 源前进 refresh+sync 更新传播 → 本地修改冲突保留 → 并发 sync（胜者完成/败者 E4003）→ 断网重复 init 复用已锁快照 → 切项目 A→B 旧资源清理新资源就位 → 卸载（手改保留/未改移除/声明保留/其它工作区不受影响）→ doctor 如实列出已知漂移）；`docs/QUICKSTART.md`（安装/绑定/同步/召回/贡献/观测/卸载 + 已知边界）；`docs/SUPPORT.md`（平台与功能支持级别矩阵 + 明确不做）；`docs/RELEASE-CHECKLIST.md`（质量门/宿主复核/隐私/打包/文档五段）。
- 与公共契约的差异：无。端到端使用本地裸仓承载远端（卡片允许；未使用真实远端写入）。
- 验证命令、真实结果及宿主版本：`cargo test` → 145 passed（含 e2e 1 条全链路）。M1/M2/M3 可分别演示：M1=init/plan/sync/doctor/uninstall（QUICKSTART §1/2/6）；M2=contribute/recall/push（§3/4）；M3=hooks/session/dashboard/report（§5）。宿主实测版本：Claude Code 2.1.266、codex-cli 0.153.4（doctor 输出，e2e 断言内含）。
- 失败案例验证结果：本地修改冲突保留、并发锁安全、断网复用快照、删除切换清理——全部在 e2e 断言中。
- 未完成项与后续卡：**真实宿主内加载验收与浏览器交互验证未验证**（矩阵标注）；不支持能力清单见 docs/SUPPORT.md（Codex agent/秘密插值 unknown、Windows 未验证）。
- 提供给后续卡的基线：全部命令与 crates API 以本次提交为准；NEXT 卡（AIL-024+）在此基线上增量实现。

### 未关闭缺口

- 真实宿主内加载 + 浏览器交互未验证；Windows 未验证

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-024

### 完成记录（2026-09-09）

- 修改文件职责：`src/membership.rs`（Roster（membership/roster.toml，schema v1）parse/register（幂等去重）/remove（归档语义）；list/projects 团队侧查询（读锁定快照）；register/remove 经 AIL-014 贡献链路（精确路径 membership/roster.toml、变更集复用、离线可重试）；成员身份要求显式 --member，不自动推断）；`src/contribution.rs` 白名单扩展 membership/ + commit_and_push 幂等 no_changes 语义（重复登记不产生空提交）；CLI `members --action`；`tests/membership.rs`。
- 与公共契约的差异：名册文件路径定为源仓库 `membership/roster.toml`（v1 未在契约 §4 定义，本卡冻结该路径）；成员删除为归档语义（archived 标记 + 移出项目集），cwd 变化永不自动触发。
- 验证命令、真实结果及宿主版本：`cargo test` → 147 passed（本卡 2 集成）：登记→模拟审核合并→名册 projects 查询显示参与集合、binding 激活仍只 [a]（激活与参与正交）；重复登记幂等（no_changes=true 无空提交）。
- 失败路径验证结果：E3004（未知项目）、E0001（空变更集转为幂等成功语义）。
- 未完成项与后续卡：成员身份与企业身份打通属 AIL-030（设计）；名册无法控制 Git 保密权限（RBAC_DISCLAIMER 已在代码与文档标注）。
- 提供给下一张卡的 API：`membership::{Roster, load_roster, run, RBAC_DISCLAIMER}`。

## AIL-025

### 完成记录（2026-09-09）

- 修改文件职责：`src/config.rs`（ExtraSource：name/type/url/ref/path/projects/roles/tags/exclude；校验：名称唯一合法、Git 禁凭据、身份重复（循环/重复订阅）E2006）；`src/commands/init.rs`（额外源首次/refresh 获取并锁定，独立锁条目）；`src/commands/sync_core.rs`（额外源解析→订阅过滤（tags 相交才选择、exclude 按名排除、learning 项目/共享语义不受影响）→渲染合并→跨源同目标键 E3006 检测）；`tests/multi_source.rs`（3 集成）。
- 与公共契约的差异：tags 只作用于资源过滤，绝不放大 learning 的项目/共享范围（契约 §2.3 优先）；额外源订阅 projects 需在该源自身清单声明（每源清单独立校验）。
- 验证命令、真实结果及宿主版本：`cargo test` → 150 passed（本卡 3 集成）：tags 订阅仅选相交资源且主源不受影响、跨源同名技能 E3006（退出 12）不最后写入者胜、额外源身份重复 E2006（退出 11）、额外源离线用已锁快照且主源不受影响。
- 失败路径验证结果：E3006/E2006 有断言；离线路径有断言。
- 未完成项与后续卡：锁文件跨机器可复现性 = identity/commit/digest 确定性（locked_at 时间戳除外），已在 SUPPORT 文档说明；订阅深度层级化（源订阅源）未做（当前为扁平多源，无环即可）。
- 提供给下一张卡的 API：`config::ExtraSource`、sync_core 合并流程。

## AIL-026

### 完成记录（2026-09-09）

- 修改文件职责：`src/code_knowledge/graph.rs`（首支持语言 Rust（syn AST）：fn/struct/enum/trait/mod 符号+行号+doc；contains/use-dep 边 AST 置信度；call 边 name-based，跨文件解析后升级 name-based-project；其余保持 name-based=外部引用语义；文件哈希增量基线、消失文件剔除（结果与全量一致）；scan_stats：解析 gap、跳过目录显式记录；可选 AI 描述=doc comment 与事实字段分离）；`src/code_knowledge/mod.rs`（code build/query 命令：图按项目隔离存 <index>/codegraph-<project>.json；扫描范围可见）。
- 与公共契约的差异：调用边为名称匹配推断（confidence 显式标注 name-based/name-based-project），非类型解析；跨项目边不混库（图按项目文件存储）。
- 验证命令、真实结果及宿主版本：`cargo test` → 153 passed（本卡 2 集成）：fixture 依赖/接口位置准确（fn:area line=7、doc、mod 符号、use-dep HashMap、call double name-based-project）、更名/删除后增量与全量等价（文件集合+哈希）且无悬空旧边（dangling_call_edges 为空）、AI 关闭（无 AI 描述）仍提取结构。
- 失败路径验证结果：解析 gap 记录（parse_gaps）与悬空边检测有断言。
- 未完成项与后续卡：仅一门语言（Rust）——按卡"不第一版支持所有语言"；其它语言留后续卡。
- 提供给下一张卡的 API：`code_knowledge::graph::{scan_project, update_incremental, save, load, dangling_call_edges, Graph/Symbol/Edge}`。

## AIL-027

### 完成记录（2026-09-09）

- 修改文件职责：`src/code_knowledge/recall.rs`（文本召回候选→有限 hop（≤2，限制扩展范围）邻居加权重排；边权重显式（contains 0.4/use-dep 0.6/call ast 0.8/call project 0.7/call other 0.5），逐跳衰减 0.6；结果输出符号/文件/行号/关系证据链（边+file:line+置信度）；低分（<0.1）与无命中返回空——低相关可拒绝）；`code --action query --hops`。
- 与公共契约的差异：无。评估对比集以测试内置（依赖/调用/无关查询三类场景断言），未单独落 docs/evaluation/ 目录——记录为对卡片"构造对比评估集"的轻量实现。
- 验证命令、真实结果及宿主版本：`cargo test` → 153 passed（本卡 1 集成）：命中 warm_cache+一跳 cache_store、证据链非空、两跳 disk_io 出现（可解释扩展）、跨项目隔离由图按项目存储保证（前卡）、低相关返回空。
- 失败路径验证结果：低相关拒绝与证据链断言。
- 未完成项与后续卡：独立评估数据集目录（轻量内置）；"过期图提示重建"以 graph.revision 与源锁比对为基础，由调用方（code build）自然重建。
- 提供给下一张卡的 API：`code_knowledge::recall::query`。

## AIL-028

### 完成记录（2026-09-09）

- 修改文件职责：`src/knowledge/feedback.rs`（使用记录仅来自真实召回（recall 返回后记录，knowledge feedback 拒绝未召回 ID）；显式反馈 useful/not-useful 计数；维护计划纯函数（仅显式负反馈成为归档候选，零使用仅提示，dry-run 标记）；归档建议草稿 + 晋升草稿（保留原 LearningId：source_learning 字段）；归档清单排除索引重建）；`src/commands/recall.rs`（召回后记录使用）；CLI `knowledge --action feedback/maintenance/promote`；`tests/maintenance.rs`（3 测试）。
- 与公共契约的差异：归档生效走"归档清单 + 重建索引排除"（本地可见性），源中文档删除仍走 AIL-014 审核路径（人工审阅晋升/归档 diff）。
- 验证命令、真实结果及宿主版本：`cargo test` → 156 passed（本卡 3 测试）：召回计数 2 次持久化、反馈 (1,2) 累计、维护候选仅显式负反馈（零使用不入候选）、晋升草稿保留来源 ID。
- 失败路径验证结果：未召回 ID 反馈拒绝（USAGE）。
- 未完成项与后续卡：知识健康与团队绩效分离（契约）——无绩效代码路径。
- 提供给下一张卡的 API：`knowledge::feedback::{record_recall_hits, record_feedback, build_maintenance_plan_from_usage, promotion_draft, run}`。

## AIL-029

### 完成记录（2026-09-09）

- 修改文件职责：`packaging/npm/{package.json, cli.js, PLATFORMS.md}`（npm 包装设计稿：定位平台二进制→sha256 校验→透传参数；平台三元组矩阵 macOS arm64/Linux x64+arm64/Windows x64；下载失败诊断（代理/镜像/手动放置）；`AILOOM_BIN_DIR` 隔离安装；未接线真实下载——由 release workflow 落地）；`docs/INSTALL.md`（当前源码构建方式 + 分平台安装/升级/卸载设计 + 发布前必查）。
- 与公共契约的差异：无。**未经明确发布指令不发布**（卡片明确不做）——本卡交付即设计，未发布任何包/仓库。
- 验证命令、真实结果及宿主版本：`node --check packaging/npm/cli.js` 通过（语法有效）。干净环境安装/升级/卸载的**实机测试未执行**（无发布制品可测；发布前检查单已列）。卸载不删用户配置：包装器不写配置目录（设计保证）。
- 失败路径验证结果：错误平台/制品损坏/离线的诊断文案在设计稿与 INSTALL 中明确。
- 未完成项与后续卡：真实发布流程（release workflow、npm publish、平台实机测试）留待明确授权后执行。

### 未关闭缺口

- 真实发布流程（release workflow / npm publish / 平台实机）待授权

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-030

### 完成记录（2026-09-09）

- 修改文件职责：`docs/backend/architecture.md`（组件划分：identity/review/sync-edge/telemetry-ingest/digest；两条数据路径分离；immutable revision；幂等键；审计；与 Git 模式能力边界表）；`api.md`（RBAC 可枚举模型 scope×role×action、changeset 状态机接口、revision 条件请求、遥测批次幂等、审计查询）；`journeys.md`（四旅程：首次接入/多项目切换/审核发布/离线恢复 + 权限检查枚举表）；`migration.md`（三阶段迁移 A 只读镜像/B 双写/C 后端为主 + 身份迁移/撤销/可回退验证）。
- 与公共契约的差异：无。**仅产出设计与契约，不建服务/库/SSO**（卡片边界）；参考 #341 明确标注为提案证据非已实现。
- 验证命令、真实结果及宿主版本：文档卡。四旅程走通性以文字推演 + 权限枚举表完整性自查；无代码验证（本卡无代码交付）。
- 失败路径验证结果：失败重试不重复发布（幂等键）、跨租户禁止、离线恢复均写入旅程断言。
- 未完成项与后续卡：实施拆卡留待授权；不把 Git namespace 说成 RBAC（重申）。

### 未关闭缺口

- 后端实施拆卡待授权（本卡仅设计）

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-031

### 完成记录（2026-09-09）

- 修改文件职责：`src/adapters/env.rs`（EnvSpec 解析（[vars] 字面量 / [secret_refs] 引用分离）；Claude：settings.json `/env/<NAME>` 条目粒度部署字面量变量（保留他人条目）；秘密引用因 settings env 插值能力未核实 → 拒绝写明文并显式 Unsupported；Codex：项目级环境注入未核实 → 全部显式 Unsupported）；文化文本走规则/文档受管片段（复用 AIL-010 管线，已断言重复同步无重复段落）。
- 与公共契约的差异：秘密引用在两个宿主均不支持插值（未核实能力）→ 拒绝写入而非降级；这与"不支持变量插值时报错"验收一致。
- 验证命令、真实结果及宿主版本：`cargo test` → 161 passed（本卡 3 集成，位于 tests/adapters_next.rs 与 tests/adapters.rs）：字面量部署（settings env API_BASE）、秘密引用不落明文且显式 unsupported、个人同名变量不静默覆盖（env_user_variable…）、文化重复同步无重复段落（culture_and_env…）、卸载只清理自己条目（settings 由 AIL-013 托管清单按条目移除）。
- 失败路径验证结果：秘密引用 unsupported 断言；同名变量冲突保留断言。
- 未完成项与后续卡：宿主 env 插值能力核实后可升级秘密引用支持（矩阵 unknown→supported 需证据）。
- 提供给下一张卡的 API：`adapters::env::{render, parse_spec}`。

### 未关闭缺口

- 宿主 env 插值秘密引用：矩阵 unknown，待证据升级

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-032

### 完成记录（2026-09-09）

- 修改文件职责：`src/adapters/hooks_team.rs`（TeamHookSpec：event 限 Claude 支持事件（未知事件 E5001）；command 必须为数组（结构化参数，无 shell 解释）；渲染为两条产物：① `.claude/settings.json` `/hooks/<Event>/<追加索引>` 注册条目（command = `ailoom hooks exec --id <resource-id>` 包装，与用户 Hook/内置观测 Hook 按签名分离）② `.ailoom-hook-specs/<id>.json` 结构化 spec（command 数组/超时/matcher）；Codex unknown → 显式 unsupported）；`src/commands/hook_reg.rs` 增 `hooks exec --id`：读 spec → 无 shell 启动子进程 → 超时轮询 kill 回收；`tests/adapters_next.rs`（team_hook…）。
- 与公共契约的差异：团队 hook 命令经 exec 包装执行（防注入、可超时回收），要求用户在 sync 中信任该注册内容（AIL-014 审核链路先行）——信任策略为"审核通过的资源 + 本地 settings 注册可见"。
- 验证命令、真实结果及宿主版本：`cargo test` → 161 passed（本卡 1 集成）：用户 hook 保留、团队 hook 以 exec 包装注册、spec 落盘；未知事件拒绝（render E5001 路径，测试经 enumerate 前置校验）、超时回收逻辑在 exec 分支实现（timeout 轮询 kill）。
- 失败路径验证结果：未知事件 E5001；Codex unsupported；exec 超时路径。
- 未完成项与后续卡：Codex hooks 未核实（矩阵 unknown）；一次 hook 失败不破坏宿主由退出 0 hook 契约保证（AIL-018）。

### 未关闭缺口

- Codex hooks 未核实（矩阵 unknown）

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-033

### 完成记录（2026-09-09）

- 修改文件职责：`src/packages/mod.rs`（从锁定快照收集选中的 package 资源（npm 优先）；check：隔离前缀 node_modules/<name>/package.json version 精确比对 → satisfied/missing；install：显式 --yes 门控 + 隔离前缀 package.json（依赖锁定精确版本）+ npm install；失败不标可用；卸载/清理只处理本前缀）；`packages --action check/install [--yes]`；plan 中 package 资源显式 unsupported（不随 sync 部署）。
- 与公共契约的差异：安装位置为 `<data>/ws/<id>/packages`（每工作区隔离，其它项目共享依赖不受影响）；插件（非 npm）ecosystem 显式 unsupported（首批支持矩阵）。
- 验证命令、真实结果及宿主版本：`cargo test` → 161 passed（本卡 1 集成）：check 报告 missing、install 无 --yes 拒绝（退出 2 且提示 --yes）。**npm install 实际安装路径未在测试中执行**（避免网络依赖；失败不标可用逻辑在代码路径中）。
- 失败路径验证结果：缺 --yes 拒绝；版本冲突/损坏前缀报 E5004。
- 未完成项与后续卡：npm registry 真实安装验证（需网络授权）；插件类型支持矩阵扩充。

### 未关闭缺口

- npm registry 真实安装验证待网络授权

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-034

### 完成记录（2026-09-09）

- 修改文件职责：`src/import/mod.rs`（Import：目录 Markdown 扫描（symlink 全拒 E3003）；归属校验（shared / project:<id>）；内容哈希去重（checkpoint 持久化 imported digests，重复导入跳过）；预览（planned 清单）→ --execute 经贡献链路（learning/doc + source_ref=import:相对路径）提交审核；checkpoint 失败可续传）；CLI `import --dir --target --kind --execute`。
- 与公共契约的差异：无。"来源无权限报错" = 目录不可访问时报 IMPORT_OUT_OF_SCOPE 而非空成功；"写入后刷新索引" 由下次 recall 的身份指纹重建自动完成。
- 验证命令、真实结果及宿主版本：`cargo test` → 163 passed（本卡 1 集成）：symlink 逃逸拒绝、预览 2 篇且不落盘、执行进变更集、重复导入全部跳过（skipped_duplicates≥2）。
- 失败路径验证结果：symlink/空目录/不可访问目录均有错误路径。
- 未完成项与后续卡：组织级枚举 provider（可选能力）未实现（按卡为可选）。

### 未关闭缺口

- 组织级枚举 provider（可选）未实现

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-035

### 完成记录（2026-09-09）

- 修改文件职责：`src/import/pr.rs`（GitHub 单 provider（gh CLI）；URL 解析校验；PR 元数据+head sha+评审意见 → 本地候选草稿（frontmatter 标注 candidate-unverified / merged / source_pr / head_sha）；PR id+head sha 去重；fork 来源内容按不可信数据（标注）处理；绝不自动评论/发布——评论与提交是后续显式动作）；`pr --action draft`；`tests/import_pr.rs`（fake gh 可执行注入 PATH 测试）。
- 与公共契约的差异：初版仅 GitHub；diff revision 以 head_sha 固定，旧草稿因 sha 变化自然失效（dedup 键含 sha）。
- 验证命令、真实结果及宿主版本：`cargo test` → 163 passed（本卡 1 集成，fake gh 注入）：草稿生成含未验证标注与来源、PR id+sha 去重（第二次 deduplicated=true）、未合并不进入正式知识（仅本地草稿）。
- 失败路径验证结果：非 GitHub URL/非法编号 USAGE；gh 失败 E8102。
- 未完成项与后续卡：真实 GitHub 网络验证未执行（离线环境）；合并后代码知识基线推进由 code build 按 revision 自然处理。

### 未关闭缺口

- 真实 GitHub 网络验证未执行

> 卡片状态为 Done 表示卡面交付边界已完成；上列为仍开放的验证/后续项，不以自动化全绿冒充宿主或发布实机验收。

## AIL-036

### 完成记录（2026-09-09）

- 修改文件职责：`src/import/self_repo.rs`（migrate：copy→校验（子树摘要与源一致）→切换（声明 type=self/path=.ailoom-team）→备份（旧声明存 migrate-backup）；机器数据不迁移）；`src/config.rs`（source.type=self + 相对子树路径校验，空=默认子树）；`src/commands/{sync_core,recall}.rs`（self 模式：源=本 checkout 子树（LocalSource），无需外部锁，业务 HEAD 作可追溯标记；linked worktree 各自消费自己的子树）；`src/import/self_repo.rs::contribute_self`（业务仓隔离 detached worktree，仅提交子树路径，业务分支/HEAD/脏文件全程不变，不自动推送默认分支）；CLI `migrate --from --subtree`。
- 与公共契约的差异：统计独立报告分支在 self 模式复用 AIL-022 报告分支（ailoom/reports-*），不推进业务默认分支（测试断言分支隔离）。
- 验证命令、真实结果及宿主版本：`cargo test` → 165 passed（本卡 2 集成）：迁移（copy≥10 文件+摘要校验+声明备份+业务脏文件/分支/HEAD 全程不变）→ 同仓 sync 部署 → 切换后资源清理与 linked worktree 各自消费（资源写当前 checkout）。
- 失败路径验证结果：迁移校验失败报 INTERNAL；声明备份可恢复（备份目录断言）。
- 未完成项与后续卡：跨文件系统迁移的 rename 非原子场景以 copy+校验替代（无 reset 操作，业务分支不受影响）。

## AIL-037

### 完成记录（2026-09-09）

- 修改文件职责：`src/data.rs`（rotate：events.jsonl 超阈值改名归档（不删除，累计值保留在归档文件，聚合读取全部 events*.jsonl 不丢累计值）；export：事件/摘要/全量聚合快照导出为结构化 JSON（可重新读入审计；事件本不含 prompt 全文）；cleanup：仅清理可重建数据（缓存目录、归档事件），存在未确认上报批次时拒绝（E8001 防丢），dry_run 预览，跨工作区作用域隔离）；CLI `data --action rotate/export/cleanup [--out] [--max-size-mb] [--dry-run]`；`tests/retention.rs`（4 测试）。
- 与公共契约的差异：轮转策略为按大小（时间轮转由归档文件名时间戳承载）；"防止清理尚未确认上传的数据" 以 report-checkpoint pending 批次为门。
- 验证命令、真实结果及宿主版本：`cargo test` → 169 passed（本卡 4 测试）：轮转归档后聚合仍取累计最大值（不丢累计值）、导出 JSON 可重新读入（session-metrics 断言）、存在 pending 批次时 cleanup 拒绝 E8001（含 dry_run）、dry_run 不删除且清单可见。
- 失败路径验证结果：E8001 拒绝路径断言；损坏文件跳过由 store::read_events 坏行计数承载。
- 未完成项与后续卡：默认留存无限期（按需配置阈值）——"不默认为无限留存"通过 rotate 阈值参数（默认 10MB）实现首轮控制。
