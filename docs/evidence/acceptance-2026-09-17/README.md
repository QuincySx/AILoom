# 2026-09-17 返工轮验收证据索引

本轮对应 [实现复审](../../reviews/2026-09-16-local-console-review.md) 的 S01–S04、Spec 轴 F01–F09、专项 Library/Workflow 缺陷与 UI U01–U04 的修复，按 [执行入口](../../initiatives/implementation-alignment-2026-09-17.md) 的 AIL-052～098 卡组织。本目录只收录本地 CLI/HTTP 层证据；真实宿主证据归 AIL-077，真实浏览器证据归 AIL-078/098，三者不互相替代。

## 证据文件

| 文件 | 内容 |
|---|---|
| `regression-tests.log` | 逐卡回归 + 真实宿主（AIL-077）+ skills.sh 真实导入（AIL-065）+ 真实浏览器走查（AIL-078/098）证据 |
| `full-gate.log` | fmt/clippy/全量测试输出（本轮最终 gate 见下） |
| `browser-onboarding-applied.png` | 真实浏览器全页截图：六步向导应用成功后的验证面板（含「需新会话」状态与撤销入口） |

环境：macOS 26 (arm64)，Rust stable；全部测试使用临时目录 + 隔离 HOME/XDG/data-root + 本地 `git init` 夹具；未触碰真实用户宿主配置、未发起网络发布。

## 卡 → 证据映射

每条映射为「验收项 → 真实入口 → 回归测试（均在 regression-tests.log 中可见）」。

| 卡 | 验收要点 | 回归测试（真实入口） | 结果 |
|---|---|---|---|
| AIL-052 | 托管入口后来被跟踪：计划不得转为 delete；apply/uninstall 拒绝改写；index/文件逐字节不变 | `tests/rework_2026_09_17.rs::ail052_tracked_managed_entry_not_deleted`（CLI）；uninstall/recover 共用 `path_is_git_tracked` 守卫（src/commands/uninstall.rs、src/sync/apply.rs 回滚语义说明见代码注释） | ✅ |
| AIL-053 | 应用后用户手改 → undo 冲突保留；可重入；重启后可继续撤销 | `::ail053_undo_preserves_user_edits_and_is_reentrant`；`tests/console_server.rs::ail072_undo_conflict_and_restart_recovery`（真实 TCP 服务重启） | ✅ |
| AIL-054 | 越界导出拒绝；预览指纹前置；覆盖前备份；外部编辑不丢 | `tests/console_server.rs::ail054_export_boundary_fingerprint_and_backup`（HTTP） | ✅ |
| AIL-055 | A 已有配置、在 B select 只写 B；目标贯穿 CLI/API/任务输出 | `::ail055_select_targets_explicit_repo`（CLI 双仓库）；`/api/profile/select` 强制 `root`（console_server 用例） | ✅ |
| AIL-056 | 拒绝独立仓库重关联；整仓搬迁（relink+别名）恢复身份/配置；prunable 可见 | `::ail056_relink_validates_identity_and_preserves_config`；`src/repo_registry.rs::prunable_state_visible_when_worktree_dir_missing` | ✅ |
| AIL-057 | 非 Git 目录 select/effective/plan/sync 闭环；Git 化后迁移提示不默默合并 | `::ail057_nongit_path_mode_closed_loop`（CLI） | ✅ |
| AIL-058 | 连续单项工作树选择互相保留；inherit=删除显式项 | `::ail058_sequential_worktree_selections_merge`；`tests/personal.rs::ail051_restore_inheritance_via_inherit_state` | ✅ |
| AIL-059 | 根→子项目→根 两种顺序一致；父子不互删；子项目落点为子项目目录 | `tests/personal.rs::ail044_personal_selection_layers_end_to_end`（更新为 AIL-059 语义） | ✅ |
| AIL-060 | 注释/未知字段保留；并发保存冲突；effective/plan 携带 revision | `::ail060_profile_format_preserving_and_conflict` | ✅ |
| AIL-061 | 个人禁用宿主后团队 artifacts 不再部署；恢复继承回到团队值 | `::ail061_disabled_host_blocks_team_artifacts`（CLI + 团队源夹具） | ✅ |
| AIL-062 | frontmatter skill 导入成功且 YAML 合法；事务导入；坏条目不锁死列表 | `::ail062_import_transaction_and_tolerant_list`（CLI） | ✅ |
| AIL-069 | missing_env_refs 进 plan notes；秘密不回显明文；$ENV 引用可保存 | `::ail069_mcp_missing_env_ref_surfaces_in_plan_notes`（CLI）；`tests/console_server.rs::ail069_mcp_secret_boundary_in_editor`（HTTP） | ✅ |
| AIL-070 | base_version 冲突；历史版本可恢复；稳定 ID 改名不丢关联 | `::ail070_artifact_history_recoverable`；`src/workflow.rs::create_bind_put_and_spec_review_flow` | ✅ |
| AIL-072 | 服务重启后持久化任务恢复可操作；中断任务标记不重放；undo 重启后可执行 | `tests/console_server.rs::ail072_undo_conflict_and_restart_recovery`（真实进程 stop/start） | ✅ |
| AIL-076 | 替代视图含未调整公司基线；基线更新后重新合成；个人补充保留 | `::ail076_baseline_change_resynthesizes_view`（CLI + git 夹具） | ✅ |

## 真实宿主与真实浏览器（2026-09-17 补充）

- **AIL-077（真实宿主）**：Claude Code 2.1.272 新会话真实调用 AILoom 部署的探针 skill，返回独有标记 `MARKER-AIL-077-X7Q`（部署→发现→调用全链路，非仅文件存在）。Codex CLI 0.154.0 部署侧通过（`.agents/skills` 落盘），调用验证因隔离环境缺 OpenAI 凭据 401 → 按卡协议记录 Blocked（不冒用用户真实账号）。
- **AIL-078/098（真实浏览器）**：ZCode 内置浏览器真实驱动（非 DOM 桩）：八页面渲染走查、六步向导完整闭环（批准→识别→宿主探测→能力保存→预览→应用→验证→撤销）、任务中心/来源/流程页面渲染。浏览器发现并修复 3 个真实缺陷（query 百分号解码、DataTable loading 合并、模块导出错误）。剩余：多宽度截图/键盘断言/双上下文冲突取证（078/098 保持 In progress）。

## 边界与保留

- 以上均为本地 CLI/HTTP 层证据。「宿主真实加载」「真实浏览器操作」「三名试用者人工反馈」分别由 AIL-077、AIL-078/098、AIL-051 负责，本轮未宣称完成。
- 复审报告的原始失败证据保留在 `docs/evidence/review-2026-09-16/`（基线 edc48bd）；本目录证明同一反例在当前工作区版本通过。
- U01–U04 的页面行为已由后端三态/root 失效契约与功能修复支撑；按蓝图，页面层重构（AIL-080–098）完成前不冒充浏览器验收。
