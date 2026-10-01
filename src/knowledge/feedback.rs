//! 知识反馈、使用统计与维护（AIL-028）。
//! 只有真实召回可贡献使用指标；维护先预览（dry-run）；低频不直接等于无价值。

use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub schema_version: u32,
    /// learning id → 使用计数（仅由 recall 返回后记录，未召回 ID 拒绝）
    pub usage: BTreeMap<String, u64>,
    /// 显式反馈：id → (useful 次数, not_useful 次数)
    pub feedback: BTreeMap<String, (u64, u64)>,
    /// 反馈事件身份（AIL-028）：事件 id → "{learning_id}:{direction}"；
    /// 同一事件重试只计一次（幂等），无显式事件 id 时每次调用生成新 id
    #[serde(default)]
    pub feedback_events: BTreeMap<String, String>,
}

fn usage_path(ctx: &AppContext) -> PathBuf {
    ctx.layout.ws_dir.join("knowledge-usage.json")
}

fn load(ctx: &AppContext) -> UsageRecord {
    std::fs::read_to_string(usage_path(ctx))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(UsageRecord {
            schema_version: 1,
            usage: Default::default(),
            feedback: Default::default(),
            feedback_events: Default::default(),
        })
}

fn save(ctx: &AppContext, r: &UsageRecord) -> Result<()> {
    crate::sync_common::atomic_write(
        usage_path(ctx).as_path(),
        serde_json::to_vec_pretty(r)?.as_slice(),
    )
}

/// 记录一次真实召回使用（由 recall 命令返回结果后调用）。
pub fn record_recall_hits(ctx: &AppContext, ids: &[String]) -> Result<()> {
    let mut r = load(ctx);
    for id in ids {
        *r.usage.entry(id.clone()).or_insert(0) += 1;
    }
    save(ctx, &r)
}

/// 记录显式反馈（有用/无用）。幂等语义：调用方提供稳定 `feedback_id`（如
/// CI 事件 id）时，同一事件重复送达只计一次；未提供时每次调用生成独立事件 id。
pub fn record_feedback(
    ctx: &AppContext,
    id: &str,
    useful: bool,
    feedback_id: Option<&str>,
) -> Result<()> {
    let mut r = load(ctx);
    let event_id = feedback_id
        .map(str::to_string)
        .unwrap_or_else(|| format!("fb-{}", crate::ids::new_id()));
    let direction = if useful { "useful" } else { "not-useful" };
    let event_key = format!("{id}:{direction}");
    if let Some(seen) = r.feedback_events.get(&event_id) {
        if seen == &event_key {
            // 同一反馈事件重试：幂等跳过
            return Ok(());
        }
    }
    r.feedback_events.insert(event_id, event_key);
    let entry = r.feedback.entry(id.to_string()).or_insert((0, 0));
    if useful {
        entry.0 += 1;
    } else {
        entry.1 += 1;
    }
    save(ctx, &r)
}

/// 归档清单路径：`<ws_dir>/archived-learnings.json`（字符串数组）。
fn archive_path(ctx: &AppContext) -> PathBuf {
    ctx.layout.ws_dir.join("archived-learnings.json")
}

fn load_archive_list(ctx: &AppContext) -> Vec<String> {
    std::fs::read_to_string(archive_path(ctx))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default()
}

fn save_archive_list(ctx: &AppContext, ids: &[String]) -> Result<()> {
    crate::sync_common::atomic_write(
        archive_path(ctx).as_path(),
        serde_json::to_vec_pretty(&ids)?.as_slice(),
    )
}

/// 归档：把经验加入归档清单（索引消费后从召回可见集合移除）；幂等。
pub fn archive(ctx: &AppContext, id: &str) -> Result<Vec<String>> {
    let mut list = load_archive_list(ctx);
    if !list.iter().any(|x| x == id) {
        list.push(id.to_string());
        save_archive_list(ctx, &list)?;
    }
    Ok(list)
}

/// 恢复：从归档清单移除，重新进入召回可见集合；幂等。
pub fn restore(ctx: &AppContext, id: &str) -> Result<Vec<String>> {
    let mut list = load_archive_list(ctx);
    list.retain(|x| x != id);
    save_archive_list(ctx, &list)?;
    Ok(list)
}

/// 归档记录：从索引可见集合移除（标记 archived），可恢复。
#[derive(Debug, Serialize)]
pub struct MaintenancePlan {
    pub dry_run: bool,
    pub archive_candidates: Vec<ArchiveCandidate>,
}

#[derive(Debug, Serialize)]
pub struct ArchiveCandidate {
    pub id: String,
    pub usage_count: u64,
    pub useful: u64,
    pub not_useful: u64,
    pub reason: String,
}

/// 维护计划（dry-run）：识别候选（陈旧或明确无用），绝不把低频直接等同无价值：
/// 仅当（明确无用反馈 > 有用反馈）或（已归档标记）才成为候选；低使用量仅提示。
pub fn build_maintenance_plan(ctx: &AppContext, _min_usage_hint: u64) -> Result<MaintenancePlan> {
    let r = load(ctx);
    Ok(build_maintenance_plan_from_usage(&r))
}

/// 纯逻辑：从使用记录构建维护计划。
pub fn build_maintenance_plan_from_usage(r: &UsageRecord) -> MaintenancePlan {
    let mut candidates = Vec::new();
    for (id, (useful, not_useful)) in &r.feedback {
        if *not_useful > *useful {
            candidates.push(ArchiveCandidate {
                id: id.clone(),
                usage_count: *r.usage.get(id).unwrap_or(&0),
                useful: *useful,
                not_useful: *not_useful,
                reason: "显式无用反馈多于有用".into(),
            });
        }
    }
    MaintenancePlan {
        dry_run: true,
        archive_candidates: candidates,
    }
}

/// 归档：在源中为经验文档打 archived 标记 → 经贡献审核路径生效。
/// 返回变更集描述（此处仅生成计划文档；提交由贡献命令复用）。
pub fn archive_draft(ctx: &AppContext, id: &str) -> Result<(PathBuf, String)> {
    let declaration = ctx
        .declaration_path()
        .and_then(|p| crate::config::ProjectDeclaration::load(&p).ok().flatten())
        .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "工作区未绑定"))?;
    let _ = declaration;
    let draft_dir = ctx.layout.ws_dir.join("maintenance");
    std::fs::create_dir_all(&draft_dir)?;
    let draft = draft_dir.join(format!("archive-{id}.md"));
    let text = format!(
        "---\ntitle: 归档建议 {id}\n---\n\n建议归档经验 `{id}`（低价值信号）。请人工审阅后删除或合并原文档。\n"
    );
    crate::sync_common::atomic_write(&draft, text.as_bytes())?;
    Ok((draft, text))
}

/// 晋升草稿：把经验改写为规则草稿（保留原 LearningId 与证据），走审核发布。
pub fn promotion_draft(ctx: &AppContext, learning_id: &str, rule_text: &str) -> Result<PathBuf> {
    let draft_dir = ctx.layout.ws_dir.join("maintenance");
    std::fs::create_dir_all(&draft_dir)?;
    let p = draft_dir.join(format!("promote-{learning_id}.md"));
    let text = format!(
        "---\nname: rule-from-{learning_id}\ndescription: 由经验 {} 晋升（保留来源）\nshared: false\nnamespace: common\nsource_learning: {learning_id}\n---\n\n{rule_text}\n",
        learning_id
    );
    crate::sync_common::atomic_write(&p, text.as_bytes())?;
    Ok(p)
}

/// 索引与删除一致性：归档后的知识重建索引时被排除（由 index build 读取归档清单）。
pub fn archived_ids(ctx: &AppContext) -> Vec<String> {
    let p = ctx.layout.ws_dir.join("archived-learnings.json");
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// knowledge 命令入口（feedback / maintenance / promote）。
pub struct KnowledgeArgs {
    pub action: String,
    pub id: Option<String>,
    pub useful: bool,
    pub text: Option<String>,
    /// 稳定反馈事件身份：同一 id 重试幂等（AIL-028）
    pub feedback_id: Option<String>,
    pub root: Option<PathBuf>,
}

pub fn run(args: &KnowledgeArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let data = crate::paths::resolve_data_root(data_root)?;
    let ctx =
        match super::location::maintenance_context(&data, args.root.as_deref().unwrap_or(&cwd))? {
            Some(ctx) => {
                if args.id.as_deref().is_some_and(|id| {
                    std::path::Path::new(id)
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                }) {
                    return Err(Error::new(code::USAGE, "知识 ID 必须是库内相对路径"));
                }
                ctx
            }
            None => AppContext::discover(data_root, &cwd, args.root.as_deref())?,
        };
    match args.action.as_str() {
        "feedback" => {
            let id = args
                .id
                .as_deref()
                .ok_or_else(|| Error::new(code::USAGE, "feedback 需要 --id <learning id>"))?;
            // 仅允许对真实召回过的 ID 反馈（未召回 ID 投票不增加使用量）
            let r = load(&ctx);
            if !r.usage.contains_key(id) {
                return Err(Error::new(
                    code::USAGE,
                    format!("`{id}` 未在本工作区召回过，不能反馈"),
                ));
            }
            let duplicated = match args.feedback_id.as_deref() {
                Some(fid) => {
                    let direction = if args.useful { "useful" } else { "not-useful" };
                    r.feedback_events
                        .get(fid)
                        .map(|seen| seen == &format!("{id}:{direction}"))
                        .unwrap_or(false)
                }
                None => false,
            };
            record_feedback(&ctx, id, args.useful, args.feedback_id.as_deref())?;
            Ok(serde_json::json!({
                "id": id,
                "useful": args.useful,
                "duplicated": duplicated,
                "feedback_id": args.feedback_id,
            }))
        }
        "archive" => {
            let id = args
                .id
                .as_deref()
                .ok_or_else(|| Error::new(code::USAGE, "archive 需要 --id <learning id>"))?;
            let list = archive(&ctx, id)?;
            Ok(serde_json::json!({
                "id": id,
                "archived": list,
                "note": "归档后索引召回排除该经验；restore 可恢复",
            }))
        }
        "restore" => {
            let id = args
                .id
                .as_deref()
                .ok_or_else(|| Error::new(code::USAGE, "restore 需要 --id <learning id>"))?;
            let list = restore(&ctx, id)?;
            Ok(serde_json::json!({
                "id": id,
                "archived": list,
                "note": "已恢复召回可见性",
            }))
        }
        "maintenance" => {
            let plan = build_maintenance_plan(&ctx, 3)?;
            let value = serde_json::json!({
                "dry_run": plan.dry_run,
                "archive_candidates": plan.archive_candidates,
            });
            if !json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            Ok(value)
        }
        "promote" => {
            let id = args.id.as_deref().ok_or_else(|| {
                Error::new(
                    code::USAGE,
                    "promote 需要 --id <learning id> 与 --text <规则草稿>",
                )
            })?;
            let text = args
                .text
                .as_deref()
                .ok_or_else(|| Error::new(code::USAGE, "promote 需要 --text <规则草稿>"))?;
            let path = promotion_draft(&ctx, id, text)?;
            Ok(serde_json::json!({
                "draft_path": path.display().to_string(),
                "note": "草稿走 ailoom contribute 审核路径；保留原 LearningId",
                "source_learning": id,
            }))
        }
        other => Err(Error::new(
            code::USAGE,
            format!("未知 knowledge 动作: {other}"),
        )),
    }
}
