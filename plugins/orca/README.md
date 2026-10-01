# AILoom ORCA 接入层

AILoom 仍是独立产品：CLI、网页和可选后台服务不依赖 ORCA。这个目录只新增一个调用现有 CLI 的插件 worker，不启动 HTTP、不复制业务逻辑，也不替代网页。

## 当前交付边界

已实现 manifest、命令注册、插件内项目绑定、真实 CLI 子进程调用、JSON 结果解析、错误与超时反馈，以及预览后应用。Node 标准库即可运行，没有 npm 依赖。

**这是可测试的命令接入层，尚不是安装后即可点击使用的完整插件。** ORCA 当前公开 panel API 没有调用自有 worker 的接口；命令面板也未验证支持配置命令的参数输入。已经提供实验性 `panel.html`（项目绑定、预览、应用和结果展示），以及 `host-patch/` 中的配套宿主补丁。默认 manifest 不启用实验面板；不能在未打补丁的发行版上宣称可用。

插件启动后由支持参数的宿主命令调用方调用 `ailoom-configure`：

```json
{
  "root": "/absolute/path/to/project",
  "executable": "/absolute/path/to/ailoom",
  "dataRoot": "/absolute/path/to/optional-data-root"
}
```

`dataRoot` 可省略（使用 AILoom 默认存储），`executable` 可省略（从 PATH 找 ailoom）。桌面 worker 的 PATH 可能与终端不同，建议用绝对路径。配置仅存在 ORCA 的本机插件存储中，不写入业务项目或随知识库同步。

命令始终操作**显式绑定的项目**，不随前台工作区自动切换、不猜测目录。返回结果和通知包含目标路径。当前仅支持本机目录，不能把远程工作区当作本机路径使用。

| 命令 | 行为 |
| --- | --- |
| ailoom-configure | 检查路径及 CLI，保存绑定 |
| ailoom-status | 检查绑定项目 |
| ailoom-plan | 返回结构化变更预览 |
| ailoom-sync | 重新检查预览是否变化，再调用 sync |
| ailoom-recover | 调用 sync --recover，恢复未完成 journal |

同一 worker 内串行执行。应用、恢复和重新绑定会清除旧预览；worker 重启后需重新预览。重新预览检查不是文件系统事务，实际写入安全仍由 AILoom CLI 的锁、冲突检测和 journal 保证。超时后不自动重试写操作，应先检查状态。

## 验证

在仓库根运行：

```sh
cargo build --locked --offline
node --test plugins/orca/bridge.test.mjs
cargo test --locked --offline --test cli
```

集成测试创建隔离 Git 项目和独立数据目录，调用真实 AILoom 生成团队源、初始化 Cursor 配置，再通过 worker 完成绑定、预览与同步。ORCA host API 使用测试替身；这不等同于真实 ORCA 安装测试。

## 后续宿主接线

完整界面需要 ORCA 支持权限明确的 panel → own worker 请求/响应，以及项目稳定身份、路径与执行主机信息。届时将界面连接到本目录的命令即可；无需增加 HTTP 或修改 CLI 业务语义。还需验证宿主安装、信任确认、命令参数及结果展示，再提供用户可验收的插件包。

来源：
- https://github.com/stablyai/orca/blob/main/examples/plugins/hello-orca/main.mjs
- https://github.com/stablyai/orca/blob/main/src/shared/plugins/plugin-host-api.ts
- https://github.com/stablyai/orca/blob/main/src/shared/plugins/plugin-manifest.ts
