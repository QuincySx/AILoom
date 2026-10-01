# 2026-09-20 急用版交付：非 Git 知识库 + Skill 管理（USABLE-LOCAL 第一批）

对应 [AIL-110](../cards/AIL-110.md) / [AIL-111](../cards/AIL-111.md) / [AIL-113](../cards/AIL-113.md) / [AIL-114](../cards/AIL-114.md) / [AIL-115](../cards/AIL-115.md)，证据见 [docs/evidence/ui-usable-2026-09-20/](../evidence/ui-usable-2026-09-20/README.md)。
本批只声明「明确范围内的可用」：两个非 Git 知识库 + 一个 Git 项目的登记、扫描、导入、引用、应用、移除、隔离与重启持久化，全部经真实浏览器 + 真实 API + 磁盘核对；不声明全站完成。

## 一、启动方式

```bash
# 前置：cargo build（本批改动需重新编译；前端 JS 经 include_str! 打进二进制，改 JS 必须重新 cargo build）
cargo build

# 启动（默认读写用户目录；首次演示建议带独立 data-root）：
target/debug/ailoom console --port 8642
# 输出「控制台已启动：http://127.0.0.1:8642/?token=…」，浏览器打开该地址即可（token 已注入页面）。

# 完全隔离演示（不碰真实数据）：
HOME=/tmp/ailoom-usable/home XDG_STATE_HOME=/tmp/ailoom-usable/xdg-state \
XDG_DATA_HOME=/tmp/ailoom-usable/xdg-data GIT_CONFIG_GLOBAL=/tmp/ailoom-usable/home/.gitconfig \
GIT_CONFIG_NOSYSTEM=1 target/debug/ailoom --data-root /tmp/ailoom-usable/data console --port 8642
```

仅本机 loopback 可访问；写操作需要页面注入的会话令牌。

## 二、使用入口

1. **项目**（主导航）：新建项目 → 选择任意普通文件夹（无 Git 也可）→ 命名/分类 → 自动打开项目配置。
2. 项目页页签：
   - 宿主：选择 Claude Code / Codex CLI 是否为此项目启用（可检测版本）。
   - Skill：右上「添加」弹窗内搜索/勾选全局资源；弹窗内「＋导入」可直接把本地 Skill 文件夹或 Git 来源加入资源库，导入后自动选中新条目，全程不离开项目。页下方「项目目录里的 Skill」可只读扫描宿主目录与自定 Skill 根（如 `skills`），区分项目自有未托管 / AILoom 托管 / 外部链接，损坏条目单独报错。
   - Markdown 指令：项目级个人指令（保存范围固定为项目默认层）。
   - 预览与应用：生成预览 → 确认应用 → 面板明确显示写入项、路径与待验证状态；可在「操作记录」撤销。
   - 每行状态分别显示：本层设置 / 有效配置来源 / 磁盘部署状态（多宿主不一致会逐宿主列出；查询失败显示未知并可重试）。
3. 全局资源中心：来源管理、更新检查；从 CC Switch 迁移入口仍在。
4. 目录失联：项目页显示失联徽标与恢复指引（移回原路径后「重新检查」；或以新路径重新登记，身份随路径变化，不自动合并）。

## 三、五分钟演示路径（与 ail115.mjs 门禁一致）

1. 新建项目登记 `知识库 甲`（中文/空格路径的普通文件夹）→ 自动打开项目页，徽标「文件夹项目」。
2. Skill 页签 → 「扫描项目目录」，Skill 根填 `skills` → 看到：会议纪要（未托管）、周报生成（嵌套）、损坏技能（报错）、链接到乙（外部链接，不读内容）。
3. 「添加Skill」→ 弹窗内「＋导入」→ 本地 Skill 文件夹选 `导入源/检索技巧`，存储名称填 `retrieval-tips` → 预览 → 确认导入 → 弹窗自动选中新条目 → 「添加所选」。
4. 宿主页签启用 Claude Code → 「预览与应用」→ 生成预览 → 应用 → 面板显示「应用完成：写入 N 项」与 `.claude/skills/retrieval-tips` 路径；回到 Skill 页签该行显示「磁盘：已部署（与资源库一致）」。
5. 「从本项目移除…」→ 选「清除本层设置」→ 应用 → 磁盘文件清理；再「添加」→ 应用 → 文件恢复。
6. 用第二个普通文件夹登记 `知识库 乙`，同样添加并应用同一 Skill → 回到甲移除并应用 → 乙的文件与状态完全不受影响。
7. 重启服务再进入：项目、引用、资源库、部署状态全部保留；两座知识库的知识文档逐字节未变，且全程无 `.git` 产生。

## 四、已知限制（如实声明，不冒充完成）

- **文件部署通过，宿主待验证**：沙盒没有真实 Claude Code / Codex CLI 消费环境的端到端调用验证；「已部署」指文件与清单一致（AIL-077 范畴）。
- ~~未托管 Skill 没有删除/就地接管入口~~ **已交付**（AIL-112）：扫描区提供「删除…」两步确认（影响预览 → 输入目录名），归档到 project-archive 可恢复；托管资源的移除仍走「从本项目移除 + 应用」。
- **MCP 仅共享定义引用与只读配置**，没有密钥安全托管、更换与作用域管理（AIL-116 保持 Backlog：安全设计需先评审，且验收依赖宿主运行时）；不能宣称 MCP 密钥已安全托管。
- **指令为统一模型**：个人指令只写项目默认层，不支持按工作树/子项目差异；统一指令与最新宿主规则（AIL-117）保持 Backlog：官方加载规则核对与真实宿主加载验证未完成。
- **中文等非 ASCII 目录**：作为来源目录合法、扫描可读；但导入个人副本时存储名必须 ASCII（小写字母/数字/点/连字符），需在导入弹窗填写。
- **macOS 原生文件夹选择器**：无头浏览器里按设计禁用（手填路径可用）；本机桌面浏览器可用。
- 次级页面补漏与来源恢复（AIL-118/119）已交付：scopes 含文件夹项目、sources 显式目标、失效引用可解除、E3003 逐项处置（仓内链接放行、外逃/悬空定位跳过）。
- 预览/应用仍为显式两步（保存不等于生效）；是否改为保存即生效待产品确认。

## 五、责任文件（本批新增/修改）

第二批（AIL-112/118/119）补充：`collections.rs`（E3003 逐项处置 + warnings）、`resource.rs`（符号链接分类放行/跳过）、`commands/personal.rs`（悬空引用软降级 unresolved_references）、`features/scopePicker.js`、`pages/sources.js`、`pages/onboarding.js`、`pages/workflows.js`、删除服务 `POST /api/project/delete-skill`。

**未完成的卡**：AIL-116（MCP 凭据——安全设计需先评审 + 验收依赖宿主运行时）、AIL-117（统一指令——官方加载规则核对与真实宿主验证未做）；两卡保持 Backlog 并在卡内记录阻塞与建议拆卡方向，不包装成完成。

- 后端：`src/console/mod.rs`（read_repos_summary 失联合成、`/api/project/scan-skills`、`/api/resources` name/path 契约 + 单测）
- 前端：`src/console/ui/features/importDialog.js`（新增）、`resourcePicker.js`（空库＋导入/部分失败契约）、`planPreview.js`（应用成功面板）、`instructionsPanel.js`、`pages/projects.js`（扫描区、移除 Dialog、saveState 提升、F06/F07/F08）、`pages/instructions.js`（isDirty）、`pages/library.js`、`features/collectionsPanel.js`（导入复用）、`services/api.js`、`console/ui.rs`（资产注册）
