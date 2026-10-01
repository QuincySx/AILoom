# 项目知识库位置与迁移

## 已实现

UI：我的目录 → 项目菜单 → 知识库。可以创建或关联本地知识库、预览迁移、迁移并切换、设置 Git 同步地址/知识分支/仓库内子目录、立即同步。已有 Git 同步设置时，迁移确认会一并尝试同步。

CLI 与 UI 共用 `knowledge::location`。项目归属按现有 RepositoryId 识别，所有 Worktree 共用；普通文件夹及受管子目录按所属项目识别。多个项目显式关联同一个知识库时，迁移会更新全部关联。

位置和同步设置保存在数据根的 `knowledge/bindings.json`，目录内 `ailoom-knowledge.json` 保存知识库身份。正文是 Markdown（默认 `learnings/`、`docs/`）。可关联已有带合法 `ailoom.toml` 的团队源目录，已有文件不重写。资源库中的来源引用不随知识库迁移改写：知识保存位置和 Skill/MCP 的来源是独立设置。

迁移：锁定 → 扫描/冲突检查 → 预览指纹 → 记录迁移意图 → 复制 → 校验新旧内容 → 原子切换所有关联。旧位置保留备份；复制中断时配置仍指向旧位置，可重新预览后继续。遇到不同内容的目标文件不会覆盖。归档、反馈等本机维护状态按知识库身份保存，换位置不改变它们。

Git 同步：临时克隆，使用专用知识分支（默认 `ailoom-knowledge`）及指定子目录。基于上次同步摘要做三方合并；同一文件双方修改则报冲突，不覆盖。成功推送后才更新本地内容和基线；不切换业务分支，不暂存业务修改。同步失败保留本地内容；迁移已成功但远端失败时明确显示“已迁移，尚未同步”。不会自动推送未配置的远端。

## CLI

```sh
# 新知识库，支持普通文件夹
ailoom init --root /path/to/project --knowledge-path /path/to/knowledge/project-a

# 也可以关联独立目录并设置远端
ailoom knowledge --action init --root /path/to/project \
  --path /path/to/knowledge/project-a \
  --remote git@github.com:team/knowledge.git --subdir projects/project-a

ailoom knowledge --action save --root /path/to/project --file note.md --name first-note
ailoom recall --root /path/to/project --query 关键词

# 两段式迁移：先预览，再使用返回的 expected 指纹执行
ailoom knowledge --action move --root /path/to/project --path /new/location --json
ailoom knowledge --action move --root /path/to/project --path /new/location \
  --expected '<预览返回的指纹>' --execute

# 需要同步时，预览与执行均带 --sync-after
ailoom knowledge --action sync --root /path/to/project
```

修改远端设置使用 `knowledge --action configure`，同样先预览、后携带 `--expected ... --execute`。空的 `--remote ''` 关闭 Git 同步。CLI `contribute` 在已绑定项目知识库时保存经验到该库；分享 Skill 已说明本地保存与远端同步的区别。

## 验证与边界

- 集成测试覆盖普通文件夹、Worktree/子目录共享、共享关联一起切换、目标冲突、过期预览、符号链接拒绝、CLI 保存/召回/归档恢复、控制台授权、Git 往返同步、仓库子目录隔离、业务分支与脏文件保护、同步失败保留本地内容。
- Orca 内置浏览器实际完成测试项目“知识库甲”的初始化和迁移；从 `docs` 子目录通过 CLI 成功召回迁移后的测试知识。
- Git 网络流程通过本地 Git 远端验证；未向用户的真实云端仓库推送。
- 不含 FTP/SFTP 适配、定时后台同步，也不等同于已完成 Agent Hook 自动总结。旧团队资源源的声明/部署流程保留。
- 当前迁移扫描限制为 128 MB / 10000 文件，符号链接与非常规文件不接受；机器之间不自动复制绝对路径绑定，需要各设备关联知识库。

验证结果：知识库专项 11 项、召回 6 项、经验 8 项、控制台 14 项、内置资源 4 项、库单元测试 106 项通过；前端组件 7 项通过，更新模块语法解析通过。`cargo build` 与 `git diff --check` 通过。严格 Clippy 被既有的 `src/console/mod.rs` 文档注释空行、`src/cc_switch.rs` unnecessary_unwrap 两处告警阻挡；本轮未修改这两处无关代码。
