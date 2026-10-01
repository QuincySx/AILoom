# 后端关键旅程（AIL-030，仅设计）

## 旅程 1：首次接入（新成员）

1. 成员运行 `ailoom init --backend <url>` → 跳转 OIDC 登录 → 设备令牌绑定（device_id 注册）
2. 拉取团队清单（projects/roles/namespaces）→ 选择 projects/roles
3. sync-edge 返回当前 revision 快照（ETag 缓存）
   断言：全程不需要 Git 凭据；设备可吊销（吊销后 401）

## 旅程 2：多项目切换

1. 成员在同一 workspace 运行 `ailoom init --project b`
2. 后端校验成员对 b 的 reader/contributor 权限
3. sync-edge 返回 b 范围 revision；索引按新身份重建
   断言：a 的经验不再召回（隔离语义与 Git 模式一致）

## 旅程 3：管理员审核发布

1. 贡献者 `ailoom contribute` → 后端 changeset（draft）
2. 维护者 review-service 查看 diff → approve → publish
3. 发布产生 immutable revision + 审计事件；通知订阅者
   失败重试：同 Idempotency-Key 重放 → 返回同一 revision（不重复发布）
   断言：禁止跨租户数据（changeset 归属 team 校验；跨租户引用 403）

## 旅程 4：离线恢复

1. 成员离线工作：事件/贡献写入本地队列（Git 模式同款 journal/checkpoint）
2. 恢复网络：上报批次重放（幂等键去重）；变更集重提（同 changeset_id 幂等）
   断言：重试不重复计数/不重复发布；本地锁定版本在离线期间可用

## 权限检查枚举（示例）

| action | admin | maintainer | contributor | reader |
|---|---|---|---|---|
| publish | ✅ | ✅ | ❌ | ❌ |
| rollback | ✅ | ✅ | ❌ | ❌ |
| register-member | ✅ | ❌ | ❌ | ❌ |
| create-changeset | ✅ | ✅ | ✅ | ❌ |
| upload-stats | ✅ | ✅ | ✅ | ❌ |
| read-revision | ✅ | ✅ | ✅ | ✅ |
