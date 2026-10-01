# 2026-09-19 UI 业务流程走查（AIL-099～AIL-109）证据索引

本轮按 [走查总纲](../../archive/initiatives/ui-business-walkthrough-2026-09-18.md) 执行 AIL-099～109 十一张卡：先复现、再修复、再以真实浏览器（Chrome CDP，非 DOM 桩）+ 真实 API + 目标文件逐字节核对验收。隔离沙盒：`/tmp/ailoom-flow`（各卡走查，data-root `/tmp/ailoom-flow/data`）与 `/tmp/ailoom-e2e`（端到端，独立实例 :8644）；服务 HOME/GIT 配置/Chrome profile 全部指向沙盒，真实用户宿主配置零改动。

## 环境

- macOS 26 (arm64)，Rust 构建产物 `target/debug/ailoom`，Chrome 153 headless（CDP :9231）。
- 浏览器驱动：仓库自带 `tests/ui_browser.mjs`（current / grouped / cc-switch / design 四种模式全绿）+ 本目录 `cdp-helper.mjs`、`e2e-journey.mjs`（端到端脚本，可重复执行）。
- 后端回归：`cargo test` 全量 43 个测试套件全部通过（含本轮修复的 `sync_common.rs`、`commands/personal.rs`、`console/mod.rs`）。

## 卡 → 证据映射

| 卡 | 关键验收 | 证据文件（本目录） |
|---|---|---|
| AIL-099 | 导航收敛为 [项目, 全局资源中心]+记录分组；流程工作台退为旧路由；操作记录页可读、技术 ID 入详情 | ail099-before-*（复现）、ail099-after-* |
| AIL-100 | 项目头部类型/目录/分支；保存范围（项目默认/仅当前工作树）；本层/有效来源/磁盘分层状态；继承回显与恢复继承；原子写并发修复 | ail100-a-claude-enabled / a2-worktree-override / a1-inherit / status-line / detail-mobile |
| AIL-101 | 添加 Dialog（分组/搜索/默认不全选/同名两来源）、失败注入零写入且勾选保留、移除确认、A2 停用覆盖/恢复继承 | ail101-initial-empty / picker-dialog / save-failed-kept / added-disabled / a2-inherit / a2-disabled-override / confirm-dialog / mobile-picker |
| AIL-102 | 承接未提交的仓库分组与引用 Dialog：grouped/cc-switch harness 全绿；引用 Dialog 双向一致；检查更新/失败逐源回显；来源移除保护；外部来源解除不删原文件 | ail102-grouped-* / reference-dialog / update-available / updated / source-error / remove-confirm / ccswitch-desktop / cc-external-preview |
| AIL-103 | 宿主页检测状态与选用分离（claude 2.1.276 / codex-cli 0.154.0 真实探测）；能力矩阵接入；停用宿主确认；用户级宿主配置零改动 | ail103-host-caps / disable-confirm / a2-inherit |
| AIL-104 | MCP 只读配置 Dialog（stdio/uvx/args/$ENV 引用掩码）；合集 MCP 禁止字面量（E2006 实测）；A/B 独立引用；picker 自动弹出缺陷修复 | ail104-mcp-config-dialog / mcp-page |
| AIL-105 | Agent 行宿主支持标注（codex 官方未确认不可启用）；其他资源按类型策略；package 不可部署且不给写控件 | ail105-agent-row / other-resources |
| AIL-106 | 指令面板项目/范围标注；AGENTS.md 只读基线展示；保存写数据区不改业务仓文件；脏状态保护；恢复继承确认带基线摘要 | ail106-saved / clear-confirm |
| AIL-107 | 预览零写入+逐文件动作清单；应用确认+真实产物核对；精确增删（.mcp.json 保留他服务）；stale 回显；旧计划拒绝；撤销冲突保留；重启持久化 | ail107-preview / applied / stale-row / task-detail / undone |
| AIL-108 | 全站 9 页面 × 7 Tab × 1440/1024/768/390 零溢出（修复 MCP 行 390px 溢出）；键盘完成移除流程；design 模式（样式一致性/combobox 键盘/Dialog 焦点）全绿；补充：scopes/sources/个人副本编辑器/onboarding 六步向导/流程工作台文档流一对一驱动 | ail108-site-desktop / detail-tabs / design-components / regression-desktop + 补充走查九图与脚本 |
| AIL-109 | 端到端：建项目→导入→选宿主→加四类资源→预览→应用→移除→再应用→检查更新→stale→撤销（恢复 4 项）→重启复核；B/C 零影响；无未处理异常 | ail109-projects-registered / preview / applied / stale-by-project-decision / undone + e2e-journey.mjs + console-e2e.log |

## 本轮修复的真实缺陷（均有复现）

1. `sync_common::atomic_write` 临时文件名仅含 pid，并发写同一目标互相 rename 掉对方 tmp 文件 → E4004（项目页并行加载首屏即触发）。
2. console `deploy-status`/`effective` 传 `data_root=None`，layout 解析到 XDG 默认根，managed 索引读空 → 磁盘状态恒为「未部署」（与 CLI 矛盾）。
3. `personal select` 接受不存在的合集资源 ID → 悬空引用 → effective/plan E3004 → 项目页整体不可用且无恢复入口（现：enable/disable 校验存在性，inherit 放行作恢复路径；来源前缀命中登记身份即放行，避免外部/失联来源被误杀）。
4. ResourcePicker 的 Dialog 缺 `open:false`，切到 Skill/MCP/Agent 页自动弹出并惰性拦截行内按钮。
5. Picker 关闭后 DOM 被销毁导致无法二次打开；`canClose` 把「无选中」当「保存中」导致 Esc 关不掉。
6. 操作记录页 onSelect 传入映射行 → 详情全空、撤销按钮永不可用（AIL-099 验收时发现于 AIL-107 真实数据复核）。
7. 项目行操作列 390px 横向溢出（flex-shrink:0 + nowrap 按钮）。
8. harness 断言随状态行改版同步（`当前有效：启用` → 分层状态）。

## 补充走查（第二轮：次级页面一对一）

首轮后自查 scopes/sources/个人副本编辑器/onboarding/流程工作台五处仅渲染级走查，第二轮逐一驱动补齐：scopes 页发现并修复 approveDir 缺失（重启后选目标 403）；库页发现并修复双 `[data-msg]` 碰撞（编辑器消息串位）。证据：ail108b-scopes-applied / sources / copy-editor / copy-conflict / copy-delete-confirm / onboarding-step3 / onboarding-preview / onboarding-applied / workflows 九张截图 + 对应驱动脚本（ail108b/c/d/e）。

## 边界与遗留

- SSH 真实克隆、宿主内真实加载（AIL-077 范畴）、指令按工作树差异与并发版本化冲突（接口缺口已在 AIL-106 卡内如实记录）不在本轮范围。
- 隔离沙盒均在 /tmp，可随时删除；手工复现路径见各卡交付记录与 `e2e-journey.mjs`。
