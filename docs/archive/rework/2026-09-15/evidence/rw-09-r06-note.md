# RW-09 · R06 修复与验证（2026-09-15，ZCode）

## 修复机制

- 基线持久化：cleanup 删除已确认归档前，把其中事件按完整会话身份
  （workspace+device+tool+session）聚合累计进 `metrics-baseline.json`
  （identities + accounted_event_ids）；只累计未入账事件（崩溃后重试不翻倍）；
  基线写入成功后才删除文件（写入中断 → 拒绝并可重试）。
- 正常入口消费基线：session metrics（列表/单会话）、dashboard /api/state、
  friction 统计、report push 快照全部改为"有效聚合"= 实时事件（剔除已清算 id，
  重投不重复计数）+ 基线累加。不通过读取归档绕过正常入口。
- 确认与累计身份沿用 RW-06/08 四元组与 keys。

## 验证（tests/retention.rs 新增 cleanup_persists_cumulative_baseline_and_
entries_stay_consistent；lib 级真实入口链路）

- 采集 e1/e2（prompt）→ rotate 归档 → 受控确认水位线（e1/e2/e3）→ cleanup：
  归档删除、metrics prompt_count=3（= 清理前，基线 2 + 实时 e3 1）；
- 重投已清理事件 e1（同 id 不同内容）→ 仍为 3（不重复计数）；
- 新事件 e4 → 4（正确累加）；
- 基线损坏：写坏 metrics-baseline.json → cleanup 返回错误（拒绝破坏性清理）；
  修复基线后重试 → cleanup 成功；
- 工作区隔离/未确认保护由既有 cleanup_scope_and_unreported_protection 继续守护；
  report push 消费基线后 digest 侧不回退（dashboard_reporting 15 passed）。

回归：retention 9 passed；session_metrics 15 passed；dashboard_reporting
15 passed；fmt/clippy 通过。
