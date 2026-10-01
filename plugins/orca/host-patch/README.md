# 实验性 ORCA 面板通信补丁

状态：补丁可应用性已检查；**尚未在完整 ORCA 工程中编译、测试或安装，不能视作已交付的宿主扩展。** `base-revision.txt` 记录完整源码检出的 revision；已在该检出中执行 `git apply --check`，通过。验证检出位于 `/tmp/ailoom-orca-host-validation`。

补丁增加 `commands.invokeOwn`：

- 新增 `commands:own` 权限及用户可见说明，使用原有权限与审计入口。
- 参数只接收 `commandId` 和 `args`，拒绝 `pluginId` 等额外字段。
- 插件身份来自原有面板会话，不能让面板指定其他插件。
- 调用原有 `invokeCommand`，保留启用、信任和已声明命令检查。
- 不修改 CSP，不增加 HTTP，不暴露通用文件/进程 API。

在单独 ORCA 源码检出中执行 `git apply --check` / `git apply` 后，必须运行其插件单元测试和完整类型检查。尤其验证：未授权、跨插件参数、失效会话、未声明命令、超大返回值、worker 超时与递归调用。

通过宿主测试后，将 AILoom 插件复制到独立打包目录，并用 `orca-plugin.panel.json` 替换该副本中的 `orca-plugin.json`。不要直接覆盖系统安装的 ORCA 或修改用户的信任记录。安装与新增权限按 ORCA 自身流程进行。

面板脚本测试使用 DOM/host 替身；实际 UI 与宿主通信仍待真机验证。默认命令版 manifest 保持不依赖新增权限。
