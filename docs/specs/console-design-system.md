# 控制台组件与样式约定

## 来源与边界

参考用户指定的 [stitch-skills / shadcn-ui](https://github.com/google-labs-code/stitch-skills/blob/main/plugins/stitch-build/skills/shadcn-ui/SKILL.md) 及其 customization-guide，采用语义颜色、显式变体和基础组件/业务组合分层。不安装 Skill，不运行 shadcn CLI，不引入 React/Tailwind 或其他依赖。

当前实现是原生 JS/CSS，**不是官方 shadcn React 组件**。原来的 artifact-design `theme.css` / `theme.js` 已于 2026-10-01 删除（AIL-138），颜色只由 `tokens.css` 定义。

## 三层职责

1. `tokens.css`：唯一设计值来源。background/foreground、primary/primary-foreground、muted、destructive、border/input/ring，统一字号、4/6/8 间距、圆角与控件尺寸：桌面控件 34px，小屏控件 44px。旧 c-* 名称只作同文件中的兼容别名。
2. `components.css` 与 `components/*.js`：按钮、输入、标签、表格、弹窗、状态徽章的视觉和基础交互，不发业务请求。
3. `base.css` 与 features/pages：应用壳和页面布局、业务组合，不定义新的颜色值。
4. `workspace.css`：页面和业务组合的布局。内容容器上限 1120px，Large／Medium 的全局导航宽 184／164px、配置目录宽 208／192px；≤700px 使用顶栏主导航与目录选择器。尺寸差异统一定义在 `tokens.css`。

实现依据是唯一的 SmallPen 设计包 [`ailoom-ui.smallpen`](../design/brand/ailoom-ui.smallpen/)，revision `6d5c1204a6eecaddaf65008ca78b12c9ad59e935ec9586380282264a8a7fa2b5`。旧品牌包已移出项目，不能依据历史导出实现页面。`components/layout.js` 提供 AppHeader／PageHeader／面包屑；`components/sectionNav.js` 提供项目导航；`components/item.js` 提供 Item、分页与操作菜单。

资源 Item 上下内边距 4px、左右 12px，内容自然撑高；小屏资源库条目收起装饰图标、左右收为 8px，保留主操作与更多操作。非交互工具行随控件高度为桌面 34px、小屏 44px，不创建内容高度 Token。标题 14/20、说明 12/16、控件标签 13/20。能力与技能列表不使用分页器。

## 使用

`Button(slot, {label:'保存', variant:'default', onPress:save})` 表达主操作；outline 为默认兼容变体，secondary/ghost/destructive 为其他变体。静态 HTML 使用同一 data-variant 契约；现有 primary/danger 类保留兼容。

按钮等待时 aria-busy + 禁用防重入，页面销毁后不再更新 DOM。Field 的错误用 aria-invalid/aria-describedby 关联。可操作表格的首列提供真实按钮，支持键盘 Enter/Space。

Item 的点击语义：进入详情／目录／项目的条目通过 `primaryAction` 渲染一个真实 button 或 a，覆盖图标、标题、说明及内容区留白，以箭头、悬停背景和完整焦点框标示入口。右侧切换、移除和菜单是主入口的兄弟节点，不嵌套、不共用事件；不使用透明伪元素扩大点击区域或 article 的 onclick 模拟按钮。勾选条目由 label 包住完整内容，点击整条选择；纯说明条目不显示指针或入口箭头。目录条目与按钮使用统一控件高度；小屏行的可点击高度不小于 44px。

状态只由共享 `.item-surface` 定义：hover 使用 muted，按下使用 pressed，选中使用 accent，焦点描边位于同一个命中框内。通用按钮 hover 不作用于它；页面组合不创建状态颜色；业务组件的响应式间距使用共享 Token。hover、按下、选中、focus 不改变边框占位、字重或宽高。目录沿用紧凑导航尺寸，但名称按钮的 hover／选中／键盘焦点也共用该状态层；展开和新增按钮是独立操作。`#/samples` 顶部提供实际 button／a／label／选中／禁用对照，记录真实事件下的尺寸和背景；不使用伪造状态截图代替验证。

全局技能的 Agent 开关使用 20px 品牌 SVG，按钮为桌面 34px／小屏 44px。开启显示品牌原色，关闭为灰色；不显示对勾、重复名称或选中底色。列表、添加弹窗与详情复用同一生成函数，所有宽度直接操作。使用原生按钮加 `aria-pressed`，名称及状态放在可访问名称与悬停提示中；禁用、忙碌和失败沿用真实部署逻辑。品牌色与关闭态颜色集中在 `tokens.css`。勾选“哪些技能”仍使用复选框，保持与“在哪个 Agent 开启”的图标开关不同的操作语义。

项目能力与本地文件页共用配置目录：Git 项目显示项目默认、Worktree 及已注册子目录，文件夹项目没有 Worktree 层。共享项目设置隐藏目录树，分为基本信息、项目说明、知识库。小屏通过配置目录选择器打开树。资源库的来源树独立，通过“全部来源”展开。资源库、全局技能和本地能力列表连续滚动；已选能力与添加弹窗在列表区连续滚动。没有每页条数、上一页／下一页或加载更多动作；勾选用完整资源 ID 保存，滚动和筛选保留选择。页面列表由浏览器延迟绘制屏外条目，内容高度仍自动撑开；选择弹窗使用视口比例约束滚动区，确认按钮保持可达。项目列表与操作记录仍可使用分页。同名不同来源不合并。全局去重依据 `effective.global_deployments` 的资源 ID／工具对，覆盖当前全部工具才标记为已全局可用。

Dialog 基于原生 dialog.showModal，进入 top layer 并让背景 inert；新建项目、能力选择、资源导入使用相同的标题/内容/底部动作区，Esc、取消和关闭按钮返回触发控件。请求期间不允许关闭表单。删除、更新和放弃修改使用同一 confirmAction，而不是浏览器 confirm。浏览器离站 beforeunload 保留系统提示，这是浏览器限制。

搜索 input 与选择器使用同一高度、边框、背景、字体、左内边距和焦点规则。`components/select.js` 将页面中的单选 select 适配为 HTML combobox/listbox；原 select 隐藏，仅保留表单值和 change 事件兼容。选项菜单使用 Popover top layer，避免被 dialog 遮挡，支持方向键、Home/End、Enter、Esc、Tab 和输入字符定位。动态选项、禁用状态和程序赋值同步到组件；卸载时清理监听。当前不支持多选 select。

交互参考 [WAI-ARIA select-only combobox](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/examples/combobox-select-only/)；浮层使用 [Popover API](https://developer.mozilla.org/en-US/docs/Web/API/Popover_API)。已验证当前 Chrome，尚未验证旧版 WebView 兼容性。

更多操作采用原生 Popover，定位限制在视口中，并在 Escape／点击外部后关闭；选择操作前先关闭菜单、恢复到可见的菜单触发按钮，再打开弹窗。

当前只提供浅色主题，不宣称已完成深色主题或完整屏幕阅读器审计。配色为当前 SmallPen 的冷灰与靛蓝方案，不声称来自 shadcn 某个预设的逐值复制。

## 防回归

`node --experimental-vm-modules --test tests/frontend_components.mjs` 检查按钮生命周期、表单语义和活动 CSS 的 Token 完整性；浏览器测试检查项目主流程、组件样例、导入面板与窄屏布局。`scripts/ui-browser.sh` 还运行 `v27-flow` 的隔离真实选用／预览／应用／记录流程，以及 `v27` 的 200 条资源场景、滚动至末项、筛选后保留选择、同名来源区分、工具与文件弹窗、1440／1024／390 和 320px 边界。大数据视觉场景只在测试标签页替换 API；生产页面没有假数据回退。
