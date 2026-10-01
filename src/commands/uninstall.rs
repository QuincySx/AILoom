//! uninstall（AIL-013）：按托管清单生成统一删除计划，预览后执行。
//! 保留用户修改与非托管内容；只影响当前工作区；重复运行幂等。

use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use crate::ids::new_id;
use crate::personal_instructions as pi;
use crate::sync::apply::{remove_by_key, ApplyReport};
use crate::sync::lock::SyncLock;
use crate::sync::manifest::ManagedManifest;
use crate::sync::plan::{current_hash_by_key, split_key, ActionKind, PlanAction, SyncPlan};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct UninstallArgs {
    pub root: Option<PathBuf>,
    /// 默认只预览；--execute 才真正删除
    pub execute: bool,
}

pub fn run(args: &UninstallArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let managed_path = ctx.layout.managed_manifest_path.clone();
    let mut managed = ManagedManifest::load(&managed_path)?
        .unwrap_or_else(|| ManagedManifest::new(&ctx.workspace.workspace_id));

    // 损坏的清单已在 load 报错；此处按清单生成删除计划
    let mut actions: Vec<PlanAction> = Vec::new();
    let mut keys: Vec<&String> = managed.items.keys().collect();
    keys.sort();
    for key in keys {
        let item = &managed.items[key];
        let (path, _) = split_key(key);
        let current = current_hash_by_key(&ctx.workspace.workspace_root, key, &item.resource_id)?;
        // AIL-052：公司文件保护对卸载同样生效——被跟踪路径一律不改写
        if pi::path_is_git_tracked(&ctx.workspace.workspace_root, &path) {
            actions.push(PlanAction {
                action: ActionKind::Conflict,
                item_key: key.clone(),
                path,
                resource_id: item.resource_id.clone(),
                target_tool: item.target_tool.clone(),
                kind: item.kind.clone(),
                reason: "目标已被 Git 跟踪（公司文件），卸载拒绝改写".into(),
                precondition_hash: current,
                desired_hash: String::new(),
                manifest_hash: Some(item.content_hash.clone()),
            });
            continue;
        }
        let (action, reason, precondition) = match &current {
            None => (ActionKind::Noop, "目标已不存在", None),
            Some(c) if *c == item.content_hash => (
                ActionKind::Delete,
                "AILoom 托管内容，可安全移除",
                current.clone(),
            ),
            Some(_) => (
                ActionKind::Conflict,
                "用户修改过，保留并冲突",
                current.clone(),
            ),
        };
        actions.push(PlanAction {
            action,
            item_key: key.clone(),
            path,
            resource_id: item.resource_id.clone(),
            target_tool: item.target_tool.clone(),
            kind: item.kind.clone(),
            reason: reason.into(),
            precondition_hash: precondition,
            desired_hash: String::new(),
            manifest_hash: Some(item.content_hash.clone()),
        });
    }

    let plan = SyncPlan {
        schema_version: 1,
        created_at: crate::ids::now_iso(),
        source_identity: String::new(),
        revision: None,
        actions,
    };

    if !args.execute {
        let value = json!({
            "mode": "preview",
            "workspace_id": ctx.workspace.workspace_id,
            "actions": plan.actions,
        });
        if !json {
            print!("{}", plan.summary());
            println!("这是预览。确认后运行 ailoom uninstall --execute");
        }
        return Ok(value);
    }

    // 执行：逐键删除 + 冲突保留 + 清单收缩
    let lock_dir = ctx.layout.ws_dir.join("locks");
    let _lock = SyncLock::acquire(&lock_dir, &ctx.device)?;
    let mut report = ApplyReport {
        ok: true,
        applied: vec![],
        noop: 0,
        skipped_conflicts: vec![],
        deployed_revision: None,
        failed: None,
        pending_journal: None,
    };
    for action in &plan.actions {
        match action.action {
            ActionKind::Delete => match remove_by_key(
                &ctx.workspace.workspace_root,
                &action.item_key,
                &action.resource_id,
            ) {
                Ok(()) => {
                    managed.items.remove(&action.item_key);
                    report.applied.push(action.path.clone());
                }
                Err(e) => {
                    report.ok = false;
                    report.failed = Some(e.to_json());
                    break;
                }
            },
            ActionKind::Noop => {
                // 目标已不存在：从清单移除，保持幂等
                managed.items.remove(&action.item_key);
                report.noop += 1;
            }
            ActionKind::Conflict => report.skipped_conflicts.push(action.path.clone()),
            _ => {}
        }
    }
    if report.ok {
        managed.save(&managed_path)?;
    }
    let _ = _lock.release();
    let _ = new_id(); // 保持 import 一致性（journal run id 由 apply 使用；此处无 journal）

    let value = json!({
        "mode": "execute",
        "workspace_id": ctx.workspace.workspace_id,
        "ok": report.ok,
        "removed": report.applied,
        "kept_conflicts": report.skipped_conflicts,
        "noop": report.noop,
        "failed": report.failed,
    });
    if !json {
        println!(
            "卸载完成：移除 {} 项，保留（冲突）{} 项",
            report.applied.len(),
            report.skipped_conflicts.len()
        );
    }
    if !report.ok {
        return Err(Error::new(code::WRITE_FAILED, "卸载未全部完成").context(value.clone()));
    }
    Ok(value)
}
