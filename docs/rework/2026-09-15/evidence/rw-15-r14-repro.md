# RW-15 · R14 反例复现与修复验证（2026-09-15，ZCode）

场景：两个绑定工作区 wsA/wsB（同一数据根）；向 B 的 events.jsonl 种入会话
rw15-s 的 2 次干预（宿主显式信号 dedup_key=intervention-N，schema type=tool）。

修复前（真实 CLI，--json）：
- 进程 cwd=wsA、payload.cwd=wsB 的 Stop：采集落在 B（captured=true），
  但 friction 决策回退进程 cwd 重发现 → 查询 A（无数据）→ 无 friction_notice。
- 反事实：同会话同数据以进程 cwd=wsB 运行 → 立即出现 friction_notice。
  与审查 R14/hook-repros.json 一致。

修复后（真实 CLI）：
- cwd=A、payload cwd=B → friction_notice 触发（打断 4 的 B 数据语义）、
  结果带 workspace_root=…/wsB、提示标记 rw15-s.prompted 落在 B，A 无标记。
- 显式 --root wsA 优先于 payload B → workspace_root=…/wsA、无提示（A 无干预）。
- 无 payload cwd 时回落进程 cwd（用例由集成测试覆盖解析优先级）。
- 事件 JSON 结果新增 workspace_root 字段（消费方按契约忽略未知字段）。

回归：tests/hook_events.rs 14 passed（含新增
stop_prompt_follows_payload_workspace_not_process_cwd）；
tests/auto_sync.rs 3 passed（自动同步调度不回归）。
