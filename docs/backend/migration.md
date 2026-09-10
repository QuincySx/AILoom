# Git 模式 → 后端迁移（AIL-030，仅设计）

## 迁移原则

1. 渐进：Git 模式与后端可长期共存（同一 CLI，数据源切换由声明选择）
2. 可回退：后端提供 Git 导出（快照 = 等价 ailoom.toml + resources/ 树），随时可退回纯 Git
3. 不迁移机器数据：绑定/事件/索引是本机的，迁移只涉及团队共享数据

## 阶段

### 阶段 A：只读镜像

- 后端 ingest 已有 Git 源（只读）；CLI 不变
- 校验点：后端 revision 与 Git commit 的 content_digest 一致

### 阶段 B：双写审核

- 贡献双写：PR（Git 审核流）+ changeset（后端队列）；以 PR 合并为准
- 校验点：两路径产物 content_digest 相等

### 阶段 C：后端为主

- 贡献/发布走后端；Git 仓作为导出归档（定时导出）
- 校验点：Git 导出可被全新 init 消费（可回退验证）

## 身份迁移

- Git 模式 member_id（显式 --member）→ 后端 OIDC subject 映射表（保留历史参与记录）
- 名册（membership/roster.toml）导入后端成员表；archived 标记保留

## 身份撤销

- 设备吊销：吊销 device_id → 该设备令牌失效；已发布内容不受影响（署名保留）
- 成员离队：名册归档语义（projects 移除 + archived 标记），历史记录不删

## 边界（重申）

- 本目录全部为设计：不实现 SSO/数据库/Web 后台；不宣称参考 #341 已实现
- Git namespace 不提供 RBAC 的结论在后端下同样成立：RBAC 是后端权限模型，不是路径约定
