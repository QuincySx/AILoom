//! 批量导入（AIL-034）：目录 Markdown → 经验/文档资源变更集（经审核路径）。
//! 显式范围预览、来源去重（内容哈希）、symlink 逃逸拒绝、失败续传 checkpoint。

pub mod pr;
pub mod self_repo;

use crate::appctx::AppContext;
use crate::error::{code, Error, Result};
use crate::ids::{new_id, now_iso};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct ImportArgs {
    pub project: Option<String>,
    /// 导入源：普通目录或本地 Git 仓库（含 .git）
    pub dir: PathBuf,
    /// 显式仓库列表：每行一个本地目录/Git 仓库路径；`#` 开头为注释
    pub repo_list: Option<PathBuf>,
    /// 目标：project:<id> 或 shared
    pub target: String,
    pub kind: String, // learning | doc
    pub root: Option<PathBuf>,
    pub execute: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCheckpoint {
    pub schema_version: u32,
    /// 候选身份（source|kind|target|name）→ 已导入记录。
    /// 去重 = 来源身份 + revision + 目标 + 类型 + 稳定文档身份（AIL-034）；
    /// digest 与上次不同 → 按同名候选原位更新，而不是增生新文档。
    pub imported: BTreeMap<String, ImportedRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedRecord {
    pub digest: String,
    /// 记录导入时的来源 revision（可追溯）
    #[serde(default)]
    pub revision: String,
    pub at: String,
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

/// 扫描目录/仓库：拒绝 symlink 逃逸；权限等扫描错误显式失败，不静默丢弃。
/// 返回 (相对路径, 绝对路径) 列表（排除 .git 内部）。
fn scan(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    if !dir.is_dir() {
        return Err(Error::new(
            code::IMPORT_OUT_OF_SCOPE,
            format!("导入目录不存在: {}", dir.display()),
        ));
    }
    let mut out = Vec::new();
    let mut scan_errors: Vec<String> = Vec::new();
    for entry in WalkDir::new(dir).follow_links(false).into_iter() {
        match entry {
            Ok(e) => {
                if e.file_type().is_symlink() {
                    return Err(Error::new(
                        code::PATH_TRAVERSAL,
                        format!("导入目录含符号链接，已拒绝: {}", e.path().display()),
                    ));
                }
                // .git 内部不属于可导入知识
                if e.file_type().is_dir() && e.file_name() == ".git" {
                    continue;
                }
                if !e.file_type().is_file() || !e.file_name().to_string_lossy().ends_with(".md") {
                    continue;
                }
                let rel = e.path().strip_prefix(dir)?.to_string_lossy().to_string();
                out.push((rel, e.path().to_path_buf()));
            }
            Err(err) => {
                scan_errors.push(format!("{}", err));
            }
        }
    }
    if !scan_errors.is_empty() {
        return Err(Error::new(
            code::IMPORT_OUT_OF_SCOPE,
            format!(
                "导入扫描有 {} 处错误（无权限/不可读等），拒绝部分成功: {}",
                scan_errors.len(),
                scan_errors
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        ));
    }
    out.sort();
    Ok(out)
}

/// 导入源：普通目录或本地 Git 仓库。
struct ImportSource {
    /// 来源身份（remote URL 或规范化路径）
    identity: String,
    /// 来源 revision（Git HEAD；目录源为 "-"）
    revision: String,
    /// 文档名前缀（防同名覆盖）
    prefix: String,
    root: PathBuf,
}

fn discover_source(path: &Path, force_prefix_hash: bool) -> Result<ImportSource> {
    let root = path.canonicalize().map_err(|_| {
        Error::new(
            code::IMPORT_OUT_OF_SCOPE,
            format!("目录不可访问: {}", path.display()),
        )
    })?;
    let is_repo = root.join(".git").exists();
    let (identity, revision) = if is_repo {
        let remote = crate::gitx::git(&root, &["remote", "get-url", "origin"])
            .ok()
            .map(|s| s.trim().to_string());
        let rev = crate::gitx::git(&root, &["rev-parse", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "-".into());
        (remote.unwrap_or_else(|| root.display().to_string()), rev)
    } else {
        (root.display().to_string(), "-".into())
    };
    let mut prefix = sanitize(
        &root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "source".into()),
    );
    if force_prefix_hash || prefix.is_empty() {
        // 列表内同名目录：前缀追加身份短哈希，保证两仓同名文档不覆盖
        prefix = format!(
            "{}-{}",
            prefix,
            crate::ids::sha256_prefix(identity.as_bytes(), 4)
        );
    }
    Ok(ImportSource {
        identity,
        revision,
        prefix,
        root,
    })
}

/// 候选稳定身份键：来源 + 类型 + 目标 + 稳定文档名（与内容无关，
/// 内容更新时同名候选原位更新）。
fn candidate_key(source_identity: &str, kind: &str, target: &str, name: &str) -> String {
    format!("{source_identity}|{kind}|{target}|{name}")
}

pub fn run(args: &ImportArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let declaration = ctx
        .declaration_path()
        .and_then(|p| crate::config::ProjectDeclaration::load(&p).ok().flatten())
        .ok_or_else(|| {
            Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
        })?;

    // 1. 目标归属校验（任何写入/扫描前）：shared 显式；project 必须在活跃绑定
    //    与团队清单中声明
    let target = match args.target.as_str() {
        "shared" => "shared".to_string(),
        p => {
            let pid = p
                .strip_prefix("project:")
                .ok_or_else(|| Error::new(code::USAGE, "--target 必须是 shared 或 project:<id>"))?;
            if !declaration.projects.iter().any(|x| x == pid) {
                return Err(Error::new(
                    code::UNKNOWN_REFERENCE,
                    format!(
                        "导入目标项目 `{pid}` 不在本工作区活跃绑定 {:?} 中，拒绝写入",
                        declaration.projects
                    ),
                ));
            }
            format!("project:{pid}")
        }
    };
    let kind = args.kind.trim().to_string();
    if !matches!(kind.as_str(), "learning" | "doc") {
        return Err(Error::new(
            code::USAGE,
            format!("--kind 必须是 learning 或 doc: {kind}"),
        ));
    }

    // 2. 解析来源（单目录/单仓 或 显式仓库列表）
    let mut source_paths: Vec<(PathBuf, bool)> = Vec::new(); // (路径, 是否追加身份哈希前缀)
    match &args.repo_list {
        Some(list) => {
            let text = std::fs::read_to_string(list).map_err(|e| {
                Error::new(code::IMPORT_OUT_OF_SCOPE, format!("仓库列表不可读: {e}"))
            })?;
            let mut seen: BTreeMap<String, usize> = Default::default();
            for line in text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
            {
                let p = PathBuf::from(line);
                let count = seen.entry(line.to_string()).or_insert(0);
                *count += 1;
                source_paths.push((p, false));
            }
            // 同名目录检测：出现 >1 次的路径其前缀需要身份哈希
            let dup: std::collections::BTreeSet<String> = seen
                .into_iter()
                .filter(|(_, n)| *n > 1)
                .map(|(k, _)| k)
                .collect();
            let _ = dup;
            if source_paths.is_empty() {
                return Err(Error::new(code::IMPORT_OUT_OF_SCOPE, "仓库列表为空"));
            }
            // 检测同名 basename（不同路径）→ 全部追加身份哈希
            let basenames: BTreeMap<String, usize> = source_paths
                .iter()
                .map(|(p, _)| {
                    p.canonicalize()
                        .unwrap_or_else(|_| p.clone())
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
                .fold(Default::default(), |mut m: BTreeMap<String, usize>, n| {
                    *m.entry(n).or_insert(0) += 1;
                    m
                });
            source_paths = source_paths
                .into_iter()
                .map(|(p, _)| {
                    let base = p
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    (p, basenames.get(&base).copied().unwrap_or(1) > 1)
                })
                .collect();
        }
        None => source_paths.push((args.dir.clone(), false)),
    }
    let mut sources: Vec<ImportSource> = Vec::new();
    for p in &source_paths {
        sources.push(discover_source(&p.0, p.1)?);
    }

    // 3. 载入清单（用于渲染产物复验）
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(crate::workspace::AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = crate::source::SourcesLock::load(&lock_path)?;
    let entry = lock
        .and_then(|l| l.sources.get(&declaration.source.name).cloned())
        .ok_or_else(|| Error::new(code::SOURCE_NOT_CACHED, "源未锁定"))?;
    let (snapshot, _) = crate::commands::sync_core::primary_snapshot(&ctx, &declaration, &entry)?;
    let manifest = crate::manifest::TeamManifest::load_from(&snapshot.root)?;
    if let Some(pid) = target.strip_prefix("project:") {
        manifest.require_project(pid)?;
    }
    let namespace = "common";
    if !manifest.namespaces.known.contains(&namespace.to_string()) {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("namespace `{namespace}` 未在团队清单声明"),
        )
        .context(serde_json::json!({ "known": manifest.namespaces.known })));
    }
    let resource_kind = if kind == "learning" {
        crate::resource::ResourceKind::Learning
    } else {
        crate::resource::ResourceKind::Doc
    };

    let mut cp = load_checkpoint(&ctx);
    let mut planned: Vec<(String, String, String, String)> = Vec::new(); // (name, doc, source_display, digest)
    let mut skipped_unchanged = 0usize;
    let mut scanned_names_per_source: Vec<(
        String,
        String,
        String,
        std::collections::BTreeSet<String>,
    )> = Vec::new(); // (identity, kind, target, names)

    for src in &sources {
        let files = scan(&src.root)?;
        if files.is_empty() {
            return Err(Error::new(
                code::IMPORT_OUT_OF_SCOPE,
                format!("导入范围内没有 Markdown 文档: {}", src.root.display()),
            ));
        }
        let mut current_names: std::collections::BTreeSet<String> = Default::default();
        for (rel, abs) in &files {
            let content = std::fs::read_to_string(abs).map_err(|e| {
                Error::new(
                    code::IMPORT_OUT_OF_SCOPE,
                    format!("文档不可读 {}: {e}", abs.display()),
                )
            })?;
            let digest = crate::ids::sha256_hex(content.as_bytes());
            let stem: String = Path::new(rel)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("doc-{}", &digest[..8]));
            // RW-12/R09：候选名（=落盘路径）必须包含目标，跨目标导入各自独立、
            // 不互相覆盖归属。兼容：旧版本候选名哈希不含 target——checkpoint 中
            // 已有旧名记录的候选继续沿用旧名（原位更新/跳过），禁止静默重归属
            // 或重复发布。
            let legacy_sid8 =
                crate::ids::sha256_prefix(format!("{}\0{rel}", src.identity).as_bytes(), 8);
            let legacy_name = format!("{}-{}-{legacy_sid8}", src.prefix, sanitize(&stem));
            let legacy_key = candidate_key(&src.identity, &kind, &target, &legacy_name);
            let sid8 = crate::ids::sha256_prefix(
                format!("{}\0{rel}\0{target}", src.identity).as_bytes(),
                8,
            );
            let name_new = format!("{}-{}-{sid8}", src.prefix, sanitize(&stem));
            let (name, key) = if cp.imported.contains_key(&legacy_key) {
                (legacy_name, legacy_key)
            } else {
                let k = candidate_key(&src.identity, &kind, &target, &name_new);
                (name_new, k)
            };
            current_names.insert(name.clone());
            if let Some(rec) = cp.imported.get(&key) {
                if rec.digest == digest {
                    skipped_unchanged += 1;
                    continue;
                }
                // 内容变化：同名候选原位更新（不增生）
            }
            let title = content
                .lines()
                .find(|l| l.starts_with("# "))
                .map(|l| l[2..].trim().to_string())
                .unwrap_or_else(|| stem.clone());
            let doc = render_import_doc(
                &name,
                &title,
                &target,
                namespace,
                &format!("import:{}#{}", src.identity, rel),
                &content,
                &kind,
            )?;
            // 渲染产物按资源契约复验（非法 YAML/归属在提交前拒绝）
            crate::resource::validate_rendered_markdown(&manifest, resource_kind, &name, &doc, rel)
                .map_err(|e| {
                    e.context(serde_json::json!({ "source": format!("import:{}", rel) }))
                })?;
            planned.push((
                name,
                doc,
                format!(
                    "import:{}@{}#{}",
                    src.identity,
                    &src.revision[..8.min(src.revision.len())],
                    rel
                ),
                digest.clone(),
            ));
            // 记录到内存（成功后落盘 checkpoint）
            cp.imported.insert(
                key,
                ImportedRecord {
                    digest: digest.clone(),
                    revision: src.revision.clone(),
                    at: now_iso(),
                },
            );
        }
        scanned_names_per_source.push((
            src.identity.clone(),
            kind.clone(),
            target.clone(),
            current_names,
        ));
    }

    // 4. 源删除审核建议：checkpoint 中该来源已有、但本次扫描不存在的候选
    let mut deletion_suggestions: Vec<Value> = Vec::new();
    for (identity, k, t, names) in &scanned_names_per_source {
        for (key, _rec) in cp
            .imported
            .range(candidate_key(identity, k, t, "").clone()..)
        {
            if !key.starts_with(&candidate_key(identity, k, t, "")) {
                break;
            }
            let name = key.rsplit('|').next().unwrap_or_default();
            if !name.is_empty() && !names.contains(name) {
                deletion_suggestions.push(json!({
                    "name": name,
                    "source": identity,
                    "kind": k,
                    "target": t,
                    "suggestion": "源中已不存在；建议在审核变更集中删除该候选",
                }));
            }
        }
    }

    // 预览
    let planned_list: Vec<Value> = planned
        .iter()
        .map(|(name, _doc, source, _)| json!({ "name": name, "source": source }))
        .collect();

    if !args.execute {
        let value = json!({
            "mode": "preview",
            "target": target,
            "kind": kind,
            "planned": planned_list,
            "skipped_unchanged": skipped_unchanged,
            "deletion_suggestions": deletion_suggestions,
        });
        if !json {
            println!(
                "导入预览：{} 篇（未变化跳过 {skipped_unchanged}，删除建议 {}）",
                planned.len(),
                deletion_suggestions.len()
            );
        }
        return Ok(value);
    }

    if planned.is_empty() {
        // 无更新也要落盘删除建议（审核输入，不自动删除）
        let mut value = json!({
            "mode": "execute",
            "target": target,
            "kind": kind,
            "imported": 0,
            "planned": planned_list,
            "skipped_unchanged": skipped_unchanged,
            "deletion_suggestions": deletion_suggestions,
            "note": "全部候选内容未变化（按来源+revision+目标+类型+身份去重）",
        });
        if !deletion_suggestions.is_empty() {
            let del_path = ctx.layout.ws_dir.join("import-deletions.json");
            crate::sync_common::atomic_write(
                del_path.as_path(),
                serde_json::to_vec_pretty(&deletion_suggestions)?.as_slice(),
            )?;
            value["deletion_suggestions_path"] = json!(del_path.display().to_string());
        }
        if !json {
            println!("没有需要导入的更新");
        }
        return Ok(value);
    }

    // 5. 执行：写入导入变更集（经贡献审核路径）；checkpoint 仅在推送确认后落盘
    let req = crate::contribution::prepare_contribution(data_root, args.root.as_deref())?;
    let (wt, cs) = crate::contribution::ensure_worktree(
        &req.ctx,
        &req.cache_repo,
        &req.source_alias,
        &req.base,
    )?;
    let result = (|| -> Result<Value> {
        let mut staged: Vec<String> = Vec::new();
        for (name, doc, _, _) in &planned {
            let rel = format!(
                "{}/{}.md",
                if kind == "learning" {
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
        let message = format!("ailoom: 批量导入 {} 篇（{}）", staged.len(), target);
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
                &message,
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
            &message,
        )
    })();
    let value = match result {
        Ok(mut v) => {
            // 推送确认后记录 checkpoint（未确认提交绝不提前记成已导入）
            crate::sync_common::atomic_write(
                checkpoint(&ctx).as_path(),
                serde_json::to_vec_pretty(&cp)?.as_slice(),
            )?;
            // 删除建议落盘（审核输入，不自动删除）
            if !deletion_suggestions.is_empty() {
                let del_path = ctx.layout.ws_dir.join("import-deletions.json");
                crate::sync_common::atomic_write(
                    del_path.as_path(),
                    serde_json::to_vec_pretty(&deletion_suggestions)?.as_slice(),
                )?;
                v["deletion_suggestions_path"] = json!(del_path.display().to_string());
            }
            v["deletion_suggestions"] = json!(deletion_suggestions);
            v["skipped_unchanged"] = json!(skipped_unchanged);
            v["mode"] = json!("execute");
            v["target"] = json!(target);
            v["kind"] = json!(kind);
            v["imported"] = json!(planned.len());
            v["planned"] = json!(planned_list);
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

/// 导入文档渲染：serde_yaml 规范序列化（标题含冒号/引号/中文不产生非法 YAML）。
fn render_import_doc(
    name: &str,
    title: &str,
    target: &str,
    namespace: &str,
    source_ref: &str,
    content: &str,
    kind: &str,
) -> Result<String> {
    let mut fm = serde_yaml::Mapping::new();
    fm.insert(
        serde_yaml::Value::from("name"),
        serde_yaml::Value::from(name),
    );
    fm.insert(
        serde_yaml::Value::from("title"),
        serde_yaml::Value::from(title),
    );
    if target == "shared" {
        fm.insert(
            serde_yaml::Value::from("shared"),
            serde_yaml::Value::from(true),
        );
    } else {
        let pid = target.trim_start_matches("project:");
        // 契约差异：learning 用单数 `project`；doc 用 `projects` 数组
        if kind == "learning" {
            fm.insert(
                serde_yaml::Value::from("project"),
                serde_yaml::Value::from(pid),
            );
        } else {
            fm.insert(
                serde_yaml::Value::from("projects"),
                serde_yaml::Value::from(vec![pid]),
            );
        }
        fm.insert(
            serde_yaml::Value::from("shared"),
            serde_yaml::Value::from(false),
        );
    }
    fm.insert(
        serde_yaml::Value::from("namespace"),
        serde_yaml::Value::from(namespace),
    );
    fm.insert(
        serde_yaml::Value::from("source_ref"),
        serde_yaml::Value::from(source_ref),
    );
    let serialized = serde_yaml::to_string(&fm)
        .map_err(|e| Error::new(code::RENDER_FAILED, format!("导入文档序列化失败: {e}")))?;
    let heading = title.replace(['\r', '\n'], " ");
    let mut out = String::from("---\n");
    out.push_str(&serialized);
    out.push_str("---\n\n");
    out.push_str(content.trim_start());
    if !out.ends_with('\n') {
        out.push('\n');
    }
    let _ = heading;
    Ok(out)
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
