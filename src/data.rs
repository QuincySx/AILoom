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

/// 日志轮转：events.jsonl 超过阈值 → 改名归档 events-archive-<ts>.jsonl（不删除，会话累计值保留在归档中，
/// 聚合读取所有 events*.jsonl 因此不丢累计值）。
fn rotate(events_file: &Path, max_size_mb: f64) -> Result<Value> {
    if !events_file.is_file() {
        return Ok(json!({ "rotated": false, "reason": "无事件文件" }));
    }
    let size = std::fs::metadata(events_file)?.len() as f64;
    let limit = max_size_mb * 1024.0 * 1024.0;
    if size <= limit {
        return Ok(json!({ "rotated": false, "size_bytes": size as u64 }));
    }
    let ts = crate::ids::now_iso().replace(':', "");
    let archive = events_file.with_file_name(format!("events-archive-{ts}.jsonl"));
    std::fs::rename(events_file, &archive)?;
    Ok(
        json!({ "rotated": true, "archive": archive.display().to_string(), "size_bytes": size as u64 }),
    )
}

/// 导出：工作区数据清单（事件/摘要/指标），默认排除自由文本与秘密（事件本就不含 prompt 全文）。
/// 导出文件可重新读入审计（JSON 结构化）。
fn export(ctx: &AppContext, out: Option<&Path>) -> Result<Value> {
    let out_dir = out
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ctx.layout.ws_dir.join("export"));
    std::fs::create_dir_all(&out_dir)?;
    let mut exported = Vec::new();
    // 事件（不含 prompt 全文）
    let events_src = ctx.layout.events_dir.join("events.jsonl");
    if events_src.is_file() {
        let dest = out_dir.join("events.jsonl");
        std::fs::copy(&events_src, &dest)?;
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
    let (events, bad) = crate::events::store::read_events(&ctx.layout.events_file)?;
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

/// 清理：按工作区拥有者清单删除可重建数据（缓存/归档事件），保护未上报批次与待审草稿。
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
    // 缓存目录（可重建）
    let cache_root = ctx.data_root.join("cache");
    if cache_root.is_dir() {
        for entry in std::fs::read_dir(&cache_root)?.flatten() {
            planned.push(entry.path().display().to_string());
        }
    }
    // 归档事件
    if ctx.layout.events_dir.is_dir() {
        for entry in std::fs::read_dir(&ctx.layout.events_dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("events-archive-") {
                planned.push(entry.path().display().to_string());
            }
        }
    }
    if dry_run {
        return Ok(json!({ "dry_run": true, "would_remove": planned }));
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
    Ok(json!({ "removed": removed }))
}
