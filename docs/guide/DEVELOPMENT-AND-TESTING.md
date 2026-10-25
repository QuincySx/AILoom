# 开发、验收与日常使用

当前候选版是 `0.4.0-rc.4`，发布到 npm `next` 通道供试装；`latest` 通道仍为 `0.3.0`。候选版的已知限制见 [更新记录](../../CHANGELOG.md)。最新版本不等于稳定版本；先修阻断问题，再交付一个固定的候选包供真实使用验收。候选通过后，同一份制品进入正式发布，不在发布时重新构建另一份。

| 用途 | 执行哪个程序 | 项目与数据 | 升级节奏 |
|---|---|---|---|
| 开发调试 | `target/debug/ailoom`，本地 `cargo build --bin ailoom` | 临时项目；独立数据目录、资源存储 | 改代码后编译并重启开发服务 |
| 发布验收 | `cargo build --release` 产物封装的 npm 候选包 | 临时项目；每轮独立数据；另测旧数据副本升级 | 固定包版本与 SHA256，验收中不更换 |
| 日常使用 | npm 安装的明确版本 | 真实项目与正常数据目录 | 候选验收通过后才更新 |

## 一键安装本地测试版

macOS / Linux 已有 Rust、Node.js 时，在仓库根运行：

```sh
npm --prefix packaging/npm run install:local
ailoom-test web
```

脚本自动完成 release 编译、npm 打包、离线安装与 SHA256 核对。固定入口为 `~/.local/bin/ailoom-test`，安装包保存在 `~/.local/opt/ailoom-test/builds/`。现有 npm `ailoom` 保留，每次安装保留上一份测试包；编译、打包或验证失败不会替换测试入口。

默认继续使用你的正常数据目录，并把已经运行的网页服务切换到新安装的程序，保留原端口。安装完成后刷新网页。测试时统一运行 `ailoom-test`，用 `ailoom-test --json service status` 查看网页实际构建；修改代码后再执行同一条安装命令。

脚本只切换当前服务，登录自启动保留原入口。已经开启自启动时，安装器会打印改用测试版的命令；需要时执行它。

仅安装、暂不切换网页服务：

```sh
npm --prefix packaging/npm run install:local -- --no-restart
```

这会生成本机的私有候选包，不会发布到 npm；本地装成功仍需按下文做功能验收。脚本输出的完整入口路径也可直接使用，不依赖 PATH 是否包含 `~/.local/bin`。

## 日常使用的当前结论

`0.2.0` 已复现团队源丢失导致配置页不可用的问题，不能据此宣布稳定。本地回归验证只代表明确路径得到验证；还要完成 npm 包与真实使用验收。

日常只保留一个 `ailoom` 安装入口。查看 `command -v ailoom`，用 `ailoom --json service status` 核对网页实际运行的程序、构建 SHA256 与 `update_required`。网页「服务」也显示正在运行的版本和程序位置。

升级安装后运行 `ailoom service restart`，再刷新网页。修复版 CLI 不会静默复用不同安装位置或旧构建启动的服务。已有任务先完成，正常停止后使用原端口启动当前程序。

这里的“当前程序”指实际执行重启的程序。使用 mise 连续执行升级与重启时，PATH 可能尚未刷新，应使用 `mise exec npm:ailoom-cli@0.4.0-rc.4 -- ailoom service restart` 指定新版本，再核对 `build.version` 和 `update_required=false`；完整步骤见 [安装与升级](INSTALL.md#升级)。

## 开发入口

在仓库根运行：

```sh
cargo build --bin ailoom
XDG_STATE_HOME="$PWD/target/dev/state" XDG_DATA_HOME="$PWD/target/dev/data" \
  target/debug/ailoom service restart --port 47931
XDG_STATE_HOME="$PWD/target/dev/state" XDG_DATA_HOME="$PWD/target/dev/data" \
  target/debug/ailoom web
```

这会隔离 AILoom 的个人配置、登记、缓存和资源存储；添加临时项目进行开发。全局宿主文件属于用户目录，涉及它们的验收使用已有的 `scripts/ui-browser.sh` 完整隔离环境。不要把真实项目当作自动化测试夹具。

开发服务用完后停止：

```sh
XDG_STATE_HOME="$PWD/target/dev/state" XDG_DATA_HOME="$PWD/target/dev/data" \
  target/debug/ailoom service stop
```

## 交付给用户之前

1. 先用回归重现故障，再改代码。来源消失、缓存丢失、离线已有缓存、失败后重试都要覆盖。
2. 跑相关模块、前端状态与浏览器检查。确认失败不会误删部署文件、改写源锁或自动重放写入。
3. 编译 release，将它装进 npm 包；从打包后解出的安装入口测试，不用 debug 版代替。此轮 macOS 的结论不能扩大为其他系统通过。
4. 验证更新前已启动服务的升级路径，核对实际程序与 SHA256。
5. 交付固定候选版与已知限制。用户只验收真实工作流程：添加项目、选资源、预览、应用、重新打开。

用户验收出现阻断就停下修复。通过后发布同一制品，日常使用固定版本；下一轮功能在开发入口继续。
