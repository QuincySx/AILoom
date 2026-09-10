# 试用摩擦日志

> 规则：一条一行，随手记。格式：`- YYYY-MM-DD | 现象（一句话）| 严重度（高/中/低）| 建议方向`
> 两周复盘时逐条转成工单；不评判、先记录。

## 日志

- 2026-09-11 | 试用开始 | - | -
- 2026-09-11 | 内置召回提示片段默认写 AGENTS.md，与 alva 项目手工维护的 AGENTS.md 冲突 | 中 | 内置片段默认关闭或改写 .claude 侧（已用 init --no-builtin 解决；候选：内置片段改默认不进 AGENTS.md）
- 2026-09-11 | 项目级 learning 需要 manifest 声明非共享 namespace，否则 contribute 报错 | 低 | 文档说明；或 contribute 自动提示可用 namespace
- 2026-09-11 | 合并贡献分支需手工 cherry-pick（单人无 PR 流时） | 低 | contribute --auto-merge 单人快捷路径（候选卡）
