# 37 张卡完成度核实与代码审查

日期：2026-09-12。基准：`54a4cac3b31a1f558dd7f84868656123f2129319`，审查当前工作区未提交改动，并按卡片检查现有实现。需求依据为 `docs/cards/AIL-001` 至 `AIL-037`、`docs/CONTRACTS.md` 和 Skill Store 规格。没有远端 issue-tracker 配置，未把外部参考项目当作本项目完成证据。

结论：**不能认定全部完成。** 账本的 37 张卡都为 Done，共 350 个已勾选项、10 个未勾选项；实现中仍有可复现缺陷和未接通的功能，且当前改动未通过全部 CI 检查。后文区分实现缺陷、必要验收缺口和仅设计交付，不把可选功能未实现一概判为缺陷。

本次未修改业务代码或任务状态，未执行真实发布、安装、对外评论或 PR 写入。复现使用临时数据。此报告是基于代码、测试与重点反例的审查，不构成对全部边界的穷尽证明。

## 验证结果

| 检查 | 结果 |
|---|---|
| 原始环境 `cargo test --locked --offline` | 失败：`tests/adapters.rs:107` 硬编码 `.ailoom/store/`，但新增路径逻辑采用继承的 `XDG_DATA_HOME`。41 个单元测试、该测试文件其余 11 个测试通过；Cargo 随后停止。 |
| 清除路径覆盖变量后完整测试 | `env -u XDG_DATA_HOME -u XDG_STATE_HOME -u AILOOM_STORE_ROOT -u AILOOM_DATA_ROOT cargo test --locked --offline --no-fail-fast`：**189 passed，0 failed**。 |
| `cargo fmt --all -- --check` | 失败：`tests/sync_apply.rs:443,475`。 |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 失败：`tests/sync_apply.rs` 中 6 个 `cloned_ref_to_slice_refs` 错误。 |
| 重点反例 | 同文件多片段恢复不完整；日志轮转使正常汇总丢历史；清理包含其他工作区缓存；SSE 无快照；迁移越界覆盖；Hook 事件名/重复/工作区归属；合法标题生成非法 YAML。 |
| 真实宿主、Windows、发布制品、GitHub/npm 线上集成 | 本次未验收，不能以本地测试通过替代。 |

测试隔离问题也是本次改动的回归：多个测试只替换子进程 HOME，没有清除 XDG/AILOOM 路径覆盖变量。因而可能向真实 XDG Store 写入测试资源。应在测试公共入口统一隔离所有路径来源，并测试合法 XDG 配置下的行为。

原始检查日志：`/tmp/ailoom-review-tests-20260912.log`（隔离后的完整测试）、`/tmp/ailoom-review-fmt-20260912.log`、`/tmp/ailoom-review-clippy-20260912.log`。重点 CLI 复现结果：`/tmp/ailoom-review-repros-20260912.json`、`/tmp/ailoom-review-migrate-20260912.json`。临时文件可能被系统清理，关键结果已记入本报告。

## 逐卡结论

“基本成立”表示对应实现和自动化证据可支持核心交付，本轮未发现阻断项；并非宣称所有平台均已验收。“需修复”是现有实现问题；“待验收”不能继续把对应验收项当作已经验证。

| 卡 | 本次结论 | 核实依据 / 未完成部分 |
|---|---|---|
| AIL-001 | 基本成立 | 领域契约、配置/资源校验、resolver 测试。 |
| AIL-002 | CI 未通过 | CLI/工程可编译，当前格式和 Clippy 失败；远端 Actions 成功运行本次未核实。 |
| AIL-003 | 基本成立 | workspace/layout 与 worktree 隔离测试；新增路径解析测试通过，测试环境隔离另需修正。 |
| AIL-004 | 基本成立 | Git 缓存、锁定快照、离线路径有实现及 source 测试。 |
| AIL-005 | 基本成立 | init/status 与绑定测试；当前额外宿主显示/选择有覆盖。 |
| AIL-006 | 基本成立 | 项目/角色选择、资源冲突与 resolver 测试。 |
| AIL-007 | 基本成立 | 计划、漂移/冲突判定及 sync_plan 测试。 |
| AIL-008 | 需修复 | 同一文件多个片段备份互相覆盖，恢复不完整（R01）。 |
| AIL-009 | 待验收 | Skills 落盘/链接有测试，真实宿主加载未验收；相关集成测试受 XDG 影响。 |
| AIL-010 | 待验收 | Rules/docs 渲染存在，真实会话发现和宿主内渲染未验证。 |
| AIL-011 | 待验收 | Agent 渲染存在，实际宿主发现/无副作用调用未验证。 |
| AIL-012 | 待验收 | MCP 配置合并有测试，“两个宿主至少各通过一种连接”没有实机证据。 |
| AIL-013 | 基本成立 | doctor/uninstall 与托管条目清理有对应集成测试。 |
| AIL-014 | 待验收 | 本地裸仓/manual 贡献链路有覆盖，真实 GitHub PR 自动创建未验证。 |
| AIL-015 | 需修复 | 标题未转义可生成非法 YAML；稳定 ID 依赖标题（R05）。 |
| AIL-016 | 基本成立 | 项目隔离召回、索引重建、learnings/recall 测试。 |
| AIL-017 | 待验收 | 内置资源有渲染测试，两宿主真实调用人工验收未完成。 |
| AIL-018 | 需修复 | 注册事件名不符合标准、重复 payload 不去重、忽略 payload.cwd（R02–R04）。 |
| AIL-019 | 需修复 | 纠正配置未使用，也没有实际宿主识别链路；依赖 Hook 漏计问题（R02、R12）。 |
| AIL-020 | 需修复 | Stop 固定查询 `prompt-summary` 而非真实 session，并丢弃提示决策（R11）。 |
| AIL-021 | 需修复 | SSE 更新未接通且初始请求可阻塞；已实测无快照（R08）。 |
| AIL-022 | 需修复 | pending 没有补传消费者、批次 ID 不稳定、digest 仅汇总本工作区（R09）。 |
| AIL-023 | 待验收 | e2e 现有测试通过，但覆盖不了上述实际链路缺陷；真实宿主、浏览器和 Windows 验收未完成。 |
| AIL-024 | 待验收 | 基本登记/查询存在；两个测试只覆盖登记后模拟合并和重复登记，没有证明不同设备并发更新集合不丢失。 |
| AIL-025 | 基本成立 | 扁平多源、标签/排除、身份重复和跨源冲突有测试；嵌套订阅明确未提供，不据此宣称支持递归订阅。 |
| AIL-026 | 需修复 | 符号 ID 不含文件/模块，跨文件同名定义被混为一个（R13）。 |
| AIL-027 | 需修复 | query 不检查图谱是否过期；同名符号污染关系召回；缺少纯文本对比评估断言（R13）。 |
| AIL-028 | 需修复 | 反馈重试累加；归档清单未接入索引，只有草稿/读取函数（R14）。 |
| AIL-029 | 设计交付，待验收 | 卡目标明确为发布前设计；10 个未勾项仍开放。安装脚本另有实质错误（S01–S02），不能视为可发布。 |
| AIL-030 | 设计交付 | 后端架构/API/旅程/迁移文档存在，本卡没有要求实现服务端；后续实施拆卡仍未交付，不应把设计当后端已实现。 |
| AIL-031 | 待验收 | 字面量/秘密引用 Unsupported 边界合理，已有冲突测试；名称含 uninstall 的测试没有执行卸载，不能凭名称证明该验收。 |
| AIL-032 | 需修复 | 注册索引不稳定；执行输出管道不消费可能导致误超时；错误退出契约不一致（R15）。 |
| AIL-033 | 需修复 | 已满足仍执行 npm、未验证精确版本/冲突、返回安装前状态；真实安装尚未验收（R16）。 |
| AIL-034 | 需修复 | 目标项目未校验；去重未区分来源/目标/类型；只有目录入口，单仓/列表入口和删除建议未闭环（R17）。 |
| AIL-035 | 需修复 | 忽略显式 data_root；force-push 不使旧草稿失效、合并后也不刷新旧草稿状态（R18）。 |
| AIL-036 | 需修复 | `../` 子树可覆盖工作区外文件；同仓贡献函数没有调用者且未复制改动（R10）。 |
| AIL-037 | 需修复 | 轮转后正常汇总不读归档；cleanup 包含全局缓存且可能删除未上报历史（R06–R07）。 |

按上表：9 张核心实现基本成立，16 张有实现问题，9 张需要补验收，1 张当前 CI 未通过，2 张按设计交付看待。分类不意味着“基本成立”的卡可脱离下游阻断问题独立宣称整产品完成。

## Standards：当前改动审查

未发现值得单独报告的 Rust 硬规范违背或判断性代码气味。工具能够检查的格式/lint 问题已在验证结果单列。安装脚本有以下 correctness 问题。

### S01 · P1 · 升级校验失败破坏现有安装

位置：[scripts/install.sh:46](../../scripts/install.sh#L46)、第 47–49、58 行。适用于没有 Cargo、走预编译二进制下载路径的用户。

下载直接覆盖现有 `ailoom` 符号链接指向的 `ailoom-$TRIPLE` 文件；本次新增的校验文件下载失败/摘要不匹配分支又直接删除该文件。升级失败后旧版本也不能运行。下载覆盖行为原已存在，新增失败清理进一步造成文件消失。应先写临时文件，校验成功后再替换正式文件。静态调用链核实，未运行用户安装器。

### S02 · P2 · 仅有 sha256sum 的 Linux 无法完成校验

位置：[scripts/install.sh:57](../../scripts/install.sh#L57)。这是基线已有表达式，本次强制校验仍保留。

`/usr/bin/shasum ... | cut ... || sha256sum ... | cut ...` 的第一条管道在 POSIX sh 中取末尾 `cut` 的退出状态。`shasum` 缺失时，`cut` 仍成功，fallback 不执行，`actual` 为空，合法制品也会被拒绝。应显式选择可用工具并检查其退出状态。

## Spec：卡片与现有实现审查

以下是当前实现已有的问题，不把它们全部归因于本次未提交 diff。按能力列出 18 组发现；同组可能包含同一卡的多个未闭环条件。

### R01 · P1 · 同文件多片段无法完整恢复（AIL-008）

需求：“中途失败可重试或恢复”。位置：[src/sync/apply.rs:204](../../src/sync/apply.rs#L204)。

备份名只由目标文件路径生成。对同一个 AGENTS.md 连续写两个片段，第二次备份覆盖第一次保存的原始文件。临时程序复现：第三个目标注入写失败后 recover 报告恢复两项，但 AGENTS.md 仍残留第一片段。需要每个恢复步骤独立备份，或每个文件只保存并恢复一次原始快照，同时校验备份摘要。

### R02 · P1 · 注册 Hook 的事件名不会进入标准聚合（AIL-018/019）

需求：“定义 session-start/prompt/tool/stop 的标准事件”。位置：[src/events/hooks.rs:123](../../src/events/hooks.rs#L123)、[src/events/schema.rs:141](../../src/events/schema.rs#L141)。

注册代码对 `SessionStart/UserPromptSubmit/PostToolUse` 直接 lowercase，生成 `sessionstart/userpromptsubmit/posttooluse`；解析器原样记录 kind，而 aggregate 只处理 `session-start/prompt/tool`。CLI 已复现写出 `type: posttooluse`。应使用明确的宿主事件到标准事件映射，测试执行实际生成的注册命令。

### R03 · P2 · 重复宿主 payload 会重复计数（AIL-018）

需求：“重复payload重复送达聚合幂等”。位置：[src/events/schema.rs:135](../../src/events/schema.rs#L135)。

每次解析都生成新 UUID，dedup_key 为空。相同 Stop payload 连送两次，两次均 `captured=true`，落盘两个事件。现有去重测试是手工构造相同 event_id，未经过实际解析入口。需要由宿主事件身份/稳定字段产生可持久化的去重依据。

### R04 · P2 · payload.cwd 被忽略，可能串工作区（AIL-018）

需求：“事件不串项目”“后台没有继承错误cwd”。位置：[src/events/hooks.rs:36](../../src/events/hooks.rs#L36)。

未显式传 `--root` 时仍用进程 cwd，未读取 payload.cwd。已实测进程位于 A、payload.cwd=B，事件记入 A。应解析 payload 并按明确优先级发现根目录。

### R05 · P2 · 合法经验标题生成非法资源，稳定 ID 也依赖标题（AIL-015）

需求：“标题/正文验证”“稳定ID不依赖标题”。位置：[src/learning.rs:90](../../src/learning.rs#L90)、第 74 行。

输入 `title: 'Fix: cache'` 能被草稿解析，但输出手拼成 `title: Fix: cache`，本项目 frontmatter 解析器报 E3002。贡献链路没有重新验证生成的资源。应使用 YAML 序列化，再按资源契约验证。另 title/body 哈希生成的 ID 在重命名时变化，不能支持同一经验稳定身份。

### R06 · P1 · 轮转使正常汇总丢掉历史（AIL-037）

需求：“日志轮转不丢会话累计值”。位置：[src/data.rs:43](../../src/data.rs#L43)、[src/events/store.rs:53](../../src/events/store.rs#L53)。

轮转仅把 events.jsonl 改名；read_events 只读传入的单文件，report/session/dashboard 仍传 events.jsonl。CLI 复现：轮转前 digest session_count=1，轮转后=0，归档文件仍存在。卡片和注释宣称的“聚合读取全部 events*.jsonl”未实现。原测试直接把归档路径传给 read_events，绕过了错误入口。

### R07 · P1 · cleanup 删除全局缓存，未上报历史也没有完整保护（AIL-037）

需求：“清理A不影响B”“防止清理尚未确认上传的数据”。位置：[src/data.rs:114](../../src/data.rs#L114)、第 120–138 行。

代码遍历 `<data_root>/cache` 全部子目录，没有检查当前工作区拥有者/其他工作区引用。已实测 A 的 dry-run 同时列出 A、B 缓存。`tests/retention.rs::cleanup_only_touches_own_cache_scope` 反而断言两者都应出现，测试预期与卡片相反。归档清理也只检查 pending 是否存在；从未发起上报的历史没有 pending，仍会进入删除清单，且未保存独立累计基线。

### R08 · P1 · 实时看板没有实际 SSE 更新链路（AIL-021）

需求：“事件更新可见”“断线重连能恢复”。位置：[src/dashboard/server.rs:82](../../src/dashboard/server.rs#L82)、第 104–121、233 行。

读取头部的 `lines().take(20).find_map(...)` 不在空行停止，首次 EventSource 请求没有 Last-Event-ID 时会等待更多输入。即使绕过该问题，STATE_VERSION 也从未被事件写入方更新：bump_version 全仓只有定义，且进程内原子变量不能感知另一个 Hook 进程。CLI/socket 复现收到 200 响应头，新增事件后没有 snapshot。需要完整 HTTP 头解析和可感知文件/跨进程更新的订阅机制。

### R09 · P1 · 统计补传、幂等与团队汇总未闭环（AIL-022）

需求：“为batch设置稳定幂等ID”“离线失败后可补传”“生成团队周期汇总”。位置：[src/reporting/mod.rs:87](../../src/reporting/mod.rs#L87)、第 121、140–150、175 行。

每次 push 都生成新 batch_id/分支；pushed_batches 没有用于去重，pending 只有插入和展示，没有重试或清除路径。因此失败后重新 push 只是新建批次，旧 pending 永远保留并持续阻止 cleanup。digest 只读取当前工作区事件，没有拉取/合并报告分支或成员设备汇总。应实现稳定批次身份、发送确认/重试状态机和真实团队汇总入口。

### R10 · P1 · 同仓迁移可越界覆盖，贡献链路没有接通（AIL-036）

需求：“定义资源子树与业务树边界”“业务dirty文件…全程不变”“贡献使用隔离worktree”。位置：[src/import/self_repo.rs:39](../../src/import/self_repo.rs#L39)、第 139–171 行。

目标直接 `workspace_root.join(subtree)` 后 copy，没有先限制为工作区内受控子树，也没有处理已有目标冲突。**CLI 实测 `--subtree ../victim` 把工作区外临时目录的 README 从 `existing business content` 覆盖为 `incoming source`，退出码仍为 0。** 应在任何写入之前验证规范化目标、符号链接和冲突，并使用可恢复的暂存流程。

另 `contribute_self` 没有调用者；它建立干净 HEAD worktree 后立即 add，未把待贡献资源复制进去，分支名也只生成字符串而没有创建。不能以该函数存在认定同仓贡献完成。

### R11 · P2 · Stop 提示查询错误 session，并丢弃结果（AIL-020）

需求：“按干预/工具失败提示总结；每会话最多一次”。位置：[src/commands/hook_reg.rs:66](../../src/commands/hook_reg.rs#L66)。

Stop 固定传入字面量 `prompt-summary`，不是 payload 中的真实 session_id；即使决策返回需要提示，也 `let _ = decision` 丢弃，没有向宿主输出。纯函数阈值测试不能证明用户会收到提示。应把实际 session 和提示输出接通。

### R12 · P2 · 纠正启发式配置是空接线（AIL-019）

需求：“纠正启发式标注且可配置”“可配置时间窗口和关键词”。位置：[src/events/aggregate.rs:148](../../src/events/aggregate.rs#L148)。

HeuristicConfig 被 `let _ = heuristic` 忽略，聚合仅认 dedup_key=correction；实际宿主 payload 解析又总把 dedup_key 设为 None。全仓没有窗口/关键词识别的使用点。应实现受控识别入口及误报测试，或明确降级并取消相应验收勾选。

### R13 · P2 · 代码图谱同名碰撞且不提示过期（AIL-026/027）

需求：“接口位置准确”“每条关系可追溯”“过期图提示重建”。位置：[src/code_knowledge/graph.rs:92](../../src/code_knowledge/graph.rs#L92)、[src/code_knowledge/recall.rs:34](../../src/code_knowledge/recall.rs#L34)、[src/code_knowledge/mod.rs:78](../../src/code_knowledge/mod.rs#L78)。

符号 ID 仅如 fn:helper，query 的 BTreeMap 用该 ID 覆盖不同文件的同名定义，调用边也混在一起，可能返回错误文件位置。query 加载旧图后直接排名，没有检查源码/版本变化；删除或更改源码后仍能返回旧事实且无重建提示。需建立带文件/模块作用域的唯一身份并校验图新鲜度。现有三个图测试也没有提供卡片所承诺的纯文本基线命中/噪声对比。

### R14 · P2 · 反馈幂等和归档索引未实现（AIL-028）

需求：“同反馈重试幂等”“归档后索引移除且可恢复”。位置：[src/knowledge/feedback.rs:52](../../src/knowledge/feedback.rs#L52)、第 138 行。

反馈入口每次调用都累加，没有反馈事件 ID 或窗口去重。archived_ids 只有定义，没有索引调用者，也没有写入归档清单的命令；archive_draft 仅生成建议文档。因而完成记录所说的“归档清单 + 重建索引排除”没有闭环。需要明确反馈身份以及可执行/恢复的归档操作。

### R15 · P2 · 团队 Hook 注册与执行存在多个验收缺口（AIL-032）

需求：“重复注册幂等”“超时回收本次子进程”“不让一次Hook失败破坏宿主任务”。位置：[src/adapters/hooks_team.rs:115](../../src/adapters/hooks_team.rs#L115)、[src/commands/hook_reg.rs:175](../../src/commands/hook_reg.rs#L175)、[src/main.rs:171](../../src/main.rs#L171)。

注册总以现有数组 len 分配键，同一事件多个新 Hook 会获得相同键，重复 sync 又产生新索引，不能保持 Noop。exec 把 stdout/stderr 设为 piped 却不消费，输出超过管道容量的正常命令会阻塞并被误判超时；只 kill 直接子进程也不能回收派生进程。`hooks exec` 错误向上传播，只有单数 `hook` 分支有退出 0 保护。现有测试只做一次注册，没有覆盖这些条件。

### R16 · P2 · 软件包安装不满足锁定、幂等与结果契约（AIL-033）

需求：“固定版本/来源”“依赖已满足不重复执行”“版本冲突可解释”。位置：[src/packages/mod.rs:100](../../src/packages/mod.rs#L100)、第 147–185、205 行。

未验证精确版本，空字符串/范围/tag 可直接进入 dependencies；同包不同版本直接覆盖。即使全部已 satisfied，仍执行 npm install；返回的 packages rows 还是安装前状态，成功后可继续显示 missing。应先校验/合并计划、跳过满足项并重新构建结果。未运行真实 npm 安装。

### R17 · P2 · 导入目标与去重范围不正确（AIL-034）

需求：“验证每个目标归属”“按source identity+revision去重”。位置：[src/import/mod.rs:85](../../src/import/mod.rs#L85)、第 118–137 行。

target 只验证 `project:` 前缀，没有查询绑定/清单中的项目，能够生成不存在项目的候选。checkpoint 仅以内容哈希去重，不含来源、目标项目或资源类型：先导入 A 后把相同内容导入 B/shared 或改为另一种资源，会被静默跳过。还缺少独立单仓/仓库列表入口、源删除建议的实现；这些不能全部归为可选组织枚举缺口。

### R18 · P2 · PR 草稿状态与存储根不可靠（AIL-035）

需求：“force-push使旧草稿失效”“合并后更新对应代码知识”，以及机器数据根可注入契约。位置：[src/import/pr.rs:96](../../src/import/pr.rs#L96)、第 99–110 行。

draft 丢弃传入的 data_root，调用 AppContext::discover(None)，显式 `--data-root` 未生效。head 改变仅新建文件，没有标记旧候选过期；同 head 从未合并变为已合并后，又会因文件已存在提前返回，旧 merged 状态不刷新。文件名也不含 repo，跨仓相同 PR 号和相同 head 前缀会冲突。没有合并事件触发代码图基线更新的入口。应使用完整候选身份并维护可追踪的状态转换。

## 修复与重新关卡的依据

先修复数据/文件边界问题（S01、R01、R06、R07、R10），以及让核心产品链路失效的 Hook、SSE、补传问题；补对应真实入口回归测试，避免继续以底层函数测试替代端到端行为。然后修复其他 R 项和 CI 门禁。

任务账本应把“实现完成”“自动化验证”“宿主/发布验收”分别记录。未验证项取消勾选并分配后续任务；AIL-029/030 保留设计范围，不需要为关卡擅自发布或实现整套后端。

两轴汇总：Standards 2 组发现，最严重为升级失败破坏原安装；Spec 18 组发现，包含文件恢复/越界覆盖、统计丢历史等 P1 问题。**189 个现有测试通过不足以支持 37/37 全部验收完成。**
