# 资源管理与首次设置改版验收

## 已验证

- 38 项定向集成测试通过：collections（8）、console_e2e（2）、console_onboarding（1）、console_server（12）、personal（4）、skill_sources（4）、skill_store_require（7）。
- 库单元测试 89 项通过，沿用排除既有的 `paths::tests::empty_xdg_does_not_count_as_set`；未宣称全仓测试全绿。
- 编辑器组件 Node 测试通过；全部 UI JavaScript 语法检查、Rust 格式与 diff 检查通过。
- 真实 Chrome：首次设置 → 指定隔离目录 → 识别并确认 → 本地 Skill 导入 → 选择资源与 Claude → 预览 → 应用 → 完成设置；实际写入隔离项目的 Skill 链接。没有执行 Skill 或启动 MCP。
- 浏览器检查 GitHub、GitLab、其他 Git、本地、skills.sh 五种表单状态；1440px 桌面与 390px 窄屏截图，页面无横向溢出，运行中无 JavaScript 异常。
- 新增测试覆盖批量检查不推进版本、批量更新旧预览拒绝、来源移除引用保护、旧 Base64 软链接逐项目切换且保留旧实体、个人副本可恢复归档。

## 证据与复现

浏览器脚本为 `tests/ui_browser.mjs`；先用独立 Chrome profile 启动 CDP :9231，再指定本机控制台 URL、截图前缀。`onboarding` 模式额外接收隔离项目与 Skill 目录。应使用新的临时项目，已有非托管产物会触发保护，不应删除用户内容来让测试通过。

本轮截图位于 `/private/tmp/ailoom-ui-review.JZ0jca/`：`before-desktop.png`、`final-desktop.png`、`final-applied.png`、`final-import.png`、`final-mobile.png`。截图是本机临时证据，不是仓库内永久资产。

## 尚未验收/未实现

- 远端 GitHub/GitLab 的实际网络、私有仓库认证；集成测试使用本地 Git fixtures。
- 宿主新会话实际调用 Skill/MCP。
- 定时后台自动检查、资源缓存安全 GC、归档的一键恢复、多项目一键部署。
- 完整全仓回归；先前的路径环境测试失败不在本次修复范围。

界面与存储规则见 `docs/specs/resource-management-lifecycle.md`。设计采用 artifact-design 的共享样式，改变导航组成、信息层级与表单密度；功能验收以实际接口和项目文件为准。
