# AILoom 公共契约 v1（已冻结）

冻结卡：AIL-001。冻结日期：2026-09-09。
本文件由 v0 草案冻结而来：草案中的数据方向、不变式、冲突矩阵全部保留并补全为可实现格式。**AI 不得把任何“示例/草案”当作已发布兼容协议；v1 之后的每次变更必须同步受影响卡片并在本文件记录变更条目。**

有效/无效示例统一放在 `docs/fixtures/contract/`（`valid/`、`invalid/`），由解析器测试逐个断言（测试随 AIL-006 落地，见文末“fixture 验证矩阵”）。

## 0. 版本与兼容规则

| 文件 | 格式 | `schema_version` | 兼容规则 |
|---|---|---|---|
| 团队源清单 `ailoom.toml` | TOML | 整数 `1` | 未知版本拒绝（`E3001`）；v1 内新增可选字段向后兼容；删除/改义字段必须升版本 |
| 项目声明 `.ailoom/project.toml` | TOML | 整数 `1` | 同上 |
| 本机绑定 `.ailoom/machine/binding.json` | JSON | 整数 `1` | 未知字段忽略（向前兼容），未知版本拒绝 |
| 源锁 `.ailoom/machine/sources.lock.json` | JSON | 整数 `1` | 同上 |
| 托管清单 `.ailoom/machine/managed-manifest.json` | JSON | 整数 `1` | 同上 |
| 事件文件 `.ailoom/machine/events/events.jsonl` | JSON Lines | 每行 `schema_version` | 未知字段忽略；未知版本该行拒绝并计数，不中断后续行 |
| 命令 JSON 输出 envelope | JSON | 顶层 `schema_version` | 消费方必须忽略未知字段 |
| 项目知识库指针 `.ailoom/knowledge.json`（v1.4） | JSON | 整数 `1` | 只含项目 UUID 与知识库寻址信息，不含正文；未知版本拒绝（`E3001`），见 [KNOWLEDGE-PORTABILITY](guide/KNOWLEDGE-PORTABILITY.md) |
| 知识库登记 `<data_root>/knowledge/bindings.json`（v1.4） | JSON | 整数 `1` | 缺省按 1 读取；未知版本拒绝（`E3001`） |
| 网页服务运行态 `<data_root>/service/runtime.json`（v1.4） | JSON | 整数 `1` | 缺省按 1 读取；版本不符视为状态无效（`E9101`） |

约定：机器可读文件统一带 `schema_version`；JSON 输出走顶层 envelope（`{"schema_version":1,"result":<类型>,...}`）；日志一律走 stderr；人类文本走 stdout。

## 1. 领域模型（术语见 CONTEXT.md）

- **Team**：由团队资源源仓库承载，清单中为 `team_id`（非空、`[a-z0-9-]{1,64}`）。
- **Project（逻辑项目）**：清单 `[projects.<id>]`，`id` 同 team_id 字符集。与仓库身份、checkout 路径**互相独立**（见 §3）。
- **Role（职能角色）**：清单 `[roles.<id>]`。与 Project 正交。
- **Workspace**：一个 checkout/worktree，由 `workspace_root`（绝对路径）标识。
- **WorkspaceBinding**：一个 workspace 对（源， projects[]， roles[]， targets[]）的选择。projects 允许 **0..n 个**；roles 允许 **0..n 个**。
- **ResourceId**：`source/kind/namespace/name` 四段。`source` 是源别名（binding 中定义，非 URL）；`kind ∈ {skill, rule, doc, agent, mcp, learning, env, hook, package}`；`namespace`/`name` 为 `[a-z0-9][a-z0-9._-]{0,63}`，name 在（source,kind,namespace）内唯一。字符串形式四段以 `/` 连接；段内禁止 `/`。
- **Revision**：一个源的 resolved commit（Git 源）或内容摘要（本地源），配 `content_digest`。**资源 revision ≠ 业务代码 HEAD。**

**目标路径冲突独立于 ResourceId 冲突**：两个不同 ResourceId 渲染出同一目标路径也是冲突；同一 ResourceId 渲染到多个目标（多工具）合法。

## 2. 选择模型（不变式，v1 冻结）

1. 角色 ∪ 项目，**取并集**；不存在 project 覆盖 role 的隐式优先级。
2. 资源元数据：`shared`（bool，显式）、`projects`（数组）、`roles`（数组）。选中条件：`shared==true` **或** `projects ∩ 活跃projects ≠ ∅` **或** `roles ∩ 活跃roles ≠ ∅`。三者同时为空/false 的资源属于清单错误（`E3005`）。
3. `learning` 的 `projects` **至多一个**项目；未标 shared 且无项目 → 清单错误。learning 选择只看 shared/项目，**roles 不参与**。
4. 未绑定任何项目的工作区只得到 `shared==true` 的资源；不因“用户没选项目”下载全部项目资源。
5. `namespace` 必须在清单 `[namespaces]` 声明；未声明的 namespace 引用失败（`E3004`）。`namespaces.shared = [...]` 列出公共 namespace；该列表仅用于清单校验与文档，资源是否共享仍以资源自身 `shared` 字段为准。
6. **MCP/Agent 的项目筛选（`projects`/`roles`/`shared` 字段作用于 agent、mcp、env、hook、package 资源）是 AILoom 独立设计**，不是参考实现（TeamAI）的既有等价能力；能力矩阵中相应行标 `ailoom-design`。
7. namespace 过滤是分发/检索相关性隔离，**不是 RBAC**；同一 Git 仓库的读取权限不因 namespace 细化。

## 3. 工作区与数据分区（v1 冻结）

| 概念 | 字段 | 用途 |
|---|---|---|
| Repository anchor | `repository_anchor`（规范化 `git://` 形式的远端 URL；无远端时为主 worktree 的 `.git` 绝对路径哈希） | 仅用于共享缓存 key |
| Workspace root | `workspace_root`（绝对、已规范化符号链接的当前 checkout） | 资源写入落点、`.ailoom/` 位置 |
| Workspace id | `workspace_id` = sha256(workspace_root)[0..16] 十六进制 | 机器数据分区、事件归属 |

三者是**独立字段**：同一 anchor 可对应多个 workspace（worktree）；同一路径在不同机器上 workspace 一致但 device 不同。

机器数据根与 SkillStore 根（全部可注入）解析优先级（高 → 低，v1.1）：

| 根 | 1 | 2 | 3 | 4 |
|---|---|---|---|---|
| `<data_root>` | `--data-root` | `$XDG_STATE_HOME/ailoom`（仅当变量已设且非空） | `AILOOM_DATA_ROOT` | `$HOME/.local/state/ailoom` |
| SkillStore | — | `$XDG_DATA_HOME/ailoom/store`（仅当变量已设且非空） | `AILOOM_STORE_ROOT` | `$HOME/.local/share/ailoom/store` |

空 XDG 变量不算已设。无任何覆盖时按 XDG 规范默认落盘；兼容迁移：旧默认 `~/.ailoom` 存在时，首次解析一次性把 `store` → `~/.local/share/ailoom/store`、`ws`/`cache` → `~/.local/state/ailoom`（其余内容如安装器的 `bin/` 原地保留）；任一步失败整体回滚并继续使用旧目录（数据保持可达，告警提示）。

```
<data_root>/cache/<source_identity_hash>/      # 源缓存，按源身份隔离
<data_root>/ws/<workspace_id>/binding.json     # 工作区级运行态副本（声明在 .ailoom/ 的为可提交副本）
<data_root>/ws/<workspace_id>/managed-manifest.json
<data_root>/ws/<workspace_id>/journal/
<data_root>/ws/<workspace_id>/events/events.jsonl
<data_root>/ws/<workspace_id>/index/           # 知识索引
<data_root>/ws/<workspace_id>/summary/         # 会话摘要
```

工作区内可提交区（留 Git，不含机器路径/秘密）：

```
.ailoom/project.toml        # 项目声明
.ailoom/README.md           # 说明（可选）
```

## 4. 文件格式 v1

### 4.1 团队源清单 `ailoom.toml`（源仓库根）

```toml
schema_version = 1
team_id = "example-team"

[projects.a]
name = "Project A"
[projects.b]
name = "Project B"

[roles.dev]
description = "Developer"
[roles.pm]
description = "Product manager"

[namespaces]
known = ["common", "team-lib"]   # 全部合法 namespace
shared = ["common"]              # 其中显式公共者

[paths]        # 各 kind 的资源根目录（相对源根；缺省用默认值）
skills = "resources/skills"
rules = "resources/rules"
docs = "resources/docs"
agents = "resources/agents"
mcp = "resources/mcp"
learnings = "resources/learnings"
env = "resources/env"
hooks = "resources/hooks"
packages = "resources/packages"
```

- `paths.*` 缺省值即上表右侧；显式值不得含 `..`、不得为绝对路径、不得含 `~`（`E3007`）。
- projects/roles/namespaces 至少声明 names（可为空表但键必须存在）。

### 4.2 资源条目

**skill**：`<skills_dir>/<name>/SKILL.md`，YAML frontmatter：

```yaml
---
name: common-greet          # 必须等于目录名
description: 问候示例
shared: true
projects: []                # 与 roles/shared 组合受 §2 约束
roles: []
namespace: common
---
```

目录内 `scripts/`、`references/`、`assets/` 及任意引用文件随目录整体复制；禁止绝对路径引用与外跳 symlink。

**rule / doc / learning**：`<rules_dir|docs_dir|learnings_dir>/<name>.md`，frontmatter 同上；learning 另支持 `source_ref`（出处引用，字符串，可空）、`project`（0/1 个项目 id）代替 `projects` 数组。learning 的 `shared: true` 与 `project` 互斥。

**agent**：`<agents_dir>/<name>.toml`：

```toml
name = "recall"
description = "按需召回团队经验"
instructions = """..."""     # 多行字符串
model = "inherit"            # 保留宿主语义的字符串
tools = ["Bash", "Read"]     # 允许为空数组 = 不限制
shared = true
projects = []
roles = []
namespace = "common"
targets = ["claude"]         # 允许的宿主；未列出的宿主不渲染

[tool_extras.claude]         # 仅该宿主认识的扩展字段
permission-mode = "plan"
```

**mcp**：`<mcp_dir>/<name>.toml`：

```toml
name = "files"
type = "stdio"               # stdio | http
command = "uvx"              # stdio 必填；args/env 可选
args = ["mcp-server-files"]
# type = "http" 时：url = "..."，headers 同 env 规则
shared = true
projects = []
roles = []
namespace = "common"
targets = ["claude"]

# 表头必须放在所有顶层字段之后：TOML 中表头之后的键都属于该表，
# 写在 [mcp.env] 下面的 shared / namespace 会被当成环境变量。
[mcp.env]                    # 值为字面量或 "$ENV:VAR_NAME" 引用
API_BASE = "https://example.internal"
TOKEN = "$ENV:AILOOM_MCP_TOKEN"
```

**env**：`<env_dir>/<name>.toml`：`[vars]` 字面量、`[secret_refs]` 键→`$ENV:NAME` 引用；`shared/projects/roles/namespace/targets` 同上。
**hook**：`<hooks_dir>/<name>.toml`：`event`、`matcher`（可选）、`command`（数组）、`timeout_ms`、`targets`、归属字段同上。
**package**：`<packages_dir>/<name>.toml`：`ecosystem`（首批 `npm`）、`version`（精确）、`source`（registry URL 或包名）、`targets`、归属字段同上。

所有条目公共校验：`shared==false` 且 `projects`、`roles` 均空 → `E3005`；引用未声明 project/role/namespace → `E3004`；`projects`/`roles` 数组内重复项 → `E3008`。

### 4.3 项目声明 `.ailoom/project.toml`（工作区，可提交）

注意 TOML 语法：顶层键（schema_version/projects/roles）必须写在任何 `[table]` 之前。

```toml
schema_version = 1
projects = ["a"]             # 0..n，重复/未知 → E3008/E3004
roles = ["dev"]              # 0..n

[source]
name = "team"                # 本工作区内的源别名 [a-z0-9-]{1,32}
type = "git"                 # git | local（v1 不含 self，见 AIL-036）
url = "https://example.com/team/resources.git"  # 禁止内嵌凭据
ref = "main"                 # 可选；缺省 = HEAD of default branch
path = ""                    # 仅 local 源：相对工作区根的路径

[targets]
claude = true
codex = true

# 可选：Skill 选用门禁。缺省（无 [require]）= 部署全部符合选择模型的 Skill。
# 有 skills 时只部署 Require ∩ 选择模型；agent 进一步收窄到 `.ailoom/agents/<name>.toml`。
[require]
skills = ["common-greet", "a-deploy"]
# agent = "inker"
```

Agent 人设（可选）`.ailoom/agents/<name>.toml`：

```toml
skills = ["common-greet"]
```

非法示例（`docs/fixtures/contract/invalid/`）：绝对机器路径、`$SECRET` 字面量值、未知 schema_version、路径穿越 `path = "../x"`、重复项目。

### 4.4 本机绑定 `.ailoom/machine/binding.json`（不提交）

```json
{
  "schema_version": 1,
  "workspace_root": "/abs/path",
  "repository_anchor": "git+ssh://example.com/team/resources.git",
  "workspace_id": "1a2b3c4d5e6f7081",
  "device_id": "b0c9…",
  "declaration": { "…project.toml 的解析结果…" },
  "created_at": "2026-09-09T12:00:00Z",
  "updated_at": "2026-09-09T12:00:00Z"
}
```

### 4.5 源锁 `.ailoom/machine/sources.lock.json`

```json
{
  "schema_version": 1,
  "sources": {
    "team": {
      "type": "git",
      "identity": "git+https://example.com/team/resources.git",
      "ref": "main",
      "resolved_commit": "97fe5b79cd77063e8ea8a2effd4812fb668574a2",
      "content_digest": "sha256:…",
      "locked_at": "2026-09-09T12:00:00Z"
    }
  }
}
```

`identity` 为规范化 URL（scheme 归一、去尾 `/`、小写 host），**不含认证信息**；锁内无凭据字段。本地源 `resolved_commit = null`，`content_digest` 为源内容树摘要，且绑定状态标 `mutable=true` 提示可变性。

## 5. 错误码与退出码（v1 冻结，AIL-002 起实现）

错误 JSON：`{"code":"E2003","message":"…","context":{…},"fix":"可选建议"}`。退出码按类固定：

| 退出码 | 类 | 码段 | 示例 |
|---|---|---|---|
| 0 | 成功 | — | — |
| 1 | 未分类错误 | E9000-E9999 | E9101 本地网页服务操作失败（v1.4） |
| 2 | 用法错误（参数） | E0001-E0999 | 未知参数、缺参数 |
| 10 | 工作区 | E1000-E1999 | E1001 未找到项目根；E1002 非法 workspace；E1003 拒绝静默写全局 |
| 11 | 源/Git | E2000-E2999 | E2001 源未缓存且离线；E2002 fetch 失败；E2003 无效 ref；E2004 URL 含凭据；E2005 缓存损坏；E2006 源身份/导入冲突；E2007 git 命令失败 |
| 12 | 清单/资源 | E3000-E3999 | E3001 未知 schema_version；E3002 缺字段；E3003 路径穿越；E3004 未知 project/role/namespace/**Require Skill** 引用；E3005 归属全空；E3006 资源身份冲突；E3007 非法路径；E3008 重复项 |
| 13 | 计划/同步 | E4000-E4999 | E4001 前置 hash 不符；E4002 目标冲突；E4003 锁被他人持有；E4004 写失败；E4005 journal 恢复失败；E4006 托管清单损坏（v1.4） |
| 14 | 宿主/适配 | E5000-E5999 | E5001 宿主目标不支持；E5002 渲染失败；E5003 能力未知；E5004 用户内容冲突 |
| 15 | 知识 | E6000-E6999 | E6001 索引损坏；E6002 经验归属非法；E6003 知识库位置/恢复状态无效或冲突（v1.4） |
| 16 | 事件 | E7000-E7999 | E7001 事件负载非法 |
| 17 | 上报/导入 | E8000-E8999 | E8001 上报未确认；E8002 导入越界 |
| 18 | 贡献/PR | E8100-E8199 | E8101 远端分叉；E8102 PR 创建失败 |

JSON envelope：`{"schema_version":1,"result":…}` 成功；失败输出 `{"schema_version":1,"code":…,"message":…,"context":…,"fix":…}`（错误字段平铺，与上方错误 JSON 一致，v1.4 起带 `schema_version`）且退出码非 0，错误只走 stderr。

## 6. 所有权与冲突矩阵（v1 保留 v0，逐行实现）

| 目标情况 | 源情况 | 默认结果 |
|---|---|---|
| 不存在 | 需要 | 创建 |
| 已托管且等于上次部署 hash | 更新 | 更新 |
| 已托管但用户修改 | 更新或删除 | 保留并冲突（`E4002`） |
| 未托管同名 | 任意 | 保留并冲突，不自动接管 |
| 已托管且未修改 | 源删除 | 计划清理 |
| 已托管目标消失 | 仍需要 | 计划恢复，展示差异 |
| 结构化配置有无关字段 | 更新自己的条目 | 保留无关字段 |

多文件 Apply 不承诺原子事务；journal 提供可检查恢复；失败不提前写成功 revision；恢复保护失败后的人为修改。

## 7. A/B/shared 最小验收场景（期望资源集合，v1 冻结）

清单：projects `a`、`b`；roles `dev`、`pm`；资源：skill `common-greet`(shared)、`dev-tooling`(roles=[dev])、`a-deploy`(projects=[a])、`b-deploy`(projects=[b])、`pm-checklist`(roles=[pm])；learning `a-postmortem`(project=a)、`b-postmortem`(project=b)、`shared-lessons`(shared)。

| 绑定 | 应得 skills | 可召回 learnings |
|---|---|---|
| projects=[a], roles=[dev] | common-greet, dev-tooling, a-deploy | a-postmortem, shared-lessons |
| projects=[b], roles=[pm] | common-greet, pm-checklist, b-deploy | b-postmortem, shared-lessons |
| projects=[], roles=[] | common-greet | shared-lessons |
| projects=[a,b], roles=[dev] | common-greet, dev-tooling, a-deploy, b-deploy | shared-lessons（learning 归属不明确 → contribute 必须指定目标） |

任何绑定不得召回另一项目的 learning；两个 worktree 绑定不同项目互不影响。

## 8. 能力矩阵要求（各适配卡维护 `docs/capabilities/`）

每行必须含：tool、tested_version、resource_kind、project/user scope、发现路径、格式、热重载/重启需求、Hook 事件、支持级别（supported / unsupported / unknown / ailoom-design）、官方来源、实际验证记录。**未知标 unknown；待核实不能标 supported。**

## 9. 观测与共享约定（v1 保留）

事件 ID、session ID、device ID、workspace ID 各有不同作用与生成规则。session 累计快照不可逐条相加。缺失值为 `null` + `availability` 标记，不写 0。纠正识别标 `heuristic`；工具错误与人工干预分开。事件默认不存完整 prompt 输入。团队共享默认白名单：计数与工具名；共享会话正文是独立显式选择。

## 10. 变更记录

- v1（2026-09-09，AIL-001）：由 v0 草案冻结；新增 ResourceId/文件格式/错误码/退出码/数据分区/最小验收集合的具体定义；MCP/Agent 项目选择标记为 AILoom 设计。
- v1.1（2026-09-14，用户决定）：§3 路径解析优先级由「自有 `AILOOM_*` 高于 XDG」改为「已设 XDG 变量高于 `AILOOM_*`」；无任何覆盖时的默认根由 `~/.ailoom` 改为 XDG 规范默认（`~/.local/state/ailoom`、`~/.local/share/ailoom/store`），并提供旧默认 `~/.ailoom` 的一次性自动迁移（失败回退旧目录并告警）。文件格式与 `schema_version` 不变。实现：`src/paths.rs`；回归：`paths` 单测 8 项 + `test_isolation` 集成。
- v1.2（2026-09-15，RW-01/S01）：§3 兼容迁移补充可达性与身份连续性要求——迁移时把 `<legacy>/device-id` 复制到新数据根（目标已存在则不覆盖），保证设备身份跨升级连续；迁移成功后在旧 `store` 位置保留指向新位置的兼容符号链接，使既有工作区指向旧 store 的绝对 Skill 链接无需重新 sync 仍可达；迁移中段失败回滚时一并清理本次新建的空目录，避免下一进程把空新根误判为新旧并存并采用。文件格式与 `schema_version` 不变。实现：`src/paths.rs`；回归：`paths` 单测 12 项 + `skill_store_require` 迁移集成 2 项。

- v1.4（2026-10-01，审查修复）：新增 E6003（知识库位置/可迁移恢复状态，原误用 E5004）与 E9101（本地网页服务，原误用 E4001）；`.ailoom/knowledge.json` 不支持的版本改报 E3001；`--json` 错误输出补顶层 `schema_version`（字段保持平铺，向后兼容），§5 前后两处写法统一；E8100-E8199 退出码按表修正为 18（原实现误为 17）；§5 补登记已在用的 E2006、E2007；非宿主文件的并发/版本不符改用 E4001、目标冲突改用 E4002、非法路径改用 E3007（原挪用 E5004，AIL-136），控制台对 E4001/E4002/E5004/E6003 统一返回 409；托管清单损坏改用新增的 E4006（原为 E9000，提示中引用了不存在的 `--force-manifest`）；`profile.toml` 解析失败与团队声明一致改为 E3002（E3001 只表示未知 schema_version），并附路径与修复建议。未托管 Skill 删除的链接恢复记录带 `schema_version: 1`。§0 登记 `.ailoom/knowledge.json`、`knowledge/bindings.json`、`service/runtime.json` 三个文件（后两者补 `schema_version`，缺省按 1 读取）；§11.2 登记 `independent_resources` 语义。

## 11. 仓库身份、个人配置层与本地控制台（v1.3，2026-09-16，AIL-039/040 新增）

本节是**新增能力契约**，不修改 §1-§10 的 v1 冻结语义；冲突时以本节描述迁移关系。

### 11.1 仓库身份与作用域模型（AIL-039）

- **RepositoryId**：`repo-` + sha256(Git common-dir 绝对路径)[0..16]。common-dir 是主要发现证据；origin 规范化 URL 只作为关联证据记录，**不作为不可变主键**。同 common-dir 的主工作树与 linked worktree 自动归组；同 origin 的独立 clone 是不同 RepositoryId，只生成关联建议（用户确认后才 `linked_repos`）。
- **WorktreeId**：仓库登记内的稳定随机 id（`sha256(登记时路径)[..12]` 起始），移动/重关联保持不变；登记路径失联标 `missing`，重关联需新路径仍是本仓库工作树。bare 根是仓库容器，不登记为可部署工作树。
- **ScopeId**：配置作用域，取值为「仓库默认 / 仓库内相对路径子项目 / 工作树覆盖 / 工作树内相对路径子项目」之一。子项目不得穿越新的 Git 边界（子模块/嵌套独立仓库按独立仓库登记）。
- **与 v1 字段关系**：`workspace_id = sha256(workspace_root)[0..16]` 不变（事件/锁/journal 分区键）；`repository_anchor` 语义修正——声明目录位于 Git 仓库内时（即使声明目录无自身 `.git`），anchor 用所在仓库的 Git 身份，不再退化为 `nongit+<路径哈希>`。影响：此类工作区的 `anchor_key`（缓存分区）变化，源快照需随下次 init/sync 重新锁定；事件、binding、journal 按 workspace_id 分区，不受影响。实现：`src/workspace.rs::find_git_root_up`、`src/repo_registry.rs`；登记文件 `<data_root>/repos/<repo_id>/registry.json`（JSON，schema_version=1，未知字段忽略）。

### 11.2 仓外个人配置与作用域继承（AIL-040）

- **PersonalProfile**：`<data_root>/profile/profile.toml`（TOML，`schema_version = 1`，仓外机器数据区；项目根不需要任何个人文件）。文件不存在 = 空个人层 = 纯 v1 行为。
- **三态选择**：资源（完整 ResourceId `source/kind/namespace/name`）与宿主统一用 `inherit | enable | disable`。**未设置**（无条目）≠ **显式继承**（`inherit`，进 trace 不改值）≠ **显式禁用**（`disable`，显式值）。
- **优先级（低 → 高）**：团队声明 → 个人仓库默认（`[repos."<repo_id>".default]`）→ 仓库子项目模板（`[[repos."<repo_id>".subprojects]]`，相对路径，浅→深依次生效）→ 当前工作树覆盖（`[repos."<repo_id>".worktrees."<wt_id>"]`）→ 当前工作树子项目覆盖（`[[repos."<repo_id>".wt_subprojects."<wt_id>"]]`，浅→深）。每个有效值必须能追溯到来源层；同层重复子项目路径是配置错误（`E3008`），不靠遍历顺序赢。
- **与 v1 选择模型关系**：团队层（§2 角色∪项目 + require 门禁）语义不变；个人层叠加在团队层之上，只影响 **AILoom 期望部署集合**，不提升源读取权限、不重写团队声明、不阻止宿主从全局/祖先目录加载能力。
- **独立资源选择**（v1.4）：任一层 `independent_resources = true` 表示「本层起不继承上层的资源选择」——计算到该层时清空此前各层的资源结果与 trace，再应用本层及更高层的选择；宿主选择仍照常继承。默认 `false`（不写入文件），旧文件语义不变。
- Git 探测错误（如 `.git` 文件损坏）必须显式报错（E2 段），不允许静默归类为非 Git。
