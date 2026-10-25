---
repo: makecindy/cindy
generated: 2026-10-25
type: implementation-study
source_ref: main
resolved_sha: b7e46bcbbe41eeee2f16381fef00df61a31a3b8b
accessed_at: 2026-10-25
---

# Cindy 如何保存开发知识

**结论：有记录，而且记录了“为何这样做、何时不该统一、旧决定如何退出”。**主要证据是仓内规则、专题台账和修改流程；另有给用户项目使用的 `project-context` 工具。它们与跨会话用户 memory 是三套不同职责。

## 知识放在哪里

| 内容 | 可见入口与边界 |
|---|---|
| 工程、模块知识 | 根 [AGENTS.md L12–33](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/AGENTS.md#L12-L33) 路由到 `docs/dev-rules/` 与模块嵌套 `AGENTS.md`；单模块规则就近放，跨模块长解释集中放。 |
| 业务行为与流程 | [product-rules 收录标准](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/docs/product-rules/README.md#L3-L10) 要求适用场景、不变量、原因、验收及例外；临时待办不升为永久规则。 |
| 现行约束与历史 | [docs 索引](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/docs/README.md#L3-L7) 区分 authoritative 与参考；[设计决策史](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/docs/design-rules/design-decision-log.md#L1-L11) 保存背景、被取代方案及现行落点，冲突时以 `DESIGN.md` 为准。 |

## 是否真记录了取舍、阶段与撤销

有具体实例，而非只有文档模板：

- **看似应统一、实际不该统一：**个人 Telegram bot 的积压消息阈值为 60 分钟；[源码注释](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/lizi-im/src/telegram/index.ts#L106-L118) 解释它为何比官方侧所述 10 分钟宽：桌面关机是预期状态，服务端离线是故障；同时避免跨夜积压集中回复。官方服务端当前实现未跨仓复核。另有[差异台账](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/docs/product-rules/telegram-bot-parity.md#L386-L423)，区分“有意不同／缺口／已同源”，要求写裁决来源，并禁止同一项挂在互相矛盾的类别。
- **阶段改变维护方式：**[共享核心迁移计划](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/apps/mobile/docs/shared-core-migration-plan.md#L201-L212) 明说迁移结束后 shared core 成为跨端语义默认落点，但桌面替换可分期，不强迫同次重写。这是局部阶段合同，不能推断存在全项目统一的“开发／维护模式”。
- **旧知识可退出：**[10-23 决策记录](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/docs/design-rules/design-decision-log.md#L17) 说明旧 Figma 读取记录长期未回写，保留有效内容后删除旧文档；[对应提交](https://github.com/makecindy/cindy/commit/390f9d14b48182ed62ad53877f3c5d4c546ede28) 同时修改实现、现行规范与历史记录。旧知识保留历史可追溯性，不继续充当现行依据。

维护靠行为变更时修正旧结论、链接实现／测试、纠错回写原文、遗漏补入口；[流程](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/docs/dev-rules/development-workflow.md#L125-L133) 明确无变化不必改文档，也不必另写经验总结。[文档检查](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/scripts/__tests__/dev-docs-contract.test.mjs#L141-L180) 能检查命令、重复和链接，不能证明理由仍成立。

## `project-context`：已实现的最小链与限制

这是**用户工作项目**内 `.cindy/project-knowledge/` 的可选知识层，不能据此断言 Cindy 自身靠它维护全部开发知识。

| 环节 | 实际接通的代码；计划／边界 |
|---|---|
| 谁触发生成 | [CLI](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/cli.ts#L16-L78) 提供 init/update/refresh；[init](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/commands/init.ts#L53-L91) 只建模块骨架并写索引，`--bootstrap` 仍[未实现](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/commands/init.ts#L131-L142)。[post-commit／Stop hook](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/README.md#L379-L418) 是手动接入示例；本次检查的四个仓库 workflow 未见该工具调用，未证实默认自动生成。 |
| 知识文档与派生索引 | [模块文件](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/knowledge.ts#L61-L84) 含职责、代码地标、边界、禁忌、演进备忘；[frontmatter](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/knowledge.ts#L106-L123) 记录 covers、同步 commit、stale、auto_update。manifest 重建索引，[TOC](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/toc.ts#L160-L175) 展示路径、摘要及 stale 警告。 |
| 代码变后怎么更新 | 首次正文由 [refresh 的 Claude 适配器](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/adapters/claude-code.ts#L43-L52) 读源码生成。**执行 update 时**按 git diff 与 covers 定位；[默认小改](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/config.ts#L27-L32) ≤5 文件且 ≤200 行则调模型改写，大改／失败标 stale；[实现](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/commands/update.ts#L139-L220) 尊重冻结开关。显式 [refresh](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/commands/refresh.ts#L106-L143) 调适配器读当前源码重建正文，成功清 stale；不是持续监控代码的服务。 |
| Desktop 如何消费 | [会话创建链](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/apps/desktop/src/main/maker-ipc/register.ts#L8352-L8376) 默认尝试本地注入，remote 跳过，[reviewMode 也跳过](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/apps/desktop/src/main/maker-ipc/register.ts#L8902-L8906)。[helper](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/apps/desktop/src/main/maker-ipc/projectContextInject.ts#L64-L85) 只放 TOC 路径，提示相关时再读取；缺失／空文件降级跳过，[测试](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/apps/desktop/src/main/maker-ipc/__tests__/projectContextInject.test.ts#L26-L55) 覆盖短指针与降级。**这里不检查 HEAD／stale，不自动 refresh，也不阻止读取旧知识。** |

跨会话[用户 memory](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/apps/desktop/help-knowledge/memory.md#L7-L35) 保存用户／项目事实与偏好；其开关、重置不能替代开发决策的有效性治理。

## 可迁移的最小做法

来源侧建议：先记录“适用阶段、约束、所选／未选方案及原因、复查条件、当前状态、取代关系”；把现行依据与历史分开；模块范围和同步 revision 可用于提示重新核验。是否增加自动生成，再依据目标系统需要判断。Cindy 的自动改写[只在 diff 意图明显时追加原因，否则略过](https://github.com/makecindy/cindy/blob/b7e46bcbbe41eeee2f16381fef00df61a31a3b8b/packages/project-context/src/commands/update.ts#L33-L46)：**没有记录的业务约束与决策动机，不能靠源码摘要可靠补出。**不宜直接复制其目录规模、审批流程，或把 advisory stale 当成失效门禁。

## 检查范围

固定上述 SHA；递归 tree 18,609 项，`truncated=false`。检查根／模块入口、dev/product/design 规则与实例、mobile 阶段计划、project-context CLI／更新／刷新／TOC／Desktop 消费及核心测试、四个 `.github/workflows`；抽查两次设计变更提交的文件与 diff。完整 tree 未见已提交的 `project-knowledge` 产物，也未见统一 ADR 目录；这不排除本地未提交知识、外部团队资料或其他决策记录。仅静态阅读，未执行来源仓库代码或测试。

## AILoom 对照：目标侧检查与建议

以下是目标侧检查后的适配判断，不代表 AILoom 已实现这些机制。

| 来源机制 | AILoom 当前边界 | 适配建议 |
| --- | --- | --- |
| 业务、开发、设计规则及历史分别维护 | [整理模板](../../src/knowledge/res/organize-wiki.md)已有概念、模块、决策、排障和证据标记，尚未明确要求业务流程与开发／维护阶段的分别判断 | 按材料组织业务规则与异常流程、工程契约和决策演进；对历史选择分别回答当时是否合理、当前是否适用 |
| covers、同步 revision、stale | [采集来源](../../src/knowledge/location/ingest.rs)记录文档的分支／提交；[报告输入](../../src/knowledge/location.rs)指纹并未把主题与代码范围建立失效关系 | 借鉴影响范围与版本证据，设计相关代码变化后的复评；保持陈旧提示与质量验收为不同判断，不照搬文件数／行数阈值 |
| 模块正文与按需目录；显式 refresh | 当前结果是每项目一份 Markdown，来源原文独立保留；[检索](../../src/knowledge/location.rs)仍按文档匹配 | 资料规模增长时可借鉴主题／模块索引与按需读取；自动改写不得替代原始证据，更不能补造从未记录的约束或选择理由 |

本轮没有修改整理程序或宣称新增自动质量门禁。上述边界应进入后续评估：索引能否定位结论与原始依据、失效知识是否仍被当作现行规则、不同阶段的选择是否被错误合并。
