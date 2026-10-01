//! Project-scoped recovery state. Only explicitly selected resources are packaged.
use super::{
    location::{digest, scan, write_tree, Binding},
    portable::Declaration,
};
use crate::{
    error::{code, Error, Result},
    profile::{PersonalProfile, RepoProfile, ScopeSelection},
    resource::ResourceEntry,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

const CATALOG: &str = "ailoom-resources.json";
const ORIGIN: &str = "ailoom-origin.json";
#[derive(Serialize, Deserialize)]
struct Origin {
    name: String,
    url: String,
    lock: crate::source::SourceLock,
}
fn origin(data: &Path, source: &str, root: &Path) -> Result<Option<Origin>> {
    if source == "personal" {
        return Ok(None);
    }
    let registry = crate::collections::load(data)?;
    if source.starts_with("collection-portable-")
        && registry
            .sources
            .get(source)
            .is_some_and(|s| s.external_path.as_deref() == Some(root))
        && root.join(ORIGIN).is_file()
    {
        return Ok(Some(serde_json::from_slice(&std::fs::read(
            root.join(ORIGIN),
        )?)?));
    }
    Ok(registry
        .sources
        .get(source)
        .filter(|s| super::portable::portable_url(&s.url))
        .map(|s| Origin {
            name: s.name.clone(),
            url: s.url.clone(),
            lock: s.lock.clone(),
        }))
}
fn fail(s: impl Into<String>) -> Error {
    Error::new(code::KNOWLEDGE_STATE_CONFLICT, s)
}
#[derive(Serialize, Deserialize)]
struct State {
    schema_version: u32,
    project_id: String,
    knowledge_id: String,
    profile: RepoProfile,
    sources: BTreeMap<String, String>,
    instructions: BTreeMap<String, String>,
}
fn scopes(p: &mut RepoProfile) -> Vec<&mut ScopeSelection> {
    p.default
        .iter_mut()
        .chain(p.subprojects.iter_mut().map(|s| &mut s.selection))
        .chain(p.worktrees.values_mut())
        .chain(
            p.wt_subprojects
                .values_mut()
                .flatten()
                .map(|s| &mut s.selection),
        )
        .collect()
}
fn state_path(b: &Binding, d: &Declaration) -> PathBuf {
    b.path
        .join("projects")
        .join(&d.project_id)
        .join("state.json")
}
fn base_path(data: &Path, d: &Declaration, repo: &str) -> PathBuf {
    data.join("knowledge/checkpoints")
        .join(&d.project_id)
        .join(format!("{repo}.json"))
}
fn token(bytes: &[u8]) -> String {
    crate::ids::sha256_hex(bytes)
}
fn worktrees(data: &Path, repo: &str, root: &Path) -> Result<BTreeMap<String, String>> {
    let discovery = if crate::repo_registry::find_git_root(root).is_some() {
        crate::repo_registry::discover_repo(root)?
    } else {
        crate::commands::personal::nongit_identity(root)
    };
    let mut reg = crate::repo_registry::RepoRegistry::resolve_or_create(data, &discovery)?;
    if !repo.starts_with("nongit-") {
        reg.refresh_worktrees(&discovery, &crate::ids::now_iso());
    }
    let mut out = BTreeMap::new();
    if repo.starts_with("nongit-") {
        out.insert("root".into(), "main".into());
        return Ok(out);
    }
    for (id, wt) in reg.worktrees {
        let key = if wt.path == root {
            "main".into()
        } else if let Some(branch) = wt.branch {
            format!("branch:{branch}")
        } else {
            format!("detached:{}", wt.head.unwrap_or_default())
        };
        if out.values().any(|v| v == &key) {
            return Err(fail("多个 Worktree 的分支相同，无法唯一恢复配置"));
        }
        out.insert(id, key);
    }
    Ok(out)
}
fn remap_worktrees(profile: &mut RepoProfile, map: &BTreeMap<String, String>) -> Result<()> {
    let key = |k: String| {
        map.get(&k)
            .cloned()
            .ok_or_else(|| fail(format!("缺少 Worktree 映射：{k}")))
    };
    profile.worktrees = std::mem::take(&mut profile.worktrees)
        .into_iter()
        .map(|(k, v)| Ok((key(k)?, v)))
        .collect::<Result<_>>()?;
    profile.wt_subprojects = std::mem::take(&mut profile.wt_subprojects)
        .into_iter()
        .map(|(k, v)| Ok((key(k)?, v)))
        .collect::<Result<_>>()?;
    Ok(())
}
fn selected_entries(data: &Path, source: &str) -> Result<(PathBuf, Vec<ResourceEntry>)> {
    if source == "personal" {
        let root = crate::personal_library::library_root(data);
        let manifest = crate::manifest::TeamManifest::load_from(&root)?;
        return Ok((
            root.clone(),
            crate::resource::enumerate(&root, &manifest, "personal", &mut vec![])?,
        ));
    }
    let sources = crate::collections::load(data)?;
    let source = sources
        .sources
        .get(source)
        .ok_or_else(|| fail(format!("无法打包未登记的资源来源：{source}")))?;
    let catalog = crate::collections::catalog(data, source)?;
    Ok((catalog.snapshot.root, catalog.entries))
}
pub fn checkpoint(data: &Path, repo: &str, b: &Binding, d: &Declaration) -> Result<Value> {
    let mut profile = PersonalProfile::load_or_default(data)?
        .repos
        .remove(repo)
        .unwrap_or_default();
    let wt_map = worktrees(data, repo, &b.project_root)?;
    remap_worktrees(&mut profile, &wt_map)?;
    let mut wanted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for scope in scopes(&mut profile) {
        for key in scope.resources.keys() {
            let source = key.split('/').next().unwrap_or("");
            wanted.entry(source.into()).or_default().insert(key.clone());
        }
    }
    let mut replacements = BTreeMap::new();
    let mut packages = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for (source, wanted) in wanted {
        let portable_id = if source.starts_with("collection-portable-") {
            source.clone()
        } else {
            format!(
                "collection-portable-{}",
                crate::ids::sha256_prefix(format!("{}:{source}", d.project_id).as_bytes(), 24)
            )
        };
        let (root, entries) = selected_entries(data, &source)?;
        let mut tree = BTreeMap::new();
        if let Some(origin) = origin(data, &source, &root)? {
            tree.insert(ORIGIN.into(), serde_json::to_vec_pretty(&origin)?);
        }
        let mut packaged = Vec::new();
        for mut entry in entries
            .into_iter()
            .filter(|e| wanted.contains(&e.id.to_string()))
        {
            let original = entry.id.to_string();
            let input = root.join(&entry.path);
            let canonical = input.canonicalize()?;
            if !canonical.starts_with(root.canonicalize()?)
                || input.symlink_metadata()?.file_type().is_symlink()
            {
                return Err(fail("资源包含外部链接，不能作为可迁移副本保存"));
            }
            let mut rel = PathBuf::from(format!(
                "resources/{}/{}/{}",
                entry.id.kind.as_str(),
                entry.id.namespace,
                entry.id.name
            ));
            if entry.id.kind != crate::resource::ResourceKind::Skill {
                if let Some(ext) = input.extension() {
                    rel.set_extension(ext);
                }
            }
            if entry.id.kind == crate::resource::ResourceKind::Skill {
                for (p, bytes) in scan(&input)? {
                    tree.insert(rel.join(p), bytes);
                }
                entry.raw = Some(std::fs::read_to_string(input.join("SKILL.md"))?);
            } else {
                let bytes = std::fs::read(&input)?;
                entry.raw =
                    Some(String::from_utf8(bytes.clone()).map_err(|_| fail("资源必须是 UTF-8"))?);
                tree.insert(rel.clone(), bytes);
            }
            entry.path = rel.to_string_lossy().into();
            entry.id.source = portable_id.clone();
            replacements.insert(original, entry.id.to_string());
            entry.raw = None; // Rehydrated from its file when reading the catalog.
            packaged.push(entry);
        }
        if wanted.iter().any(|key| !replacements.contains_key(key)) {
            return Err(fail("部分已选资源缺失，请恢复来源后再保存恢复状态"));
        }
        tree.insert(CATALOG.into(), serde_json::to_vec_pretty(&packaged)?);
        let hash = digest(&tree);
        let rel = format!("projects/{}/resources/{hash}", d.project_id);
        packages.insert(rel.clone(), tree);
        sources.insert(portable_id, rel);
    }
    for scope in scopes(&mut profile) {
        scope.resources = std::mem::take(&mut scope.resources)
            .into_iter()
            .map(|(k, v)| (replacements.get(&k).cloned().unwrap_or(k), v))
            .collect();
    }
    let mut instructions = BTreeMap::new();
    for (key, wt) in std::iter::once(("project".to_string(), None)).chain(
        wt_map
            .iter()
            .map(|(id, key)| (key.clone(), Some(id.as_str()))),
    ) {
        let p = crate::personal_instructions::entry_path(data, repo, wt);
        if p.is_file() {
            instructions.insert(key, std::fs::read_to_string(p)?);
        }
    }
    let path = state_path(b, d);
    let old = std::fs::read(&path).ok();
    // Keep scopes of worktrees not yet present on this machine.
    if let Some(old) = &old {
        let previous: State = serde_json::from_slice(old)?;
        for (key, value) in previous.profile.worktrees {
            if !wt_map.values().any(|v| v == &key) {
                profile.worktrees.entry(key).or_insert(value);
            }
        }
        for (key, value) in previous.profile.wt_subprojects {
            if !wt_map.values().any(|v| v == &key) {
                profile.wt_subprojects.entry(key).or_insert(value);
            }
        }
        for (key, value) in previous.instructions {
            if key != "project" && !wt_map.values().any(|v| v == &key) {
                instructions.entry(key).or_insert(value);
            }
        }
        for (key, value) in previous.sources {
            sources.entry(key).or_insert(value);
        }
    }
    let required: BTreeSet<String> = scopes(&mut profile)
        .into_iter()
        .flat_map(|s| {
            s.resources
                .keys()
                .map(|k| k.split('/').next().unwrap_or("").to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    sources.retain(|key, _| required.contains(key));
    let state = State {
        schema_version: 1,
        project_id: d.project_id.clone(),
        knowledge_id: d.knowledge_id.clone(),
        profile,
        sources,
        instructions,
    };
    let bytes = serde_json::to_vec_pretty(&state)?;
    if let Some(old) = &old {
        let base = std::fs::read_to_string(base_path(data, d, repo)).ok();
        if old != &bytes && base.as_deref() != Some(&token(old)) {
            return Err(fail(
                "知识库中的项目配置已更新；请先恢复并核对，避免覆盖其他设备的修改",
            ));
        }
    }
    for (rel, tree) in packages {
        let path = b.path.join(rel);
        if path.exists() && scan(&path)? != tree {
            return Err(fail("已有资源快照被修改，请先核对"));
        }
        write_tree(&path, &tree)?;
    }
    crate::sync_common::atomic_write(&path, &bytes)?;
    crate::sync_common::atomic_write(&base_path(data, d, repo), token(&bytes).as_bytes())?;
    Ok(
        json!({"saved":true,"resources":replacements.len(),"project_id":d.project_id,"knowledge_id":d.knowledge_id}),
    )
}
/// Catalogs read through the existing collection/adapters path; validate before exposing entries.
pub fn catalog(path: &Path, source: &str) -> Result<Option<crate::collections::Catalog>> {
    if !path.join(CATALOG).exists() {
        return Ok(None);
    }
    let tree = scan(path)?;
    let mut entries: Vec<ResourceEntry> = serde_json::from_slice(
        tree.get(Path::new(CATALOG))
            .ok_or_else(|| fail("资源目录缺少清单"))?,
    )?;
    let mut ids = BTreeSet::new();
    for e in &mut entries {
        if e.id.source != source
            || !crate::manifest::valid_name(&e.id.name)
            || !crate::manifest::valid_name(&e.id.namespace)
            || !ids.insert(e.id.to_string())
        {
            return Err(fail("恢复资源身份无效或重复"));
        }
        crate::manifest::validate_relative_path("resource.path", &e.path)?;
        if !Path::new(&e.path).starts_with("resources") {
            return Err(fail("恢复资源路径无效"));
        }
        let raw_path = if e.id.kind == crate::resource::ResourceKind::Skill {
            Path::new(&e.path).join("SKILL.md")
        } else {
            PathBuf::from(&e.path)
        };
        e.raw = Some(
            String::from_utf8(
                tree.get(&raw_path)
                    .ok_or_else(|| fail("恢复资源文件缺失"))?
                    .clone(),
            )
            .map_err(|_| fail("恢复资源不是 UTF-8"))?,
        );
    }
    Ok(Some(crate::collections::Catalog {
        snapshot: crate::source::Snapshot {
            identity: source.into(),
            resolved_commit: None,
            content_digest: digest(&tree),
            root: path.into(),
            ref_: None,
            locked_at: crate::ids::now_iso(),
            mutable: false,
        },
        entries,
        skills_root: "resources/skill".into(),
        warnings: vec![],
    }))
}

pub fn local_fingerprint(data: &Path, repo: &str) -> Result<Value> {
    let profile = PersonalProfile::load_or_default(data)?;
    let instructions = crate::personal_instructions::entry_base(data, repo);
    Ok(
        json!({"profile_revision":profile.revision,"collections_revision":crate::collections::load(data)?.revision,"instructions":if instructions.exists(){digest(&scan(&instructions)?)}else{String::new()}}),
    )
}
/// Validate conflicts before any writes; replay is idempotent after an interrupted restore.
pub fn restore(
    data: &Path,
    repo: &str,
    b: &Binding,
    d: &Declaration,
    execute: bool,
) -> Result<Value> {
    let path = state_path(b, d);
    if !path.exists() {
        return Ok(json!({"resources":0,"pending_worktrees":[]}));
    }
    let bytes = std::fs::read(&path)?;
    let mut state: State = serde_json::from_slice(&bytes)?;
    if state.schema_version != 1
        || state.project_id != d.project_id
        || state.knowledge_id != d.knowledge_id
    {
        return Err(fail("项目恢复状态的身份不匹配"));
    }
    let map = worktrees(data, repo, &b.project_root)?;
    let reverse: BTreeMap<_, _> = map.into_iter().map(|(k, v)| (v, k)).collect();
    let pending: BTreeSet<_> = state
        .profile
        .worktrees
        .keys()
        .chain(state.profile.wt_subprojects.keys())
        .filter(|k| !reverse.contains_key(*k))
        .cloned()
        .collect();
    state
        .profile
        .worktrees
        .retain(|k, _| reverse.contains_key(k));
    state
        .profile
        .wt_subprojects
        .retain(|k, _| reverse.contains_key(k));
    remap_worktrees(&mut state.profile, &reverse)?;
    let mut profile = PersonalProfile::load_or_default(data)?;
    if let Some(existing) = profile.repos.get(repo) {
        let same_or_new_worktree = existing.default == state.profile.default
            && existing.subprojects == state.profile.subprojects
            && existing
                .worktrees
                .iter()
                .all(|(key, value)| state.profile.worktrees.get(key) == Some(value))
            && existing
                .wt_subprojects
                .iter()
                .all(|(key, value)| state.profile.wt_subprojects.get(key) == Some(value));
        if serde_json::to_value(existing)? != json!({}) && !same_or_new_worktree {
            return Err(fail("本机已有不同的项目选用配置；恢复不会覆盖，请先核对"));
        }
    }
    profile.repos.insert(repo.into(), state.profile);
    profile.validate()?;
    let mut additions = Vec::new();
    let mut available = BTreeSet::new();
    for (id, rel) in state.sources {
        crate::manifest::validate_relative_path("source.path", &rel)?;
        if !Path::new(&rel).starts_with(format!("projects/{}/resources", d.project_id))
            || !id.starts_with("collection-portable-")
        {
            return Err(fail("恢复来源路径无效"));
        }
        let source_path = b.path.join(rel);
        let cat = catalog(&source_path, &id)?.ok_or_else(|| fail("恢复来源缺失"))?;
        available.extend(cat.entries.iter().map(|e| e.id.to_string()));
        let origin = if source_path.join(ORIGIN).exists() {
            let origin: Origin = serde_json::from_slice(&std::fs::read(source_path.join(ORIGIN))?)?;
            if !super::portable::portable_url(&origin.url) {
                return Err(fail("恢复资源的远端地址无效"));
            }
            crate::source::GitSource::new(&origin.url, origin.lock.ref_.as_deref())?;
            Some(origin)
        } else {
            None
        };
        additions.push(crate::collections::Collection {
            id: id.clone(),
            name: origin
                .as_ref()
                .map(|o| o.name.clone())
                .unwrap_or_else(|| "项目知识库".into()),
            url: origin
                .as_ref()
                .map(|o| o.url.clone())
                .unwrap_or_else(|| format!("knowledge+{}", d.knowledge_id)),
            lock: crate::source::SourceLock {
                kind: if origin.is_some() { "git" } else { "local" }.into(),
                identity: origin
                    .as_ref()
                    .map(|o| o.lock.identity.clone())
                    .unwrap_or(id),
                ref_: origin.as_ref().and_then(|o| o.lock.ref_.clone()),
                resolved_commit: origin.as_ref().and_then(|o| o.lock.resolved_commit.clone()),
                content_digest: cat.snapshot.content_digest,
                locked_at: crate::ids::now_iso(),
            },
            migration: None,
            external_path: Some(source_path),
        });
    }
    for scope in scopes(profile.repos.get_mut(repo).unwrap()) {
        if scope.resources.keys().any(|id| !available.contains(id)) {
            return Err(fail("恢复配置引用了缺失的资源"));
        }
    }
    let mut instruction_writes = Vec::new();
    for (key, text) in state.instructions {
        let wt = if key == "project" {
            None
        } else {
            match reverse.get(&key) {
                Some(v) => Some(v.as_str()),
                None => continue,
            }
        };
        let p = crate::personal_instructions::entry_path(data, repo, wt);
        if p.exists() && std::fs::read_to_string(&p)? != text {
            return Err(fail("本机项目说明与恢复内容不同，请先核对"));
        }
        instruction_writes.push((p, text));
    }
    let count = additions.len();
    crate::source::with_file_lock(&data.join("collections/registry.lock"), || {
        let mut registry = crate::collections::load(data)?;
        for addition in additions {
            if let Some(old) = registry.sources.get(&addition.id) {
                if old.url != addition.url
                    || old.lock.content_digest != addition.lock.content_digest
                {
                    return Err(fail("本机已有不同版本的同名恢复来源，请先核对"));
                }
            }
            registry.sources.insert(addition.id.clone(), addition);
        }
        if execute {
            let revision = profile.revision;
            profile.save_with_guard(data, Some(revision))?;
            registry.revision += 1;
            crate::sync_common::atomic_write(
                &crate::collections::registry_path(data),
                &serde_json::to_vec_pretty(&registry)?,
            )?;
            for (p, text) in instruction_writes {
                crate::sync_common::atomic_write(&p, text.as_bytes())?;
            }
            crate::sync_common::atomic_write(&base_path(data, d, repo), token(&bytes).as_bytes())?;
        }
        Ok(())
    })?;
    Ok(json!({"sources":count,"pending_worktrees":pending}))
}

/// Move local source bindings together with the knowledge directory, never leave links to backups.
pub fn relocate(data: &Path, old: &Binding, next: &Binding) -> Result<()> {
    crate::source::with_file_lock(&data.join("collections/registry.lock"), || {
        let mut registry = crate::collections::load(data)?;
        let mut changed = false;
        for source in registry
            .sources
            .values_mut()
            .filter(|s| s.id.starts_with("collection-portable-"))
        {
            if let Some(rel) = source
                .external_path
                .as_ref()
                .and_then(|p| p.strip_prefix(&old.path).ok())
            {
                let path = next.path.join(rel);
                let cat =
                    catalog(&path, &source.id)?.ok_or_else(|| fail("迁移后的资源快照缺失"))?;
                if cat.snapshot.content_digest != source.lock.content_digest {
                    return Err(fail("迁移后的资源内容不一致"));
                }
                source.external_path = Some(path);
                changed = true;
            }
        }
        if changed {
            registry.revision += 1;
            crate::sync_common::atomic_write(
                &crate::collections::registry_path(data),
                &serde_json::to_vec_pretty(&registry)?,
            )?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use serde_json::json;

    fn write_catalog(dir: &Path, entries: serde_json::Value) {
        std::fs::create_dir_all(dir.join("resources/skill/demo")).unwrap();
        std::fs::write(
            dir.join("resources/skill/demo/SKILL.md"),
            "---\nname: demo\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(dir.join(CATALOG), entries.to_string()).unwrap();
    }

    fn entry(source: &str, name: &str, path: &str) -> serde_json::Value {
        json!({
            "id": {"source": source, "kind": "skill", "namespace": "common", "name": name},
            "meta": {"namespace": "common", "shared": true},
            "path": path, "description": "d", "raw": null
        })
    }

    #[test]
    fn catalog_reads_valid_snapshot_and_rejects_forged_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(
            catalog(dir, "src").unwrap().is_none(),
            "没有清单 = 不是恢复快照"
        );

        write_catalog(dir, json!([entry("src", "demo", "resources/skill/demo")]));
        let cat = catalog(dir, "src").unwrap().expect("有效快照");
        assert_eq!(cat.entries.len(), 1);
        assert!(
            cat.entries[0].raw.as_deref().unwrap().contains("body"),
            "原文随条目载入"
        );

        for (bad, why) in [
            (
                json!([entry("other", "demo", "resources/skill/demo")]),
                "来源不符",
            ),
            (
                json!([
                    entry("src", "demo", "resources/skill/demo"),
                    entry("src", "demo", "resources/skill/demo")
                ]),
                "重复身份",
            ),
            (
                json!([entry("src", "demo", "elsewhere/demo")]),
                "路径不在 resources 下",
            ),
            (
                json!([entry("src", "demo", "resources/../../etc")]),
                "路径穿越",
            ),
            (
                json!([entry("src", "missing", "resources/skill/missing")]),
                "文件缺失",
            ),
        ] {
            write_catalog(dir, bad);
            assert!(catalog(dir, "src").is_err(), "{why} 必须拒绝");
        }
    }
}
