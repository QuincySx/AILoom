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

pub(super) fn sources_unlocked(data: &Path, root: &Path, id: &str) -> Result<Vec<Source>> {
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

fn source_unlocked(data: &Path, root: &Path, id: &str, source_id: &str) -> Result<Source> {
    sources_unlocked(data, root, id)?
        .into_iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| fail("请选择本项目已有的工作目录或子目录"))
}

fn snapshot(
    source: Source,
    b: &Binding,
    tree: &Tree,
    key: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Snapshot> {
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
    let local_library = b
        .path
        .strip_prefix(&b.project_root)
        .ok()
        .map(|rel| source.worktree.join(rel));
    for entry in walkdir::WalkDir::new(&source.path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            (e.path() == source.path || !excluded(&e.file_name().to_string_lossy()))
                && !e.path().starts_with(&b.path)
                && !local_library
                    .as_ref()
                    .is_some_and(|p| e.path().starts_with(p))
                && !(e.file_type().is_dir() && e.path().join(MARKER).is_file())
                && (e.path() == source.path
                    || !e.file_type().is_dir()
                    || crate::repo_registry::find_git_root(e.path()).as_deref() != Some(e.path()))
        })
    {
        if cancelled() {
            return Err(fail("整理已取消，已保存的原始资料保留"));
        }
        if skipped.len() >= 100 {
            skipped.push(json!({"reason":"未读取项已达 100 条，后续候选资料未继续读取"}));
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                skipped.push(json!({"reason":"工作目录内有无法读取的路径，该路径未读取"}));
                continue;
            }
        };
        if entry.file_type().is_symlink() || !entry.file_type().is_file() {
            continue;
        }
        if !entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| {
                matches!(
                    s.to_ascii_lowercase().as_str(),
                    "md" | "markdown" | "mdx" | "rst" | "adoc"
                )
            })
        {
            continue;
        }
        let path = entry.path().strip_prefix(&source.worktree)?.to_path_buf();
        if documents.len() >= 2000 || size >= 64 * 1024 * 1024 {
            skipped.push(json!({"path":path,"reason":"已达到本轮每个工作目录 2000 篇 / 64 MB 的读取范围，后续候选资料未继续读取"}));
            break;
        }
        if entry
            .metadata()
            .map(|m| m.len() > 4 * 1024 * 1024)
            .unwrap_or(true)
        {
            skipped.push(json!({"path":path,"reason":"无法读取或超过 4 MB"}));
            continue;
        }
        if entry
            .metadata()
            .is_ok_and(|m| m.len() as usize > 64 * 1024 * 1024 - size)
        {
            skipped.push(
                json!({"path":path,"reason":"已达到本轮 64 MB 的读取范围，后续候选资料未继续读取"}),
            );
            break;
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
        if size + bytes.len() > 64 * 1024 * 1024 {
            skipped.push(json!({"path":path,"reason":"超出本轮每个工作目录 64 MB 的读取范围，后续候选资料未继续读取"}));
            break;
        }
        size += bytes.len();
        let text = match std::str::from_utf8(&bytes) {
            Ok(s) => s,
            Err(_) => {
                skipped.push(json!({"path":path,"reason":"不是 UTF-8 文本"}));
                continue;
            }
        };
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
        source_unlocked(data, &root, &id, source_id)?,
        b,
        &tree,
        &project_key(&id, &root)?,
        &|| false,
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
    let s = snapshot(
        source_unlocked(data, &root, &id, source_id)?,
        b,
        &tree,
        &key,
        &|| false,
    )?;
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
    save_snapshot(b, &mut tree, &key, &s, &selected, false)
}

fn save_snapshot(
    b: &Binding,
    tree: &mut Tree,
    key: &str,
    s: &Snapshot,
    selected: &BTreeSet<&PathBuf>,
    automatic: bool,
) -> Result<Value> {
    let mut m = manifest(tree, key)?;
    let metadata_path = manifest_path(key);
    // Compact metadata makes the per-record byte budget exact without repeatedly serializing history.
    let mut size = tree.values().map(Vec::len).sum::<usize>()
        - tree.get(&metadata_path).map_or(0, Vec::len)
        + serde_json::to_vec(&m)?.len();
    let mut files = tree.len() + usize::from(!tree.contains_key(&metadata_path));
    let byte_limit = if automatic { 124 } else { 128 } * 1024 * 1024;
    let file_limit = if automatic { 9999 } else { 10000 };
    let mut added = 0;
    let mut reused = 0;
    let mut writes = Tree::new();
    let mut skipped = Vec::new();
    let mut skipped_count = 0;
    for (d, bytes) in s
        .documents
        .iter()
        .filter(|(d, _)| selected.contains(&d.path))
    {
        let path = document_path(key, &d.hash);
        if let Some(previous) = tree.get(&path) {
            if previous != bytes {
                return Err(fail("知识库中的原文与内容摘要不符，未覆盖"));
            }
        }
        let mut origin = s.origin.clone();
        origin.document = d.path.clone();
        let last = m.records.iter().rev().find(|r| {
            r.origin.worktree_id == origin.worktree_id && r.origin.document == origin.document
        });
        let changed = !last.is_some_and(|r| r.hash == d.hash && r.origin == origin);
        let record = Record {
            hash: d.hash.clone(),
            origin,
            captured_at: crate::ids::now_iso(),
        };
        let record_bytes = if changed {
            serde_json::to_vec(&record)?.len() + usize::from(!m.records.is_empty())
        } else {
            0
        };
        let new_file = !tree.contains_key(&path);
        let growth = record_bytes + if new_file { bytes.len() } else { 0 };
        if growth > 0 && (size + growth > byte_limit || files + usize::from(new_file) > file_limit)
        {
            if !automatic {
                return Err(fail("采集后知识库超过 128 MB / 10000 文件限制，未保存"));
            }
            skipped_count += 1;
            if skipped.len() < 100 {
                skipped.push(json!({"path":d.path,"reason":"知识库剩余容量不足，本轮未保存；已为整理结果预留空间"}));
            }
            continue;
        }
        size += growth;
        files += usize::from(new_file);
        if new_file {
            writes.insert(path.clone(), bytes.clone());
            tree.insert(path, bytes.clone());
            added += 1;
        } else {
            reused += 1;
        }
        if changed {
            m.records.push(record);
        }
    }
    if skipped_count > skipped.len() {
        skipped.push(json!({"reason":format!("另有 {} 篇候选资料因容量不足未保存", skipped_count-skipped.len())}));
    }
    let metadata = serde_json::to_vec(&m)?;
    tree.insert(metadata_path.clone(), metadata.clone());
    if tree.len() > 10000 || tree.values().map(Vec::len).sum::<usize>() > 128 * 1024 * 1024 {
        return Err(fail("采集后知识库超过 128 MB / 10000 文件限制，未保存"));
    }
    let document_count = current_documents(tree, key)?.len();
    // Originals become visible only after the final manifest write. A failed batch can be retried.
    write_tree(&b.path, &writes)?;
    crate::sync_common::atomic_write(&b.path.join(metadata_path), &metadata)?;
    Ok(
        json!({"saved":added+reused,"new_documents":added,"reused_documents":reused,"document_count":document_count,"revision_count":m.records.len(),"skipped":skipped}),
    )
}

/// Called only by an explicitly started organization job, under the knowledge lock.
/// Directories are discovered from this repository; recursive reads already include children.
pub(super) fn collect_unlocked(
    data: &Path,
    root: &Path,
    approved: &[Source],
    cancelled: &impl Fn() -> bool,
    progress: &impl Fn(&str),
) -> Result<Value> {
    let cat = load(data)?;
    let (id, root) = project(data, root, &cat)?;
    let b = binding(&cat, &id)?;
    let mut tree = checked_tree(b)?;
    let key = project_key(&id, &root)?;
    let sources = sources_unlocked(data, &root, &id)?;
    let mut saved = 0;
    let mut skipped = Vec::new();
    for source in approved {
        if cancelled() {
            return Err(fail("整理已取消，已保存的原始资料保留"));
        }
        if !sources
            .iter()
            .any(|s| s.id == source.id && s.path == source.path && s.worktree == source.worktree)
        {
            skipped.push(json!({"directory":source.label,"reason":"工作目录已不可用或不再属于本项目，本轮未读取"}));
            continue;
        }
        progress(&format!("正在读取工作目录资料：{}", source.label));
        let snapshot = snapshot(source.clone(), b, &tree, &key, cancelled);
        let s = match snapshot {
            Ok(s) => s,
            Err(e) if !cancelled() => {
                skipped.push(json!({"directory":source.label,"reason":e.message}));
                continue;
            }
            Err(e) => return Err(e),
        };
        skipped.extend(
            s.skipped
                .iter()
                .map(|v| json!({"directory":source.label,"path":v["path"],"reason":v["reason"]})),
        );
        if cancelled() {
            return Err(fail("整理已取消，已保存的原始资料保留"));
        }
        let selected = s.documents.iter().map(|(d, _)| &d.path).collect();
        if !s.documents.is_empty() {
            let result = save_snapshot(b, &mut tree, &key, &s, &selected, true)?;
            skipped.extend(
                result["skipped"].as_array().into_iter().flatten().map(
                    |v| json!({"directory":source.label,"path":v["path"],"reason":v["reason"]}),
                ),
            );
            saved += s.documents.len();
        }
    }
    Ok(json!({"worktree_count":approved.len(),"documents_read":saved,"skipped":skipped}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_storage_budget_keeps_useful_sources_and_reserves_a_report_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let binding = Binding {
            id: uuid::Uuid::new_v4().to_string(),
            project_root: tmp.path().join("project"),
            path: tmp.path().join("knowledge"),
            remote: None,
            branch: default_branch(),
            subdir: String::new(),
            baseline: BTreeMap::new(),
        };
        // Synthetic existing entries exercise the file budget without materializing 9997 files.
        let mut tree: Tree = (0..9997)
            .map(|i| (PathBuf::from(format!("resources/{i}")), Vec::new()))
            .collect();
        let origin = Origin {
            worktree_id: "main".into(),
            directory: "main".into(),
            document: PathBuf::new(),
            branch: Some("main".into()),
            commit: None,
            worktree_dirty: true,
        };
        let documents = ["first", "second"]
            .map(|name| {
                let bytes = format!("# {name}").into_bytes();
                (
                    Document {
                        path: format!("{name}.md").into(),
                        title: name.into(),
                        bytes: bytes.len(),
                        hash: crate::ids::sha256_hex(&bytes),
                        state: "new",
                    },
                    bytes,
                )
            })
            .into();
        let snapshot = Snapshot {
            source: Source {
                id: "main".into(),
                label: "main".into(),
                path: binding.project_root.clone(),
                worktree: binding.project_root.clone(),
            },
            origin,
            documents,
            skipped: vec![],
            expected: String::new(),
        };
        let selected = snapshot.documents.iter().map(|(d, _)| &d.path).collect();
        let result =
            save_snapshot(&binding, &mut tree, "project", &snapshot, &selected, true).unwrap();
        assert_eq!(result["saved"], 1);
        assert_eq!(result["skipped"][0]["path"], "second.md");
        assert_eq!(
            tree.len(),
            9999,
            "the manifest is counted and a report slot remains"
        );
        assert_eq!(current_documents(&tree, "project").unwrap().len(), 1);
        assert!(binding.path.join(manifest_path("project")).is_file());
    }
}
