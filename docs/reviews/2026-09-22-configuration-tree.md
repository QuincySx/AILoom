# 项目、Worktree 与子目录配置树

用户确认：普通文件夹项目直接添加子目录；Git 项目按仓库身份只显示一次，下面列出主工作树和其他 Worktree。每个工作树可以添加子目录，子目录可以继续嵌套。只展示主动登记的配置节点。

每个子节点默认继承父节点能力，并可增减。关闭“继承父节点能力”后只使用本节点明确选择的能力，父节点未来新增能力不会混入；这个节点的后代仍可继承它。AI 工具选择单独沿用现有继承规则。

## 实现

- Git 项目节点编辑项目默认选择，已有 Worktree 自动发现、归组，并提供刷新入口。此入口不创建或删除 Git Worktree。
- Worktree 和子目录独立配置；新增子路径校验目录存在、不越界、不跨嵌套 Git 边界。
- 独立选择持久化到节点配置，解析时截断祖先能力，恢复继承不删除本节点的显式选择。
- 右侧显示节点类型、父节点和继承开关，并按 Skill、MCP、Agent 等类型分组；空类型保留添加入口。
- Git 项目默认配置不直接写入某个工作树。具体节点仍通过底部统一预览、应用，其他节点不会被顺带写入。

## 实际验证

全部通过 Orca 内置浏览器，服务使用 `--no-open`；文件操作只涉及 `/tmp/ailoom-dirux` 测试夹具。

1. 普通文件夹知识库甲添加 docs，默认继承根目录 meeting-notes；关闭继承后清空继承能力，手动添加 doc-search。
2. docs 下添加 meetings，父节点显示 docs，继承 doc-search，不混入根目录 meeting-notes。应用后只在 docs/meetings 下生成 Claude、Codex 的 doc-search 入口。
3. Git 项目丙只显示一条，子节点为 main 和 feature；项目默认启用 meeting-notes、Claude。
4. feature 添加 web。关闭 feature 的能力继承后 web 跟随其变空；main 仍然继承 meeting-notes。
5. feature 独立添加 doc-search 后，web 继承它。应用仅生成 feature/web/.claude/skills/doc-search；main 和 feature 根目录均没有新增入口，web 已有 old-notes 保留。
6. 浏览器重新打开后保持节点选择和继承状态，右侧三个类型分组均存在；桌面没有横向溢出。

自动检查：12 项 profile 单测、6 项 profile 集成测试、1 项来源部署回归、6 项前端组件测试通过；构建、JS 语法与 diff 空白检查通过。

截图：[Git → Worktree → 子目录](../evidence/configuration-tree-2026-09-22/git-worktree-child.png)。

## 子 Agent 环境入口修订

根据后续交互澄清，添加入口只放在当前项目行，不在 Worktree 或各级子目录放加号。用户从项目内直接选择任意已有文件夹，Git 项目先选择 Worktree；不要求逐层登记中间目录。选择保存为配置节点，默认继承最近的已配置父级，也可关闭能力继承。普通目录不自动加入导航。

可选发现折叠在添加弹窗中，用户点击才扫描 2 或 3 层。统一 adapter 目录标记注册表支持 Claude、Anthropic、Codex、共享 Agent 目录以及现有规则宿主目录；这些只是候选线索。扫描只读、不跟随符号链接、跳过依赖与嵌套 Git，最多访问 5000 个目录。用户选择候选并确认后才登记。Git Worktree 仍自动发现。

验证：Rust 扫描器 2 项测试通过，前端组件 6 项测试通过，cargo build 通过，git diff --check 通过。在 Orca 内置浏览器中实际选择无 Agent 配置的 notes 文件夹并保存，确认默认继承 meeting-notes；可选扫描只返回 docs/meetings 和 web，取消不新增环境；Git 项目自动显示 main/feature Worktree。
