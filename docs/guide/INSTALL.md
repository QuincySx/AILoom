# 安装、升级与卸载

## 安装

用 npm（需要 Node 20 及以上；包里带着 macOS、Linux、Windows 的预编译版本，约 21 MB）：

```bash
npm install -g --ignore-scripts ailoom-cli
ailoom version
ailoom web                  # 启动后台服务并打开网页
```

或从源码安装（需要 Rust 1.85 及以上，[rustup](https://rustup.rs) 一条命令装好）：

```bash
git clone <本仓库> && cd ailoom
scripts/install.sh          # 编译并安装到 ~/.cargo/bin/ailoom
ailoom version
ailoom web                  # 启动后台服务并打开网页
```

`scripts/install.sh` 也能从自建的下载源安装预编译版本（`AILOOM_DOWNLOAD_BASE=<下载地址>`，下载、校验 sha256 后安装到 `~/.ailoom/bin/ailoom`）；项目本身不发布 GitHub Release。

安装过程中任何一步失败都会保留原来的安装。

## 升级

npm 安装只更新程序包，已经运行的网页服务需要用新版本程序重启。常规版本使用 `latest`；试装候选版时，将它替换为明确的候选版本号。

```bash
npm install -g --ignore-scripts ailoom-cli@latest
ailoom service restart
ailoom --json service status
```

使用 mise 升级当前候选版时，明确指定执行重启和核对的版本：

```bash
npm_config_ignore_scripts=true mise use -g --pin npm:ailoom-cli@0.4.0-rc.14
mise exec npm:ailoom-cli@0.4.0-rc.14 -- ailoom service restart
mise exec npm:ailoom-cli@0.4.0-rc.14 -- ailoom --json service status
```

mise 的 shell 激活通常在下一次命令提示符出现时刷新 PATH；连续执行安装和普通 `ailoom service restart`，可能仍由旧版本重启自己。`mise exec` 为这次命令明确选择版本，不依赖提示符刷新。见 [mise 官方说明](https://mise.jdx.dev/faq.html)。

核对状态中的 `build.version` 是否为安装的版本、`update_required` 是否为 `false`，然后刷新网页。若出现 `E9101`，先核对命令版本并用新版本执行重启；只有状态明确为 `unmanaged`、无法自动停止时，才按错误提示处理旧进程。

脚本安装的重新运行一次 `scripts/install.sh`，默认会尝试重启正在运行的服务；`AILOOM_INSTALL_RESTART=0` 可关闭自动重启。关闭后，手动执行上述重启和状态核对。

旧版本留下的数据，新版本直接就能用，不需要手工迁移；个别格式会在第一次同步时自动更新。

## 数据在哪里、怎么备份

| 内容 | 默认位置 | 备份 |
|---|---|---|
| 个人配置、资源库、操作记录、项目知识库（默认位置时） | `~/.local/state/ailoom`（设置了 `XDG_STATE_HOME` 时为 `$XDG_STATE_HOME/ailoom`；`--data-root` 可另行指定） | **需要备份** |
| Skill 实体缓存 | `~/.local/share/ailoom/store`（设置了 `XDG_DATA_HOME` 时为 `$XDG_DATA_HOME/ailoom/store`） | 不需要，重新同步会恢复 |
| 部署到项目和全局目录里的文件 | 各项目的 `.claude/`、`.agents/` 等，以及 `~/.claude/skills`、`~/.agents/skills` | 不需要，重新同步会恢复 |

备份就是复制数据目录。知识库放在自定义位置的，也一起备份那个目录，换机恢复见 [项目与知识库恢复](KNOWLEDGE-PORTABILITY.md)。

不要把数据目录放在 `/tmp` 这类临时目录里，系统会定期清理。

## 卸载

```bash
ailoom service disable && ailoom service stop    # 关闭登录自启动并停止网页服务
ailoom uninstall --execute                       # 在每个项目里运行：移除 AILoom 部署的文件（你改过的文件会保留）
cargo uninstall ailoom                           # 删除程序（npm 安装的：npm uninstall -g ailoom-cli；下载安装的：scripts/install.sh uninstall）
```

卸载不会删除你的数据。确定不要了，再手动删除上表里的数据目录。

## 遇到问题

- `ailoom diagnose`：生成一份诊断报告文件，反馈问题时附上它。网页上在右上角「服务 → 导出诊断信息」。报告里没有你的用户名、令牌，也没有资源正文。
- `ailoom doctor`：检查当前项目的配置与部署是否正常。
- 网页服务的日志：数据目录下的 `service/service.log`。

## 平台支持

| 平台 | 状态 |
|---|---|
| macOS（Apple 芯片 / Intel） | 可用，日常开发与测试都在 macOS 上 |
| Linux（x86_64 / arm64） | CI 在 Ubuntu 上跑全部测试；未经人工完整验证 |
| Windows | 未验证；后台服务与登录自启动不支持，可以用 `ailoom console` 前台运行 |

