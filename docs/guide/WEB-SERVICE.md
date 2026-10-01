# CLI 与网页服务

两个入口独立运行，共用项目配置、资源库和知识库。只安装一个 `ailoom` 可执行文件；普通 CLI 不需要网页后台。

## 日常使用

```sh
ailoom web                 # 自动启动或复用后台，并打开网页
ailoom web --no-open       # 不打开浏览器，只输出访问地址
ailoom service start       # 只启动后台
ailoom service status      # 运行状态、端口和登录自启动设置
ailoom service stop        # 等待现有任务完成后退出
```

关闭浏览器标签后服务继续运行。网页右上角「服务」可以查看状态、切换登录自启动、停止服务。停止网页服务不影响之后的 CLI 命令；未保存的网页表单需要先保存。

`ailoom console --no-open` 保留为前台调试入口，Ctrl+C 结束。它也遵守单实例规则：已有服务时不会另开一份，改用 `ailoom web` 进入现有服务。

## 登录自启动（默认关闭）

```sh
ailoom service enable      # 下次登录启动；不启动当前服务
ailoom service disable     # 取消下次登录启动；不停止当前服务
```

- macOS：当前用户的 `~/Library/LaunchAgents/dev.ailoom.web.<id>.plist`，由 launchd 在登录时启动。
- Linux：当前用户的 systemd 服务，要求可用的 `systemctl --user` 会话。服务文件位于 `$XDG_CONFIG_HOME/systemd/user` 或 `~/.config/systemd/user`。
- Windows：支持手动后台启动、打开网页和停止；登录自启动暂不支持，网页开关会禁用。

无需管理员权限。配置记录二进制、数据目录和资源存储的绝对路径；二进制安装位置变更后，重新执行 `service enable` 更新配置。停止后不会立即被自启动设置重新拉起；自启动不是进程崩溃监视器。

卸载前先执行 `service disable`，再执行 `service stop`。这两条命令不会删除项目、能力或知识库。

## 数据目录、端口与日志

```sh
ailoom --data-root /path/to/data web --port 8650
ailoom --data-root /path/to/data service status
ailoom --data-root /path/to/data service stop
```

同一个数据目录只运行一个网页服务，管理其中所有项目。不同数据目录相互独立。`--port` 是新实例的首选端口，默认 47831；占用时最多顺延 49 个端口。复用现有实例时保留现有端口；如果显式传入的 `--port` 与运行中的端口不同，会给出提示，JSON 结果中带 `requested_port`。

服务只监听 `127.0.0.1`。运行状态与日志位于 `<data-root>/service/`，后台日志为 `service.log`；启动时大于 4 MiB 的日志保留为 `service.previous.log`。Unix 下该目录权限为 0700，会话状态文件为 0600。访问地址、状态输出和日志都不包含会话令牌：页面打开时由本机服务注入，写操作必须携带它。

停止时拒绝新写入，等待已接收的请求及后台任务完成。CLI 最多等 30 秒，超时会说明仍在停止，不强杀进程；可再次查看 `service status`。进程锁是判断服务是否存在的依据，不会仅根据旧 PID 结束进程。

## 验证范围

已覆盖真实 CLI 子进程的独立运行、后台启动和复用、并发启动、端口冲突、隔离数据目录、异常退出恢复、信号退出、受保护的控制 API、停止时等待请求和任务。自启动配置覆盖生成、macOS plist 语法校验、启停配置及注册失败回滚；测试不注册真实用户登录服务，也不模拟实际注销/重新登录。Linux 和 Windows 进程行为仍需对应系统实机验收。

平台行为参考：[Apple launchd](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html)、[systemd systemctl](https://www.freedesktop.org/software/systemd/man/latest/systemctl.html)。

测试：`tests/service.rs`（服务进程）、`tests/console_*.rs`（控制台 API 与安全）、`scripts/ui-browser.sh`（浏览器验收）。逐轮验证记录见 [evidence/](../evidence/) 与相关卡片。
