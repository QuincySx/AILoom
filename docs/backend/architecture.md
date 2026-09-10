# AILoom 管理后端架构设计（AIL-030，仅设计）

> 状态：**设计文档**。本目录不实现服务、不建库、不做 SSO。参考 Tencent/teamai-cli#341 为提案证据，不当作已实现。

## 目标

为 AILoom Git 模式提供**可选**的托管后端：集中身份、审核发布、统计汇总；Git 模式继续可用（后端是增强而非替代）。

## 架构原则

1. **两条数据路径分离**：资源发布路径（变更集→审核→发布 revision）与遥测路径（会话计数上报）独立认证、独立存储、独立限流。
2. **revision 不可变**：已发布 revision 不可改写；回滚 = 发布指向旧 revision 的新指针（指针可变，内容不可变）。
3. **幂等优先**：所有写接口接受 Idempotency-Key；同键重放返回首次结果。
4. **审计**：审核发布/回滚/权限变更全部留审计事件（who/when/what/diff-ref）。

## 组件

| 组件 | 职责 | 替代的 Git 模式动作 |
|---|---|---|
| identity-service | 组织/团队/项目/成员/设备注册与认证（OIDC 对接） | 成员名册（AIL-024）|
| review-service | 变更集队列、差异渲染、批准/拒绝、发布 revision、回滚 | 贡献 PR 审核路径（AIL-014）|
| sync-edge | revision 分发：ETag/条件请求，客户端拉取锁定快照 | Git clone/fetch（AIL-004）|
| telemetry-ingest | 白名单统计批次接收（幂等键 + 去重）| Git 报告分支（AIL-022）|
| digest-service | 周期团队汇总生成 | digest 命令 |

## 与 Git 模式的能力边界

| 能力 | Git 模式 | 后端新增 |
|---|---|---|
| 内容级访问控制（RBAC） | ❌（namespace 只是相关性隔离） | ✅ 权限模型（见 api.md） |
| 集中审核与回滚 | PR 审核分支 | 状态机（见 journeys.md） |
| 团队实时遥测 | 报告分支（非实时） | 准实时 ingest |
| 设备管理/吊销 | ❌ | ✅ 设备绑定与撤销 |
