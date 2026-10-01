# 项目与知识库恢复

项目和知识库各有一个 UUID v4。它们在创建时生成并持久化，不根据 Git 地址、用户名或绝对路径计算。

- 项目身份：业务目录 `.ailoom/knowledge.json` 的 `project_id`。
- 知识库身份：知识库根 `ailoom-knowledge.json` 的 `id`；项目声明用 `knowledge_id` 引用它。
- 本机仓库 ID、Worktree ID：仍使用现有注册表，用于本机定位，不作为跨机器身份。
- 读命令输出时注意区分：`ailoom knowledge --action status|init` 结果顶层的 `project_id` 是**本机注册表 ID**（如 `repo-…`）；可迁移的项目 UUID 在 `.ailoom/knowledge.json` 与恢复状态（`state.project_id`）中。
- 所有 Worktree 和受管子目录沿用主项目的知识库绑定。一个知识库也可以保存多个项目，各自的配置放在 `projects/<project UUID>/`。

移动、改名、换 SSH/HTTPS 地址、clone 到另一台机器均不重新生成 UUID。复制配置和知识库表示继续使用同一个身份。要创建独立项目，应重新初始化身份，不能根据路径变化自动猜测。

## 哪些内容随项目走

`.ailoom/knowledge.json` 只记录 UUID 和寻找知识库的方法：

| 保存位置 | 项目声明 | 换机器时 |
| --- | --- | --- |
| 项目内部 | 项目相对路径 | 自动填入并核对 ID |
| 外部 Git 仓库或其子目录 | 无凭据的远端地址、分支、仓库内相对路径 | 选择已有知识库，或选择新位置克隆 |
| 外部普通目录 / 无远端 Git | `directory`，不记录绝对路径 | 先同步目录，再选择本机位置 |

实际绝对路径只保存在本机 `knowledge/bindings.json`。初始化、保存恢复配置和迁移会更新声明；只查看页面不会登记项目或联网。现有团队源的 `.ailoom/project.toml` 保留原语义。

这两个文件要随各自目录同步：业务项目中的声明、知识库中的标识和内容。AILoom 不会因保存配置而自动提交或推送 Git。

## 保存位置与迁移

- **默认位置**：新项目的知识库默认由 AILoom 托管，放在本机数据目录 `<data_root>/knowledge/projects/<项目 ID>`。添加项目时也可以改为项目内目录、外部普通目录或外部 Git 仓库（及其子目录）。
- **迁移**：可以把已有知识库整体搬到新位置。流程是「预览 → 复制 → 逐文件核对 → 切换登记」，执行时必须带上预览返回的 `expected`。
  - 原位置保留为备份，不删除。
  - 同一知识库下的所有项目一起切换。
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

知识库 `projects/<project UUID>/state.json` 保存项目的宿主选择、能力选择、子目录作用域和项目说明。已选资源的原文与 Skill 附属文件保存在内容哈希命名的资源快照中。只打包选用的资源，不收集未托管的本地文件，也不复制整个用户资源库。

保存选用配置、项目说明，以及应用改动前，会更新恢复副本。项目设置 → 知识库的“保存恢复配置”可以手动更新；已有项目也通过这个入口生成声明。直接在外部修改资源原文后，应用改动或手动保存恢复配置即可更新快照。

恢复后，资源先以知识库内的离线副本登记。原 Git 来源的无凭据地址、分支和版本一并保留，可在资源库显式检查更新；检查前不联网。原来源只有本机绝对路径的，恢复为离线副本，不能拿旧机器路径当远端。项目说明和选择恢复到本机，AI 工具入口文件仍走现有“预览 → 应用改动”，不会在恢复时覆盖原生 Skill、Rules、Agent 或启动 MCP。

主 Worktree 按 `main` 关联，其余 Worktree 按分支（游离状态按 HEAD）关联。未出现在本机的 Worktree 配置保留在恢复状态中，不自动创建 Worktree。Worktree 出现后可再次恢复。

## CLI

```sh
# 初始化：只选保存目录
ailoom knowledge --action init --root /work/app --path /work/knowledge/apps/app

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
