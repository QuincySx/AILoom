# 本地管理、Git 模型与 onboarding：需求及回复审查

日期：2026-09-15。审查对象为用户提供的两段需求及 AI 回复；核对当前 c4a56a4 + 工作区返工修改、账本与现有验收日志。本轮不重做全部代码 review，不把新需求当作旧卡已承诺功能；也不覆盖其他 agent 的实现修改。

## 能否使用

可以将已有 CLI 用于受控项目试用；还不能称为用户期待的成熟个人配置管理器。当前主账本在新增本轮卡之前为 35 Done / 3 Blocked；最新执行者日志可汇总出 **286 passed / 0 failed / 1 ignored**。这是有文件证据的测试结果，本轮没有独立重跑，更不代表全部宿主与完整产品体验已验收。上一份审查的 253/4/1 是修复前结果，应按版本区分。

AIL-009/012 的缺口是 AILoom 集成尚未取得必需证据，不能直接定性为“宿主坏了且只能等待”。AIL-014 缺少真实 PR 创建验收。RW-18 声称全完成但又保留未重跑宿主/浏览器等范围，不能据此推导“Claude 侧所有能力完整可用”。本次保留已有状态，新增卡负责新能力和指定宿主复核；旧关卡准确性仍需独立复审。

## 全部需求映射

| 需求 | 现状与缺口 | 本轮卡 |
|---|---|---|
| 管理 grill-with-docs → to-spec → to-tickets → implement | 可分发 skill/doc；没有阶段、产物、版本、关联与进度的工作流管理；不能凭四个名字认定本机有这些文件 | 043、045、049 |
| 管理对齐、规格、票据、实现与验收文档 | doc 是部署副本与索引，learning 是经验；不是所有文档都应作为 learning，也不能反复 sync 覆盖正在编辑的规格 | 045、049 |
| 公司 AGENTS.md 尽量不动，个人调整不进入公司 Git | 当前 rules/docs/builtin 都可能修改入口文件；仅 --no-builtin 不阻止其余写入。还需保护 CLAUDE.md、已有宿主配置和 index | 040、042、050 |
| 每个仓库不同 skill/MCP/宿主/能力 | 有资源选择和适配，但 require 主要是 skills + agent，缺独立 MCP 与通用资源的三态开关、继承解释、实际生效验证 | 041、044、048 |
| Git 身份优先、同仓 worktree 归为一个项目视图 | 有 anchor/workspace 字段，但 anchor 仅缓存；没有本地仓库注册表、完整发现/搬迁/相同远端克隆确认 | 039、040、048 |
| worktree 下子项目额外配置 | 最近声明能独立部署，不代表父子合并。子目录若无自身 .git，build_workspace 当前 is_git=false，anchor 变 nongit | 039、040、044、048 |
| Web 快速设置源、修改资源 | 现有 HTTP 是单工作区 GET 看板，不是源编辑器/配置服务；CLI JSON 是输出格式，不是完整 HTTP API | 043、046、049、050 |
| 极低摩擦 onboarding | 当前暴露团队源清单/绑定/路径等概念；缺个人模式、已有资源导入、无副作用预检、恢复与宿主验证 | 047、051 |

现有资源共九类：skill、rule、doc、agent、mcp、learning、env、hook、package。`agent` 是宿主角色/子代理定义，**不等于 AGENTS.md 指令入口**。targets 除 claude/codex 外已有 extra、alva 和 rules-only registry；覆盖深度应逐宿主×资源×作用域记录，不能说只有两个，也不能说所有宿主都有九类完整支持。证据：src/config.rs、src/adapters/{mod,registry,docs,rules,builtin,skills}.rs。

## 回复中的主要问题

1. **“Git root 优先与子项目配置互斥”错误。** 身份发现和配置作用域查找是两步：先找所属 Git 仓库/工作树，再找仓内已登记作用域。子模块/嵌套独立仓库应在新 Git 边界停止继承。现有最近声明逻辑不是唯一可行实现，更不应让用户在两个合理需求中二选一。
2. **“不关终端就永远不会提交”无可靠依据。** 无法知道另一个 AI 实际用了什么命令。若是 assume-unchanged/skip-worktree，它们是 index 状态，不以终端寿命为契约，也不是可靠的已跟踪文件本地修改保护机制。Git 官方 FAQ 明确反对将它们用于此用途。[Git FAQ](https://git-scm.com/docs/gitfaq#_common_issues)、[update-index](https://git-scm.com/docs/git-update-index)。因此不能把自动 skip-worktree 当核心产品方案。
3. **“ignore 即不提交”必须分清文件状态。** ignore/exclude 对已跟踪文件无效；个人新增文件可用 common-dir/info/exclude 避免普通 add 收入，但仍不是强制 add 或人为复制的安全边界。不能通过 git rm --cached 移除公司已跟踪文件来假装满足“公司仓不动”。[Git ignore](https://git-scm.com/docs/gitignore)。
4. **“Codex 没有项目支持，只能等或改全局”结论过强。** 当前官方列出项目技能目录 `.agents/skills`，AILoom 却使用 `.ailoom/skills` + skills.config；需要检查适配与版本。官方也列出受信项目的 MCP 配置。旧测试失败可证明那次加载失败，无法定位一定是宿主能力缺失。[Skills](https://learn.chatgpt.com/docs/build-skills)、[MCP](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)。
5. **“本地 override”不是跨宿主通用追加。** Codex 同目录 override 优先且每目录至多加载一个文件；只写个人几行会遮掉公司基线。Claude 有单独的本地指令文件并与基线共同加载，语义不同。方案必须逐宿主验证，不能承诺个人“删两行”会从所有指令来源中消除这两行。[Codex 指令发现](https://learn.chatgpt.com/docs/agent-configuration/agents-md)、[Claude memory](https://code.claude.com/docs/en/memory)。
6. **“必须先建团队源”不适合作为首次使用门槛。** 后端可复用 local source/清单，但用户不必创建团队、Git 远端或手写 TOML；可自动建立个人资源库，导入已选本地 skill。源码现有 local/self 支持也说明不必先有远端团队仓。
7. **“JSON envelope 等于 API，所以 Web 很便宜”低估了写入流程。** 复用命令业务逻辑合理，但还缺请求契约、作用域校验、冲突检测、计划版本、任务进度/取消与恢复、并发保存、错误模型和浏览器访问控制。不能把任意 CLI 参数拼接暴露给网页。
8. **“loopback 就足够安全”不适合可写控制台。** 读看板先例不能直接证明可以安全暴露本地修改/启动进程能力。仅允许已登记根、会话校验、Origin/Host 检查、拒绝跨站写入、转义不可信源内容，是新增写接口的验收内容。
9. **“改 project.toml 就能开关所有能力”不准确。** 本地模式根本不应默认改可提交声明；还需显式 disable、继承、源和宿主隔离。AILoom 停止部署某项，也不一定阻止宿主从用户全局目录或祖先目录加载它，界面必须区分“本工具未部署”与“宿主已禁用”。
10. **“doc/learning 能管理整个研发生命周期”混淆资源与产物。** skill 是方法；spec/tickets/implementation notes 是一次工作的产物。需要稳定关联与版本，且默认留在个人数据区，明确导出到公司目录才发生写入。自动运行 agent 或自动关卡不在本轮默认范围。
11. **“worktree list 一条命令就够”只解决枚举。** 它列的是同一 Git 仓库关联工作树，彼此不必位于同一父文件夹；不会枚举磁盘上所有相同 origin 的克隆。还要处理 bare、detached、locked、prunable、失联路径和子模块。`--porcelain -z` 便于无损解析路径。[Git worktree](https://git-scm.com/docs/git-worktree)。
12. **“保留注释就完成配置编辑”不够。** 需要对版本/指纹做并发检测，保护未知字段、用户改动和已有 dirty/staged 内容；源文件保存、当前作用域应用、贡献到团队远端是不同动作。

## 建议的 Git 模型

Git 仓库是配置归组；worktree 是真实落点；子项目是仓内相对路径作用域；Project 保持业务归属，不被 origin 字符串替代。用 Git 元数据识别 common-dir/工作树，不以 `.git` 必须是目录为前提，也不以远端 URL 作为不可变唯一身份。[Git rev-parse](https://git-scm.com/docs/git-rev-parse)。

同一 common-dir 下工作树自动同组；不同 clone 的相同远端仅建议关联，由用户确认；无远端的 Git 仓仍使用 Git 身份，只有明确非 Git 文件夹才退回路径身份。Git 探测错误不能静默当非 Git。路径搬迁更新位置，不重建业务项目或吞并事件历史。

配置建议分层为：团队显式声明 → 个人仓库默认 → 仓库子项目模板 → 当前 worktree 覆盖 → 当前 worktree 子项目覆盖。只有显式值覆盖，资源开关是继承/开/关；同层冲突阻止应用。新 worktree 可继承配置，但“被发现”不等于获得自动写盘授权。事件、锁、journal 仍按工作树/实际作用域隔离。

## Onboarding 的成功标准

第一次不要求理解 team/source/namespace；默认个人本地模式，允许无网络、无远端、没有现成团队源。用户完成“选工作目录 → 看仓库与工作树归属 → 选宿主和能力 → 预览具体改动 → 应用并验证”即可使用。Grill→Spec→Tickets→Implement 作为可选流程包，不自动调用 AI、不自动创建团队远端。

“完成”必须至少有一个宿主真正发现并调用选中 skill，且公司已跟踪文件和 index 不变；MCP 仅在连接及一次无副作用调用通过后标可用。缺依赖可跳过继续配置其他能力，保留准确状态。新手看到的是下一步解决动作，不是退出码、堆栈或大量路径术语。用户可返回修改、中断后继续、一键撤销本工具本次改动。

实现规格与 13 张新卡见 [本地控制台计划](../initiatives/local-console.md)；这些是待实现能力，不是现状承诺。
