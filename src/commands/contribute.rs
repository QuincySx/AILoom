//! contribute 命令（AIL-015）：经验文档 → 明确归属 → 贡献审核路径。
//! 失败保留草稿；同 ID 重试不重复创建。

use crate::appctx::AppContext;
use crate::contribution::{self, ContributionRequest};
use crate::error::{code, Error, Result};
use crate::learning::{decide_target, parse, render_source_file, stable_id, LearningTarget};
use crate::manifest::TeamManifest;
use crate::source::SourcesLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub struct ContributeArgs {
    pub file: PathBuf,
    pub project: Option<String>,
    pub shared: bool,
    pub namespace: Option<String>,
    pub message: String,
    pub provider: String,
    pub root: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LearningRecord {
    schema_version: u32,
    /// learning id → 提交状态
    contributed: std::collections::BTreeMap<String, ContributedInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContributedInfo {
    branch: String,
    changeset_id: String,
    status: String,
    at: String,
}

fn record_path(ctx: &AppContext) -> PathBuf {
    ctx.layout.ws_dir.join("learnings.json")
}

fn load_record(ctx: &AppContext) -> LearningRecord {
    std::fs::read_to_string(record_path(ctx))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(LearningRecord {
            schema_version: 1,
            contributed: Default::default(),
        })
}

/// 简化：为已有内容直接构造变更集文件（不经 from_dir 复制）。
struct ContributionInput {
    rel_path: String,
    content: String,
}

pub fn run(
    args: &ContributeArgs,
    json: bool,
    data_root: Option<&std::path::Path>,
) -> Result<Value> {
    let req = contribution::prepare_contribution(data_root, args.root.as_deref())?;
    let ContributionRequest {
        ctx, declaration, ..
    } = &req;

    // 1. 解析经验文档（失败保留草稿：原文件不动）
    let text = std::fs::read_to_string(&args.file).map_err(|e| {
        Error::new(
            code::USAGE,
            format!("无法读取经验文档 {}: {e}", args.file.display()),
        )
    })?;
    let doc = parse(&text)?;
    let id = stable_id(&doc);

    // 2. 归属决策
    let target = decide_target(args.project.as_deref(), args.shared, &declaration.projects)?;

    // 3. 重试去重：同 ID 已推送则直接返回既有信息
    let mut record = load_record(ctx);
    if let Some(existing) = record.contributed.get(&id) {
        if existing.status == "pushed" || existing.status == "pr-created" {
            let value = serde_json::json!({
                "learning_id": id,
                "deduplicated": true,
                "branch": existing.branch,
                "changeset_id": existing.changeset_id,
                "status": existing.status,
            });
            if !json {
                crate::logging::info(format!("经验 {id} 已在变更集中，未重复创建"));
            }
            return Ok(value);
        }
    }

    // 4. namespace 决策与清单校验
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(crate::workspace::AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = SourcesLock::load(&lock_path)?;
    let entry = lock
        .and_then(|l| l.sources.get(&declaration.source.name).cloned())
        .ok_or_else(|| Error::new(code::SOURCE_NOT_CACHED, "源未锁定"))?;
    // 从缓存清单读取 namespace 与 learnings 目录
    let src_git = crate::source::GitSource::new(
        entry.identity.trim_start_matches("git+"),
        entry.ref_.as_deref(),
    )?;
    let snapshot = src_git.resolve(&ctx.source_cache(&src_git.identity), Some(&entry))?;
    let manifest = TeamManifest::load_from(&snapshot.root)?;
    let namespace = match &args.namespace {
        Some(ns) => {
            if !manifest.namespaces.known.contains(ns) {
                return Err(Error::new(
                    code::UNKNOWN_REFERENCE,
                    format!("namespace `{ns}` 未在团队清单声明"),
                )
                .context(serde_json::json!({ "known": manifest.namespaces.known })));
            }
            ns.clone()
        }
        None => match &target {
            LearningTarget::Shared => {
                manifest.namespaces.shared.first().cloned().ok_or_else(|| {
                    Error::new(
                        code::UNKNOWN_REFERENCE,
                        "清单无共享 namespace，请显式指定 --namespace",
                    )
                })?
            }
            LearningTarget::Project(_) => manifest
                .namespaces
                .known
                .iter()
                .find(|k| !manifest.namespaces.shared.contains(k))
                .cloned()
                .ok_or_else(|| {
                    Error::new(
                        code::UNKNOWN_REFERENCE,
                        "清单无非共享 namespace，请显式指定 --namespace",
                    )
                })?,
        },
    };
    if let LearningTarget::Project(p) = &target {
        manifest.require_project(p)?;
    }

    // 5. 组装源内文件并提交
    let content = render_source_file(&doc, &id, &target, &namespace);
    let rel = format!("{}/{}.md", manifest.effective_paths().learnings, id);
    let input = ContributionInput {
        rel_path: rel.clone(),
        content,
    };

    let value = submit_input(&req, &input, &args.message, &args.provider, &id)?;

    // 6. 记录
    record.contributed.insert(
        id.clone(),
        ContributedInfo {
            branch: value["branch"].as_str().unwrap_or_default().to_string(),
            changeset_id: value["changeset_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            status: if value["pr_url"].is_null() {
                "pushed".into()
            } else {
                "pr-created".into()
            },
            at: crate::ids::now_iso(),
        },
    );
    crate::sync_common::atomic_write(
        record_path(ctx).as_path(),
        serde_json::to_vec_pretty(&record)?.as_slice(),
    )?;

    if !json {
        crate::logging::info(format!(
            "经验 {} 已提交审核（项目/共享: {}）",
            id,
            value["target"].as_str().unwrap_or("?")
        ));
    }
    Ok(value)
}

fn submit_input(
    req: &ContributionRequest,
    input: &ContributionInput,
    message: &str,
    provider: &str,
    id: &str,
) -> Result<Value> {
    let (wt, cs) = crate::contribution::ensure_worktree(
        &req.ctx,
        &req.cache_repo,
        &req.source_alias,
        &req.base,
    )?;
    let result = (|| -> Result<Value> {
        let dest = wt.path.join(&input.rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, input.content.as_bytes())?;
        crate::gitx::git(&wt.path, &["add", "--", &input.rel_path])?;
        let value = crate::contribution::commit_and_push(
            crate::contribution::PushEnv {
                ctx: &req.ctx,
                wt: &wt,
                cs: &cs,
                identity: &req.source_identity,
                provider,
                source_alias: &req.source_alias,
            },
            std::slice::from_ref(&input.rel_path),
            None,
            message,
        )?;
        let mut v = value;
        v["learning_id"] = serde_json::json!(id);
        v["target"] = serde_json::json!(input.rel_path);
        Ok(v)
    })();
    wt.cleanup();
    result
}

// 防止未使用告警（结构体仅作为命名载体）
#[allow(dead_code)]
fn _touch_input(_: &ContributionInput) {}
