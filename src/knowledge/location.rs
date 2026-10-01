//! Project-owned knowledge: location-independent content, atomic shared bindings.
//! CLI and console call the same service. A move copies and verifies before switching;
//! the old directory remains a backup. Worktrees share the repository binding.
use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

const MARKER: &str = "ailoom-knowledge.json";
type Tree = BTreeMap<PathBuf, Vec<u8>>;
#[derive(Clone, Serialize, Deserialize)]
pub struct Binding {
    pub id: String,
    pub project_root: PathBuf,
    pub path: PathBuf,
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub subdir: String,
    #[serde(default)]
    pub baseline: BTreeMap<PathBuf, String>,
}
fn default_branch() -> String {
    "ailoom-knowledge".into()
}
#[derive(Serialize, Deserialize)]
struct Catalog {
    /// 契约 §0：机器可读文件带 schema_version；旧文件缺省按 1 读取。
    #[serde(default = "schema_v1")]
    schema_version: u32,
    projects: BTreeMap<String, Binding>,
}
impl Default for Catalog {
    fn default() -> Self {
        Catalog {
            schema_version: 1,
            projects: BTreeMap::new(),
        }
    }
}
fn schema_v1() -> u32 {
    1
}
fn fail(s: impl Into<String>) -> Error {
    Error::new(code::KNOWLEDGE_STATE_CONFLICT, s)
}
fn catalog_path(data: &Path) -> PathBuf {
    data.join("knowledge/bindings.json")
}
fn load(data: &Path) -> Result<Catalog> {
    match std::fs::read(catalog_path(data)) {
        Ok(v) => {
            let cat: Catalog = serde_json::from_slice(&v)?;
            if cat.schema_version != 1 {
                return Err(Error::new(
                    code::SCHEMA_VERSION,
                    format!("不支持的知识库登记版本：{}", cat.schema_version),
                ));
            }
            Ok(cat)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Catalog::default()),
        Err(e) => Err(e.into()),
    }
}
fn persist(data: &Path, cat: &Catalog) -> Result<()> {
    crate::sync_common::atomic_write(&catalog_path(data), &serde_json::to_vec_pretty(cat)?)
}
fn lock(data: &Path) -> Result<std::fs::File> {
    std::fs::create_dir_all(data.join("knowledge"))?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data.join("knowledge/operation.lock"))?;
    fs2::FileExt::lock_exclusive(&f)?;
    Ok(f)
}
/// Resolve an existing ancestor without following links silently at a new leaf.
pub fn destination(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if absolute
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(fail("路径不能包含 .."));
    }
    let mut ancestor = absolute.as_path();
    let mut tail = Vec::new();
    while !ancestor.exists() {
        if ancestor.symlink_metadata().is_ok() {
            return Err(fail("目标包含悬空链接"));
        }
        tail.push(
            ancestor
                .file_name()
                .ok_or_else(|| fail("无效路径"))?
                .to_owned(),
        );
        ancestor = ancestor.parent().ok_or_else(|| fail("无效路径"))?;
    }
    if !tail.is_empty() && !ancestor.is_dir() {
        return Err(fail("目标的上级路径不是文件夹"));
    }
    let mut result = ancestor.canonicalize()?;
    for c in tail.into_iter().rev() {
        result.push(c);
    }
    Ok(result)
}
fn project(data: &Path, root: &Path, cat: &Catalog) -> Result<(String, PathBuf)> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(fail("项目必须是文件夹"));
    }
    if crate::repo_registry::find_git_root(&root).is_some() {
        let d = crate::repo_registry::discover_repo(&root)?;
        // Reuse registered identity, including explicitly relinked repositories.
        let reg = crate::repo_registry::RepoRegistry::resolve_or_create(data, &d)?;
        return Ok((reg.repo_id, d.identity.repo_root));
    }
    if let Some((id, b)) = cat
        .projects
        .iter()
        .filter(|(_, b)| root.starts_with(&b.project_root))
        .max_by_key(|(_, b)| b.project_root.components().count())
    {
        return Ok((id.clone(), b.project_root.clone()));
    }
    // Registered non-Git sub-environments belong to their project root.
    let mut owner = root.clone();
    for ancestor in root.ancestors() {
        if super::portable::read(ancestor)?.is_some() {
            owner = ancestor.into();
            break;
        }
    }
    if let Ok(entries) = std::fs::read_dir(data.join("repos")) {
        let mut depth = 0;
        for entry in entries.flatten() {
            let p = entry.path().join("registry.json");
            if let Ok(v) = std::fs::read(&p)
                .ok()
                .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
                .ok_or(())
            {
                if v["repo_id"].as_str().unwrap_or("").starts_with("nongit-") {
                    if let Some(p) = v["common_dir"]
                        .as_str()
                        .and_then(|p| Path::new(p).canonicalize().ok())
                    {
                        if root.starts_with(&p) && p.components().count() > depth {
                            depth = p.components().count();
                            owner = p;
                        }
                    }
                }
            }
        }
    }
    Ok((
        crate::commands::personal::nongit_identity(&owner)
            .identity
            .repo_id,
        owner,
    ))
}
pub(super) fn scan(path: &Path) -> Result<Tree> {
    let mut tree = Tree::new();
    let mut size = 0u64;
    for e in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
    {
        let e = e.map_err(|e| fail(format!("读取知识库失败: {e}")))?;
        if e.file_type().is_symlink() {
            return Err(fail(format!("知识库包含符号链接: {}", e.path().display())));
        }
        if e.file_type().is_dir() {
            continue;
        }
        if !e.file_type().is_file() {
            return Err(fail("知识库包含非常规文件"));
        }
        size += e.metadata().map_err(|e| fail(e.to_string()))?.len();
        if size > 128 * 1024 * 1024 || tree.len() >= 10000 {
            return Err(fail("知识库超过本次迁移限制（128 MB / 10000 文件）"));
        }
        tree.insert(
            e.path().strip_prefix(path)?.to_path_buf(),
            std::fs::read(e.path())?,
        );
    }
    Ok(tree)
}
fn hashes(tree: &Tree) -> BTreeMap<PathBuf, String> {
    tree.iter()
        .map(|(p, v)| (p.clone(), crate::ids::sha256_hex(v)))
        .collect()
}
pub(super) fn digest(tree: &Tree) -> String {
    crate::ids::sha256_hex(&serde_json::to_vec(&hashes(tree)).unwrap())
}
pub(super) fn write_tree(path: &Path, tree: &Tree) -> Result<()> {
    for (rel, bytes) in tree {
        crate::sync_common::atomic_write(&path.join(rel), bytes)?;
    }
    Ok(())
}
fn checked_tree(b: &Binding) -> Result<Tree> {
    uuid::Uuid::parse_str(&b.id).map_err(|_| fail("知识库标识无效"))?;
    let tree = scan(&b.path)?;
    let marker: Value = serde_json::from_slice(
        tree.get(Path::new(MARKER))
            .ok_or_else(|| fail("知识库标识缺失，位置未切换"))?,
    )?;
    if marker["schema_version"] != 1 || marker["id"].as_str() != Some(&b.id) {
        return Err(fail("知识库身份与项目配置不一致"));
    }
    Ok(tree)
}
fn binding<'a>(cat: &'a Catalog, id: &str) -> Result<&'a Binding> {
    cat.projects
        .get(id)
        .ok_or_else(|| fail("尚未初始化项目知识库"))
}
#[derive(Default, Deserialize)]
pub struct Request {
    pub action: String,
    pub root: PathBuf,
    pub path: Option<PathBuf>,
    pub expected: Option<String>,
    #[serde(default)]
    pub execute: bool,
    #[serde(default)]
    pub sync_after: bool,
    pub remote: Option<String>,
    pub branch: Option<String>,
    pub subdir: Option<String>,
}
fn sync_settings(b: &mut Binding, r: &Request) -> Result<()> {
    let before = (b.remote.clone(), b.branch.clone(), b.subdir.clone());
    if let Some(remote) = &r.remote {
        b.remote = if remote.trim().is_empty() {
            None
        } else {
            if !Path::new(remote).is_absolute() {
                crate::source::GitSource::new(remote, None)?;
            }
            Some(remote.clone())
        };
    }
    if let Some(branch) = &r.branch {
        if branch.is_empty()
            || branch.starts_with('-')
            || branch.contains([' ', ':', '~', '^', '\\'])
            || branch.contains("..")
        {
            return Err(fail("无效同步分支"));
        }
        b.branch = branch.clone();
    }
    if let Some(subdir) = &r.subdir {
        if Path::new(subdir).is_absolute()
            || Path::new(subdir)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || subdir.split('/').any(|c| c == ".git")
        {
            return Err(fail("仓库子目录必须是普通相对路径"));
        }
        b.subdir = subdir.clone();
    }
    if before != (b.remote.clone(), b.branch.clone(), b.subdir.clone()) {
        b.baseline.clear();
    }
    Ok(())
}
pub(super) fn inspect_directory(path: &Path) -> Result<Value> {
    let path = destination(path)?;
    let mut ancestor = path.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or_else(|| fail("无效路径"))?;
    }
    if !ancestor.is_dir() {
        return Err(fail("请选择文件夹"));
    }
    let repo = match git(ancestor, &["rev-parse", "--show-toplevel"]) {
        Ok(repo) => repo.trim().to_owned(),
        Err(_) => return Ok(json!({"path":path,"git":null})),
    };
    let branch = git(ancestor, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "游离 HEAD".into());
    let remote = git(ancestor, &["remote", "get-url", "origin"])
        .ok()
        .map(|s| crate::gitx::redact_credentials(s.trim()));
    Ok(json!({"path":path,"git":{"root":repo,"branch":branch,"remote":remote}}))
}
pub fn run(data: &Path, r: &Request) -> Result<Value> {
    if r.action == "inspect" {
        return inspect_directory(r.path.as_deref().ok_or_else(|| fail("请选择保存目录"))?);
    }
    let _guard = lock(data)?;
    let mut cat = load(data)?;
    let (id, root) = project(data, &r.root, &cat)?;
    if r.action == "clone" {
        return super::portable::clone_repository(
            &root,
            r.path.as_deref().ok_or_else(|| fail("请选择克隆目录"))?,
        );
    }
    let recovery = super::portable::recovery(&root)?;
    if r.action == "status" {
        return Ok(match cat.projects.get(&id) {
            None => {
                json!({"initialized":false,"project_id":id,"project_root":root,"default_path":recovery["suggested_path"].as_str().map(PathBuf::from).unwrap_or_else(|| data.join("knowledge/projects").join(&id)),"recovery":recovery})
            }
            Some(b) if !b.path.exists() => {
                json!({"initialized":false,"project_id":id,"project_root":root,"previous_path":b.path,"default_path":recovery["suggested_path"],"recovery":recovery})
            }
            Some(b) => {
                let tree = checked_tree(b)?;
                json!({"initialized":true,"project_id":id,"project_root":root,"location":b,"files":tree.len()-1,"fingerprint":digest(&tree),"pending":hashes(&tree)!=b.baseline,"recovery":recovery})
            }
        });
    }
    if r.action == "recover"
        || (r.action == "init" && !recovery.is_null() && !cat.projects.contains_key(&id))
    {
        let declaration = super::portable::read(&root)?.ok_or_else(|| fail("项目没有恢复配置"))?;
        let suggested = super::portable::suggested(&root, &declaration)?;
        let dest = destination(
            r.path
                .as_deref()
                .or(suggested.as_deref())
                .ok_or_else(|| fail("请同步知识库并选择本机目录，或从 Git 克隆"))?,
        )?;
        let b = Binding {
            id: declaration.knowledge_id.clone(),
            project_root: root.clone(),
            path: dest,
            remote: None,
            branch: default_branch(),
            subdir: String::new(),
            baseline: BTreeMap::new(),
        };
        let tree = checked_tree(&b)?;
        if let Some(existing) = cat
            .projects
            .values()
            .find(|old| old.id == b.id && old.path != b.path && old.path.exists())
        {
            return Err(fail(format!(
                "该知识库已关联到 {}；请选用该目录或使用迁移",
                existing.path.display()
            )));
        }
        let restored = super::state::restore(data, &id, &b, &declaration, false)?;
        let token = crate::ids::sha256_hex(&serde_json::to_vec(
            &json!({"declaration":declaration,"path":b.path,"tree":digest(&tree),"old":cat.projects.get(&id),"local":super::state::local_fingerprint(data,&id)?}),
        )?);
        if !r.execute {
            return Ok(
                json!({"preview":true,"expected":token,"to":b.path,"files":tree.len()-1,"project_id":id,"knowledge_id":b.id,"state":restored}),
            );
        }
        if r.expected.as_deref() != Some(&token) {
            return Err(fail("恢复内容已变化，请重新预览"));
        }
        register(data, &id, &b)?;
        super::state::restore(data, &id, &b, &declaration, true)?;
        cat.projects.insert(id.clone(), b.clone());
        persist(data, &cat)?;
        return Ok(json!({"recovered":true,"initialized":true,"project_id":id,"location":b}));
    }
    if r.action == "init" {
        let dest = destination(
            r.path
                .as_deref()
                .unwrap_or(&data.join("knowledge/projects").join(&id)),
        )?;
        if let Some(b) = cat.projects.get(&id) {
            if b.path == dest {
                checked_tree(b)?;
                let declaration = super::portable::describe(b)?;
                let state = if b.remote.is_none() {
                    super::state::checkpoint(data, &id, b, &declaration)?
                } else {
                    Value::Null
                };
                super::portable::write(&b.project_root, &declaration)?;
                return Ok(json!({"initialized":true,"project_id":id,"location":b,"state":state}));
            }
            return Err(fail("项目已有知识库，请使用迁移位置"));
        }
        let mut b = Binding {
            id: crate::ids::new_id(),
            project_root: root,
            path: dest.clone(),
            remote: None,
            branch: default_branch(),
            subdir: String::new(),
            baseline: BTreeMap::new(),
        };
        sync_settings(&mut b, r)?;
        if dest.join(MARKER).is_file() {
            let v: Value = serde_json::from_slice(&std::fs::read(dest.join(MARKER))?)?;
            b.id = v["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| fail("无效知识库标识"))?
                .into();
            checked_tree(&b)?;
            if let Some(existing) = cat.projects.values().find(|old| old.id == b.id) {
                if existing.path != dest {
                    return Err(fail("该知识库已有其他活动位置；请使用迁移或选择原位置"));
                }
                b.remote = existing.remote.clone();
                b.branch = existing.branch.clone();
                b.subdir = existing.subdir.clone();
                b.baseline = existing.baseline.clone();
            }
        } else {
            if dest.exists() {
                scan(&dest)?;
                for name in ["learnings", "docs"] {
                    if dest.join(name).exists() && !dest.join(name).is_dir() {
                        return Err(fail(format!("{name} 已存在且不是文件夹")));
                    }
                }
            }
            std::fs::create_dir_all(dest.join("learnings"))?;
            std::fs::create_dir_all(dest.join("docs"))?;
            crate::sync_common::atomic_write(
                &dest.join(MARKER),
                &serde_json::to_vec_pretty(&json!({"schema_version":1,"id":b.id}))?,
            )?;
        }
        let declaration = super::portable::describe(&b)?;
        super::portable::write(&b.project_root, &declaration)?;
        register(data, &id, &b)?;
        cat.projects.insert(id.clone(), b.clone());
        persist(data, &cat)?;
        // Legacy explicit remote-sync initialization adopts a remote marker on first sync.
        // Keep its empty bootstrap eligible; new directory-based initialization snapshots now.
        let state = if b.remote.is_none() {
            super::state::checkpoint(data, &id, &b, &declaration)?
        } else {
            Value::Null
        };
        return Ok(json!({"initialized":true,"project_id":id,"location":b,"state":state}));
    }
    let old = binding(&cat, &id)?.clone();
    let source = checked_tree(&old)?;
    if r.action == "checkpoint" {
        let declaration = super::portable::describe(&old)?;
        let state = super::state::checkpoint(data, &id, &old, &declaration)?;
        super::portable::write(&old.project_root, &declaration)?;
        return Ok(json!({"saved":true,"declaration":declaration,"state":state}));
    }
    if r.action == "sync" {
        return sync(data, &mut cat, &old, &source);
    }
    if r.action == "configure" {
        let mut next = old.clone();
        sync_settings(&mut next, r)?;
        let token = crate::ids::sha256_hex(&serde_json::to_vec(
            &json!({"old":old,"next":next,"tree":digest(&source)}),
        )?);
        if !r.execute {
            return Ok(
                json!({"preview":true,"expected":token,"from":old.path,"to":old.path,"files":source.len()-1,"affected_projects":cat.projects.iter().filter(|(_,b)|b.id==old.id).map(|(id,_)|id).collect::<Vec<_>>(),"configuration_only":true}),
            );
        }
        if r.expected.as_deref() != Some(&token) {
            return Err(fail("配置已变化，请重新预览"));
        }
        for b in cat.projects.values_mut().filter(|b| b.id == old.id) {
            let root = b.project_root.clone();
            *b = next.clone();
            b.project_root = root;
        }
        persist(data, &cat)?;
        return Ok(json!({"configured":true,"location":next}));
    }
    if r.action != "move" {
        return Err(fail("未知知识库操作"));
    }
    let dest = destination(r.path.as_deref().ok_or_else(|| fail("请选择目标目录"))?)?;
    if old.path.starts_with(&dest) || dest.starts_with(&old.path) {
        return Err(fail("新旧位置相同或嵌套，不能迁移"));
    }
    if dest.exists() && !dest.is_dir() {
        return Err(fail("目标不是文件夹"));
    }
    let target = if dest.exists() {
        scan(&dest)?
    } else {
        Tree::new()
    };
    let resumable = target.iter().all(|(p, v)| source.get(p) == Some(v))
        && std::fs::read_dir(data.join("knowledge/migrations"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                std::fs::read(entry.path())
                    .ok()
                    .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
                    .map(|v| {
                        v["old"]["id"].as_str() == Some(&old.id)
                            && v["next"]["path"].as_str() == dest.to_str()
                            && v["digest"].as_str() == Some(&digest(&source))
                    })
                    .unwrap_or(false)
            });
    if !target.is_empty() && target != source && !resumable {
        return Err(fail("目标目录已有其他内容，不能覆盖；请选择新的子目录"));
    }
    let mut next = old.clone();
    next.path = dest.clone();
    sync_settings(&mut next, r)?;
    let token = crate::ids::sha256_hex(&serde_json::to_vec(
        &json!({"source":digest(&source),"target":digest(&target),"old":old,"next":next,"sync_after":r.sync_after}),
    )?);
    let affected: Vec<_> = cat
        .projects
        .iter()
        .filter(|(_, b)| b.id == old.id)
        .map(|(id, _)| id.clone())
        .collect();
    if !r.execute {
        return Ok(
            json!({"preview":true,"expected":token,"from":old.path,"to":dest,"files":source.len()-1,"affected_projects":affected,"remote":next.remote}),
        );
    }
    if r.expected.as_deref() != Some(&token) {
        return Err(fail("知识库或迁移设置已变化，请重新预览"));
    }
    // Durable intent and old binding remain available even after process interruption.
    let record = data
        .join("knowledge/migrations")
        .join(format!("{}.json", crate::ids::new_id()));
    crate::sync_common::atomic_write(
        &record,
        &serde_json::to_vec_pretty(
            &json!({"schema_version":1,"state":"copying","old":old,"next":next,"affected_projects":affected,"digest":digest(&source)}),
        )?,
    )?;
    write_tree(&dest, &source)?;
    if scan(&dest)? != source || checked_tree(&old)? != source {
        return Err(fail("迁移期间内容发生变化；原位置仍有效，请重新预览"));
    }
    super::state::relocate(data, &old, &next)?;
    for b in cat.projects.values_mut().filter(|b| b.id == old.id) {
        let root = b.project_root.clone();
        *b = next.clone();
        b.project_root = root;
    }
    for b in cat.projects.values().filter(|b| b.id == old.id) {
        let declaration = super::portable::describe(b)?;
        super::portable::write(&b.project_root, &declaration)?;
    }
    persist(data, &cat)?;
    // 位置已切换：记录收尾失败不回滚迁移，但必须如实告知记录仍停在 copying。
    let record_written = crate::sync_common::atomic_write(
        &record,
        &serde_json::to_vec_pretty(
            &json!({"schema_version":1,"state":"switched","old":old,"next":next,"affected_projects":affected}),
        )?,
    );
    let mut result = json!({"moved":true,"location":next,"backup":old.path,"record":record,"affected_projects":affected,"synced":false});
    if let Err(e) = record_written {
        result["record_warning"] = json!(format!("迁移已完成，但迁移记录未更新为 switched：{e}"));
    }
    if r.sync_after && next.remote.is_some() {
        match sync(data, &mut cat, &next, &source) {
            Ok(v) => {
                result["synced"] = json!(true);
                result["location"] = v["location"].clone();
            }
            Err(e) => {
                result["sync_error"] = json!(e.to_string());
            }
        }
    }
    Ok(result)
}

pub fn is_initialized(data: &Path, root: &Path) -> Result<bool> {
    let cat = load(data)?;
    let (id, _) = project(data, root, &cat)?;
    Ok(cat.projects.contains_key(&id))
}
pub fn save(data: &Path, root: &Path, file: &Path, name: &str) -> Result<Value> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(fail("名称必须是单个文件名"));
    }
    let content = std::fs::read(file)?;
    if content.is_empty() || content.len() > 4 * 1024 * 1024 {
        return Err(fail("知识文档需为非空 Markdown，且不超过 4 MB"));
    }
    std::str::from_utf8(&content).map_err(|_| fail("知识文档必须是 UTF-8 文本"))?;
    let _guard = lock(data)?;
    let cat = load(data)?;
    let (id, _) = project(data, root, &cat)?;
    let b = binding(&cat, &id)?;
    checked_tree(b)?;
    let name = if name.ends_with(".md") {
        name.into()
    } else {
        format!("{name}.md")
    };
    let dest = b.path.join("learnings").join(name);
    // No silent overwrite of another note; repeated identical saves are idempotent.
    if dest.exists() && std::fs::read(&dest)? != content {
        return Err(fail("同名知识已存在，请使用新名称"));
    }
    crate::sync_common::atomic_write(&dest, &content)?;
    Ok(json!({"saved":true,"path":dest,"knowledge_id":b.id}))
}
pub fn recall(data: &Path, root: &Path, query: &str, limit: usize) -> Result<Value> {
    recall_filtered(data, root, query, limit, None)
}
pub fn recall_filtered(
    data: &Path,
    root: &Path,
    query: &str,
    limit: usize,
    kind: Option<&str>,
) -> Result<Value> {
    let _guard = lock(data)?;
    let cat = load(data)?;
    let (id, _) = project(data, root, &cat)?;
    let b = binding(&cat, &id)?;
    let tree = checked_tree(b)?;
    let ctx = state_context(data, root, b)?;
    let archived = super::feedback::archived_ids(&ctx);
    let mut index = super::index::KnowledgeIndex {
        meta: super::index::IndexMeta {
            schema_version: 1,
            revision: None,
            content_digest: digest(&tree),
            active_projects: vec![id],
            active_roles: vec![],
            identity: b.id.clone(),
            built_at: crate::ids::now_iso(),
        },
        documents: Vec::new(),
        postings: BTreeMap::new(),
    };
    for (p, v) in &tree {
        if p.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Ok(body) = std::str::from_utf8(v) else {
            continue;
        };
        let id = p.to_string_lossy().to_string();
        if archived.contains(&id) {
            continue;
        }
        let title = body
            .lines()
            .find(|l| l.starts_with("# "))
            .unwrap_or(&id)
            .trim_start_matches("# ")
            .to_string();
        let document_kind = if p.components().any(|c| c.as_os_str() == "docs") {
            "doc"
        } else if p.components().any(|c| c.as_os_str() == "rules") {
            "rule"
        } else if p.components().any(|c| c.as_os_str() == "skills") {
            "skill"
        } else {
            "learning"
        };
        if kind.is_some_and(|k| k != document_kind) {
            continue;
        }
        let position = index.documents.len();
        for (text, weight) in [(title.as_str(), 3.0), (body, 1.0), (id.as_str(), 2.0)] {
            for term in super::index::tokenize(text) {
                *index
                    .postings
                    .entry(term)
                    .or_default()
                    .entry(position)
                    .or_insert(0.0) += weight;
            }
        }
        index.documents.push(super::index::IndexDocument {
            id: id.clone(),
            kind: document_kind.into(),
            name: id,
            title,
            scope: "project".into(),
            source_path: b.path.join(p).to_string_lossy().into(),
            body: body.into(),
        });
    }
    let hits = super::search::search(&index, query, limit);
    super::feedback::record_recall_hits(
        &ctx,
        &hits.iter().map(|h| h.id.clone()).collect::<Vec<_>>(),
    )?;
    Ok(json!({"results":hits,"knowledge_id":b.id,"query":query}))
}

fn state_context(data: &Path, root: &Path, b: &Binding) -> Result<crate::appctx::AppContext> {
    let mut layout = crate::paths::layout_for(data, &format!("knowledge-{}", b.id), &b.id);
    layout.ws_dir = data.join("knowledge/state").join(&b.id);
    layout.index_dir = layout.ws_dir.join("index");
    Ok(crate::appctx::AppContext {
        data_root: data.into(),
        layout,
        device: crate::config::device_id(data)?,
        workspace: crate::workspace::Workspace {
            workspace_root: root.canonicalize()?,
            repository_anchor: b.id.clone(),
            anchor_key: b.id.clone(),
            workspace_id: b.id.clone(),
            is_git: crate::repo_registry::find_git_root(root).is_some(),
            declaration_path: None,
        },
    })
}
pub fn maintenance_context(data: &Path, root: &Path) -> Result<Option<crate::appctx::AppContext>> {
    let cat = load(data)?;
    let (id, _) = project(data, root, &cat)?;
    cat.projects
        .get(&id)
        .map(|b| state_context(data, root, b))
        .transpose()
}

pub(super) fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !out.status.success() {
        return Err(fail(format!(
            "Git 同步失败：{}",
            crate::gitx::redact_credentials(String::from_utf8_lossy(&out.stderr).trim())
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into())
}
fn sync(data: &Path, cat: &mut Catalog, b: &Binding, local: &Tree) -> Result<Value> {
    let remote = b
        .remote
        .as_deref()
        .ok_or_else(|| fail("知识库仅保存在本地，尚未设置 Git 同步地址"))?;
    git(
        data,
        &["check-ref-format", &format!("refs/heads/{}", b.branch)],
    )?;
    let temp = data.join("knowledge/sync").join(crate::ids::new_id());
    std::fs::create_dir_all(&temp)?;
    let checkout = temp.join("repo");
    git(
        &temp,
        &[
            "clone",
            "--no-checkout",
            "--",
            remote,
            checkout.to_str().ok_or_else(|| fail("无效路径"))?,
        ],
    )?;
    let remote_ref = format!("refs/remotes/origin/{}", b.branch);
    let exists = std::process::Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &remote_ref])
        .current_dir(&checkout)
        .status()?
        .success();
    if exists {
        git(&checkout, &["checkout", "-B", &b.branch, &remote_ref])?;
    } else {
        git(&checkout, &["switch", "--orphan", &b.branch])?;
    }
    let dir = checkout.join(&b.subdir);
    let resolved = destination(&dir)?;
    if !resolved.starts_with(checkout.canonicalize()?) {
        return Err(fail("远端知识目录包含越界链接"));
    }
    let incoming = if dir.exists() {
        scan(&dir)?
    } else {
        Tree::new()
    };
    let original = local.clone();
    let mut next = b.clone();
    let mut local = local.clone();
    if let Some(marker) = incoming.get(Path::new(MARKER)) {
        let m: Value = serde_json::from_slice(marker)?;
        if m["id"].as_str() != Some(&b.id) {
            if local.len() == 1 && b.baseline.is_empty() {
                next.id = m["id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| fail("远端知识库标识无效"))?
                    .into();
                local.insert(PathBuf::from(MARKER), marker.clone());
            } else {
                return Err(fail("远端子目录属于另一份知识库，请更换子目录"));
            }
        }
    } else if !incoming.is_empty() {
        return Err(fail("远端子目录已有非知识库内容，拒绝覆盖"));
    }
    if cat
        .projects
        .values()
        .any(|x| x.id == next.id && x.path != b.path)
    {
        return Err(fail(
            "本机已经关联这份远端知识库，请关联已有本地位置，避免产生两份活动副本",
        ));
    }
    uuid::Uuid::parse_str(&next.id).map_err(|_| fail("远端知识库标识无效"))?;
    let lhash = hashes(&local);
    let rhash = hashes(&incoming);
    let mut merged = Tree::new();
    let keys: std::collections::BTreeSet<_> = lhash
        .keys()
        .chain(rhash.keys())
        .chain(b.baseline.keys())
        .cloned()
        .collect();
    let mut conflicts = Vec::new();
    for p in keys {
        let l = lhash.get(&p);
        let r = rhash.get(&p);
        let base = b.baseline.get(&p);
        let selected = if l == r {
            local.get(&p)
        } else if l == base {
            incoming.get(&p)
        } else if r == base {
            local.get(&p)
        } else {
            conflicts.push(p.display().to_string());
            None
        };
        if let Some(v) = selected {
            merged.insert(p, v.clone());
        }
    }
    if !conflicts.is_empty() {
        return Err(fail(format!(
            "同一知识存在冲突，未改写本地或远端：{}",
            conflicts.join("、")
        )));
    }
    if !merged.contains_key(Path::new(MARKER)) {
        return Err(fail("知识库标识不能删除"));
    }
    // Never publish writes based on a stale copy of externally edited local files.
    if checked_tree(b)? != original {
        return Err(fail("同步期间本地知识已变化，请重试"));
    }
    for p in incoming.keys().filter(|p| !merged.contains_key(*p)) {
        std::fs::remove_file(dir.join(p))?;
    }
    write_tree(&dir, &merged)?;
    let pathspec = if b.subdir.is_empty() { "." } else { &b.subdir };
    git(
        &checkout,
        &["--literal-pathspecs", "add", "-A", "--", pathspec],
    )?;
    let staged = git(&checkout, &["diff", "--cached", "--name-only"])?;
    if !staged.trim().is_empty() {
        git(
            &checkout,
            &[
                "-c",
                "user.name=AILoom",
                "-c",
                "user.email=knowledge@ailoom.local",
                "commit",
                "-m",
                "Update project knowledge",
            ],
        )?;
    }
    git(
        &checkout,
        &["push", "origin", &format!("HEAD:refs/heads/{}", b.branch)],
    )?;
    if checked_tree(b)? != original {
        return Err(fail(
            "远端已同步，但本地在同步期间被修改；已保留本地修改，请重新同步",
        ));
    }
    // Remote now contains the merged version. A failed local write can be retried.
    for p in local.keys().filter(|p| !merged.contains_key(*p)) {
        std::fs::remove_file(b.path.join(p))?;
    }
    write_tree(&b.path, &merged)?;
    next.baseline = hashes(&merged);
    for x in cat.projects.values_mut().filter(|x| x.id == b.id) {
        let root = x.project_root.clone();
        *x = next.clone();
        x.project_root = root;
        if let Some(mut declaration) = super::portable::read(&x.project_root)? {
            declaration.knowledge_id = next.id.clone();
            super::portable::write(&x.project_root, &declaration)?;
        }
    }
    persist(data, cat)?;
    let _ = std::fs::remove_dir_all(&temp);
    Ok(json!({"synced":true,"files":merged.len()-1,"location":next}))
}

fn register(data: &Path, id: &str, b: &Binding) -> Result<()> {
    let discovery = if crate::repo_registry::find_git_root(&b.project_root).is_some() {
        crate::repo_registry::discover_repo(&b.project_root)?
    } else {
        crate::commands::personal::nongit_identity(&b.project_root)
    };
    let mut registry = crate::repo_registry::RepoRegistry::resolve_or_create(data, &discovery)?;
    if !id.starts_with("nongit-") {
        registry.refresh_worktrees(&discovery, &crate::ids::now_iso());
    }
    registry.save(data)
}

pub fn checkpoint_project(data: &Path, repo: &str) -> Result<()> {
    let guard = lock(data)?;
    let cat = load(data)?;
    let Some(b) = cat.projects.get(repo).cloned() else {
        return Ok(());
    };
    drop(guard);
    run(
        data,
        &Request {
            action: "checkpoint".into(),
            root: b.project_root,
            ..Default::default()
        },
    )
    .map_err(|e| fail(format!("本机配置已保存，但知识库恢复副本未更新：{e}")))?;
    Ok(())
}
