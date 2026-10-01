//! 代码事实命令（AIL-026/027）：build/query。项目图按项目隔离存储。

use crate::appctx::AppContext;
pub mod graph;
pub mod recall;

use crate::code_knowledge::graph::Graph;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct CodeArgs {
    pub action: String, // build | query
    pub query: Option<String>,
    pub hops: usize,
    pub root: Option<PathBuf>,
}

pub fn run(args: &CodeArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let declaration = ctx
        .declaration_path()
        .and_then(|p| ProjectDeclaration::load(&p).ok().flatten())
        .ok_or_else(|| {
            Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
        })?;
    let project = declaration.projects.first().cloned().ok_or_else(|| {
        Error::new(
            code::USAGE,
            "代码知识需要工作区绑定一个项目（代码图谱按项目隔离）",
        )
    })?;
    let graph_path = ctx
        .layout
        .index_dir
        .join(format!("codegraph-{project}.json"));

    match args.action.as_str() {
        "build" => {
            // 增量：读取旧图，文件哈希基线，重解析变化文件；schema 不符 → 全量重建
            let old = graph::load(&graph_path)?;
            let mut g = match old {
                Some(g)
                    if g.schema_version == graph::GRAPH_SCHEMA_VERSION
                        && g.revision == current_revision(&ctx, &declaration)? =>
                {
                    g
                }
                _ => Graph::default(),
            };
            g.schema_version = graph::GRAPH_SCHEMA_VERSION;
            graph::update_incremental(
                &mut g,
                &ctx.workspace.workspace_root,
                current_revision(&ctx, &declaration)?,
            )?;
            graph::save(&g, &graph_path)?;
            let value = json!({
                "project": project,
                "files": g.files.len(),
                "parsed_ok": g.scan_stats.parsed_ok,
                "parse_gaps": g.scan_stats.parse_gaps,
                "skipped_dirs": g.scan_stats.skipped_dirs,
                "symbols": g.files.values().map(|f| f.symbols.len()).sum::<usize>(),
                "edges": g.files.values().map(|f| f.edges.len()).sum::<usize>(),
            });
            if !json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            Ok(value)
        }
        "query" => {
            let query = args
                .query
                .as_deref()
                .ok_or_else(|| Error::new(code::USAGE, "query 需要 --query <关键词>"))?;
            let g: Graph = graph::load(&graph_path)?.ok_or_else(|| {
                Error::new(code::INDEX_CORRUPT, "代码图谱不存在")
                    .fix("先运行 ailoom code --action build")
            })?;
            // 过期检查（AIL-027/RW-11）：源码 revision 变化或**Worktree 内容指纹**
            // 变化（未提交的修改/删除/新增，覆盖非 Git 工作区）都只做提示性返回，
            // 不冒充新事实。指纹只取文件内容，不使用 mtime 或单独 HEAD。
            let current = current_revision(&ctx, &declaration)?;
            let worktree = graph::worktree_fingerprint(&ctx.workspace.workspace_root)?;
            let revision_stale = g.revision != current;
            let content_stale = g.content_fingerprint.as_deref() != Some(worktree.as_str());
            let stale = revision_stale || content_stale;
            let results = crate::code_knowledge::recall::query(&g, query, args.hops, 10);
            let note = if revision_stale {
                "图已过期（源码/版本变化），结果可能含旧事实；请运行 ailoom code --action build 重建"
            } else if content_stale {
                "图已过期（Worktree 内容与构图时不一致，含未提交改动），结果可能含旧事实；请运行 ailoom code --action build 重建"
            } else {
                "图与当前 revision 及 Worktree 内容一致"
            };
            let value = json!({
                "project": project,
                "results": results,
                "graph_stale": stale,
                "graph_revision": g.revision,
                "current_revision": current,
                "content_stale": content_stale,
                "worktree_fingerprint": worktree,
                "note": note,
                "dangling_call_edges": graph::dangling_call_edges(&g).len(),
            });
            if !json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            Ok(value)
        }
        other => Err(Error::new(
            code::USAGE,
            format!("未知 code 动作: {other}（build/query）"),
        )),
    }
}

fn current_revision(ctx: &AppContext, declaration: &ProjectDeclaration) -> Result<Option<String>> {
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(crate::workspace::AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = crate::source::SourcesLock::load(&lock_path)?;
    let lock_commit = lock
        .and_then(|l| l.sources.get(&declaration.source.name).cloned())
        .and_then(|e| e.resolved_commit);
    // 过期基准（AIL-027）= 团队源锁 + 工作区 HEAD：被扫描的业务源码变化同样使图过期
    let ws_head = crate::gitx::git(&ctx.workspace.workspace_root, &["rev-parse", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string());
    Ok(match (lock_commit, ws_head) {
        (Some(l), Some(h)) => Some(format!("{l}+{h}")),
        (Some(l), None) => Some(l),
        (None, Some(h)) => Some(h),
        (None, None) => None,
    })
}
