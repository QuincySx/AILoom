# RW-14 · R12/R13 修复与验证（2026-09-15，ZCode）

- R12（编号边界）：mark_stale_candidates 旧实现 starts_with(prefix) 使
  #1 前缀匹配 #10/#11 文件名。修复：匹配 `{prefix}-`（编号结束边界）+
  完整文件名精确比对当前候选。
- R13（截断身份）：候选文件名由 head8 改为完整 head SHA；同 8 位前缀不同
  完整 SHA 得到不同文件/身份（不误去重），force-push（同 PR 新 head）正确
  将旧候选标 stale 并写入新候选；dedup_key 本就含完整 SHA（机器身份完整，
  frontmatter 记录 provider/repo（source_pr url）/PR 编号/完整 head_sha），
  文件名即展示可短写的部分现也使用完整 SHA。
- 兼容：旧截断文件 `{prefix}-<head8>.md` 仅当 frontmatter head_sha 与当前
  完整 SHA 一致（同一候选旧命名）时迁移到新文件名并删除旧文件；前缀碰撞的
  不同 head 保持原样按不同候选 stale 标记——不静默覆盖。

验证（tests/import_pr.rs 新增 pr_candidate_identity_boundaries_and_full_sha，
fake gh 可变 head）：
- #1/#10/#11 并存；更新 #1 → stale_marked=1，#10/#11 状态不变；
- deadbeef1111 与 deadbeef2222（同 8 位前缀）→ 不同 draft_path、均
  deduplicated=false；同完整 head 重复 → deduplicated=true；
- force-push 后旧 head 候选内容含 candidate-stale；
- legacy 截断文件（head_sha 匹配）→ 迁移到完整 SHA 文件名、去重、旧文件删除。
回归：cargo test --test import_pr → 12 passed（含既有 PR 用例不回归）。
