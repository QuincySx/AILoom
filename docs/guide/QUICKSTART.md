# AILoom 快速上手

本文覆盖最小可用链路：绑定团队源 → 同步资源 → 检索经验 → 贡献 → 卸载。
术语见 [CONTEXT.md](../../CONTEXT.md)；文件格式与错误码见 [CONTRACTS.md](../CONTRACTS.md)。

## 个人模式（默认，无需团队源/远端/TOML）

适合「我在公司仓库里管理自己的 skills / 指令」的场景。全部个人数据存放在
机器数据区（`~/.local/state/ailoom`），不改公司已跟踪文件，也不需要 Git 远端。

```bash
cd /path/to/公司仓库
ailoom web                # 启动或复用本地网页服务（仅 loopback），自动打开浏览器
```

网页有四个页面：

| 页面 | 用途 |
|---|---|
| **我的目录** | 左侧是项目与 Worktree / 子目录树；右侧管理当前目录使用的 AI 工具与能力 |
| **全局规则与 Agent** | 编辑各 AI 工具用户级的 Rules / Agent 原生文件（见 [原生文件](NATIVE-FILES.md)） |
| **资源库** | 导入 Skill / MCP / Rules / Agent（GitHub、GitLab、其他 Git、本地文件夹、skills.sh），从 CC Switch 迁移，检查更新 |
| **操作记录** | 每次应用的文件改动；可撤销 |

第一次使用：

1. **添加项目**：在「我的目录」点 ＋，选择项目文件夹，填写名称；同时确定项目知识库的保存目录（默认在本机数据区 `<data_root>/knowledge/projects/<项目 ID>`，也可选项目内或外部目录，见 [项目与知识库恢复](KNOWLEDGE-PORTABILITY.md)）。
2. **选择 AI 工具**：在目录页「用于这些 AI 工具」中点选 Claude Code、Codex CLI 等。
3. **添加能力**：点「添加能力」从资源库选择 Skill / MCP / Rules / Agent；库里没有的，先在「资源库 → 导入资源」导入（只复制，不执行脚本）。
4. **预览并应用**：项目默认配置对所有 Worktree 生效，修改即时保存；进入具体 Worktree 后点「查看改动」，确认将写入的文件再应用。公司已跟踪的文件会被明确跳过。
5. **验证**：在 AI 工具中**新开会话**调用该能力——文件落盘不等于宿主已加载，真实调用通过才算完成。需要撤销时到「操作记录」。

目录页下方的「本地已有」列出项目里原本就有、不由 AILoom 管理的 Skill / Rules / Agent。可以保持原样、导入为个人副本，或在双重确认后删除（原目录移入本机归档，可恢复）。

CLI 等价操作（个人模式）：

```bash
ailoom library --action import --dir /path/to/my-skill --execute   # 导入资源库
ailoom personal --action effective                                  # 查看有效配置与来源
ailoom personal --action select --host claude --state enable        # 启用宿主
ailoom personal --action select --resource personal/skill/personal/my-skill --state enable
ailoom personal --action plan                                       # 预览
ailoom personal --action sync                                       # 应用
```

指令调整：`ailoom personal --action instructions --file ~/my.md` 保存个人偏好
（Claude 走追加式 `.claude/rules/ailoom-personal.md`；Codex 生成包含公司
AGENTS.md 全文的本地视图，不遮蔽公司基线）。

日常管理在网页「我的目录」与「资源库」完成；个人文档与流程产物默认保存在仓外数据区。

## 多个 Git 合集：按需引用 Skill / MCP

可以同时添加自己的合集、团队合集和第三方 Skill 仓库。合集是来源目录，
**添加来源不等于启用全部资源**。资源 ID 包含来源 ID，避免不同仓库的同名资源被混淆。

Web 操作：在「资源库 → 导入资源」选择 GitHub / GitLab / 其他 Git 并确认版本，然后在「我的目录」的目录页
点「添加能力」选择具体资源，最后进入 Worktree 预览并应用。

CLI 操作：

```bash
ailoom collection --action preview --name my-tools --url https://github.com/owner/collection.git
# 使用上一步返回的 preview_id，确认该快照：
ailoom collection --action apply --preview-id <preview_id>
ailoom collection --action list
# 使用目录返回的完整资源 ID；在目标项目目录执行：
ailoom personal --action select --host claude --state enable
ailoom personal --action select --resource <collection-id>/skill/common/chosen --state enable
ailoom personal --action select --resource <collection-id>/mcp/common/search --state enable
ailoom personal --action plan
ailoom personal --action sync
```

普通 Skill 仓库通过递归查找 `SKILL.md` 识别，无需额外清单。
Skill/MCP 混合集使用下文的 `ailoom.toml` 和资源目录约定；不自动识别任意第三方
MCP 清单格式。MCP 密钥只写 `$ENV:变量名` 引用；添加、预览合集不会启动 MCP。

来源锁定到 commit。更新时重新 preview（传 `--source <collection-id>` 和原 URL），
再 apply 确认版本；每个项目仍需预览并应用，不会立刻改变其他 Worktree 的部署。
修改过的部署内容会受到冲突保护；上游删除了正在引用的资源时，需要显式调整引用。
这里的更新机制不包含定时拉取或无人确认的自动升级。

日常更新：在「资源库」点击 **检查全部更新**，查看版本、检查时间和错误，
再点 **更新全部可用版本** 确认。项目不会被自动修改；按项目预览并应用新版。

从 Git 导入的个人 Skill 副本也可在资源库检查并更新。本地修改会阻止覆盖；
更新应用检查时的 commit，上游随后出现的新版本需要再次检查。CLI 可绑定检查凭证：

```bash
ailoom library --action check-update --skill <name>
ailoom library --action update --skill <name> --preview-id <preview_id> --execute
```

再次检查开始时旧凭证就失效，检查失败会保留错误状态，需重新检查。
兼容的无凭证 CLI 更新使用已保存的候选，
没有候选时先检查一次；Web 更新必须传本次检查凭证。

删除：项目不再使用时先停用引用并应用；资源库的「移除来源」会阻止移除仍被启用的合集。
移除只归档来源登记，保留缓存和历史实体；个人副本删除后移入 `library-archive`。
CLI 对应 `ailoom collection --action check`，以及
`ailoom collection --action remove --source <id>`（预览，确认后加 `--execute`）。

新 Skill 实体使用可读目录 `store/github.com/owner/repo/revisions/<commit>/…`。
旧 Base64 目录保留，已部署项目在确认同步后逐个切到新路径。
完整规则及尚未实现的磁盘清理边界见 [资源管理生命周期](../specs/resource-management-lifecycle.md)。

## 前置

- Rust 1.85+（`cargo build` 产出 `target/debug/ailoom`）
- git 2.x
- 团队资源源：一个包含 `ailoom.toml` 与 `resources/` 的 Git 仓库（结构见下）

```toml
# ailoom.toml（源仓库根）
schema_version = 1
team_id = "example-team"

[projects.a]
name = "Project A"

[roles.dev]
description = "Developer"

[namespaces]
known = ["common", "team-lib"]
shared = ["common"]
```

资源放在 `resources/skills|rules|docs|agents|mcp|learnings` 等目录（契约 §4.1/§4.2）。

## 1. 绑定工作区

```bash
cd /path/to/你的业务仓库
ailoom init --url git@github.com:team/resources.git --project a --role dev
```

- 生成可提交声明 `.ailoom/project.toml`（提交到业务仓库可选）。
- 生成机器数据：绑定在平台数据目录；源锁在 `.ailoom/machine/`（自带 gitignore，勿提交）。
- **零项目合法**：不传 `--project` 时只获得 shared 资源，绝不自动激活。
- 重复 `init` 幂等；显式传参才覆盖。
- **跨机器 clone 业务仓库后**：重新执行 `ailoom init` 即可恢复本机绑定。

## 2. 计划与同步

```bash
ailoom plan              # 预览差异，无任何写入
ailoom sync              # 按当前源锁执行计划（冲突目标自动跳过并保留）
ailoom sync --refresh    # 先推进源锁到团队 ref 最新，再同步
ailoom sync --recover    # 若曾中断，先恢复 journal
```

同步后：技能进入 `.claude/skills/`（Claude Code）与 `.agents/skills/` + `.codex/config.toml`（Codex）；
规则进入 `.claude/rules/` 与 `AGENTS.md` 受管片段；MCP 进入 `.mcp.json` 与 `.codex/config.toml`。

### 无感知自动同步（默认开）

安装 hooks 后，Claude Code 的 **SessionStart / UserPromptSubmit** 会在后台按 TTL 触发 `sync --refresh`（不挡会话）：

- 默认间隔 **1 天**（同一 session 开很久时，到期后在你下一次说话时补洞）
- 环境变量：
  - `AILOOM_AUTO_SYNC=0` — 关闭
  - `AILOOM_AUTO_SYNC_INTERVAL=1d|2d|7d` — 改间隔（也支持 `24h` 或秒数）
- 状态文件：`<data_root>/ws/<id>/auto_sync.json`
- 盘上更新 ≠ 当轮模型已吃到新规则；涉及 rules/agents/MCP 时可能提示需要新开 session

紧急跟上团队仓：手动 `ailoom sync --refresh`。

### Skill 实体与软链（ADR-0001）

Skill **实体**只保存在用户 SkillStore，按**源仓库**分桶。根目录优先级（契约 v1.1）：`$XDG_DATA_HOME/ailoom/store`（已设时）> `AILOOM_STORE_ROOT` > `~/.local/share/ailoom/store`。机器数据根同理：`--data-root` > `$XDG_STATE_HOME/ailoom`（已设时）> `AILOOM_DATA_ROOT` > `~/.local/state/ailoom`；旧默认 `~/.ailoom` 首次运行自动迁移到规范位置。

```text
<store_root>/<host>/<owner>/<repo>/revisions/<version>/<skill>/
```

例如 `store/github.com/owner/repo/revisions/<commit>/<skill>/`。早期版本使用 base64url 编码的 `source_key` 目录，旧目录保留，已部署项目在下次确认同步时逐个切到可读路径。业务 Workspace 里 `.claude/skills/<name>`（及 Codex `.agents/skills/<name>`）是指向上述实体目录的 **symlink**，不再每仓复制一份。

可选在 `.ailoom/project.toml` 声明 Require，只部署选用的 Skill：

```toml
[require]
skills = ["common-greet"]
# agent = "inker"   # 可选，进一步收窄到 .ailoom/agents/inker.toml
```

## 3. 检索经验（隔离召回）

```bash
ailoom recall --query "缓存 发布"
```

只返回当前绑定（活跃项目 ∪ shared）范围的知识；索引自动构建与重建。
**注意：这是相关性隔离，不是访问控制。**

## 4. 贡献经验 / 资源

```bash
# 经验：写好 Markdown（frontmatter 含 title）
ailoom contribute --file my-lesson.md
```

- **已有项目知识库**（网页添加项目时创建，或 `ailoom knowledge --action init`）：经验直接保存到本项目知识库，远端同步用 `ailoom knowledge --action sync`。此时 `--project`、`--shared` 等团队 PR 参数不生效，会给出提示。
- **团队源 PR 流程**（未初始化项目知识库）：单项目绑定默认归属该项目；多项目绑定必须显式 `--project <id>` 或 `--shared`。会建立变更集分支并推送，输出 PR 链接或手动审核提示。

```bash
# 资源修改：在团队源克隆中改好后
ailoom push --from /path/to/resources-clone --message "更新问候技能"
```

审核合并后执行 `ailoom sync --refresh`（或 `ailoom init --refresh && ailoom sync`）获取新版本。

## 5. 事件观测（可选）

```bash
ailoom hooks --action install    # 注册 Claude Code hooks（写入 .claude/settings.json，可幂等重跑）
ailoom dashboard --port 7777     # 本地看板（仅 127.0.0.1）
ailoom session --action metrics  # 会话/Token/干预统计（缺失显示 unavailable）
ailoom report --action digest    # 本地汇总（缺失来源可见）
```

## 6. 卸载

```bash
ailoom uninstall           # 预览将移除的托管内容
ailoom uninstall --execute # 执行：只删 AILoom 托管条目，用户修改保留
ailoom doctor              # 体检：声明/绑定/锁/漂移/宿主工具（--strict 时有失败项退出 10，可用于 CI）
```

## 已知边界

- 宿主范围：Claude Code 与 Codex CLI 为主要宿主；Grok、Pi、OpenCode、Cursor 等为附加宿主。各能力的支持程度与核实状态以 [能力矩阵](../capabilities/) 为准（统一核实见 AIL-142）。
- PR 自动创建仅在 GitHub 远端 + gh CLI 可用时；其它源输出手动审核路径。
- 宿主内真实加载/调用验收需交互环境，当前以文件落盘 + 官方路径断言为准（矩阵中标注"未验证"）。
