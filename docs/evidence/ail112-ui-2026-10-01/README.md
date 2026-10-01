# AIL-112 删除入口迁移后的浏览器验收（2026-10-01）

背景：删除与「接管说明」原先只存在于已不可达的 `pages/projects.js` `detail()` 中（AIL-138 清理时发现），现迁到目录页「本地已有」面板（`features/nativeFiles.js` + `features/skillActions.js`）。

环境：隔离 HOME / XDG / 宿主目录；`ailoom console --port 47961 --no-open`；独立 profile 的 headless Chrome（`--remote-debugging-port=9245`）。项目为临时 Git 仓库，含 `.claude/skills/local-one/SKILL.md`，先用 `ailoom personal --action effective --root <项目>` 登记。

重复执行：

```sh
SKILL_DIR=<项目>/.claude/skills/local-one SHOTS=<输出前缀> \
  node cdp.mjs <chrome调试端口> "http://127.0.0.1:<控制台端口>/#/projects/<repo_id>" ./delete_flow.mjs
```

结果（OK，页面无脚本错误）：

| 步骤 | 断言 | 截图 |
|---|---|---|
| 预览后取消 | 预览显示精确目录、文件数与恢复方式；取消后目录不变 | [1-preview.png](1-preview.png) |
| 输入错误名称 | 提示名称不一致，目录不变 | [2-wrong-name.png](2-wrong-name.png) |
| 输入正确名称 | 目录移入 `<data_root>/project-archive/<时间戳>-local-one/`（含 SKILL.md），列表刷新 | [3-deleted.png](3-deleted.png) |
