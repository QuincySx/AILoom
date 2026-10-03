# 更新记录

## 0.2.0（2026-10-02，测试版）

第一个可以拿来试用的版本。

### 新增

- **全局 Skill**：「全局配置 → Skill」里启用的 Skill 部署到 `~/.claude/skills`（Claude Code）与 `~/.agents/skills`（Codex 等），所有项目都能用；项目里不再重复部署。目录里其他工具装的 Skill 只列出、不改动，同名时可以替换并随时还原。CLI：`ailoom global`。
- 本地文件夹导入的 Skill 可以检查更新、应用更新。
- 资源库里坏掉的条目可以在网页上直接打开修复或删除。
- 同步可以撤销（包括删除类的同步）；CLI 同步会给出撤销命令。
- **诊断报告**：`ailoom diagnose`（网页「服务 → 导出诊断信息」）生成一份可以直接发给维护者的文件，不含用户名、令牌和资源内容。
- **升级**：`ailoom service restart`；重新运行安装脚本时，正在运行的网页服务会自动换成新版本。
- 安装、升级、数据位置与备份、卸载的完整说明见 `docs/guide/INSTALL.md`；仓库补上 `LICENSE`（Apache-2.0）。
- **npm 安装**：`npm install -g ailoom-cli`，npm 会自动装上当前平台的预编译版本（macOS、Linux、Windows）。

### 修复

- 从 Git 导入的 Skill 刚导入就提示有更新、更新又失败；按提示「预览 → 执行」更新必定报错。
- 资源库里一个坏条目会让所有项目的计划和同步失败。
- 团队 Hook 注册的命令 CLI 不接受，部署出去的团队 Hook 每次触发都失败（旧条目下次同步自动替换）。
- 从 Git 导入时，仓库里指向仓库外的链接会把本机其他目录的文件复制进资源库。
- 项目声明里按文档手写的 `ref` 被忽略（旧的 `ref_` 仍可读）。
- `doctor` 遇到损坏文件直接退出；纯个人模式被判为有问题。
- 升级后旧版本启动的网页服务仍在运行时，`ailoom service stop` 报「已停止」，`ailoom web` 还会对同一数据目录再起一个服务；现在会指出该进程并说明如何结束。
- 关闭工具后 `.git/info/exclude` 里的条目不会移除。
- 嵌套仓库与 linked worktree 会继承外层目录的项目声明。
- 带 BOM 的 SKILL.md 导入后丢失描述；多处报错不带路径或修复提示；多处提示里的命令写法 CLI 不接受。
- Linux 上团队 Hook 超时时，回收进程的信号会打到 AILoom 自身所在的进程组，连同调用方一起被杀掉。
- `ailoom init --url` 不写 `--ref` 时固定用 `main`，默认分支是 master 的团队仓库绑定失败；现在跟随远端默认分支。
- 兼容较旧的 git（2.30 起，如 Debian 11）：Worktree 枚举、仓库身份、锁定原因与 `.git/info/exclude` 定位不再依赖新版 git 才有的参数。

### 已知限制

- Claude Code、Codex 从用户级目录加载全局 Skill 尚未在真机上验证。
- Grok、Pi、OpenCode、Cursor 只按官方文档路径部署，未逐一真机验证；Windows 未验证。
