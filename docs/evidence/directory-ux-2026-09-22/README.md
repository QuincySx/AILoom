# 目录优先 Web 整理 · 验收证据（AIL-121～128）· 2026-09-22

沙盒：`/tmp/ailoom-dirux`（console :8648；重置：`zsh /tmp/ailoom-dirux/reset.sh`，夹具重建：`scripts/setup.sh`）。
夹具：两个非 Git 知识库（kb-jia 含 web/docs/notes 嵌套、kb-yi 对照）、Git 项目丙双工作树（main + feature，含 web/docs）、
合集来源仓库（meeting-notes/doc-search）、本地个人副本（notes-helper）、未托管 Skill（proj-bing/web/.claude/skills/old-notes）。
全程仅在沙盒内操作，未触碰用户真实知识库；脚本可重复执行（每张卡执行前重置沙盒、自建场景基线）。

## 门禁结果（run-all.sh 一次连续通过）

| 脚本 | 覆盖 | 结果 |
|---|---|---|
| ail121-verify | AIL-121/122：中间层继承修复、目录 Dialog、取消零写入、慢响应逆序竞态 | 17/17 PASS（ail121-verify.log） |
| ail123-verify | AIL-123：在此停用/恢复继承/移除语义 + 添加/预览/应用 + 磁盘与跨目录隔离断言 | 22/22 PASS |
| ail124-verify | AIL-124：共享设置独立路由、Markdown 项目级、草稿保护、往返/后退恢复目录与页签 | 14/14 PASS |
| ail125-verify | AIL-125：noop 禁应用、旧计划 stale 零写入、切目标作废、撤销冲突保留 | 13/13 PASS |
| ail126-verify | AIL-126：来源组+本地资源组、检查更新不改锁版本、按资源引用 Dialog、200 条密度 | 13/13 PASS |
| ail127-verify | AIL-127：10 条路由清扫、刷新不重放写入、高级配置上下文往返、失联恢复、390px | 21/21 PASS |
| ail128-nongit | AIL-128：非 Git 根/子目录旅程、未托管删除双确认（含防爆破令牌作废）、重启持久化 | 18/18 PASS |
| ail128-focus | AIL-128：Dialog 焦点进入/Esc/焦点还原 | 3/3 PASS |

Rust 侧：`cargo test` 全绿（101 lib + 集成；含本轮新增
`profile::tests::middle_layer_inheritance_visible_in_subdirectory`、
`scan_skills::tests::sub_scan_discovers_host_skills_and_respects_boundary` 两个契约回归）。

## 旅程对照（AIL-128 线稿矩阵）

| 旅程 | UI | API | 文件 | 证据 |
|---|---|---|---|---|
| 无 Git 知识库根目录 | ✅ | ✅ effective/deploy-status root 作用域 | ✅ kb-jia/.claude/skills/meeting-notes | ail128-j1-nongit-root.png |
| 知识库子目录增删 | ✅ | ✅ scope=web | ✅ web/.claude/skills 增→清理；docs 哈希不变 | ail128-j2-subdir.png |
| Git 两工作树 + web 继承 | ✅ | ✅ origin=worktree_override | ✅ web 部署、main 根/另一工作树零变化 | ail121-web-view-fixed.png、ail123-*.png |
| 项目共享 Markdown | ✅ | ✅ repo_default 层 | ✅ 仅数据区（待应用） | ail124-*.png |
| 全局来源 → 引用 Dialog | ✅ | ✅ profile revision | ✅ 取消零写入 | ail126-*.png |
| 慢请求/冲突/失联/取消 | ✅ | ✅ stale-plan 拒绝 | ✅ 过期计划零写入 | ail121/125/127 截图 |
| 重启持久化 | ✅ | ✅ 根+子目录解析一致 | ✅ 配置保留 | ail128-j8-restart.png |

## 关键缺陷与修复记录

1. **AIL-121 核心**：旧 UI `diffOf` 只比较 repo_default 与当前层，web 目录在「项目默认停用 + 工作树启用」时误报
   「已停用/跟随项目默认」（修复前证据 ail121-before-web-view.png）。修复：全部继承/差异计算改用服务端 trace
   （`state/target.js` 的 `diffOf/upstreamOf/atLayer`），恢复继承预测 = 排除本层后最高非 inherit 表态。
2. **扫描契约缺口**：子目录扫描时 `.claude` 被点前缀剪枝且深度 3 不足，`web/.claude/skills/<n>/SKILL.md` 无法发现。
   修复 `scan_skills.rs`（宿主目录例外 + 深度 6）+ 回归测试。
3. **部署清单作用域**：`/api/deploy-status` 只按工作树根计算，目录内部署漏报「未部署」。补 `scope` 参数
   （personal.rs / console / api.js 同步）。
4. **计划目标重复拼接**：PlanPreview 把 resolvedPath（含子目录）当 root 又传 scope，子目录计划 E1002。
   修复为 `target.rootPath ?? target.path`。
5. **stale 结果不可见**：计划过期时 error/result 未落盘，前端只见空「应用失败」。修复 jobs.rs 持久化 + JobPanel
   识别 stale 显示「计划已过期…请重新生成预览」。
6. **旧计划正确性**：预览后任何配置变化（profile 字节）→ 服务端指纹校验拒绝（本轮以 UI 主路径验证零写入）。

## 已知限制与未验证项（如实记录）

- 旧门禁脚本（docs/evidence/ui-usable-2026-09-20/ 的 ail110~120 系列）针对已退役的「配置层下拉」UI 与已清理的
  /tmp/ailoom-usable 夹具，未按原样重跑。其中安全关键路径（未托管删除双确认、非 Git 闭环、重启持久化、文件隔离）
  已由本轮 ail128-nongit 等价覆盖；其余断言（四档作用域下拉、工作树视角旧文案等）随 AIL-122/123 的 UI 语义变更失效，
  由本轮脚本取代。
- MCP 凭据安全（AIL-116）与宿主指令规则（AIL-117）不在本轮范围：目录页布局完成 ≠ 这两项能力完成。
- 宿主内真实加载/调用（Claude Code / Codex 实际读取部署文件）未验证，属 AIL-077 范畴；本轮验证到「文件按预期落盘 +
  deploy-status 一致」为止。
- SSH 来源真实克隆、CC Switch 真实迁移仍沿用上轮结论（transport 别名合组为合成数据验证）。
- 非 Git 子目录的团队层提示（effective.notes）沿用后端既有文案，未改动其语义。

## 复跑方式

```sh
# 前置：本机 Chrome 调试端口 :9231；cargo build 完成
zsh docs/evidence/directory-ux-2026-09-22/scripts/run-all.sh   # 需要 /tmp/ailoom-dirux 沙盒脚本
# 或逐卡：见 scripts/ail121~ail128-*.mjs（依赖 cdp.mjs，sandbox 常量见文件头注释）
```
