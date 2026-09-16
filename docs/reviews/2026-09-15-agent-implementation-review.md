# Agent 本轮实现与 ticket 完成度复审

日期：2026-09-15。固定基线 `54a4cac3b31a1f558dd7f84868656123f2129319`，审查终点 `c4a56a4`；覆盖返工提交 `a3f243f`、路径契约调整 `7c6fdd2`、脚手架/发布准备 `c4a56a4`。沿用上一轮审查范围，135 个文件有变化；开始审查时工作区干净。

**结论：不能接受本轮所有 Done。** 主卡账本为 35 Done / 3 Blocked，共 38 张。按规范与需求两个轴审查，发现 4 项规范/契约问题和 15 项需求实现问题。两轴分别保留，不合并排名。包含旧问题未修完及本轮新增回归，不将每项都归为新增缺陷。

此次只新增审查报告与证据副本，未修改业务实现、卡片状态或 Git 历史。所有主动反例使用临时目录；未进行真实发布、远端推送、消息发送或真实宿主调用。已有宿主记录作为执行者证据阅读，未独立重跑。全卡核对不等于穷尽所有运行边界。

## 验证

| 检查 | 本轮结果 |
|---|---|
| `cargo test --locked --offline --all-features --no-fail-fast` | 253 passed、4 failed、1 ignored；退出 101。4 个失败均为 installer 用例，本地 HTTP fixture 连接失败。 |
| `cargo test --locked --offline --test installer` | 单独重跑仍 2 passed、4 failed，均为相同 fixture 连接失败；不能宣称当前环境全绿。尚未确定 fixture 启动失败根因，不据此推断生产下载器必然失败。 |
| `cargo fmt --all -- --check` | 通过。 |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 通过。 |
| 主动 CLI / socket 反例 | Store 迁移断链、空配置首次 Hook 同步失败、跨工作区 Stop 漏提示、派生进程绕过 Hook 超时、迁移内部 symlink 越界写、会话设备合并、Stop 后状态不恢复、清理后累计丢失。 |
| 未重跑 | ignored 的真实 npm 安装、真实宿主/浏览器、Linux/Windows、远端 Actions 和真实发布。 |

日志保存在本报告相邻 `2026-09-15-evidence/`。现有 2026-09-13 验收不能替代当前 HEAD 的验证，尤其 AIL-038 引用的全量次数没有对应本轮完整日志。

## Standards

### S01 · P1 · 默认根迁移使现有技能链接失效（AIL-003/009）

位置：[src/paths.rs:105](../../src/paths.rs#L105)、[src/sync/apply.rs:383](../../src/sync/apply.rs#L383)。依据：CONTEXT 中 Store 实体与 Require 链接契约、CONTRACTS §3 兼容迁移。

旧 `~/.ailoom/store` 被直接移走，既有绝对 symlink 仍指向旧路径。隔离 CLI 仅运行 `status`，虽然因无工作区退出 10，仍先迁移 Store；旧链接下 SKILL.md 由可读变为不存在，新 Store 内容存在。升级会影响尚未重新 sync 的其他工作区。应在迁移时维持已部署链接可达。迁移清单还未包含 device-id，设备身份连续性需一并核验（附带静态观察，不另计）。

### S02 · P1 · 首次同步团队 Hook 无法创建嵌套配置（AIL-008/032）

位置：[src/sync/apply.rs:604](../../src/sync/apply.rs#L604)。依据：CONTRACTS §6“目标不存在/需要 → 创建”。

`ensure_json_array_at` 为 `/hooks/Stop` 的每层缺省值都创建数组，第一层 hooks 应是对象。真实 `source --minimal` → 添加有效 Stop hook → init → 首次 sync，退出 13，内层 E5004“JSON pointer 父节点不是对象: /Stop”。已有测试提前写入 hooks 对象，绕过失败路径。应仅在叶子创建数组，并补空 settings 文件场景。

### S03 · P2 · 精确版本门禁接受不完整版本并拒绝合法 prerelease（AIL-033）

位置：[src/packages/mod.rs:272](../../src/packages/mod.rs#L272)。依据：CONTRACTS §4.2 version“精确”。静态数据流确认。

`numeric.len() >= 2` 放行 `1.2` 并原样交给 npm；安装后的 `1.2.N` 又不能通过字符串相等检查，导致反复 missing/安装。全字符串禁 x 同时拒绝 `1.2.3-next.1`。应验证完整精确版本，并保持安装与检查的版本语义一致；本次未运行 npm。

### S04 · P2 · 升级后段失败不恢复旧安装（AIL-029）

位置：[scripts/install.sh:114](../../scripts/install.sh#L114)。依据：卡片第 26 行“所有失败分支保留原 binary/链接”。静态控制流确认。

先替换 FINAL，再移动摘要及更新链接；后两步任一步失败，set -e 退出而 trap 只清临时文件。原链接已指向新 binary，旧版本无法回滚。需对末段切换做失败注入并保证事务恢复。此项与本轮 HTTP fixture 失败是不同问题。

未发现需要单列的判断性代码气味；格式和 lint 单列于验证，不当作人工规范发现。

## Spec

### R01 · P1 · 同仓迁移仍能沿内部链接越界写（AIL-036）

位置：[src/import/self_repo.rs:118](../../src/import/self_repo.rs#L118)、[src/import/self_repo.rs:246](../../src/import/self_repo.rs#L246)。需求：“目标或源含越界链接…迁移失败”。

校验只覆盖 subtree 自身与目标叶子，未验证内部祖先。临时 `.ailoom-team/resources → <工作区外>/outside`，源含 `resources/new.md`、外部该文件原不存在；真实 migrate 退出 0，外部 new.md 被写入，声明还切为 self。应在物化前校验每一目标路径的祖先及链接，不仅检查最终文件是否存在。证据 `migrate.json`。

### R02 · P2 · 会话身份在多个层面合并或覆盖（AIL-019/021/022）

位置：[src/events/aggregate.rs:262](../../src/events/aggregate.rs#L262)、[src/dashboard/server.rs:180](../../src/dashboard/server.rs#L180)、[src/reporting/mod.rs:129](../../src/reporting/mod.rs#L129)。需求：“不同 workspace/provider/device 的同名 session 不错误合并”。

聚合按 tool/session 分组，忽略 device；CLI 两设备同名会话实际合成 prompt_count=2 的一条会话。dashboard 又按 workspace/session 合并 provider；report 文件名仅含日期与原始 session 哈希，不同 provider 可插入同一路径互相覆盖（后两条为静态链路）。应统一完整会话身份贯穿聚合、UI、共享记录和去重。

### R03 · P2 · Stop 后继续输入仍永久显示 idle（AIL-021）

位置：[src/dashboard/server.rs:226](../../src/dashboard/server.rs#L226)。需求：“状态按最新事件变化，不因曾经 Stop 永久 idle”。

状态函数忽略传入 events，只检查 stop_count>0。真实 Hook→socket Start→Stop→新 Prompt 后 `/api/state` 仍 idle。应按最新生命周期事件判定。

### R04 · P2 · 关闭上报仍能补传 pending（AIL-022）

位置：[src/reporting/mod.rs:337](../../src/reporting/mod.rs#L337)。需求：“上报关闭无外部写入”。静态调用链确认。

push 分支检查 reporting_enabled，retry_action 不检查，直接 prepare_contribution→push_batch。已有 pending 后关闭开关，再调用 retry 仍会写远端。所有上报写入口应共用开关检查，队列保留待重新启用。

### R05 · P2 · 跨日重报把同一累计会话计入多次（AIL-022）

位置：[src/reporting/mod.rs:119](../../src/reporting/mod.rs#L119)、[src/reporting/mod.rs:482](../../src/reporting/mod.rs#L482)。需求：“同 session 更新不会反复累计”。静态数据流确认。

build_batch 使用上传当天日期作为文件名，digest 将日期放入去重键。隔日重报同一累计 session 形成两份独立汇总项，totals 重复计数。应明确记录是日增量还是累计快照，按相同语义生成与汇总。

### R06 · P2 · 清理确认归档后本地累计指标消失（AIL-037）

位置：[src/data.rs:180](../../src/data.rs#L180)、[src/data.rs:199](../../src/data.rs#L199)。需求：“保存可核查累计与幂等基线”。

确认水位线只保护未上报数据，没有保存累计基线；确认后的日志直接删除，而 metrics 仍只从日志重算。CLI 轮转→确认水位线→cleanup 后，正常 metrics 从一条 session 变成 `sessions=[]`。清理应先持久化并让正常读入口消费累计基线。

### R07 · P2 · 同一文件内嵌套模块同名符号仍碰撞（AIL-026）

位置：[src/code_knowledge/graph.rs:92](../../src/code_knowledge/graph.rs#L92)。需求：“嵌套模块、同名类型/函数…不会静默选错目标”。静态构造反例。

`mod a { fn helper() {} } mod b { fn helper() {} }` 同处 src/lib.rs 时均生成 `fn:helper@src/lib.rs`。本轮文件身份修复未包含模块作用域。应将完整作用域用于符号与关系身份。

### R08 · P2 · 未提交的源码修改/删除不使图谱过期（AIL-027）

位置：[src/code_knowledge/mod.rs:124](../../src/code_knowledge/mod.rs#L124)。需求：“构图后修改/删除源码再 query，不能无提示返回旧文件位置”。静态数据流确认。

新鲜度仅比较源锁和 Git HEAD；修改或删除源码但未提交，两者不变，query 仍称 graph_stale=false。应比较被扫描内容的指纹或等价工作树状态，覆盖非 Git 工作区。

### R09 · P2 · 同一文档跨目标导入覆盖原归属（AIL-034）

位置：[src/import/mod.rs:337](../../src/import/mod.rs#L337)。需求：去重区分来源/目标/类型，换目标不误跳过、内容更新原位处理。静态数据流确认。

checkpoint key 含 target，但生成 name/path 不含 target。先导入项目 A 再 shared，会处理为新候选却写入同一文件名，覆盖原资源归属；两个 checkpoint 仍各自认为已完成。需让目标隔离同时体现在资源身份与落盘路径。

### R10 · P2 · 合并后构图用错项目与 revision（AIL-035）

位置：[src/import/pr.rs:203](../../src/import/pr.rs#L203)。需求：“合并后触发正确项目/revision 构图”。静态数据流确认。

advance_graph_baseline 忽略 project 参数，选绑定列表首项目；同时扫描当前 checkout，却把远端 PR head 填作 revision。多项目工作区或未更新本地代码时，图谱内容与归属/版本证据不符。应选择明确目标项目和已确认合并版本的源码快照。

### R11 · P2 · 构图前先记成功，失败无法重试（AIL-035）

位置：[src/import/pr.rs:175](../../src/import/pr.rs#L175)。需求：失败可恢复/重复导入幂等。

先把 identity 写入 graph-baseline.jsonl，再读取/构建/保存图；任一步失败后重试命中 marker，直接 already-advanced。成功标记应在图保存完成后写入，或用可重入状态机区分处理中与已完成。静态控制流确认。

### R12 · P2 · 更新 PR #1 会错误作废 #10/#11 草稿（AIL-035）

位置：[src/import/pr.rs:132](../../src/import/pr.rs#L132)。需求：“同 PR 旧版本失效”。

文件名 prefix 用 starts_with 匹配，没有 PR 编号结束边界。相同仓库 #1 的前缀也匹配 #10/#11，不同 head 时被改为 candidate-stale。应比较结构化身份或加入明确分隔符。静态字符串反例。

### R13 · P2 · PR 候选使用截断 SHA 作为身份（AIL-035）

位置：[src/import/pr.rs:274](../../src/import/pr.rs#L274)。需求：“候选身份包含 provider/repo/PR/完整 head”。

head 前八位被用于候选/checkpoint 身份；同前缀不同完整 SHA 会误去重，force-push 不能更新候选。完整 SHA 应用于身份，截断仅用于显示。静态构造反例。

### R14 · P2 · Stop 提示又回到进程工作区查询（AIL-018/020）

位置：[src/commands/hook_reg.rs:70](../../src/commands/hook_reg.rs#L70)，自动同步亦在第 34 行重新发现上下文。需求：显式 root > payload.cwd > 进程 cwd；提示对应真实 session/工作区。

事件采集已正确落 B，但后续重新用进程 cwd=A 创建上下文。CLI 在 B 种入两次干预，cwd A、payload.cwd B 的 Stop 无提示；相同 payload 改 cwd B 后立即产生 systemMessage。应沿用采集时解析好的上下文。证据 `hook-repros.json`。

### R15 · P2 · 派生进程持有输出管道可绕过 Hook 超时（AIL-032）

位置：[src/commands/hook_reg.rs:279](../../src/commands/hook_reg.rs#L279)、[src/commands/hook_reg.rs:294](../../src/commands/hook_reg.rs#L294)。需求：超时回收、失败不阻塞宿主。

主子进程结束后轮询退出，再无超时地 join 输出线程；仍持有 stdout/stderr 的孙进程让 read_to_string 等待。真实 Python hook 启动 sleep(2) 子进程后立即退出，timeout_ms=100，CLI 实际等待 2.05 秒并返回 executed=true/timed_out=false。长期派生进程可无限等待。超时应覆盖进程树与输出排空全过程。证据 `hook-repros.json`。

## 逐卡结论

“本轮未发现阻断”仅指当前证据支持相应核心交付，不替代外部验收；有共享依赖问题的卡需跟随修复回归。表中是审查建议，未直接改动 ticket 状态。

| 卡片 | 账本 | 本轮结论 |
|---|---|---|
| [AIL-001](../cards/AIL-001.md) | Done | 本轮未发现独立阻断；路径契约实现问题见 003。 |
| [AIL-002](../cards/AIL-002.md) | Done | 验证未通过：installer 4 项失败，不能记录全量全绿。 |
| [AIL-003](../cards/AIL-003.md) | Done | 需修复 S01：默认根迁移断链。 |
| [AIL-004](../cards/AIL-004.md) | Done | 本轮未发现新阻断。 |
| [AIL-005](../cards/AIL-005.md) | Done | 本轮未发现新阻断。 |
| [AIL-006](../cards/AIL-006.md) | Done | 本轮未发现新阻断。 |
| [AIL-007](../cards/AIL-007.md) | Done | 本轮未发现新阻断。 |
| [AIL-008](../cards/AIL-008.md) | Done | 需修复 S02：嵌套 JSON 首次写入；上一轮多片段恢复已有回归覆盖。 |
| [AIL-009](../cards/AIL-009.md) | Blocked | 保持 Blocked：Codex skills 实机加载缺口；另受 S01 影响。 |
| [AIL-010](../cards/AIL-010.md) | Done | 已有两宿主规则发现记录，本轮未发现新阻断。 |
| [AIL-011](../cards/AIL-011.md) | Done | 已有 Claude agent 调用记录，本轮未发现新阻断。 |
| [AIL-012](../cards/AIL-012.md) | Blocked | 保持 Blocked：Codex MCP 连接未满足验收，不以 Unsupported 替代原必需项。 |
| [AIL-013](../cards/AIL-013.md) | Done | 本轮未发现新阻断。 |
| [AIL-014](../cards/AIL-014.md) | Blocked | 保持 Blocked：真实 GitHub PR 自动创建未验收。 |
| [AIL-015](../cards/AIL-015.md) | Done | YAML/稳定 LearningId 已修复并有测试，本轮未发现新阻断。 |
| [AIL-016](../cards/AIL-016.md) | Done | 本轮未发现新阻断；归档清理后的历史问题另见 037。 |
| [AIL-017](../cards/AIL-017.md) | Done | 已有两宿主 CLI 召回证据，本轮未发现新阻断。 |
| [AIL-018](../cards/AIL-018.md) | Done | 需跟进 R14：payload 工作区只在采集阶段生效。 |
| [AIL-019](../cards/AIL-019.md) | Done | 需修复 R02：设备身份隔离不完整。 |
| [AIL-020](../cards/AIL-020.md) | Done | 需修复 R14：Stop 上下文错误。 |
| [AIL-021](../cards/AIL-021.md) | Done | 需修复 R02/R03：会话身份和生命周期状态。 |
| [AIL-022](../cards/AIL-022.md) | Done | 需修复 R02/R04/R05：身份、关闭后补传、跨日重复累计。 |
| [AIL-023](../cards/AIL-023.md) | Done | 不能关卡：关键反例仍失败，本轮全量测试也未全绿；Windows 仍未验证。 |
| [AIL-024](../cards/AIL-024.md) | Done | 本轮新增并发名册测试存在，未发现新阻断。 |
| [AIL-025](../cards/AIL-025.md) | Done | 本轮未发现新阻断。 |
| [AIL-026](../cards/AIL-026.md) | Done | 需修复 R07：同文件模块作用域碰撞。 |
| [AIL-027](../cards/AIL-027.md) | Done | 需修复 R08：未提交源码不触发 stale。 |
| [AIL-028](../cards/AIL-028.md) | Done | 反馈去重与归档/恢复已有真实入口测试，本轮未发现新阻断。 |
| [AIL-029](../cards/AIL-029.md) | Done | 需修复 S04；安装器测试当前失败，真实发布继续后置。 |
| [AIL-030](../cards/AIL-030.md) | Done | 设计交付可保留：B-01～B-08 子卡与开放问题已补齐；不代表后端实现完成。 |
| [AIL-031](../cards/AIL-031.md) | Done | 卸载托管 env/保留用户值新增断言存在，本轮未发现新阻断。 |
| [AIL-032](../cards/AIL-032.md) | Done | 需修复 S02/R15：首次同步及派生进程超时。 |
| [AIL-033](../cards/AIL-033.md) | Done | 需修复 S03：精确版本校验。 |
| [AIL-034](../cards/AIL-034.md) | Done | 需修复 R09：跨目标资源覆盖。 |
| [AIL-035](../cards/AIL-035.md) | Done | 需修复 R10～R13：项目/版本、失败重试、PR身份。 |
| [AIL-036](../cards/AIL-036.md) | Done | 需修复 R01：内部 symlink 越界写。 |
| [AIL-037](../cards/AIL-037.md) | Done | 需修复 R06：清理后累计基线丢失。 |
| [AIL-038](../cards/AIL-038.md) | Done | 脚手架 3 个测试通过；release 仅静态检查，未运行矩阵；全量通过声明应更新为当前结果。 |

## 证据与复审要求

证据附件包含标准轴详细反例、需求轴补充说明、完整检查日志及主审 CLI 输出。静态发现已经逐项标明，不声称全部都有自动化复现。临时 fixture 可能被系统清理，关键触发条件与结果已保存在本报告。

返工应按相应责任文件修复，并让现有真实入口测试覆盖上述反例；先解决迁移、首次同步与隔离身份，再复验依赖链。无需为关卡执行未授权的真实发布。重新关卡必须提交当前代码版本、对应反例结果及完整检查输出，不能只更新勾选和测试次数。

本轮计数：Standards 4 项，最高 P1（迁移断链、首次同步失败）；Spec 15 项，最高 P1（同仓迁移越界写）。
