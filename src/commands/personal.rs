//! 个人模式命令（AIL-044 接线；消费 AIL-039/040/042/043 的服务）。
//!
//! 部署期望 = 团队层（若有声明且源就绪；个人模式关闭内置与索引片段）
//! + 个人层有效配置（三态选择解释，profile 驱动）
//! + 个人指令条目（042）。所有个人新增经公司文件守卫；不写 .ailoom/project.toml。

use crate::adapters::{render as render_artifacts, ToolTargets};
use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use crate::personal_instructions as pi;
use crate::profile::{
    resolve_effective, EffectiveConfig, PersonalProfile, ResolveScopeRequest, ScopeSelection,
    TriState,
};
use crate::repo_registry::{self, RepoDiscovery, RepoRegistry};
use crate::resolver::{resolve, ResolveRequest};
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
    /// 当前工作树在登记中的 id
    pub wt_id: String,
    /// 当前作用域相对仓库根的路径（None = 工作树根）
    pub active_rel: Option<String>,
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
}

/// 团队层：有声明且源就绪 → Some(prepared)；无声明 → None；
/// 声明存在但源未锁定 → None + note（个人模式不强制团队 init）。
fn team_layer(
    data_root: Option<&Path>,
    explicit_root: Option<&Path>,
    notes: &mut Vec<String>,
) -> Result<Option<Prepared>> {
    let cwd = std::env::current_dir()?;
    let ws = crate::workspace::discover(&cwd, explicit_root)?;
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
    // Git 身份（AIL-039）：个人模式以 Git 仓库为前提；非 Git 显式报错不当路径模式写
    let repo = repo_registry::discover_repo(&root)?;
    let ctx = AppContext::discover(data_root, &root, explicit_root)?;
    // 登记并刷新工作树
    let mut registry = RepoRegistry::load_or_create(data_root_resolved, &repo)?;
    registry.refresh_worktrees(&repo, &crate::ids::now_iso());
    registry.save(data_root_resolved)?;
    let wt_canon = repo
        .current_worktree
        .canonicalize()
        .unwrap_or_else(|_| repo.current_worktree.clone());
    let wt_id = registry
        .worktrees
        .values()
        .find(|w| w.path == wt_canon)
        .map(|w| w.id.clone())
        .ok_or_else(|| {
            Error::new(
                code::WORKSPACE_INVALID,
                "当前工作树未进入登记（内部一致性错误）",
            )
        })?;
    // 当前作用域相对路径：显式指定优先；否则 root 相对所在 worktree
    let active_rel = match scope_rel {
        Some(rel) => Some(rel),
        None => {
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
    };

    let mut notes = Vec::new();
    let team = team_layer(data_root, explicit_root, &mut notes)?;

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
    let effective = resolve_effective(ResolveScopeRequest {
        profile: &profile,
        repo_id: &repo.identity.repo_id,
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
    let enabled_hosts: Vec<String> = effective
        .hosts
        .iter()
        .filter(|(_, h)| h.enabled)
        .map(|(k, _)| k.clone())
        .collect();

    let mut artifacts: Vec<crate::adapters::common::Artifact> = Vec::new();

    // 团队层产物：个人模式剔除内置资源与文档索引片段（会写公司 AGENTS.md/CLAUDE.md），
    // 再剔除被个人层显式禁用的资源
    if let Some(p) = &team {
        for a in &p.artifacts {
            if a.resource_id.starts_with("ailoom-builtin/")
                || a.resource_id == "ailoom-internal/doc-index"
            {
                continue;
            }
            if disabled_ids.contains(&a.resource_id) {
                continue;
            }
            artifacts.push(a.clone());
        }
    }

    // 个人库层：被启用的 personal/* 资源（AIL-043 库 → AIL-040 选择）
    let personal_enabled: Vec<String> = effective
        .resources
        .iter()
        .filter(|(k, v)| v.deployed && k_starts_personal(k))
        .map(|(k, _)| k.clone())
        .collect();
    let mut unsupported = Vec::new();
    if !personal_enabled.is_empty() {
        let lib = crate::personal_library::ensure_library(data_root_resolved)?;
        let src = LocalSource::new(&lib.path)?;
        let snap = src.resolve()?;
        let manifest = crate::manifest::TeamManifest::load_from(&snap.root)?;
        let desired = resolve(ResolveRequest {
            snapshot_root: &snap.root,
            manifest: &manifest,
            source: crate::personal_library::LIBRARY_TEAM_ID,
            identity: &src.identity,
            revision: snap.resolved_commit.clone(),
            content_digest: snap.content_digest.clone(),
            active_projects: &[],
            active_roles: &[],
        })?;
        let targets = ToolTargets {
            claude: enabled_hosts.iter().any(|h| h == "claude"),
            codex: enabled_hosts.iter().any(|h| h == "codex"),
            extra: enabled_hosts
                .iter()
                .filter(|h| h.as_str() != "claude" && h.as_str() != "codex")
                .cloned()
                .collect(),
        };
        // 过滤出被启用的资源
        let filtered_desired = filter_desired_by_ids(&desired, &personal_enabled);
        let (mut lib_artifacts, lib_unsupported) = render_artifacts(
            &filtered_desired,
            &snap.root,
            &targets,
            &ctx.workspace.workspace_root,
        )?;
        artifacts.append(&mut lib_artifacts);
        unsupported.extend(lib_unsupported);
    }

    // 个人指令条目（AIL-042）
    let entry = pi::load_entry(
        data_root_resolved,
        &repo.identity.repo_id,
        Some(wt_id.as_str()),
    );
    let instr_artifacts = match entry {
        Some(content) => pi::render(&ctx.workspace.workspace_root, &enabled_hosts, &content)?,
        None => Vec::new(),
    };
    artifacts.extend(instr_artifacts);

    // 公司文件守卫（个人模式全部产物适用）
    let (mut artifacts, skipped) =
        pi::guard_company_files(&ctx.workspace.workspace_root, artifacts)?;
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
    let managed = ManagedManifest::load(&managed_path)?
        .unwrap_or_else(|| ManagedManifest::new(&ctx.workspace.workspace_id));
    let mut plan = build_plan(
        &artifacts,
        &managed,
        &ctx.workspace.workspace_root,
        "personal-mode",
        None,
    )?;
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

    Ok(PersonalPrepare {
        ctx,
        repo,
        registry,
        wt_id,
        active_rel,
        profile,
        effective,
        artifacts,
        plan,
        managed,
        managed_path,
        skipped,
        notes,
    })
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
    let actions: Vec<&str> = p
        .plan
        .actions
        .iter()
        .filter(|a| !matches!(a.action, crate::sync::plan::ActionKind::Noop))
        .map(|_| "pending")
        .collect();
    Ok(json!({
        "repo_id": p.repo.identity.repo_id,
        "repo_root": p.repo.identity.repo_root,
        "worktree": p.repo.current_worktree,
        "worktree_id": p.wt_id,
        "active_rel": p.active_rel,
        "hosts": p.effective.hosts,
        "resources": p.effective.resources,
        "pending_actions": actions.len(),
        "skipped": p.skipped,
        "notes": p.notes,
    }))
}

pub struct SelectArgs {
    pub resource: Option<String>,
    pub host: Option<String>,
    pub state: String,
    pub subproject: Option<String>,
    pub worktree: bool,
}

/// `personal select` / `personal host`：写入个人层三态选择（profile，仓外）。
pub fn select(args: &SelectArgs, data_root_resolved: &Path) -> Result<Value> {
    let state = match args.state.as_str() {
        "enable" | "enabled" => TriState::Enable,
        "disable" | "disabled" => TriState::Disable,
        "inherit" => TriState::Inherit,
        other => {
            return Err(Error::new(
                code::USAGE,
                format!("state 非法: {other}（enable | disable | inherit）"),
            ))
        }
    };
    let mut profile = PersonalProfile::load_or_default(data_root_resolved)?;
    // 单仓库 CLI 场景：首次选择时从登记目录取最近登记的仓库；控制台按 repo_id 显式操作
    if profile.repos.is_empty() {
        let discovered = discover_latest_repo_id(data_root_resolved)?;
        profile.repos.entry(discovered).or_default();
    }
    let repo_id = profile.repos.keys().last().cloned().unwrap_or_default();
    let repo_entry = profile.repos.entry(repo_id.clone()).or_default();
    let mut sel = ScopeSelection::default();
    if let Some(res) = &args.resource {
        sel.resources.insert(res.clone(), state);
    }
    if let Some(host) = &args.host {
        sel.hosts.insert(host.clone(), state);
    }
    if args.resource.is_none() && args.host.is_none() {
        return Err(Error::new(
            code::USAGE,
            "需要 --resource <完整资源ID> 或 --host <宿主名>",
        ));
    }
    let scope_desc = if args.worktree {
        // 需要当前工作树 id
        let wt = discover_current_wt_id(data_root_resolved, &repo_id)?;
        if args.subproject.is_some() {
            let path = args.subproject.clone().unwrap();
            let list = repo_entry.wt_subprojects.entry(wt).or_default();
            match list.iter_mut().find(|s| s.path == path) {
                Some(s) => {
                    if let Some(r) = &args.resource {
                        s.selection.resources.insert(r.clone(), state);
                    }
                    if let Some(h) = &args.host {
                        s.selection.hosts.insert(h.clone(), state);
                    }
                }
                None => {
                    list.push(crate::profile::SubprojectSelection {
                        path,
                        selection: sel.clone(),
                    });
                }
            }
            "worktree 子项目".to_string()
        } else {
            repo_entry.worktrees.insert(wt, sel.clone());
            "当前工作树".to_string()
        }
    } else if let Some(path) = &args.subproject {
        let list = &mut repo_entry.subprojects;
        match list.iter_mut().find(|s| &s.path == path) {
            Some(s) => {
                if let Some(r) = &args.resource {
                    s.selection.resources.insert(r.clone(), state);
                }
                if let Some(h) = &args.host {
                    s.selection.hosts.insert(h.clone(), state);
                }
            }
            None => {
                list.push(crate::profile::SubprojectSelection {
                    path: path.clone(),
                    selection: sel.clone(),
                });
            }
        }
        format!("仓库子项目模板 {path}")
    } else {
        repo_entry.default = Some(match repo_entry.default.take() {
            Some(mut d) => {
                if let Some(r) = &args.resource {
                    d.resources.insert(r.clone(), state);
                }
                if let Some(h) = &args.host {
                    d.hosts.insert(h.clone(), state);
                }
                d
            }
            None => sel.clone(),
        });
        "仓库默认".to_string()
    };
    profile.save(data_root_resolved)?;
    Ok(json!({
        "repo_id": repo_id,
        "scope": scope_desc,
        "resource": args.resource,
        "host": args.host,
        "state": args.state,
    }))
}

fn discover_latest_repo_id(data_root: &Path) -> Result<String> {
    let dir = data_root.join("repos");
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for e in std::fs::read_dir(&dir)
        .map_err(|_| Error::new(code::WORKSPACE_INVALID, "没有已登记仓库"))?
        .flatten()
    {
        let reg = e.path().join("registry.json");
        if let Ok(meta) = std::fs::metadata(&reg) {
            if let Ok(m) = meta.modified() {
                let id = e.file_name().to_string_lossy().to_string();
                if best.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
                    best = Some((m, id));
                }
            }
        }
    }
    best.map(|(_, id)| id)
        .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "没有已登记仓库"))
}

fn discover_current_wt_id(data_root: &Path, repo_id: &str) -> Result<String> {
    // 当前 cwd 所在 worktree 的登记 id
    let cwd = std::env::current_dir()?;
    let repo = repo_registry::discover_repo(&cwd)?;
    if repo.identity.repo_id != repo_id {
        // 当前目录与最近登记仓库不同：仍按 repo_id 的登记 + 当前发现刷新
    }
    let mut reg = RepoRegistry::load_or_create(data_root, &repo)?;
    reg.refresh_worktrees(&repo, &crate::ids::now_iso());
    reg.save(data_root)?;
    let wt_canon = repo
        .current_worktree
        .canonicalize()
        .unwrap_or_else(|_| repo.current_worktree.clone());
    reg.worktrees
        .values()
        .find(|w| w.path == wt_canon)
        .map(|w| w.id.clone())
        .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "当前工作树未登记"))
}

/// `personal instructions`：保存/清除个人指令条目。
pub fn instructions(
    file: Option<&Path>,
    clear: bool,
    worktree_scoped: bool,
    data_root_resolved: &Path,
) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let repo = repo_registry::discover_repo(&cwd)?;
    let mut reg = RepoRegistry::load_or_create(data_root_resolved, &repo)?;
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
    let repo_id = repo.identity.repo_id.clone();
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
    let file = file.ok_or_else(|| Error::new(code::USAGE, "需要 --file <markdown> 或 --clear"))?;
    let content = std::fs::read_to_string(file)?;
    pi::save_entry(data_root_resolved, &repo_id, scope_wt, &content)?;
    Ok(json!({
        "saved": true,
        "repo_id": repo_id,
        "worktree_scoped": worktree_scoped,
        "note": "已保存到机器数据区；运行 ailoom personal sync 应用到当前工作树",
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
        "repo_id": p.repo.identity.repo_id,
        "worktree_id": p.wt_id,
        "active_rel": p.active_rel,
        "effective_enabled": p.effective.enabled_resources(),
        "skipped": p.skipped,
        "notes": p.notes,
        "has_conflicts": p.plan.has_conflicts(),
    }))
}

/// `personal sync`：应用个人模式部署（复用 sync 的 apply/journal/lock）。
pub fn sync(
    explicit_root: Option<&Path>,
    scope_rel: Option<String>,
    data_root: Option<&Path>,
    data_root_resolved: &Path,
) -> Result<Value> {
    let p = prepare_personal(data_root, explicit_root, scope_rel, data_root_resolved)?;
    let _run_id = format!("personal-{}", crate::ids::new_id());
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
        // exclude 登记：仅个人新增产物路径（引用计数，多工作树共享 common exclude）
        let patterns = pi::exclude_patterns(&p.artifacts);
        if !patterns.is_empty() {
            crate::git_exclude::add_patterns(
                &p.repo.identity.common_dir,
                data_root_resolved,
                &p.repo.identity.repo_id,
                &patterns,
            )?;
        }
    }
    Ok(json!({
        "ok": report.ok,
        "applied": report.applied,
        "noop": report.noop,
        "skipped_conflicts": report.skipped_conflicts,
        "failed": report.failed,
        "pending_journal": report.pending_journal,
        "skipped_company_files": p.skipped,
        "notes": p.notes,
        "effective_enabled": p.effective.enabled_resources(),
    }))
}
