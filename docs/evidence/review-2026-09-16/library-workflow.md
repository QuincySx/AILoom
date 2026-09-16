# Library / Workflow Spec review

Range: c4a56a4...edc48bd. Read-only source review; only isolated fixture writes. No real credentials read, no remote writes.

## 1. P1: Ordinary Skill import corrupts library

Spec AIL-043:29–30: “空机器本地离线首次使用自动建立合法个人库” and “完整校验名称/资源身份/链接边界”.

Source: src/personal_library.rs:427 concatenates `{yaml}{extras}{body}` without newline. Lines 300–325 materialize official library before enumerate at line 330; failure leaves invalid file.

Observed fixture input SKILL.md:
```yaml
---
name: ordinary
description: Ordinary skill
---
# Ordinary
```
Observed resulting library file:
```yaml
---
name: ordinary
description: Ordinary skillnamespace: personal
shared: true

---
# Ordinary
```
Commands used absolute target/debug/ailoom, temporary cwd and explicit --data-root; child environment replaced with PATH=/usr/bin:/bin and temporary HOME, USERPROFILE, XDG_DATA_HOME, XDG_STATE_HOME.

`ailoom --json --data-root <fixture>/data library --action import --dir <fixture>/skill --execute`
Exit 12; stdout empty; stderr:
```json
{"code":"E3002","context":null,"fix":null,"message":"frontmatter 解析失败: mapping values are not allowed in this context at line 2 column 37"}
```
`ailoom --json --data-root <fixture>/data library --action list`
Exit 12; stdout empty; identical stderr. Original input unchanged. Fixture path is recorded in the companion evidence inventory.

## 2. P1: Editor accepts invalid resources and prevents repair

Spec AIL-049:29: “编辑个人 skill/doc/MCP、校验错误定位”; completion line 54 claims “doc/mcp … 保存前复验”.

src/console/mod.rs:1064–1083 only parses frontmatter for Skill. Doc/MCP receive no validation, Skill ownership/namespace/name are not verified. PUT a valid MCP resource with content `[` and current fingerprint: code writes it and returns saved. Next GET/list/PUT first enumerate entire library and fail. Static counterexample, not executed.

## 3. P1: Export destroys externally edited files

Spec AIL-045:32: “重复导出/版本冲突可恢复，已有 dirty 文档不被覆盖”.

src/workflow.rs:364–379 rejects tracked paths only, then writes unconditionally. Export an artifact to an untracked spec.md, externally edit, export again: changes overwritten without fingerprint, backup or revision recovery. Preview does not bind target state to execute. Static counterexample.

## 4. P2: Artifact versions do not protect content or renamed identity

Spec AIL-045:30: “重命名不丢链接”; AIL-049:32: “支持现有文件导入及版本冲突处理”.

src/workflow.rs:230–242 derives artifact ID from workflow/stage/title, writes unconditionally, then increments numeric version. Two saves of same title overwrite without base version or retained content. rename_artifact preserves old ID, but put_artifact with new title creates another ID; existing links continue to old content. Static counterexamples.

## 5. P2: MCP raw body returned without secret boundary

Spec AIL-049:31: “秘密不显示明文、不进入导出”; initiatives/local-console.md: “秘密只使用引用与缺失状态，不通过 API/日志/导出暴露”.

src/console/mod.rs:1023–1028 returns full resource body, rendered into editor. No rejection or redaction for literal credential fields. A valid MCP resource containing a dummy literal token is returned verbatim. Static evidence only, no real credential access.

Parent independently reported `missing_env_refs` exists only as definition in src/adapters/mcp.rs:121, with no callers; personal.rs has no missing-env handling. Thus AIL-049 completion line 54 claim “MCP 缺失引用状态进入个人 plan/sync notes（044）” lacks wiring. Parent supplied this search result; not independently rerun here.

## 6. P2: Required source/workflow actions missing while Done

Exact requirements:
- AIL-043:32: “本地源/远端订阅/团队贡献分开；保存个人库不自动 git add/commit/push，远端只读源编辑产生个人副本或贡献草稿。”
- AIL-043:33: “禁用/移除源展示受影响作用域；无效文件准确定位；保留注释、未知字段与外部编辑，保存有指纹冲突检查。”
- AIL-045:29: “四个 skill 通过用户选定实际资源身份绑定，缺项显示缺失；不能凭名字假定安装，不从未知位置静默复制。”
- AIL-049:32: “流程页关联对齐/spec/ticket/实现/验收材料，支持现有文件导入及版本冲突处理；规格改变后下游需复核可见。”
- AIL-049:33: “浏览器安全预览 Markdown，保留注释/未知字段/外部修改；删除/停用源有影响预览，保留可恢复草稿。”
- Shared normative local-console.md section Skill、MCP、源和文档: “远程团队源默认作为订阅只读；修改进入个人副本或明确贡献草稿，不隐式 commit/push。支持启停订阅、固定版本、离线状态、无效资源可定位；删除源先展示受影响作用域与托管项。”

src/console/web.rs:319–340 only lists personal resources/editor and workflow creation. Lines 390–405 show read-only binding table, existing artifact view/export, and new text artifact entry. No subscription management/copy flow, no binding controls, no existing-file import. /api/workflows/bind exists but page never invokes it; record_input lacks real entry point.

AIL-049:56 explicitly acknowledges “远程订阅源的个人副本编辑流（订阅只读→个人副本）尚未接入控制台（个人库为本地源）”, but line 57 says all mandatory acceptance passed. The shared specification and card clauses above make this an unmet requirement, not an optional scope suggestion.
