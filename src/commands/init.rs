//! init：绑定团队源、工作区、项目与角色（AIL-005）。
//! 重复 init 幂等；显式参数覆盖；manifest 引用校验；锁离线安全。

use crate::appctx::AppContext;
use crate::config::{ProjectDeclaration, SourceDeclaration, TargetsDeclaration};
use crate::error::{code, Error, Result};
use crate::ids::now_iso;
use crate::manifest::TeamManifest;
use crate::source::{GitSource, LocalSource, Snapshot, SourceLock, SourcesLock};
use crate::workspace::{AILOOM_DIR, DECLARATION_FILE};
use std::path::PathBuf;

pub struct InitArgs {
    /// 关闭内置资源部署
    pub no_builtin: bool,
    pub url: Option<String>,
    pub ref_: Option<String>,
    pub name: Option<String>,
    pub local_path: Option<String>,
    /// 出现即整体替换（"重复绑定只改显式参数"）
    pub projects: Vec<String>,
    pub roles: Vec<String>,
    pub targets: Vec<String>,
    pub refresh: bool,
    pub root: Option<PathBuf>,
}

/// 解析后的源信息与是否需要更新锁。
struct Resolved {
    snapshot: Snapshot,
    lock_entry: SourceLock,
    unchanged: bool,
    old_locked_at: Option<String>,
}

pub fn run(
    args: &InitArgs,
    json: bool,
    data_root: Option<&std::path::Path>,
) -> Result<serde_json::Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;

    // 1. 载入既有声明（无则默认骨架），仅显式参数覆盖
    let decl_path = ctx.workspace.declaration_path.clone().unwrap_or_else(|| {
        ctx.workspace
            .workspace_root
            .join(AILOOM_DIR)
            .join(DECLARATION_FILE)
    });
    let existing = ProjectDeclaration::load(&decl_path)?;
    let mut decl = existing.clone().unwrap_or(ProjectDeclaration {
        schema_version: 1,
        source: SourceDeclaration {
            name: "team".into(),
            kind: "git".into(),
            url: None,
            ref_: None,
            path: None,
        },
        projects: vec![],
        roles: vec![],
        targets: TargetsDeclaration::default(),
        builtins: true,
        reporting_enabled: false,
        extra_sources: vec![],
        require: Default::default(),
    });

    if let Some(url) = &args.url {
        decl.source.kind = "git".into();
        decl.source.url = Some(url.clone());
        decl.source.path = None;
        if args.ref_.is_none() && decl.source.ref_.is_none() {
            decl.source.ref_ = Some("main".into());
        }
    }
    if let Some(local) = &args.local_path {
        decl.source.kind = "local".into();
        decl.source.path = Some(local.clone());
        decl.source.url = None;
        decl.source.ref_ = None;
    }
    if let Some(r) = &args.ref_ {
        decl.source.ref_ = Some(r.clone());
    }
    if let Some(name) = &args.name {
        decl.source.name = name.clone();
    }
    if !args.projects.is_empty() {
        decl.projects = args.projects.clone();
    }
    if !args.roles.is_empty() {
        decl.roles = args.roles.clone();
    }
    if args.no_builtin {
        decl.builtins = false;
    }
    if !args.targets.is_empty() {
        let mut t = TargetsDeclaration {
            claude: false,
            codex: false,
            extra: vec![],
        };
        for target in &args.targets {
            match target.as_str() {
                "claude" => t.claude = true,
                "codex" => t.codex = true,
                other => {
                    if crate::adapters::is_extra_target(other) {
                        t.extra.push(other.to_string());
                        if let Some(spec) = crate::adapters::registry::lookup(other) {
                            if !spec.verified {
                                eprintln!(
                                    "[ailoom] 警告：宿主 `{other}` 发现路径尚未官方核实（file-placed）；真实加载需自行验收"
                                );
                            }
                        }
                    } else {
                        // 与 personal select 的未知宿主同一错误码
                        return Err(Error::new(
                            code::UNKNOWN_REFERENCE,
                            format!(
                                "未知 target: {other}（支持 claude/codex/grok/pi/opencode/cursor/alva/antigravity）"
                            ),
                        ));
                    }
                }
            }
        }
        decl.targets = t;
    }
    if existing.is_none() && decl.source.kind == "git" && decl.source.url.is_none() {
        return Err(Error::new(
            code::USAGE,
            "首次 init 需要 --url <团队源 Git URL> 或 --local-path <目录>",
        )
        .fix("示例：ailoom init --url git@github.com:team/resources.git --project a --role dev"));
    }
    decl.validate()?;

    // 2. 解析源 + 校验清单引用
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let mut lock = SourcesLock::load(&lock_path)?.unwrap_or_default();
    let old_entry = lock.sources.get(&decl.source.name).cloned();

    let resolved = match decl.source.kind.as_str() {
        "git" => {
            let url = decl.source.url.clone().unwrap_or_default();
            let git_src = GitSource::new(&url, decl.source.ref_.as_deref())?;
            let cache_root = ctx.source_cache(&git_src.identity);
            let snapshot = if args.refresh || old_entry.is_none() {
                git_src.refresh(&cache_root)?
            } else {
                git_src.resolve(&cache_root, old_entry.as_ref())?
            };
            validate_refs(&snapshot, &decl)?;
            let unchanged = old_entry
                .as_ref()
                .map(|o| {
                    o.identity == git_src.identity
                        && o.resolved_commit == snapshot.resolved_commit
                        && o.content_digest == snapshot.content_digest
                })
                .unwrap_or(false);
            Resolved {
                lock_entry: SourceLock {
                    kind: "git".into(),
                    identity: git_src.identity.clone(),
                    ref_: decl.source.ref_.clone(),
                    resolved_commit: snapshot.resolved_commit.clone(),
                    content_digest: snapshot.content_digest.clone(),
                    locked_at: String::new(),
                },
                snapshot,
                unchanged,
                old_locked_at: old_entry.as_ref().map(|o| o.locked_at.clone()),
            }
        }
        "local" | "self" => {
            let path = decl.source.path.clone().unwrap_or_default();
            let base = if PathBuf::from(&path).is_absolute() {
                PathBuf::from(&path)
            } else {
                ctx.workspace.workspace_root.join(&path)
            };
            let local = LocalSource::new(&base)?;
            let snapshot = local.resolve()?;
            validate_refs(&snapshot, &decl)?;
            let unchanged = old_entry
                .as_ref()
                .map(|o| o.content_digest == snapshot.content_digest)
                .unwrap_or(false);
            Resolved {
                lock_entry: SourceLock {
                    kind: "local".into(),
                    identity: local.identity.clone(),
                    ref_: None,
                    resolved_commit: None,
                    content_digest: snapshot.content_digest.clone(),
                    locked_at: String::new(),
                },
                snapshot,
                unchanged,
                old_locked_at: old_entry.as_ref().map(|o| o.locked_at.clone()),
            }
        }
        other => {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("未知 source.type: {other}"),
            ))
        }
    };

    // 额外订阅源：首次 init 或 --refresh 时获取并锁定（独立锁条目）
    let mut extra_lock_changed = false;
    for es in &decl.extra_sources {
        let es_entry = lock.sources.get(&es.name).cloned();
        let snapshot = if es.kind == "git" {
            let src = GitSource::new(es.url.as_deref().unwrap_or_default(), es.ref_.as_deref())?;
            let cache_root = ctx.source_cache(&src.identity);
            if args.refresh || es_entry.is_none() {
                src.refresh(&cache_root)?
            } else {
                src.resolve(&cache_root, es_entry.as_ref())?
            }
        } else {
            let p = es.path.clone().unwrap_or_default();
            let base = if PathBuf::from(&p).is_absolute() {
                PathBuf::from(&p)
            } else {
                ctx.workspace.workspace_root.join(&p)
            };
            crate::source::LocalSource::new(&base)?.resolve()?
        };
        lock.sources.insert(
            es.name.clone(),
            crate::source::SourceLock {
                kind: es.kind.clone(),
                identity: snapshot.identity.clone(),
                ref_: es.ref_.clone(),
                resolved_commit: snapshot.resolved_commit.clone(),
                content_digest: snapshot.content_digest.clone(),
                locked_at: now_iso(),
            },
        );
        extra_lock_changed = true;
    }

    let mut lock_entry = resolved.lock_entry;
    lock_entry.locked_at = if resolved.unchanged {
        resolved.old_locked_at.unwrap_or_else(now_iso)
    } else {
        now_iso()
    };
    let lock_changed = !resolved.unchanged || extra_lock_changed;
    lock.sources.insert(decl.source.name.clone(), lock_entry);

    // 3. 幂等落盘：声明仅变化时写入
    let new_text = toml::to_string_pretty(&decl)?;
    let declaration_changed = existing
        .as_ref()
        .map(|e| {
            toml::to_string_pretty(e)
                .map(|t| t != new_text)
                .unwrap_or(true)
        })
        .unwrap_or(true);
    if declaration_changed {
        crate::sync_common::atomic_write(&decl_path, new_text.as_bytes())?;
    }

    // 4. 绑定（机器数据，不提交）
    let binding = crate::config::Binding {
        schema_version: 1,
        workspace_root: ctx.workspace.workspace_root.display().to_string(),
        repository_anchor: ctx.workspace.repository_anchor.clone(),
        workspace_id: ctx.workspace.workspace_id.clone(),
        device_id: ctx.device.clone(),
        declaration: decl.clone(),
        created_at: crate::config::Binding::load(&ctx.layout.binding_path)?
            .map(|b| b.created_at)
            .unwrap_or_else(now_iso),
        updated_at: now_iso(),
    };
    binding.save(&ctx.layout.binding_path)?;

    // 5. 锁文件（workspace 内 machine 目录，自 gitignore）
    if lock_changed || !lock_path.exists() {
        let machine_dir = lock_path.parent().unwrap();
        std::fs::create_dir_all(machine_dir)?;
        let gi = machine_dir.join(".gitignore");
        if !gi.exists() {
            std::fs::write(&gi, "*\n")?;
        }
        lock.save(&lock_path)?;
    }

    let value = serde_json::json!({
        "workspace_root": ctx.workspace.workspace_root.display().to_string(),
        "workspace_id": ctx.workspace.workspace_id,
        "declaration_path": decl_path.display().to_string(),
        "declaration_changed": declaration_changed,
        "binding_path": ctx.layout.binding_path.display().to_string(),
        "lock_path": lock_path.display().to_string(),
        "lock_changed": lock_changed,
        "source": {
            "identity": resolved.snapshot.identity,
            "resolved_commit": resolved.snapshot.resolved_commit,
            "content_digest": resolved.snapshot.content_digest,
            "snapshot_root": resolved.snapshot.root.display().to_string(),
            "mutable": resolved.snapshot.mutable,
        },
        "projects": decl.projects,
        "roles": decl.roles,
        "targets": { "claude": decl.targets.claude, "codex": decl.targets.codex },
    });

    if !json {
        println!(
            "已绑定工作区 {}（项目 {}，角色 {}）；声明{}、锁{}",
            ctx.workspace.workspace_root.display(),
            // 零项目合法（只取 shared 资源），显示为「无」而不是空白
            if decl.projects.is_empty() {
                "无（仅共享资源）".to_string()
            } else {
                decl.projects.join(", ")
            },
            if decl.roles.is_empty() {
                "无".to_string()
            } else {
                decl.roles.join(", ")
            },
            if declaration_changed {
                "已写入"
            } else {
                "无变化"
            },
            if lock_changed {
                "已更新"
            } else {
                "无变化"
            },
        );
    }
    Ok(value)
}

fn validate_refs(snapshot: &Snapshot, decl: &ProjectDeclaration) -> Result<()> {
    let manifest = TeamManifest::load_from(&snapshot.root)?;
    for p in &decl.projects {
        manifest.require_project(p)?;
    }
    for r in &decl.roles {
        manifest.require_role(r)?;
    }
    Ok(())
}
