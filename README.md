# AILoom

以 Rust 构建的项目与团队 AI 资源、知识和协作工具。

**状态：2026-09-17 需求实现对齐：98 张主卡，58 Backlog、4 Blocked、36 Done。**
新增47张执行卡（28张功能修复/补齐/验收 + 19张Web分层实施），11张旧主卡已根据复审重新打开。执行者从[本轮入口](docs/initiatives/implementation-alignment-2026-09-17.md)开始，Web按[Token→组件→状态→页面蓝图](docs/initiatives/web-console-blueprint-2026-09-17.md)逐步实施；全局状态见[任务看板](docs/BACKLOG.md)。旧测试通过不代表本轮需求已验收。

## 快速开始

```bash
cargo build --release
target/release/ailoom --help
```

完整流程（绑定→同步→检索→贡献→观测→卸载）见 [docs/QUICKSTART.md](docs/QUICKSTART.md)。

## 能力概览

- **项目资源**：Git/本地/同仓团队源、版本锁定、离线可用、计划/同步/冲突保留/journal 恢复（AIL-003—008）
- **宿主适配**：Claude Code 与 Codex 的 Skills/Rules/Agent/MCP（发现路径按官方文档核实，见 [docs/capabilities/](docs/capabilities/)）（AIL-009—012）
- **诊断**：doctor 体检、按托管清单安全卸载（AIL-013）
- **知识闭环**：经验贡献（明确归属）、关键词+中文索引、项目隔离召回（AIL-014—017）
- **可观测**：Hook 事件协议、会话/Token/干预聚合、摩擦提示（每会话一次）、本地 loopback 看板、Git 报告分支统计（默认关闭）（AIL-018—023）
- **NEXT/LATER**：成员名册、多源订阅（标签订阅/排除）、Rust 代码图谱与图召回、知识反馈与维护、npm 分发设计、管理后端设计、环境配置、团队 Hook、包依赖、批量导入、PR 知识候选（AIL-024—037）

## 任务看板与文档

- [任务看板与实施顺序](docs/BACKLOG.md)（当前状态以 cards.json 为准）
- [返工派工单](docs/REWORK.md) · [完成度与代码审查报告](docs/reviews/2026-09-15-agent-implementation-review.md)
- [公共契约 v1](docs/CONTRACTS.md)（文件格式/错误码/冲突矩阵）
- [能力矩阵](docs/capabilities/)（官方来源与核实日期；未知标 unknown）
- [快速上手](docs/QUICKSTART.md) · [支持范围](docs/SUPPORT.md) · [发布检查单](docs/RELEASE-CHECKLIST.md) · [安装](docs/INSTALL.md)
- [管理后端设计](docs/backend/architecture.md)（仅设计，AIL-030）

## 验证

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked --all-features
```

2026-09-15 第二轮返工：286 个测试通过 / 0 失败 / 1 ignored（真实 npm registry 用例），fmt/Clippy 干净；S01–S04、R01–R15 全部反例以真实入口修复并回归（[返工派工单](docs/REWORK.md)、[子卡索引](docs/rework/2026-09-15/README.md)）。2026-09-13 真机验收记录保留；Linux/Windows、真实 GitHub 写入仍未验证，边界见各卡。逐卡证据见各卡「重新关卡记录」。

后续能力独立保留，不以同步器代替完整产品目标。

- [本地控制台与 Onboarding 需求规格（本轮返工中）](docs/initiatives/local-console.md)
- [个人配置/Git 需求及回复审查](docs/reviews/2026-09-15-onboarding-requirements-review.md)
