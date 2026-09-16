# RW-17 · fixture 失败复现与根因定位证据（2026-09-15，ZCode）

起始代码版本 c4a56a4a4a713cb4781c7a34ca93299d1fc5d9da。

## 1. 当前测试失败复现

命令：`cargo test --locked --offline --test installer`
结果：1 passed / 5 failed（退出 101）。

- 4 项 HTTP 用例失败，stderr 均为 `curl: (7) Failed to connect to 127.0.0.1 port <P> after 0 ms: Couldn't connect to server`（端口每次随机，为 fixture 应使用的端口）。
- 第 5 项 `npm_wrapper_locates_local_artifact_and_passes_through` 在本执行环境因 PATH 无 `node`（node 由 mise 管理，位于 `~/.local/share/mise/shims/node`）在 `Command::new("node").output()` 处 NotFound panic。该失败与审查轮「2 passed / 4 failed」差异是环境 PATH 差异，不是新回归；审查环境 node 在 PATH 中。

## 2. 服务端根因定位（逐层）

1. fixture 子进程 `python3 -m http.server <port> --bind 127.0.0.1 --directory <tmp>` 由 `/usr/bin/python3`（Xcode 工具链 Python 3.9.6，实际执行体 `Python.app/Contents/MacOS/Python`）启动，进程保持存活（ps STAT=SN），无任何 stderr 输出（被 `Stdio::null()` 吞掉）。
2. `lsof -nP -iTCP:<port>` 与 `netstat -an | grep <port>` 均无监听记录；同机 python `socket.connect` 得到 `Connection refused`（errno 61）。即服务进程存活但从未完成 bind/listen。
3. 裸 `python3 -c` 的 socket `bind()/listen()` 在同一环境立即成功（毫秒级），说明不是沙箱禁止监听。
4. 计时测量（父进程轮询 connect）：
   - 第 1 次：端口在 ~23s 后才可连接；
   - 第 2 次：~14.9s 后可连接。
   即本机 `python3 -m http.server` 从 spawn 到端口就绪需 15~25 秒。
5. 原 fixture 仅轮询 50×100ms=5s，超时后**不报错继续返回 Fixture**，随后 install.sh 的 curl 立即连接被拒。

## 3. 结论

- 根因：本机 `python3 -m http.server` 启动到端口就绪耗时 15~25s（远超 fixture 5s 就绪等待），且 fixture 就绪失败被静默吞掉（stderr 进 null、超时后照常返回），导致 curl 对未就绪端口连接失败。**不是生产下载器/安装脚本缺陷**；审查轮的 4 项失败即同一 fixture 问题。
- 次要问题：(a) bind→drop→spawn 存在端口被抢的 TOCTOU 竞争；(b) 服务进程早退不会被察觉（`expect` 只覆盖 spawn，不覆盖 spawn 后立刻退出）。
- node 依赖问题属于执行环境 PATH 配置，用例本身要求成立（npm wrapper 是 AIL-029 交付物），不跳过：运行时需保证 node 在 PATH（如 mise shim 目录）。

