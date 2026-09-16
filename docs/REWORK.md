# 2026-09-15 返工派工单

> 当前执行入口已切换到[2026-09-17需求实现对齐](initiatives/implementation-alignment-2026-09-17.md)与[Web详细蓝图](initiatives/web-console-blueprint-2026-09-17.md)。本文件下方18张RW及38张旧主卡统计仅描述历史批次，不代表当前全局完成；本地控制台039/040/042～050已重新打开。全局状态见BACKLOG/cards.json。

**本轮 18 张子卡已全部完成（18 Done / 0 Backlog）；主卡 35 Done / 3 Blocked / 0 Backlog（AIL-009/012/014 保持 Blocked）。**

- [子卡索引与领取顺序](rework/2026-09-15/README.md) / [子卡机器账本](rework/2026-09-15/cards.json)
- [主卡看板](BACKLOG.md) / [主卡机器账本](cards.json)
- [本轮审查：4 项 Standards + 15 项 Spec](reviews/2026-09-15-agent-implementation-review.md)
- [审查证据](reviews/2026-09-15-evidence/) / [本轮前卡面与状态快照](reviews/2026-09-15-before-rework/README.md)

## 可直接转交的派工文字

> 实现 `docs/rework/2026-09-15/RW-XX.md`。先读 README、CONTEXT、CONTRACTS、IMPLEMENTATION-GUIDE、REWORK、该子卡和父卡原始需求。核对实际代码与前置接口，再领取为 In progress 并填写负责人、代码版本和责任文件。
>
> 你不是独自工作，不回滚他人修改。只拥有子卡分配的文件/函数，共享文件先交接。先复现审查反例，再修复和验证；静态发现若被证伪，保留反证并修正审查结论，不强行实现错误假设。
>
> 每项验收记录真实入口、命令、退出码、关键输出和证据路径。旧 Done、旧次数、测试名称、函数存在都不能代替当前证据。受控本地 fixture 与真实宿主/外部验收分开写。
>
> 更新子卡卡头/子账本/索引及父卡/主账本/看板。父卡关联子卡未全部完成或原始必需验收有缺口时不能 Done。缺少实际依赖/环境时写 Blocked 和解除条件；未实施只是 Backlog。

## 先领取什么

- P1：RW-01 Store 迁移可达性、RW-02 首次 Hook 同步、RW-05 迁移越界写；RW-17 定位测试 fixture 失败，为安装器修复提供可靠验证。RW-17 是验证基础优先级，不把 fixture 故障描述为已确认生产 P1。
- 可独立推进：RW-03 包版本、RW-06 完整会话身份、RW-10 模块符号身份、RW-12 导入目标隔离、RW-14 PR 身份、RW-15 Hook 工作区。
- 按交接推进：RW-17→04；RW-06→07/08→09；RW-10→11，RW-11+14→13；RW-02+15→16。
- 最终：RW-18 收集全部本地修复与当前版本全链路证据。外部必需验收没有解除时不能声称全链路全部完成。

## 共享文件的唯一责任与交接

| 区域 | 分工 |
|---|---|
| paths / 默认 Store 迁移 | RW-01；不让其他卡顺手改变路径优先级。 |
| sync/apply JSON 创建 | RW-02；适配器不另建写入/回滚实现。 |
| tests/installer.rs | RW-17 先修服务夹具并交接，RW-04 再加入升级事务故障场景。 |
| aggregate / dashboard / reporting | RW-06 先冻结身份；RW-07 改生命周期；RW-08 改报告开关/日期；RW-09 消费确认与累计契约。 |
| events/hooks + commands/hook_reg.rs | RW-15 负责 run_hook_cmd 上下文；RW-16 交接后负责 hooks exec 进程/管道。 |
| tests/adapters_next.rs | RW-02 首次 sync、RW-03 packages、RW-16 exec 按函数区分；不全文件重排。 |
| code_knowledge | RW-10 身份→RW-11 内容新鲜度→RW-13 已确认合并快照，不各做一套 revision。 |
| import/pr.rs | RW-14 完整候选身份→RW-13 构图与成功标记事务；tests/import_pr.rs 与 RW-12 按用例区分。 |

## 重新关卡材料

1. 当前代码版本、负责人、责任文件与接口交接记录。
2. 每个审查反例修复前/后真实行为；静态发现的实际复现或反证。
3. 子卡及父卡原始必需验收逐项映射：入口→命令→退出码/输出→证据。
4. 相关回归及 fmt/Clippy；共享核心按需全量，最终 RW-18 必须提交本轮全量结果。测试数由日志统计，不能沿用 253/0。
5. 宿主/provider/平台版本，已验证与未验证边界；任何未完成必需项的阻塞与解除条件。

## 保留与后置

AIL-009/012/014 保持 Blocked，原外部必需验收不取消；009 同时受 RW-01 本地回归影响，不能继续笼统称本地证据完整。其他本轮未发现阻断的主卡保留 Done，不整体重做。

AIL-038 保留脚手架实现，仅重新核对本地准备与当前全量检查声明。AIL-030 仍为设计交付，B-01～B-08 的后端实施保持独立 Open。实际 GitHub Release/npm publish 不属于本轮返工执行要求；不为勾选擅自发布。此派工单仅准备任务，没有发消息或启动其他 agent。
