# 控制台组件与样式约定

## 来源与边界

参考用户指定的 [stitch-skills / shadcn-ui](https://github.com/google-labs-code/stitch-skills/blob/main/plugins/stitch-build/skills/shadcn-ui/SKILL.md) 及其 customization-guide，采用语义颜色、显式变体和基础组件/业务组合分层。不安装 Skill，不运行 shadcn CLI，不引入 React/Tailwind 或其他依赖。

当前实现是原生 JS/CSS，**不是官方 shadcn React 组件**。原来的 artifact-design `theme.css` / `theme.js` 文件保留供历史兼容，但应用壳不再加载；颜色不再跨两套主题间接定义。

## 三层职责

1. `tokens.css`：唯一设计值来源。background/foreground、primary/primary-foreground、muted、destructive、border/input/ring，统一字号、4/8 间距、圆角、44px 控件高度。旧 c-* 名称只作同文件中的兼容别名。
2. `components.css` 与 `components/*.js`：按钮、输入、标签、表格、弹窗、状态徽章的视觉和基础交互，不发业务请求。
3. `base.css` 与 features/pages：应用壳和页面布局、业务组合，不定义新的颜色值。

## 使用

`Button(slot, {label:'保存', variant:'default', onPress:save})` 表达主操作；outline 为默认兼容变体，secondary/ghost/destructive 为其他变体。静态 HTML 使用同一 data-variant 契约；现有 primary/danger 类保留兼容。

按钮等待时 aria-busy + 禁用防重入，页面销毁后不再更新 DOM。Field 的错误用 aria-invalid/aria-describedby 关联。可操作表格的首列提供真实按钮，支持键盘 Enter/Space。

Dialog 基于原生 dialog.showModal，进入 top layer 并让背景 inert；新建项目、项目设置、资源导入使用相同的标题/内容/底部动作区，Esc、取消和关闭按钮返回触发控件。请求期间不允许关闭表单。删除、更新和放弃修改使用同一 confirmAction，而不是浏览器 confirm。浏览器离站 beforeunload 保留系统提示，这是浏览器限制。

搜索 input 与选择器使用同一高度、边框、背景、字体、左内边距和焦点规则。`components/select.js` 将页面中的单选 select 适配为 HTML combobox/listbox；原 select 隐藏，仅保留表单值和 change 事件兼容。选项菜单使用 Popover top layer，避免被 dialog 遮挡，支持方向键、Home/End、Enter、Esc、Tab 和输入字符定位。动态选项、禁用状态和程序赋值同步到组件；卸载时清理监听。当前不支持多选 select。

交互参考 [WAI-ARIA select-only combobox](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/examples/combobox-select-only/)；浮层使用 [Popover API](https://developer.mozilla.org/en-US/docs/Web/API/Popover_API)。已验证当前 Chrome，尚未验证旧版 WebView 兼容性。

当前只提供浅色主题，不宣称已完成深色主题或完整屏幕阅读器审计。配色为本项目选择的中性方案，不声称来自 shadcn 某个预设的逐值复制。

## 防回归

`node --experimental-vm-modules --test tests/frontend_components.mjs` 检查按钮生命周期、表单语义和活动 CSS 的 Token 完整性；浏览器测试检查项目主流程、组件样例、导入面板与窄屏布局。
