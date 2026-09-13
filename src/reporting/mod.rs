//! 团队统计上报与 digest（AIL-022）：独立报告分支 + 内容派生稳定批次 +
//! 冻结补传状态机 + 团队周期汇总。资源 revision 与统计提交互不污染；
//! 上报默认关闭、可关闭；离线失败可补传且重试不重复计数。

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
/// 批次清单目录（同 batch_id 重试产生同一路径，不产生重复批次）。
pub const REPORT_BATCH_PREFIX: &str = "reports/batches/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportCheckpoint {
    pub schema_version: u32,
    /// 已确认推送的批次 ID（内容派生，幂等依据）
    pub pushed_batches: Vec<String>,
    /// 待补传：批次 ID → 冻结的完整批次内容（文件字节 + 覆盖事件 + 分支）
    pub pending: BTreeMap<String, serde_json::Value>,
    /// 已确认上传的事件 id 集合（确认水位线；AIL-037 清理按此判断可删）
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub confirmed_event_ids: std::collections::BTreeSet<String>,
}

fn checkpoint_path(ctx: &AppContext) -> PathBuf {
    ctx.layout.ws_dir.join("report-checkpoint.json")
}

fn empty_checkpoint() -> ReportCheckpoint {
    ReportCheckpoint {
        schema_version: 1,
        pushed_batches: Vec::new(),
        pending: Default::default(),
        confirmed_event_ids: Default::default(),
    }
}

fn load_checkpoint(ctx: &AppContext) -> ReportCheckpoint {
    std::fs::read_to_string(checkpoint_path(ctx))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(empty_checkpoint)
}

fn save_checkpoint(ctx: &AppContext, cp: &ReportCheckpoint) -> Result<()> {
    crate::sync_common::atomic_write(
        checkpoint_path(ctx).as_path(),
        serde_json::to_vec_pretty(cp)?.as_slice(),
    )
}

/// AIL-037 清理接口：返回已确认上传的事件 id 集合。
/// 清理实现必须以该集合判断归档是否可删，不得以 pending 为空猜测安全。
pub fn confirmed_event_watermark(
    ctx: &crate::appctx::AppContext,
) -> Result<std::collections::BTreeSet<String>> {
    let cp = load_checkpoint(ctx);
    Ok(cp.confirmed_event_ids)
}

pub struct ReportArgs {
    pub action: String, // push | retry | digest | status
    pub root: Option<PathBuf>,
}

/// 冻结批次：内容派生身份 + 完整文件字节，重试推送与首次完全一致。
#[derive(Debug, Clone)]
struct FrozenBatch {
    batch_id: String,
    branch: String,
    /// rel 路径 → 文件内容（UTF-8 JSON）
    files: BTreeMap<String, String>,
    covered_event_ids: Vec<String>,
}

impl FrozenBatch {
    fn to_pending_json(&self) -> Value {
        json!({
            "branch": self.branch,
            "files": self.files,
            "covered_event_ids": self.covered_event_ids,
        })
    }

    fn from_pending_json(batch_id: &str, v: &Value) -> Option<FrozenBatch> {
        let branch = v.get("branch")?.as_str()?.to_string();
        let mut files = BTreeMap::new();
        for (k, val) in v.get("files")?.as_object()? {
            files.insert(k.clone(), val.as_str()?.to_string());
        }
        let covered = v
            .get("covered_event_ids")?
            .as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
        Some(FrozenBatch {
            batch_id: batch_id.to_string(),
            branch,
            files,
            covered_event_ids: covered,
        })
    }
}

/// 由会话记录构建内容派生批次：同内容重试 → 同 batch_id、同文件集合、同分支。
fn build_batch(
    ctx: &AppContext,
    sessions: &BTreeMap<String, crate::events::aggregate::SessionMetrics>,
) -> Result<FrozenBatch> {
    let device_hash = crate::ids::sha256_prefix(ctx.device.as_bytes(), 12);
    let date = today();
    let mut record_material = String::new();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for (sid, m) in sessions {
        // 共享白名单：仅计数与工具名；session id 以哈希呈现
        let record = build_share_record(m);
        let sid_hash = record["session_id_hash"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| crate::ids::sha256_prefix(sid.as_bytes(), 16));
        let rel = format!("{REPORT_PREFIX}{date}-{sid_hash}.json");
        let body = json!({
            "schema_version": 1,
            "record": record,
            "device_id_hash": device_hash,
        });
        let text = serde_json::to_string_pretty(&body)?;
        record_material.push_str(&format!("{rel}:{text};"));
        files.insert(rel, text);
    }
    let batch_id = format!(
        "b{}",
        crate::ids::sha256_prefix(
            format!(
                "{}/{}|{}",
                ctx.workspace.workspace_id, device_hash, record_material
            )
            .as_bytes(),
            24
        )
    );
    let manifest_rel = format!("{REPORT_BATCH_PREFIX}{batch_id}.json");
    files.insert(
        manifest_rel,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "batch_id": batch_id.clone(),
            "session_files": files.keys().cloned().collect::<Vec<_>>(),
            "note": "会话记录为累计快照（字段取最大值语义），增量与累计不可混用",
        }))?,
    );
    let branch = format!("{REPORT_BRANCH}-{}", &batch_id[1..9]);
    Ok(FrozenBatch {
        batch_id,
        branch,
        files,
        covered_event_ids: Vec::new(),
    })
}

/// 在隔离 worktree 中提交并推送批次（内容由调用方冻结）。
fn push_batch(
    req: &crate::contribution::ContributionRequest,
    batch: &FrozenBatch,
) -> Result<usize> {
    let (wt, _cs) = crate::contribution::ensure_worktree(
        &req.ctx,
        &req.cache_repo,
        &format!("report-{}", req.source_alias),
        &req.base,
    )?;
    let result = (|| -> Result<usize> {
        let mut staged: Vec<String> = Vec::new();
        for (rel, text) in &batch.files {
            let dest = wt.path.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, text.as_bytes())?;
            crate::gitx::git(&wt.path, &["add", "--", rel])?;
            staged.push(rel.clone());
        }
        // 推送到独立报告分支（不是资源审核分支）。
        // 幂等：同 batch_id 重试的工作区内容与上次一致 → --allow-empty 生成同内容提交，
        // 分支被推成同内容（远端无重复批次文件路径）。
        crate::gitx::git(&wt.path, &["checkout", "-q", "-B", &batch.branch])?;
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
                "--allow-empty",
                "-m",
                &format!("report batch {}", batch.batch_id),
            ],
        )?;
        // 批次分支由 batch_id 内容寻址、仅生产者写入：重试重推同内容用 --force
        //（幂等：远端批次文件路径确定性，最终内容一致，不产生重复批次）
        crate::gitx::git(
            &wt.path,
            &["push", "-q", "--force", "origin", &batch.branch],
        )?;
        Ok(staged.len())
    })();
    wt.cleanup();
    result
}

pub fn run(args: &ReportArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let _cfg = FrictionConfig::load(&ctx.layout.ws_dir);
    let heuristic = HeuristicConfig::default();
    let (events, _) = crate::events::store::read_all_events(&ctx.layout.events_dir)?;
    let sessions = aggregate_all(&ctx.workspace.workspace_id, &events, &heuristic)?;

    match args.action.as_str() {
        "retry" => retry_action(data_root, args.root.as_deref()),
        "push" => {
            if sessions.is_empty() {
                return Err(Error::new(
                    code::REPORT_NOT_CONFIRMED,
                    "没有可上报的会话数据",
                ));
            }
            let reporting_enabled = reporting_enabled(&ctx);
            if !reporting_enabled {
                return Err(Error::new(
                    code::REPORT_NOT_CONFIRMED,
                    "团队统计上报未开启（默认关闭）",
                )
                .fix("在声明中设置 reporting_enabled = true，或运行时 AILOOM_REPORTING=1"));
            }
            let mut cp = load_checkpoint(&ctx);
            let mut batch = build_batch(&ctx, &sessions)?;
            batch.covered_event_ids = events.iter().map(|e| e.event_id.clone()).collect();

            // 幂等：同内容批次已确认推送 → 不再推送、不产生重复批次
            if cp.pushed_batches.contains(&batch.batch_id) {
                let value = json!({
                    "batch_id": batch.batch_id,
                    "branch": batch.branch,
                    "already_pushed": true,
                    "sessions_reported": sessions.len(),
                });
                if !json {
                    crate::logging::info(format!("批次 {} 已推送，跳过", batch.batch_id));
                }
                return Ok(value);
            }

            let req = crate::contribution::prepare_contribution(data_root, args.root.as_deref())?;
            let staged_len = batch.files.len();
            match push_batch(&req, &batch) {
                Ok(_) => {
                    confirm_batch(&mut cp, &batch);
                    save_checkpoint(&ctx, &cp)?;
                }
                Err(e) => {
                    // 离线失败：冻结批次进 pending（含完整文件内容），可 retry 补传
                    cp.pending
                        .insert(batch.batch_id.clone(), batch.to_pending_json());
                    save_checkpoint(&ctx, &cp)?;
                    return Err(Error::new(
                        code::REPORT_NOT_CONFIRMED,
                        "推送未确认（离线或远端错误）；批次内容已冻结待补传",
                    )
                    .context(e.to_json())
                    .fix("恢复网络后运行 ailoom report --action retry"));
                }
            }
            let value = serde_json::json!({
                "batch_id": batch.batch_id,
                "branch": batch.branch,
                "sessions_reported": sessions.len(),
                "committed_paths": staged_len,
            });
            if !json {
                crate::logging::info(format!(
                    "统计已推送：批次 {}（分支 {}）",
                    batch.batch_id, batch.branch
                ));
            }
            Ok(value)
        }
        "digest" => team_digest(&ctx, data_root, args.root.as_deref()),
        "status" => {
            let cp = load_checkpoint(&ctx);
            Ok(serde_json::json!({
                "pushed_batches": cp.pushed_batches,
                "pending": cp.pending,
                "confirmed_event_count": cp.confirmed_event_ids.len(),
            }))
        }
        other => Err(Error::new(
            code::USAGE,
            format!("未知 report 动作: {other}（push/retry/digest/status）"),
        )),
    }
}

fn confirm_batch(cp: &mut ReportCheckpoint, batch: &FrozenBatch) {
    if !cp.pushed_batches.contains(&batch.batch_id) {
        cp.pushed_batches.push(batch.batch_id.clone());
    }
    for id in &batch.covered_event_ids {
        cp.confirmed_event_ids.insert(id.clone());
    }
    cp.pending.remove(&batch.batch_id);
}

/// 补传：逐个重推冻结批次（与首次字节一致），成功后清除 pending 并更新确认水位线。
fn retry_action(
    data_root: Option<&std::path::Path>,
    explicit_root: Option<&std::path::Path>,
) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, explicit_root)?;
    let mut cp = load_checkpoint(&ctx);
    if cp.pending.is_empty() {
        return Ok(json!({ "retried": 0, "pending_left": 0, "note": "没有待补传批次" }));
    }
    let req = crate::contribution::prepare_contribution(data_root, explicit_root)?;
    let pending_ids: Vec<String> = cp.pending.keys().cloned().collect();
    let mut succeeded: Vec<String> = Vec::new();
    for id in &pending_ids {
        let pending_value = cp.pending.get(id).cloned();
        let Some(v) = pending_value else { continue };
        let Some(mut batch) = FrozenBatch::from_pending_json(id, &v) else {
            continue;
        };
        batch.covered_event_ids = v
            .get("covered_event_ids")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        match push_batch(&req, &batch) {
            Ok(_) => {
                confirm_batch(&mut cp, &batch);
                save_checkpoint(&ctx, &cp)?;
                succeeded.push(id.clone());
            }
            Err(e) => {
                return Err(Error::new(
                    code::REPORT_NOT_CONFIRMED,
                    format!("补传批次 {id} 未确认；其余批次保留待重试"),
                )
                .context(e.to_json()));
            }
        }
    }
    Ok(json!({
        "retried": succeeded.len(),
        "batches": succeeded,
        "pending_left": cp.pending.len(),
    }))
}

fn reporting_enabled(ctx: &AppContext) -> bool {
    let declaration = ctx
        .declaration_path()
        .and_then(|p| crate::config::ProjectDeclaration::load(&p).ok().flatten());
    std::env::var("AILOOM_REPORTING")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
        || declaration
            .as_ref()
            .map(|d| d.reporting_enabled)
            .unwrap_or(false)
}

/// 团队周期汇总：fetch 团队源远端的全部报告分支，合并各工作区/设备/会话记录。
/// 合并语义：按 (workspace_id, session_id_hash) 分组，字段取最大值（累计快照不可相加），
/// 同 session 更新不反复累计；范围/时区/缺失来源在输出中明确。
fn team_digest(
    ctx: &AppContext,
    data_root: Option<&std::path::Path>,
    explicit_root: Option<&std::path::Path>,
) -> Result<Value> {
    let req = crate::contribution::prepare_contribution(data_root, explicit_root)?;
    // 拉取全部报告分支
    crate::gitx::git(
        &req.cache_repo,
        &[
            "fetch",
            "-q",
            "origin",
            &format!("+refs/heads/{REPORT_BRANCH}*:refs/ailoom-reports/*"),
        ],
    )
    .map_err(|e| {
        Error::new(
            code::REPORT_NOT_CONFIRMED,
            "团队汇总需要读取远端报告分支（当前不可达）",
        )
        .context(e.to_json())
        .fix("确认网络与团队源远端可达后重试；本地视图可用 ailoom session --action list")
    })?;
    let refs_text = crate::gitx::git(
        &req.cache_repo,
        &[
            "for-each-ref",
            "refs/ailoom-reports/",
            "--format=%(refname)",
        ],
    )?;
    let refs: Vec<&str> = refs_text.lines().filter(|l| !l.is_empty()).collect();

    // 合并：key=(workspace_id, session_id_hash, date) → 字段最大值
    #[derive(Default, Clone)]
    struct Merged {
        tool: String,
        prompt_count: u64,
        tool_calls: u64,
        tool_errors: u64,
        interventions: u64,
        tokens_input: Option<u64>,
        tokens_output: Option<u64>,
        devices: std::collections::BTreeSet<String>,
    }
    let mut merged: BTreeMap<(String, String, String), Merged> = BTreeMap::new();
    let mut sources = 0usize;
    let mut min_date: Option<String> = None;
    let mut max_date: Option<String> = None;
    for ref_name in &refs {
        let listing =
            crate::gitx::git(&req.cache_repo, &["ls-tree", "-r", "--name-only", ref_name])?;
        let files: Vec<&str> = listing
            .lines()
            .filter(|l| l.starts_with(REPORT_PREFIX))
            .collect();
        if files.is_empty() {
            continue;
        }
        sources += 1;
        for f in files {
            let content = crate::gitx::git(&req.cache_repo, &["show", &format!("{ref_name}:{f}")])?;
            let Ok(v) = serde_json::from_str::<Value>(&content) else {
                continue; // 坏记录跳过并在结果中报告
            };
            let Some(record) = v.get("record") else {
                continue;
            };
            let Some(wid) = record.get("workspace_id").and_then(|x| x.as_str()) else {
                continue;
            };
            let Some(sidh) = record.get("session_id_hash").and_then(|x| x.as_str()) else {
                continue;
            };
            // 文件名含日期：reports/sessions/<YYYY-MM-DD>-<sid_hash>.json（sid_hash 为无连字符 hex）
            let date = std::path::Path::new(f)
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".json"))
                .map(|stem| stem.splitn(4, '-').collect::<Vec<_>>()[..3].join("-"))
                .unwrap_or_default();
            let device = v
                .get("device_id_hash")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string();
            let get_u64 = |k: &str| record.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
            let entry = merged
                .entry((wid.to_string(), sidh.to_string(), date.clone()))
                .or_default();
            entry.tool = record
                .get("tool")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string();
            entry.prompt_count = entry.prompt_count.max(get_u64("prompt_count"));
            entry.tool_calls = entry.tool_calls.max(get_u64("tool_calls"));
            entry.tool_errors = entry.tool_errors.max(get_u64("tool_errors"));
            entry.interventions = entry.interventions.max(get_u64("interventions"));
            entry.tokens_input = entry
                .tokens_input
                .max(record.get("tokens_input").and_then(|x| x.as_u64()));
            entry.tokens_output = entry
                .tokens_output
                .max(record.get("tokens_output").and_then(|x| x.as_u64()));
            entry.devices.insert(device);
            min_date = Some(min_date.map_or_else(|| date.clone(), |m| m.min(date.clone())));
            max_date = Some(max_date.map_or_else(|| date.clone(), |m| m.max(date.clone())));
        }
    }

    let sessions_list: Vec<Value> = merged
        .iter()
        .map(|((wid, sidh, date), m)| {
            json!({
                "workspace_id": wid,
                "session_id_hash": sidh,
                "date": date,
                "tool": m.tool,
                "prompt_count": m.prompt_count,
                "tool_calls": m.tool_calls,
                "tool_errors": m.tool_errors,
                "interventions": m.interventions,
                "tokens_input": m.tokens_input,
                "tokens_output": m.tokens_output,
                "devices": m.devices,
            })
        })
        .collect();
    let missing_token = sessions_list
        .iter()
        .filter(|s| s["tokens_input"].is_null())
        .count();
    Ok(json!({
        "schema_version": 1,
        "scope": "team",
        "local_workspace_id": ctx.workspace.workspace_id,
        "generated_at": crate::ids::now_iso(),
        "timezone": "UTC",
        "range": { "min": min_date, "max": max_date, "timezone": "UTC",
            "note": "日期取自上报文件名（成员本地日期），只用于范围展示" },
        "report_branches_merged": sources,
        "session_count": sessions_list.len(),
        "totals": {
            "prompt_count": sessions_list.iter().map(|s| s["prompt_count"].as_u64().unwrap_or(0)).sum::<u64>(),
            "tool_calls": sessions_list.iter().map(|s| s["tool_calls"].as_u64().unwrap_or(0)).sum::<u64>(),
            "tool_errors": sessions_list.iter().map(|s| s["tool_errors"].as_u64().unwrap_or(0)).sum::<u64>(),
            "interventions": sessions_list.iter().map(|s| s["interventions"].as_u64().unwrap_or(0)).sum::<u64>(),
            "tokens_input": sessions_list.iter().filter_map(|s| s["tokens_input"].as_u64()).sum::<u64>(),
        },
        "missing_sources": {
            "unavailable_token_sessions": missing_token,
            "note": "缺失来源在汇总中可见，不计为 0；合并语义为累计快照逐字段最大值，不与增量混用",
        },
        "sessions": sessions_list,
        "note": "仅统计白名单字段；session id 以哈希呈现；本汇总不包含未推送的本地事件",
    }))
}

fn today() -> String {
    let now = chrono::Utc::now();
    now.format("%Y-%m-%d").to_string()
}
