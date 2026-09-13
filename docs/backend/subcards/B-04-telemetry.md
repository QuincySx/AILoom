# B-04 · telemetry-ingest：幂等统计批次（设计子卡，Open）

- 状态：Open；依赖：AIL-022 批次契约（batch_id 稳定身份、pending 补传）
- 对应：architecture.md「telemetry-ingest」、journeys 旅程 4

## 范围

`POST /telemetry/batches`：白名单计数批次接收；Idempotency-Key = batch_id；
重复提交幂等 200；availability=unavailable 字段透传为缺失，不计 0（契约 §9）。

## 验收

- [ ] 同 batch 重放：计数不翻倍，返回首次结果
- [ ] 与 Git 报告分支路径的 digest 汇总可合并（与 AIL-022 digest-service 会签）
- 错误路径：schema 非法 → E7001 风格拒绝，不入库

## 开放问题

批次 schema 与 AIL-022 报告分支字段映射：领取者与 AIL-022 维护者会签（见索引）。
