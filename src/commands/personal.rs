//! 个人模式命令（AIL-044 接线；消费 AIL-039/040/042/043 的服务）。
//!
//! 部署期望 = 团队层（若有声明且源就绪；个人模式关闭内置与索引片段）
//! + 个人层有效配置（三态选择解释，profile 驱动）
//! + 个人指令条目（042）。所有个人新增经公司文件守卫；不写 .ailoom/project.toml。
//!
//! 2026-09-16 复审返工：
//! - F06：非 Git 路径模式显式登记（nongit-<hash>），effective/select/plan/sync 全链路可用。
//! - F07：子项目作用域的部署落点是子项目目录（<worktree>/<rel>/…），与仓库根作用域
//!   物理隔离——统一计划器按作用域过滤清理动作，父子 sync 不再互删同一入口。
//! - F03：团队层产物按最终 enabled_hosts 过滤（个人禁用宿主后团队资源不再照写）。
//! - S02：公司文件保护覆盖全部计划动作（含清单清理的删除/恢复），且 apply 前复核。
//! - F01/F02：select 以显式仓库/Worktree 根定位身份，逐字段合并不整层替换。
//! - F09：MCP 缺失引用环境变量检查进入 notes（只报缺失，不读取/输出值）。

use crate::adapters::{render_lenient as render_artifacts, ToolTargets};
use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use crate::personal_instructions as pi;
use crate::profile::{
    resolve_effective, EffectiveConfig, PersonalProfile, ResolveScopeRequest, SelectKey,
    SelectScope, TriState,
};
use crate::repo_registry::{self, RepoDiscovery, RepoIdentity, RepoRegistry};
use crate::resolver::{resolve_isolating, ResolveRequest};
use crate::source::LocalSource;
use crate::sync::manifest::ManagedManifest;
use crate::sync::plan::build_plan;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::sync_core::Prepared;

/// 个人模式准备产物。
pub struct PersonalPrepare {
    pub ctx: AppContext,
    pub repo: RepoDiscovery,
    pub registry: RepoRegistry,
    /// 当前 Worktree 在登记中的 id（非 Git 路径模式为 "root"）
    pub wt_id: String,
    /// 当前作用域相对仓库根的路径（None = Worktree 根）
    pub active_rel: Option<String>,
    /// 部署落点：仓库默认/worktree 作用域 = Worktree 根；子项目作用域 = Worktree 根/<rel>
    pub scope_dir: PathBuf,
    /// 非 Git 路径模式（F06）
    pub is_nongit: bool,
    pub profile: PersonalProfile,
    pub effective: EffectiveConfig,
    pub artifacts: Vec<crate::adapters::common::Artifact>,
    pub plan: crate::sync::plan::SyncPlan,
    pub managed: ManagedManifest,
    pub managed_path: PathBuf,
    /// 被公司文件守卫跳过的目标与原因
    pub skipped: Vec<pi::SkippedTarget>,
    /// 团队层就绪说明（声明存在但源未锁定时为提示语）
    pub notes: Vec<String>,
    /// AIL-119：引用了但来源不可解析的资源 id（软降级，不再硬失败）
    pub unresolved_references: Vec<String>,
}

/// 团队层：有声明且源就绪 → Some(prepared)；无声明 → None；
/// 声明存在但源未锁定 → None + note（个人模式不强制团队 init）。
fn team_layer(
    data_root: Option<&Path>,
    explicit_root: Option<&Path>,
    notes: &mut Vec<String>,
) -> Result<Option<Prepared>> {
    let cwd = std::env::current_dir()?;
    // 个人模式：非 Git / 无声明目录没有团队层，不因此报错（AIL-057 闭环）
    let ws = match crate::workspace::discover(&cwd, explicit_root) {
        Ok(ws) => ws,
        Err(e) if e.code == code::WORKSPACE_ROOT_NOT_FOUND => {
            notes.push("无团队声明（非 Git 或未绑定目录）：跳过团队层".into());
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let decl_path = match &ws.declaration_path {
        Some(p) => p.clone(),
        None => return Ok(None),
    };
    let declaration = crate::config::ProjectDeclaration::load(&decl_path)?;
    if declaration.is_none() {
        return Ok(None);
    }
    match super::sync_core::prepare(data_root, explicit_root) {
        Ok(p) => Ok(Some(p)),
        Err(e) if e.code == code::SOURCE_NOT_CACHED => {
            notes.push(
                "存在团队声明但团队源未锁定：个人模式跳过团队层（需要时运行 ailoom init 完成锁定）"
                    .into(),
            );
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// 非 Git 路径模式身份（F06）：`nongit-` + 路径哈希。无 Git 元数据可用，
/// 身份即路径本身；路径搬迁视为新模式（与 Git 仓的身份证据模型不同，显式说明）。
pub fn nongit_identity(root: &Path) -> RepoDiscovery {
    let repo_id = format!(
        "nongit-{}",
        crate::ids::sha256_prefix(root.to_string_lossy().as_bytes(), 16)
    );
    RepoDiscovery {
        identity: RepoIdentity {
            repo_root: root.to_path_buf(),
            common_dir: root.to_path_buf(),
            repo_id,
            origin_normalized: None,
            origin_raw: None,
        },
        worktrees: Vec::new(),
        start: root.to_path_buf(),
        current_worktree: root.to_path_buf(),
    }
}

fn nongit_ctx(data_root: Option<&Path>, root: &Path) -> Result<AppContext> {
    let data_root_buf = crate::paths::resolve_data_root(data_root)?;
    let data_root = &data_root_buf;
    let anchor = format!(
        "nongit+{}",
        crate::ids::sha256_prefix(root.to_string_lossy().as_bytes(), 32)
    );
    let workspace = crate::workspace::Workspace {
        workspace_root: root.to_path_buf(),
        anchor_key: crate::ids::sha256_prefix(anchor.as_bytes(), 16),
        repository_anchor: anchor,
        workspace_id: crate::ids::workspace_id_from_root(root),
        is_git: false,
        declaration_path: None,
    };
    let layout =
        crate::paths::layout_for(data_root, &workspace.workspace_id, &workspace.anchor_key);
    let device_id = crate::config::device_id(data_root)?;
    Ok(AppContext {
        data_root: data_root_buf,
        layout,
        workspace,
        device: device_id,
    })
}

/// 校验作用域相对路径并返回部署落点（F07）：目录必须存在（未匹配不自动创建）、
/// 不越界、不穿越嵌套 Git 边界。
fn validate_scope_dir(
    discovery: Option<&RepoDiscovery>,
    worktree_root: &Path,
    rel: &str,
) -> Result<PathBuf> {
    crate::manifest::validate_relative_path("scope", rel)?;
    let scope_dir = worktree_root.join(rel);
    if !scope_dir.is_dir() {
        return Err(Error::new(
            code::WORKSPACE_INVALID,
            format!("子项目目录在本 Worktree 不存在（未匹配；不自动创建业务目录）: {rel}"),
        )
        .context(serde_json::json!({ "rel": rel })));
    }
    if let Some(d) = discovery {
        let check = repo_registry::check_subproject(d, rel);
        if !check.ok {
            return Err(Error::new(code::ILLEGAL_PATH, check.reason)
                .context(serde_json::json!({ "rel": rel })));
        }
    }
    Ok(scope_dir)
}

/// 清理动作的作用域归属（F07）：仓库根 sync 只清理根作用域条目；
/// 子项目 sync 只清理本子项目路径下的条目——父子不互删。
fn path_in_scope(path: &str, active_rel: Option<&str>, registered_rels: &[String]) -> bool {
    // A directory may deploy inherited choices without ever registering an
    // override. Recover its owner from the adapter destination, so root and
    // parent plans cannot delete entries belonging to that child directory.
    let parts: Vec<_> = path.split('/').collect();
    if let Some(index) = parts.iter().position(|part| {
        matches!(
            *part,
            ".claude"
                | ".agents"
                | ".codex"
                | ".ailoom"
                | ".grok"
                | ".pi"
                | ".opencode"
                | ".cursor"
                | ".antigravity"
                | ".mcp.json"
                | "CLAUDE.md"
                | "AGENTS.md"
                | "AGENTS.override.md"
        )
    }) {
        return parts[..index].join("/") == active_rel.unwrap_or("");
    }
    let owner = registered_rels
        .iter()
        .filter(|r| !r.is_empty() && path.starts_with(&format!("{r}/")))
        .max_by_key(|r| r.len())
        .map(String::as_str)
        .unwrap_or("");
    owner == active_rel.unwrap_or("")
}

#[test]
fn directory_cleanup_preserves_unregistered_and_nested_scopes() {
    assert!(path_in_scope(".claude/skills/a", None, &[]));
    assert!(!path_in_scope("web/.claude/skills/a", None, &[]));
    assert!(path_in_scope("web/.claude/skills/a", Some("web"), &[]));
    assert!(!path_in_scope(
        "web/docs/.agents/skills/a",
        Some("web"),
        &[]
    ));
    assert!(!path_in_scope("AGENTS.md", Some("web"), &[]));
    assert!(!path_in_scope(
        "web/docs/custom.txt",
        Some("web"),
        &["web".into(), "web/docs".into()]
    ));
}

/// 撤回中途失败的个人同步：按与 personal sync 相同的方式定位工作区（Git 与非 Git 目录都适用），
/// 持同步锁执行 journal 恢复，与正在进行的同步互斥。
pub fn recover(root: &Path, data_root: Option<&Path>) -> Result<Value> {
    let root = root
        .canonicalize()
        .map_err(|e| Error::new(code::WORKSPACE_INVALID, format!("目录不可用: {e}")))?;
    let ctx = match repo_registry::classify_path(&root)? {
        repo_registry::PathClass::Git(_) => AppContext::discover(data_root, &root, Some(&root))?,
        repo_registry::PathClass::NonGit { root } => nongit_ctx(data_root, &root)?,
    };
    let _lock =
        crate::sync::lock::SyncLock::acquire(&ctx.layout.ws_dir.join("locks"), &ctx.device)?;
    let report =
        crate::sync::apply::recover(&ctx.layout.journal_dir, &ctx.workspace.workspace_root)?;
    let value = json!({
        "ok": report.ok,
        "recovered": report.recovered,
        "skipped_user_modified": report.skipped_user_modified,
        "broken_backups": report.broken_backups,
        "pending_runs": report.pending_runs,
    });
    if !report.ok {
        return Err(Error::new(
            code::JOURNAL_RESTORE_FAILED,
            "恢复失败：存在缺失或摘要不匹配的备份，未清理恢复点",
        )
        .context(value));
    }
    Ok(value)
}

/// 渲染结果：显式选中的资源库 / 合集资源。
pub(crate) struct SelectedRender {
    pub artifacts: Vec<crate::adapters::common::Artifact>,
    pub unsupported: Vec<crate::adapters::UnsupportedItem>,
    pub notes: Vec<String>,
    pub entries: Vec<crate::resource::ResourceEntry>,
    /// 引用了但来源读取失败或条目已不存在的合集资源
    pub unresolved: std::collections::BTreeSet<String>,
}

/// 把显式选中的资源库（personal/*）与合集（collection-*）资源渲染为产物；
/// 项目部署与全局部署共用，保证两边对坏条目、缺失来源、外部目录的处理一致。
/// `library_root` / `collection_root` 是渲染时的落点根（项目为 Worktree 根 / 作用域目录）。
pub(crate) fn render_selected(
    data_root_resolved: &Path,
    ids: &[String],
    targets: &ToolTargets,
    library_root: &Path,
    collection_root: &Path,
) -> Result<SelectedRender> {
    // 资源库层：被启用的 personal/* 资源（AIL-043 库 → AIL-040 选择）
    let personal_enabled: Vec<String> = ids
        .iter()
        .filter(|k| k_starts_personal(k))
        .cloned()
        .collect();
    let mut personal_selected_entries: Vec<crate::resource::ResourceEntry> = Vec::new();
    let mut unsupported = Vec::new();
    let mut notes = Vec::new();
    let mut unresolved_refs = std::collections::BTreeSet::new();
    let mut artifacts = Vec::new();
    if !personal_enabled.is_empty() {
        let lib = crate::personal_library::ensure_library(data_root_resolved)?;
        let src = LocalSource::new(&lib.path)?;
        let snap = src.resolve()?;
        let manifest = crate::manifest::TeamManifest::load_from(&snap.root)?;
        let (desired, invalid_entries) = resolve_isolating(ResolveRequest {
            snapshot_root: &snap.root,
            manifest: &manifest,
            source: crate::personal_library::LIBRARY_TEAM_ID,
            identity: &src.identity,
            revision: snap.resolved_commit.clone(),
            content_digest: snap.content_digest.clone(),
            active_projects: &[],
            active_roles: &[],
        })?;
        if !invalid_entries.is_empty() {
            notes.push(format!(
                "资源库中有 {} 个条目无效，已跳过（不影响其他资源）：{}",
                invalid_entries.len(),
                invalid_entries
                    .iter()
                    .map(|e| e.path.as_str())
                    .collect::<Vec<_>>()
                    .join("、")
            ));
        }
        // 过滤出被启用的资源
        let filtered_desired = filter_desired_by_ids(&desired, &personal_enabled);
        // AIL-044：不凭名字假定安装——被启用但资源库中不存在的资源显式 unsupported
        let resolved_ids: std::collections::BTreeSet<String> = filtered_desired
            .selected
            .iter()
            .map(|s| s.id.clone())
            .collect();
        for id in &personal_enabled {
            if resolved_ids.contains(id) {
                continue;
            }
            if let Some(bad) = invalid_library_entry(&invalid_entries, id) {
                unsupported.push(crate::adapters::UnsupportedItem {
                    resource_id: id.clone(),
                    tool: "-".into(),
                    kind: bad.kind.as_str().into(),
                    reason: format!(
                        "资源定义无效，未部署：{}：{}（已部署的文件保留；修复后重新同步）",
                        bad.path, bad.error.message
                    ),
                });
            } else {
                unsupported.push(crate::adapters::UnsupportedItem {
                    resource_id: id.clone(),
                    tool: "-".into(),
                    kind: "resource".into(),
                    reason: "资源库中不存在该资源（不凭名字假定安装）；请在资源库导入或修正资源 ID"
                        .into(),
                });
            }
        }
        personal_selected_entries = filtered_desired
            .selected
            .iter()
            .map(|s| s.entry.clone())
            .collect();
        let (mut lib_artifacts, lib_unsupported) =
            render_artifacts(&filtered_desired, &snap.root, targets, library_root)?;
        artifacts.append(&mut lib_artifacts);
        unsupported.extend(lib_unsupported);
    }

    // 合集订阅只提供目录，必须按完整资源 ID 显式选用；不套团队 shared 自动启用规则。
    let collections = crate::collections::load(data_root_resolved)?;
    let mut remaining: std::collections::BTreeSet<String> = ids
        .iter()
        .filter(|id| id.starts_with("collection-"))
        .cloned()
        .collect();
    for source in collections.sources.values() {
        let prefix = format!("{}/", source.id);
        let selected_ids: Vec<String> = remaining
            .iter()
            .filter(|id| id.starts_with(&prefix))
            .cloned()
            .collect();
        if selected_ids.is_empty() {
            continue;
        }
        // AIL-119：来源读取失败（快照缺失/路径失联）不再硬失败 —— 记入 notes，
        // 该来源的已登记引用进入 unresolved_references，由前端提供解除入口。
        let catalog = match crate::collections::catalog(data_root_resolved, source) {
            Ok(c) => c,
            Err(e) => {
                notes.push(format!(
                    "来源「{}」读取失败：{}；其已登记引用标记为来源不可用，可在项目中解除",
                    source.name, e
                ));
                continue;
            }
        };
        let mut selected = Vec::new();
        for entry in catalog.entries {
            let id = entry.id.to_string();
            if !selected_ids.contains(&id) {
                continue;
            }
            remaining.remove(&id);
            personal_selected_entries.push(entry.clone());
            selected.push(crate::resolver::Selected {
                id,
                kind: entry.id.kind.as_str().into(),
                name: entry.id.name.clone(),
                namespace: entry.id.namespace.clone(),
                reason: "个人显式引用合集资源".into(),
                entry,
            });
        }
        crate::resolver::check_target_conflicts(&selected)?;
        let desired = crate::resolver::DesiredSet {
            source: source.id.clone(),
            // 不同锁定版本使用不同实体；更新一个 Worktree 不能经共享 symlink 偷改其他 Worktree。
            identity: format!(
                "{}#{}",
                source.lock.identity,
                source.lock.resolved_commit.as_deref().unwrap_or_default()
            ),
            skills_root: catalog.skills_root,
            revision: source.lock.resolved_commit.clone(),
            content_digest: source.lock.content_digest.clone(),
            active_projects: vec![],
            active_roles: vec![],
            selected,
            excluded: vec![],
        };
        let (mut rendered, missing) =
            render_artifacts(&desired, &catalog.snapshot.root, targets, collection_root)?;
        if source.external_path.is_some() {
            for artifact in &mut rendered {
                if let crate::adapters::common::ArtifactBody::Symlink { source_dir, .. } =
                    &artifact.body
                {
                    artifact.body = crate::adapters::common::ArtifactBody::ExternalSymlink {
                        target: source_dir.clone(),
                    };
                }
            }
        }
        // 个人合集不生成公司指令索引；其余仍经同一所有权/公司文件守卫。
        rendered.retain(|a| a.resource_id != "ailoom-internal/doc-index");
        artifacts.extend(rendered);
        unsupported.extend(missing);
    }
    if !remaining.is_empty() {
        // AIL-119：来源读取失败或条目消失时不再硬失败整个项目页 ——
        // 记入 notes 与未解析引用列表（effective 返回 unresolved_references），
        // 前端展示「来源不可用」行并提供解除引用入口；plan 跳过对应产物。
        let ids: Vec<String> = remaining.iter().cloned().collect();
        notes.push(format!(
            "引用的合集资源不存在（来源读取失败或已删除）：{}；可在项目中解除引用",
            ids.join(", ")
        ));
        for id in ids {
            unresolved_refs.insert(id);
        }
        remaining.clear();
    }
    Ok(SelectedRender {
        artifacts,
        unsupported,
        notes,
        entries: personal_selected_entries,
        unresolved: unresolved_refs,
    })
}

/// 组装个人模式部署准备（不写盘；plan 纯只读）。
pub fn prepare_personal(
    data_root: Option<&Path>,
    explicit_root: Option<&Path>,
    scope_rel: Option<String>,
    data_root_resolved: &Path,
) -> Result<PersonalPrepare> {
    let root = match explicit_root {
        Some(r) => r
            .canonicalize()
            .map_err(|e| Error::new(code::WORKSPACE_INVALID, format!("显式根不可用: {e}")))?,
        None => std::env::current_dir()?,
    };
    // Git 身份优先（F06）：明确非 Git → 路径模式；Git 探测错误 → 显式失败
    let (repo, is_nongit) = match repo_registry::classify_path(&root)? {
        repo_registry::PathClass::Git(d) => (d, false),
        repo_registry::PathClass::NonGit { root } => (nongit_identity(&root), true),
    };
    let ctx = if is_nongit {
        nongit_ctx(data_root, &root)?
    } else {
        AppContext::discover(data_root, &root, explicit_root)?
    };
    let worktree_root = ctx.workspace.workspace_root.clone();
    // 解析登记身份（F05）：搬迁/别名命中时以已登记 repo_id 为准（配置不丢）
    let mut registry = RepoRegistry::resolve_or_create(data_root_resolved, &repo)?;
    if !is_nongit {
        registry.refresh_worktrees(&repo, &crate::ids::now_iso());
        registry.save(data_root_resolved)?;
    } else {
        registry.save(data_root_resolved)?;
    }
    let repo_id = registry.repo_id.clone();
    let wt_canon = repo
        .current_worktree
        .canonicalize()
        .unwrap_or_else(|_| repo.current_worktree.clone());
    let wt_id = if is_nongit {
        "root".to_string()
    } else {
        registry
            .worktrees
            .values()
            .find(|w| w.path == wt_canon)
            .map(|w| w.id.clone())
            .ok_or_else(|| {
                Error::new(
                    code::WORKSPACE_INVALID,
                    "当前 Worktree 未进入登记（内部一致性错误）",
                )
            })?
    };
    // 当前作用域相对路径：显式指定优先；否则 root 相对所在 worktree（仅 Git 模式）
    let active_rel = match scope_rel {
        Some(rel) => Some(rel),
        None => {
            if is_nongit {
                None
            } else {
                let rel = root
                    .strip_prefix(&repo.current_worktree)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if rel.is_empty() || rel == "." {
                    None
                } else {
                    Some(rel)
                }
            }
        }
    };
    // F07：作用域落点。子项目作用域必须命中真实目录，不自动创建业务目录。
    let scope_dir = match &active_rel {
        Some(rel) => validate_scope_dir(
            if is_nongit { None } else { Some(&repo) },
            &worktree_root,
            rel,
        )?,
        None => worktree_root.clone(),
    };

    let mut notes = Vec::new();
    let mut unresolved_refs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let team = if active_rel.is_some() {
        notes.push(
            "子项目作用域：团队层资源部署在仓库根，宿主从祖先目录仍可见；本作用域只管理子项目内的个人能力（不重复部署/删除仓库根条目）"
                .into(),
        );
        None
    } else {
        team_layer(data_root, explicit_root, &mut notes)?
    };

    // 团队层期望 id 集（v1 解析结果，语义不变）
    let team_enabled: Vec<String> = team
        .as_ref()
        .map(|p| {
            p.desired
                .deployable()
                .iter()
                .map(|s| s.id.clone())
                .collect()
        })
        .unwrap_or_default();
    let team_hosts: Vec<String> = team
        .as_ref()
        .map(|p| {
            let mut v = Vec::new();
            if p.declaration.targets.claude {
                v.push("claude".to_string());
            }
            if p.declaration.targets.codex {
                v.push("codex".to_string());
            }
            v.extend(p.declaration.targets.extra.iter().cloned());
            v
        })
        .unwrap_or_default();

    let profile = PersonalProfile::load_or_default(data_root_resolved)?;
    // F06/AIL-057：目录后来初始化 Git 时的显式迁移提示（不默默合并）
    if !is_nongit {
        let old_nongit = nongit_identity(&worktree_root).identity.repo_id;
        if profile.repos.contains_key(&old_nongit) && old_nongit != repo_id {
            notes.push(format!(
                "检测到该目录在非 Git 路径模式下的个人配置（{old_nongit}）；运行 ailoom personal --action migrate-nongit --root <路径> 迁移到 Git 身份（{repo_id}），不自动合并"
            ));
        }
    }
    let effective = resolve_effective(ResolveScopeRequest {
        profile: &profile,
        repo_id: &repo_id,
        worktree_id: &wt_id,
        active_rel: active_rel.as_deref(),
        team_enabled: &team_enabled,
        team_hosts: &team_hosts,
    });

    let disabled_ids: Vec<String> = effective
        .resources
        .iter()
        .filter(|(_, v)| !v.deployed)
        .map(|(k, _)| k.clone())
        .collect();
    // F03：最终启用宿主（个人层三态解释后）。团队层与资源库产物都按它过滤。
    let enabled_hosts: Vec<String> = effective
        .hosts
        .iter()
        .filter(|(_, h)| h.enabled)
        .map(|(k, _)| k.clone())
        .collect();

    let mut artifacts: Vec<crate::adapters::common::Artifact> = Vec::new();

    // 团队层产物：个人模式剔除内置资源与文档索引片段（会写公司 AGENTS.md/CLAUDE.md），
    // 再剔除被个人层显式禁用的资源与被禁用宿主的产物（F03）
    if let Some(p) = &team {
        for a in &p.artifacts {
            if super::sync_core::layer_of(&a.resource_id) == super::sync_core::Layer::TeamOnly {
                continue;
            }
            if disabled_ids.contains(&a.resource_id) {
                continue;
            }
            if !enabled_hosts.iter().any(|h| h == &a.target_tool) {
                continue;
            }
            artifacts.push(a.clone());
        }
    }

    // 资源库与合集层：被启用的 personal/* 与 collection-* 资源（与全局部署共用渲染）
    let selected_ids: Vec<String> = effective
        .resources
        .iter()
        .filter(|(_, v)| v.deployed)
        .map(|(k, _)| k.clone())
        .collect();
    let targets = ToolTargets {
        claude: enabled_hosts.iter().any(|h| h == "claude"),
        codex: enabled_hosts.iter().any(|h| h == "codex"),
        extra: enabled_hosts
            .iter()
            .filter(|h| h.as_str() != "claude" && h.as_str() != "codex")
            .cloned()
            .collect(),
    };
    let rendered = render_selected(
        data_root_resolved,
        &selected_ids,
        &targets,
        &worktree_root,
        &scope_dir,
    )?;
    let personal_selected_entries = rendered.entries;
    let unsupported = rendered.unsupported;
    notes.extend(rendered.notes);
    unresolved_refs.extend(rendered.unresolved);
    // F07：子项目作用域的产物落进子项目目录
    for mut a in rendered.artifacts {
        if let Some(rel) = &active_rel {
            a.path = PathBuf::from(rel).join(&a.path);
        }
        artifacts.push(a);
    }
    // AIL-152：已全局部署的 Skill 不在项目里重复部署（宿主里只出现一份）
    let global_skipped =
        crate::global_skills::skip_globally_deployed(data_root_resolved, &mut artifacts);
    if !global_skipped.is_empty() {
        notes.push(format!(
            "{} 个 Skill 已全局启用，项目内不重复部署：{}",
            global_skipped.len(),
            global_skipped
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("、")
        ));
    }

    // 个人指令条目（AIL-042）：Codex 替代视图按作用域取最近基线（子项目用其目录内基线）
    let entry = pi::load_entry(data_root_resolved, &repo_id, Some(wt_id.as_str()));
    let instr_artifacts = match entry {
        Some(content) => {
            let mut arts = pi::render(&scope_dir, &enabled_hosts, &content)?;
            if let Some(rel) = &active_rel {
                for a in &mut arts {
                    a.path = PathBuf::from(rel).join(&a.path);
                }
            }
            arts
        }
        None => Vec::new(),
    };
    artifacts.extend(instr_artifacts);

    // 公司文件守卫（个人模式全部产物适用）
    let (mut artifacts, mut skipped) = pi::guard_company_files(&worktree_root, artifacts)?;
    if !skipped.is_empty() {
        notes.push(format!(
            "公司文件保护：{} 个目标被跳过（详见 skipped）",
            skipped.len()
        ));
    }

    // 目标键冲突（团队+个人合并后统一检查）
    {
        let mut seen: std::collections::BTreeMap<String, String> = Default::default();
        for a in &artifacts {
            let key = a.item_key();
            match seen.get(&key) {
                Some(prev) if *prev != a.resource_id => {
                    return Err(Error::new(
                        code::RESOURCE_ID_CONFLICT,
                        "个人模式合并后多个资源渲染到同一目标，须显式排除其一",
                    )
                    .context(
                        serde_json::json!({ "target": key, "resources": [prev, a.resource_id] }),
                    ));
                }
                _ => {
                    seen.insert(key, a.resource_id.clone());
                }
            }
        }
    }

    artifacts.sort_by_key(|a| a.item_key());
    let managed_path = ctx.layout.managed_manifest_path.clone();
    let managed =
        ManagedManifest::load_for_workspace(&managed_path, &ctx.workspace.workspace_root)?
            .unwrap_or_else(|| ManagedManifest::new(&ctx.workspace.workspace_id));
    let mut plan = build_plan(&artifacts, &managed, &worktree_root, "personal-mode", None)?;
    for u in &unsupported {
        plan.actions.push(crate::sync::plan::PlanAction {
            action: crate::sync::plan::ActionKind::Unsupported,
            path: String::new(),
            item_key: format!("unsupported/{}", u.resource_id),
            resource_id: u.resource_id.clone(),
            target_tool: u.tool.clone(),
            kind: u.kind.clone(),
            reason: u.reason.clone(),
            precondition_hash: None,
            desired_hash: String::new(),
            manifest_hash: None,
        });
    }

    // S02 + F07：计划后处理。
    // - 公司文件保护覆盖全部清理动作：被跟踪路径不因「从期望集合移除」变成删除目标；
    // - 清理动作按作用域过滤：父子作用域不互相移除对方的托管条目。
    {
        let mut registered_rels: Vec<String> = registry.subprojects.keys().cloned().collect();
        let repo_prof = profile.repo(&repo_id);
        for sp in &repo_prof.subprojects {
            registered_rels.push(sp.path.clone());
        }
        for sps in repo_prof.wt_subprojects.values() {
            for sp in sps {
                registered_rels.push(sp.path.clone());
            }
        }
        registered_rels.sort();
        registered_rels.dedup();
        use super::sync_core::{is_cleanup, layer_of, Layer};
        // 只由团队同步部署的条目（内置资源、文档索引）在团队层仍需要时不清理（C-01）。
        let team_only_keys: std::collections::BTreeSet<String> = team
            .as_ref()
            .map(|p| {
                p.artifacts
                    .iter()
                    .filter(|a| layer_of(&a.resource_id) == Layer::TeamOnly)
                    .map(|a| a.item_key())
                    .collect()
            })
            .unwrap_or_default();
        // 定义无效的资源只是「这次没法渲染」，不是「用户不要了」：已部署的文件保留到修复为止。
        let invalid_ids: std::collections::BTreeSet<&str> = unsupported
            .iter()
            .filter(|u| u.tool == "-" && u.kind != "resource")
            .map(|u| u.resource_id.as_str())
            .collect();
        let mut kept_invalid = 0usize;
        let mut removed = Vec::new();
        plan.actions.retain(|a| {
            if !is_cleanup(a) {
                return true;
            }
            if team_only_keys.contains(&a.item_key) {
                return false;
            }
            if invalid_ids.contains(a.resource_id.as_str()) {
                kept_invalid += 1;
                return false;
            }
            if a.path.is_empty() {
                return true;
            }
            if pi::path_is_git_tracked(&worktree_root, &a.path) {
                skipped.push(pi::SkippedTarget {
                    path: a.path.clone(),
                    reason: "公司已跟踪文件：个人模式不执行清理/删除（保护覆盖计划动作）".into(),
                });
                removed.push(a.path.clone());
                return false;
            }
            if !path_in_scope(&a.path, active_rel.as_deref(), &registered_rels) {
                removed.push(a.path.clone());
                return false;
            }
            true
        });
        if kept_invalid > 0 {
            notes.push(format!(
                "{kept_invalid} 个已部署文件属于定义无效的资源，本次不清理（修复资源后重新同步）"
            ));
        }
        if !removed.is_empty() {
            notes.push(format!(
                "统一计划器：{} 个清理动作超出当前作用域或受公司文件保护，已从计划移除（不跨作用域互删）",
                removed.len()
            ));
        }
    }

    // F09：MCP 引用缺失诊断进入 notes（只报缺失键名，不读取/输出值）
    {
        let deployed_ids: std::collections::BTreeSet<String> = artifacts
            .iter()
            .filter(|a| a.kind == "mcp")
            .map(|a| a.resource_id.clone())
            .collect();
        for e in &personal_selected_entries {
            if e.id.kind != crate::resource::ResourceKind::Mcp {
                continue;
            }
            if !deployed_ids.contains(&e.id.to_string()) {
                continue;
            }
            if let Ok(spec) = crate::adapters::mcp::parse_spec(e) {
                let missing = crate::adapters::mcp::missing_env_refs(&spec);
                if !missing.is_empty() {
                    notes.push(format!(
                        "MCP {} 缺失引用环境变量: {}（仅检查存在性，不输出值；设置成功 ≠ 连接成功）",
                        e.id,
                        missing.join(", ")
                    ));
                }
            }
        }
    }

    Ok(PersonalPrepare {
        ctx,
        repo,
        registry,
        wt_id,
        active_rel,
        scope_dir,
        is_nongit,
        profile,
        effective,
        artifacts,
        plan,
        managed,
        managed_path,
        skipped,
        notes,
        unresolved_references: unresolved_refs.into_iter().collect(),
    })
}

/// 资源库中可部署资源的完整 ID 集合（与 prepare_personal 的解析口径一致，含 MCP 等非 Skill 资源）。
/// 把资源 ID（source/kind/namespace/name）对应回枚举时被隔离的无效条目。
/// 定义坏掉时 namespace 可能读不出来，只按 kind + name 匹配。
fn invalid_library_entry<'a>(
    invalid: &'a [crate::resource::InvalidEntry],
    id: &str,
) -> Option<&'a crate::resource::InvalidEntry> {
    let parts: Vec<&str> = id.split('/').collect();
    let (kind, name) = (parts.get(1)?, parts.get(3)?);
    invalid
        .iter()
        .find(|e| e.kind.as_str() == *kind && e.name == *name)
}

fn personal_library_ids(
    data_root: &Path,
) -> Result<(
    std::collections::BTreeSet<String>,
    Vec<crate::resource::InvalidEntry>,
)> {
    let lib = crate::personal_library::ensure_library(data_root)?;
    let src = LocalSource::new(&lib.path)?;
    let snap = src.resolve()?;
    let manifest = crate::manifest::TeamManifest::load_from(&snap.root)?;
    let (desired, invalid) = resolve_isolating(ResolveRequest {
        snapshot_root: &snap.root,
        manifest: &manifest,
        source: crate::personal_library::LIBRARY_TEAM_ID,
        identity: &src.identity,
        revision: snap.resolved_commit.clone(),
        content_digest: snap.content_digest.clone(),
        active_projects: &[],
        active_roles: &[],
    })?;
    Ok((
        desired.selected.into_iter().map(|s| s.id).collect(),
        invalid,
    ))
}

fn k_starts_personal(k: &str) -> bool {
    k.starts_with(crate::personal_library::LIBRARY_TEAM_ID)
}

fn filter_desired_by_ids(
    desired: &crate::resolver::DesiredSet,
    ids: &[String],
) -> crate::resolver::DesiredSet {
    let mut d = desired.clone();
    d.selected.retain(|s| ids.contains(&s.id));
    d
}

// ---------------------------------------------------------------------------
// 子命令
// ---------------------------------------------------------------------------

/// `personal effective`：解释当前作用域有效配置（每值来源）。
pub fn effective(
    explicit_root: Option<&Path>,
    scope_rel: Option<String>,
    data_root: Option<&Path>,
    data_root_resolved: &Path,
) -> Result<Value> {
    let p = prepare_personal(data_root, explicit_root, scope_rel, data_root_resolved)?;
    let pending = p
        .plan
        .actions
        .iter()
        .filter(|a| !matches!(a.action, crate::sync::plan::ActionKind::Noop))
        .count();
    Ok(json!({
        "repo_id": p.registry.repo_id,
        "repo_root": p.repo.identity.repo_root,
        "worktree": p.repo.current_worktree,
        "worktree_id": p.wt_id,
        "active_rel": p.active_rel,
        "scope_dir": p.scope_dir,
        "is_nongit": p.is_nongit,
        "profile_revision": p.profile.revision,
        "hosts": p.effective.hosts,
        "resources": p.effective.resources,
        "unresolved_references": p.unresolved_references,
        "pending_actions": pending,
        "skipped": p.skipped,
        "notes": p.notes,
        // AIL-152：已全局部署的资源（项目页标注「已全局启用」，项目内不重复部署）
        "global_skills": crate::global_skills::deployed_set(data_root_resolved)
            .into_iter()
            .map(|(id, _)| id)
            .collect::<std::collections::BTreeSet<_>>(),
    }))
}

pub struct SelectArgs {
    pub resource: Option<String>,
    pub host: Option<String>,
    pub state: String,
    pub subproject: Option<String>,
    pub worktree: bool,
    /// F01：显式仓库/Worktree 根（CLI --repo 或 API root）。缺省用进程 cwd，
    /// 不再回退到「profile 里最近登记的仓库」——那会把配置写错仓库。
    pub repo_root: Option<PathBuf>,
    /// F08：并发保护。提供时与磁盘 revision 不一致 → 冲突。
    pub base_revision: Option<u64>,
}

/// `personal select`：写入个人层三态选择（profile，仓外，格式保留）。
pub fn select(args: &SelectArgs, data_root_resolved: &Path) -> Result<Value> {
    let state = TriState::parse_state(&args.state).ok_or_else(|| {
        Error::new(
            code::USAGE,
            format!("state 非法: {}（enable | disable | inherit）", args.state),
        )
    })?;
    if args.resource.is_none() && args.host.is_none() {
        return Err(Error::new(
            code::USAGE,
            "需要 --resource <完整资源ID> 或 --host <宿主名>",
        ));
    }
    // 一次只写一个键；之前同时给出时会静默丢掉 --host
    if args.resource.is_some() && args.host.is_some() {
        return Err(Error::new(
            code::USAGE,
            "--resource 与 --host 不能同时使用：一次只设置一项",
        )
        .fix("分两次执行：先 --host <宿主> --state …，再 --resource <资源ID> --state …"));
    }
    // C-02：宿主名与资源 ID 形状在写盘前校验；非法值会污染 profile 并让之后所有写操作失败。
    // inherit 仍放行任意值——它只删除条目，是清理历史脏数据的恢复路径。
    if state != TriState::Inherit {
        if let Some(host) = &args.host {
            if !matches!(host.as_str(), "claude" | "codex")
                && !crate::adapters::is_extra_target(host)
            {
                return Err(Error::new(
                    code::UNKNOWN_REFERENCE,
                    format!("未知宿主: {host}（支持 claude/codex/grok/pi/opencode/cursor/alva/antigravity）"),
                ));
            }
        }
        if let Some(resource) = &args.resource {
            let parts: Vec<&str> = resource.split('/').collect();
            let kinds = [
                "skill", "rule", "doc", "agent", "mcp", "learning", "env", "hook", "package",
            ];
            if parts.len() != 4 || parts.iter().any(|p| p.is_empty()) || !kinds.contains(&parts[1])
            {
                return Err(Error::new(
                    code::UNKNOWN_REFERENCE,
                    format!("资源 ID 格式无效: {resource}（应为 source/kind/namespace/name）"),
                ));
            }
            if parts[0] == crate::personal_library::LIBRARY_TEAM_ID {
                let (ids, invalid) = personal_library_ids(data_root_resolved)?;
                if !ids.contains(resource) {
                    if let Some(bad) = invalid_library_entry(&invalid, resource) {
                        return Err(Error::new(
                            code::MANIFEST_MISSING_FIELD,
                            format!(
                                "资源库条目无效，不能启用: {}：{}",
                                bad.path, bad.error.message
                            ),
                        )
                        .fix("修复该文件后重试；或在资源库删除这个条目"));
                    }
                    return Err(Error::new(
                        code::UNKNOWN_REFERENCE,
                        format!("资源库中不存在: {resource}（请先导入，或检查资源 ID）"),
                    ));
                }
            }
        }
    }
    // AIL-107：enable/disable 的合集/资源库引用必须真实存在，
    // 否则会写入悬空引用，后续 effective/plan 全部 E3004、项目页无法打开。
    // inherit（清除本层设置）无需校验——它只删除条目，是悬空引用的恢复路径。
    // 团队源资源（source 段不是 collection-）由绑定解析层校验，这里不拦。
    if state != TriState::Inherit {
        if let Some(resource) = &args.resource {
            let is_collection_ref = resource.starts_with("collection-");
            if is_collection_ref {
                // 先按来源前缀确认登记身份：外部来源/目录失联时 catalog 可能枚举失败，
                // 但引用本身合法（plan 阶段再报具体错误），不能在这里把 select 卡死。
                let prefix_ok = crate::collections::load(data_root_resolved)
                    .map(|registry| {
                        registry
                            .sources
                            .keys()
                            .any(|sid| resource.starts_with(&format!("{sid}/")))
                    })
                    .unwrap_or(false);
                let mut listed_ok = false;
                if !prefix_ok {
                    let (local, _) = crate::personal_library::list_tolerant(data_root_resolved);
                    if local.iter().any(|e| &e.id == resource) {
                        listed_ok = true;
                    } else {
                        listed_ok = crate::collections::list(data_root_resolved)
                            .map(|v| {
                                v["sources"]
                                    .as_array()
                                    .map(|sources| {
                                        sources.iter().any(|s| {
                                            s["resources"]
                                                .as_array()
                                                .map(|rs| {
                                                    rs.iter().any(|r| r["id"] == json!(resource))
                                                })
                                                .unwrap_or(false)
                                        })
                                    })
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false);
                    }
                }
                if !prefix_ok && !listed_ok {
                    return Err(Error::new(
                        code::USAGE,
                        format!("引用的资源不存在: {resource}（请先在资源库导入，或检查资源 ID）"),
                    ));
                }
            }
        }
    }
    // F01：目标仓库身份来自显式根或 cwd 的真实发现，绝不用 profile 键序猜
    let anchor = args
        .repo_root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let (repo_id, wt_id) = match repo_registry::classify_path(&anchor)? {
        repo_registry::PathClass::Git(d) => {
            let mut reg = RepoRegistry::resolve_or_create(data_root_resolved, &d)?;
            reg.refresh_worktrees(&d, &crate::ids::now_iso());
            reg.save(data_root_resolved)?;
            let wt_canon = d
                .current_worktree
                .canonicalize()
                .unwrap_or_else(|_| d.current_worktree.clone());
            let wt = reg
                .worktrees
                .values()
                .find(|w| w.path == wt_canon)
                .map(|w| w.id.clone())
                .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "当前 Worktree 未登记"))?;
            (reg.repo_id.clone(), wt)
        }
        repo_registry::PathClass::NonGit { root } => {
            (nongit_identity(&root).identity.repo_id, "root".to_string())
        }
    };
    let scope = if args.worktree {
        if let Some(path) = &args.subproject {
            SelectScope::WorktreeSubproject(wt_id.clone(), path.clone())
        } else {
            SelectScope::Worktree(wt_id.clone())
        }
    } else if let Some(path) = &args.subproject {
        SelectScope::RepoSubproject(path.clone())
    } else {
        SelectScope::RepoDefault
    };
    let key = if let Some(res) = &args.resource {
        SelectKey::Resource(res.clone())
    } else {
        SelectKey::Host(args.host.clone().unwrap_or_default())
    };
    let new_rev = crate::profile::select_scoped_in_place(
        data_root_resolved,
        &repo_id,
        &scope,
        &key,
        state,
        args.base_revision,
    )?;
    // 选择已保存：恢复副本更新失败只作为警告返回，不把成功的写入报成失败。
    let warnings: Vec<String> =
        crate::knowledge::location::checkpoint_project(data_root_resolved, &repo_id)
            .err()
            .map(|e| e.message)
            .into_iter()
            .collect();
    let scope_desc = match &scope {
        SelectScope::RepoDefault => "仓库默认".to_string(),
        SelectScope::RepoSubproject(p) => format!("仓库子项目模板 {p}"),
        SelectScope::Worktree(w) => format!("Worktree {w}"),
        SelectScope::WorktreeSubproject(w, p) => format!("Worktree {w} 子项目 {p}"),
    };
    Ok(json!({
        "repo_id": repo_id,
        "worktree_id": wt_id,
        "scope": scope_desc,
        "resource": args.resource,
        "host": args.host,
        "state": args.state,
        "revision": new_rev,
        "warnings": warnings,
    }))
}

/// AIL-122：列出某 Worktree 下「有单独配置」的目录（来自真实 profile 记录，
/// 不按文件夹存在猜测）。目录选择器据此标注；失联目录 missing=true（可查看、不可应用）。
pub fn project_dirs(
    explicit_root: Option<&Path>,
    _data_root: Option<&Path>,
    data_root_resolved: &Path,
) -> Result<Value> {
    let root = match explicit_root {
        Some(r) => r
            .canonicalize()
            .map_err(|e| Error::new(code::WORKSPACE_INVALID, format!("显式根不可用: {e}")))?,
        None => std::env::current_dir()?,
    };
    let (repo, is_nongit) = match repo_registry::classify_path(&root)? {
        repo_registry::PathClass::Git(d) => (d, false),
        repo_registry::PathClass::NonGit { root } => (nongit_identity(&root), true),
    };
    let registry = RepoRegistry::resolve_or_create(data_root_resolved, &repo)?;
    let repo_id = registry.repo_id.clone();
    let wt_canon = repo
        .current_worktree
        .canonicalize()
        .unwrap_or_else(|_| repo.current_worktree.clone());
    let wt_id = if is_nongit {
        "root".to_string()
    } else {
        registry
            .worktrees
            .values()
            .find(|w| w.path == wt_canon)
            .map(|w| w.id.clone())
            .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "当前 Worktree 未登记"))?
    };
    let profile = crate::profile::PersonalProfile::load_or_default(data_root_resolved)?;
    let rp = profile.repo(&repo_id);
    let mut dirs: Vec<Value> = Vec::new();
    // 项目共享子项目模板（影响所有 Worktree 的对应目录）
    for sp in &rp.subprojects {
        let exists = wt_canon.join(&sp.path).is_dir();
        dirs.push(json!({ "path": sp.path, "layer": "repo", "missing": !exists, "inherit_resources": !sp.selection.independent_resources }));
    }
    // 当前 Worktree 的子目录覆盖
    if let Some(sps) = rp.wt_subprojects.get(&wt_id) {
        for sp in sps {
            let exists = wt_canon.join(&sp.path).is_dir();
            dirs.push(json!({ "path": sp.path, "layer": "worktree", "missing": !exists, "inherit_resources": !sp.selection.independent_resources }));
        }
    }
    dirs.sort_by(|a, b| {
        a["path"]
            .as_str()
            .unwrap_or("")
            .cmp(b["path"].as_str().unwrap_or(""))
    });
    Ok(json!({
        "repo_id": repo_id,
        "worktree_id": wt_id,
        "is_nongit": is_nongit,
        "dirs": dirs,
        "root_inherits": !rp.worktrees.get(&wt_id).map(|s| s.independent_resources).unwrap_or(false),
        "profile_revision": profile.revision,
    }))
}

/// Configure a persistent node, including an empty node that only inherits.
pub fn configure_scope(
    root: &Path,
    relative: Option<&str>,
    worktree: bool,
    inherit_resources: bool,
    revision: Option<u64>,
    data_root: &Path,
) -> Result<Value> {
    let root = root.canonicalize()?;
    let info = project_dirs(Some(&root), Some(data_root), data_root)?;
    let repo_id = info["repo_id"].as_str().unwrap();
    let wt_id = info["worktree_id"].as_str().unwrap();
    if let Some(rel) = relative {
        let discovery = match repo_registry::classify_path(&root)? {
            repo_registry::PathClass::Git(d) => Some(d),
            _ => None,
        };
        let path = validate_scope_dir(discovery.as_ref(), &root, rel)?.canonicalize()?;
        if !path.starts_with(&root) {
            return Err(Error::new(
                code::ILLEGAL_PATH,
                "子目录不能通过符号链接越出项目",
            ));
        }
    }
    let scope = match (worktree, relative) {
        (true, Some(rel)) => SelectScope::WorktreeSubproject(wt_id.into(), rel.into()),
        (true, None) => SelectScope::Worktree(wt_id.into()),
        (false, Some(rel)) => SelectScope::RepoSubproject(rel.into()),
        (false, None) => SelectScope::RepoDefault,
    };
    let revision = crate::profile::select_scoped_in_place(
        data_root,
        repo_id,
        &scope,
        &SelectKey::InheritResources,
        if inherit_resources {
            TriState::Enable
        } else {
            TriState::Disable
        },
        revision,
    )?;
    crate::knowledge::location::checkpoint_project(data_root, repo_id)?;
    Ok(json!({"revision":revision,"inherit_resources":inherit_resources}))
}

/// Project defaults must not accidentally display a worktree's overrides.
pub fn project_effective(root: &Path, data_root: &Path) -> Result<Value> {
    let info = project_dirs(Some(root), Some(data_root), data_root)?;
    let profile = PersonalProfile::load_or_default(data_root)?;
    let mut notes = Vec::new();
    let team = team_layer(Some(data_root), Some(root), &mut notes)?;
    let enabled: Vec<String> = team
        .as_ref()
        .map(|p| {
            p.desired
                .deployable()
                .iter()
                .map(|s| s.id.clone())
                .collect()
        })
        .unwrap_or_default();
    let mut hosts = Vec::new();
    if let Some(p) = &team {
        if p.declaration.targets.claude {
            hosts.push("claude".into());
        }
        if p.declaration.targets.codex {
            hosts.push("codex".into());
        }
        hosts.extend(p.declaration.targets.extra.iter().cloned());
    }
    let effective = resolve_effective(ResolveScopeRequest {
        profile: &profile,
        repo_id: info["repo_id"].as_str().unwrap(),
        worktree_id: "",
        active_rel: None,
        team_enabled: &enabled,
        team_hosts: &hosts,
    });
    Ok(
        json!({"resources":effective.resources,"hosts":effective.hosts,
        "profile_revision":profile.revision,"notes":notes,"pending_actions":0}),
    )
}

/// AIL-057：非 Git 路径模式目录后来初始化 Git 时的显式身份迁移。
/// 个人配置与指令条目从 nongit-<hash> 迁到 Git repo_id；不丢弃、不默默合并：
/// 目标 Git 身份已有配置时拒绝并让用户显式选择。
pub fn migrate_nongit(explicit_root: Option<&Path>, data_root_resolved: &Path) -> Result<Value> {
    let root = match explicit_root {
        Some(r) => r
            .canonicalize()
            .map_err(|e| Error::new(code::WORKSPACE_INVALID, format!("路径不可用: {e}")))?,
        None => std::env::current_dir()?,
    };
    let git = match repo_registry::classify_path(&root)? {
        repo_registry::PathClass::Git(d) => d,
        repo_registry::PathClass::NonGit { .. } => {
            return Err(Error::new(
                code::WORKSPACE_INVALID,
                "该目录仍不是 Git 仓库；无需迁移",
            ))
        }
    };
    let mut reg = RepoRegistry::resolve_or_create(data_root_resolved, &git)?;
    reg.refresh_worktrees(&git, &crate::ids::now_iso());
    reg.save(data_root_resolved)?;
    let git_id = reg.repo_id.clone();
    let nongit_id = nongit_identity(&root).identity.repo_id;
    if nongit_id == git_id {
        return Ok(json!({ "migrated": false, "note": "身份相同，无需迁移" }));
    }
    // 指令条目迁移
    let old_dir = crate::personal_instructions::entry_base(data_root_resolved, &nongit_id);
    let new_dir = crate::personal_instructions::entry_base(data_root_resolved, &git_id);
    let mut moved_instructions = false;
    if old_dir.is_dir() && !new_dir.exists() {
        std::fs::rename(&old_dir, &new_dir)?;
        moved_instructions = true;
    }
    // 个人配置迁移
    let mut profile = PersonalProfile::load_or_default(data_root_resolved)?;
    let had_profile = profile.repos.contains_key(&nongit_id);
    if had_profile {
        if profile.repos.contains_key(&git_id) {
            return Err(Error::new(
                code::TARGET_CONFLICT,
                format!(
                    "Git 身份 {git_id} 已存在个人配置；为避免覆盖，请手工核对 profile.toml 后删除不要的一侧（nongit 键 {nongit_id}）"
                ),
            ));
        }
        let entry = profile.repos.remove(&nongit_id).unwrap();
        profile.repos.insert(git_id.clone(), entry);
        profile.save(data_root_resolved)?;
    }
    Ok(json!({
        "migrated": had_profile || moved_instructions,
        "from": nongit_id,
        "to": git_id,
        "profile_moved": had_profile,
        "instructions_moved": moved_instructions,
        "note": "迁移完成；旧 nongit 登记保留为历史，不再被 personal 流程选中",
    }))
}

/// AIL-067：能力在当前 Worktree 的实际部署版本状态。
/// 列出引用作用域、已部署哈希与库内当前哈希；过期显示待同步，不宣称已生效。
/// AIL-121 契约补充：部署清单必须与查看作用域一致 —— 传入 scope 后按该目录
/// 计算期望产物（web/.claude/skills/…），目录视图的磁盘状态不再漏报。
pub fn deploy_status(
    explicit_root: Option<&Path>,
    scope_rel: Option<String>,
    data_root: Option<&Path>,
    data_root_resolved: &Path,
) -> Result<Value> {
    let p = prepare_personal(data_root, explicit_root, scope_rel, data_root_resolved)?;
    let mut items = Vec::new();
    for a in &p.artifacts {
        let current_desired = a.desired_hash().unwrap_or_default();
        // The manifest records the last write, not what still exists after undo
        // or a manual edit. Report the actual entry at the selected scope.
        let deployed_hash = crate::sync::plan::current_hash_by_key(
            &p.repo.current_worktree,
            &a.item_key(),
            &a.resource_id,
        )?;
        let up_to_date = deployed_hash.as_deref() == Some(current_desired.as_str());
        items.push(json!({
            "resource_id": a.resource_id,
            "tool": a.target_tool,
            "path": a.path,
            "deployed": deployed_hash.is_some(),
            "up_to_date": up_to_date,
            "state": if deployed_hash.is_none() {
                "not-deployed"
            } else if up_to_date {
                "current"
            } else {
                "stale"
            },
        }));
    }
    Ok(json!({
        "repo_id": p.registry.repo_id,
        "worktree_id": p.wt_id,
        "active_rel": p.active_rel,
        "scope_dir": p.scope_dir.display().to_string(),
        "items": items,
        "issues": p.plan.actions.iter().filter(|a| matches!(a.action, crate::sync::plan::ActionKind::Unsupported | crate::sync::plan::ActionKind::Conflict)).collect::<Vec<_>>(),
        "note": "库更新后旧计划自动失效（计划指纹绑定库内容）；部署到其他 Worktree 需分别选择并应用",
    }))
}

/// `personal instructions`：保存/清除个人指令条目。
pub fn instructions(
    file: Option<&Path>,
    clear: bool,
    worktree_scoped: bool,
    data_root_resolved: &Path,
) -> Result<Value> {
    // 先读输入文件：文件不存在时报出路径，且不留下仓库登记等副作用
    let content = match (clear, file) {
        (true, _) => None,
        (false, Some(f)) => Some(std::fs::read_to_string(f).map_err(|e| {
            Error::new(
                code::USAGE,
                format!("读取 --file 失败: {}: {e}", f.display()),
            )
            .fix("确认文件路径；指令文件是普通 Markdown")
        })?),
        (false, None) => {
            return Err(Error::new(code::USAGE, "需要 --file <markdown> 或 --clear"));
        }
    };
    let cwd = std::env::current_dir()?;
    let repo = repo_registry::discover_repo(&cwd)?;
    let mut reg = RepoRegistry::resolve_or_create(data_root_resolved, &repo)?;
    reg.refresh_worktrees(&repo, &crate::ids::now_iso());
    reg.save(data_root_resolved)?;
    let wt_canon = repo
        .current_worktree
        .canonicalize()
        .unwrap_or_else(|_| repo.current_worktree.clone());
    let wt_id = reg
        .worktrees
        .values()
        .find(|w| w.path == wt_canon)
        .map(|w| w.id.clone());
    let repo_id = reg.repo_id.clone();
    let scope_wt: Option<&str> = if worktree_scoped {
        wt_id.as_deref()
    } else {
        None
    };
    if clear {
        pi::clear_entry(data_root_resolved, &repo_id, scope_wt)?;
        return Ok(
            json!({ "cleared": true, "repo_id": repo_id, "worktree_scoped": worktree_scoped }),
        );
    }
    let content = content.unwrap_or_default();
    pi::save_entry(data_root_resolved, &repo_id, scope_wt, &content)?;
    Ok(json!({
        "saved": true,
        "repo_id": repo_id,
        "worktree_scoped": worktree_scoped,
        "note": "已保存到机器数据区；运行 ailoom personal --action sync 应用到当前 Worktree",
    }))
}

/// `personal plan`：输出计划与有效配置解释（无写入）。
pub fn plan(
    explicit_root: Option<&Path>,
    scope_rel: Option<String>,
    data_root: Option<&Path>,
    data_root_resolved: &Path,
) -> Result<Value> {
    let p = prepare_personal(data_root, explicit_root, scope_rel, data_root_resolved)?;
    Ok(json!({
        "summary": p.plan.summary(),
        "actions": p.plan.actions,
        "repo_id": p.registry.repo_id,
        "worktree_id": p.wt_id,
        "active_rel": p.active_rel,
        "scope_dir": p.scope_dir,
        "is_nongit": p.is_nongit,
        "profile_revision": p.profile.revision,
        "effective_enabled": p.effective.enabled_resources(),
        "skipped": p.skipped,
        "notes": p.notes,
        "has_conflicts": p.plan.has_conflicts(),
    }))
}

/// apply 前的公司文件复核（S02 纵深防御）：计划与执行之间跟踪状态可能变化。
fn verify_no_tracked_targets(p: &PersonalPrepare) -> Result<()> {
    for a in &p.plan.actions {
        if a.path.is_empty() {
            continue;
        }
        if pi::path_is_git_tracked(&p.ctx.workspace.workspace_root, &a.path) {
            return Err(Error::new(
                code::TARGET_CONFLICT,
                format!(
                    "公司文件保护：{} 已被 Git 跟踪，拒绝执行计划动作（请重新 plan）",
                    a.path
                ),
            ));
        }
    }
    Ok(())
}

/// 供 jobs.rs 的 apply 任务复用（相同保护与管道）。
pub fn apply_prepared_personal(p: &PersonalPrepare) -> Result<crate::sync::apply::ApplyReport> {
    verify_no_tracked_targets(p)?;
    crate::knowledge::location::checkpoint_project(&p.ctx.data_root, &p.registry.repo_id)?;
    let journal_root = &p.ctx.layout.journal_dir;
    let lock_dir = p.ctx.layout.ws_dir.join("locks");
    let mut managed = p.managed.clone();
    let report = crate::sync::apply::apply(
        &p.plan,
        &p.artifacts,
        &mut managed,
        &p.ctx.workspace.workspace_root,
        &lock_dir,
        journal_root,
        &p.ctx.device,
    )?;
    if report.ok {
        managed.save(&p.managed_path)?;
        if !p.is_nongit {
            // 每次都对账（包括本次一个都没部署）：关闭宿主/资源后旧条目要能移除
            let owner = format!("{}:{}", p.wt_id, p.active_rel.as_deref().unwrap_or(""));
            crate::git_exclude::sync_patterns(
                &p.repo.identity.common_dir,
                &p.ctx.data_root,
                &p.registry.repo_id,
                &owner,
                &pi::exclude_patterns(&p.artifacts),
            )?;
        }
    }
    Ok(report)
}
