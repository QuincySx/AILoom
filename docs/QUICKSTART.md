# AILoom 快速上手

本文覆盖最小可用链路：绑定团队源 → 同步资源 → 检索经验 → 贡献 → 卸载。
术语见 [CONTEXT.md](../CONTEXT.md)；文件格式与错误码见 [CONTRACTS.md](CONTRACTS.md)。

## 前置

- Rust 1.75+（`cargo build` 产出 `target/debug/ailoom`）
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

同步后：技能进入 `.claude/skills/`（Claude Code）与 `.ailoom/skills/` + `.codex/config.toml`（Codex）；
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

Skill **实体**只保存在用户 SkillStore，按**源仓库**分桶。根目录优先级：`AILOOM_STORE_ROOT` > `$XDG_DATA_HOME/ailoom/store`（仅当已设）> `~/.ailoom/store`。机器数据根同理：`--data-root` > `AILOOM_DATA_ROOT` > `$XDG_STATE_HOME/ailoom` > `~/.ailoom`。

```text
<store_root>/<source_key>/.meta/SOURCE.json
<store_root>/<source_key>/<相对 skills 根>/
```

`source_key` 为规范化仓库 URL/路径的可逆 base64url。业务 Workspace 里 `.claude/skills/<name>`（及 Codex `.ailoom/skills/<name>`）是指向上述实体目录的 **symlink**，不再每仓复制一份。

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
# 经验：写好 Markdown（frontmatter 含 title），明确归属
ailoom contribute --file my-lesson.md            # 单项目绑定默认归属该项目
ailoom contribute --file my-lesson.md --shared   # 显式共享全团队
# 多项目绑定必须显式 --project <id> 或 --shared

# 资源修改：在团队源克隆中改好后
ailoom push --from /path/to/resources-clone --message "更新问候技能"
```

两者都会建立变更集分支并推送，输出 PR 链接或手动审核提示；
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
ailoom doctor              # 体检：声明/绑定/锁/漂移/宿主工具
```

## 已知边界（初版）

- 双宿主范围：Claude Code 与 Codex CLI；能力矩阵见 [capabilities/](capabilities/)（Codex 项目级 Agent/秘密插值为 unknown，显式 unsupported）。
- PR 自动创建仅在 GitHub 远端 + gh CLI 可用时；其它源输出手动审核路径。
- 宿主内真实加载/调用验收需交互环境，当前以文件落盘 + 官方路径断言为准（矩阵中标注"未验证"）。
