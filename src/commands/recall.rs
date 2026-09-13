//! recall 命令（AIL-016）：按需检索本地知识索引；缺失/过期/损坏自动重建。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use crate::knowledge::index::{self, KnowledgeIndex};
use crate::knowledge::search::{search, SearchHit};
use crate::manifest::TeamManifest;
use crate::resolver::{resolve, ResolveRequest};
use crate::source::{GitSource, SourcesLock};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct RecallArgs {
    pub query: String,
    pub kind: Option<String>,
    pub limit: usize,
    pub root: Option<PathBuf>,
    pub rebuild: bool,
}

pub fn run(args: &RecallArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let decl_path = ctx.declaration_path().ok_or_else(|| {
        Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
    })?;
    let declaration = ProjectDeclaration::load(&decl_path)?
        .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "工作区未绑定"))?;
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

    let (snapshot, identity) = match declaration.source.kind.as_str() {
        "git" => {
            let src = GitSource::new(
                entry.identity.trim_start_matches("git+"),
                entry.ref_.as_deref(),
            )?;
            let snap = src.resolve(&ctx.source_cache(&src.identity), Some(&entry))?;
            (snap, src.identity.clone())
        }
        "local" | "self" => {
            let p = if declaration.source.kind == "self"
                && declaration
                    .source
                    .path
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty()
            {
                ".ailoom-team".to_string()
            } else {
                declaration.source.path.clone().unwrap_or_default()
            };
            let base = if PathBuf::from(&p).is_absolute() {
                PathBuf::from(&p)
            } else {
                ctx.workspace.workspace_root.join(&p)
            };
            let src = crate::source::LocalSource::new(&base)?;
            (src.resolve()?, src.identity.clone())
        }
        other => {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("未知 source.type: {other}"),
            ))
        }
    };

    let manifest = TeamManifest::load_from(&snapshot.root)?;
    let desired = resolve(ResolveRequest {
        snapshot_root: &snapshot.root,
        manifest: &manifest,
        source: &declaration.source.name,
        identity: &identity,
        revision: snapshot.resolved_commit.clone(),
        content_digest: snapshot.content_digest.clone(),
        active_projects: &declaration.projects,
        active_roles: &declaration.roles,
    })?;

    let index_dir = ctx.layout.index_dir.clone();
    let index: KnowledgeIndex = if args.rebuild {
        let idx = index::build(&desired, &snapshot.root)?;
        index::save_atomic(&idx, &index_dir)?;
        idx
    } else {
        // 缺失/过期/损坏：自动重建（原子替换）
        match index::load(&index_dir, &desired) {
            Ok(Some(idx)) => idx,
            Ok(None) | Err(_) => {
                let idx = index::build(&desired, &snapshot.root)?;
                index::save_atomic(&idx, &index_dir)?;
                idx
            }
        }
    };

    let mut hits: Vec<SearchHit> = search(&index, &args.query, args.limit);
    if let Some(kind) = &args.kind {
        hits.retain(|h| &h.kind == kind);
    }
    // 归档闭环消费（AIL-028）：归档清单中的经验从召回可见集合排除
    let archived = crate::knowledge::feedback::archived_ids(&ctx);
    let archived_count = hits.iter().filter(|h| archived.contains(&h.id)).count();
    hits.retain(|h| !archived.contains(&h.id));
    // 真实召回才贡献使用指标（AIL-028）
    crate::knowledge::feedback::record_recall_hits(
        &ctx,
        &hits.iter().map(|h| h.id.clone()).collect::<Vec<_>>(),
    )?;

    let value = json!({
        "index_version": index.meta.revision,
        "fingerprint": crate::knowledge::index::fingerprint(&desired),
        "documents": index.documents.len(),
        "results": hits,
        "archived_filtered": archived_count,
        "note": "索引过滤是相关性隔离，不构成访问控制",
    });
    if !json {
        println!(
            "索引 {}（{} 篇文档），查询 {:?}：",
            value["fingerprint"].as_str().unwrap_or("?"),
            index.documents.len(),
            args.query
        );
        if hits.is_empty() {
            println!("（无匹配）");
        }
        for h in &hits {
            println!(
                "  {:<5} {:id_width$}  {}",
                h.kind,
                h.id,
                h.excerpt,
                id_width = 48
            );
        }
    }
    Ok(value)
}
