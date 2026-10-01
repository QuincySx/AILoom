# 2026-09-20 急用版（非 Git 知识库 + Skill 管理）验收证据

对应卡片：AIL-110 / 111 / 113 / 114 / 115（第一批 USABLE-LOCAL）。
基线：HEAD 630bc4e + 当轮未提交修改。全部验证在隔离沙盒 `Isolated Sandbox /tmp/ailoom-usable` 完成：
独立 `HOME` / `XDG_STATE_HOME` / `XDG_DATA_HOME` / `GIT_CONFIG_GLOBAL`（gpgsign=false）/ `--data-root`，
不读真实 CC Switch 数据库与真实密钥，不改真实用户项目。

## 夹具

- `知识库 甲`（非 Git，中文+空格路径）：`读书笔记.md`、`skills/会议纪要`（合法）、`skills/分类/周报生成`（嵌套）、`skills/损坏技能`（无 frontmatter）、`skills/链接到乙 → 乙/skills/术语表`（越界符号链接）。
- `知识库 乙`（非 Git）：`使用说明.md`、`skills/术语表`。
- `项目 丙`（Git）：README.md 单提交仓库。
- `导入源/检索技巧`（中文目录名，用于内联导入，导入时提供 ASCII 存储名 `retrieval-tips`）。
- 基线哈希：`baseline-kb-hashes.txt`（首轮）、`pre-scan-hashes.txt`（扫描/验收对比用）。

## 重复执行方法

```bash
# 1) 启动隔离服务（端口 8646，占用时自动 +1，注意日志实际端口）
HOME=/tmp/ailoom-usable/home XDG_STATE_HOME=/tmp/ailoom-usable/xdg-state \
XDG_DATA_HOME=/tmp/ailoom-usable/xdg-data GIT_CONFIG_GLOBAL=/tmp/ailoom-usable/home/.gitconfig \
GIT_CONFIG_NOSYSTEM=1 nohup target/debug/ailoom --data-root /tmp/ailoom-usable/data \
console --port 8646 --no-open >> /tmp/ailoom-usable/evidence/console.log 2>&1 &

# 2) 启动无头 Chrome（CDP :9231）
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
  --remote-debugging-port=9231 --user-data-dir=/tmp/ailoom-usable/chrome-profile \
  --no-first-run about:blank &

# 3) 逐卡走查（依赖顺序执行；ail110 开头的「取消登记零写入」断言要求全新数据区，故先 reset）
export PATH="$HOME/.local/share/mise/installs/node/24/bin:$PATH"
cd /tmp/ailoom-usable/evidence
./reset115.sh && node ail110.mjs && node ail110-relink.mjs && node ail111.mjs && node ail113.mjs && node ail114.mjs
./reset115.sh && node ail115.mjs
node ail112.mjs && node ail118.mjs && node ail119.mjs
```

每张脚本自带 PASS/FAIL 断言与截图输出；`cdp.mjs` 为 CDP 驱动。ail113/ail114 开头通过真实服务调用
（`/api/profile/select inherit`、`/api/library/delete`）清理上次运行残留，可重复执行。
2026-09-21 复核：全套按上述顺序在 HEAD 630bc4e + 未提交修改上重跑全绿（110=21、relink=5、111=11、
113=17、114=28、115=22、112=11、118=11、119=8 PASS）。当轮修复两处脚本基建（非产品代码）：
ail110.mjs 宿主行选择器随 UI 重构同步（`small`→`.row-head strong`，Claude Code）；reset115.sh 的
kill 模式收窄为 `pgrep -f "target/debug/ailoom"`（原模式按命令行匹配 "ailoom-usable" 会误杀
user-data-dir 位于沙盒内的无头 Chrome）。

## 证据索引

| 卡 | 脚本 | 关键截图/文件 | 结论 |
|---|---|---|---|
| AIL-110 | ail110.mjs、ail110-relink.mjs | 01~09 截图、ail110-cli-effective.txt | 登记/宿主/指令/plan/apply/重启/CLI 对照全部通过；失联可恢复状态为当轮修复 |
| AIL-111 | ail111.mjs | 01~04 截图 | 扫描只读、分类（托管/未托管/外部链接/损坏）、跨项目隔离、重启可用 |
| AIL-113 | ail113.mjs | 01~05 截图 | 空库可打开＋＋导入、内联导入回填选择、F03 name/path 契约（另有 Rust 单测 `console_tests::local_entry_json_includes_name_and_path`）、取消零写入、重启一致 |
| AIL-114 | ail114.mjs | 01~07 截图 | F04 移除两语义（含上层启用警告与 plan 无改动验证）、F05 部分失败报告与重试、F06 范围控件禁用与草稿保护、F07 应用结果面板、F08 不一致/未知/重试 |
| AIL-115 | reset115.sh + ail115.mjs | 01~12 截图 | 全旅程门禁：登记→扫描→内联导入→添加→宿主→预览→应用→文件与状态→移除→再应用→B 隔离→重启持久化→知识库哈希不变 |

## 修复前反例（当轮已修）

- F01：项目页看不到本目录 Skill（ail111-01 之前无扫描区）→ 新增 `/api/project/scan-skills`。
- F02/F03：空库「添加」禁用、本地副本无 name/path（ail113-01 之前）→ `/api/resources` 契约修复＋选择器改造。
- F04：移除一律 inherit 且承诺清理（上层启用时与现实矛盾）→ 移除 Dialog 双语义＋继承预测。
- F05：批量中途失败已写项不报告 → 逐项结果与重试契约。
- F06：切范围直接丢稿 → 脏确认＋指令页禁用范围控件；独立指令页补 `isDirty`。
- F07：应用成功无任何结果（ail110-06 为修复前反例截图）→ JobPanel 成功面板。
- F08：查询失败显示「未部署」、双宿主取最优 → 未知+重试、不一致逐宿主列出。
- 当轮新发现：非 Git 项目目录失联无恢复指引（ail110-08 前后对比）；中文目录名导入 E3002
  （导入 Dialog 增加存储名称输入）；扫描把托管链接按 Store 根误判为外部链接（改用 `resolve_store_root` 规范化比较）。

## 2026-09-20 UI 重构补充（用户反馈）

用户指出 Skill 页签问题：同一 Skill 在引用列表与扫描区重复出现、行内控件错位无标签、三行状态堆叠冗长。
重构（repro-user-view-1180.png 为修复前复现，redesign-*.png 为修复后）：

- 状态行改为徽章横排（本层/有效/磁盘，绿=一致、琥珀=待更新/不一致），文案不变（保持脚本断言兼容）；
- 引用行两段式：名称+来源徽章 / 描述 / 状态徽章 / 资源 id，右侧动作列「本层设置」标签 + 下拉 + 保存 + 移除单行对齐，窄屏自动换行（390/1180/1440 无横向溢出）；
- 扫描区去重：已在上方引用列表中的托管部署不再重复展示，改为汇总提示，只列本地特有条目；
- 宿主/其他资源/简单行同步应用同一结构。

回归：ail115 门禁 22 项、ail114 全部断言重跑通过；知识文件哈希不变（ail112 遗留探针文件已清理并加入其清理步骤）。

## 2026-09-20 二次重构（用户二次反馈）

第一版去重补丁引入了三句重复文案，且状态徽章仍是内部术语（本层/有效/期望部署）、裸资源 ID 占一行。
按成熟"已安装资源管理"惯例（Chrome 扩展/VS Code 扩展面板）重做：

- 行 = 名称 + 一个状态徽章（已启用/已停用/未启用）+ 一句描述 + 一行次要小字（磁盘：已部署/待更新/未部署/部署不一致/未知（重试）· 作用范围/来源）；
- 资源 ID 移出行外（改为行 title 提示）；动作列：启用/停用下拉 + 保存 + 移除，右对齐单行；
- 扫描区收窄为「本地 Skill」小节：单行状态（未发现 / 发现 N 项本地 Skill；另有 M 项托管部署见上方列表），空态不再重复文案；
- 修复：AIL-119 来源提示消息不再覆盖刚保存的操作反馈。

回归：ail114（26 项）、ail115（21 项）、ail111（11 项）全部通过；脚本断言同步更新到新文案，ail115 增加"已引用则跳过导入"可重入处理。
