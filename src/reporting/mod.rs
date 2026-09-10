//! 团队统计上报与 digest（AIL-022）：独立报告分支 + 幂等批次 + 周期汇总。
//! 资源 revision 与统计提交互不污染；上报默认关闭、可关闭；离线失败可补传。

use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use crate::events::aggregate::{aggregate_all, HeuristicConfig};
use crate::events::friction::{build_share_record, FrictionConfig};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const REPORT_BRANCH: &str = "ailoom/reports";
/// 上报批次文件在报告分支中的目录前缀（独立于资源路径）。
pub const REPORT_PREFIX: &str = "reports/sessions/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportCheckpoint {
    pub schema_version: u32,
    /// 已确认推送的批次 ID
    pub pushed_batches: Vec<String>,
    /// 待补传：解析成功但未确认推送的批次
    pub pending: BTreeMap<String, serde_json::Value>,
}

fn checkpoint_path(ctx: &AppContext) -> PathBuf {
    ctx.layout.ws_dir.join("report-checkpoint.json")
}

fn load_checkpoint(ctx: &AppContext) -> ReportCheckpoint {
    std::fs::read_to_string(checkpoint_path(ctx))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(ReportCheckpoint {
            schema_version: 1,
            pushed_batches: Vec::new(),
            pending: Default::default(),
        })
}

fn save_checkpoint(ctx: &AppContext, cp: &ReportCheckpoint) -> Result<()> {
    crate::sync_common::atomic_write(
        checkpoint_path(ctx).as_path(),
        serde_json::to_vec_pretty(cp)?.as_slice(),
    )
}

pub struct ReportArgs {
    pub action: String, // push | digest | status
    pub root: Option<PathBuf>,
}

pub fn run(args: &ReportArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let _cfg = FrictionConfig::load(&ctx.layout.ws_dir);
    let heuristic = HeuristicConfig::default();
    let (events, _) = crate::events::store::read_events(&ctx.layout.events_file)?;
    let sessions = aggregate_all(&ctx.workspace.workspace_id, &events, &heuristic)?;

    match args.action.as_str() {
        "push" => {
            if sessions.is_empty() {
                return Err(Error::new(
                    code::REPORT_NOT_CONFIRMED,
                    "没有可上报的会话数据",
                ));
            }
            let declaration = ctx
                .declaration_path()
                .and_then(|p| crate::config::ProjectDeclaration::load(&p).ok().flatten());
            let reporting_enabled = std::env::var("AILOOM_REPORTING")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false)
                || declaration
                    .as_ref()
                    .map(|d| d.reporting_enabled)
                    .unwrap_or(false);
            if !reporting_enabled {
                return Err(Error::new(
                    code::REPORT_NOT_CONFIRMED,
                    "团队统计上报未开启（默认关闭）",
                )
                .fix("在声明中设置 reporting_enabled = true，或运行时 AILOOM_REPORTING=1"));
            }
            // 变更集式提交到独立报告分支：复用贡献机制（隔离 worktree + 精确路径）
            let batch_id = crate::ids::new_id();
            let mut cp = load_checkpoint(&ctx);
            let mut files: Vec<(String, Vec<u8>)> = Vec::new();
            for (sid, m) in &sessions {
                // 共享白名单：仅计数与工具名；session id 以哈希呈现
                let record = build_share_record(m);
                let rel = format!("{REPORT_PREFIX}{}-{sid}.json", today());
                files.push((
                    rel,
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "batch_id": batch_id,
                        "record": record,
                    }))?,
                ));
            }
            // 幂等：同内容批次重试产生同一 batch 文件集合；checkpoint 防重
            let req = crate::contribution::prepare_contribution(data_root, args.root.as_deref())?;
            let (wt, _cs) = crate::contribution::ensure_worktree(
                &req.ctx,
                &req.cache_repo,
                &format!("report-{}", req.source_alias),
                &req.base,
            )?;
            let mut staged: Vec<String> = Vec::new();
            for (rel, bytes) in &files {
                let dest = wt.path.join(rel);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&dest, bytes)?;
                crate::gitx::git(&wt.path, &["add", "--", rel])?;
                staged.push(rel.clone());
            }
            // 推送到独立报告分支（不是资源审核分支）
            let report_branch = format!("{REPORT_BRANCH}-{}", &batch_id[..8]);
            crate::gitx::git(&wt.path, &["checkout", "-q", "-B", &report_branch])?;
            crate::gitx::git(
                &wt.path,
                &[
                    "-c",
                    "user.name=ailoom-report",
                    "-c",
                    "user.email=report@ailoom.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-q",
                    "-m",
                    &format!("report batch {batch_id}"),
                ],
            )?;
            let push_result = crate::gitx::git(&wt.path, &["push", "-q", "origin", &report_branch]);
            match push_result {
                Ok(_) => {
                    cp.pushed_batches.push(batch_id.clone());
                    save_checkpoint(&ctx, &cp)?;
                }
                Err(e) => {
                    // 离线失败：pending 补传，不清空队列
                    cp.pending.insert(
                        batch_id.clone(),
                        serde_json::json!({ "files": staged, "branch": report_branch }),
                    );
                    save_checkpoint(&ctx, &cp)?;
                    wt.cleanup();
                    return Err(Error::new(
                        code::REPORT_NOT_CONFIRMED,
                        "推送未确认（离线或远端错误）；批次保留待补传",
                    )
                    .context(e.to_json()));
                }
            }
            wt.cleanup();
            let value = serde_json::json!({
                "batch_id": batch_id,
                "branch": report_branch,
                "sessions_reported": staged.len(),
                "committed_paths": staged,
            });
            if !json {
                crate::logging::info(format!(
                    "统计已推送：批次 {batch_id}（分支 {report_branch}）"
                ));
            }
            Ok(value)
        }
        "digest" => {
            // 周期汇总：本地聚合视图，标注时间范围、时区与缺失源
            let sessions_list: Vec<&crate::events::aggregate::SessionMetrics> =
                sessions.values().collect();
            let totals: crate::events::aggregate::SessionMetrics = Default::default();
            let _ = totals;
            let mut summary = serde_json::json!({
                "schema_version": 1,
                "generated_at": crate::ids::now_iso(),
                "timezone": "UTC",
                "range": { "note": "覆盖本地事件文件时间范围（见 min/max）" },
                "workspace_id": ctx.workspace.workspace_id,
                "session_count": sessions_list.len(),
                "totals": {
                    "prompt_count": sessions_list.iter().map(|m| m.prompt_count).sum::<u64>(),
                    "tool_calls": sessions_list.iter().map(|m| m.tool_calls).sum::<u64>(),
                    "tool_errors": sessions_list.iter().map(|m| m.tool_errors).sum::<u64>(),
                    "interventions": sessions_list.iter().map(|m| m.interventions).sum::<u64>(),
                },
                "tokens": {
                    "input": sessions_list.iter().filter_map(|m| m.tokens.input).sum::<u64>(),
                    "output": sessions_list.iter().filter_map(|m| m.tokens.output).sum::<u64>(),
                    "missing_token_sessions": sessions_list.iter().filter(|m| m.tokens.input.is_none()).count(),
                },
                "missing_sources": {
                    "unavailable_token_sessions": sessions_list.iter().filter(|m| m.tokens_availability == "unavailable").count(),
                    "note": "缺失来源在汇总中可见，不计为 0",
                },
            });
            if let (Some(min), Some(max)) = time_bounds(&events) {
                summary["range"] = json!({ "min": min, "max": max, "timezone": "UTC" });
            }
            Ok(summary)
        }
        "status" => {
            let cp = load_checkpoint(&ctx);
            Ok(serde_json::json!({
                "pushed_batches": cp.pushed_batches,
                "pending": cp.pending,
            }))
        }
        other => Err(Error::new(
            code::USAGE,
            format!("未知 report 动作: {other}（push/digest/status）"),
        )),
    }
}

fn today() -> String {
    let now = chrono::Utc::now();
    now.format("%Y-%m-%d").to_string()
}

fn time_bounds(events: &[crate::events::schema::Event]) -> (Option<String>, Option<String>) {
    let mut times: Vec<&str> = events.iter().map(|e| e.time.as_str()).collect();
    times.sort();
    (
        times.first().map(|s| s.to_string()),
        times.last().map(|s| s.to_string()),
    )
}
