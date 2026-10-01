# AILoom：ORCA 插件可行性与 Cursor 实测

记录日期：2026-09-27（Cursor 实际会话开始于 2026-09-26 23:55 JST）。

## 结论

1. Cursor 的基础 Rules、Skill、自定义子 Agent、stdio MCP 均完成真实会话验证。MCP 需要在 Cursor 中启用，写入配置不等于已启用。
2. AILoom 可以做 ORCA 集成插件，但现有插件 API 不足以直接承载完整管理网页。先保留 Rust 核心与 CLI，做命令/事件集成；完整原生面板需补宿主 API。
3. 不建议现在删除 AILoom 网页服务、宿主适配器或知识库同步核心。本轮只调研，没有安装插件、重构或删代码。

## Cursor 验收

沿用前轮由真实 ailoom init + sync 生成的隔离项目：
/private/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/ailoom-host-check-i902vrv_/project

使用 Cursor 桌面 UI 的 computer use，未向用户业务项目发送测试请求。会话名：AILoom adapter verification；界面模型为 Grok 4.7 High。

| 项目 | 观察到的证据 | 结论 |
| --- | --- | --- |
| Rules | Customize → Rules 列出项目 AGENTS 和 ailoom-probe；打开 .cursor/rules/ailoom-probe.mdc 确认 alwaysApply: true；会话返回 RULE-PROBE-84B1 | 原生规则发现与项目指令遵循通过；由于 AGENTS.md 含相同标记，未单独隔离证明标记仅来自 mdc |
| Skill | 会话读取 SKILL.md 并返回 SKILL-PROBE-7C29 | 基础技能实际使用通过 |
| 子 Agent | 第一轮活动中出现 AILoom probe marker 子任务、Grok 4.7 High、Completed；返回 AGENT-PROBE-61A3 | 实际委派调用通过 |
| MCP | 初次 UNAVAILABLE；Customize → MCPs 显示 ailoom-probe Disabled；详情明确来源 .cursor/mcp.json。启用后 Connected，发现 probe_ping。第二轮出现 MCP tool output 并返回 MCP-PROBE-19D2 | stdio 服务发现、连接与实际调用通过 |

测试完成后已把 ailoom-probe 的启用开关恢复为 off，保留验收会话与文件。没有处理其他 MCP 的登录，没有修改业务项目。

这不代表远程 HTTP MCP、复杂权限、模型映射、条件 Rules、所有 Cursor 版本都已通过。适配产品应区分：已生成配置 / 宿主已发现或启用 / 已实际调用。

## ORCA 插件是什么

官方 main 分支提供 orca-plugin.json，manifestVersion 1、pluginApi 1；示例位于 examples/plugins/hello-orca。

插件可贡献面板、命令、事件订阅及若干声明式内容。面板是 sandbox iframe，worker 是独立 Node 子进程。worker 入口 default export activate(orca)，可以注册命令和事件。包含 main 的插件跨入 trusted Node worker 授权层级。

源码仍明确标为 experimental，API 冻结前没有兼容性承诺。本结论来自当前官方源码和示例，未安装并运行 AILoom 插件原型；不能将源码可行性视为已验收插件。

## 当前限制：不能把网页直接塞进面板

- plugin-host-api.ts：面板仅允许 workspace.readContext、terminal.sendText、notifications.show。没有自定义面板→worker RPC；storage/settings 等调用也不向面板开放。
- workspace.readContext：只有 branch、displayName、terminals，不提供当前目录绝对路径或完整项目树。
- plugin-panel-shell.ts：connect-src 'none'，不能直接 fetch AILoom 的 localhost API；也不能靠 iframe 嵌入现有网页绕过。
- 公共宿主能力没有通用文件系统/Git/进程执行接口。可信 Node worker 可作为本地适配层调用现有 CLI，这是一条根据进程实现作出的技术推断，仍需实际插件验证；不能把它当成受细粒度 process:exec 权限保护的 API。
- 事件只有 worktree.created、worktree.removed、agent.status.changed。状态变化不是完整会话内容或知识提取 Hook，不能据此删掉全部宿主 Hooks。
- 插件私有 KV 存储总量上限 5 MiB、单值 256 KiB，不应拿来替代项目知识仓库。

## 可以复用与保留的边界

| AILoom 能力 | 插件形态下的处理 |
| --- | --- |
| 主题、面板容器、命令入口、通知、插件启停与更新 | 可复用 ORCA，插件版无需另造 |
| 当前工作区入口、Worktree 新建/删除感知 | 可利用上下文与事件；现有路径解析/API 缺口需补齐，不能立刻删完整目录管理 |
| 网页端口、浏览器启动、Web 后台服务管理 | 当前轻集成仍需保留；只有业务 RPC 接通且完整 UI 原生化后，插件发行版才可省去 |
| Skill/Rules/Agent/MCP 各宿主格式适配 | 必须保留，ORCA 托管终端不等于替我们适配资源 |
| 本地已有与托管资源区分、继承、冲突保护、应用与恢复 | 必须保留，是 AILoom 的核心业务 |
| 项目/知识库稳定 ID、个人路径绑定、迁移与 Git 同步 | 必须保留，不能搬成 ORCA 私有设置，否则换设备和离开 ORCA 就失去可移植性 |
| CLI 与必要的宿主 Hooks | 保留，支持 ORCA 之外的终端和会话 |

## 建议方案

第一步：做薄的 ORCA 集成，复用同一份 AILoom CLI/core。提供检查/恢复/应用等明确命令和事件提醒；完整管理继续使用现有 Web。工作区首次绑定需要明确项目路径，不能用显示名称当项目身份。

第二步：若要原生面板，先给 ORCA 补三个正式契约：插件面板调用自己 worker 的受控请求/响应；稳定的工作区标识及路径/位置能力；任务进度、取消和结果返回。大文件/长任务还要适配消息大小和超时预算。

第三步：再把管理 UI 改成 ORCA 面板，由 worker 调用 AILoom CLI。此时插件版可不启动 HTTP 服务，但独立版 CLI/Web 继续共享同一核心。不要分叉两套知识库和适配逻辑，也不要把隐式终端输入当通用后台 RPC。

## 官方源码依据

- 示例清单：https://github.com/stablyai/orca/blob/main/examples/plugins/hello-orca/orca-plugin.json
- 示例 worker：https://github.com/stablyai/orca/blob/main/examples/plugins/hello-orca/main.mjs
- 完整清单契约：https://github.com/stablyai/orca/blob/main/src/shared/plugins/plugin-manifest.ts
- 公共 API 与存储限制：https://github.com/stablyai/orca/blob/main/src/shared/plugins/plugin-host-api.ts
- 工作区投影与实现：https://github.com/stablyai/orca/blob/main/src/main/plugins/plugin-host-method-bindings.ts
- 面板安全策略：https://github.com/stablyai/orca/blob/main/src/shared/plugins/plugin-panel-shell.ts
- Node worker 启动：https://github.com/stablyai/orca/blob/main/src/main/plugins/plugin-host-process.ts
- worker 执行模型：https://github.com/stablyai/orca/blob/main/src/main/plugins/plugin-host-runtime.ts
- worker 信任边界：https://github.com/stablyai/orca/blob/main/src/shared/plugins/plugin-consent-fingerprint.ts

调研通过 ORCA 自己的内置浏览器完成。本轮终端执行工具因 codex-code-mode-host 文件缺失无法启动，所以没有执行构建、修改兼容矩阵或运行插件原型；本报告通过编辑器保存。
