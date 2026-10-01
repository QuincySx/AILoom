//! sync 命令（AIL-008/009—012 接线）：执行经校验的计划。

use super::sync_core::prepare;
use crate::error::{code, Error, Result};
use crate::ids::new_id;
use crate::sync::apply::{apply, recover};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct SyncArgs {
    pub root: Option<PathBuf>,
    pub recover: bool,
    pub refresh: bool,
    pub from_auto: bool,
}

pub fn run(args: &SyncArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    if args.recover {
        let cwd = std::env::current_dir()?;
        let ctx = crate::appctx::AppContext::discover(data_root, &cwd, args.root.as_deref())?;
        // 恢复与同步互斥：持锁期间另一次 sync 的 journal 不会被当作遗留回滚。
        let _lock =
            crate::sync::lock::SyncLock::acquire(&ctx.layout.ws_dir.join("locks"), &ctx.device)?;
        let report = recover(&ctx.layout.journal_dir, &ctx.workspace.workspace_root)?;
        let value = json!({
            "ok": report.ok,
            "recovered": report.recovered,
            "skipped_user_modified": report.skipped_user_modified,
            "broken_backups": report.broken_backups,
            "pending_runs": report.pending_runs,
        });
        if !json {
            if report.recovered.is_empty() && report.skipped_user_modified.is_empty() {
                println!("没有待恢复的同步 journal");
            } else {
                if !report.recovered.is_empty() {
                    println!(
                        "已恢复 {} 项: {}",
                        report.recovered.len(),
                        report.recovered.join(", ")
                    );
                }
                if !report.skipped_user_modified.is_empty() {
                    crate::logging::warn(format!(
                        "跳过 {} 项（目标已被修改，拒绝覆盖）: {:?}",
                        report.skipped_user_modified.len(),
                        report.skipped_user_modified
                    ));
                }
            }
            if !report.ok {
                crate::logging::error(format!(
                    "备份损坏/缺失，无法安全恢复: {:?}；恢复点保留: {:?}",
                    report.broken_backups, report.pending_runs
                ));
            }
        }
        if !report.ok {
            return Err(Error::new(
                code::JOURNAL_RESTORE_FAILED,
                "恢复失败：存在缺失或摘要不匹配的备份，未清理恢复点",
            )
            .context(value)
            .fix("人工检查 journal 运行目录与 backup/ 后重试，或确认后手工移除"));
        }
        return Ok(value);
    }

    let mut lock_refreshed = false;
    if args.refresh {
        let cwd = std::env::current_dir()?;
        let ctx = crate::appctx::AppContext::discover(data_root, &cwd, args.root.as_deref())?;
        let decl_path = ctx.declaration_path().ok_or_else(|| {
            Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
        })?;
        let declaration =
            crate::config::ProjectDeclaration::load(&decl_path)?.ok_or_else(|| {
                Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
            })?;
        lock_refreshed = crate::commands::source_refresh::refresh_source_locks(&ctx, &declaration)?;
        if !json && !args.from_auto {
            crate::logging::info(if lock_refreshed {
                "源锁已推进到声明 ref 最新"
            } else {
                "源锁无变化（已是最新）"
            });
        }
    }

    let p = prepare(data_root, args.root.as_deref())?;
    if p.plan.has_conflicts() {
        // 冲突项会在 apply 中跳过；但首次同步出现冲突时提示更友好
        if !args.from_auto {
            crate::logging::warn("计划包含冲突项，对应目标将被跳过（保留现状）");
        }
    }
    let run_id = format!("{}-{}", p.ctx.workspace.workspace_id, new_id());
    let journal_root = &p.ctx.layout.journal_dir;
    let lock_dir = p.ctx.layout.ws_dir.join("locks");
    let mut managed = p.managed.clone();
    let report = apply(
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
    }

    if args.from_auto && report.ok {
        let touched_sensitive = report.applied.iter().any(|a| {
            let s = a.to_lowercase();
            s.contains("/rule")
                || s.contains("rule/")
                || s.contains("agents/")
                || s.contains("mcp")
                || s.contains(".mcp.json")
                || s.contains("settings.json")
                || s.contains("agents.md")
        });
        crate::events::auto_sync::record_success(
            &p.ctx.layout.ws_dir,
            report.applied.len(),
            touched_sensitive,
        );
    }

    let value = json!({
        "ok": report.ok,
        "applied": report.applied,
        "noop": report.noop,
        "skipped_conflicts": report.skipped_conflicts,
        "skipped_unsupported": report.skipped_unsupported,
        "deployed_revision": report.deployed_revision,
        "failed": report.failed,
        "pending_journal": report.pending_journal,
        "unsupported": p.unsupported,
        "lock_refreshed": lock_refreshed,
        "from_auto": args.from_auto,
    });
    if !json && !args.from_auto {
        if report.ok {
            println!(
                "同步完成：写入 {} 项，无操作 {} 项，冲突跳过 {} 项",
                report.applied.len(),
                report.noop,
                report.skipped_conflicts.len()
            );
        } else {
            crate::logging::error(format!(
                "同步失败：{}；恢复点 {}",
                report
                    .failed
                    .as_ref()
                    .map(|f| f["message"].as_str().unwrap_or("?"))
                    .unwrap_or("?"),
                report.pending_journal.as_deref().unwrap_or("?")
            ));
        }
    }
    if !report.ok {
        return Err(
            Error::new(code::WRITE_FAILED, "同步未全部完成；详见 --json 输出")
                .context(value.clone())
                .fix(format!("运行 ailoom sync --recover 后重试（run {run_id}）")),
        );
    }
    Ok(value)
}
