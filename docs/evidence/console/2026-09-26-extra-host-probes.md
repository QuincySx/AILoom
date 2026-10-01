# 本机宿主适配检查（2026-09-26）

## 环境与范围

使用独立临时项目、独立 AILoom data/store 和合成资源源；资源由真实 `ailoom init` + `ailoom sync` 生成，未手工替代适配器输出。没有更改业务项目文件、登录信息或安装/升级宿主。

- Grok Build：`1.0.3 (1a29d5bc12d4)`。本机 `agent` 命令也是 Grok，不能用它冒充 Cursor CLI。
- OpenCode：`1.15.11`，来自 Zed external_agents 安装目录，不在当前 PATH 中。诊断与请求均带 `--pure`，不运行第三方插件。
- Cursor 桌面：`3.21.16 (8ae78e8eee1e63479c7e0504b664bc0a80c68000, arm64)`。没有找到独立 cursor-agent；`cursor agent --help` 启动器尝试自动安装，已中止，未完成安装。

## Grok

- `inspect --json` 识别 `.grok/skills/ailoom-probe/SKILL.md`、`.grok/agents/ailoom-probe.md`、`.grok/rules/ailoom-personal.md` 和项目 MCP 配置。
- 无工具单轮请求从项目规则返回 `RULE-PROBE-84B1`。
- 实际技能请求返回 `SKILL-PROBE-7C29`。
- `--agent ailoom-probe` 实际请求返回配置中的 `AGENT-PROBE-61A3`。此项验证自定义代理入口；未验证由父代理自主选择并委派子代理。
- 对照：只移开 `.grok/skills` 中的测试链接时，Grok 会从 `.cursor/skills` 兼容加载同名技能；移开所有兼容目录中的测试链接后，该技能消失。所有测试链接已恢复。
- 首次 MCP doctor 与实际会话均被未信任目录保护拦住。doctor 提示的 `--trust` 未改变本机版本结果；通过 Grok 原生交互中的目录信任确认后，doctor 显示命令存在、服务启动、协议握手成功、发现 1 个工具，`healthy: true`。仅信任了合成测试目录。
- 信任后重新发起实际 MCP 请求，3 个模型轮次完成并返回 `MCP-PROBE-19D2`，与合成服务返回值一致。

## OpenCode

- `debug skill` 返回 AILoom 生成的项目技能路径及准确正文；移开该链接后技能消失，随后恢复。
- `debug agent ailoom-probe` 返回 `mode: subagent`、正确描述及 `AGENT-PROBE-61A3` 指令正文。
- `mcp list` 显示测试服务 `ailoom-probe connected`。
- 模型请求返回 `APIError`，HTTP 401，`Upstream request failed: Invalid credential`。没有读取或更改凭据，也没有切换用户模型/供应商。规则实际遵循、Skill/Agent/MCP 模型调用仍待有效登录后验收。
- 注意：`debug skill` 的大量输出写入 pipe 时出现 JSON 截断；写入临时文件再解析能得到完整结果。这是探测输出问题，不是资源缺失。

## Cursor

桌面应用可启动并截图，但当前系统没有可操作的焦点窗口，computer-use 两次点击均返回 `window_not_focused`（包括带 restore-window 的重试）。应用激活后仍未获得焦点。未冒充成功，未向用户的业务项目发送测试请求。用户随后已将 Cursor 切到前台；再次截图可见应用，但点击仍返回 `window_not_focused`。权限诊断显示 accessibility/screenshots 均 granted。备用原生控制接口启动失败（`Sky Computer Use native pipe startup failed`）。因此阻塞点是自动化控制链路，不能归因于用户未切前台；本轮资源加载与实际调用验收尚未完成。

## 结论边界

配置生成、宿主发现、模型调用分别记录；一个层级成功不能代替其他层级。测试仅覆盖无条件规则、基础 Skill、基础代理和本地 stdio MCP；不代表远程 MCP、复杂权限、模型映射、条件规则全部通过。
