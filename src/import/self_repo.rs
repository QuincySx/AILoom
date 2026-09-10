//! 同仓资源模式与迁移（AIL-036）：迁移（copy→校验→切换→备份）+ 同仓贡献。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use crate::ids::now_iso;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SELF_SUBTREE_DEFAULT: &str = ".ailoom-team";

pub struct MigrateArgs {
    /// 现有独立团队源目录（ailoom.toml + resources/）
    pub from: PathBuf,
    /// 子树路径（默认 .ailoom-team）
    pub subtree: String,
    pub root: Option<PathBuf>,
}

/// 迁移：copy → 校验（摘要一致）→ 切换（声明改为 self，保留备份）。机器数据不动。
pub fn run_migrate(
    args: &MigrateArgs,
    json: bool,
    data_root: Option<&std::path::Path>,
) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let from = args.from.canonicalize().map_err(|_| {
        Error::new(
            code::USAGE,
            format!("--from 目录不存在: {}", args.from.display()),
        )
    })?;
    let subtree = if args.subtree.is_empty() {
        SELF_SUBTREE_DEFAULT.to_string()
    } else {
        args.subtree.clone()
    };
    let dest = ctx.workspace.workspace_root.join(&subtree);

    // 1. copy：整树复制（跳过 .git 与机器数据）
    let mut copied = 0usize;
    copy_tree(&from, &dest, &mut copied)?;

    // 2. 校验：摘要一致
    let src_digest = crate::source::tree_digest(&from)?;
    let dst_digest = crate::source::tree_digest(&dest)?;
    if src_digest != dst_digest {
        return Err(Error::new(
            code::INTERNAL,
            format!("迁移校验失败：源 {src_digest} 与目标 {dst_digest} 不一致"),
        )
        .fix("删除子树后重新运行迁移"));
    }

    // 3. 切换：声明改写为 self 模式（备份旧声明）
    let decl_path = ctx
        .declaration_path()
        .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "工作区未绑定"))?;
    let backup_dir = ctx.layout.ws_dir.join("migrate-backup");
    std::fs::create_dir_all(&backup_dir)?;
    let backup = backup_dir.join(format!("project-{}.toml.bak", now_iso().replace(':', "")));
    if decl_path.is_file() {
        std::fs::copy(&decl_path, &backup)?;
    }
    let text = std::fs::read_to_string(&decl_path)?;
    let new_text = switch_to_self(&text, &subtree);
    let decl: ProjectDeclaration = crate::config::ProjectDeclaration::parse(&new_text)?;
    crate::sync_common::atomic_write(
        decl_path.as_path(),
        toml::to_string_pretty(&decl)?.as_bytes(),
    )?;

    let value = json!({
        "mode": "migrate",
        "subtree": subtree,
        "copied_files": copied,
        "content_digest": dst_digest,
        "declaration_backup": backup.display().to_string(),
        "note": "机器数据（绑定/事件/索引）保留在外置数据根，未迁移",
    });
    if !json {
        crate::logging::info(format!("迁移完成：子树 {subtree}（{copied} 个文件）"));
    }
    Ok(value)
}

fn copy_tree(from: &Path, to: &Path, copied: &mut usize) -> Result<()> {
    for entry in walkdir::WalkDir::new(from)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry.path().strip_prefix(from)?;
        if rel
            .components()
            .any(|c| c.as_os_str() == ".git" || c.as_os_str() == "machine")
        {
            continue;
        }
        let dest = to.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &dest)?;
            *copied += 1;
        }
    }
    Ok(())
}

fn switch_to_self(text: &str, subtree: &str) -> String {
    // 声明改写：type=self、path=子树；保留 projects/roles/targets（仅替换 [source] 段关键字段）
    let mut out = String::new();
    let mut in_source = false;
    for line in text.lines() {
        if line.starts_with('[') {
            in_source = line.starts_with("[source]");
        }
        if in_source {
            if line.starts_with("type =") {
                out.push_str("type = \"self\"\n");
                continue;
            }
            if line.starts_with("path =") {
                out.push_str(&format!("path = \"{subtree}\"\n"));
                continue;
            }
            if line.starts_with("url =") || line.starts_with("ref =") {
                continue; // self 模式不需要 url/ref（保留 name 作为源别名）
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !out.contains("path =") {
        // 在 [source] 后补 path
        out = out.replace("[source]", &format!("[source]\npath = \"{subtree}\"\n"));
    }
    out
}

/// 同仓贡献：在业务仓创建隔离 detached worktree，仅提交子树路径。
pub fn contribute_self(ctx: &AppContext, message: &str) -> Result<Value> {
    let business = &ctx.workspace.workspace_root;
    if !business.join(".git").exists() {
        return Err(Error::new(code::USAGE, "同仓模式要求业务仓库为 Git 仓库"));
    }
    let branch = format!("ailoom/contribute-self-{}", &crate::ids::new_id()[..8]);
    let wt = ctx
        .layout
        .ws_dir
        .join(format!("self-contribute-{}", &crate::ids::new_id()[..8]));
    crate::gitx::git(
        business,
        &[
            "worktree",
            "add",
            "--detach",
            wt.to_str().unwrap_or_default(),
            "HEAD",
        ],
    )?;
    let commit = crate::gitx::git(&wt, &["rev-parse", "HEAD"])?;
    let result = (|| -> Result<Value> {
        crate::gitx::git(&wt, &["add", "--", SELF_SUBTREE_DEFAULT])?;
        let staged = crate::gitx::git(&wt, &["diff", "--cached", "--name-only"])?;
        if staged.trim().is_empty() {
            return Ok(json!({ "no_changes": true }));
        }
        crate::gitx::git(
            &wt,
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
        let new_head = crate::gitx::git(&wt, &["rev-parse", "HEAD"])?;
        Ok(json!({
            "branch": branch,
            "commit": new_head.trim(),
            "pushed": false,
            "note": "同仓模式：贡献分支在业务仓库，由团队推送到远端审核；未推送业务默认分支",
        }))
    })();
    let _ = crate::gitx::git(
        business,
        &[
            "worktree",
            "remove",
            "--force",
            wt.to_str().unwrap_or_default(),
        ],
    );
    let _ = crate::gitx::git(business, &["worktree", "prune"]);
    let _ = std::fs::remove_dir_all(&wt);
    let _ = commit;
    result
}

/// 供 status/doctor 展示：同仓子树摘要。
pub fn subtree_digest(ctx: &AppContext) -> Option<String> {
    let dir = ctx.workspace.workspace_root.join(SELF_SUBTREE_DEFAULT);
    if dir.is_dir() {
        crate::source::tree_digest(&dir).ok()
    } else {
        None
    }
}
