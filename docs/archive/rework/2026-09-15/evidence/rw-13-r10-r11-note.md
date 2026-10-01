# RW-13 · R10/R11 修复与验证（2026-09-15，ZCode）

## 机制（src/import/pr.rs advance_graph_baseline 重写）

- R10 项目目标：显式 --project 参数校验工作区绑定；未指定 → skipped（不隐式选
  第一个绑定项目）；未绑定 → skipped（注明绑定列表）。
- R10 快照：合并 head 必须在本仓对象库（cat-file 校验）；物化为 detached
  worktree 后扫描构图并记录 revision=head——图内容与版本证据严格对应，
  本地 checkout 落后不冒充；head 不可得 → 显式 pending（不写成功标记）。
- R11 重试：成功标记（state=success）仅在 load/build/save 全部成功后落盘；
  任一失败错误上抛不写标记；dedup 分支（草稿已存在且 merged 状态未变）也调用
  幂等 advance——上次失败后重试可在此完成；成功后重复 → already-advanced。
  旧版本无 state 字段的标记沿用"已推进"语义（兼容）。

## 验证（tests/import_pr.rs）

- 既有 pr_state_transitions 用例改用对象库中真实合并 head（fake sha 无法满足
  快照可证要求），并断言标记含 state=success。
- 新增 pr_merge_graph_baseline_targeting_snapshot_and_retry：
  - 双项目绑定 [a,b]：未指定项目 → skipped（注明原因）；未绑定 c → skipped；
  - --project b → advanced：codegraph-b.json revision=合并 head、files 含
    工作区不存在的 src/extra.rs（快照证明）、codegraph-a.json 未创建；
  - 幂等：删除草稿后重放同 (PR,head) → already-advanced；
  - pending：伪造不在对象库的 head → pending 且无成功标记；
  - 重试：图文件损坏 → CLI 非零且无成功标记 → 修复后重试 advanced+标记。

回归：cargo test --test import_pr → 13 passed；fmt/clippy 通过。
