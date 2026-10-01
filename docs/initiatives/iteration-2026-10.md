# 2026-10 稳定与收敛迭代

- 起始日期：2026-10-01
- 输入：[2026-10-01 项目审查报告](../reviews/2026-10-01-project-review.md)（两轴代码审查 + CLI / UI / 测试 / 文档全量普查）
- 卡片：AIL-129～AIL-147，状态以 [cards.json](../cards.json) 为准，总表见 [看板](../BACKLOG.md)

## 为什么做这一轮

功能面已经很宽（128 张卡里 122 张标为 Done），但审查暴露了三类系统性问题：

1. **正确性与安全**：未托管 Skill 删除接口能归档任意含 SKILL.md 的目录；团队同步与个人同步互相删除对方的部署；`/api/jobs` 可路径穿越。
2. **门禁失效**：clippy、fmt 在 CI 上必红；MSRV 声明与代码不符；前端测试与文档检查都不在 CI；85 张 Done 卡的必需验收项未勾选，状态纪律名存实亡。
3. **一致性**：CLI 输出与错误码偏离契约；UI 术语、设计 token 与死代码堆积；宿主能力在文档与代码间互相矛盾；文档入口多、过时内容多。

本轮不加新功能，目标是让「标为 Done 的东西真的可信」，并把后续开发的成本降下来。

## 迭代节奏

| 阶段 | 里程碑 | 卡片 | 退出条件 |
|---|---|---|---|
| I0 首批修复 | IT-0 | AIL-129（Done） | 阻断与高危问题修复并有回归测试；fmt / clippy / 全量测试通过 |
| I1 门禁与关卡 | IT-1 | AIL-130～133 | CI 覆盖 MSRV、前端、文档；Done 卡验收项可信；控制台 API 错误结构统一；路由全覆盖 |
| I2 CLI 一致性 | IT-2 | AIL-134～137 | 每个命令的人类输出、JSON 输出、退出码都符合契约并有测试；单一 Web 入口 |
| I3 UI 收敛 | IT-3 | AIL-138～140 | 无死代码；设计 token 无覆盖；390px 可用；术语与 CONTEXT.md 一致 |
| I4 架构与范围 | IT-4 | AIL-141～144，接续 AIL-116、117 | 已实现能力都有卡；单一宿主注册表；console 后端按领域拆分 |
| I5 文档与发布 | IT-5 | AIL-145～147 | 产品文档与现状一致并走通；发布清单可执行 |

阶段之间按依赖推进，不要求严格串行：I1 是其余阶段的前提（门禁不可信时后续验收都不可信）；I2、I3 可并行；I4 的 AIL-141 需要先拿到用户决策。

## 需要用户决策的事项

| 事项 | 背景 | 建议 |
|---|---|---|
| 知识库位置迁移是否在范围内 | `knowledge/location.rs` 会复制并切换知识库位置，与 AIL-110「默认登记原目录，不移动正文」冲突 | 保留为显式高级操作，默认路径不移动；据此修订 AIL-110（AIL-141） |
| Done 卡的验收口径 | 85 张 Done 卡的勾选框为空，交付记录写在卡末 | 勾选框即验收依据；逐卡补勾或改回 In progress（AIL-131） |
| MSRV | 声明 1.75，CI 只跑 stable | 实测最低版本后在 CI 固化（AIL-130） |
| `--json` 错误格式 | 契约 §5 曾有平铺与嵌套两种写法 | 本轮已统一为平铺 + 顶层 `schema_version`（向后兼容）；如需嵌套格式需升版本 |
| Web 入口 | `console` / `web` / `dashboard` 并存 | 只保留 `web` 为用户入口（AIL-137） |

## I0 已完成（AIL-129）

| 问题 | 处理 | 回归 |
|---|---|---|
| AIL-112 删除接口可越权归档 | 删除绑定项目根与服务端扫描结果；令牌绑定项目；内容与链接目标指纹；链接摘除前写恢复记录 | `tests/console_delete_skill.rs` |
| C-01 团队 / 个人同步互删（P0） | 按资源归属分层清理：团队层不清理个人层条目，个人层不清理团队独有条目 | `team_and_personal_sync_do_not_undo_each_other` |
| U-07 `/api/jobs` 路径穿越 | 任务 id 只允许字母数字与连字符 | `load_job_rejects_path_traversal_ids` |
| C-02 select 写入非法值 | 写盘前校验宿主与资源 ID；恢复副本失败改为警告 | `personal_select_rejects_invalid_resource_and_host_before_writing` |
| C-03 本地源 import 必败 | 抽出 `primary_snapshot`，plan / recall / import / contribute 共用 | `import_works_with_local_path_source` |
| C-04 E81xx 退出码 | 按契约返回 18 | `exit_codes_follow_segment_table` |
| C-05 错误 JSON 缺版本 | 平铺字段 + 顶层 `schema_version` | CLI 测试 |
| C-07 人类模式无输出 | library / personal / data 输出摘要或结果 | 手工验证 |
| U-01 新装能力库报错 | 未初始化的资源库视为空 | 手工验证 |
| 错误码码段、`schema_version`、迁移记录吞错 | E6003 / E9101；三个机器文件登记并带版本；记录失败如实告警 | 知识库与服务测试 |
| T-01 / T-03 clippy 与 fmt | 全部修复 | CI 命令本地通过 |

文档同时完成了第一阶段整理：[文档地图](../README.md)、`guide/`（用户手册）、`design/`（设计稿）、`archive/`（历史轮次）；根目录 ORCA 评审移入 `reviews/`；看板总表改由 `scripts/docs_check.py --write` 生成。

## 执行规则

沿用 [开发与关卡流程](../IMPLEMENTATION-GUIDE.md)，本轮补充：

1. 每张卡开工前在 cards.json 与卡头改为 In progress；完成后逐项勾选必需验收并在交付记录中给出证据位置。
2. 修复类问题先写失败的回归测试，再改代码。
3. 每次提交前运行 README「验证」一节的四条命令。
4. 本轮结束时，把本文件移入 `archive/initiatives/`，并在下一轮入口写明遗留项。
