# Spec 轴审查附件 — 2026-09-15

基线 `54a4cac`，审查目标 `c4a56a4a4a713cb4781c7a34ca93299d1fc5d9da`。只读审查；没有修改业务代码、真实外部写入或访问秘密。规范来源为 `docs/cards/AIL-XXX.md`、`docs/REWORK.md`。后者明确“必需项未完成就保持开放”“禁止仅靠…全仓测试全绿关卡”。下面涉及卡片均不能仅凭目前 Done 声称视为闭环。

## 实际 CLI / socket 验证

复现使用已有 `target/debug/ailoom`，HOME/USERPROFILE/XDG_DATA_HOME/XDG_STATE_HOME 均置于临时目录，并显式 `--data-root`；仅将当前仓库作为只读 workspace 发现根。隔离目录：`/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/ailoom-spec-oynv7klw`。没有新建仓库内测试文件。所有列出的 CLI 命令退出码 0。

### S01 · P2 · AIL-019/021/022：会话身份在聚合和消费者处仍丢维度

需求（AIL-019 本轮重新验收）：“不同 workspace/provider/device 的同名 session 不错误合并”；实施步骤：“按session+provider+device去重”。

位置：`src/events/aggregate.rs:151-157,260-268`；`src/dashboard/server.rs:168` 的 build_state 分组；`src/reporting/mod.rs:124-137`；`src/events/friction.rs` 的 build_share_record。

实现 aggregate_all 只按 tool/session 分组，不含 device；aggregate_session 更未限定 provider。dashboard 仍按 workspace/session 聚合。report 文件名取 build_share_record 的 session_id_hash（仅原始 session 字符串），在 files.insert 时不同 provider 同名会话写同一路径。

CLI 反例：经 Hook 产生 review-s 的 prompt，再复制该受控事件为另一 event_id/device_id，保留 workspace/provider/session 相同。运行 `session --action metrics`，实际只有一条 session，`prompt_count: 2`，没有设备区分。provider 在 dashboard/report 中丢失属于静态调用链核验，未单独运行远端报告。

### S02 · P2 · AIL-021：Stop 后会话永久 idle

需求（本轮必须交付 3）：“断线回收连接线程/订阅，状态按最新事件变化，不因曾经 Stop 永久 idle”。

位置：`src/dashboard/server.rs:222-229`。`session_state` 只要 `stop_count > 0` 即返回 idle，不看最新事件类型。

实际入口：依次执行生成语义一致的 `hook --tool claude --event SessionStart`、Stop、UserPromptSubmit，payload 均有 `session_id: review-s` 与正确 cwd，最后 prompt 为 next task。启动 dashboard，裸 TCP GET /api/state。

实际输出摘要：`session_id=review-s, prompt_count=1, stop_count=1, state=idle`。最新事件是 prompt，仍被旧 Stop 压为 idle。关闭服务使用该测试所拥有的进程句柄 terminate/wait。

### S03 · P2 · AIL-037：清理丢掉累计基线

需求（本轮必须交付 1）：“统一活动日志/归档读取与去重，轮转和追加使用一致锁/唯一归档名，保存可核查累计与幂等基线”；实施步骤 6：“保留最小幂等checkpoint避免重放翻倍”。

位置：`src/data.rs:158-180,194-200`；`src/events/store.rs` 的 read_all_events 仅重读现存事件文件。

步骤：正常 Hook 采集后 `data --action rotate --max-size-mb 0.000001`；在隔离 report-checkpoint.json 写入确认水位线（显式 fixture，未声称真实报告推送），覆盖所有测试事件 ID、pending 为空；`data --action cleanup`；再 `session --action metrics`。

实际输出：轮转 `rotated: true`；cleanup 的 removed 包含唯一 events-archive 文件；随后 `result.sessions: []`。清理前有 review-s，清理后全部累计历史从正常查询消失。checkpoint 只保留 ID，不能恢复计数/token/session；也未被 append_event 用于拒绝被清理历史的重投。

### S04 · P1 · AIL-036：内部目标符号链接绕过迁移边界

需求：“目标或源含越界链接…迁移失败”；“业务文件…全程不变”（AIL-036 迁移验收要求）。

位置：`src/import/self_repo.rs:111-140,246-252`。

触发：目标子树 .ailoom-team 本身是正常目录，但其中 resources 是指向工作区外隔离目录的符号链接；源有 resources/new.md，外部 new.md 尚不存在。预检 final target.is_file/symlink_metadata 均不能识别父目录链接；copy_tree 接受现有目录链接并跟随写入。

本子审静态确认；主审随后通过真实 migrate CLI 复现退出码 0 且外部文件被创建，主审记录为实际验证。未向真实用户数据目录写入。

## 静态链路明确，未新增实际 CLI 验证

### S05 · P2 · AIL-022：retry 绕过关闭上报

需求（本轮重新验收）：“上报关闭无外部写入”；原始必测：“关闭上报不落远端”。

位置：`src/reporting/mod.rs:334-355`。push 分支检查 reporting_enabled，但 retry_action 加载 pending 后直接 prepare_contribution/push_batch。

反例步骤：启用上报时离线 push 形成 pending；关闭配置并不再设置 AILOOM_REPORTING；网络恢复后 report --action retry 仍执行 git push。应在 retry 的实际外发前应用同一开关。

### S06 · P2 · AIL-022：按上传日期划分累计会话导致跨日翻倍

需求：“同 session 更新不会反复累计”；“明确累计快照和增量不可混用”。

位置：`src/reporting/mod.rs:119,129,481-482`。

反例：同一结束会话累计 prompt_count=10，第一天 report push，第二天事件未变再次 push。build_batch 采用 today，文件名和 batch_id 都改变；digest 按 (workspace, session_hash, date) 分组，形成两个 session，totals 计为20。不是按事件周期切分，也不是同一累计快照取最大。

### S07 · P2 · AIL-026：同文件嵌套模块符号仍碰撞

需求：“嵌套模块、同名类型/函数…不会静默选错目标”。

位置：`src/code_knowledge/graph.rs:92`。

反例源码 `mod a { fn helper() {} } mod b { fn helper() {} }` 位于 src/lib.rs，两者同为 `fn:helper@src/lib.rs`。ID 加文件路径修复跨文件碰撞，却没有模块限定名；召回按 ID 建表会覆盖，调用边混合。

### S08 · P2 · AIL-027：未提交源码改动不产生过期提示

需求：“构图后修改/删除源码再 query，不能无提示返回旧文件位置”。

位置：`src/code_knowledge/mod.rs:124`。

反例：构图后修改/删除 Rust 文件，保持 HEAD 不变，再 query。freshness 仅比较 HEAD 与源锁，返回旧位置但 graph_stale=false。需要覆盖实际构图输入版本，不能仅以 git HEAD 代表工作树。

### S09 · P2 · AIL-034：跨目标导入覆盖资源归属

需求：按目标定义去重/更新，跨目标成功验收；卡面本轮要求来源/目标/类型维度身份隔离。

位置：`src/import/mod.rs:337-339,481-494`。

反例：同一来源文件先导入 project A，再导入 shared。checkpoint 区分 target，但 candidate name 不含 target，最终路径只按 kind/name，相同目标文件被第二次覆盖、归属改为 shared；并非两份独立目标资源。

### S10 · P2 · AIL-035：合并构图更新错误项目和版本

需求：“合并后触发正确项目/revision 构图”。

位置：`src/import/pr.rs:203` 的 advance_graph_baseline。

反例：绑定 [A,B]，显式指定 B 的 PR 合并同步。函数未接收/使用目标 project，选首绑定项目 A；构图扫描当前 checkout，但 revision 标为 PR head，当前 checkout 未必包含该提交内容。最终 A 的图谱被不匹配源码版本更新。

### S11 · P2 · AIL-035：构图失败后被成功标记阻断恢复

需求：失败路径与幂等推进要求，合并后更新基线必须闭环。

位置：`src/import/pr.rs:175`、去重返回 `:289`。

反例：图谱文件损坏使合并后的构图失败。代码先写候选/成功去重标记，再调用构图。重试从已经存在的候选与 merged 状态判定重复直接返回，无法补建图谱。成功标记应在必要后续步骤完成后确认，或持久化可恢复阶段。

### S12 · P2 · AIL-035：PR 前缀匹配失效其他 PR

需求：“新 head 使同 PR 旧版本失效”。

位置：`src/import/pr.rs:132`。

反例：已有同仓 #10/#11 候选，再更新 #1。starts_with(prefix) 的 prefix 没有 PR 号码终止分隔边界，#1 匹配 #10/#11，导致其他 PR 草稿被错误 stale。

### S13 · P2 · AIL-035：候选身份只使用八位 head

需求：“候选身份包含…完整 head”。

位置：`src/import/pr.rs:274-289`。

反例：同一个 PR 两个不同完整 SHA 具有相同前八位；两次 merged 状态相同。文件名相同，已有文件分支只比较 merged，第二个版本被误去重，未刷新候选。

## 覆盖与限制

审查覆盖 events、dashboard、reporting、data、learning、knowledge feedback、code_knowledge、import/mod、import/pr、import/self_repo、commands/contribute。对应 015、018、019、020、021、022、026、027、028、034、035、036、037，及共同契约影响。015 的 YAML 序列化和持久身份修复、028 的反馈重试幂等未发现需另报的新缺陷。018/020 的 Hook 提示根目录问题由主审报告，避免在本附件重复计数。

本附件没有把未运行的静态反例写成已通过实际验证；没有以测试数量代替 spec 验收。全量 tests/fmt/clippy 由主审统一执行并归档。
