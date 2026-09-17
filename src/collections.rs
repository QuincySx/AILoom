//! 仓外资源合集订阅：Git 快照锁定，添加源与选用资源分离。
//! 普通 Skill 仓库可直接枚举；混合 Skill/MCP 等资源复用 ailoom.toml。
use crate::error::{code, Error, Result};
use crate::resource::{ResourceEntry, ResourceId, ResourceKind, ResourceMeta};
use crate::source::{GitSource, Snapshot, SourceLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub url: String,
    pub lock: SourceLock,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    pub revision: u64,
    pub sources: BTreeMap<String, Collection>,
}

#[derive(Serialize, Deserialize)]
struct Preview {
    base_revision: u64,
    source: Collection,
}

pub fn registry_path(data: &Path) -> PathBuf {
    data.join("collections/registry.json")
}

pub fn load(data: &Path) -> Result<Registry> {
    let p = registry_path(data);
    if !p.exists() {
        return Ok(Registry::default());
    }
    Ok(serde_json::from_slice(&std::fs::read(p)?)?)
}

fn cache(data: &Path, identity: &str) -> PathBuf {
    data.join("collections/cache")
        .join(crate::ids::cache_key_from_identity(identity))
}

fn snapshot(data: &Path, source: &Collection) -> Result<Snapshot> {
    let git = GitSource::new(&source.url, source.lock.ref_.as_deref())?;
    let cache = cache(data, &git.identity);
    let commit = source
        .lock
        .resolved_commit
        .as_deref()
        .ok_or_else(|| Error::new(code::SOURCE_CACHE_CORRUPT, "合集缺少锁定 commit"))?;
    // 读取列表/计划不自动联网恢复，不因上游变化偷偷前移版本。
    if !cache.join("snapshots").join(commit).is_dir() {
        return Err(Error::new(
            code::SOURCE_NOT_CACHED,
            "合集快照缺失，请明确检查更新后重新添加",
        ));
    }
    git.resolve(&cache, Some(&source.lock))
}

pub struct Catalog {
    pub snapshot: Snapshot,
    pub entries: Vec<ResourceEntry>,
    pub skills_root: String,
}

fn enumerate(snapshot: Snapshot, id: &str) -> Result<Catalog> {
    // 不跟随上游符号链接；即使只读预览也不读取仓库外文件。
    let mut files = Vec::new();
    for e in walkdir::WalkDir::new(&snapshot.root) {
        let e = e.map_err(|e| Error::new(code::SOURCE_CACHE_CORRUPT, e.to_string()))?;
        if e.file_type().is_symlink() {
            return Err(Error::new(
                code::PATH_TRAVERSAL,
                "合集包含符号链接，须在源中改为普通文件后添加",
            ));
        }
        if e.file_type().is_file() && e.file_name() == "SKILL.md" {
            files.push(e.path().to_path_buf());
        }
    }
    let (entries, skills_root) = if snapshot.root.join("ailoom.toml").is_file() {
        let m = crate::manifest::TeamManifest::load_from(&snapshot.root)?;
        (
            crate::resource::enumerate(&snapshot.root, &m, id)?,
            m.effective_paths().skills,
        )
    } else {
        let mut entries = Vec::new();
        for file in files {
            let raw = std::fs::read_to_string(&file)?;
            let (meta, _) = crate::resource::parse_frontmatter(&raw)?;
            let dir = file.parent().unwrap();
            let name = meta
                .as_ref()
                .and_then(|m| m.name.clone())
                .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default();
            let namespace = meta
                .as_ref()
                .and_then(|m| m.namespace.clone())
                .unwrap_or_else(|| "default".into());
            if !crate::manifest::valid_name(&name) || !crate::manifest::valid_name(&namespace) {
                return Err(Error::new(
                    code::MANIFEST_MISSING_FIELD,
                    "合集 Skill 需要合法的 name/namespace",
                ));
            }
            let rel = dir
                .strip_prefix(&snapshot.root)
                .unwrap()
                .to_string_lossy()
                .to_string();
            // 根目录 Skill 用绝对快照目录，以保证 store 相对键非空；来源仍是锁定快照。
            let path = if rel.is_empty() {
                snapshot.root.to_string_lossy().to_string()
            } else {
                rel
            };
            entries.push(ResourceEntry {
                id: ResourceId {
                    source: id.into(),
                    kind: ResourceKind::Skill,
                    namespace: namespace.clone(),
                    name,
                },
                meta: ResourceMeta {
                    shared: false,
                    projects: vec![],
                    roles: vec![],
                    namespace,
                    tags: vec![],
                },
                path,
                description: meta.and_then(|m| m.description).unwrap_or_default(),
                raw: Some(raw),
            });
        }
        (entries, ".".into())
    };
    let mut ids = BTreeSet::new();
    for e in &entries {
        if !ids.insert(e.id.to_string()) {
            return Err(Error::new(
                code::RESOURCE_ID_CONFLICT,
                format!("合集重复资源身份: {}", e.id),
            ));
        }
        if e.id.kind == ResourceKind::Mcp {
            let spec = crate::adapters::mcp::parse_spec(e)?;
            // 合集可以分发 MCP 配置，但不能把凭据作为配置携带。
            if spec
                .env
                .iter()
                .chain(spec.headers.iter())
                .any(|(_, v)| !v.starts_with("$ENV:"))
            {
                return Err(Error::new(
                    code::SOURCE_CONFLICT,
                    "合集 MCP 的 env/headers 仅允许 $ENV:名称 引用",
                ));
            }
        }
    }
    if entries.is_empty() {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            "仓库未发现 Skill；混合资源合集请提供 ailoom.toml",
        ));
    }
    Ok(Catalog {
        snapshot,
        entries,
        skills_root,
    })
}

pub fn catalog(data: &Path, source: &Collection) -> Result<Catalog> {
    enumerate(snapshot(data, source)?, &source.id)
}

fn entry_value(entry: &ResourceEntry, source: &Collection) -> Value {
    json!({ "id": entry.id.to_string(), "kind": entry.id.kind.as_str(), "name": entry.id.name,
        "description": entry.description, "source_id": source.id, "source_name": source.name,
        "source_url": source.url, "revision": source.lock.resolved_commit, "path": entry.path, "readonly": true })
}

pub fn list(data: &Path) -> Result<Value> {
    let registry = load(data)?;
    let mut sources = Vec::new();
    for source in registry.sources.values() {
        let mut v = serde_json::to_value(source)?;
        v["references"] = references(data, &format!("{}/", source.id))?;
        v["store_path"] = json!(crate::store::source_bucket(
            &crate::store::resolve_store_root()?,
            &format!(
                "{}#{}",
                source.lock.identity,
                source.lock.resolved_commit.as_deref().unwrap_or("working")
            )
        ));
        let check_path = data
            .join("collections/checks")
            .join(format!("{}.json", source.id));
        if check_path.is_file() {
            let mut check: Value = serde_json::from_slice(&std::fs::read(check_path)?)?;
            if check["base_commit"] == json!(source.lock.resolved_commit) {
                if check["state"] == "available"
                    && check["preview"]["registry_revision"] != json!(registry.revision)
                {
                    check["state"] = json!("stale");
                }
                v["update"] = check;
            } else if check["preview"]["source"]["lock"]["resolved_commit"]
                == json!(source.lock.resolved_commit)
            {
                check["state"] = json!("current");
                v["update"] = check;
            }
        }
        match catalog(data, source) {
            Ok(c) => {
                v["resources"] = json!(c
                    .entries
                    .iter()
                    .map(|e| entry_value(e, source))
                    .collect::<Vec<_>>())
            }
            Err(e) => {
                v["resources"] = json!([]);
                v["error"] = json!(e.to_string());
            }
        }
        sources.push(v);
    }
    Ok(json!({ "revision": registry.revision, "sources": sources }))
}

/// 显式联网预览（新源或已有源更新）。只写隔离缓存，不注册、不启用、不部署。
pub fn preview(
    data: &Path,
    name: &str,
    url: &str,
    ref_: Option<&str>,
    update_id: Option<&str>,
) -> Result<Value> {
    let registry = load(data)?;
    if !(url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("ssh://")
        || url.starts_with("file://")
        || Path::new(url).is_absolute()
        || url.starts_with("git@"))
    {
        return Err(Error::new(
            code::USAGE,
            "合集须使用 HTTPS、SSH 或本地 Git 路径，不支持外部 Git helper 协议",
        ));
    }
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(Error::new(code::USAGE, "合集名称须为 1–128 字节"));
    }
    let git = GitSource::new(url, ref_)?;
    let id = format!(
        "collection-{}",
        crate::ids::sha256_prefix(git.identity.as_bytes(), 16)
    );
    if let Some(update_id) = update_id {
        if update_id != id || !registry.sources.contains_key(&id) {
            return Err(Error::new(
                code::SOURCE_CONFLICT,
                "更新目标与原合集来源不一致",
            ));
        }
    } else if registry.sources.contains_key(&id) {
        return Err(Error::new(
            code::SOURCE_CONFLICT,
            "该合集已添加，请在已有合集上检查更新",
        ));
    }
    let snap = git.refresh(&cache(data, &git.identity))?;
    let source = Collection {
        id: id.clone(),
        name: name.into(),
        url: git.url.clone(),
        lock: SourceLock {
            kind: "git".into(),
            identity: git.identity.clone(),
            ref_: git.ref_.clone(),
            resolved_commit: snap.resolved_commit.clone(),
            content_digest: snap.content_digest.clone(),
            locked_at: snap.locked_at.clone(),
        },
    };
    let cat = enumerate(snap, &id)?;
    let resources: Vec<Value> = cat
        .entries
        .iter()
        .map(|e| entry_value(e, &source))
        .collect();
    let previous = registry.sources.get(&id);
    let old_ids: BTreeSet<String> = match previous {
        Some(s) => catalog(data, s)?
            .entries
            .iter()
            .map(|e| e.id.to_string())
            .collect(),
        None => BTreeSet::new(),
    };
    let new_ids: BTreeSet<String> = cat.entries.iter().map(|e| e.id.to_string()).collect();
    let removed: Vec<_> = old_ids.difference(&new_ids).cloned().collect();
    let token = crate::ids::new_id();
    let preview = Preview {
        base_revision: registry.revision,
        source: source.clone(),
    };
    let dir = data.join("collections/previews");
    std::fs::create_dir_all(&dir)?;
    crate::sync_common::atomic_write(
        &dir.join(format!("{token}.json")),
        &serde_json::to_vec_pretty(&preview)?,
    )?;
    Ok(
        json!({ "preview_id": token, "registry_revision": registry.revision, "source": source, "previous_commit": previous.and_then(|s| s.lock.resolved_commit.as_ref()),
        "resources": resources, "removed": removed, "note": "添加或更新合集仅锁定资源目录；项目只部署显式选用项，MCP 不在添加时启动" }),
    )
}

pub fn apply_preview(data: &Path, token: &str) -> Result<Value> {
    apply_previews(data, &[token.to_string()])
}

pub fn apply_previews(data: &Path, tokens: &[String]) -> Result<Value> {
    if tokens.is_empty() || tokens.len() > 100 {
        return Err(Error::new(code::USAGE, "请选择 1–100 个已预览的合集"));
    }
    let mut previews = Vec::new();
    let mut seen = BTreeSet::new();
    for token in tokens {
        if uuid::Uuid::parse_str(token).is_err() {
            return Err(Error::new(code::USAGE, "无效预览 ID"));
        }
        let preview: Preview = serde_json::from_slice(&std::fs::read(
            data.join("collections/previews")
                .join(format!("{token}.json")),
        )?)?;
        if !seen.insert(preview.source.id.clone()) {
            return Err(Error::new(code::USAGE, "不能重复更新同一来源"));
        }
        previews.push(preview);
    }
    crate::source::with_file_lock(&data.join("collections/registry.lock"), || {
        let mut registry = load(data)?;
        for preview in &previews {
            if registry.revision != preview.base_revision {
                return Err(Error::new(
                    code::USER_CONTENT_CONFLICT,
                    "合集列表已变更，请重新预览",
                ));
            }
            let next = catalog(data, &preview.source)?;
            if let Some(old) = registry.sources.get(&preview.source.id) {
                let next_ids: BTreeSet<_> = next.entries.iter().map(|e| e.id.to_string()).collect();
                for entry in catalog(data, old)?.entries {
                    let id = entry.id.to_string();
                    if !next_ids.contains(&id)
                        && !references(data, &id)?.as_array().unwrap().is_empty()
                    {
                        return Err(Error::new(
                            code::USER_CONTENT_CONFLICT,
                            format!("上游移除了仍被项目引用的资源 {id}；先取消引用或替换后再更新"),
                        ));
                    }
                }
            }
        }
        for preview in &previews {
            registry
                .sources
                .insert(preview.source.id.clone(), preview.source.clone());
        }
        registry.revision += 1;
        crate::sync_common::atomic_write(
            &registry_path(data),
            &serde_json::to_vec_pretty(&registry)?,
        )?;
        Ok(
            json!({ "source": previews[0].source, "updated": previews.len(), "revision": registry.revision, "note": "已保存合集；尚未修改任何项目或启动 MCP" }),
        )
    })
}

/// 明确启用的引用；同时包含仓库默认、子项目以及工作树覆盖，供更新/移除预览使用。
pub fn references(data: &Path, id_or_prefix: &str) -> Result<Value> {
    let profile = crate::profile::PersonalProfile::load_or_default(data)?;
    let mut refs = Vec::new();
    for (repo_id, repo) in &profile.repos {
        let mut scan = |selection: &crate::profile::ScopeSelection, scope: String| {
            for (id, state) in &selection.resources {
                let matches = if id_or_prefix.ends_with('/') {
                    id.starts_with(id_or_prefix)
                } else {
                    id == id_or_prefix
                };
                if matches && *state == crate::profile::TriState::Enable {
                    refs.push(json!({"repo_id": repo_id, "scope": scope, "resource_id": id}));
                }
            }
        };
        if let Some(s) = &repo.default {
            scan(s, "仓库默认".into());
        }
        for s in &repo.subprojects {
            scan(&s.selection, format!("子项目 {}", s.path));
        }
        for (wt, s) in &repo.worktrees {
            scan(s, format!("工作树 {wt}"));
        }
        for (wt, subs) in &repo.wt_subprojects {
            for s in subs {
                scan(&s.selection, format!("工作树 {wt}/{}", s.path));
            }
        }
    }
    Ok(json!(refs))
}

/// 检查会获取远端，但只持久化检查结果与候选快照，不改变库版本或项目。
pub fn check_updates(data: &Path, source_id: Option<&str>) -> Result<Value> {
    let registry = load(data)?;
    if source_id.is_some_and(|id| !registry.sources.contains_key(id)) {
        return Err(Error::new(code::UNKNOWN_REFERENCE, "来源不存在"));
    }
    let mut items = Vec::new();
    for s in registry
        .sources
        .values()
        .filter(|s| source_id.map_or(true, |id| s.id == id))
    {
        let mut status = json!({"source_id":s.id, "name":s.name, "base_commit":s.lock.resolved_commit, "checked_at":crate::ids::now_iso()});
        match preview(data, &s.name, &s.url, s.lock.ref_.as_deref(), Some(&s.id)) {
            Ok(p) => {
                status["state"] = json!(if p["source"]["lock"]["resolved_commit"]
                    == json!(s.lock.resolved_commit)
                {
                    "current"
                } else {
                    "available"
                });
                status["preview"] = p;
            }
            Err(e) => {
                status["state"] = json!("error");
                status["error"] = json!(e.to_string());
            }
        }
        let dir = data.join("collections/checks");
        std::fs::create_dir_all(&dir)?;
        crate::sync_common::atomic_write(
            &dir.join(format!("{}.json", s.id)),
            &serde_json::to_vec_pretty(&status)?,
        )?;
        items.push(status);
    }
    Ok(json!({"items":items}))
}

/// 从目录移除来源而不删除快照、实体或项目文件。保留归档以便恢复和人工核查。
pub fn remove(data: &Path, id: &str, execute: bool) -> Result<Value> {
    crate::source::with_file_lock(&data.join("collections/registry.lock"), || {
        let mut registry = load(data)?;
        let source = registry
            .sources
            .get(id)
            .ok_or_else(|| Error::new(code::UNKNOWN_REFERENCE, "来源不存在"))?
            .clone();
        let refs = references(data, &format!("{id}/"))?;
        if execute {
            if !refs.as_array().unwrap().is_empty() {
                return Err(Error::new(
                    code::USER_CONTENT_CONFLICT,
                    "来源仍被项目启用，请先取消引用并应用",
                ));
            }
            let archive = data.join("collections/archive");
            std::fs::create_dir_all(&archive)?;
            crate::sync_common::atomic_write(
                &archive.join(format!("{}-{}.json", id, crate::ids::new_id())),
                &serde_json::to_vec_pretty(&source)?,
            )?;
            registry.sources.remove(id);
            registry.revision += 1;
            crate::sync_common::atomic_write(
                &registry_path(data),
                &serde_json::to_vec_pretty(&registry)?,
            )?;
        }
        Ok(
            json!({"source":source, "references":refs, "executed":execute, "note":"只移除资源库来源登记；缓存、历史版本和现有项目文件保留，不删除远端仓库"}),
        )
    })
}
