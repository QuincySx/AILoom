# Standards 审查证据（2026-09-15）

范围：git diff 54a4cac...c4a56a4；只读审查，未修改仓库代码。下述前两项使用现有 target/debug/ailoom 真实 CLI 在完全临时 HOME/工作区下复现；二进制未另行重建，源码路径已交叉核实。临时夹具保留在 `/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/ailoom-review-blfnc5j_`。

## 1. P1 默认根迁移使已部署 Skill 链接失效（已复现）

位置：src/paths.rs:105–117；部署依据 src/sync/apply.rs:383。

规范：CONTEXT.md SkillStore/Require 定义明确实体在共享 Store、部署结果是链接；docs/CONTRACTS.md §3 默认根兼容迁移要求“数据保持可达”；docs/IMPLEMENTATION-GUIDE.md 公共实施规则要求机器根可注入。

复现设置：创建临时 HOME/.ailoom/store/team/skill/SKILL.md，外部工作区模拟链接 existing-skill 指向该绝对 skill 路径；子进程 HOME 指向临时目录，XDG_STATE_HOME、XDG_DATA_HOME、AILOOM_DATA_ROOT、AILOOM_STORE_ROOT 均为空。执行 `target/debug/ailoom status`，cwd 为不含工作区的临时目录。

实际输出断言：`migration 10 old link readable False new tree True`。status 因未找到工作区退出 10，但 AppContext::discover 在发现工作区前已解析 data_root 并完成迁移。旧链接目标消失、新 `.local/share/ailoom/store/team/skill/SKILL.md` 存在。

影响：迁移发生时所有其他已部署工作区的绝对 Skill 链接同时失效；要求逐个重新 sync 才恢复。迁移应维持旧链接可达性。

附带静态证据（未单独复现，不另算主 finding）：迁移清单仅 store/ws/cache，遗漏 `device-id`。src/config.rs:292 从新 data_root/device-id 读取或创建身份，因此升级会生成新 device_id，旧绑定与历史事件仍携带旧设备身份；设备连续性及团队设备归属会发生变化。请在迁移完整性修复中一并验证。

## 2. P1 空配置首次同步团队 Hook 失败（已复现）

位置：src/sync/apply.rs:604–606；调用方 src/adapters/hooks_team.rs:122–123。

规范：docs/CONTRACTS.md §6 冲突矩阵“目标不存在/需要 → 创建”；docs/IMPLEMENTATION-GUIDE.md 步骤5“完成最短真实链路”，REWORK.md 团队 Hook 工作线 AIL-032 要求稳定身份、多 Hook 幂等与真实链路。

复现步骤（同一隔离环境）：
1. `ailoom source --dir <tmp>/source --team-id test --minimal`，退出0。
2. 在 source/resources/hooks/stop.toml 写入：
```toml
name="stop"
event="Stop"
command=["echo","ok"]
timeout_ms=1000
shared=true
namespace="common"
targets=["claude"]
```
3. 对空 ws 执行 git init；在 ws 运行 `ailoom init --local-path ../source --project a`，退出0。
4. 未预建 `.claude/settings.json`，运行 `ailoom sync`。

实际：退出13，`E4004 同步未全部完成`；failed 内层为 `E5004`，message 为 `JSON pointer 父节点不是对象: /Stop`。先前已经部署 hook spec 和 builtin agent，产生 pending journal。

根因：ensure_json_array_at 对 /hooks/Stop 的第一层 hooks 使用 []，第二层要求对象，必然报错。只应在叶子创建数组。tests/adapters_next.rs::team_hook_registered_and_user_hooks_preserved 预先建立 hooks.Stop，故没有覆盖首次空配置。

## 3. P2 精确版本验证接受范围并拒绝合法版本（静态确定）

位置：src/packages/mod.rs:272–276。

规范：docs/CONTRACTS.md §4.2 package 的 version 必须精确；代码新增注释亦明确“版本必须为精确 semver”。

数据流：validate_exact_version 允许 numeric.len() >= 2，因此 `1.2` 通过；run 将 p.version 原样写入 package.json dependencies 并运行 npm install。`1.2` 是浮动 patch 版本要求，安装得到 `1.2.N` 后 check_npm 按字符串与 `1.2` 比较不会满足，后续仍重复安装。另一方面 body.contains 包含字符 x/X，合法 `1.2.3-next.1` 被拒绝。

本项没有执行 npm、没有安装依赖或访问外部 registry；结论为代码数据流与版本语义检查。

## 4. P2 升级后段失败未保留旧安装（静态确定）

位置：scripts/install.sh:114–116。

规范：docs/REWORK.md 安装器工作线要求“升级失败保留旧安装”；脚本顶部自述“所有失败分支保留原安装”。

执行顺序：`mv -f "$TMP" "$FINAL"` 已替换现有 binary；后续 `mv -f "$TMP_SHA" "$SHA_FILE"` 或 `ln -sfn "$FINAL" "$LINK"` 任一步失败，set -e 退出，trap 仅清除临时文件，未恢复旧 FINAL。既有 LINK 指向 FINAL 的升级场景中，即使脚本报失败，旧安装也已被替换。

触发条件：checksum 目标更新失败或链接更新失败，例如文件权限/文件系统错误；需要对最终 binary、sidecar、链接切换分别故障注入。此项未运行故障注入，明确为静态控制流证据。

## 判断性气味

没有值得独立提出的高价值气味；以上均为行为/契约问题。未把尚未复现的迁移回滚分支列为 finding。
