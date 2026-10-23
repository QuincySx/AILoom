//! Incremental, project-owned source documents. Originals are immutable; the manifest commits a batch.
use super::*;
use std::collections::BTreeSet;
use std::io::Read;

pub(super) const PREFIX: &str = "imports/worktrees";

#[derive(Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub worktree: PathBuf,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Origin {
    pub worktree_id: String,
    pub directory: String,
    pub document: PathBuf,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub worktree_dirty: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    hash: String,
    origin: Origin,
    captured_at: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    schema_version: u32,
    records: Vec<Record>,
}

pub(super) fn project_key(id: &str, root: &Path) -> Result<String> {
    Ok(crate::knowledge::portable::read(root)?
        .map(|d| d.project_id)
        .unwrap_or_else(|| id.into()))
}

fn manifest_path(project: &str) -> PathBuf {
    Path::new(PREFIX).join(project).join("manifest.json")
}

fn document_path(project: &str, hash: &str) -> PathBuf {
    Path::new(PREFIX).join(project).join(format!("{hash}.md"))
}

fn manifest(tree: &Tree, project: &str) -> Result<Manifest> {
    let Some(bytes) = tree.get(&manifest_path(project)) else {
        return Ok(Manifest {
            schema_version: 1,
            ..Default::default()
        });
    };
    let m: Manifest = serde_json::from_slice(bytes)?;
    if m.schema_version != 1
        || m.records
            .iter()
            .any(|r| r.hash.len() != 64 || !r.hash.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(fail("采集记录格式无效"));
    }
    Ok(m)
}

/// Only the latest revision for each worktree/document is current. Historical originals remain on disk.
pub(super) fn current_documents(
    tree: &Tree,
    project: &str,
) -> Result<BTreeMap<PathBuf, Vec<Origin>>> {
    let m = manifest(tree, project)?;
    let mut latest = BTreeMap::new();
    for record in m.records {
        latest.insert(
            (
                record.origin.worktree_id.clone(),
                record.origin.document.clone(),
            ),
            record,
        );
    }
    let mut result: BTreeMap<PathBuf, Vec<Origin>> = BTreeMap::new();
    for record in latest.into_values() {
        let path = document_path(project, &record.hash);
        let bytes = tree
            .get(&path)
            .ok_or_else(|| fail("已采集的原文缺失，请恢复知识库原文"))?;
        if crate::ids::sha256_hex(bytes) != record.hash {
            return Err(fail("已采集的原文发生变化，请恢复原文"));
        }
        result.entry(path).or_default().push(record.origin);
    }
    Ok(result)
}

fn sources_unlocked(data: &Path, root: &Path, id: &str) -> Result<Vec<Source>> {
    let discovery = if crate::repo_registry::find_git_root(root).is_some() {
        crate::repo_registry::discover_repo(root)?
    } else {
        crate::commands::personal::nongit_identity(root)
    };
    let registry = crate::repo_registry::RepoRegistry::resolve_or_create(data, &discovery)?;
    let profile = crate::profile::PersonalProfile::load_or_default(data)?;
    let rp = profile.repo(id);
    let mut result = BTreeMap::new();
    let mut bases: Vec<_> = discovery
        .worktrees
        .iter()
        .filter(|w| w.deployable() && w.path.is_dir())
        .map(|w| (w.path.clone(), w.stable_key(), w.branch.clone()))
        .collect();
    if crate::repo_registry::find_git_root(root).is_none() {
        bases.push((root.to_path_buf(), String::new(), None));
    }
    for (path, fallback_key, branch) in bases {
        let base = path.canonicalize()?;
        let key = registry
            .worktrees
            .values()
            .find(|w| w.path == base)
            .map(|w| w.id.clone())
            .unwrap_or(fallback_key);
        let name = if base == root {
            "主工作目录".to_string()
        } else {
            branch
                .as_deref()
                .unwrap_or("游离工作目录")
                .trim_start_matches("refs/heads/")
                .to_string()
        };
        let mut dirs: BTreeSet<String> = registry.subprojects.keys().cloned().collect();
        dirs.extend(rp.subprojects.iter().map(|s| s.path.clone()));
        if let Some(children) = rp.wt_subprojects.get(&key) {
            dirs.extend(children.iter().map(|s| s.path.clone()));
        }
        dirs.insert(String::new());
        for rel in dirs {
            let joined = base.join(&rel);
            let Ok(path) = joined.canonicalize() else {
                continue;
            };
            if !path.is_dir() || !path.starts_with(&base) {
                continue;
            }
            // A nested independent repository is not a child of this project.
            if let Some(git_root) = crate::repo_registry::find_git_root(&path) {
                if git_root.canonicalize()? != base {
                    continue;
                }
            }
            result.entry(path.clone()).or_insert_with(|| Source {
                id: crate::ids::sha256_hex(path.to_string_lossy().as_bytes()),
                label: if rel.is_empty() {
                    name.clone()
                } else {
                    format!("{name} / {rel}")
                },
                path,
                worktree: base.clone(),
            });
        }
    }
    let mut result: Vec<_> = result.into_values().collect();
    result.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(result)
}

pub fn status(data: &Path, root: &Path) -> Result<Value> {
    let _guard = lock(data)?;
    let cat = load(data)?;
    let (id, root) = project(data, root, &cat)?;
    let b = binding(&cat, &id)?;
    let tree = checked_tree(b)?;
    let key = project_key(&id, &root)?;
    let current = current_documents(&tree, &key)?;
    Ok(
        json!({"sources":sources_unlocked(data,&root,&id)?,"document_count":current.len(),"revision_count":manifest(&tree,&key)?.records.len(),"knowledge_path":b.path}),
    )
}

fn excluded(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".ssh"
            | ".gnupg"
            | ".aws"
            | ".kube"
            | ".codex"
            | ".claude"
            | ".agents"
            | ".ailoom"
            | ".pi"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "vendor"
            | ".cache"
            | ".venv"
            | "venv"
            | "__pycache__"
    ) || name.starts_with(".env")
        || name.to_ascii_lowercase().contains("credential")
}

#[derive(Serialize)]
struct Document {
    path: PathBuf,
    title: String,
    bytes: usize,
    hash: String,
    state: &'static str,
}

struct Snapshot {
    source: Source,
    origin: Origin,
    documents: Vec<(Document, Vec<u8>)>,
    skipped: Vec<Value>,
    expected: String,
}

fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn snapshot(
    data: &Path,
    root: &Path,
    id: &str,
    b: &Binding,
    tree: &Tree,
    source_id: &str,
    key: &str,
) -> Result<Snapshot> {
    let source = sources_unlocked(data, root, id)?
        .into_iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| fail("请选择本项目已有的工作目录或子目录"))?;
    if b.path == source.path {
        return Err(fail("不能把知识库本身作为采集来源"));
    }
    current_documents(tree, key)?;
    let m = manifest(tree, key)?;
    let worktree_dirty = if crate::repo_registry::find_git_root(&source.worktree).is_some() {
        !git_text(
            &source.worktree,
            &["status", "--porcelain=v1", "--untracked-files=normal"],
        )
        .ok_or_else(|| fail("无法读取来源工作目录的 Git 状态，请修复后重新扫描"))?
        .is_empty()
    } else {
        false
    };
    let origin = Origin {
        worktree_id: crate::ids::sha256_hex(source.worktree.to_string_lossy().as_bytes()),
        directory: source.label.clone(),
        document: PathBuf::new(),
        branch: git_text(
            &source.worktree,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
        ),
        commit: git_text(&source.worktree, &["rev-parse", "--verify", "HEAD"]),
        worktree_dirty,
    };
    let mut documents = Vec::new();
    let mut skipped = Vec::new();
    let mut size = 0usize;
    for entry in walkdir::WalkDir::new(&source.path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            (e.path() == source.path || !excluded(&e.file_name().to_string_lossy()))
                && !e.path().starts_with(&b.path)
                && (e.path() == source.path
                    || !e.file_type().is_dir()
                    || crate::repo_registry::find_git_root(e.path()).as_deref() != Some(e.path()))
        })
    {
        let entry = entry.map_err(|_| fail("来源目录有无法读取的路径，请缩小采集范围"))?;
        if entry.file_type().is_symlink() || !entry.file_type().is_file() {
            continue;
        }
        if !entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "md" | "markdown"))
        {
            continue;
        }
        let path = entry.path().strip_prefix(&source.worktree)?.to_path_buf();
        if entry
            .metadata()
            .map(|m| m.len() > 4 * 1024 * 1024)
            .unwrap_or(true)
        {
            skipped.push(json!({"path":path,"reason":"无法读取或超过 4 MB"}));
            continue;
        }
        let read = (|| -> std::io::Result<Vec<u8>> {
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW);
            }
            let file = options.open(entry.path())?;
            let mut bytes = Vec::new();
            file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        })();
        let bytes = match read {
            Ok(bytes) if !bytes.is_empty() && bytes.len() <= 4 * 1024 * 1024 => bytes,
            _ => {
                skipped.push(json!({"path":path,"reason":"无法读取、空文件或超过 4 MB"}));
                continue;
            }
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(s) => s,
            Err(_) => {
                skipped.push(json!({"path":path,"reason":"不是 UTF-8 文本"}));
                continue;
            }
        };
        size += bytes.len();
        if size > 64 * 1024 * 1024 || documents.len() >= 2000 {
            return Err(fail("一次最多采集 2000 篇或 64 MB，请选择较小的子目录"));
        }
        let hash = crate::ids::sha256_hex(&bytes);
        let last = m
            .records
            .iter()
            .rev()
            .find(|r| r.origin.worktree_id == origin.worktree_id && r.origin.document == path);
        let document_origin = Origin {
            document: path.clone(),
            ..origin.clone()
        };
        let state = if last.is_some_and(|r| r.hash == hash && r.origin == document_origin) {
            "unchanged"
        } else if last.is_some_and(|r| r.hash == hash) {
            "context_updated"
        } else if last.is_some() {
            "updated"
        } else if m.records.iter().any(|r| r.hash == hash) {
            "duplicate"
        } else {
            "new"
        };
        let title = text
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .unwrap_or_else(|| path.to_str().unwrap_or("未命名文档"))
            .to_string();
        documents.push((
            Document {
                path,
                title,
                bytes: bytes.len(),
                hash,
                state,
            },
            bytes,
        ));
    }
    documents.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    let expected = crate::ids::sha256_hex(&serde_json::to_vec(
        &json!({"source":source,"origin":origin,"documents":documents.iter().map(|(d,_)|d).collect::<Vec<_>>(),"manifest":m,"knowledge_id":b.id,"knowledge_path":b.path}),
    )?);
    Ok(Snapshot {
        source,
        origin,
        documents,
        skipped,
        expected,
    })
}

pub fn preview(data: &Path, root: &Path, source_id: &str) -> Result<Value> {
    let _guard = lock(data)?;
    let cat = load(data)?;
    let (id, root) = project(data, root, &cat)?;
    let b = binding(&cat, &id)?;
    let tree = checked_tree(b)?;
    let s = snapshot(
        data,
        &root,
        &id,
        b,
        &tree,
        source_id,
        &project_key(&id, &root)?,
    )?;
    Ok(
        json!({"source":s.source,"origin":s.origin,"documents":s.documents.iter().map(|(d,_)|d).collect::<Vec<_>>(),"skipped":s.skipped,"expected":s.expected}),
    )
}

pub fn apply(
    data: &Path,
    root: &Path,
    source_id: &str,
    expected: &str,
    selected: &[PathBuf],
) -> Result<Value> {
    if selected.is_empty() {
        return Err(fail("请先勾选要保存的开发文档"));
    }
    let _guard = lock(data)?;
    let cat = load(data)?;
    let (id, root) = project(data, root, &cat)?;
    let b = binding(&cat, &id)?;
    let mut tree = checked_tree(b)?;
    let key = project_key(&id, &root)?;
    let s = snapshot(data, &root, &id, b, &tree, source_id, &key)?;
    if s.expected != expected {
        return Err(fail("来源文档、分支或采集记录已变化，请重新扫描"));
    }
    let selected: BTreeSet<&PathBuf> = selected.iter().collect();
    if selected
        .iter()
        .any(|p| !s.documents.iter().any(|(d, _)| &d.path == *p))
    {
        return Err(fail("所选文档不在本次扫描清单中"));
    }
    let mut m = manifest(&tree, &key)?;
    let mut added = 0;
    let mut reused = 0;
    let mut writes = Tree::new();
    for (d, bytes) in s
        .documents
        .iter()
        .filter(|(d, _)| selected.contains(&d.path))
    {
        let path = document_path(&key, &d.hash);
        if let Some(previous) = tree.get(&path) {
            if previous != bytes {
                return Err(fail("知识库中的原文与内容摘要不符，未覆盖"));
            }
            reused += 1;
        } else {
            writes.insert(path.clone(), bytes.clone());
            tree.insert(path, bytes.clone());
            added += 1;
        }
        let mut origin = s.origin.clone();
        origin.document = d.path.clone();
        let last = m.records.iter().rev().find(|r| {
            r.origin.worktree_id == origin.worktree_id && r.origin.document == origin.document
        });
        if !last.is_some_and(|r| r.hash == d.hash && r.origin == origin) {
            m.records.push(Record {
                hash: d.hash.clone(),
                origin,
                captured_at: crate::ids::now_iso(),
            });
        }
    }
    let metadata = serde_json::to_vec_pretty(&m)?;
    tree.insert(manifest_path(&key), metadata.clone());
    if tree.len() > 10000 || tree.values().map(Vec::len).sum::<usize>() > 128 * 1024 * 1024 {
        return Err(fail("采集后知识库超过 128 MB / 10000 文件限制，未保存"));
    }
    let document_count = current_documents(&tree, &key)?.len();
    // Originals become visible only after the final manifest write. A failed batch can be retried.
    write_tree(&b.path, &writes)?;
    crate::sync_common::atomic_write(&b.path.join(manifest_path(&key)), &metadata)?;
    Ok(
        json!({"saved":selected.len(),"new_documents":added,"reused_documents":reused,"document_count":document_count,"revision_count":m.records.len()}),
    )
}
