//! plan/sync 共用流程（AIL-009—012 接线）：绑定 → 快照 → 解析 → 渲染 → 计划。

use crate::adapters::{render, ToolTargets, UnsupportedItem};
use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use crate::manifest::TeamManifest;
use crate::resolver::{resolve, DesiredSet, ResolveRequest};
use crate::source::{GitSource, LocalSource, SourcesLock};
use crate::sync::manifest::ManagedManifest;
use crate::sync::plan::{build_plan, ActionKind, PlanAction, SyncPlan};
use std::path::PathBuf;

pub struct Prepared {
    pub ctx: AppContext,
    pub declaration: ProjectDeclaration,
    pub desired: DesiredSet,
    pub snapshot_root: PathBuf,
    pub artifacts: Vec<crate::adapters::common::Artifact>,
    pub unsupported: Vec<UnsupportedItem>,
    pub plan: SyncPlan,
    pub managed: ManagedManifest,
    pub managed_path: PathBuf,
}

pub fn prepare(
    data_root: Option<&std::path::Path>,
    explicit_root: Option<&std::path::Path>,
) -> Result<Prepared> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, explicit_root)?;
    let decl_path = ctx.workspace.declaration_path.clone().ok_or_else(|| {
        Error::new(
            code::WORKSPACE_INVALID,
            "工作区未绑定：缺少 .ailoom/project.toml",
        )
        .fix("先运行 ailoom init")
    })?;
    let declaration = ProjectDeclaration::load(&decl_path)?.ok_or_else(|| {
        Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
    })?;

    // 源锁 → 快照（离线安全）
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(crate::workspace::AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = SourcesLock::load(&lock_path)?;
    // self 模式：源即本 checkout 子树，无需外部锁；以业务 HEAD 作可追溯标记
    let entry = if declaration.source.kind == "self" {
        let head = crate::gitx::git_optional(&ctx.workspace.workspace_root, &["rev-parse", "HEAD"])
            .unwrap_or_default();
        crate::source::SourceLock {
            kind: "self".into(),
            identity: format!("self+{}", ctx.workspace.workspace_id),
            ref_: None,
            resolved_commit: (!head.is_empty()).then_some(head),
            content_digest: String::new(),
            locked_at: String::new(),
        }
    } else {
        lock.as_ref()
            .and_then(|l| l.sources.get(&declaration.source.name).cloned())
            .ok_or_else(|| {
                Error::new(
                    code::SOURCE_NOT_CACHED,
                    format!("源 `{}` 未锁定", declaration.source.name),
                )
                .fix("运行 ailoom init 完成首次锁定")
            })?
    };

    let (snapshot, identity) = match declaration.source.kind.as_str() {
        "git" => {
            let url = declaration.source.url.clone().unwrap_or_default();
            let src = GitSource::new(&url, declaration.source.ref_.as_deref())?;
            let cache = ctx.source_cache(&src.identity);
            let snap = src.resolve(&cache, Some(&entry))?;
            (snap, src.identity.clone())
        }
        "local" | "self" => {
            // self 模式（AIL-036）：团队资源保存在业务仓库子树（默认 .ailoom-team/），机器数据外置
            let p = if declaration.source.kind == "self"
                && declaration
                    .source
                    .path
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty()
            {
                ".ailoom-team".to_string()
            } else {
                declaration.source.path.clone().unwrap_or_default()
            };
            let base = if PathBuf::from(&p).is_absolute() {
                PathBuf::from(&p)
            } else {
                ctx.workspace.workspace_root.join(&p)
            };
            let src = LocalSource::new(&base)?;
            let snap = src.resolve()?;
            (snap, src.identity.clone())
        }
        other => {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("未知 source.type: {other}"),
            ))
        }
    };

    let manifest = TeamManifest::load_from(&snapshot.root)?;
    let desired: DesiredSet = resolve(ResolveRequest {
        snapshot_root: &snapshot.root,
        manifest: &manifest,
        source: &declaration.source.name,
        identity: &identity,
        revision: snapshot.resolved_commit.clone(),
        content_digest: snapshot.content_digest.clone(),
        active_projects: &declaration.projects,
        active_roles: &declaration.roles,
    })?;

    // 额外订阅源（AIL-025）：独立锁、独立范围；tags 过滤 + exclude 排除；跨源目标冲突在合并后检测
    let mut extra: Vec<(DesiredSet, PathBuf)> = Vec::new();
    for es in &declaration.extra_sources {
        let entry = lock
            .as_ref()
            .and_then(|l| l.sources.get(&es.name).cloned())
            .ok_or_else(|| {
                Error::new(
                    code::SOURCE_NOT_CACHED,
                    format!("额外源 `{}` 未锁定", es.name),
                )
                .fix("运行 ailoom init 完成首次锁定")
            })?;
        let (snapshot, identity) = if es.kind == "git" {
            let src = GitSource::new(es.url.as_deref().unwrap_or_default(), es.ref_.as_deref())?;
            let snap = src.resolve(&ctx.source_cache(&src.identity), Some(&entry))?;
            (snap, src.identity.clone())
        } else {
            let p = es.path.clone().unwrap_or_default();
            let base = if PathBuf::from(&p).is_absolute() {
                PathBuf::from(&p)
            } else {
                ctx.workspace.workspace_root.join(&p)
            };
            let src = LocalSource::new(&base)?;
            (src.resolve()?, src.identity.clone())
        };
        let m = TeamManifest::load_from(&snapshot.root)?;
        let d = resolve(ResolveRequest {
            snapshot_root: &snapshot.root,
            manifest: &m,
            source: &es.name,
            identity: &identity,
            revision: snapshot.resolved_commit.clone(),
            content_digest: snapshot.content_digest.clone(),
            active_projects: &es.projects,
            active_roles: &es.roles,
        })?;
        let root = snapshot.root.clone();
        extra.push((apply_subscription_filter(d, &es.tags, &es.exclude), root));
    }

    let require_keys = crate::require::effective_require_keys(
        &declaration.require,
        &ctx.workspace.workspace_root,
    )?;
    let mut desired = desired;
    if let Some(keys) = &require_keys {
        let mut refs: Vec<&DesiredSet> = vec![&desired];
        refs.extend(extra.iter().map(|(d, _)| d));
        crate::require::assert_require_resolvable(&refs, keys)?;
        desired = crate::require::filter_skills_by_require(desired, keys);
        extra = extra
            .into_iter()
            .map(|(d, root)| (crate::require::filter_skills_by_require(d, keys), root))
            .collect();
    }

    let targets = ToolTargets {
        claude: declaration.targets.claude,
        codex: declaration.targets.codex,
        extra: declaration.targets.extra.clone(),
    };
    let (mut artifacts, mut unsupported) = render(
        &desired,
        &snapshot.root,
        &targets,
        &ctx.workspace.workspace_root,
    )?;
    // 额外源渲染（各自快照与过滤后的 desired）
    for (ed, root) in &extra {
        let (a, u) = render(ed, root, &targets, &ctx.workspace.workspace_root)?;
        artifacts.extend(a);
        unsupported.extend(u);
    }

    // 内置资源（召回 Agent / 总结 Skill），声明可关闭
    if declaration.builtins {
        artifacts.extend(crate::adapters::builtin::render(
            &crate::adapters::ToolTargets {
                claude: declaration.targets.claude,
                codex: declaration.targets.codex,
                extra: declaration.targets.extra.clone(),
            },
            true,
        ));
    }

    // Codex skills.config 汇总产物
    let mut all_codex_skills: Vec<String> = desired
        .deployable()
        .iter()
        .filter(|s| s.entry.id.kind == crate::resource::ResourceKind::Skill)
        .filter(|s| resource_allows_tool(s.entry.raw.as_deref(), "codex"))
        .map(|s| s.entry.id.name.clone())
        .collect();
    for (ed, _) in &extra {
        all_codex_skills.extend(
            ed.deployable()
                .iter()
                .filter(|s| s.entry.id.kind == crate::resource::ResourceKind::Skill)
                .filter(|s| resource_allows_tool(s.entry.raw.as_deref(), "codex"))
                .map(|s| s.entry.id.name.clone()),
        );
    }
    all_codex_skills.sort();
    all_codex_skills.dedup();
    let codex_skill_names = all_codex_skills;
    if targets.codex {
        crate::adapters::skills::render_config(
            &codex_skill_names,
            &ctx.workspace.workspace_root,
            &mut artifacts,
        )?;
    }

    artifacts.sort_by_key(|a| a.item_key());
    // 跨源目标冲突：同 item_key 不同 resource_id → E3006（不最后写入者胜）
    {
        let mut seen: std::collections::BTreeMap<String, String> = Default::default();
        for a in &artifacts {
            let key = a.item_key();
            match seen.get(&key) {
                Some(prev) if *prev != a.resource_id => {
                    return Err(Error::new(
                        code::RESOURCE_ID_CONFLICT,
                        "跨源资源渲染到同一目标，须显式排除其一",
                    )
                    .context(
                        serde_json::json!({ "target": key, "resources": [prev, a.resource_id] }),
                    ));
                }
                Some(_) => {}
                None => {
                    seen.insert(key, a.resource_id.clone());
                }
            }
        }
    }

    let managed_path = ctx.layout.managed_manifest_path.clone();
    let managed = ManagedManifest::load(&managed_path)?
        .unwrap_or_else(|| ManagedManifest::new(&ctx.workspace.workspace_id));
    let mut plan = build_plan(
        &artifacts,
        &managed,
        &ctx.workspace.workspace_root,
        &identity,
        snapshot.resolved_commit.clone(),
    )?;

    // Unsupported 显式进入计划
    for u in &unsupported {
        plan.actions.push(PlanAction {
            action: ActionKind::Unsupported,
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

    Ok(Prepared {
        ctx,
        declaration,
        desired,
        snapshot_root: snapshot.root,
        artifacts,
        unsupported,
        plan,
        managed,
        managed_path,
    })
}

fn resource_allows_tool(raw: Option<&str>, tool: &str) -> bool {
    match crate::adapters::common::resource_targets(raw) {
        None => true,
        Some(list) => list.iter().any(|t| t == tool),
    }
}

/// 订阅过滤：tags 相交才选择；exclude 按资源名排除；learning 的项目/共享语义不受 tags 影响。
fn apply_subscription_filter(mut d: DesiredSet, tags: &[String], exclude: &[String]) -> DesiredSet {
    if tags.is_empty() && exclude.is_empty() {
        return d;
    }
    let mut kept = Vec::new();
    for s in d.selected.drain(..) {
        let tag_ok = tags.is_empty() || tags.iter().any(|t| s.entry.meta.tags.contains(t));
        let excluded = exclude.contains(&s.entry.id.name);
        if tag_ok && !excluded {
            kept.push(s);
        } else {
            let reason = if excluded {
                let hit = exclude
                    .iter()
                    .find(|x| **x == s.entry.id.name)
                    .cloned()
                    .unwrap_or_default();
                format!("订阅排除：{hit}（source={}）", d.source)
            } else {
                "订阅 tags 不相交".into()
            };
            d.excluded.push(crate::resolver::Excluded {
                id: s.id.clone(),
                reason,
            });
        }
    }
    d.selected = kept;
    d
}
