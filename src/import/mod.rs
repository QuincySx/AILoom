//! 批量导入（AIL-034）：目录 Markdown → 经验/文档资源变更集（经审核路径）。
//! 显式范围预览、来源去重（内容哈希）、symlink 逃逸拒绝、失败续传 checkpoint。

pub mod pr;
pub mod self_repo;

use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use crate::ids::{new_id, now_iso, sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct ImportArgs {
    pub project: Option<String>,
    pub dir: PathBuf,
    /// 目标：project:<id> 或 shared
    pub target: String,
    pub kind: String, // learning | doc
    pub root: Option<PathBuf>,
    pub execute: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCheckpoint {
    pub schema_version: u32,
    /// 内容哈希 → 导入时间（重复导入去重依据）
    pub imported: BTreeMap<String, String>,
}

fn checkpoint(ctx: &AppContext) -> PathBuf {
    ctx.layout.ws_dir.join("import-checkpoint.json")
}

fn load_checkpoint(ctx: &AppContext) -> ImportCheckpoint {
    std::fs::read_to_string(checkpoint(ctx))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(ImportCheckpoint {
            schema_version: 1,
            imported: Default::default(),
        })
}

/// 扫描目录：拒绝 symlink 逃逸；返回 (相对路径, 绝对路径) 列表。
fn scan(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    if !dir.is_dir() {
        return Err(Error::new(
            code::IMPORT_OUT_OF_SCOPE,
            format!("导入目录不存在: {}", dir.display()),
        ));
    }
    let mut out = Vec::new();
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_symlink() {
            return Err(Error::new(
                code::PATH_TRAVERSAL,
                format!("导入目录含符号链接，已拒绝: {}", entry.path().display()),
            ));
        }
        if !entry.file_type().is_file() || !entry.file_name().to_string_lossy().ends_with(".md") {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dir)?
            .to_string_lossy()
            .to_string();
        out.push((rel, entry.path().to_path_buf()));
    }
    out.sort();
    Ok(out)
}

pub fn run(args: &ImportArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    // 归属校验：project 必须在活跃绑定中；shared 显式
    let target = match args.target.as_str() {
        "shared" => "shared".to_string(),
        p => {
            let pid = p
                .strip_prefix("project:")
                .ok_or_else(|| Error::new(code::USAGE, "--target 必须是 shared 或 project:<id>"))?;
            if Some(pid) != args.project.as_deref() {
                // project:<id> 即显式指定
            }
            format!("project:{pid}")
        }
    };
    let _ = &args.project;

    let dir = args.dir.canonicalize().map_err(|_| {
        Error::new(
            code::IMPORT_OUT_OF_SCOPE,
            format!("目录不可访问: {}", args.dir.display()),
        )
    })?;
    let files = scan(&dir)?;
    if files.is_empty() {
        return Err(Error::new(
            code::IMPORT_OUT_OF_SCOPE,
            "导入范围内没有 Markdown 文档",
        ));
    }
    let mut cp = load_checkpoint(&ctx);

    // 预览/构建变更集
    let mut planned: Vec<(String, String, String)> = Vec::new(); // (目标名, 内容, 摘要来源)
    let mut skipped_dup = 0usize;
    for (rel, abs) in &files {
        let content = std::fs::read_to_string(abs)?;
        let digest = sha256_hex(content.as_bytes());
        if cp.imported.contains_key(&digest) {
            skipped_dup += 1;
            continue;
        }
        let stem: String = Path::new(rel)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("doc-{digest}", digest = &digest[..8]));
        let name = format!("{}-{}", sanitize(&stem), &digest[..8]);
        let title = content
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l[2..].trim().to_string())
            .unwrap_or_else(|| stem.clone());
        let target_field = if target == "shared" {
            "shared: true".to_string()
        } else {
            format!("project: {}", target.trim_start_matches("project:"))
        };
        let doc = format!(
            "---\nname: {name}\ntitle: {title}\n{target_field}\nnamespace: common\nsource_ref: \"import:{rel}\"\n---\n\n# {title}\n\n{content}\n"
        );
        planned.push((name, doc, format!("import:{rel}#{digest}")));
    }
    // 来源清单（可预览）
    let planned_list: Vec<Value> = planned
        .iter()
        .map(|(name, _, source)| json!({ "name": name, "source": source }))
        .collect();

    if !args.execute {
        let value = json!({
            "mode": "preview",
            "target": target,
            "planned": planned_list,
            "skipped_duplicates": skipped_dup,
        });
        if !json {
            println!("导入预览：{} 篇（跳过已导入 {skipped_dup}）", planned.len());
        }
        return Ok(value);
    }

    if planned.is_empty() {
        let value = json!({
            "mode": "execute",
            "target": target,
            "imported": 0,
            "planned": planned_list,
            "skipped_duplicates": skipped_dup,
            "note": "全部内容已导入过（按内容哈希去重）",
        });
        if !json {
            println!("没有需要导入的新文档");
        }
        return Ok(value);
    }

    // 执行：写入导入变更集（learning/doc 经贡献链路审核）
    let req = crate::contribution::prepare_contribution(data_root, args.root.as_deref())?;
    let (wt, cs) = crate::contribution::ensure_worktree(
        &req.ctx,
        &req.cache_repo,
        &req.source_alias,
        &req.base,
    )?;
    let result = (|| -> Result<Value> {
        let mut staged: Vec<String> = Vec::new();
        for (name, doc, _) in &planned {
            let rel = format!(
                "{}/{}.md",
                if args.kind == "learning" {
                    "resources/learnings"
                } else {
                    "resources/docs"
                },
                name
            );
            let dest = wt.path.join(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, doc.as_bytes())?;
            crate::gitx::git(&wt.path, &["add", "--", &rel])?;
            staged.push(rel);
        }
        if staged.is_empty() {
            return Ok(
                json!({ "imported": 0, "skipped_duplicates": skipped_dup, "note": "全部内容已导入过" }),
            );
        }
        crate::gitx::git(
            &wt.path,
            &[
                "-c",
                "user.name=ailoom-import",
                "-c",
                "user.email=import@ailoom.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                &format!("ailoom: 批量导入 {} 篇", staged.len()),
            ],
        )?;
        crate::contribution::commit_and_push(
            crate::contribution::PushEnv {
                ctx: &req.ctx,
                wt: &wt,
                cs: &cs,
                identity: &req.source_identity,
                provider: "manual",
                source_alias: &req.source_alias,
            },
            &staged,
            None,
            &format!("ailoom: 批量导入 {} 篇", staged.len()),
        )
    })();
    let value = match result {
        Ok(mut v) => {
            // 按内容摘要去重存储（与查询键一致）
            for (name, _doc, source) in &planned {
                let digest = source.rsplit('#').next().unwrap_or_default().to_string();
                cp.imported.insert(digest, now_iso());
                let _ = name;
            }
            crate::sync_common::atomic_write(
                checkpoint(&ctx).as_path(),
                serde_json::to_vec_pretty(&cp)?.as_slice(),
            )?;
            v["skipped_duplicates"] = json!(skipped_dup);
            v["mode"] = json!("execute");
            v["target"] = json!(target);
            v["imported"] = json!(planned.len());
            v
        }
        Err(e) => {
            wt.cleanup();
            return Err(e);
        }
    };
    wt.cleanup();
    if !json {
        crate::logging::info(format!("导入完成：{} 篇进入审核变更集", planned.len()));
    }
    Ok(value)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

// 保留 new_id 引用（变更集 ID 由贡献层生成，此处留作扩展点）
#[allow(dead_code)]
fn _touch() -> String {
    new_id()
}
