# QUICKSTART 走查（2026-10-01，AIL-145）

按重写后的 [快速上手](../../guide/QUICKSTART.md)，在隔离的 HOME / XDG / 宿主目录中依次执行 CLI 步骤，覆盖：

- **个人模式**：导入个人库 → 选择宿主与资源 → 预览 → 同步 → 个人指令。
- **本地 Git 合集**：预览 → 应用 → 选择资源 → 同步 → 检查更新。
- **团队源**：绑定 → 预览 → 同步 → 召回 → 体检 → 卸载。

脚本：[walk.sh](walk.sh)（运行前需构建 `target/debug/ailoom`）。输出：[output.txt](output.txt)，临时路径已替换为 `<tmp>`。

结果：每一步退出码都是 0。另外核对了三处文件结果：个人库 Skill 和合集 Skill 都部署到 `.claude/skills/`，卸载后托管的 Skill 被移除。

发现的问题：`personal effective/select/sync`、`collection apply/check` 在人类模式下直接打印 JSON，归入 AIL-135 第 5 步（人类可读输出）。
