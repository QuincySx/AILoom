---
repo: yetone/magpie
generated: 2026-10-25
type: implementation-study
source_ref: main
resolved_sha: db6d1540ef18f4bdc408a5fd4937e7ffa68fb376
accessed_at: 2026-10-25
---

# Magpie 如何保存开发知识

固定上述提交研究三个问题：知识放哪里；理由、约束和变化怎样留下；哪些最小机制可借鉴。

## 入口与分工

开发知识由仓库文件及 Git 历史承载。[AGENTS.md](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/AGENTS.md#L3-L23) 要求改代码前读模块参考、规范和教训；[CLAUDE.md](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/CLAUDE.md#L1-L2) 引用同一入口。

| 材料 | 保存的知识 |
| --- | --- |
| `docs/subsystems/` | 跨包职责、运行路径、状态、约束、事实来源与验证；一个模块可跨仓库 |
| `LESSONS.md` | 合入后发现的问题：规则 → 为什么 → 提交/Issue 证据 → 出现日期 |
| `docs/code-standards.md` | 从反复问题提炼的审查规则、验收和发布流程 |
| Issue/PR、提交正文 | 提案、前后行为、验证记录；保留修改过程 |

以上是开发者知识；产品的 [Library](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/docs/subsystems/library.md#L1-L10) 向用户的各个 Agent 分发指令、技能等配置；[gateway hooks](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/docs/subsystems/gateway-request-hooks.md#L111-L113) 提到 PowerContext recall/capture 集成。它们不证明本仓库的开发决策存于产品知识库。

## 怎样维护并修正

[模块维护协议](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/docs/subsystems/README.md#L21-L46) 规定：契约变化同 PR 更新所属参考；事实只放一处，链接源码和符号；现状与提案分开；PR 写触发条件和前后行为，审查者沿真实调用路径核验。纯内部重构无需改写文档。[提交规范](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/docs/code-standards.md#L197-L214) 要求验证留在提交正文，便于后来维护者追查。

[LESSONS 自述每晚复盘](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/LESSONS.md#L4-L19)，不同日期累计三次的教训进入规范。实际 [8731dd9 提交](https://github.com/yetone/magpie/commit/8731dd9f386f8ab4e0c02c41e804df00bcf25ec9) 同改两份文件，把四项教训迁入标准，留下迁移说明。这证明有实践样本；本次未验证复盘是否自动运行。

## 理由与阶段变化的实例

- **看似更省却不默认采用：** [Claude bridge 参考](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/docs/subsystems/claude-subscription-bridge.md#L163-L195) 同记默认保留原系统提示、缩短选项、作者标注的封禁风险，以及特定版本实测节省约 0.9–1.2K tokens/请求。轻客户端省 35%，Claude Code 型客户端约 4%；还注明缓存标记的例外。相关 [提交正文](https://github.com/yetone/magpie/commit/ec64543952a67d12193828492ab30e323a54a2ca) 保存测量条件与检查，[测试](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/internal/gateway/claude_lean_prompt_test.go#L79-L144) 覆盖例外和切换状态。风险是来源作者的判断，本研究未核验厂商政策。
- **阶段改变旧选择：** [9fcf6a6 提交](https://github.com/yetone/magpie/commit/9fcf6a6e7fd18601ed08622c19d39e24eb08dc0c) 记录维护者要求逐步退役内置 Provider、插件已有对等能力，因此撤掉回迁入口，并同改指引、模块文档及测试。之后源码按退役状态把失败重试从六小时改为十分钟：[retryWait](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/internal/provider/migrate.go#L70-L85)。知识包含成立条件，旧方案随阶段变更。

## 可借鉴与边界

**推断：** 最小可迁移单元是“入口索引 + 所属模块的现行契约/理由 + 有证据的教训 + 随变更更新及审查”。对影响后续判断的选择，补齐背景、候选方案、约束、选择理由、证据和重新评估条件即可；不必复制其高频夜审或三天晋升阈值。本报告不判断 AILoom 的具体落点。

所读入口未定义统一 ADR 编号、`superseded` 状态或复审期限；旧结论主要靠改现行文档与 Git 历史追溯。也会漂移：[迁移表](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/docs/subsystems/provider-plugins.md#L129-L145) 的 `failed` 仍泛称内置继续服务，而同页及[源码](https://github.com/yetone/magpie/blob/db6d1540ef18f4bdc408a5fd4937e7ffa68fb376/internal/provider/migrate.go#L289-L317) 已说明退役账户无人服务。因此文档是待核验的上下文，不能替代实现证据。

检查范围：GitHub API 固定 SHA 的递归树完整（6,418 项，`truncated=false`）；读取根指引、LESSONS、规范、contributors、subsystems 索引及 provider-plugins/library/Claude bridge/routing/request-hooks、session-relocation，抽查上述源码/测试与按该 SHA 限定的文件历史。历史为定向样本，未穷尽全部 PR/Issue，也未执行 Magpie 代码或测试；未检查外部私有知识和聊天记录，不能据此断言项目不存在其他记录。

## AILoom 对照：目标侧检查与建议

以下是本项目的适配判断，不是 Magpie 已实现的 AILoom 功能。

| 来源机制 | AILoom 当前边界 | 可迁移部分 |
| --- | --- | --- |
| 模块参考记录现行契约、理由及证据 | [整理模板](../../src/knowledge/res/organize-wiki.md) 已有主题、决策背景、约束和来源要求，结果仍是每项目一份报告 | 先把业务规则、业务与开发流程、模块职责及决策分清；对抉择保留适用阶段与重评条件，不必立即复制目录结构 |
| 教训带 why 和证据，成熟后进入规范 | 原文与版本由[采集模块](../../src/knowledge/location/ingest.rs)保存，尚无独立的知识审批/晋升状态 | 经验晋升为约定应核对来源、范围与证据；重复次数仅作为复核线索，不单独证明正确 |
| 契约变更同 PR 维护知识 | [发布流程](../../src/knowledge/location.rs)的指纹校验防止整理期间库内容变更；[生成校验](../../src/knowledge/organize.rs)尚不判断知识的语义正确性或生成后代码变化 | 后续应建立受影响知识的复评入口，分别判断“当时为何成立”和“现在是否仍适用”；这不是现有自动门禁 |

开发知识的范围已写入 [项目术语](../../CONTEXT.md)。上述建议需用包含业务流程、阶段变化、冲突和旧方案被替代的实际资料验收，不能仅以提示词包含这些词语判定合格。
