# RW-05 · R01 反例复现（2026-09-15，ZCode，基线 c4a56a4 工作区，修复前二进制）

场景：业务仓 ws（绑定独立源 src）；`ws/.ailoom-team` 为真实目录，
`.ailoom-team/resources → /tmp/rw05-repro/outside`（工作区外）；src 含
`resources/new.md`（外部原不存在）与 `resources/skills/...`。

真实 CLI `ailoom --json migrate --from <src>`：

- 退出 0，`"mode":"migrate"`、copied_files=3。
- 越界后果：工作区外 `/tmp/rw05-repro/outside/new.md`（内容 "fresh content"）与
  `outside/skills/` 被创建；声明切换为 `type = "self"`。与审查 R01 一致。

原因：`validate_subtree` 只校验子树路径参数自身的组件（.ailoom-team），不覆盖
物化目标内部更深的祖先；预检只检查最终叶子（`is_file()`/`symlink_metadata()`
都会解析中间链接，文件不存在时两者皆 false）；物化 `create_dir_all`/`copy`
沿中间链接写出工作区。

（附）子树路径参数本身含链接（`.ailoom-team → 外部`）时现有代码已拒绝
（E8002 符号链接指向工作区外），不受本卡影响。
