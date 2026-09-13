# B-06 · 审计事件与查询（设计子卡，Open）

- 状态：Open；依赖：B-02；对应：architecture.md 原则 4、api.md「审计」

## 范围

publish/rollback/权限变更全部落审计事件（who/when/what/diff-ref）；
`GET /audit?team=&action=` 只读查询。

## 验收

- [ ] 三个触发点（发布/回滚/权限变更）各产生一条可查询审计
- [ ] 审计不可通过业务接口篡改（只读）
- 错误路径：查询条件非法 → E 契约 400

## 开放问题

留存期限与索引方案：领取者决定并记录（见索引）。
