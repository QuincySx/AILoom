# 后端 API 契约草案（AIL-030，仅设计）

所有写接口要求 `Idempotency-Key` 头（幂等键规则见 architecture.md）。错误结构沿用 AILoom 错误契约（code/message/context/fix）。

## 身份与权限

- `GET /orgs/{org}/teams/{team}/projects` — 项目列表（RBAC 可枚举：scope→allowed actions）
- `POST /orgs/{org}/teams/{team}/members` — 登记成员（member_id、projects[]；幂等）
- `DELETE /orgs/{org}/teams/{team}/members/{id}/projects/{pid}` — 移除（归档语义）
- `GET /orgs/{org}/teams/{team}/roster` — 名册查询（对应 members list/projects）

权限模型（可枚举）：`scope ∈ {org, team, project}` × `role ∈ {admin, maintainer, contributor, reader}`；
检查函数 `can(role, action, resource) -> bool`，action 集合显式列出（publish/rollback/register/upload-stats）。

## 资源发布

- `POST /changesets` — 创建变更集（body：base revision + 精确文件清单；返回 changeset_id）
- `POST /changesets/{id}/submit` — 提交审核（状态机见 journeys.md）
- `POST /changesets/{id}/approve|reject` — 审核（审计事件）
- `POST /changesets/{id}/publish` — 发布为不可变 revision（返回 revision_id + ETag）
- `POST /revisions/{id}/rollback` — 回滚 = 新指针指向旧 revision（不删历史）

revision 语义：
- immutable；内容寻址（content_digest）
- `GET /revisions/{id}` — 条件请求 `If-None-Match: <ETag>` → 304（对应 sync-edge 增量）

## 遥测

- `POST /telemetry/batches` — 白名单计数批次（Idempotency-Key = batch_id；重复幂等 200）
- 缺失来源标注透传（availability=unavailable 不计 0）

## 审计

- `GET /audit?team=&action=` — 审计事件查询（只读）
