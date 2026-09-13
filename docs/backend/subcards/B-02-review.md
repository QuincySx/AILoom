# B-02 · review-service：变更集状态机/发布/回滚（设计子卡，Open）

- 状态：Open；依赖：B-01；被依赖：B-03、B-06
- 对应设计：api.md「资源发布」、journeys.md 旅程 3、architecture.md 原则 2/4

## 范围

changeset 状态机 draft → submitted → approved/rejected → published（终态不可变）；
回滚 = 新指针指向旧 revision（不删历史）；发布产生 immutable revision + ETag + 审计事件。

## 输入 / 输出

- 输入：changeset（base revision + 精确文件清单，对应 AIL-014 白名单语义）
- 输出：revision_id/ETag、审计事件（who/when/what/diff-ref）

## 验收

- [ ] 全状态机转换有测试；非法转换（如 draft→published）显式拒绝
- [ ] 同 Idempotency-Key 重放 publish → 返回同一 revision（不重复发布）
- [ ] 回滚后 `GET /revisions/{old}` 仍可用且内容字节相等（不可变证明）
- [ ] 跨租户 changeset 引用 → 403
- 错误路径：审批人=提交人 → 拒绝（职责分离，与 Git 模式 PR 审核一致）

## 入口 / 退出

- 入口：B-01 提供 token/权限
- 退出：验收全过；diff 渲染格式决定已记录（开放问题见索引）
