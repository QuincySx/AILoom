# AILoom

AILoom 管理项目与团队共享的 AI 资源、经验及工作记录。

## Language

**Team（团队）**：共享资源和协作约定的一组成员。
_Avoid_: 用一个本地目录代指团队。

**Project（逻辑项目）**：团队定义的业务归属，用来组织相关资源和经验；与成员职能独立。
_Avoid_: projectRoot、当前目录、仓库路径。

**Role（职能角色）**：成员承担的职能及其所需资源，不表达业务项目归属。
_Avoid_: 为每个项目与职能组合创建独立角色。

**Workspace（工作区）**：成员当前工作的一个独立代码工作副本（可以是业务仓根，也可以是仓内子目录）。
_Avoid_: Team、逻辑项目、仓库所有副本的总称。

**WorkspaceBinding（工作区绑定）**：某个工作区与资源源、逻辑项目、职能选择之间的关联。
_Avoid_: 成员参与过的项目名册。

**SkillSource（技能源）**：存放 Skill 的一个 Git 或本地仓库；一个源对应 store 里的一个分桶。
_Avoid_: 把「短名字」或「逻辑项目」当成源身份。

**SourceKey（源键）**：由规范化后的源 identity（URL 或本地路径，不含凭据）经可逆编码得到的分桶目录名。
_Avoid_: 不可逆短哈希当唯一主键、人手起的易撞短名。

**SkillStore（技能库）**：本机用户目录下按 SkillSource 分桶保存的 Skill 实体树（默认 `~/.ailoom/store/<source_key>/…`）；桶内元数据在 `.meta/`，skill 路径相对源仓 skills 根。
_Avoid_: 每个业务仓各复制一份实体、把业务工程塞进技能库、在 store 下再套 `sources/` 分类层。

**Require（选用）**：某个 Workspace（及可选 Agent 人设）声明需要哪些 Skill；部署结果是链接而非再存一份实体。
_Avoid_: 隐式「全仓所有 skill 都装上」。

**Resource（资源）**：可以被共享和发布的技能、规则、文档、Agent 定义或 MCP 服务定义。
_Avoid_: 会话事件、机器缓存。

**ResourceRevision（资源版本）**：一组已确定内容的资源快照。
_Avoid_: 当前业务代码版本、浮动分支名称。

**Learning（经验）**：有来源、主题和共享范围的可复用问题处理知识。
_Avoid_: 全文会话、未经分析的日志；默认不等于分形长文。

**ManagedArtifact（托管产物）**：AILoom 可证明由自己部署和维护的本地资源内容（含其创建的 symlink）。
_Avoid_: 工具目录中的所有文件。

**Session（会话）**：某个宿主工具在一个工作区里的连续交互记录。
_Avoid_: 单次回答、一次工具调用。

**Intervention（人工干预）**：用户打断、拒绝或纠正 AI 的观测记录；部分来自启发式识别。
_Avoid_: 确定的模型错误率、成员绩效。
