# Spec: SkillStore 分桶实体 + Workspace 软链 + Require 选用

> 2026-09-17 修订：本页的 Base64 磁盘目录布局已被 [资源管理生命周期](resource-management-lifecycle.md) 替代。
> SourceKey 保留历史身份兼容；新实体使用可读来源路径。旧目录不会自动删除。

**Status:** local draft（本机规格，未挂 issue tracker）  
**Triage:** ready-for-agent（本地约定）  
**Related ADR:** ADR-0001 SkillStore + symlink  
**Glossary:** CONTEXT.md（SkillSource / SourceKey / SkillStore / Require / Workspace / ManagedArtifact）

---

## Problem Statement

团队希望把 Skill 维护在少数中央 SkillSource 仓库里，业务 Workspace 只声明需要哪些 Skill，并在宿主工具目录里「看起来装好了」。今天若每仓复制实体，会浪费磁盘、难对齐版本、也不符合「人无感跟上团队仓」的预期。成员不会去读手册；他们期望打开会话就能用到已选 Skill，且同一份实体可被多个 Workspace 共享。

## Solution

Skill 实体只落在本机 SkillStore，按 SkillSource 分桶；业务 Workspace 的托管路径只挂 symlink。Workspace（及可选 Agent 人设）用 Require 声明需要哪些 Skill；`sync`（含零感知自动同步）负责刷新 store、对齐本工作区链接集合。机器状态（binding、锁、事件）不进 SkillStore。

## User Stories

1. As a Team maintainer, I want Skills to live in a central SkillSource repo, so that many business Workspaces share one source of truth.
2. As a Team maintainer, I want Skill identity to be the path under the skills root (not a floating short alias alone), so that namespaces like `inking/line-art` stay unambiguous.
3. As a developer, I want my Workspace to declare which Skills it Requires, so that I do not get every Skill in the SkillSource installed.
4. As a developer, I want Required Skills to appear under `.claude/skills/<name>` (and Codex equivalents) as symlinks, so that host tools load them without a second copy.
5. As a developer, I want Skill entities under a shared store root (`AILOOM_STORE_ROOT` > `$XDG_DATA_HOME/ailoom/store` > `~/.ailoom/store/<source_key>/…`), so that multiple Workspaces share one materialized tree.
6. As a developer, I want `source_key` to be a reversible encoding of the normalized SkillSource identity, so that I can map a bucket back to its Git/local origin.
7. As a developer, I want git SSH and HTTPS forms of the same SkillSource to share one SourceKey, so that I do not get duplicate buckets.
8. As a developer, I want credentials stripped from identity before keying, so that tokens never become directory names.
9. As a developer, I want bucket metadata under `.meta/SOURCE.json`, so that skill folders never collide with sidecar files.
10. As a developer, I want store paths relative to the SkillSource skills root (no `resources/skills/` prefix, no extra `sources/` or nested `skills/` wrapper under store), so that the tree stays shallow and readable.
11. As a developer, I want `ailoom sync` to materialize/update SkillStore entities then create/update/remove Workspace symlinks to match the desired set, so that drift is correctable in one command.
12. As a developer, I want `ailoom plan` to show symlink create/update/delete/conflict without writing, so that I can preview ownership outcomes.
13. As a developer, I want ManagedArtifact tracking to treat symlinks as first-class managed content, so that uninstall and conflict rules still apply.
14. As a developer, I want SessionStart / prompt hooks to optionally auto-run refresh sync on a TTL (default 1 day), so that I stay current without remembering to sync.
15. As a developer, I want to disable auto-sync via env (`AILOOM_AUTO_SYNC=0`) and tune interval (`AILOOM_AUTO_SYNC_INTERVAL`), so that CI or air-gapped machines stay quiet.
16. As a developer, I want manual `ailoom sync --refresh` to force catching up to the SkillSource tip, so that urgent updates do not wait for TTL.
17. As a Team maintainer, I want revision lock behavior preserved (offline-safe by default), so that auto-sync does not silently break reproducibility unless refresh is intended.
18. As a developer working in a monorepo subdir Workspace, I want Require and sync to bind to that Workspace root, so that sibling packages can Require different Skill subsets.
19. As a developer, I want optional Agent persona files to further narrow Required Skills, so that a role-specific agent does not pull the whole Workspace set.
20. As a developer, I want unknown Required Skill names to fail loudly with a clear error, so that typos do not silently deploy nothing.
21. As a developer, I want removing a Require entry to remove the corresponding managed symlink on next sync, so that uninstall of a Skill from a Workspace is declarative.
22. As a developer, I want two Workspaces pointing at the same SkillSource to share store entities but keep independent symlink sets, so that Require stays local to each Workspace.
23. As a developer, I want conflict detection when two sources would claim the same host skill path, so that last-writer-wins never happens (E3006 class behavior).
24. As a developer, I want rules/agents/MCP to keep their existing deploy model in this slice, so that Skill topology changes do not silently rewrite other resource kinds.
25. As a security-conscious developer, I want Skills to declare env *names* only (not store secrets in SkillStore), so that credentials stay in the host environment or future alias maps—not in the skill tree.
26. As a Windows user, I want an explicit unsupported error for symlink Skill deploy until supported, so that I am not left with a half-written copy.
27. As a developer, I want `doctor` / `status` to report broken symlinks or missing store targets, so that I can repair without reading store internals.
28. As a developer, I want uninstall to remove managed symlinks without deleting SkillStore entities still used by other Workspaces, so that shared entities survive.
29. As a Team maintainer, I want Knowledge repos and fractal long-form docs left out of this slice, so that Skill distribution ships without unresolved knowledge-governance debates.
30. As an agent implementer, I want domain terms from CONTEXT.md used in UX copy and errors, so that users see Workspace / SkillStore / Require consistently.

## Implementation Decisions

- **Respect ADR-0001**: Skill entities live only in SkillStore; Workspace host paths for Skills are symlinks to store directories; `sync` owns materialize + link alignment.
- **Store layout (frozen for this slice)**:
  - `AILOOM_STORE_ROOT` > `$XDG_DATA_HOME/ailoom/store` (only if set) > `~/.ailoom/store`
  - `<store>/<source_key>/.meta/SOURCE.json`
  - `<store>/<source_key>/<rel-under-skills-root>/`
  - No `store/sources/` namespace; no `store/<key>/skills/` wrapper; no copying of `resources/` prefix into store.
- **SourceKey**: normalize identity (strip creds; unify git@/https; drop `.git`; lowercase; local as `local:<canon>`) then reversible base64url. Do not switch primary key to short human slugs.
- **Skill relative path**: strip configured skills root from resource path; reject parent-dir escape. Today enumeration may be one-level; layout must still allow nested rel paths (e.g. `inking/line-art`) when discovery supports them.
- **Require**: Workspace declaration (extend `.ailoom/project.toml` contract) expresses which Skills this Workspace needs; optional Agent persona files may further narrow. Deployed set = selection model (project ∪ role ∪ shared, plus existing multi-source tags/exclude) **intersected** with Require when Require is present. Exact TOML shape must be added to CONTRACTS in the same change set that implements parsing—prototype direction from product grilling:

```toml
# prototype shape (decision sketch, not yet frozen in CONTRACTS)
[[sources]]
name = "skills"
url = "https://example.com/team/skills.git"

[require]
skills = ["inking/line-art", "arch/system-design"]
```

  Until CONTRACTS is updated, treat the above as the intended seam; do not invent a second Require channel outside declaration/binding.
- **Selection vs Require**: existing resolver continues to decide eligibility by Project/Role/shared; Require is an additional Workspace-local gate so “eligible but not required” does not deploy. If Require is absent, preserve today’s deploy-all-eligible behavior for backward compatibility unless CONTRACTS explicitly flips the default.
- **Auto-sync**: hooks schedule background refresh on TTL (default 86400s); honor disable/interval env vars; must not write global dirs outside established data/store roots.
- **Modules (conceptual)**: SkillStore path/key/materialize; skills adapter emits symlink artifacts; plan/apply understand symlink create/update/delete/conflict; project declaration parsing for Require; sync_core wires selection→require→render; hooks/auto_sync for zero-perception refresh; docs (QUICKSTART, CONTRACTS, CONTEXT) stay aligned with ADR-0001.
- **Non-goals in modules**: do not move rules/agents/MCP/env/hooks into SkillStore in this slice; do not implement Knowledge SkillSource; do not build a credential vault.
- **Windows**: symlink apply remains explicit failure with actionable message until a later platform decision.
- **Uninstall**: remove managed Workspace links; do not garbage-collect SkillStore buckets automatically in this slice (optional GC can be a later story).

## Testing Decisions

### Seams (confirm)

**Primary seam (preferred, ideally the only one):** CLI integration around `init` → `plan`/`sync` (and optionally hook-triggered auto-sync) against a temporary business Workspace + SkillSource, with `HOME` / `AILOOM_STORE_ROOT` / `--data-root` isolated.

Assert **external behavior only**:
- Store contains entity at `<store>/<source_key>/<rel>/` with `.meta/SOURCE.json`
- Workspace skill path is a symlink whose target is that entity
- Require filters which links exist
- `plan` is dry-run; second `sync` is idempotent
- Conflict / unknown require / Windows unsupported paths surface stable error codes
- Uninstall removes links but leaves store entities when not GC’d

**Secondary seam (only if needed for pure functions):** SkillStore unit tests for normalize/key round-trip and rel-under-skills-root (already the pattern for path math). Do not add new mid-layer mocks for adapters if CLI coverage already proves the behavior.

Prior art: existing adapter/e2e/multi_source integration tests that drive the binary with temp repos; store module unit tests for key/layout.

### What makes a good test here

- Observe host-visible paths, symlink metadata, store tree, command exit codes/JSON issues—not private helper call sequences.
- Prefer one high seam so Require, store layout, and symlink apply are not tested as three disconnected stories.

## Out of Scope

- Knowledge repository bootstrap, indexing, or fractal long-form induction into Learning
- Moving rules, agents, MCP, env, hooks, or packages onto symlink/SkillStore topology
- Windows junction/symlink enablement beyond explicit error
- Automatic SkillStore garbage collection / refcounting across Workspaces
- Credential alias vault or secret materialization into skills
- New host tools beyond existing Claude / Codex targets
- Dashboard / KB Health visualization
- Changing TeamAI parity goals beyond this distribution topology

## Further Notes

- Partial landing already exists: ADR-0001, SkillStore layout helpers, skills symlink artifacts, auto-sync TTL hooks. This spec’s remaining center of gravity is **Require in the Workspace declaration + CONTRACTS freeze + end-to-end assertions**, plus any cleanup to keep docs/errors on the flattened store layout.
- Issue tracker was not configured for this project session; artifact is local at `docs/specs/0001-skill-store-require.md`. Apply tracker label `ready-for-agent` when publishing later.
- If the primary CLI seam above does **not** match maintainer expectations (e.g. Prefer a pure `sync_core` API seam instead), adjust Testing Decisions before implementation—do not multiply seams.
