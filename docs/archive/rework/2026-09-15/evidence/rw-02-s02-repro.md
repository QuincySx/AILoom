# RW-02 · S02 反例复现与修复验证（2026-09-15，ZCode，基线 c4a56a4 工作区）

## 修复前（真实 CLI，二进制为 c4a56a4 构建）

场景：`ailoom source --dir team --minimal` → 添加 `resources/hooks/stop-note.toml`
（event=Stop、targets=[claude]、shared=true）→ commit → 工作区 init → 首次 sync。

- `init` 退出 0；首次 `sync` **退出 13**：
  `[E5004] JSON pointer 父节点不是对象: /Stop`，与审查 S02 一致（"已有测试提前写入
  hooks 对象，绕过失败路径"——旧测试预写了 `{"hooks":{"Stop":[...]}}`，未覆盖空 settings）。
  同步留有 journal 恢复点，`sync --recover` 可恢复。

## 修复后（重建二进制）

- `sync --recover` 退出 0；首次 `sync` 退出 0。
- 生成的 `ws/.claude/settings.json`：`hooks` 为对象、`hooks/Stop` 为数组、
  条目含托管 exec 签名命令（`ailoom hooks exec --id team/hook/common/stop-note --event Stop`）。
- 重复 `sync --json`：`{"applied":[],"noop":2,"ok":true}` —— Noop 且不改写文件。
- 回归：`cargo test --locked --offline --lib sync::apply` → 5 passed（ensure_json_array_at
  的不存在/{}/已有 hooks 对象/类型冲突/null 视为缺省/一级与根指针语义）；
  `cargo test --locked --offline --test adapters_next` → 13 passed / 1 ignored
  （ignored 为既有真实 npm 安装用例），含新增 2 项：
  - team_hook_first_sync_creates_nested_config（三种输入 + 冲突原文件逐字节不变 + Noop）
  - team_hooks_same_event_first_sync_from_empty_settings（同事件两团队 Hook 共存）
- fmt/Clippy 通过。
