# 项目优先控制台验收 · 2026-09-17

## 通过

- `cargo test --test console_server`：13 项通过，包含项目分类/名称重启持久化、未授权路径拒绝、A/B 指令隔离。
- `console_e2e` 2 项、`console_onboarding` 1 项通过，保留原有部署路径兼容。
- `node --experimental-vm-modules --test tests/frontend_components.mjs` 通过。
- 实际 Chrome 页面测试：默认项目页、添加普通文件夹、启用宿主、保存并回读指令、项目搜索与分类、全局资源中心导入来源切换；无页面 JS 异常。
- 1440px 桌面与 390px 窄屏截图检查，无页面横向溢出。

截图在 `/private/tmp/ailoom-project-review.O1wiB5/after-*.png`。运行脚本为 `tests/ui_browser.mjs` 的 `projects` 模式，使用隔离临时项目和数据区，不触碰用户业务项目。

## 边界

- macOS 原生目录选择桥接已实现，但尚未人工操作原生对话框验收；其他平台明确提示手动绝对路径。
- 当前为本机 HTTP 控制台，不声称已经封装成原生 WebView。
- 项目页面保存仓库默认配置，选中的工作目录决定本次部署落点；高级子项目/工作树覆盖仍在旧兼容作用域页面。
- 未对远端认证或宿主实际调用 Skill/MCP 做新增实测；不宣称所有资源能力已通过真实宿主验证。
