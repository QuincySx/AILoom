# RW-06 · R02 修复与验证（2026-09-15，ZCode）

## 冻结的完整会话身份

**workspace_id + device_id + tool + session_id 四元组**（契约 §9 各 ID 职责）。
兼容策略：事件数据本身已携带全部四元组（无数据迁移）；SessionMetrics 新增
device_id 字段（serde default，旧消费方按契约忽略未知字段）；aggregate_all
键从 `<tool>/<sid>` 变为 `<tool>/<sid>@<device>`（旧键无设备维度，属身份缺陷
修复；上报记录身份派生自四元组，跨版本的一次性格式迁移在 digest 合并视图
中表现为并存行、不互相覆盖）。

## 消费者核查（不以改分组字符串结案）

- src/events/aggregate.rs：aggregate_all 四元组分组；aggregate_session_scoped
  增加 device 过滤；新增 session_identities（同名 session 的完整身份列表）。
- src/dashboard/server.rs build_state：删除看板层自行按 (workspace,session)
  分组，改按 workspace 分组后统一复用 aggregate_all（与 CLI 同一语义）。
- src/reporting/mod.rs：build_batch 的文件名/记录身份经 build_share_record 的
  session_id_hash = sha256(workspace|device|tool|session)[16]——不同设备/宿主
  不写同一文件、不被 digest 合并；同一完整身份同日重投幂等（同文件同内容）。
- src/commands/session.rs：单 session 查询在多完整身份时输出
  identity_ambiguous + identities（可解释，不静默合并）。
- src/events/friction.rs build_share_record：身份派生更新（见上）。

## 验证

- 单元（session_metrics 15 passed，含新增 full_identity_prevents_cross_device_
  and_cross_tool_merging）：三身份（跨设备/跨宿主）→ 三条独立会话、各 1 prompt；
  identities 列出 3 个；不同身份 session_id_hash 互异。
- 集成（dashboard_reporting 12 passed，含新增 same_session_across_devices_
  distinct_in_metrics_and_dashboard）：真实 CLI 注入同 session 不同设备事件 →
  metrics 列表 2 条独立（device 不同）；单 session 查询 identity_ambiguous=true
  且 identities=2；/api/state 同样 2 条（看板与 CLI 同一聚合语义，既有
  dashboard_state_matches_cli_metrics 继续守护一致性）。
- 上报文件隔离由身份哈希直接保证（单元断言），跨日累计语义由 RW-08 处理。
