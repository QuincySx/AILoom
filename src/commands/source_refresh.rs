//! 推进源锁到声明 ref 最新（供 init --refresh 与 sync --refresh 共用）。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use crate::ids::now_iso;
use crate::source::{GitSource, LocalSource, SourceLock, SourcesLock};
use crate::workspace::AILOOM_DIR;
use std::path::PathBuf;

/// 刷新主源与额外源的锁；返回是否发生锁变更。
pub fn refresh_source_locks(ctx: &AppContext, decl: &ProjectDeclaration) -> Result<bool> {
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let mut lock = SourcesLock::load(&lock_path)?.unwrap_or_default();
    let old_entry = lock.sources.get(&decl.source.name).cloned();
    let mut changed = false;

    match decl.source.kind.as_str() {
        "git" => {
            let url = decl.source.url.clone().unwrap_or_default();
            let git_src = GitSource::new(&url, decl.source.ref_.as_deref())?;
            let cache_root = ctx.source_cache(&git_src.identity);
            let snapshot = git_src.refresh(&cache_root)?;
            let unchanged = old_entry
                .as_ref()
                .map(|o| {
                    o.identity == git_src.identity
                        && o.resolved_commit == snapshot.resolved_commit
                        && o.content_digest == snapshot.content_digest
                })
                .unwrap_or(false);
            if !unchanged {
                changed = true;
            }
            lock.sources.insert(
                decl.source.name.clone(),
                SourceLock {
                    kind: "git".into(),
                    identity: git_src.identity.clone(),
                    ref_: decl.source.ref_.clone(),
                    resolved_commit: snapshot.resolved_commit.clone(),
                    content_digest: snapshot.content_digest.clone(),
                    locked_at: if unchanged {
                        old_entry
                            .as_ref()
                            .map(|o| o.locked_at.clone())
                            .unwrap_or_else(now_iso)
                    } else {
                        now_iso()
                    },
                },
            );
        }
        "local" | "self" => {
            let path = decl.source.path.clone().unwrap_or_default();
            let path = if decl.source.kind == "self" && path.is_empty() {
                ".ailoom-team".to_string()
            } else {
                path
            };
            let base = if PathBuf::from(&path).is_absolute() {
                PathBuf::from(&path)
            } else {
                ctx.workspace.workspace_root.join(&path)
            };
            let local = LocalSource::new(&base)?;
            let snapshot = local.resolve()?;
            let unchanged = old_entry
                .as_ref()
                .map(|o| o.content_digest == snapshot.content_digest)
                .unwrap_or(false);
            if !unchanged {
                changed = true;
            }
            lock.sources.insert(
                decl.source.name.clone(),
                SourceLock {
                    kind: decl.source.kind.clone(),
                    identity: local.identity.clone(),
                    ref_: None,
                    resolved_commit: None,
                    content_digest: snapshot.content_digest.clone(),
                    locked_at: if unchanged {
                        old_entry
                            .as_ref()
                            .map(|o| o.locked_at.clone())
                            .unwrap_or_else(now_iso)
                    } else {
                        now_iso()
                    },
                },
            );
        }
        other => {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("未知 source.type: {other}"),
            ));
        }
    }

    for es in &decl.extra_sources {
        let es_entry = lock.sources.get(&es.name).cloned();
        let snapshot = if es.kind == "git" {
            let src = GitSource::new(es.url.as_deref().unwrap_or_default(), es.ref_.as_deref())?;
            let cache_root = ctx.source_cache(&src.identity);
            src.refresh(&cache_root)?
        } else {
            let p = es.path.clone().unwrap_or_default();
            let base = if PathBuf::from(&p).is_absolute() {
                PathBuf::from(&p)
            } else {
                ctx.workspace.workspace_root.join(&p)
            };
            LocalSource::new(&base)?.resolve()?
        };
        let unchanged = es_entry
            .as_ref()
            .map(|o| {
                o.content_digest == snapshot.content_digest
                    && o.resolved_commit == snapshot.resolved_commit
            })
            .unwrap_or(false);
        if !unchanged {
            changed = true;
        }
        lock.sources.insert(
            es.name.clone(),
            SourceLock {
                kind: es.kind.clone(),
                identity: snapshot.identity.clone(),
                ref_: es.ref_.clone(),
                resolved_commit: snapshot.resolved_commit.clone(),
                content_digest: snapshot.content_digest.clone(),
                locked_at: now_iso(),
            },
        );
    }

    if changed || !lock_path.exists() {
        let machine_dir = lock_path.parent().unwrap();
        std::fs::create_dir_all(machine_dir)?;
        let gi = machine_dir.join(".gitignore");
        if !gi.exists() {
            std::fs::write(&gi, "*\n")?;
        }
        lock.save(&lock_path)?;
    }
    Ok(changed)
}
