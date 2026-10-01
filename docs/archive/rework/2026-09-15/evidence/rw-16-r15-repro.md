# RW-16 · R15 反例复现与修复验证（2026-09-15，ZCode）

## 修复前（真实 CLI，基线实现构建；团队 hook leak：立即退出但派生 sleep 持管）

`hooks --action exec --id team/hook/common/leak`（timeout_ms=100）：
- 结果 executed=true、timed_out=false；
- 总耗时 4.82s（等待持有管道的派生 sleep 4.732s 结束）。
与审查 R15 一致（"实际等待 2 秒并返回 executed=true/timed_out=false"）。

## 修复后（真实 CLI）

同一 fixture：
- 结果 executed=false、timed_out=true（截止覆盖管道排空：派生进程持管超过
  截止 → 按超时回收进程组，不再误报成功）；
- 连续 3 次耗时 0.17~0.18s；sleep 无残留（进程组 kill）。
- 大输出（512KB > 管道容量）正常命令 → executed=true、timed_out=false，
  0.09s（持续消费不误超时）。

## 机制

- 读线程经 mpsc 通道回传输出；总截止时间覆盖「子进程退出等待 + 管道排空」；
  子进程已退出但管道未在截止前排空 → kill 进程组（回收派生进程树）后按超时返回；
  回收后有界（200ms）排空用于诊断日志。读线程分离，不阻塞宿主。
- 平台边界：进程组回收为 Unix 实现（kill -9 -PGID）；非 Unix 平台仅回收直接
  子进程（既有 kill_process_group 的 cfg 分支），未测平台不冒充通过。

回归：adapters_next 15 passed（含新增
team_hook_exec_timeout_covers_detached_pipe_holders 与既有
team_hook_exec_output_timeout_and_failure_semantics）。
