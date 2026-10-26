# 项目与知识库恢复

每个项目和它的知识集合各有一个 UUID v4；多个项目共用一份知识库 Git 仓库，按项目分文件夹。它们在创建时生成并持久化，不根据 Git 地址、用户名或绝对路径计算。

- 项目身份：业务目录 `.ailoom/knowledge.json` 的 `project_id`。
- 知识库身份：项目知识目录中的 `.ailoom/library.json`（旧格式为 `ailoom-knowledge.json`） 的 `id`；项目声明用 `knowledge_id` 引用它。
- 本机仓库 ID、Worktree ID：仍使用现有注册表，用于本机定位，不作为跨机器身份。
- 读命令输出时注意区分：`ailoom knowledge --action status|init` 结果顶层的 `project_id` 是**本机注册表 ID**（如 `repo-…`）；可迁移的项目 UUID 在 `.ailoom/knowledge.json` 与恢复状态（`state.project_id`）中。
- 所有 Worktree 和受管子目录沿用主项目的知识库绑定。同一知识库仓库中的各项目默认使用 `<仓库名>--<项目UUID>/` 文件夹，正文、原文和配置都位于自己的目录中。同名仓库按完整项目 UUID 区分；新项目保存时分配 UUID，查看设置不生成身份。恢复可按两个 UUID 找回已有目录，不依赖新机器的代码目录名。
- `.ailoom/knowledge.json` 和项目知识目录 `.ailoom/project/project.json` 记录无凭据的代码仓库 Git 地址，供 UUID、Git 链接与名称查询；目录名和本机绝对路径不作为项目身份。项目知识中的记录随知识仓库一起迁移，与 `.ailoom/library.json` 的归属核对。

移动、改名、换 SSH/HTTPS 地址、clone 到另一台机器均不重新生成 UUID。复制配置和知识库表示继续使用同一个身份。要创建独立项目，应重新初始化身份，不能根据路径变化自动猜测。

## 哪些内容随项目走

`.ailoom/knowledge.json` 记录 UUID、代码仓库 Git 地址和寻找知识库的方法：

| 保存位置 | 项目声明 | 换机器时 |
| --- | --- | --- |
| 项目内部 | 项目相对路径 | 自动填入并核对 ID |
| 外部 Git 仓库或其子目录 | 无凭据的远端地址、分支、仓库内相对路径 | 选择已有知识库，或选择新位置克隆 |
| 外部普通目录 / 无远端 Git | `directory`，不记录绝对路径 | 先同步目录，再选择本机位置 |

实际绝对路径只保存在本机 `knowledge/bindings.json`。初始化、保存恢复配置和迁移会更新声明；只查看页面不会登记项目或联网。现有团队源的 `.ailoom/project.toml` 保留原语义。

这两个文件要随各自目录同步：业务项目中的声明、知识库中的标识和内容。AILoom 不会因保存配置而自动提交或推送 Git。

## 从各工作目录采集开发文档

在项目 → 知识库中直接开始整理，后台自动读取本仓库主 Worktree、其他仍存在的 Worktree 和受管子目录中的候选文档，无需先扫描、勾选或逐篇导入。它们汇入同一个项目文件夹；其他代码仓库的资料不参与本次整理。

采集保留原文，不修改来源文件。相同内容复用一份快照，各自记录来源目录、文档路径、分支、提交和采集时是否有未提交内容。资料更新保留历史，不同分支分别记录；已删除的来源不会自动删除已保存的快照。

原文位于 `<仓库名>--<项目UUID>/sources/`，来源台账位于该目录及 `<仓库名>--<项目UUID>/.ailoom/`。采集跳过认证、依赖、构建、缓存目录和符号链接；候选资料仍须由 AI 结合对应代码判断价值、归属与适用性。

准备删除 Worktree 时，先确认文档已保存，再进行下方的知识整理。若知识库本身位于待删除的 Worktree 内，应先通过“保存位置与恢复配置”迁移到保留的目录。采集不会删除任何 Worktree。

## 手动整理知识

在项目 → 知识库中，直接点击“开始整理”即可使用内置规则全面整理已入库资料，无需填写提示词。执行工具默认 Codex；需要额外限定主题时，可以展开“补充整理要求（可选）”，留空仍使用默认规则。目前支持检测 Codex、Claude Code、Pi 和 Cursor Agent CLI；需要先在终端完成相应工具的安装和登录，模型与服务沿用本机工具配置。

整理读取本知识库的 Markdown 资料，并可只读核对主工作目录中的项目代码。内置规则覆盖业务知识与例外、开发流程、模块契约、决策取舍和问题经验，保留当时的目标、内外部约束、代价、适用阶段和后续演变。未记录的原因明确保留缺口，不从当前代码编造历史理由；不同阶段、分支的做法可以并存。

提示词参考 Karpathy 的 LLM Wiki 思路，知识单元有稳定主题 ID、内部导航、跨来源关联、证据及适用状态，并保留维护建议、待核实事项、完整来源目录、本轮演进摘要与自检。再次整理沿用已有主题入口，回到原始资料核实；旧报告不作为独立证据。配置恢复副本、资源快照和之前生成的整理报告不计作原始经验。采集文档的老快照仍保留在磁盘，正常整理只提供各来源的最新版本；历史理由缺少当前原文支持时需要补充旧证据。

整理结果按模块、概念和独立问题保存在 `<仓库名>--<项目UUID>/wiki/`，项目首页 `<仓库名>--<项目UUID>/README.md` 提供导航，`sources/` 保留原文。人工和 AI 编辑同一份 Markdown；不同项目目录互不混写。提示词见 [整理模板](../../src/knowledge/res/organize-wiki.md)，设计见 [开发知识生命周期](../specs/development-knowledge-lifecycle.md)。

AI 需要查 SDK 等其他项目的知识时，用 `ailoom --json knowledge --action locate --query <UUID、名称、Git地址或路径>` 定位，再使用返回的 recall_command。定位只返回入口，普通整理与检索不会混入其他项目内容。不同组织下的同名仓库、多个知识副本或共享同一 Git 地址的不同项目返回多个候选，不自动合并。

新机器只复制或克隆知识仓库，也可以用 `locate --repository <本地知识仓库> --query <UUID或Git地址>` 查找其中尚未在本机登记的项目，再用 `recall --path <返回的项目知识目录> --query <问题>` 检索。没有对应代码副本时返回 code_available: false；可查知识不代表已经核对当前代码。只查本地元数据，不联网搜索远端，也不因查找而登记代码项目。

代码 Git 地址使用 SSH 别名且缺少正式地址时，可执行 `ailoom knowledge --action checkpoint --root <代码目录> --repository-url https://github.com/组织/仓库.git` 明确补充。初始化和保存恢复配置会记住已知地址，换连接或路径后 UUID 不变。历史地址是查询别名；仓库转让或别名失效时可人工修订两侧声明中的 repository_urls，不能把地址命中当作远端所有权验证。

任务在后台执行，离开页面后继续；返回知识库页面会恢复进度，也可取消。CLI 失败、超过 30 分钟或整理期间知识内容发生变化时，保留原有知识和上次报告。已生成但未能保存的草稿可以在操作记录中查看。服务重启后的任务标记为中断，不自动重新运行。

“整理完成”表示生成并保存，不表示独立质量评估通过。新提示词要求稿件标记 `review_status: unreviewed` 并披露自检范围；运行时尚未按该标记过滤检索，也未串联自动评估、退回修订或代码变化后的过期复核。可用 [独立评估模板](../../src/knowledge/res/review-wiki.md) 对照同批原文审查，失败时按具体问题修订并重评。

Codex 通过官方的 [`codex exec` 非交互入口](https://learn.chatgpt.com/docs/non-interactive-mode) 调用，并明确使用只读沙箱；其他工具也仅开放只读分析。本机 CLI 使用自身已配置的模型服务，AILoom 不代管其登录凭据。

## 保存位置与迁移

- **知识库仓库**：添加项目时选择一个共用仓库（如 `/work/agent-knowledge`），项目知识自动放入其中的 `<仓库名>--<项目UUID>/`。后续新项目沿用这个仓库；首次未选择时使用 `<data_root>/knowledge/repository/`。已有项目关联不自动改变。
- **迁移**：可以把已有知识库整体搬到新位置。流程是「预览 → 复制 → 逐文件核对 → 切换登记」，执行时必须带上预览返回的 `expected`。
  - 原位置保留为备份，不删除。
  - 只切换当前项目的知识目录，同仓 Worktree 一起沿用；其他项目目录不变。
  - 迁移记录保存在 `<data_root>/knowledge/migrations/`。
- **会被拒绝的迁移**：
  - 目标目录非空，或与原位置相同、互相嵌套；
  - 知识库内含符号链接；
  - 预览之后内容发生了变化；
  - 超过 128 MB / 10000 文件上限。

  被拒绝时登记不变，目标目录不写入任何内容。

```bash
ailoom knowledge --action move --root <项目> --path <新位置>                      # 预览
ailoom knowledge --action move --root <项目> --path <新位置> --expected <预览值> --execute
```

## 恢复内容

项目目录中的 `.ailoom/project/state.json`（旧格式为 `projects/<project UUID>/state.json`） 保存项目的宿主选择、能力选择、子目录作用域和项目说明。已选资源的原文与 Skill 附属文件保存在内容哈希命名的资源快照中。只打包选用的资源，不收集未托管的本地文件，也不复制整个用户资源库。

保存选用配置、项目说明，以及应用改动前，会更新恢复副本。项目设置 → 知识库的“保存恢复配置”可以手动更新；已有项目也通过这个入口生成声明。直接在外部修改资源原文后，应用改动或手动保存恢复配置即可更新快照。

恢复后，资源先以知识库内的离线副本登记。原 Git 来源的无凭据地址、分支和版本一并保留，可在资源库显式检查更新；检查前不联网。原来源只有本机绝对路径的，恢复为离线副本，不能拿旧机器路径当远端。项目说明和选择恢复到本机，AI 工具入口文件仍走现有“预览 → 应用改动”，不会在恢复时覆盖原生 Skill、Rules、Agent 或启动 MCP。

主 Worktree 按 `main` 关联，其余 Worktree 按分支（游离状态按 HEAD）关联。未出现在本机的 Worktree 配置保留在恢复状态中，不自动创建 Worktree。Worktree 出现后可再次恢复。

## CLI

```sh
# 同一个知识库仓库，按项目自动分文件夹
ailoom knowledge --action init --root /work/app --repository /work/agent-knowledge
ailoom knowledge --action init --root /work/sdk --repository /work/agent-knowledge

# 迁入同一仓库：先预览，再带 expected 执行；旧目录保留
ailoom knowledge --action move --root /work/app --repository /work/agent-knowledge

# 已有项目保存可迁移声明、选用配置和托管资源
ailoom knowledge --action checkpoint --root /work/app

# 新机器查看恢复来源
ailoom knowledge --action status --root /new/app

# 外部 Git：显式选择新克隆目录（clone 输出知识库子目录）
ailoom knowledge --action clone --root /new/app --path /new/knowledge-repo

# 外部目录：同步过来后选择；项目内相对位置可省略 --path
ailoom knowledge --action recover --root /new/app --path /new/knowledge-repo/apps/app

# 核对预览后，使用返回的 expected 执行
ailoom knowledge --action recover --root /new/app --path /new/knowledge-repo/apps/app --execute --expected <指纹>
```

UI 添加项目与项目设置调用同一组服务。Git 认证沿用用户本机配置，失败保留现场，不代配凭据。克隆不会运行 checkout filters、仓库 hooks、子模块或资源脚本；含链接/子模块的仓库需要用户在本机准备后关联。现有目录不会被 clone 覆盖。

## 冲突与边界

- 知识库 UUID 不匹配、配置/资源缺失、符号链接、预览后内容变化时拒绝恢复。
- 本机已有不同选择、项目说明或恢复资源时不覆盖，要求先核对。
- 每个项目恢复状态使用本机记录的上次摘要检查并发；另一台设备改过状态时，不把旧本机配置直接写回去。Git 的提交、推送冲突仍由显式 Git 同步处理。
- 未恢复到本机的 Worktree 配置和所需资源继续保留。
- 恢复不复制机器凭据、插件安装状态、操作历史或托管文件清单；不声称插件已安装。
- 目录校验与迁移沿用 128 MB / 10000 文件上限。
