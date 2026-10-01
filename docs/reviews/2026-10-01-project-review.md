# 2026-10-01 项目审查报告

- 范围：HEAD 630bc4e 之上的未提交工作区（78 个已跟踪文件，约 9k 行新增代码），以及 CLI / Web 控制台 / 测试 / 文档全量普查。
- 方法：两轴代码审查（Standards：仓库规范 + 代码坏味道；Spec：对照 AIL-099～128 卡片），再加一次只读普查；普查在隔离 HOME 中真实运行 CLI、控制台 API 与浏览器测试。
- 后续：问题已拆入 [2026-10 迭代](../initiatives/iteration-2026-10.md) 的 AIL-129～147；首批修复见 [AIL-129](../cards/AIL-129.md)。
- 行号为审查当时的快照，之后的修改可能使其漂移。

## 处理状态

更新于 2026-10-01，审查后的第二轮修复结束时。

| 编号 | 状态 | 去向 |
|---|---|---|
| AIL-112 删除越权、指纹、链接恢复 | 已修复；删除入口已迁到当前界面并通过浏览器验收 | AIL-129、AIL-138 |
| C-01、C-02、C-03、C-04、C-05、C-07、U-01、U-07、T-01、T-03 | 已修复 | AIL-129 |
| 错误码码段、schema_version、迁移记录吞错 | 已修复 | AIL-129 |
| T-02 MSRV、U-02 浏览器测试、D-01 统计 | 已修复（MSRV 1.85 进入 CI；浏览器验收六个模式全部通过；看板由脚本生成）；还差一次 GitHub CI 实跑 | AIL-130 |
| Done 卡验收项未勾选；AIL-104/115/119/125 缺口 | 已核对：84 张卡勾选 223 项；27 张改状态（Blocked 1、Superseded 12、In progress 14）；54 张保留 Done 待补证据 | AIL-131 |
| U-08、U-10（token）、T-04 | 已修复 | AIL-132 |
| T-06 | 已修复（68/68 路由被测试引用，有守卫测试） | AIL-133 |
| C-06、C-08、C-09、C-10、C-17 | 已修复 | AIL-134 |
| C-13、C-14、C-15、C-16、C-19 | 已修复；personal / collection 已有人类可读输出，report 等 5 个命令待做 | AIL-135 |
| C-11、C-12、C-20、E5004 滥用 | 已修复 | AIL-136 |
| C-21 | 已修复（只保留 ailoom web） | AIL-137 |
| U-03 | 已修复 | AIL-138 |
| U-04 孤儿路由 | 已删除 | AIL-138 |
| U-06、U-09（窄屏） | 部分（控件高度、原生 prompt、窄屏导航已修；硬编码像素与 data-variant 待做） | AIL-139 |
| U-05、U-10、C-18 | 已修复 | AIL-140 |
| U-09（术语） | 已统一为「资源库」「Worktree」 | AIL-140 |
| Spec 轴范围蔓延（原生文件、服务、知识库迁移、附加宿主、ORCA） | 知识库迁移已追认（AIL-149）；其余待补卡 | AIL-141 |
| D-08、D-09、D-10；宿主知识分散 | D-10 已在 SUPPORT 中按功能区分写清；其余需按官方文档核实 | AIL-142 |
| console/mod.rs 过大、重复的目录校验 | 已修复（约 2700 行 → 约 740 行，按领域拆为 9 个模块） | AIL-143 |
| 字符串充当领域类型、SelectKey、profile unwrap | 待处理 | AIL-144 |
| D-05～D-14 | 大部分已修复（QUICKSTART、SUPPORT、INSTALL、WEB-SERVICE 已按现状重写，并在干净 HOME 中走查） | AIL-145 |
| D-15、D-16 断链；根目录 ORCA 评审 | 已处理 | 文档整理 |
| D-17、D-18 | 部分（文档地图、归档） | AIL-146 |
| D-12 | 待处理 | AIL-147 |
| 新发现：并发 sync 把进行中的 journal 误判为遗留 | 已修复 | AIL-148 |
| 新发现：AIL-112 删除入口在当前界面不可达 | 已修复 | AIL-138 |

## 两轴代码审查摘要

### Standards

硬性违规（均已修复）：
- 知识库与网页服务错误挪用 E5004 / E4001 → 新增 E6003、E9101。
- 新增机器文件缺 `schema_version` → 已补。
- 契约改动未登记 → CONTRACTS v1.4。
- 迁移记录写入失败被吞 → 改为如实告警。

判断类坏味道（转入 AIL-142～144）：
- 字符串充当领域类型。
- 宿主知识分散（改一处要动很多文件）。
- 端口字面量与目录校验重复。
- `SelectKey::InheritResources` 不属于同一类 key。
- `console/mod.rs` 单文件职责过多。

### Spec

- 18 张卡标为 Done，但必需验收项未勾选（普查扩展为 85 张）→ AIL-131。
- 缺失或部分完成：AIL-104、119、125、115 → AIL-131。
- 范围蔓延：
  - 原生文件、网页服务、知识库迁移、附加宿主、ORCA 都没有卡片。
  - 知识库迁移与 AIL-110 冲突。
  - Skill 扫描范围扩大。
  - 以上均转入 AIL-141。
- AIL-112 实现错误：
  - 授权根可由客户端指定。
  - 删除不核对扫描结果。
  - 指纹只算路径和长度。
  - 符号链接不复核。
  - 链接删除后无法恢复。
  - 以上均已修复（AIL-129）。

## 全量普查

## 0. 总览

| 类别 | P0 | P1 | P2 | P3 |
|---|---|---|---|---|
| CLI | 1 | 6 | 7 | 7 |
| UI | 0 | 2 | 7 | 1（合并 6 小项） |
| 测试/质量 | 0 | 2 | 3 | 0 |
| 文档 | 0 | 7 | 9 | 3 |

最需要先处理的 5 件事：
1. **C-01（P0）**：`ailoom sync` 和 `ailoom personal --action sync` 互相撤销对方的部署。
2. **C-02（P1）**：`personal select` 不校验资源 ID 和宿主。一个错字就会让之后所有 personal 写操作失败。
3. **C-03（P1）**：`import` 对本地目录源（`--local-path`）一律失败。
4. **T-01/T-02（P1）**：clippy 和 fmt 都不通过，CI 必红；声明的 MSRV 1.75 实际已被违反。
5. **D-01/D-02（P1）**：README 和 BACKLOG 的卡片状态严重过时：写的是 98 张卡、58 张 Backlog，实际 128 张、122 张 Done。

---

## 1. CLI

### C-01 ［CLI］P0：团队 sync 和个人 sync 互相撤销对方的部署（来回翻转）
- 复现（工作区已执行 init，个人库有 skill `hello`，并写了个人指令）：
  ```
  ailoom personal --action select --resource personal/skill/personal/hello --host claude --state enable
  ailoom personal --action instructions --file pi.md
  ailoom --json personal --action sync
  #   applied 中删除了内置 .claude/agents/ailoom-recall.md、.claude/skills/ailoom-share-learning/SKILL.md、.codex/config.toml、AGENTS.md(recall-hint)
  ailoom plan
  #   delete .agents/skills/hello / .claude/skills/hello / .claude/rules/ailoom-personal.md / AGENTS.override.md
  #   create ailoom-recall.md / share-learning / .codex/config.toml / AGENTS.md
  ailoom --json sync   # 8 项全部翻转回去
  ailoom --json personal --action plan   # 又要翻回来
  ```
- 影响：
  - 个人层的 skill 和指令会被下一次团队 `sync` 清掉，包括 hook 触发的 auto-sync 和 `sync --refresh`。
  - 内置的召回 Agent 和分享 Skill 会被 `personal sync` 清掉。
  - 两条主路径互不兼容，用户无论怎么操作都处于"未同步"状态。
- 建议：
  - 统一成一个 desired set 计算入口：团队 sync 叠加 profile 层，personal 层继承内置资源（`--no-builtin` 除外）。
  - 补一条回归测试：`sync → personal sync → plan` 应当全部为 noop。

### C-02 ［CLI］P1：`personal select` 不校验资源 ID 和宿主，非法值会污染 profile
- 复现：
  ```
  ailoom personal --action select --resource bad --host claude --state enable
  # exit=14: [E5004] 本机配置已保存，但知识库恢复副本未更新：[E5004] 无法打包未登记的资源来源：bad
  ```
  - 之后任何写操作都会失败，错误同上，包括 `select`（换成合法资源也一样）、`instructions --file`、`personal sync`。其中 `personal sync` 返回 exit=1 `[E9000] [E5004] …`。
  - `--json personal --action plan` 的 `effective_enabled` 里出现 `"bad"`。`profile.toml` 里写入了 `bad = "enable"`。
  - 只有先执行 `--resource bad --state inherit` 才能恢复。
  - `--host nohost` 同样会被接受并保存（exit=0）。
  - 复现前提：项目知识库已 `knowledge --action init`，因为写 profile 时会触发 checkpoint。
- 问题：
  - 输入没有校验。
  - 报错时实际已经落盘（"已保存，但…"），但退出码不为 0，脚本无法判断状态。
  - 错误码被重复包装成 E9000。
- 建议：
  - select 时校验 `--resource` 必须是 effective 候选集里的完整 ResourceId，`--host` 必须在宿主注册表中，否则返回 E3004 / exit 12 且不写盘。
  - checkpoint 失败应作为警告放进 `warnings`，不应让已成功的写操作返回失败。
  - 外层不要再把 E5004 包成 E9000。

### C-03 ［CLI/代码］P1：`import` 对本地目录源一律失败（源身份被当成 Git 源）
- 复现：
  ```
  cd proj2 && ailoom init --local-path ../audit-home/team-src && ailoom sync   # 正常
  ailoom import --dir ../docsrc --target shared
  # exit=11 [E2006] 锁文件中的源身份与声明不一致
  # context: declared "git+local+git+/…/team-src" vs locked "local+git+/…/team-src"
  ```
- 根因：`src/import/mod.rs:283-287` 无论 `declaration.source.kind` 是什么，都执行 `GitSource::new(entry.identity.trim_start_matches("git+"), …)`。对 `local`/`self` 源会拼出 `git+local+…` 这个错误身份。`src/commands/recall.rs:56-70` 已经按 kind 分支处理，可以参照。
- 同样的写法还出现在 `src/commands/contribute.rs:121`（旧 PR 贡献路径，未验证该路径能否触达）。
- 建议：抽一个按 kind 解析快照的公共函数（`sync_core.rs:78/102` 已有类似逻辑），让 import/contribute/recall 共用。并补一条 `local-path + import` 的集成测试。

### C-04 ［CLI］P1：贡献/PR 类错误 E81xx 的退出码是 17，契约要求 18
- 复现：`ailoom pr --url https://github.com/a/b/pull/1`（gh 未登录）→ `[E8102] gh api 失败…`，**exit=17**。
- 证据：`src/error.rs:64` 写的是 `"8" => 17, // 上报/导入/贡献`，按第二位数字分段，没有区分 E8000 和 E8100。`docs/CONTRACTS.md:264-265` 规定 E8000-E8999 对应 17、E8100-E8199 对应 18。
- 建议：`exit_code_for` 先判断 `E81` 前缀返回 18，或者修改契约。补一条单测。

### C-05 ［CLI/契约］P1：`--json` 错误输出不带 `schema_version`，且契约前后矛盾
- 复现：`ailoom --json status`（在非仓库目录）→ stderr 输出 `{"code":"E1001","context":{…},"fix":…,"message":…}`，exit=10。其他所有 `--json` 失败路径也一样，比如 `--json init`、`--json members --action projects`、`--json personal --action undo --id nope`。
- 契约：
  - `docs/CONTRACTS.md:267` 规定失败输出 `{"schema_version":1,"error":{code,message,context,fix}}`。
  - 同一文件 `:250` 又写成扁平的 `{"code":…}`。
  - 实现（`src/error.rs:104-111` 的 `to_json`）是扁平格式，不带版本号。
- 说明：已知问题只覆盖 knowledge/service 两处，这里是**全局**的 CLI 错误 envelope。
- 建议：
  - 先定稿 CONTRACTS §5，二选一并删掉矛盾的那一行。推荐 `:267` 的带版本格式。
  - 然后在 `output::emit_error` 统一加上，并更新 tests/cli.rs 的断言。

### C-06 ［CLI］P2：`--json` 模式下 clap 解析错误不是 JSON，也没有错误码
- 复现：
  - `ailoom --json bogus`，或 `ailoom --json library`（缺 `--action`）。stderr 是 clap 的纯文本 `error: unrecognized subcommand…`，exit=2，没有 `E0001`。
  - 裸跑 `ailoom` 会把 help 打到 **stdout** 并 exit=2。
- 建议：用 `Cli::try_parse()` 捕获 `clap::Error`。`--json` 模式下按契约把它映射为 `E0001` 的 JSON。help 和 version 类不在此列。

### C-07 ［CLI］P1：多个命令在人类模式下完全不输出
- 复现（exit=0，stdout 和 stderr 都是空的）：
  - `ailoom library --action list|init|import|sources|check-update|update`
  - `ailoom personal --action effective|select|plan|migrate-nongit`
  - `ailoom data --action rotate|cleanup --dry-run`
- 证据：
  - `src/commands/library.rs:23` 有一行 `let _ = json;`。
  - `src/main.rs` 中 Library、Personal（约 699 行）、Data（约 401 行）三个分支都只写了 `if cli.json { output::emit_json(..) }`，没有 else。
- 影响：用户无法知道 `import` 是否预览成功；`data cleanup --dry-run` 的"预览"什么也看不到；`personal plan` 也看不到计划内容（json 里其实有现成的 `summary` 字段）。
- 建议：
  - 至少在 else 分支打印 `value["summary"]` / `note`，或者一个简表。
  - 补一条测试，断言这些命令的人类模式 stdout 非空。

### C-08 ［CLI/契约］P2：人类模式的输出风格不统一，有的结果只写到 stderr
- 现象：
  - `report`、`packages`、`code`、`knowledge`、`collection`、`members` 在人类模式下直接打印 pretty JSON。
  - `init`、`sync`、`source`、`contribute`、`hooks`、`uninstall --execute` 的结果只以 `[ailoom:info] …` 日志的形式写到 **stderr**，stdout 为空（例如 `ailoom sync` 的输出是 `stderr: [ailoom:info] 同步完成：写入 8 项…`）。
- 契约：`docs/CONTRACTS.md:20` 规定"日志一律走 stderr；人类文本走 stdout"。现在结果文本被当成日志处理，管道和 `| less` 都拿不到。
- 建议：结果摘要用 `println!` 输出到 stdout，`[ailoom:info]` 只用于过程日志。为 JSON 结果的命令补一个最小的人类格式。

### C-09 ［CLI］P2：status/doctor 的人类输出直接打印了 Rust Debug 格式
- 复现：
  - `ailoom status` 输出 `源: team (git, ref main)  项目 Array [String("a")]  角色 Array [String("dev")]`。
  - `ailoom doctor` 输出 `OK "git"`、`!! "declaration"`，并且 `详情:` 后面是原始 JSON。
- 证据：
  - `src/commands/status.rs:228` 对 `serde_json::Value` 用了 `{:?}`。
  - `src/commands/doctor.rs:312-313` 用 `{}` 打印 `Value::String`，字符串带了引号。
- 建议：先 `as_array()` 再 join 成 `a, dev`；`c["check"].as_str()`。

### C-10 ［CLI］P2：doctor 检查失败时退出码仍为 0
- 复现：在未 init 的 Git 仓库里执行 `ailoom --json doctor`，得到 `result.ok=false`（declaration/binding/source-lock 三项失败），但 **exit=0**。
- 影响：CI 和脚本无法用 `ailoom doctor` 做门禁。同一场景下 `plan`/`sync` 会返回 exit=10。
- 建议：有 `!!` 项时返回第一个失败项对应的类退出码，或者固定返回 10/13。可加 `--no-fail` 保留旧行为。

### C-11 ［CLI］P2：IO 错误不带路径，并且落到 E9000（exit 1）
- 复现：
  - `ailoom init --local-path ../nonexist` → `[E9000] IO 错误: No such file or directory (os error 2)`，exit=1。
  - `ailoom contribute --file ../missing.md --project a` → 同上。
  - `ailoom --json personal --action undo --id nope` → `{"code":"E9000","message":"任务不存在"}`，exit=1。
- 建议：
  - 这些都是可预期的用户输入错误，应当先 `exists()` 检查，再分别给出 `E1002`（工作区）、`E0001`（用法）、`E8002` 这类带 `context.path` 的错误。
  - `From<io::Error>` 的默认转换应带上路径，可以用 `map_err` 包一层。

### C-12 ［CLI］P2：同一目录重复导入个人库会报"冲突"
- 复现：
  ```
  ailoom library --action import --dir ../myskills/hello --execute
  ailoom --json library --action import --dir ../myskills/hello --execute
  # exit=11 E2006 导入冲突：…与库内差异文件: ["SKILL.md"]
  ```
- 根因：导入时会往库内 SKILL.md 的 frontmatter 注入 `namespace: personal`、`shared: true`（`diff` 能看到多出这 2 行），而比较时拿的是改写后的文件和源文件。
- 建议：比较时剔除注入的字段，或者改用 `.ailoom-import.json` 里的 `imported_digest` 比较。内容相同就返回 noop。另外"导入冲突"用的是源/Git 类码 E2006，码段不合适。

### C-13 ［CLI］P2：`contribute` 在知识库模式下不校验参数，多个参数被静默忽略
- 复现（项目知识库已初始化）：`ailoom contribute --file learn.md --project zzz --provider bogus` → exit=0，输出"经验已保存到项目知识库"。
- 问题：`zzz` 不是清单里的项目，契约要求返回 E3004；`--provider`、`--shared`、`--namespace`、`--message` 在新路径下没有作用，但 help 仍按 PR 贡献流程描述它们。
- 建议：校验 `--project`；对已经无效的参数给出警告，或者改 help 说明。

### C-14 ［CLI］P1：子命令 about 与实际动作不一致，help 普遍偏旧
- `library` 的 about 列了 7 个动作，`--action` 实际支持 9 个（少了 `import-entry`、`delete`）。未知动作的报错（`library.rs:174`）也只列出 7 个。
- `personal` 的 about 列了 5 个，实际 9 个。
- `knowledge` 的 about 写的是 `feedback | maintenance | promote`，实际 14 个。
- `report` 少了 `retry`；`hooks` 少了 `exec`；`collection` 少了 `check`/`remove`。
- `init --target` 的 help 列出 `claude/codex/grok/pi/opencode/cursor`，但报错提示里还支持 `alva/antigravity`（`ailoom init --local-path … --target bogus` 的输出就是证据）。
- 没有任何说明的参数：
  - `knowledge` 的 `--execute/--branch/--file/--name/--query/--limit`
  - `collection` 的 `--action/--name/--url/--ref`
  - `members --action`（说明只有"动作"二字）
  - `web`、`service start`、`service enable` 的 `--port`
- `knowledge --useful` 的说明写"useful | not-useful"，实际是布尔开关。
- 建议：动作列表用 `clap::ValueEnum` 定义，about、help 和报错都从同一个枚举生成，从根上防止漂移。长期可把 `--action` 风格改成真正的子命令，与 `service` 保持一致。

### C-15 ［CLI］P3：参数取值不校验
- `recall --kind bogus`、`recall --limit 0`、`recall -q ""` 都 exit=0，返回空结果。
- `code --hops 5` 和 `--hops 99999999999` 都被接受，而 help 写的是 0-2。
- `data --action rotate --max-size-mb 0` 被接受。
- 建议：用 clap 的 `value_parser!(u8).range(0..=2)` 和 ValueEnum。

### C-16 ［CLI］P3：`web`/`service start` 复用已有服务时静默忽略 `--port`
- 复现：先 `service start --port 48123`，再 `web --no-open --port 48999` 和 `service start --port 48124`，都返回 48123，没有任何提示。
- 建议：请求的端口和运行中的端口不一致时输出一行警告，并在 JSON 里带上 `requested_port`。

### C-17 ［CLI］P3：人类模式的 plan 中出现看起来重复的行
- 复现：`ailoom plan` 输出两行 `create AGENTS.md`。实际是两个 fragment（recall-hint 和 team-basics），JSON 里能通过 `item_key` 区分。
- 建议：人类输出附上 `#fragment:<id>`，或者合并显示成"AGENTS.md（2 个片段）"。

### C-18 ［CLI］P3：`scan-skills` 把内置 skill 标为 unmanaged
- 复现：`ailoom --json personal --action scan-skills --root .`，其中 `ailoom-share-learning` 的 `management` 为 `"unmanaged"`，但它是 `sync` 部署的内置资源（托管清单里的 item_key 是 `.claude/skills/ailoom-share-learning/SKILL.md`，文件级托管）。
- 建议：判断托管状态时，把"目录下的 SKILL.md 被托管"也算作托管。

### C-19 ［CLI］P3：提示文案前后矛盾，或混入内部术语
- `personal --action migrate-nongit` 在 Git 仓库里返回 `migrated:false`，`note` 却写"迁移完成"。
- `personal --scope /etc` 的报错是"**清单** scope 路径非法"，但这个值来自 CLI 参数，不是清单。
- hook 的"payload 无 cwd，使用进程 cwd"属于警告，却用 `[ailoom:error]` 级别输出。
- 面向用户的输出里夹带内部编号：顶层 help 的 `（AIL-0xx）`、`report digest` 的 "RW-08/R05"、`personal --repo` help 里的 "F01"。

### C-20 ［CLI］P3：错误码类别用错，部分错误码不在契约表里
- `source --dir <已有>` 冲突返回的是 `E5004`（宿主/适配类，exit 14），应属于清单/用法类。
- `init --url not-a-url` 返回 E2002，嵌套的 context 里还有一个 `E2007`。`E2006`/`E2007` 没有登记在 `CONTRACTS.md:258` 的码表里。
- 建议：补全码表，并加一个测试，断言 `error::code` 里的常量都在 CONTRACTS 表中出现。

### C-21 ［CLI］P3：三个 Web 入口并存
- `console`（前台，默认端口 47831）、`web`（后台服务，47831）、`dashboard`（7777）。
- `console` 退出时打印"网页服务已停止"。文档在主入口上口径不一致（见 D-06）。
- 建议：保留 `web` 作为唯一入口；`console` 隐藏或标记为调试用途；`dashboard` 并入 Web 控制台，或在 help 中注明其用途。

### 未执行或未验证
- 没有测试 `init --url` 带凭据的情况：本机安全 hook 禁止在命令中出现含 userinfo 的 URL。
- 没有执行 `service enable/disable`：`src/service/autostart.rs:126-133` 会调用真实的 `launchctl enable gui/<uid>/…`，修改本机 launchd 覆盖数据库，不受临时 HOME 隔离。这一点本身值得注意（P3，未验证影响）：测试和文档里都应说明该命令有系统级副作用。
- 没有测试 `pr`/`push`/`members register` 的真实远端流程（需要 gh 认证和远端仓库）。

---

## 2. UI（Web 控制台）

测试方式：
- 在隔离的 HOME（`audit-home-ui`）里用 `ailoom console --port 47951 --no-open` 前台启动服务，再用 curl 逐条调用路由。
- 浏览器侧用临时 profile 的无头 Chrome（端口 9241）跑 `tests/ui_browser.mjs` 的副本，副本只改了端口。
- 两个进程最后都按 PID 停掉了。另外发现本机 9231 端口上已有一个 headless Chrome（profile `/tmp/ailoom-usable`），应该是你在用的，没动它。

路由对照结论：
- 前端 `services/api.js` 调用的接口，在后端全部存在。
- 后端有、前端没用的接口只有两个：`GET /api/events` 和 `POST /api/service/probe`（后者由 CLI 内部使用，属正常）。
- 磁盘上的 UI 文件和 `src/console/ui.rs` 的嵌入表一致。
- Host/Origin 校验、写请求 token 校验都按预期生效。

### U-01 ［UI/后端］P1：全新安装后打开「能力库」直接显示错误
- 复现：在全新 HOME 下请求 `GET /api/library/list`（或 `/api/resources`），或者执行 CLI `ailoom --json library --action list`（本次 CLI 实测同样复现）。返回的 `issues` 里有 `清单不可读: [E3002] 源快照缺少 ailoom.toml … | context:{…}`。
- 页面上看到的：
  - 「以下能力无法读取」，后面跟一长串绝对路径和原始错误码。
  - 同一屏的空状态又写着「没有匹配的能力」，但用户根本没搜索过。
- 根因：`src/personal_library.rs:938` 一带没有区分「库还没初始化」和「清单损坏」两种情况。
- 建议：库根目录或清单不存在时，返回空列表、不放进 issues。空状态文案改成「还没有能力，导入第一个」。

### U-02 ［测试/UI］P1：`tests/ui_browser.mjs` 在当前工作树上各模式全部失败，CI 也不跑前端测试
- current/grouped 模式：卡在 `ui_browser.mjs:203`，因为导入弹窗里已经没有 `[data-import]`（`importDialog.js` 只剩 `data-import-msg`）。
- design 模式：卡在 `:117`。`#/projects` 现在由 `workspace.js` 渲染，页面上没有 `[data-filter]`。
- `.github/workflows/ci.yml` 两个前端测试都不跑：`ui_browser.mjs` 和 `frontend_components.mjs`。后者本地跑 11/11 通过。
- 建议：
  - 按现在的 DOM 更新选择器。
  - 把 `frontend_components.mjs`（只需要 node）加进 CI。
  - 在 `ui_browser.mjs` 文件头写清楚启动 Chrome 的命令，以及各模式的参数。

### U-03 ［UI］P2：死代码与未被引用的文件
- 走不到的代码：
  - `pages/projects.js:92-710` 的 `detail()` 约 618 行。`#/projects/manage` 挂载时总是传 `{}`，项目详情已经改由 `workspace.js` 负责。其中 `:164` 还有一个指向 `#/scopes` 的「高级配置…」链接。
  - `pages/scopes.js`、`pages/overview.js` 虽然还在 ROUTES 里，但 `app.js:165` 会先把它们重定向到 `#/projects`。
- 只被死页面引用的模块：
  - `features/scopePicker.js`、`features/capabilityMatrix.js`。
  - `features/importPreview.js` 不在任何 import 链上。
- 嵌入了但不加载的文件：`theme.css`、`theme.js` 仍由 `ui.rs` 嵌入并对外提供，但 shell 不加载，测试里甚至断言了它不加载。
- 没有任何引用的导出：
  - `state/draft.js`：patchDraft / saveDraft / resolveConflict / DEFAULT_DRAFT
  - `state/target.js:82-104`：LAYER_ORDER / SUBPROJECT_KEYS / originPath / upstreamOf
  - `planPreview.js:126` 的 VerificationPanel
  - `select.js:6` 的 Select
- 建议：直接删除。需要留档的话，用 git 历史即可。

### U-04 ［UI］P2：孤儿路由
- `#/workflows`、`#/sources`、`#/instructions` 都标记为 hidden。唯一链接到它们的是已经失效的 `overview.js`，所以只能手动输入 hash 才能进入。
- 这和 `app.js:36` 的注释「退为辅助入口」不一致。
- 建议：要么给它们补一个入口，要么下线。

### U-05 ［UI］P2：未知项目 id 被显示成「首次使用」
- 复现：打开 `#/projects/nonexistent`。
- 页面显示「添加你的第一个目录」，但实际上已经有项目（`workspace.js:436-437`）。
- 建议：显示「项目不存在」，并提供返回入口。

### U-06 ［UI］P2：违背 `docs/specs/console-design-system.md` 的地方
- `--control-height` 被全局覆盖：`workspace.css:162` 在 `:root` 上把它改成 32px，覆盖了 `tokens.css:49` 的 44px。
- 大量硬编码：`workspace.css` 有 263 处硬编码 px，而 `var()` 只有 69 处。多处字号是 11–12px（:175/178/180–197/201），低于 `--fs-tiny` 的 13px。
- 实测 `#/projects` 页面上 29 个可见按钮，高度全部低于 44px（26–40px），字号 12/13/14px 混用。
- `features/workflowStageList.js:104,107` 用了浏览器原生 `prompt()`，没有走规范要求的 Dialog / confirmAction。
- 规范要求静态 HTML 用 `data-variant` 契约，但所有 pages/features 里一处都没用。目前是靠类名实现：primary 24 处，danger 6 处。
- 规范定义的三层职责里没有 `workspace.css` 这一层。
- 建议：
  - 删掉对全局 token 的覆盖，改成 token 化的「紧凑密度」变体。
  - 用 Dialog 替换 `prompt()`。
  - 同步更新规范，把 `workspace.css` 的定位写进去。

### U-07 ［后端/安全］P2：`GET /api/jobs/<id>` 存在路径穿越
- 复现：`curl --path-as-is 'http://127.0.0.1:<port>/api/jobs/../../../../outside-job'` 能读到 jobs 目录以外、形状像 Job 的 JSON 文件。这个 GET 不需要 token（由 UI 子任务实测）。
- 根因：`src/console/jobs.rs:107-108` 的 `load_job` 直接执行 `jobs_dir.join(format!("{id}.json"))`，没有校验 id（代码已核对）。CLI `personal --action undo --id` 也走同一个函数（未验证）。
- 影响：服务只监听 loopback，且只能读到能被解析成 Job 的 JSON，所以实际风险有限。但这和其他路由的严格校验风格不一致。
- 建议：id 只允许 `^[a-z]+-[0-9a-f-]{36}$` 这种格式，或者只从内存表里查。

### U-08 ［后端］P2：控制台错误响应不规范
- 错误 JSON 只有 `{"error":"…"}`，没有 code 字段。
- `src/console/mod.rs` 里大约 50 处直接把 `Error` 的 Display 结果返回给用户，例如 `[E3007] 仓库根不是子项目 | context: {"rel":"."}`，前端会原样显示出来。只有 2 处用的是 `e.message`。
- 请求体不是合法 JSON 时，`mod.rs:264` 一带会静默当作 Null 处理，最后返回一个误导性的「需要 state」。
- 未知方法（DELETE/PATCH/OPTIONS/HEAD）返回的是 404「未知路径」，而不是 405。
- 建议：
  - 统一成 `{error, code, hint}` 结构。
  - JSON 解析失败时返回 400「请求体不是合法 JSON」。
  - 已知路径收到不支持的方法时返回 405。

### U-09 ［UI］P2：术语不统一、中英混排、窄屏折行
- 同一个概念有多种叫法：
  - 导航叫「能力库」，页面标题叫「资源库」（`collectionsPanel.js:54`），其他地方还有「资源中心」「个人库」。而 `CONTEXT.md` 定义的术语是「全局资源中心」。
  - 导航写「全局规则与 Agent」，页面标题写「全局 Rules 与 Agent」。
  - 「Worktree」和「工作树」混用（`sources.js`）。
- 混入英文或原始值：
  - `onboarding.js:19` 有英文 kicker「GET STARTED」。
  - `sources.js:31` 的列名是小写 `skill`。
  - `scopes.js:107-109` 直接显示 enable / disable / inherit。
- 排版：`projects.js:365` 的「在AI 工具」缺一个空格。
- 窄屏：390px 宽度下顶部导航文字竖着折行，例如「我的目/录」「全局规/则与/Agent」。
- 建议：以 CONTEXT.md 为准，建一张术语表，统一替换。导航在窄屏下改成汉堡菜单或横向滚动。

### U-10 ［UI］P3：其他打磨项
- 项目共享视图的页脚一直显示「项目默认配置已保存」（`workspace.js:329`），即使什么都没保存。
- token 出现在 URL 和日志里：
  - 启动 URL 带 `?token=`，还会打印到 stderr（`service/mod.rs:296,366`、`console/mod.rs:115`）。
  - 前端并不读取这个参数，而且 `GET /` 不需要鉴权就会把 token 注入页面。
  - 这和 `web.rs:1`「不进日志」的注释相矛盾。
- `/api/events` 只接受 query 里的 token，不认 header，而且前端没有用到它。建议删除，或者统一鉴权方式。
- `api.js` 把 404 归类为 `'validation'`。
- 标题不一致：`document.title` 是「AILoom 本地控制台」，`shell.html` 里是「AILoom · 资源工作台」。`app.js:165` 的注释写的是 `/settings`，和实际的正则不符。
- 数据根优先级：设置了 `XDG_STATE_HOME` 时，`AILOOM_DATA_ROOT` 会被忽略（`src/paths.rs:27-31`）。这符合 CONTRACTS v1.1 的明确决定，不算 bug。但「产品专属变量反而被通用变量覆盖」不太直观，建议在 `--data-root` 的 help 和 INSTALL 文档里写明。

### U-未验证
- 断线时 `checkConnection` 每 10 秒 notify 一次，可能会反复刷屏。
- Button 的 `onPress` 只有 try/finally，没有 catch。如果调用方也没 catch，会产生未处理的 Promise rejection。
- console 的 Mutex 中毒导致连锁 panic（见 T-04）。

---

## 3. 测试与质量

### T-00 测试结果（通过）
- `cargo test`：49 个测试二进制，**476 个通过、0 个失败、1 个忽略**。忽略的是 `packages_real_npm_install`，需要真实 npm 网络。日志见 （审查临时目录，未入库）。

### T-01 ［测试/CI］P1：`cargo clippy --all-targets -- -D warnings` 失败，共 7 个 error
- `.github/workflows/ci.yml:37` 跑的正是这条命令，所以 CI 必红。错误列表（行号为运行时的快照）：
  - `src/console/mod.rs`：`is_none_or` 违反 MSRV，共 2 处，代码片段 `meta.projects.as_ref().is_none_or(...)`。
  - `src/console/mod.rs`：`kind.clone()` 作用于 Copy 类型 `ResourceKind`，位置在 `id: crate::resource::ResourceId {source:"personal".into(),kind:kind.clone(),…`。
  - `src/console/mod.rs`：doc 注释后有空行（`empty_line_after_doc_comments`）。
  - `src/cc_switch.rs:251`：先判断 `is_none` 再 `unwrap`，即 `url.as_ref().unwrap()`。
  - `src/store.rs:345`、`:348`（测试代码）：`root.join(&key)` 中的 needless borrow。
- 日志：（审查临时目录，未入库）。
- 建议：修掉这 7 处；本地可加 pre-commit 钩子跑 `cargo clippy`。

### T-02 ［质量］P1：声明的 MSRV 1.75 已被违反，CI 也不验证 MSRV
- 证据：`Cargo.toml:5` 写 `rust-version = "1.75"`，但 console/mod.rs 用了 `Option::is_none_or`（1.82 才稳定），clippy 的 `incompatible_msrv` 已经报出来。CI 只用 `dtolnay/rust-toolchain@stable`（`ci.yml:26`），没有 1.75 的 job。
- 未验证：依赖树（Cargo.lock）是否还能在 1.75 下编译。本机没有 1.75 工具链。
- 建议：二选一。要么把 `rust-version` 提升到实际最低版本（至少 1.82），要么改写这两处，并在 CI 加一个 `cargo +1.75 check` job。

### T-03 ［质量］P2：`cargo fmt --check` 失败，31 个文件共 146 处 diff
- 热点：
  - `src/console/mod.rs` 41 处
  - `src/knowledge/location.rs` 11 处
  - `src/commands/personal.rs` 10 处
  - `profile.rs`、`collections.rs` 各 9 处
  - `scan_skills.rs`、`discover_directories.rs` 各 8 处
  - 另有若干 tests/ 文件
- CI 第 35 行会在这里失败。建议跑 `cargo fmt --all`，但等并行改动稳定后再统一执行，避免冲突。

### T-04 ［质量］P2：非测试代码中的 unwrap/expect 热点（共 138 处，没有 `expect(`）
| 文件 | 次数 | 说明 |
|---|---|---|
| src/profile.rs | 34 | 大多是 toml_edit 中"刚确保是表"之后的 `as_table_mut().unwrap()`，例如 :359。有保护，但很脆弱，建议用 `entry().or_insert` 改写 |
| src/console/mod.rs | 19 | 主要是 `state.events.lock().unwrap()`、`shutdown_flag.lock().unwrap()` |
| src/console/jobs.rs | 17 | `state.jobs.lock().unwrap()`（:193、:426 等） |
| src/sync/apply.rs | 14 | 如 :484 `segs.split_last().unwrap()`、:661 `as_array_mut().unwrap()` |
| src/cc_switch.rs | 9 | :251 是 clippy 已报的那处；:434 `entry["path"].as_str().unwrap()` |
| collections.rs 5 / personal.rs 4 / delete_skill.rs 3 / mcp.rs 3 / hosts.rs 3 / 其余各 ≤2 | | |
- 风险：console 和 jobs 中的 Mutex `lock().unwrap()`。只要有一个请求处理线程在持锁时 panic，锁就会中毒，之后所有请求都会连锁 panic，后台服务不可用（推测，未构造复现）。
- 建议：在 console 中统一用 `lock().unwrap_or_else(|e| e.into_inner())`，或者换成 `parking_lot`。sync/apply 中的 unwrap 改成返回 `E4004`。

### T-05 ［质量］TODO/FIXME
- 在 `src/`、`tests/`、`src/console/ui` 中都没有发现 `TODO`、`FIXME`、`XXX`、`HACK`、`unimplemented!`、`todo!(`。

### T-06 ［测试］P2：新模块的覆盖情况
- 无法生成行覆盖率：`cargo llvm-cov` 自动安装 `llvm-tools-preview` 时因 `libLLVM.dylib` 冲突而回滚。以下是静态统计。

| 模块 | 行数 | 内联 #[test] | 集成测试 |
|---|---|---|---|
| src/native_files.rs | 488 | 0 | tests/native_files.rs（6 个） |
| src/native_files/local.rs | 465 | 0 | tests/native_local.rs（11 个） |
| src/service/mod.rs | 388 | 0 | tests/service.rs（7 个） |
| src/service/autostart.rs | 301 | 2 | — |
| src/knowledge/location.rs | 875 | 0 | tests/knowledge_location.rs（14 个） |
| src/knowledge/portable.rs | 264 | 0 | tests/knowledge_portable.rs（8 个） |
| src/knowledge/state.rs | 566 | 0 | 只有 knowledge_portable 间接调用（checkpoint/recover/move） |
| src/adapters/hosts.rs | 390 | 0 | tests/host_adapters.rs（11 个） |
| src/adapters/discovery.rs | 32 | 0 | host_adapters.rs:247 有一处断言 |
| src/commands/scan_skills.rs | 222 | 1 | native_files.rs:123/144 |
| src/commands/discover_directories.rs | 99 | 2 | **没有 HTTP 层测试** |
- 结论：所有新模块都有集成测试，没有"零覆盖"的模块。但以下缺口已确认：
  - console 路由表里有 68 条 `/api/*` 路由，其中 **26 条在 tests/ 中没有出现过**（按字面路径匹配；动态拼接的已人工排除）：
    - `/api/collections/{check,remove,update}`
    - `/api/deploy-status`、`/api/effective`
    - `/api/fs/{pick-directory,read}`
    - `/api/library/{check-update,delete,import-entry,import-git,list,sources,update}`
    - `/api/migrations/cc-switch/location`、`/api/profile/scope`
    - `/api/project/{dirs,discover-directories,scan-skills}`
    - `/api/repo/relink`、`/api/resources/mcp-detail`
    - `/api/workflows`、`/api/workflows/{bind,record-input,rename,reviewed}`
  - `knowledge::state::{catalog, relocate}` 没有直接断言。
  - C-01（团队 sync 与个人 sync 互相翻转）、C-03（local 源 import）都缺少回归测试，说明跨模块组合的场景没有覆盖。
- 建议：按 C-01/C-03 补组合测试；给 26 条未覆盖路由各补至少一条冒烟测试（200 + 错误形状）。

---

## 4. 文档

脚本放在 （审查临时目录，未入库）：cards.py（三方一致性）、links.py（断链）、orphans.py（孤儿文档）、cmdcheck.py（文档命令与 help 对照）。

### D-01 ［文档］P1：README 和 BACKLOG 里的卡片统计过时
- `README.md:5` 和 `docs/BACKLOG.md:5` 写的是「98 张主卡，58 Backlog、4 Blocked、36 Done」。
- 实际情况：
  - `cards.json` 有 **128 张**：Done 122、Blocked 4（AIL-009/012/014/051）、Backlog 2（AIL-116/117）。
  - `docs/cards/*.md` 也是 128 个文件，状态和 cards.json 完全一致。
- 建议：用脚本从 cards.json 生成统计行，并放进 CI 检查。

### D-02 ［文档］P1：BACKLOG 总表有 58 张卡的状态与 cards.json 不一致
- 涉及的卡：AIL-039、040、042～050、052～098。它们在 cards.json 和卡片 md 里都是 Done，BACKLOG 表格里却还是 Backlog。
- 依赖、里程碑、规模三项是一致的。
- `AIL-120` 在 cards.json 里缺 `priority` 字段，BACKLOG 写的是 P1。
- 建议：BACKLOG 表由 cards.json 生成，不再手工维护。

### D-03 ［文档］P2：多个执行入口的叙述过时
- `BACKLOG.md:16` 写「本次只建卡…未实施新卡」，`:151` 写「本轮仅规划」，但 121～128 已经全部 Done。另外 `:133` 误用了 H1 标题。
- `IMPLEMENTATION-GUIDE.md:3` 仍然把 2026-09-17 对齐轮作为当前执行线。
- `COVERAGE.md:3` 写「当前共 51 张主卡」。
- `CONTEXT.md:75` 写「待实现……由 AIL-039/040 负责」，这两张卡都已 Done。
- `README.md:52` 写「286 个测试」，实测是 476 个通过加 1 个忽略。

### D-04 ［文档］P2：85 张 Done 卡的验收项全部是未勾选的 `- [ ]`
- 例如 AIL-052～098、AIL-121～128 都是这样，AIL-002/003/008 等旧卡各有大约 10 项未勾。
- 交付记录写在卡片末尾（例如 AIL-121 写的是「未通过或未验证：无」），和满屏的空勾选框互相矛盾。
- 建议：关卡时同步勾选；或者在卡片模板里写明勾选框不作为验收依据。

### D-05 ［文档］P1：QUICKSTART 里的 Codex Skill 路径过时
- `QUICKSTART.md:147`、`:172` 写的是 `.ailoom/skills/<name>`。
- 代码已经改成 `.agents/skills`（`src/adapters/common.rs:308` 的 `CODEX_NATIVE_SKILLS_DIR`）。本次 `ailoom plan` 的实际输出也是 `create .agents/skills/common-greet`。

### D-06 ［文档］P1：QUICKSTART 的 Web 部分和当前 UI 不符
- `:16-29` 写的是「浏览器向导六步」，实际 `onboarding.js` 只有 3 步（`tests/console_onboarding.rs:116` 断言的也是 3 步）。
- `:42`、`:50` 引用的「仓库与作用域」「资源库与流程」「资源库 → 导入资源」都已经不在导航里。现在的导航只有：我的目录 / 全局规则与 Agent / 能力库 / 操作记录（`app.js:27-39`）。
- `:77` 的「更新中心」是隐藏路由。「更新全部可用版本」这个按钮在 UI 代码里搜不到。
- `:13` 把 `ailoom console` 当作主入口，但 README 和 WEB-SERVICE 把 `ailoom web` 定为主入口（`WEB-SERVICE.md:17`）。
- 建议：按现在的 3 个页面重写这一节，入口统一写 `ailoom web`。

### D-07 ［文档］P2：QUICKSTART 对 Store 目录格式的描述前后矛盾
- `:97-98` 写的是可读路径 `store/github.com/owner/repo/revisions/<commit>`。
- `:171-172` 又写 `source_key` 是「可逆 base64url」。

### D-08 ［文档/代码］P1：宿主支持范围在多份文档里过时，且互相矛盾
- `QUICKSTART.md:225`、`SUPPORT.md:5-10`、`SCOPE.md` 都写「双宿主 Claude/Codex」。但 `init --target` 实际接受 8 个宿主，见 C-14。
- `capabilities/cursor-antigravity.md`（2026-09-11）写 Cursor 只支持 rules、未经验证。这和 `extra-hosts.md` 冲突。
- CLI 提示还在指向这份旧文档：`src/commands/doctor.rs:292`、`status.rs:168`。
- `extra-hosts.md:3` 写「Cursor 操作受阻」，但根目录的 ORCA 报告（09-27）已经记录了 Cursor 的 Rules、Skill、子 Agent、MCP 都在真实会话里验证过。

### D-09 ［文档/代码］P1：Codex Agent 支持情况在文档之间、代码之间互相矛盾
- `capabilities/codex.md:11` 和 `src/adapters/agents.rs:121-129` 都写「官方未确认，Unsupported」。
- `NATIVE-FILES.md:12,18` 却写已经核实了 `.codex/agents/*.toml`，而且原生编辑功能已经支持它。
- 建议：先确认官方文档，再统一同步适配器和这两份文档。

### D-10 ［文档/代码］P1：skip-worktree 的口径矛盾
- `SUPPORT.md:19,24` 和 `src/personal_instructions.rs:12` 都写「不使用 skip-worktree」。
- `NATIVE-FILES.md:33` 写了要用，`src/native_files/local.rs:427` 也确实执行了 `git update-index --skip-worktree`。
- 建议：SUPPORT 按功能分开写，并注明风险。

### D-11 ［文档］P2：SUPPORT.md 整体过时
- 日期还停在 2026-09-16。
- `:50` 写「AIL-024—037 未实现或部分实现」，这些卡已经全部 Done。
- Web 服务、原生文件、知识库迁移、合集这几项能力都没列进去。

### D-12 ［文档］P2：INSTALL.md 与现状不符
- `:3` 写「设计稿，cargo build 是唯一受支持的方式」。但 AIL-029 已经 Done，`scripts/install.sh` 也已存在（支持 auto/cargo/download 三种方式和 uninstall），只是没有任何文档提到它。
- 包名前后不一致：`:29` 写 `ailoom-cli`，`:57` 写 `ailoom`。
- `AILOOM_BIN_DIR` 默认是 `~/.ailoom/bin`，这是旧的数据根，和 XDG 迁移不一致。
- `packaging/npm/cli.js:13` 的下载地址是 `example.invalid`。

### D-13 ［文档］P2：WEB-SERVICE.md 末尾的验证记录过时
- `:56` 说 `console_onboarding.rs:116` 匹配的是已经删掉的文案，但现在这一行已经和 UI 一致了。
- `:54` 里的测试计数是 09-25 的快照。
- 建议：验证记录移到 evidence 目录，产品文档里不写测试计数。

### D-14 ［文档］P2：README 能力概览缺少新功能
- 没有提到 Web 服务、原生 Rules/Agent、知识库迁移、多宿主、Orca 插件。
- 没有链接 NATIVE-FILES.md、extra-hosts.md、plugins/orca。

### D-15 ［文档］P2：活跃卡片里有 36 个断链
- links.py 一共检查了 1344 个相对链接，发现 119 个断链，锚点链接 0 个错误。
- 活跃卡片里的 36 个都来自同一类「重新关卡记录」行，涉及 AIL-002/003/008/018～023/026/027/029/032～038。
- 原因：路径写成了 `evidence/logs/2026-09-15-rework-full-validation.log` 和 `rework/2026-09-15/README.md`，少了 `../`。目标文件本身都存在。示例：`docs/cards/AIL-018.md:90`。
- 建议：批量补上 `../`。

### D-16 ［文档］P3：其余 83 个断链
- `docs/rework/2026-09-15/RW-18.md:36` 有 2 个，少了 `../../`。
- 归档快照 `docs/reviews/2026-09-15-before-rework/` 里有 81 个：复制后目录深度变了，原来的 `../reviews/2026-09-12-*.md` 应该改成 `../../2026-09-12-*.md`。

### D-17 ［文档］P2：30 份非证据文档没有任何入链，且大量文档未纳入 git
- 孤儿文档举例：
  - NATIVE-FILES.md、SCOPE.md、COVERAGE.md、REFERENCES.md、friction-log.md
  - specs/console-design-system.md、project-first-console.md、cc-switch-source-migration.md
  - adr/0001
  - backend/ 下 8 张 B 卡
  - capabilities/alva.md、cursor-antigravity.md
  - reviews 下 11 份
  - 根目录的 ORCA 报告
- 从 README 出发可以链接到 258 份非证据 md 中的 224 份。
- `git status` 显示 297 个未跟踪文件，其中 54 份是 md，包括 AIL-099～128 的卡片、WEB-SERVICE、NATIVE-FILES、KNOWLEDGE-PORTABILITY、extra-hosts 等。已跟踪并修改过的 README 和 cards.json 都在引用它们，所以如果只提交一部分，会出现大量断链。

### D-18 ［文档］P3：docs/ 体积过大
- docs/ 总共 24MB，其中 evidence 占 21MB（176 张 PNG）。
- evidence 里还混有 28 个 mjs、5 个 sh、2 个 py 的验证脚本，都没有进 CI。

### D-19 ［文档/CLI］P3：`knowledge --action init` 的输出里有两个含义不同的 project_id
- 顶层的 `project_id` 是本机 id，例如 `repo-e89…`；`state.project_id` 是一个 UUID。
- KNOWLEDGE-PORTABILITY.md 只说明了 UUID 那一个。
- 建议：把顶层字段改名为 `local_project_id`，或者在文档里说明两者的区别。

### 已核实无问题
- 产品文档里 66 条带 `--flag`/`--action` 的 `ailoom` 命令，在真实 CLI 中全部存在（cmdcheck.py 对照 `--help` 验证）。
- 数据根优先级的描述和 `src/paths.rs:20-50` 一致。

### 根目录 `ORCA-PLUGIN-CURSOR-REVIEW-2026-09-27.md` 的去向
- 改名后移到 `docs/reviews/2026-09-27-orca-plugin-cursor-review.md`。
- 其中 Cursor 的实测结论并入 `docs/capabilities/extra-hosts.md`，同时改掉「Cursor 受阻」的说法。
- 在 `plugins/orca/README.md` 里加一条「背景调研」链接指向它。报告里写的「未安装插件、未执行构建」已经被后来的 `plugins/orca/` 实现部分取代，需要在文首加一句状态说明。
- 根目录只保留 README.md 和 CONTEXT.md。

### 建议的 docs 目录整理方案

**唯一入口（三层，每层只保留一个入口）**
1. 用户入口：`README.md`。只写简介、安装、`ailoom web` 和一张文档地图。不写卡片统计、测试计数和执行轮次。
2. 用户手册：`docs/QUICKSTART.md` 是唯一的上手文档，按当前 UI 重写。WEB-SERVICE、NATIVE-FILES、KNOWLEDGE-PORTABILITY、INSTALL 作为专题页，统一移到 `docs/guide/`，全部由 QUICKSTART 链接过去。
3. 开发入口：新建 `docs/DEVELOPING.md`，合并 IMPLEMENTATION-GUIDE、SCOPE 和 RELEASE-CHECKLIST 里的流程部分。状态以 `cards.json` 为唯一来源，`BACKLOG.md` 由脚本生成，禁止手改。

**合并**
- `SUPPORT.md` 合并到 `capabilities/README.md`，作为能力矩阵总表。下面按宿主分页：claude-code、codex、extra-hosts（含 Cursor/Grok/Pi/OpenCode）、alva。
- `cursor-antigravity.md` 里关于 Antigravity 和 zcode 的内容，并入 extra-hosts 的「未接入」小节，原文件归档。
- `COVERAGE.md`、`REFERENCES.md` 放进 DEVELOPING 的附录，或者直接归档。
- `specs/`、`adr/` 各加一个 README 索引，并从 DEVELOPING.md 链接过去。

**归档**（统一放到 `docs/archive/<日期或轮次>/`，附一份 README 说明「这是历史，不代表现状」）
- `REWORK.md`、`rework/2026-09-15/`、`reviews/2026-09-15-before-rework/`。顺手修复其中的断链，或者在 README 里声明这些链接不保证可达。
- `initiatives/` 下的 5 份轮次入口。
- `reviews/` 里 09-12～09-24 的过程审查，以及 `friction-log.md`（09-11 之后再没更新过）。
- `backend/` 移到 `docs/design/backend/`，并注明「仅设计，未实施」。`wireframes/` 和 `prototypes/` 合并到 `docs/design/`。

**证据**
- `docs/evidence/` 只保留 md 摘要和日志。PNG 迁到 Git LFS 或外部存储。可复用的脚本移到 `tests/` 或 `scripts/`。

**配套自动化（CI）**
- 跑 links.py（断链）、cards.py（cards.json、卡片 md、BACKLOG 三方一致）、cmdcheck.py（文档里的命令与 `--help` 对齐）。
- 代码里引用的文档路径也纳入检查，例如 `doctor.rs:292`、`status.rs:168`、`capability.rs:164`。

---

## 5. 推测/未验证问题汇总
- 依赖树能否在 Rust 1.75 下编译（T-02）：本机没有 1.75 工具链，未验证。
- console 的 Mutex 被 poison 后是否会连锁 panic（T-04）：没有构造复现。
- `src/commands/contribute.rs:121` 的旧 PR 贡献路径是否和 C-03 有同样的问题：这条路径在新的知识库模式下没有触达。
- `personal --action undo --id <穿越路径>` 是否会经由 `load_job` 读到工作区外的文件（U-07 的 CLI 侧）：未验证。
- `service enable/disable` 会调用真实的 `launchctl`：为了避免改动本机 launchd，没有执行。
- UI：`checkConnection` 断线后可能每 10 秒提示一次；Button 的 `onPress` 可能产生未处理的 Promise rejection。
