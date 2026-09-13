# AILoom

以 Rust 构建的项目与团队 AI 资源、知识和协作工具。

**状态：2026-09-13 返工轮次完成：34 张 Done、3 张 Blocked（AIL-009/012/014，外部授权或宿主能力阻塞）、0 张 Backlog。**
验证：253 测试全绿 + fmt/Clippy 干净（[全量日志](docs/evidence/logs/2026-09-13-full-validation.log)）；Claude Code/Codex 真机验收证据见 [宿主验收记录](docs/evidence/2026-09-13-host-acceptance.md)。执行者从 [任务看板](docs/BACKLOG.md) 领取未关闭项。

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

- [任务看板与实施顺序](docs/BACKLOG.md)（3 张 Blocked，`cards.json` 为状态真相）
- [返工派工单](docs/REWORK.md) · [完成度与代码审查报告](docs/reviews/2026-09-12-completion-review.md)
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

2026-09-13 返工轮次：253 个测试全部通过，fmt/Clippy 干净；Claude Code 2.1.266 与 Codex 0.153.4 真机验收完成（skill/规则/agent/MCP/hooks/SSE/召回），Codex 项目级 skills/MCP 实测不加载已降级记录。逐卡证据见各卡「重新关卡记录」。

后续能力独立保留，不以同步器代替完整产品目标。
