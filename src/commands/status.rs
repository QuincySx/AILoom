//! status：展示有效来源与声明/锁/部署一致性（AIL-005）。不含机器秘密。

use crate::appctx::AppContext;
use crate::config::{Binding, ProjectDeclaration};
use crate::error::Result;
use crate::manifest::TeamManifest;
use crate::source::{GitSource, SourcesLock};
use crate::sync::manifest::ManagedManifest;
use crate::workspace::{AILOOM_DIR, DECLARATION_FILE};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct StatusArgs {
    pub root: Option<PathBuf>,
}

pub fn run(args: &StatusArgs, _json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;

    let mut issues: Vec<Value> = Vec::new();
    let decl_path = ctx
        .workspace
        .workspace_root
        .join(AILOOM_DIR)
        .join(DECLARATION_FILE);
    let declaration = ProjectDeclaration::load(&decl_path)?;
    if declaration.is_none() {
        issues.push(json!({
            "code": "E1002",
            "message": "工作区未绑定：缺少 .ailoom/project.toml",
            "fix": "运行 ailoom init 完成绑定",
        }));
    }
    let binding = Binding::load(&ctx.layout.binding_path)?;
    if binding.is_none() && declaration.is_some() {
        issues.push(json!({
            "code": "E1002",
            "message": "本机绑定缺失（可能从其它机器 clone）",
            "fix": "重新运行 ailoom init 以生成本机绑定",
        }));
    }
    let binding_matches = match (&binding, &declaration) {
        (Some(b), Some(d)) => {
            b.declaration.projects == d.projects
                && b.declaration.roles == d.roles
                && b.declaration.targets == d.targets
        }
        _ => false,
    };
    if let (Some(_), Some(_)) = (&binding, &declaration) {
        if !binding_matches {
            issues.push(json!({
                "code": "E1002",
                "message": "本机绑定与声明不一致",
                "fix": "重新运行 ailoom init 同步绑定",
            }));
        }
    }

    // 锁与快照
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = SourcesLock::load(&lock_path)?;
    let mut snapshot_info = json!({ "available": false });
    if let Some(decl) = &declaration {
        let entry = lock
            .as_ref()
            .and_then(|l| l.sources.get(&decl.source.name).cloned());
        match entry {
            None => {
                if declaration.is_some() {
                    issues.push(json!({
                        "code": "E2001",
                        "message": format!("源 `{}` 尚未锁定", decl.source.name),
                        "fix": "运行 ailoom init 完成首次锁定",
                    }));
                }
            }
            Some(entry) => {
                if decl.source.kind == "git" {
                    let git_src = GitSource::new(
                        entry.identity.trim_start_matches("git+"),
                        entry.ref_.as_deref(),
                    )?;
                    let cache_root = ctx.source_cache(&git_src.identity);
                    match git_src.resolve(&cache_root, Some(&entry)) {
                        Ok(snap) => match TeamManifest::load_from(&snap.root) {
                            Ok(m) => {
                                snapshot_info = json!({
                                    "available": true,
                                    "team_id": m.team_id,
                                    "resolved_commit": snap.resolved_commit,
                                    "content_digest": snap.content_digest,
                                });
                            }
                            Err(e) => {
                                snapshot_info = json!({ "available": false });
                                issues.push(json!({ "code": e.code, "message": e.message }));
                            }
                        },
                        Err(e) => {
                            snapshot_info = json!({ "available": false, "reason": e.code });
                            issues.push(
                                json!({ "code": e.code, "message": e.message, "fix": e.fix }),
                            );
                        }
                    }
                } else {
                    // 本地源：直接检查目录
                    let p = decl.source.path.as_deref().unwrap_or_default();
                    let base = if PathBuf::from(p).is_absolute() {
                        PathBuf::from(p)
                    } else {
                        ctx.workspace.workspace_root.join(p)
                    };
                    match crate::source::LocalSource::new(&base).and_then(|l| l.resolve()) {
                        Ok(snap) => match TeamManifest::load_from(&snap.root) {
                            Ok(m) => {
                                snapshot_info = json!({
                                    "available": true,
                                    "team_id": m.team_id,
                                    "mutable": true,
                                    "content_digest": snap.content_digest,
                                });
                            }
                            Err(e) => {
                                snapshot_info = json!({ "available": false });
                                issues.push(json!({ "code": e.code, "message": e.message }));
                            }
                        },
                        Err(e) => {
                            snapshot_info = json!({ "available": false, "reason": e.code });
                            issues.push(json!({ "code": e.code, "message": e.message }));
                        }
                    }
                }
            }
        }
    }

    if let Ok(Some(managed)) = ManagedManifest::load(&ctx.layout.managed_manifest_path) {
        for (key, reason) in
            crate::require::broken_skill_symlinks(&ctx.workspace.workspace_root, &managed)
        {
            issues.push(json!({
                "code": "skill-store-link",
                "message": format!("Skill 链接异常: {key} ({reason})"),
                "fix": "运行 ailoom sync 重建链接",
            }));
        }
    }

    let value = json!({
        "workspace": {
            "root": ctx.workspace.workspace_root.display().to_string(),
            "workspace_id": ctx.workspace.workspace_id,
            "repository_anchor": ctx.workspace.repository_anchor,
            "is_git": ctx.workspace.is_git,
        },
        "declaration": declaration.as_ref().map(|d| json!({
            "path": decl_path.display().to_string(),
            "source": {
                "name": d.source.name,
                "type": d.source.kind,
                "ref": d.source.ref_,
            },
            "projects": d.projects,
            "roles": d.roles,
            "targets": { "claude": d.targets.claude, "codex": d.targets.codex },
            "require": {
                "skills": d.require.skills,
                "agent": d.require.agent,
            },
        })),
        "binding": binding.as_ref().map(|b| json!({
            "path": ctx.layout.binding_path.display().to_string(),
            "device_id": b.device_id,
            "matches_declaration": binding_matches,
        })),
        "lock": {
            "path": lock_path.display().to_string(),
            "sources": lock.as_ref().map(|l| json!({
                // 只输出白名单字段；不含任何凭据
                "names": l.sources.keys().cloned().collect::<Vec<_>>(),
            })),
        },
        "snapshot": snapshot_info,
        "ok": issues.is_empty(),
        "issues": issues,
    });

    if !_json {
        let w = &value["workspace"];
        println!(
            "工作区: {} (id {})",
            w["root"].as_str().unwrap_or("?"),
            w["workspace_id"].as_str().unwrap_or("?")
        );
        if let Some(d) = value.get("declaration").filter(|v| !v.is_null()) {
            println!(
                "源: {} ({}, ref {})  项目 {:?}  角色 {:?}",
                d["source"]["name"].as_str().unwrap_or("?"),
                d["source"]["type"].as_str().unwrap_or("?"),
                d["source"]["ref"].as_str().unwrap_or("<默认>"),
                d["projects"].clone(),
                d["roles"].clone(),
            );
        }
        println!("快照可用: {}", value["snapshot"]["available"]);
        for issue in value["issues"].as_array().unwrap_or(&vec![]) {
            println!(
                "! [{}] {}",
                issue["code"].as_str().unwrap_or("?"),
                issue["message"].as_str().unwrap_or("?")
            );
        }
    }
    Ok(value)
}
