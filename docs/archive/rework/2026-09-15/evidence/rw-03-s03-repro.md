# RW-03 · S03 反例复现与修复对比（2026-09-15，ZCode）

真实 CLI（同一工作区场景，team 源含 package version = "1.2"）：

- 修复前（c4a56a4 实现构建）：`packages --action check` 退出 0，
  `packages: [{"package":"short-pkg","state":"missing","version":"1.2"}]` ——
  不完整版本被放行并原样交给 npm（deps 写入 "1.2"，npm 解析为范围装最新 1.2.x，
  安装后 package.json version 为 1.2.N ≠ "1.2"，已满足判断永远 false，
  missing→install 反复循环）。与审查 S03 一致。
- 修复后：`packages --action check` 退出 12，
  E3002 "包 short-pkg 的 version 必须是精确版本（如 1.2.3 或 1.2.3-next.1），
  核心必须是三段数字（拒绝范围/tag/不完整版本）: `1.2`" —— 启动安装器之前拒绝。
- prerelease 接受：`1.2.3-next.1` 声明 check 退出 0 且报 missing（集成断言）；
  预置 node_modules version=1.2.3-next.1 后 `install --yes` npm_ran=false、
  假 npm 零调用（版本语义一致，无重装循环）。

回归：cargo test --lib packages → 1 passed（校验矩阵 20+ 用例）；
cargo test --test adapters_next → 14 passed / 1 ignored（真实 npm registry
用例保持既有 ignored，未重跑真实 npm，已如实记录）。
