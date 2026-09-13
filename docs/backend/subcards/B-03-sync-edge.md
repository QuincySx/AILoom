# B-03 · sync-edge：revision 分发与条件请求（设计子卡，Open）

- 状态：Open；依赖：B-02（发布产物）；对应：api.md revision 语义、journeys 旅程 1/2

## 范围

`GET /revisions/{id}` 支持 `If-None-Match` → 304；客户端锁定快照与 ETag 缓存；
按项目/角色范围的分发（相关性隔离，非 RBAC——边界与 Git 模式一致）。

## 验收

- [ ] 内容未变 → 304；内容变化 → 200 + 新 ETag
- [ ] 已发布 revision 任何字段不可变（字节级对比）
- [ ] 成员切换项目后拉取范围随之变化（旅程 2 断言）
- 错误路径：revision 不存在 → 404（E 契约）；ETag 损坏 → 全量重取

## 退出

验收全过；ETag 方案决定已记录（开放问题见索引）。
