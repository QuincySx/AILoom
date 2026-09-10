//! 贡献与审核发布（AIL-014）：
//! 本地修改 → 变更集（源缓存的独立 worktree、独立分支）→ 精确路径提交 → 推送 → PR/手动审核。
//! 绝不触碰业务仓库的 checkout/分支；绝不 `git add .`；绝不提交机器数据。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use crate::ids::{new_id, now_iso};
use crate::source::SourcesLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

pub const CONTRIBUTION_BRANCH_PREFIX: &str = "ailoom/contribute/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Changeset {
    pub id: String,
    pub branch: String,
    pub base: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetState {
    pub schema_version: u32,
    /// 源别名 → 变更集（重试复用同一 id/分支，避免重复 PR）
    pub changesets: std::collections::BTreeMap<String, Changeset>,
}

fn state_path(ctx: &AppContext) -> PathBuf {
    ctx.layout.ws_dir.join("contributions.json")
}

fn load_state(ctx: &AppContext) -> ChangesetState {
    std::fs::read_to_string(state_path(ctx))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(ChangesetState {
            schema_version: 1,
            changesets: Default::default(),
        })
}

fn save_state(ctx: &AppContext, state: &ChangesetState) -> Result<()> {
    crate::sync_common::atomic_write(
        state_path(ctx).as_path(),
        serde_json::to_vec_pretty(state)?.as_slice(),
    )
}

/// 变更集白名单：共享资源字段（ailoom.toml + resources/）；机器数据与杂物拒绝。
pub fn contribution_allowed(rel: &str) -> bool {
    let allowed = rel == "ailoom.toml"
        || rel.starts_with("resources/")
        || rel == crate::membership::ROSTER_FILE
        || rel.starts_with("membership/");
    let denied = [
        ".ailoom",
        "machine",
        "node_modules",
        "target/",
        ".git",
        ".env",
    ];
    let junk_ext = ["log", "tmp", "swp", "bak", "pid"];
    let has_hidden = rel.split('/').any(|seg| seg.starts_with('.'));
    let has_junk_ext = rel.rsplit('.').nth(1).is_some()
        && std::path::Path::new(rel)
            .extension()
            .map(|e| junk_ext.contains(&e.to_string_lossy().as_ref()))
            .unwrap_or(false);
    allowed && !denied.iter().any(|d| rel.contains(d)) && !has_hidden && !has_junk_ext
}

/// 扫描 from 目录：返回白名单内的相对路径（字典序）。
fn scan_contribution(from: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(from)?
            .to_string_lossy()
            .to_string();
        if contribution_allowed(&rel) {
            files.push(rel);
        }
    }
    files.sort();
    Ok(files)
}

pub(crate) struct Worktree {
    pub(crate) path: PathBuf,
    pub(crate) cache_repo: PathBuf,
}

impl Worktree {
    pub(crate) fn cleanup(self) {
        let _ = crate::gitx::git(
            &self.cache_repo,
            &[
                "worktree",
                "remove",
                "--force",
                self.path.to_str().unwrap_or_default(),
            ],
        );
        let _ = crate::gitx::git(&self.cache_repo, &["worktree", "prune"]);
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 获取/创建变更集的独立 worktree（业务仓库零接触）。
pub(crate) fn ensure_worktree(
    ctx: &AppContext,
    cache_repo: &Path,
    source_alias: &str,
    base: &str,
) -> Result<(Worktree, Changeset)> {
    let mut state = load_state(ctx);
    let (id, branch) = match state.changesets.get(source_alias) {
        Some(cs) => (cs.id.clone(), cs.branch.clone()),
        None => {
            let id = new_id();
            let branch = format!("{CONTRIBUTION_BRANCH_PREFIX}{}", &id[..8]);
            (id, branch)
        }
    };
    let wt = ctx
        .data_root
        .join("ws")
        .join(&ctx.workspace.workspace_id)
        .join(format!("contribute-{}", &id[..8]));
    let _ = std::fs::remove_dir_all(&wt);
    // 分支已存在（重试）→ 复用其 tip；否则从 base 建分支
    let branch_exists = crate::gitx::git_optional(
        cache_repo,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .map(|s| !s.is_empty())
    .unwrap_or(false);
    if branch_exists {
        crate::gitx::git(
            cache_repo,
            &[
                "worktree",
                "add",
                "--detach",
                wt.to_str().unwrap_or_default(),
                &branch,
            ],
        )?;
        crate::gitx::git(&wt, &["checkout", "-q", "-B", &branch])?;
    } else {
        crate::gitx::git(
            cache_repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &branch,
                wt.to_str().unwrap_or_default(),
                base,
            ],
        )?;
    }
    let cs = Changeset {
        id,
        branch,
        base: base.to_string(),
        status: "open".into(),
        updated_at: now_iso(),
    };
    state
        .changesets
        .insert(source_alias.to_string(), cs.clone());
    save_state(ctx, &state)?;
    Ok((
        Worktree {
            path: wt,
            cache_repo: cache_repo.to_path_buf(),
        },
        cs,
    ))
}

fn gh_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn create_pr_if_possible(
    identity: &str,
    branch: &str,
    message: &str,
    provider: &str,
) -> Option<String> {
    if provider == "manual" || !identity.contains("github.com") || !gh_available() {
        return None;
    }
    let repo = identity
        .trim_start_matches("git+")
        .split_once("//")
        .map(|(_, rest)| {
            rest.trim_end_matches('.')
                .trim_end_matches("git")
                .trim_end_matches('/')
        });
    let repo = repo?;
    let output = Command::new("gh")
        .args([
            "pr", "create", "--repo", repo, "--head", branch, "--title", message, "--body", message,
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .find(|l| l.starts_with("http"))
            .map(|l| l.trim().to_string()),
        Ok(o) => {
            crate::logging::warn(format!(
                "gh pr create 未成功：{}（分支已推送，可走手动审核路径）",
                String::from_utf8_lossy(&o.stderr).trim()
            ));
            None
        }
        Err(_) => None,
    }
}

/// 提交上下文（收敛参数个数）。
pub(crate) struct PushEnv<'a> {
    pub(crate) ctx: &'a AppContext,
    pub(crate) wt: &'a Worktree,
    pub(crate) cs: &'a Changeset,
    pub(crate) identity: &'a str,
    pub(crate) provider: &'a str,
    pub(crate) source_alias: &'a str,
}

/// 提交精确路径并推送；返回 PR 链接或手动审核提示。
pub(crate) fn commit_and_push(
    env: PushEnv<'_>,
    rel_paths: &[String],
    from_dir: Option<&Path>,
    message: &str,
) -> Result<Value> {
    let (ctx, wt, cs, identity, provider, source_alias) = (
        env.ctx,
        env.wt,
        env.cs,
        env.identity,
        env.provider,
        env.source_alias,
    );
    if rel_paths.is_empty() {
        return Err(Error::new(
            code::USAGE,
            "变更集为空：没有允许贡献的修改（白名单：ailoom.toml 与 resources/）",
        ));
    }
    // 复制文件进 worktree
    for rel in rel_paths {
        let dest = wt.path.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(from) = from_dir {
            std::fs::copy(from.join(rel), &dest)?;
        }
    }
    // 精确路径 add（绝不 git add .）
    for rel in rel_paths {
        crate::gitx::git(&wt.path, &["add", "--", rel])?;
    }
    let staged = crate::gitx::git(&wt.path, &["diff", "--cached", "--name-only"])?;
    let mut no_changes = false;
    if staged.trim().is_empty() {
        // 变更集分支已包含相同内容（重试幂等）：不产生空提交
        no_changes = true;
    }
    let commit = if no_changes {
        crate::gitx::git(&wt.path, &["rev-parse", "HEAD"])?
    } else {
        crate::gitx::git(
            &wt.path,
            &[
                "-c",
                "user.name=ailoom-contribute",
                "-c",
                "user.email=contribute@ailoom.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                message,
            ],
        )?;
        crate::gitx::git(&wt.path, &["rev-parse", "HEAD"])?
    };

    // 推送；分叉 → E8101；网络/权限 → E8102 可重试
    if let Err(push_err) = crate::gitx::git(&wt.path, &["push", "-q", "origin", &cs.branch]) {
        let stderr = push_err.to_json()["context"]["stderr"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let non_ff = stderr.contains("fetch first") || stderr.contains("rejected");
        return Err(if non_ff {
            Error::new(code::CONTRIBUTION_DIVERGED, "远端分支分叉：推送被拒绝")
                .context(push_err.to_json())
                .fix(format!(
                    "重试将复用变更集 {}（分支 {}）并以本地提交覆盖；请先确认远端无他人变更",
                    cs.id, cs.branch
                ))
        } else {
            Error::new(code::PR_CREATE_FAILED, "推送失败（网络或权限）")
                .context(push_err.to_json())
                .fix("恢复后重新执行同一命令，将复用变更集重试")
        });
    }

    let pr_url = create_pr_if_possible(identity, &cs.branch, message, provider);

    // 更新变更集状态
    let mut state = load_state(ctx);
    if let Some(cs_mut) = state.changesets.get_mut(source_alias) {
        cs_mut.status = if pr_url.is_some() {
            "pr-created".into()
        } else {
            "pushed".into()
        };
        cs_mut.updated_at = now_iso();
    }
    save_state(ctx, &state)?;

    Ok(json!({
        "changeset_id": cs.id,
        "branch": cs.branch,
        "base": cs.base,
        "commit": commit.trim(),
        "committed_paths": staged.trim().lines().collect::<Vec<_>>(),
        "no_changes": no_changes,
        "pr_url": pr_url,
        "manual_review": format!(
            "请审核分支 {}（base {}）；合并后运行 ailoom init --refresh 获取已发布版本",
            cs.branch, cs.base
        ),
    }))
}

fn load_declaration(ctx: &AppContext) -> Result<ProjectDeclaration> {
    let decl_path = ctx.declaration_path().ok_or_else(|| {
        Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
    })?;
    ProjectDeclaration::load(&decl_path)?
        .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "工作区未绑定"))
}

/// 变更集运行参数（push 与 contribute 共用）。
pub struct ContributionRequest {
    pub ctx: crate::appctx::AppContext,
    pub declaration: ProjectDeclaration,
    pub source_alias: String,
    pub source_identity: String,
    pub base: String,
    pub cache_repo: PathBuf,
}

pub fn prepare_contribution(
    data_root: Option<&std::path::Path>,
    explicit_root: Option<&std::path::Path>,
) -> Result<ContributionRequest> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, explicit_root)?;
    let declaration = load_declaration(&ctx)?;
    if declaration.source.kind != "git" {
        return Err(Error::new(
            code::USAGE,
            "本地目录源没有远端可贡献；请克隆团队源为 Git 仓库后绑定",
        ));
    }
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(crate::workspace::AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = SourcesLock::load(&lock_path)?;
    let entry = lock
        .and_then(|l| l.sources.get(&declaration.source.name).cloned())
        .ok_or_else(|| Error::new(code::SOURCE_NOT_CACHED, "源未锁定").fix("先运行 ailoom init"))?;
    let base = entry
        .resolved_commit
        .clone()
        .ok_or_else(|| Error::new(code::USAGE, "本地源没有 resolved commit，无法建立变更集"))?;
    let cache_repo = ctx.source_cache(&entry.identity).join("repo");
    if !cache_repo.is_dir() {
        return Err(Error::new(
            code::SOURCE_NOT_CACHED,
            "源缓存不存在，先 ailoom init",
        ));
    }
    let source_alias = declaration.source.name.clone();
    Ok(ContributionRequest {
        ctx,
        declaration,
        source_alias,
        source_identity: entry.identity.clone(),
        base,
        cache_repo,
    })
}

/// 执行一次变更集提交与推送。
pub fn submit(
    req: ContributionRequest,
    rel_paths: &[String],
    from_dir: Option<&Path>,
    message: &str,
    provider: &str,
) -> Result<Value> {
    let (wt, cs) = ensure_worktree(&req.ctx, &req.cache_repo, &req.source_alias, &req.base)?;
    let result = commit_and_push(
        PushEnv {
            ctx: &req.ctx,
            wt: &wt,
            cs: &cs,
            identity: &req.source_identity,
            provider,
            source_alias: &req.source_alias,
        },
        rel_paths,
        from_dir,
        message,
    );
    wt.cleanup();
    result
}

/// AIL-014：资源贡献入口（`ailoom push --from <已修改的源克隆>`）。
pub struct PushArgs {
    pub from: PathBuf,
    pub message: String,
    pub provider: String,
    pub root: Option<PathBuf>,
}

pub fn run_push(args: &PushArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let req = prepare_contribution(data_root, args.root.as_deref())?;
    let from = args.from.canonicalize().map_err(|_| {
        Error::new(
            code::USAGE,
            format!("--from 目录不存在: {}", args.from.display()),
        )
    })?;
    if !from.is_dir() {
        return Err(Error::new(code::USAGE, "--from 必须是目录"));
    }
    let rel_paths = scan_contribution(&from)?;
    let value = submit(req, &rel_paths, Some(&from), &args.message, &args.provider)?;
    if !json {
        crate::logging::info(format!(
            "贡献已推送：分支 {}（变更集 {}）",
            value["branch"].as_str().unwrap_or("?"),
            value["changeset_id"].as_str().unwrap_or("?")
        ));
    }
    Ok(value)
}
