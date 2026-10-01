# AILoom

以 Rust 构建的本地优先工具，把项目与团队的 AI 资源（Skill、Rules、Agent、MCP）、知识与协作经验统一管理，并部署到各家 AI 编程工具。

> 当前迭代：[2026-10 稳定与收敛迭代](docs/initiatives/iteration-2026-10.md) · 任务状态见 [任务看板](docs/BACKLOG.md)（以 [cards.json](docs/cards.json) 为准）

## 快速开始

```bash
cargo build --release
target/release/ailoom --help
```

CLI 命令执行完即退出，无需常驻服务。网页按需启动：

```sh
ailoom web                  # 启动或复用后台服务并打开网页
ailoom service stop         # 停止网页服务
ailoom service enable       # 可选：登录时自动启动（会写入系统登录项）
ailoom service disable      # 关闭登录自启动
```

上手流程见 [快速上手](docs/guide/QUICKSTART.md)；网页服务的命令、平台支持与日志见 [CLI 与网页服务](docs/guide/WEB-SERVICE.md)。

## 能力概览

- **团队资源**：Git / 本地 / 同仓团队源，版本锁定、离线可用；计划 → 同步 → 冲突保留 → journal 恢复。
- **个人层**：仓外个人配置，按仓库 / Worktree / 子目录三态选择资源与宿主；资源库、资源合集、CC Switch 来源迁移。
- **宿主适配**：Claude Code、Codex 为主要宿主；Grok、Pi、OpenCode、Cursor 等为附加宿主，能力与核实状态见 [能力矩阵](docs/capabilities/)。
- **原生文件**：在网页里直接编辑各宿主的 Rules / Agent 原生文件，见 [原生文件](docs/guide/NATIVE-FILES.md)。
- **知识闭环**：经验贡献、中文检索与项目隔离召回；知识库位置迁移与换机恢复，见 [项目与知识库恢复](docs/guide/KNOWLEDGE-PORTABILITY.md)。
- **观测**：Hook 事件、会话 / Token 聚合、摩擦提示、本地 loopback 看板。
- **网页控制台**：目录优先的项目工作台、全局规则与 Agent、资源库、操作记录。
- **ORCA 接入层**（实验）：见 [plugins/orca](plugins/orca/README.md)。

## 文档

- [文档地图](docs/README.md)：用户手册、契约、能力矩阵、开发流程与历史归档的入口。
- [公共契约](docs/CONTRACTS.md)：文件格式、错误码、退出码与冲突矩阵。
- [开发与关卡流程](docs/IMPLEMENTATION-GUIDE.md) · [发布检查单](docs/RELEASE-CHECKLIST.md)

## 验证

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked --all-features
python3 scripts/docs_check.py      # 断链、卡片状态一致性、看板生成区
```

测试数量与逐卡证据记录在各卡片与 [docs/reviews/](docs/reviews/)，README 不再维护快照数字。
