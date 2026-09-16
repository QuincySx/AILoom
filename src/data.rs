//! 事件留存、数据导出与清理（AIL-037）。

use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct DataArgs {
    pub action: String,       // rotate | export | cleanup
    pub out: Option<PathBuf>, // export 目标目录
    pub max_size_mb: f64,     // rotate 阈值（MB）
    pub dry_run: bool,
    pub root: Option<PathBuf>,
}

pub fn run(args: &DataArgs, _json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    match args.action.as_str() {
        "rotate" => rotate(&ctx.layout.events_file, args.max_size_mb),
        "export" => export(&ctx, args.out.as_deref()),
        "cleanup" => cleanup(&ctx, args.dry_run),
        other => Err(Error::new(
            code::USAGE,
            format!("未知 data 动作: {other}（rotate/export/cleanup）"),
        )),
    }
}

/// 日志轮转：委托 events::store::rotate_events_file（唯一归档名 + 与追加一致锁）；
/// 聚合经 read_all_events 读取全部活动+归档，累计值不丢。
fn rotate(events_file: &Path, max_size_mb: f64) -> Result<Value> {
    let size = if events_file.is_file() {
        std::fs::metadata(events_file)?.len()
    } else {
        0
    };
    match crate::events::store::rotate_events_file(events_file, max_size_mb)? {
        Some(archive) => Ok(json!({
            "rotated": true,
            "archive": archive.display().to_string(),
            "size_bytes": size,
        })),
        None => Ok(json!({ "rotated": false, "size_bytes": size })),
    }
}

/// 导出：工作区数据清单（事件/摘要/指标），默认排除自由文本与秘密（事件本就不含 prompt 全文）。
/// 导出文件可重新读入审计（JSON 结构化）。
fn export(ctx: &AppContext, out: Option<&Path>) -> Result<Value> {
    let out_dir = out
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ctx.layout.ws_dir.join("export"));
    std::fs::create_dir_all(&out_dir)?;
    let mut exported = Vec::new();
    // 事件：活动 + 全部归档（审计完整链路；事件不含 prompt 全文/秘密）
    if ctx.layout.events_dir.is_dir() {
        for entry in std::fs::read_dir(&ctx.layout.events_dir)?.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_file() && (name == "events.jsonl" || name.starts_with("events-archive-")) {
                let dest = out_dir.join(&name);
                std::fs::copy(&p, &dest)?;
                exported.push(dest.display().to_string());
            }
        }
    }
    // 上报 checkpoint（聚合基线审计：确认水位线与批次）
    let cp_path = ctx.layout.ws_dir.join("report-checkpoint.json");
    if cp_path.is_file() {
        let dest = out_dir.join("report-checkpoint.json");
        std::fs::copy(&cp_path, &dest)?;
        exported.push(dest.display().to_string());
    }
    // 摘要
    if ctx.layout.summary_dir.is_dir() {
        for entry in std::fs::read_dir(&ctx.layout.summary_dir)?.flatten() {
            let p = entry.path();
            if p.extension().map(|e| e == "json").unwrap_or(false) {
                let dest = out_dir.join(format!("summary-{}", entry.file_name().to_string_lossy()));
                std::fs::copy(&p, &dest)?;
                exported.push(dest.display().to_string());
            }
        }
    }
    // 聚合指标（重建一份全量快照）
    let heuristic = crate::events::aggregate::HeuristicConfig::default();
    let (events, bad) = crate::events::store::read_all_events(&ctx.layout.events_dir)?;
    let all =
        crate::events::aggregate::aggregate_all(&ctx.workspace.workspace_id, &events, &heuristic)?;
    let metrics_path = out_dir.join("session-metrics.json");
    crate::sync_common::atomic_write(
        metrics_path.as_path(),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "bad_event_lines": bad,
            "sessions": all.values().collect::<Vec<_>>(),
        }))?
        .as_slice(),
    )?;
    exported.push(metrics_path.display().to_string());
    Ok(json!({ "exported": exported, "dir": out_dir.display().to_string() }))
}

/// 清理：按工作区拥有者清单删除可重建数据——
/// 仅当前工作区自己的源缓存（cache/<本 anchor>）与"已确认上传且无坏行"的归档事件。
/// 未上报事件（无确认水位线覆盖）一律保留；dry-run 计划与执行完全一致。
fn cleanup(ctx: &AppContext, dry_run: bool) -> Result<Value> {
    // 待审草稿/未上报批次存在时拒绝清理（防丢）
    let pending_report = ctx.layout.ws_dir.join("report-checkpoint.json");
    if pending_report.is_file() {
        let text = std::fs::read_to_string(&pending_report)?;
        let v: Value = serde_json::from_str(&text)?;
        if v["pending"]
            .as_object()
            .map(|o| !o.is_empty())
            .unwrap_or(false)
        {
            return Err(Error::new(
                code::REPORT_NOT_CONFIRMED,
                "存在未确认上报的批次，拒绝清理（先补传或显式丢弃）",
            ));
        }
    }

    let mut planned: Vec<String> = Vec::new();
    let mut skipped_shared = 0usize;
    // 1. 只清理本工作区 anchor 拥有的源缓存；且该缓存未被其他工作区共享引用
    //    （同仓另一 worktree / 其他 checkout 共享 anchor 时保留，保证对方离线可读锁定内容）
    let mut shared_by_others = false;
    let ws_root_dir = ctx.data_root.join("ws");
    if ws_root_dir.is_dir() {
        for entry in std::fs::read_dir(&ws_root_dir)?.flatten() {
            if entry.path() == ctx.layout.ws_dir {
                continue;
            }
            let binding = entry.path().join("binding.json");
            if let Ok(text) = std::fs::read_to_string(&binding) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(anchor) = v["repository_anchor"].as_str() {
                        if crate::ids::sha256_prefix(anchor.as_bytes(), 16)
                            == ctx.workspace.anchor_key
                        {
                            shared_by_others = true;
                        }
                    }
                }
            }
        }
    }
    if ctx.layout.cache_root.is_dir() {
        if shared_by_others {
            skipped_shared += 1;
        } else {
            planned.push(ctx.layout.cache_root.display().to_string());
        }
    }
    // 2. 归档事件：必须全部事件 id 都在确认水位线内，且无坏行（损坏文件报告并保留）
    let confirmed = crate::reporting::confirmed_event_watermark(ctx)?;
    if ctx.layout.events_dir.is_dir() {
        for entry in std::fs::read_dir(&ctx.layout.events_dir)?.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !(p.is_file() && name.starts_with("events-archive-")) {
                continue;
            }
            let (events, bad) = crate::events::store::read_events(&p)?;
            if bad > 0 {
                // 损坏归档：保留并报告，不能全量删
                continue;
            }
            let unconfirmed = events
                .iter()
                .filter(|e| !confirmed.contains(&e.event_id))
                .count();
            if unconfirmed > 0 {
                // 从未上报或未确认的历史：保留
                continue;
            }
            planned.push(p.display().to_string());
        }
    }

    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "would_remove": planned,
            "confirmed_event_watermark": confirmed.len(),
            "skipped_shared_cache": skipped_shared,
            "note": "仅列出本工作区缓存与已确认上传的归档；其他工作区数据不在范围内",
        }));
    }
    // RW-09/R06：删除已确认归档前，把其中事件按完整会话身份累计进基线文件，
    // 正常读入口（session/report/dashboard）消费基线，累计视图不因清理丢失。
    // 幂等：已在基线 accounted 集合中的事件不再累加（写入中断后重试不翻倍）。
    // 基线损坏 → 拒绝破坏性清理并可修复后重试。
    let planned_archives: Vec<&String> = planned
        .iter()
        .filter(|p| {
            Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("events-archive-"))
                .unwrap_or(false)
        })
        .collect();
    if !planned_archives.is_empty() {
        let heuristic = crate::events::aggregate::HeuristicConfig::load(&ctx.layout.ws_dir);
        let mut archived_events: Vec<crate::events::schema::Event> = Vec::new();
        for p in &planned_archives {
            let (mut evs, bad) = crate::events::store::read_events(Path::new(p))?;
            if bad > 0 {
                return Err(Error::new(
                    code::INTERNAL,
                    format!("归档 {p} 含 {bad} 个坏行，拒绝清理（先人工检查）"),
                ));
            }
            archived_events.append(&mut evs);
        }
        let (mut identities, mut accounted) =
            match crate::events::aggregate::load_metrics_baseline(&ctx.layout.ws_dir)? {
                Some((i, a)) => (i, a),
                None => (Default::default(), Default::default()),
            };
        // 只累计尚未入账的事件（崩溃后重试不翻倍）
        let fresh: Vec<&crate::events::schema::Event> = archived_events
            .iter()
            .filter(|e| !accounted.contains(&e.event_id))
            .collect();
        let fresh_aggregate = crate::events::aggregate::aggregate_all(
            &ctx.workspace.workspace_id,
            &fresh.iter().map(|e| (*e).clone()).collect::<Vec<_>>(),
            &heuristic,
        )?;
        for (k, m) in fresh_aggregate {
            let entry = identities.entry(k).or_default();
            if entry.session_id.is_empty() {
                entry.session_id = m.session_id.clone();
                entry.workspace_id = m.workspace_id.clone();
                entry.tool = m.tool.clone();
                entry.device_id = m.device_id.clone();
            }
            crate::events::aggregate::add_metrics(entry, &m);
        }
        for e in &fresh {
            accounted.insert(e.event_id.clone());
        }
        crate::events::aggregate::save_metrics_baseline(
            &ctx.layout.ws_dir,
            &identities,
            &accounted,
        )?;
    }
    let mut removed = Vec::new();
    for p in &planned {
        let path = PathBuf::from(p);
        if path.is_dir() {
            crate::sync_common::remove_dir_all_guarded(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
        removed.push(p.clone());
    }
    Ok(json!({ "removed": removed, "dry_run": false }))
}
