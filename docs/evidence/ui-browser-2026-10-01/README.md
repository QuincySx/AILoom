# 浏览器验收全模式通过（2026-10-01，AIL-130）

命令：`scripts/ui-browser.sh`。它会在临时目录里隔离 HOME / XDG / 宿主目录，并启动独立的控制台和 headless Chrome，结束后只停掉自己启动的进程。

结果：current、grouped、cc-switch、projects、onboarding、design 六个模式全部 PASS，各模式最后一行 `errors` 均为空。日志见本目录 `*.log`，截图只保留两张代表性的，其余在 `target/ui-browser/`，不入库。

本轮为适配当前界面所做的修订（`tests/ui_browser.mjs`）：

- **current**：导入对话框的判定从已删除的 `[data-import]` 改为 `[data-provider]`。
- **grouped**：能力库改为平铺目录，断言改成按来源标签统计 mock 资源，并检查搜索结果。
- **projects / design**：
  - 旧项目详情页（`data-tab`、`data-entries`）已删除，改为测「添加项目」对话框（Esc 关闭后焦点回到触发按钮）→ 目录页宿主开关 → 390px 下无横向溢出 → 管理列表的搜索与分类。
  - design 的工具栏样式检查移到 `#/projects/manage`。
- **onboarding**：引导页只做导航，改为验证三个入口分别跳转正确。
- **端口**：Chrome 调试端口改由 `CDP_PORT` 指定，不再写死 9231。

验收中发现并已修复的问题：组件样例页有 15 个按钮矮于 44px。原因是 `workspace.css` 在 `:root` 上把 `--control-height` 全局改成 32px，现在只在 `.workspace-explorer` 内生效（U-06）。

## 稳定性排查（同日后续）

console 后端拆分（AIL-143）之后，浏览器验收出现间歇失败，其中每批第 3 轮最为集中。逐层排查的结论如下：

1. **应用本身没有问题。**
   - 用 curl 并发 60 个请求、以及保持 8 个空闲预连接后再请求，`GET /` 均正常返回。
   - 冷启动的 Chrome 先停在 `about:blank`、再用 `Page.navigate` 打开时，4 次中 4 次都正常渲染。
2. **测试工具的问题。**
   - 冷启动时，`json/new?<URL>` 新建的标签页停留在 `about:blank`，并未导航。现在改为先开空白页，再显式调用 `Page.navigate`。
   - `waitFor` 遇到求值异常时改为重试。
   - CDP 命令加 30 秒超时，卡住时明确报错，而不是被外层超时静默杀掉进程。
3. **环境因素。** 失败的那几轮，Chrome 的后台更新程序被拉起（可执行文件版本 154，更新程序报告 152）。此时无头实例打开了 6 个连接却不发送任何请求，`Page.navigate` 一直无响应。启动参数加上 `--disable-background-networking --disable-component-update --disable-sync` 等关闭后台任务的开关之后，连续三轮均为 6/6 通过，没有触发任何重试。
4. **兜底重试。** `scripts/ui-browser.sh` 只在出现「Chrome 卡住」（`Page.navigate` 无响应）这类失败时，重启 Chrome 并重跑该模式一次，结果标注为「重启后重试通过」。其他断言失败一律直接判为失败。

另外：`shell.html` 增加了空 favicon，消除每次加载都出现的 `/favicon.ico` 404。
