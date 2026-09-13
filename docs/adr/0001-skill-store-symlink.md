# Skill 实体进用户 Store，Workspace 只挂软链

我们要把「每仓复制一份 skills」改成「用户目录按源仓库分桶存一份实体，业务 Workspace 只链过去」。

布局（store 根优先级：`AILOOM_STORE_ROOT` > `$XDG_DATA_HOME/ailoom/store` > `~/.ailoom/store`）：

```text
<store_root>/<source_key>/
  .meta/SOURCE.json
  <相对 skills 根的路径>/   # 如 inking/line-art，不含 resources/skills 前缀
```

`source_key` 为规范化仓库 identity 的可逆 base64url。Workspace 的 `.claude/skills/<name>`（及 Codex 对应路径）是指向该桶内 skill 子目录的 symlink；`sync` 负责更新 store 并对齐本工作区链接集合。Store 只存 Skill 实体；机器状态不进 store。Knowledge 仓与分形长文不在本决定范围。

**Status:** accepted
