# RW-12 · R09 反例复现与修复验证（2026-09-15，ZCode）

## 修复前

静态+代码路径确认（与审查 R09 一致）：candidate_key 含 target，但候选名
sid8 = sha256(identity\0rel) 不含 target——同一文档先导入 project:a 再 shared
会生成相同文件名，第二次执行在变更集中覆盖第一次的资源归属；
两个 checkpoint 各自认为已完成。

## 修复后（真实 CLI 验证，工作区 + 本地团队源）

- import project:a → 候选名 docsdir-postmortem-1b43fb1f；
- import shared → 候选名 docsdir-postmortem-75a3f90a（不同名字）；
- 贡献链路真实落库：cache 仓库两个独立提交分别新增
  resources/learnings/docsdir-postmortem-1b43fb1f.md（project:a）与
  resources/learnings/docsdir-postmortem-75a3f90a.md（shared）——两文件共存，
  归属互不覆盖。

## 机制与兼容

- 新候选名：sid8 = sha256(identity\0rel\0target)。
- 兼容旧 checkpoint：旧名（哈希不含 target）已存在的候选沿用旧名原位更新/跳过，
  不静默重归属、不重复发布（集成测试用真实 checkpoint 键改写验证识别）。
- 删除建议按 (identity, kind, target) 作用域遍历，天然随新名字方案作用域正确。

回归：cargo test --test import_pr → 11 passed（含新增
import_target_isolation_and_legacy_checkpoint_compat）。
