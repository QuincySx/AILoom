# CC Switch Skill 来源迁移

## 当前范围与边界

入口：资源中心 → 从 CC Switch 迁移。迁移的是仓库来源，不是 CC Switch 本地 Skill 的文件副本。

默认入口为「扫描 CC Switch 来源」：默认定位用户目录下 `.cc-switch`，允许通过原生文件夹选择器选择自定义数据目录。用户点击扫描后，产品通过只读 SQLite 组件查询固定的 Skill 来源字段；启动服务和打开弹窗都不自动读取数据库。**用户无需操作 SQL、导出数据库或准备清单。** JSON 文件/粘贴仅作为跨机器迁移的备用入口。

本次 Agent 仅在隔离的模拟数据库上执行了扫描测试，不访问用户的真实混合凭据数据库；真实扫描由用户在产品界面点击触发。不能把此前发现的 44 个本地 SKILL.md 当作 44 个已验证的来源。

产品内固定查询如下（仅供实现说明，不要求用户执行）：

```sql
SELECT id, name, directory, repo_owner, repo_name, repo_branch, readme_url
FROM skills ORDER BY name LIMIT 1001;
```

字段依据：[CC Switch Skills DAO](https://github.com/farion1231/cc-switch/blob/main/src-tauri/src/database/dao/skills.rs)。`directory` 是本地安装名，不能直接当作仓库路径；嵌套路径优先从 `readme_url` 提取。参见 [Skill 服务的 choose_doc_path](https://github.com/farion1231/cc-switch/blob/main/src-tauri/src/services/skill.rs)。核对日期：2026-09-17；未来 schema 改动需要重新核对。

## 流程

1. 扫描 CC Switch：用户点击后只读查询 skills 来源字段，离线解析，不联网、不登记。支持官方 snake_case / camelCase 字段，备用 JSON 也支持显式 repo_url、repo_path、source_url。HTTPS、Git SSH 和 skills.sh 入口可解析。来源缺失、地址冲突、不安全路径单项报错。
2. 勾选 Skill：有来源项默认选中；无效项禁选；全选/取消全选。清单或勾选变化使后续预览失效。
3. 拉取预览：按仓库合并，复用合集 Git 缓存/不可变快照。保留分支；同仓库多分支明确拒绝，不自动换分支。准确路径优先，其次唯一名称/目录名匹配；不唯一、缺失、拉取失败按仓库展示。此步骤只写缓存和预览。
4. 确认登记：只提交 ready 仓库；有错误的仓库明确保留在结果里。所有 ready 仓库使用同一 registry revision 原子提交；过期预览全部拒绝。整个仓库的可用资源目录纳入合集，但不自动部署或启用任何项目资源、MCP。
5. 来源管理：每个新合集保存 CC Switch 原 id、显示名、本地目录名、发现入口、上游地址、分支、已解析路径和资源 id。界面展示迁移时记录；现有检查更新机制负责后续 Git 更新，保留迁移记录。

已有同仓库同分支：匹配其当前锁定快照并显示“已存在”，不覆盖、不自动更新。不同分支冲突。重复操作可重新扫描/预览；已登记来源会跳过。一次最多 1000 条 Skill、100 个仓库。已有合集一次只锁定一个分支。

## 服务接口

全部复用控制台 Host / Origin / 会话 token 校验。

- `GET /api/migrations/cc-switch/location`：仅返回默认目录字符串，不读文件。
- `POST /api/migrations/cc-switch/read`：`{directory?, confirm_source_read:true}` → 读取固定 skills 字段并返回 scan_id、逐项来源/错误。自定义目录必须先授权；默认目录只对固定来源查询开放，不加入通用文件读取授权根。
- `POST /api/migrations/cc-switch/scan`：`{manifest: [...]}` → scan_id、逐项来源/错误。
- `POST /api/migrations/cc-switch/preview`：`{scan_id, selected: [item_id]}` → preview_id、仓库分组、锁定版本、匹配路径、失败信息。
- `POST /api/migrations/cc-switch/apply`：`{preview_id}` → 登记结果。执行只接受服务端保存的预览 ID，不信任前端传入的替换 URL 或路径。

清单和预览保存于数据根 `migrations/cc-switch/`，只持久化归一化来源字段；合集内 `migration` 保留导入时溯源信息。CC Switch 的 Skill 文件不写入、不删除，数据库以只读方式打开。没有执行仓库脚本或安装器。

## SQLite 组件边界

macOS 使用系统 `/usr/bin/sqlite3`；其他平台需可用的 sqlite3 CLI，目前未验证非 macOS 平台。固定启用 `-readonly -safe -nofollow -json` 和 `trusted_schema=OFF`，显式空 init 避免加载用户启动脚本；不接受用户 SQL、不加载扩展、不读取 providers/settings，不接受 skills 视图或虚拟表。查询失败不返回原始 SQLite stderr，避免暴露无关数据。依据：[SQLite CLI 文档](https://www.sqlite.org/cli.html)。

只读查询需要 SQLite 正常可读取的数据库，包括当前 WAL 状态；不使用可能忽略 WAL 的 immutable 副本。记录上限 1000 项，输出上限 2 MB；版本不兼容、数据库忙、目录错误在界面明确提示。
