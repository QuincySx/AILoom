//! session 命令（AIL-019/020）：指标、transcript 导入、本地摘要与显式共享。

use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use crate::events::aggregate;
use crate::events::aggregate::{parse_claude_transcript, session_identities, HeuristicConfig};
use crate::events::friction::{
    build_local_summary, build_share_record, friction_score, should_prompt, was_prompted,
    FrictionConfig,
};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct SessionArgs {
    pub action: String,
    pub session: Option<String>,
    pub file: Option<PathBuf>,
    pub share: Option<PathBuf>,
    pub root: Option<PathBuf>,
}

pub fn run(args: &SessionArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let heuristic = HeuristicConfig::load(&ctx.layout.ws_dir);
    let cfg = FrictionConfig::load(&ctx.layout.ws_dir);

    match args.action.as_str() {
        "metrics" | "summary" => {
            let (events, bad_lines) =
                crate::events::store::read_all_events(&ctx.layout.events_dir)?;
            if !json && bad_lines > 0 {
                crate::logging::warn(format!("事件文件含 {bad_lines} 个坏行（已跳过）"));
            }
            let mut out;
            match &args.session {
                Some(sid) => {
                    // RW-09/R06：有效聚合 = 实时事件（剔除已清算）+ 累计基线
                    let (m, mut identities) = aggregate::effective_session(
                        &ctx.layout.ws_dir,
                        &ctx.workspace.workspace_id,
                        sid,
                        &events,
                        &heuristic,
                    )?;
                    let mut v = json!(m);
                    // RW-06/R02：同名 session 存在多个完整身份（tool/device）时
                    // 显式呈现歧义，不静默合并
                    if identities.len() < 2 {
                        identities = session_identities(&ctx.workspace.workspace_id, sid, &events);
                    }
                    if identities.len() > 1 {
                        v["identity_ambiguous"] = json!(true);
                        v["identities"] = json!(identities);
                        v["note"] = json!("同名 session 在本工作区存在多个完整身份（tool/device）；本结果为跨身份聚合，细分见 identities");
                    }
                    if args.action == "summary" {
                        let prompted = was_prompted(&ctx.layout.summary_dir, sid);
                        v["friction"] = json!({
                            "score": friction_score(&m, &cfg),
                            "should_prompt": should_prompt(&m, &cfg),
                            "prompted_before": prompted,
                        });
                        if args.action == "summary" {
                            // 保存本地摘要（结构化，无自由文本）
                            let summary = build_local_summary(&m, &cfg);
                            let path = ctx.layout.summary_dir.join(format!("{sid}.json"));
                            crate::sync_common::atomic_write(
                                path.as_path(),
                                serde_json::to_vec_pretty(&summary)?.as_slice(),
                            )?;
                            v["summary_path"] = json!(path.display().to_string());
                        }
                    }
                    out = v;
                }
                None => {
                    // RW-09/R06：有效聚合 = 实时事件 + 清理前累计基线
                    let all = aggregate::effective_all(
                        &ctx.layout.ws_dir,
                        &ctx.workspace.workspace_id,
                        &events,
                        &heuristic,
                    )?;
                    out = json!({ "sessions": all.values().collect::<Vec<_>>() });
                }
            }
            // 共享：显式独立动作，白名单重建
            if let Some(share_path) = &args.share {
                let sid = args
                    .session
                    .as_ref()
                    .ok_or_else(|| Error::new(code::USAGE, "--share 需要指定 --session <id>"))?;
                let (m, _) = aggregate::effective_session(
                    &ctx.layout.ws_dir,
                    &ctx.workspace.workspace_id,
                    sid,
                    &events,
                    &heuristic,
                )?;
                let record = build_share_record(&m);
                crate::sync_common::atomic_write(
                    share_path.as_path(),
                    serde_json::to_vec_pretty(&record)?.as_slice(),
                )?;
                out["shared_to"] = json!(share_path.display().to_string());
                if !json {
                    crate::logging::info(format!(
                        "共享记录（仅计数与工具名）已写入 {}",
                        share_path.display()
                    ));
                }
            }
            if !json {
                if args.action == "summary" && args.session.is_some() {
                    println!("{}", serde_json::to_string_pretty(&out)?);
                } else {
                    println!(
                        "会话数: {}",
                        out["sessions"].as_array().map(|a| a.len()).unwrap_or(1)
                    );
                }
            }
            // 无事件的显式不足说明
            if args.action == "summary" && events.is_empty() {
                out["note"] = json!("本会话没有采集到事件：无法生成有效摘要（数据不足）");
            }
            Ok(out)
        }
        "ingest" => {
            // transcript 导入：显式提供的文件（默认不扫描历史）
            let file = args
                .file
                .as_ref()
                .ok_or_else(|| Error::new(code::USAGE, "ingest 需要 --file <transcript.jsonl>"))?;
            let events = parse_claude_transcript(file, &ctx.workspace.workspace_id, &ctx.device)?;
            let mut appended = 0usize;
            for e in &events {
                if crate::events::store::append_event(&ctx.layout.events_file, e)? {
                    appended += 1;
                }
            }
            let value = json!({ "parsed": events.len(), "appended": appended, "deduplicated": events.len() - appended });
            if !json {
                crate::logging::info(format!(
                    "导入 {} 条 token 快照，新增 {}，去重 {}",
                    events.len(),
                    appended,
                    events.len() - appended
                ));
            }
            Ok(value)
        }
        other => Err(Error::new(
            code::USAGE,
            format!("未知 session 动作: {other}"),
        )),
    }
}

/// 摩擦提示决策（供 hook stop 事件调用）：只读决策，是否达到阈值且尚未提示。
/// 实际的“已提示”状态由投递方通过 `claim_prompted` 原子认领，避免
/// “已标记却无可见提示”或双路径竞争。
pub fn friction_check_for_session(ctx: &AppContext, session_id: &str) -> Result<Value> {
    let cfg = FrictionConfig::load(&ctx.layout.ws_dir);
    let heuristic = HeuristicConfig::load(&ctx.layout.ws_dir);
    let (events, _) = crate::events::store::read_all_events(&ctx.layout.events_dir)?;
    // RW-09/R06：摩擦统计同样消费累计基线（清理后仍可提示/统计）
    let m = aggregate::effective_session(
        &ctx.layout.ws_dir,
        &ctx.workspace.workspace_id,
        session_id,
        &events,
        &heuristic,
    )
    .map(|(m, _)| m);
    match m {
        Err(_) => Ok(json!({ "prompt": false, "note": "无事件数据" })),
        Ok(m) => {
            let already = was_prompted(&ctx.layout.summary_dir, session_id);
            Ok(json!({
                "prompt": should_prompt(&m, &cfg) && !already,
                "score": friction_score(&m, cfg_clone(&cfg)),
                "interventions": m.interventions,
                "tool_errors": m.tool_errors,
                "corrections": m.corrections_heuristic,
                "prompt_enabled": cfg.prompt_enabled,
            }))
        }
    }
}

fn cfg_clone(cfg: &FrictionConfig) -> &FrictionConfig {
    cfg
}
