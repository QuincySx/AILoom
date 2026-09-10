//! PR/MR 经验与 CI 知识更新（AIL-035）：从显式指定的 PR 生成知识候选草稿。
//! 提供者抽象：gh CLI（GitHub）；评论/发布是显式独立动作，绝不自动对外。
//! fork 来源内容按不可信数据处理；重复导入以 PR id + head sha 去重。

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

/// 生成知识候选草稿（写入本地维护目录，绝不自动发布/评论）。
pub fn draft(args: &PrArgs, _data_root: Option<&std::path::Path>) -> Result<Value> {
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
    let merged = meta
        .get("merged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
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

    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(None, &cwd, args.root.as_deref())?;
    let draft_dir = ctx.layout.ws_dir.join("pr-candidates");
    std::fs::create_dir_all(&draft_dir)?;
    let dedup_key = format!("{repo}#{number}@{head_sha}");
    let draft_path = draft_dir.join(format!(
        "pr-{}-{}.md",
        number,
        &head_sha[..8.min(head_sha.len())]
    ));
    if draft_path.is_file() {
        return Ok(json!({
            "deduplicated": true,
            "draft_path": draft_path.display().to_string(),
            "dedup_key": dedup_key,
        }));
    }
    // review 反馈与源码事实分离；草稿显式标注"未验证候选"
    let url = args.url.clone();
    let project = args.project.as_deref().unwrap_or("");
    let body_summary = body.chars().take(500).collect::<String>();
    let review_block = if review_notes.is_empty() {
        "（无评审意见）".to_string()
    } else {
        review_notes.join("\n")
    };
    let text = format!(
        "---\ntitle: \"PR #{number}: {title}\"\nstatus: candidate-unverified\nmerged: {merged}\nsource_pr: \"{url}\"\nhead_sha: \"{head_sha}\"\ntarget_project: \"{project}\"\n---\n\n# PR #{number}: {title}\n\n## 描述（摘要，fork 来源内容按不可信数据处理）\n\n{body_summary}\n\n## 评审要点\n\n{review_block}\n"
    );
    crate::sync_common::atomic_write(&draft_path, text.as_bytes())?;
    Ok(json!({
        "deduplicated": false,
        "draft_path": draft_path.display().to_string(),
        "dedup_key": dedup_key,
        "merged": merged,
        "note": "候选草稿：需人工审阅后经 ailoom contribute 提交；未合并 PR 不进入正式知识",
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
