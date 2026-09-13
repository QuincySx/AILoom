//! doctor（AIL-013）：只读体检，每条问题给错误码、位置、原因和修复命令；不改任何状态。

use crate::appctx::AppContext;
use crate::config::{Binding, ProjectDeclaration};
use crate::error::Result;
use crate::manifest::TeamManifest;
use crate::source::{GitSource, SourcesLock};
use crate::sync::manifest::ManagedManifest;
use crate::sync::plan::{current_hash_by_key, ActionKind};
use crate::workspace::{AILOOM_DIR, DECLARATION_FILE};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

pub struct DoctorArgs {
    pub root: Option<PathBuf>,
}

pub fn run(args: &DoctorArgs, _json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let mut checks: Vec<Value> = Vec::new();
    fn push(checks: &mut Vec<Value>, name: &str, ok: bool, detail: Value, fix: Option<String>) {
        checks.push(json!({
            "check": name,
            "ok": ok,
            "detail": detail,
            "fix": fix,
        }));
    }

    // 1. Git 可用
    let git_ok = Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    push(
        &mut checks,
        "git",
        git_ok,
        json!({}),
        if git_ok {
            None
        } else {
            Some("安装 git 并确认在 PATH 中".into())
        },
    );

    // 2. 声明
    let decl_path = ctx
        .workspace
        .workspace_root
        .join(AILOOM_DIR)
        .join(DECLARATION_FILE);
    let declaration = ProjectDeclaration::load(&decl_path).unwrap_or_else(|e| {
        push(
            &mut checks,
            "declaration",
            false,
            e.to_json(),
            Some("修正 .ailoom/project.toml 或删除后重新 ailoom init".into()),
        );
        None
    });
    if declaration.is_some() {
        push(
            &mut checks,
            "declaration",
            true,
            json!({ "path": decl_path.display().to_string() }),
            None,
        );
    } else if !checks
        .iter()
        .any(|c| c["check"] == "declaration" && c["ok"] == false)
    {
        push(
            &mut checks,
            "declaration",
            false,
            json!({ "missing": decl_path.display().to_string() }),
            Some("运行 ailoom init".into()),
        );
    }

    // 3. 绑定
    let binding = Binding::load(&ctx.layout.binding_path).unwrap_or(None);
    match &binding {
        Some(b) => {
            let matches = declaration
                .as_ref()
                .map(|d| {
                    b.declaration.projects == d.projects
                        && b.declaration.roles == d.roles
                        && b.declaration.targets == d.targets
                })
                .unwrap_or(false);
            push(
                &mut checks,
                "binding",
                matches,
                json!({ "path": ctx.layout.binding_path.display().to_string() }),
                if matches {
                    None
                } else {
                    Some("运行 ailoom init 同步绑定".into())
                },
            );
        }
        None => push(
            &mut checks,
            "binding",
            false,
            json!({ "missing": ctx.layout.binding_path.display().to_string() }),
            Some("运行 ailoom init".into()),
        ),
    }

    // 4. 源锁与快照（离线安全）
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = SourcesLock::load(&lock_path)?;
    if let Some(decl) = &declaration {
        match lock
            .as_ref()
            .and_then(|l| l.sources.get(&decl.source.name).cloned())
        {
            None => push(
                &mut checks,
                "source-lock",
                false,
                json!({ "source": decl.source.name }),
                Some("运行 ailoom init".into()),
            ),
            Some(entry) => {
                let snap_result = if decl.source.kind == "git" {
                    GitSource::new(
                        entry.identity.trim_start_matches("git+"),
                        entry.ref_.as_deref(),
                    )
                    .and_then(|g| g.resolve(&ctx.source_cache(&g.identity), Some(&entry)))
                } else {
                    decl.source
                        .path
                        .as_deref()
                        .map(|p| {
                            let base = if PathBuf::from(p).is_absolute() {
                                PathBuf::from(p)
                            } else {
                                ctx.workspace.workspace_root.join(p)
                            };
                            crate::source::LocalSource::new(&base).and_then(|l| l.resolve())
                        })
                        .transpose()
                        .map(Option::unwrap)
                };
                match snap_result {
                    Ok(snap) => match TeamManifest::load_from(&snap.root) {
                        Ok(m) => push(
                            &mut checks,
                            "source",
                            true,
                            json!({ "identity": entry.identity, "team_id": m.team_id, "resolved_commit": entry.resolved_commit }),
                            None,
                        ),
                        Err(e) => push(
                            &mut checks,
                            "source",
                            false,
                            e.to_json(),
                            Some("恢复网络后 ailoom init --refresh".into()),
                        ),
                    },
                    Err(e) => push(
                        &mut checks,
                        "source",
                        false,
                        e.to_json(),
                        Some("检查网络；离线时确认缓存快照存在".into()),
                    ),
                }
            }
        }
    } else {
        push(
            &mut checks,
            "source-lock",
            false,
            json!({ "reason": "无声明" }),
            Some("运行 ailoom init".into()),
        );
    }

    // 5. 托管清单与目标漂移
    match ManagedManifest::load(&ctx.layout.managed_manifest_path)? {
        None => push(
            &mut checks,
            "managed-manifest",
            true,
            json!({ "items": 0 }),
            None,
        ),
        Some(managed) => {
            let mut drifted: Vec<Value> = Vec::new();
            for (key, item) in &managed.items {
                let current =
                    current_hash_by_key(&ctx.workspace.workspace_root, key, &item.resource_id)
                        .unwrap_or(None);
                match current {
                    None => drifted.push(json!({ "key": key, "state": "missing" })),
                    Some(c) if c != item.content_hash => {
                        drifted.push(json!({ "key": key, "state": "user-modified" }))
                    }
                    _ => {}
                }
            }
            push(
                &mut checks,
                "managed-manifest",
                drifted.is_empty(),
                json!({ "items": managed.items.len(), "drifted": drifted }),
                if drifted.is_empty() {
                    None
                } else {
                    Some("被修改的目标由 ailoom plan/sync 处理（保留并冲突）；确认后可 ailoom uninstall 预览".into())
                },
            );
            let broken =
                crate::require::broken_skill_symlinks(&ctx.workspace.workspace_root, &managed);
            push(
                &mut checks,
                "skill-store-links",
                broken.is_empty(),
                json!({ "broken": broken.iter().map(|(k, r)| json!({"key": k, "reason": r})).collect::<Vec<_>>() }),
                if broken.is_empty() {
                    None
                } else {
                    Some("运行 ailoom sync 重建 SkillStore 链接；或检查 AILOOM_STORE_ROOT".into())
                },
            );
        }
    }

    // 6. 宿主工具（仅提示，不是错误）
    for (tool, bin_name) in [("claude", "claude"), ("codex", "codex")] {
        let version = Command::new(bin_name)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            });
        push(
            &mut checks,
            &format!("host-{tool}"),
            true,
            json!({ "installed": version.is_some(), "version": version }),
            version
                .is_none()
                .then(|| format!("未检测到 {bin_name}（仅提示；不安装宿主工具）")),
        );
    }

    // 7. 未核实 rules 宿主（仍会 file-placed；ok=true，detail 标明 unverified）
    if let Some(decl) = &declaration {
        let unverified: Vec<String> = decl
            .targets
            .extra
            .iter()
            .filter_map(|t| {
                crate::adapters::registry::lookup(t)
                    .filter(|s| !s.verified)
                    .map(|s| s.tool.to_string())
            })
            .collect();
        if !unverified.is_empty() {
            push(
                &mut checks,
                "rules-host-verified",
                true,
                json!({ "unverified": unverified, "level": "file-placed" }),
                Some(
                    "这些宿主发现路径尚未官方核实：产物会落盘，但真实加载需自行验收（见 docs/capabilities/cursor-antigravity.md）"
                        .into(),
                ),
            );
        }
    }

    let ok = checks.iter().all(|c| c["ok"] == true);
    let value = json!({
        "workspace_id": ctx.workspace.workspace_id,
        "ok": ok,
        "checks": checks,
    });
    if !_json {
        println!(
            "工作区 {} 体检（{} 项）：",
            ctx.workspace.workspace_id,
            checks.len()
        );
        for c in &checks {
            let mark = if c["ok"] == true { "OK " } else { "!! " };
            println!("{mark}{}", c["check"]);
            if c["ok"] == false {
                println!("   详情: {}", c["detail"]);
                if let Some(fix) = c["fix"].as_str() {
                    println!("   修复: {fix}");
                }
            } else if let Some(tip) = c["fix"].as_str() {
                println!("   提示: {tip}");
            }
        }
    }
    Ok(value)
}

// ActionKind 引用保留给后续扩展（避免未使用告警时删除语义提示）
#[allow(dead_code)]
fn _touch(_: ActionKind) {}
