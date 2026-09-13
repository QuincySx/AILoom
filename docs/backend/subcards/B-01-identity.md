# B-01 · identity-service：组织/成员/设备/RBAC（设计子卡，Open）

- 状态：Open（未领取）
- 依赖：—；被依赖：B-02、B-05、B-08
- 责任模块（建议）：`server/identity/`（未来 crate），对外仅 HTTP 契约
- 对应设计：architecture.md「identity-service」、api.md「身份与权限」、journeys.md 旅程 1/2

## 范围

组织/团队/项目/成员/设备的注册与查询；OIDC 登录对接；`can(role, action, resource)`
权限检查（api.md 权限枚举表）；成员名册（roster）的服务端等价物（对应 AIL-024 语义：
归档而非删除、历史保留）。

## 输入 / 输出

- 输入：OIDC token、成员登记请求（member_id、projects[]）、权限查询
- 输出：member/device/roster 资源；`can()` 布尔判定；401/403 错误（AILoom 错误契约）

## 验收（实施时逐条执行）

- [ ] 同 Idempotency-Key 重复登记成员 → 返回首次结果，不产生重复行
- [ ] 未认证 → 401（E 契约封装）；越权 → 403 + 所需 action 名
- [ ] 移除成员项目 = 归档语义：roster 查询不再列出，历史审计可查
- [ ] 跨租户（org/team 不匹配）访问一律 403（journeys 旅程 3 断言）
- 错误路径：OIDC 提供方不可用 → 登录失败显式报错，不降级匿名

## 入口 / 退出

- 入口：存储选型由维护者决定（见索引开放问题表）
- 退出：本卡验收全过 + B-05 联调用例通过

## 开放问题

- OIDC provider 选型与部署形态：由本卡领取者决定并记录（见索引）
