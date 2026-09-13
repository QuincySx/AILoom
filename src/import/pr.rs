//! PR/MR 经验与 CI 知识更新（AIL-035）：从显式指定的 PR 生成知识候选草稿。
//! 提供者抽象：gh CLI（GitHub）；评论/发布是显式独立动作，绝不自动对外。
//! fork 来源内容按不可信数据处理。
//!
//! 候选身份 = provider/repo/PR 号/完整 head sha（文件名含 repo，head 取前 8 位可读）。
//! 状态转换：新 head 使同 PR 旧候选失效（candidate-stale，可查询不可当最新）；
//! 同 head 合并状态变化刷新（candidate-merged），合并后幂等推进对应项目代码图基线；
//! 关闭未合并（pr_state=closed）不进入正式知识。

use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

pub struct PrArgs {
    pub action: String, // draft
    pub url: String,
    pub project: Option<String>,
    pub root: Option<PathBuf>,
}

/// 解析 GitHub PR URL → (owner/repo, number)。
pub fn parse_pr_url(url: &str) -> Result<(String, u64)> {
    let trimmed = url.trim().trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .ok_or_else(|| Error::new(code::USAGE, "仅支持 GitHub PR URL（初版单 provider）"))?;
    let mut segs = rest.split('/');
    let owner = segs.next().unwrap_or_default();
    let repo = segs.next().unwrap_or_default();
    let keyword = segs.next().unwrap_or_default();
    let num = segs.next().unwrap_or_default();
    if keyword != "pull" {
        return Err(Error::new(
            code::USAGE,
            "URL 不是 PR（应为 /OWNER/REPO/pull/N）",
        ));
    }
    let num: u64 = num
        .parse()
        .map_err(|_| Error::new(code::USAGE, "PR 编号非法"))?;
    Ok((format!("{owner}/{repo}"), num))
}

/// 通过 gh CLI 拉取 PR 元数据/差异/评审（需本机 gh 已认证）。
fn gh_json(repo: &str, args: &[&str]) -> Result<Value> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{repo}/{}", args.join("/"))])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| Error::new(code::PR_CREATE_FAILED, format!("gh 不可用: {e}")))?;
    if !output.status.success() {
        return Err(Error::new(
            code::PR_CREATE_FAILED,
            format!(
                "gh api 失败：{}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    serde_json::from_str(&String::from_utf8_lossy(&output.stdout))
        .map_err(|e| Error::new(code::PR_CREATE_FAILED, format!("gh 响应解析失败: {e}")))
}

/// 候选文件名前缀：provider + repo + PR 号（跨仓同号不冲突）。
fn candidate_prefix(repo: &str, number: u64) -> String {
    let repo_part = repo.replace('/', "__");
    format!("pr-github-{repo_part}-{number}")
}

/// 渲染候选草稿（serde_yaml 规范序列化：引号/换行标题安全）。
#[allow(clippy::too_many_arguments)]
fn render_draft(
    number: u64,
    title: &str,
    status: &str,
    pr_state: &str,
    merged: bool,
    url: &str,
    head_sha: &str,
    project: &str,
    body_summary: &str,
    review_block: &str,
) -> Result<String> {
    let mut fm = serde_yaml::Mapping::new();
    fm.insert(
        serde_yaml::Value::from("title"),
        serde_yaml::Value::from(format!("PR #{number}: {title}")),
    );
    fm.insert(
        serde_yaml::Value::from("status"),
        serde_yaml::Value::from(status),
    );
    fm.insert(
        serde_yaml::Value::from("pr_state"),
        serde_yaml::Value::from(pr_state),
    );
    fm.insert(
        serde_yaml::Value::from("merged"),
        serde_yaml::Value::from(merged),
    );
    fm.insert(
        serde_yaml::Value::from("source_pr"),
        serde_yaml::Value::from(url),
    );
    fm.insert(
        serde_yaml::Value::from("head_sha"),
        serde_yaml::Value::from(head_sha),
    );
    fm.insert(
        serde_yaml::Value::from("target_project"),
        serde_yaml::Value::from(project),
    );
    let serialized = serde_yaml::to_string(&fm)
        .map_err(|e| Error::new(code::RENDER_FAILED, format!("候选草稿序列化失败: {e}")))?;
    let heading_title = title.replace(['\r', '\n'], " ");
    Ok(format!(
        "---\n{serialized}---\n\n# PR #{number}: {heading_title}\n\n## 描述（摘要，fork 来源内容按不可信数据处理）\n\n{body_summary}\n\n## 评审要点\n\n{review_block}\n"
    ))
}

/// 把同 PR 的其他 head 候选标记为过期（force-push 后旧候选可查询但不可当最新）。
fn mark_stale_candidates(draft_dir: &std::path::Path, prefix: &str, current_head8: &str) -> usize {
    let Ok(entries) = std::fs::read_dir(draft_dir) else {
        return 0;
    };
    let mut marked = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with(prefix) || !name.ends_with(".md") {
            continue;
        }
        // 当前 head 的候选不标记
        let stem = name.trim_end_matches(".md");
        if stem.ends_with(current_head8) {
            continue;
        }
        let path = e.path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if text.contains("status: candidate-stale") {
            continue;
        }
        let updated = text
            .replacen("status: candidate-unverified", "status: candidate-stale", 1)
            .replacen("status: candidate-merged", "status: candidate-stale", 1);
        if updated != text && crate::sync_common::atomic_write(&path, updated.as_bytes()).is_ok() {
            marked += 1;
        }
    }
    marked
}

/// 合并后推进代码图基线（幂等）：同一 (repo, PR, head) 只触发一次。
/// 图对象是当前工作区扫描（与 `code --action build` 同一入口语义），
/// 基线记录把合并事件（provider/repo/PR/head）与项目图谱推进关联起来。
fn advance_graph_baseline(
    ctx: &AppContext,
    repo: &str,
    number: u64,
    head_sha: &str,
    project: &str,
) -> Result<Value> {
    let marker_path = ctx.layout.ws_dir.join("graph-baseline.jsonl");
    let identity = format!("{repo}#{number}@{head_sha}");
    if marker_path.is_file() {
        let text = std::fs::read_to_string(&marker_path)?;
        if text.lines().any(|l| l.contains(&identity)) {
            return Ok(json!({ "graph_baseline": "already-advanced", "identity": identity }));
        }
    }
    // 幂等标记先行落盘（写入失败不推进，避免半状态）
    if let Some(parent) = marker_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&marker_path)?;
        writeln!(
            f,
            "{}",
            json!({
                "identity": identity,
                "project": project,
                "merged_head": head_sha,
                "at": crate::ids::now_iso(),
            })
        )?;
        f.flush()?;
    }
    // 推进项目图（与 code build 相同入口：增量扫描当前工作区）
    let declaration = ctx
        .declaration_path()
        .and_then(|p| crate::config::ProjectDeclaration::load(&p).ok().flatten());
    let graph_value = match declaration {
        Some(d) if !d.projects.is_empty() => {
            let project = d.projects.first().cloned().unwrap_or_default();
            let graph_path = ctx
                .layout
                .index_dir
                .join(format!("codegraph-{project}.json"));
            let mut g = crate::code_knowledge::graph::load(&graph_path)?.unwrap_or_default();
            g.schema_version = crate::code_knowledge::graph::GRAPH_SCHEMA_VERSION;
            crate::code_knowledge::graph::update_incremental(
                &mut g,
                &ctx.workspace.workspace_root,
                Some(head_sha.to_string()),
            )?;
            crate::code_knowledge::graph::save(&g, &graph_path)?;
            json!({ "project": project, "files": g.files.len() })
        }
        _ => json!({ "project": project, "skipped": "workspace 无绑定项目" }),
    };
    Ok(json!({ "graph_baseline": "advanced", "identity": identity, "graph": graph_value }))
}

/// 生成知识候选草稿（写入显式数据根下的本地维护目录，绝不自动发布/评论）。
pub fn draft(args: &PrArgs, data_root: Option<&std::path::Path>) -> Result<Value> {
    let (repo, number) = parse_pr_url(&args.url)?;
    let meta = gh_json(&repo, &["pulls", &number.to_string()])?;
    let title = meta
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let head_sha = meta
        .pointer("/head/sha")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if head_sha.is_empty() {
        return Err(Error::new(
            code::PR_CREATE_FAILED,
            "PR 元数据缺少 head sha，拒绝生成无证据候选",
        ));
    }
    let merged = meta
        .get("merged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let pr_state = meta
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("open")
        .to_string();
    let body = meta
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let reviews = gh_json(&repo, &["pulls", &number.to_string(), "reviews"])?;
    let review_notes: Vec<String> = reviews
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|r| r.get("body").and_then(|b| b.as_str()))
        .filter(|b| !b.trim().is_empty())
        .take(5)
        .map(|b| format!("- {}", b.trim()))
        .collect();

    // 显式数据根贯通（AIL-035）：候选一律写入指定/发现的 ctx 目录
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let draft_dir = ctx.layout.ws_dir.join("pr-candidates");
    std::fs::create_dir_all(&draft_dir)?;

    let prefix = candidate_prefix(&repo, number);
    let head8 = &head_sha[..8.min(head_sha.len())];
    let draft_path = draft_dir.join(format!("{prefix}-{head8}.md"));

    // 候选状态：合并→candidate-merged；其余（open/closed 未合并）→candidate-unverified，
    // 关闭未合并永不进入正式知识（仅记录 pr_state=closed 供查询）
    let status = if merged {
        "candidate-merged"
    } else {
        "candidate-unverified"
    };

    if draft_path.is_file() {
        // 同 head：合并状态变化 → 刷新；无变化 → 幂等去重
        let existing = std::fs::read_to_string(&draft_path)?;
        let existing_merged = existing.contains("merged: true");
        if existing_merged == merged {
            return Ok(json!({
                "deduplicated": true,
                "updated": false,
                "draft_path": draft_path.display().to_string(),
                "dedup_key": format!("{repo}#{number}@{head_sha}"),
                "status": status,
                "pr_state": pr_state,
            }));
        }
        let body_summary = body.chars().take(500).collect::<String>();
        let review_block = if review_notes.is_empty() {
            "（无评审意见）".to_string()
        } else {
            review_notes.join("\n")
        };
        let text = render_draft(
            number,
            &title,
            status,
            &pr_state,
            merged,
            &args.url,
            &head_sha,
            args.project.as_deref().unwrap_or(""),
            &body_summary,
            &review_block,
        )?;
        crate::sync_common::atomic_write(&draft_path, text.as_bytes())?;
        let mut baseline = json!({ "graph_baseline": "not-merged" });
        if merged {
            baseline = advance_graph_baseline(
                &ctx,
                &repo,
                number,
                &head_sha,
                args.project.as_deref().unwrap_or(""),
            )?;
        }
        return Ok(json!({
            "deduplicated": true,
            "updated": true,
            "draft_path": draft_path.display().to_string(),
            "dedup_key": format!("{repo}#{number}@{head_sha}"),
            "status": status,
            "pr_state": pr_state,
            "merged": merged,
            "note": "同 head 合并状态已刷新",
            "baseline": baseline,
        }));
    }

    // 新 head：先失效同 PR 旧候选（force-push 语义），再写新候选
    let stale_marked = mark_stale_candidates(&draft_dir, &prefix, head8);

    let body_summary = body.chars().take(500).collect::<String>();
    let review_block = if review_notes.is_empty() {
        "（无评审意见）".to_string()
    } else {
        review_notes.join("\n")
    };
    let text = render_draft(
        number,
        &title,
        status,
        &pr_state,
        merged,
        &args.url,
        &head_sha,
        args.project.as_deref().unwrap_or(""),
        &body_summary,
        &review_block,
    )?;
    crate::sync_common::atomic_write(&draft_path, text.as_bytes())?;
    let mut baseline = json!({ "graph_baseline": "not-merged" });
    if merged {
        baseline = advance_graph_baseline(
            &ctx,
            &repo,
            number,
            &head_sha,
            args.project.as_deref().unwrap_or(""),
        )?;
    }
    Ok(json!({
        "deduplicated": false,
        "updated": false,
        "draft_path": draft_path.display().to_string(),
        "dedup_key": format!("{repo}#{number}@{head_sha}"),
        "status": status,
        "pr_state": pr_state,
        "merged": merged,
        "stale_marked": stale_marked,
        "note": "候选草稿：需人工审阅后经 ailoom contribute 提交；未合并 PR 不进入正式知识",
        "baseline": baseline,
    }))
}

pub fn run(args: &PrArgs, _json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    match args.action.as_str() {
        "draft" => draft(args, data_root),
        other => Err(Error::new(
            code::USAGE,
            format!("未知 pr 动作: {other}（draft）"),
        )),
    }
}
