# RW-11 · R08 说明与修复验证（2026-09-15，ZCode）

R08 为静态数据流发现（旧 current_revision 仅比较源锁+HEAD，未提交修改/删除
不改变两者 → query graph_stale=false），无法在修复前用当前二进制"真实复现"
——旧实现确实如此（无内容维度输入）。修复后验证（真实 CLI + 集成测试
uncommitted_worktree_changes_stale_query_and_rebuild_recovers）：

- 构图后未提交修改 src/lib.rs → query 返回 graph_stale=true、
  content_stale=true、note 注明"工作树内容与构图时不一致（含未提交改动）"。
- code build 重建 → query graph_stale=false（恢复新鲜）。
- 未提交删除 src/lib.rs → query graph_stale=true。
- 指纹语义（单元断言）：同内容重写（mtime 变）指纹不变；内容变化/新增/删除
  均改变指纹；worktree_fingerprint 不要求 Git（纯目录可算）。
- 覆盖面：非 Git 工作区由指纹直接覆盖（不依赖 git rev-parse）；HEAD/源锁
  维度由既有 code_query_reports_stale_graph_after_revision_change 继续守护。

实现：graph.rs 新增 worktree_fingerprint（与 scan_project 相同过滤：跳过
target/./仅 .rs；对内容字节做 sha256 后整体摘要）；Graph 增加
content_fingerprint（serde default 兼容旧文件）；query 以 revision+指纹双
维度判定过期并输出 content_stale/worktree_fingerprint。
