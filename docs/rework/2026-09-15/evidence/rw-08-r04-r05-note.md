# RW-08 · R04/R05 修复与验证（2026-09-15，ZCode）

## 修复

- R04（补传开关）：retry_action 与 push 共用 reporting_enabled 检查——关闭时
  retried=0、reporting_enabled=false、pending 原样保留、零远端写入；开关恢复
  后 retry 补传成功并清空 pending。
- R05（跨日累计）：digest 合并键从 (workspace, session_id_hash, date) 改为
  (workspace, session_id_hash)——记录语义冻结为**累计快照**（逐字段最大值），
  同身份跨日重报合并为一条不重复累计；上报日期收集为 dates 数组仅作范围展示；
  输出新增 merge_semantics 说明。旧日期文件天然兼容（键不再含日期）。

## 验证（tests/dashboard_reporting.rs 新增
report_retry_gated_by_switch_and_crossday_snapshot_merged_once）

- 真实 pending：远端不可达 → push 失败（exit 17）且批次冻结进 pending；
- 关闭 reporting：retry → retried=0 且远端 refs 逐字节不变；push → E8001 拒绝、
  远端仍零写入；pending 保留；
- 开关恢复：retry → retried=1、pending 清空（status 确认）；
- 跨日合并：报告分支注入昨日同名身份文件（prompt=1）与今日（prompt=2）→
  digest 单条记录、prompt_count=2（最大值而非相加 3）、dates 含两天；
- 幂等与水位线：重复推送 already_pushed、confirmed_event_count 更新（既有
  report_pending_retry_repushes_and_clears / report_push_idempotent_on_lost_
  response 用例更新为开启开关后补传并继续通过）。

回归：cargo test --test dashboard_reporting → 15 passed；fmt/clippy 通过。
