//! 同仓资源模式与迁移（AIL-036）：
//! 迁移 = 校验（越界/覆盖/重叠拒绝）→ 暂存 → 摘要校验 → 显式切换 → 备份与可恢复记录；
//! 同仓贡献 = 隔离 worktree 中复制精确子树变更、创建真实可追溯分支，业务 checkout 不受影响。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use crate::ids::now_iso;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

pub const SELF_SUBTREE_DEFAULT: &str = ".ailoom-team";

pub struct MigrateArgs {
    /// 现有独立团队源目录（ailoom.toml + resources/）
    pub from: PathBuf,
    /// 子树路径（默认 .ailoom-team）
    pub subtree: String,
    pub root: Option<PathBuf>,
}

fn out_of_scope(msg: String) -> Error {
    Error::new(code::IMPORT_OUT_OF_SCOPE, msg)
        .fix("子树必须是工作区内的相对路径（如 .ailoom-team），且不得指向工作区外")
}

/// 规范化并校验子树路径：相对、不含 `..`、祖先/末端符号链接不得逃出工作区。
/// 所有写入之前必须调用；返回规范化后的绝对目标路径。
pub fn validate_subtree(workspace_root: &Path, subtree: &str) -> Result<PathBuf> {
    if subtree.is_empty() {
        return Err(out_of_scope("子树路径为空".into()));
    }
    let rel = Path::new(subtree);
    if rel.is_absolute() {
        return Err(out_of_scope(format!("子树路径必须是相对路径: {subtree}")));
    }
    let mut norm = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(p) => norm.push(p),
            Component::CurDir => {}
            _ => {
                return Err(out_of_scope(format!(
                    "子树路径不得包含 `..` 或路径前缀: {subtree}"
                )))
            }
        }
    }
    if norm.as_os_str().is_empty() {
        return Err(out_of_scope("子树路径为空".into()));
    }
    let ws_canon = workspace_root
        .canonicalize()
        .map_err(|e| Error::new(code::WORKSPACE_INVALID, format!("工作区根无法解析: {e}")))?;
    // 逐级检查：祖先或末端若是符号链接，解析后必须仍在工作区内。
    // 组件不存在时（首次迁移的常规情形）停止下钻：更深路径尚无符号链接可逃逸。
    let mut cur = ws_canon.clone();
    for comp in norm.components() {
        cur = cur.join(comp);
        let meta = match cur.symlink_metadata() {
            Ok(m) => m,
            Err(_) => break,
        };
        if meta.file_type().is_symlink() {
            let resolved = cur.canonicalize().map_err(|e| {
                Error::new(
                    code::INTERNAL,
                    format!("符号链接无法解析 {}: {e}", cur.display()),
                )
            })?;
            if !resolved.starts_with(&ws_canon) {
                return Err(out_of_scope(format!(
                    "符号链接指向工作区外: {} → {}",
                    cur.display(),
                    resolved.display()
                )));
            }
        }
    }
    Ok(ws_canon.join(norm))
}

/// 迁移：预检冲突 → 暂存（数据根，跨文件系统安全）→ 摘要校验 → 物化到子树 → 切换声明（保留备份与可恢复记录）。
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
    // 1. 任何写入之前：规范化校验子树（越界/绝对路径/符号链接逃逸拒绝）
    let dest = validate_subtree(&ctx.workspace.workspace_root, &subtree)?;
    // 源与目标重叠（含嵌套）会导致自复制：拒绝
    if from.starts_with(&dest) || dest.starts_with(&from) {
        return Err(out_of_scope(format!(
            "源目录 {} 与目标子树 {} 重叠，拒绝迁移",
            from.display(),
            dest.display()
        )));
    }

    // 2. 预检已有文件冲突：已有业务文件不被隐式覆盖
    for entry in walkdir::WalkDir::new(&from)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(&from)?;
        let target = dest.join(rel);
        if target.is_file() {
            let same = std::fs::read(entry.path())? == std::fs::read(&target)?;
            if !same {
                return Err(Error::new(
                    code::IMPORT_OUT_OF_SCOPE,
                    format!("目标已存在且内容不同，拒绝隐式覆盖: {}", target.display()),
                )
                .fix("确认后手工处理既有文件，或删除子树后重新迁移"));
            }
        } else if target.symlink_metadata().is_ok() {
            return Err(Error::new(
                code::IMPORT_OUT_OF_SCOPE,
                format!(
                    "目标位置已存在且不是常规文件，拒绝覆盖: {}",
                    target.display()
                ),
            ));
        }
    }

    // 3. 暂存到数据根（复制而非 rename，不假定跨文件系统原子性）
    let staging = ctx
        .layout
        .ws_dir
        .join(format!("migrate-staging-{}", now_iso().replace(':', "")));
    std::fs::create_dir_all(&staging)?;
    let mut copied = 0usize;
    copy_tree(&from, &staging, &mut copied)?;

    // 4. 摘要校验（失败保留暂存目录作为证据）
    let src_digest = crate::source::tree_digest(&from)?;
    let staging_digest = crate::source::tree_digest(&staging)?;
    if src_digest != staging_digest {
        return Err(Error::new(
            code::INTERNAL,
            format!("暂存校验失败：源 {src_digest} 与暂存 {staging_digest} 不一致"),
        )
        .fix(format!("检查暂存目录 {} 后重试", staging.display())));
    }

    // 5. 可恢复记录（物化与切换期间的中断都以此追溯）
    let record_path = ctx.layout.ws_dir.join("migrate-record.json");
    write_record(
        &record_path,
        json!({
            "state": "staged",
            "subtree": subtree,
            "staging": staging.display().to_string(),
            "dest": dest.display().to_string(),
            "files": copied,
            "digest": staging_digest,
            "at": now_iso(),
        }),
    )?;

    // 6. 物化：暂存 → 子树（逐文件复制；预检已排除内容冲突）
    let mut materialized = 0usize;
    copy_tree(&staging, &dest, &mut materialized)?;

    // 7. 切换：声明改写为 self 模式（备份旧声明；改写失败不影响业务文件）
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

    write_record(
        &record_path,
        json!({
            "state": "switched",
            "subtree": subtree,
            "staging": staging.display().to_string(),
            "dest": dest.display().to_string(),
            "files": copied,
            "digest": staging_digest,
            "declaration_backup": backup.display().to_string(),
            "at": now_iso(),
        }),
    )?;

    let value = json!({
        "mode": "migrate",
        "subtree": subtree,
        "copied_files": copied,
        "content_digest": staging_digest,
        "declaration_backup": backup.display().to_string(),
        "recover_record": record_path.display().to_string(),
        "note": "机器数据（绑定/事件/索引）保留在外置数据根，未迁移",
    });
    if !json {
        crate::logging::info(format!("迁移完成：子树 {subtree}（{copied} 个文件）"));
    }
    Ok(value)
}

fn write_record(path: &Path, value: Value) -> Result<()> {
    crate::sync_common::atomic_write(path, serde_json::to_vec_pretty(&value)?.as_slice())
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

/// 读取声明中的同仓子树（self 模式 [source].path；非 self 或缺省返回默认值）。
fn declared_subtree(ctx: &AppContext) -> String {
    ctx.declaration_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| ProjectDeclaration::parse(&t).ok())
        .filter(|d| d.source.kind == "self")
        .and_then(|d| d.source.path)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| SELF_SUBTREE_DEFAULT.to_string())
}

/// 同仓贡献（真实命令入口）：在业务仓建立隔离 detached worktree（HEAD），
/// 复制当前子树内容、创建真实分支并提交；业务工作区/HEAD/分支不受影响。
/// worktree 清理后分支仍保留在共享 refs 中，可审查、可重试（再次运行生成新分支）。
pub fn contribute_self(ctx: &AppContext, message: &str) -> Result<Value> {
    let business = ctx.workspace.workspace_root.clone();
    if !business.join(".git").exists() {
        return Err(Error::new(code::USAGE, "同仓模式要求业务仓库为 Git 仓库"));
    }
    // 子树路径以声明为准；写入前同样校验
    let subtree = declared_subtree(ctx);
    let subtree_dir = validate_subtree(&business, &subtree)?;
    if !subtree_dir.is_dir() {
        return Err(Error::new(
            code::USAGE,
            format!("同仓子树不存在: {}", subtree_dir.display()),
        )
        .fix("先运行 ailoom migrate 迁移资源到同仓子树"));
    }

    let id = crate::ids::new_id()[..8].to_string();
    let branch = format!("ailoom/contribute-self-{id}");
    let wt = ctx.layout.ws_dir.join(format!("self-contribute-{id}"));
    crate::gitx::git(
        &business,
        &[
            "worktree",
            "add",
            "--detach",
            "-q",
            wt.to_str().unwrap_or_default(),
            "HEAD",
        ],
    )?;

    let result = (|| -> Result<Value> {
        // 复制精确资源变更：以当前业务子树内容覆盖 worktree 中的 HEAD 版本
        let wt_subtree = wt.join(&subtree);
        if wt_subtree.exists() {
            crate::sync_common::remove_dir_all_guarded(&wt_subtree)?;
        }
        std::fs::create_dir_all(&wt_subtree)?;
        let mut n = 0usize;
        copy_tree(&subtree_dir, &wt_subtree, &mut n)?;
        crate::gitx::git(&wt, &["add", "-A", "--", &subtree])?;
        let staged = crate::gitx::git(&wt, &["diff", "--cached", "--name-only"])?;
        if staged.trim().is_empty() {
            return Ok(json!({
                "no_changes": true,
                "note": "子树相对 HEAD 无待贡献改动",
            }));
        }
        // 创建真实分支并提交（worktree 共享 refs；分支在 worktree 移除后仍存在）
        crate::gitx::git(&wt, &["switch", "-q", "-c", &branch])?;
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
        let new_head = crate::gitx::git(&wt, &["rev-parse", "HEAD"])?
            .trim()
            .to_string();
        Ok(json!({
            "branch": branch,
            "commit": new_head,
            "subtree": subtree,
            "files": staged.lines().count(),
            "pushed": false,
            "note": "同仓模式：贡献分支在业务仓库，由团队推送到远端审核；未推送业务默认分支",
        }))
    })();

    let contributed = result.is_ok()
        && result
            .as_ref()
            .ok()
            .and_then(|v| v.get("branch").cloned())
            .is_some();
    let _ = crate::gitx::git(
        &business,
        &[
            "worktree",
            "remove",
            "--force",
            wt.to_str().unwrap_or_default(),
        ],
    );
    let _ = crate::gitx::git(&business, &["worktree", "prune"]);
    let _ = std::fs::remove_dir_all(&wt);

    if contributed {
        // 分支必须在 worktree 清理后仍可验证存在
        crate::gitx::git(&business, &["rev-parse", "--verify", &branch])?;
    }
    result
}

/// 供 status/doctor 展示：同仓子树摘要（按声明子树，缺省默认）。
pub fn subtree_digest(ctx: &AppContext) -> Option<String> {
    let subtree = declared_subtree(ctx);
    let dir = ctx.workspace.workspace_root.join(subtree);
    if dir.is_dir() {
        crate::source::tree_digest(&dir).ok()
    } else {
        None
    }
}
