# 文档地图

每类文档只有一个入口。产品文档描述「现在是什么」；卡片、评审与证据记录「怎么做到的」；归档只供追溯，不代表现状。

## 用户手册 `guide/`

| 文档 | 内容 |
|---|---|
| [快速上手](guide/QUICKSTART.md) | 唯一的上手文档：安装、绑定、同步、网页、个人层 |
| [安装](guide/INSTALL.md) | 构建、安装脚本与分发方式 |
| [CLI 与网页服务](guide/WEB-SERVICE.md) | `ailoom web` / `service`、平台支持、日志 |
| [原生文件](guide/NATIVE-FILES.md) | 在网页中编辑各宿主 Rules / Agent 原生文件 |
| [项目与知识库恢复](guide/KNOWLEDGE-PORTABILITY.md) | 知识库位置、迁移与换机恢复 |
| [支持范围](guide/SUPPORT.md) | 平台、宿主与已知限制 |

## 契约与能力事实

- [公共契约](CONTRACTS.md)：文件格式、`schema_version`、错误码与退出码、冲突矩阵、变更记录。
- [术语表 CONTEXT.md](../CONTEXT.md)：领域术语以此为准，UI 与 CLI 文案跟随。
- [能力矩阵 `capabilities/`](capabilities/)：各宿主的官方来源、核实日期与支持程度——[Claude Code](capabilities/claude-code.md) · [Codex](capabilities/codex.md) · [附加宿主](capabilities/extra-hosts.md) · [Cursor / Antigravity（旧）](capabilities/cursor-antigravity.md) · [alva](capabilities/alva.md)。
- 设计规格 `specs/`：[资源管理生命周期](specs/resource-management-lifecycle.md) · [项目优先的控制台](specs/project-first-console.md) · [控制台组件与样式](specs/console-design-system.md) · [CC Switch 来源迁移](specs/cc-switch-source-migration.md) · [SkillStore 与 Require](specs/0001-skill-store-require.md)。
- 架构决策 `adr/`：[Skill 实体进 Store](adr/0001-skill-store-symlink.md) · [仓库身份与个人层](adr/0002-repository-scope-personal-layer.md)。
- 范围：[范围与关键约束](SCOPE.md) · [参考能力覆盖表](COVERAGE.md) · [参考证据](REFERENCES.md)。

## 开发与交付

- [开发与关卡流程](IMPLEMENTATION-GUIDE.md)：领卡、实施、验收与状态纪律。
- [任务看板](BACKLOG.md)：总表由 `scripts/docs_check.py --write` 从 [cards.json](cards.json) 生成，禁止手改生成区。
- 当前迭代入口 `initiatives/`：[2026-10 稳定与收敛迭代](initiatives/iteration-2026-10.md)。
- 卡片 `cards/`、评审 `reviews/`（按日期）、证据 `evidence/`（截图、日志、复现脚本）。
- [发布检查单](RELEASE-CHECKLIST.md) · 契约夹具 `fixtures/`。

## 设计稿 `design/`

[管理后端设计（仅设计，未实施）](design/backend/architecture.md) · [目录优先线稿](design/wireframes/directory-ux/index.html) · [业务逻辑线稿](design/wireframes/business-logic.html) · [知识生命周期原型](design/prototypes/knowledge-lifecycle/README.md)。

## 历史归档 `archive/`

见 [归档说明](archive/README.md)。归档内容保留原貌供追溯，其中的链接与结论不保证仍然成立。

## 维护规则

1. 新文档先确定归属的类别；产品文档不写卡片编号、测试次数和执行轮次。
2. 状态只改 `cards.json` 和卡头，然后运行 `python3 scripts/docs_check.py --write`。
3. 一轮迭代结束后，把该轮入口移入 `archive/initiatives/`。
