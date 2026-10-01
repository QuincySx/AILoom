# B-07 · Git 导出与迁移阶段 A/B/C（设计子卡，Open）

- 状态：Open；依赖：B-01、B-02、B-03；对应：migration.md 全部阶段

## 范围

阶段 A 只读镜像（ingest Git 源，digest 校验）；阶段 B 双写审核（以 PR 合并为准，
两路径产物 digest 相等）；阶段 C 后端为主 + Git 导出归档（可回退：全新 init 可消费导出）。
成员身份映射：member_id → OIDC subject 映射表；roster.toml 导入保留 archived。

## 验收

- [ ] A 退出：后端 revision 与 Git commit content_digest 一致
- [ ] B 退出：两路径产物 content_digest 相等
- [ ] C 退出：导出树通过全新 `ailoom init` + sync 全链路（可回退验证）
- 错误路径：导出中断 → 原子替换（临时目录 + rename），不留半成品
