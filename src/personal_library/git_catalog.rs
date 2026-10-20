//! 按仓库浏览并安装所选 Skill；预览固定快照，安装保留原分支用于后续更新。
use super::*;
use crate::source::{git::Scope, GitSource, SourceLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
struct Candidate {
    id: String,
    name: String,
    description: String,
    repo_path: String,
    installed: bool,
    conflict: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Preview {
    url: String,
    lock: SourceLock,
    skills: Vec<Candidate>,
    #[serde(default)]
    binding_stamp: String,
}

#[cfg(test)]
fn repository_key(url: &str) -> String {
    crate::repository_identity::key(url)
}

fn repository_path(raw: &str) -> String {
    let path = raw.trim_matches('/').trim_start_matches("./");
    if path == "." {
        String::new()
    } else {
        path.to_string()
    }
}

fn installed_paths(data: &Path, identity: &str) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for entry in list_tolerant(data).0 {
        if let Some(source) = entry.source {
            if source.repo_url.as_deref().is_some_and(|url| {
                crate::repository_identity::key_for(data, url)
                    .ok()
                    .as_deref()
                    == Some(identity)
            }) {
                paths.insert(repository_path(
                    source.repo_path.as_deref().unwrap_or_default(),
                ));
            }
        }
    }
    for source in crate::collections::load(data)?.sources.values() {
        if crate::repository_identity::key_for(data, source.repository_url())? != identity {
            continue;
        }
        if let Ok(catalog) = crate::collections::catalog(data, source) {
            for entry in catalog.entries {
                if entry.id.kind == ResourceKind::Skill {
                    let path = source
                        .migration
                        .as_ref()
                        .and_then(|m| {
                            m.skills
                                .iter()
                                .find(|s| s.resource_id.as_deref() == Some(&entry.id.to_string()))
                        })
                        .and_then(|s| s.repo_path.clone())
                        .unwrap_or_else(|| {
                            if Path::new(&entry.path).is_absolute() {
                                String::new()
                            } else {
                                entry.path
                            }
                        });
                    paths.insert(repository_path(&path));
                }
            }
        }
    }
    Ok(paths)
}

pub fn preview(data: &Path, url: &str, ref_: Option<&str>) -> Result<Value> {
    if !(url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("ssh://")
        || url.starts_with("git://")
        || url.starts_with("git@")
        || url.starts_with("file://")
        || Path::new(url).is_absolute())
    {
        return Err(Error::new(
            code::USAGE,
            "请填写 Git 仓库克隆地址或已批准的本地 Git 路径",
        ));
    }
    let binding_stamp = crate::repository_identity::binding_stamp(data)?;
    let git = GitSource::new(url, ref_)?.with_scope(Scope::Skills);
    let snapshot = git.refresh(&skill_git_cache(data, &git.identity))?;
    let installed = installed_paths(data, &crate::repository_identity::key_for(data, &git.url)?)?;
    let mut skills = Vec::new();
    for file in walkdir::WalkDir::new(&snapshot.root).follow_links(false) {
        let file = file.map_err(|e| Error::new(code::SOURCE_CACHE_CORRUPT, e.to_string()))?;
        if !file.file_type().is_file() || file.file_name() != "SKILL.md" {
            continue;
        }
        let dir = file.path().parent().unwrap();
        let path = dir
            .strip_prefix(&snapshot.root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let id = if path.is_empty() {
            ".".into()
        } else {
            path.clone()
        };
        let fallback = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let (name, description, conflict) = match import_preview_inner(data, dir, None, false) {
            Ok(p) => (
                p.skill_name,
                p.description,
                (!p.conflicts.is_empty()).then(|| "资源库已有同名 Skill，请先处理名称冲突".into()),
            ),
            Err(e) => (fallback, String::new(), Some(e.message)),
        };
        skills.push(Candidate {
            id,
            name,
            description,
            installed: installed.contains(&path),
            repo_path: path,
            conflict,
        });
    }
    if skills.is_empty() {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            "仓库内没有找到 SKILL.md",
        ));
    }
    let mut counts = BTreeMap::new();
    for skill in &skills {
        *counts.entry(skill.name.clone()).or_insert(0) += 1;
    }
    for skill in &mut skills {
        if counts[&skill.name] > 1 && !skill.installed {
            skill.conflict = Some("此仓库有多个同名 Skill，需先明确安装名称".into());
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name).then(a.repo_path.cmp(&b.repo_path)));
    let lock = SourceLock {
        kind: "git".into(),
        identity: git.identity,
        ref_: git.ref_,
        resolved_commit: snapshot.resolved_commit,
        content_digest: snapshot.content_digest,
        locked_at: snapshot.locked_at,
    };
    let token = crate::ids::new_id();
    let stored = Preview {
        url: git.url,
        lock,
        skills,
        binding_stamp,
    };
    let dir = data.join("library/git-previews");
    std::fs::create_dir_all(&dir)?;
    crate::sync_common::atomic_write(
        &dir.join(format!("{token}.json")),
        &serde_json::to_vec_pretty(&stored)?,
    )?;
    Ok(
        json!({"preview_id":token,"url":stored.url,"ref":stored.lock.ref_,"resolved_commit":stored.lock.resolved_commit,"skills":stored.skills}),
    )
}

pub fn install(data: &Path, token: &str, selected: &[String]) -> Result<Value> {
    crate::source_rebinding::with_binding_lock(data, || install_locked(data, token, selected))
}

fn install_locked(data: &Path, token: &str, selected: &[String]) -> Result<Value> {
    if uuid::Uuid::parse_str(token).is_err() {
        return Err(Error::new(code::USAGE, "无效预览 ID，请重新扫描仓库"));
    }
    if selected.is_empty() || selected.len() > 100 {
        return Err(Error::new(code::USAGE, "请选择 1–100 个 Skill"));
    }
    let preview: Preview = serde_json::from_slice(&std::fs::read(
        data.join("library/git-previews")
            .join(format!("{token}.json")),
    )?)?;
    if preview.binding_stamp != crate::repository_identity::binding_stamp(data)? {
        return Err(Error::new(
            code::PRECONDITION_FAILED,
            "来源已变更，请重新扫描仓库",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut chosen = Vec::new();
    for id in selected {
        let skill = preview
            .skills
            .iter()
            .find(|s| &s.id == id)
            .ok_or_else(|| Error::new(code::USAGE, "所选 Skill 不在本次预览中"))?;
        if !seen.insert(id) || skill.installed || skill.conflict.is_some() {
            return Err(Error::new(
                code::PRECONDITION_FAILED,
                "所选 Skill 已安装、存在冲突或重复，请重新扫描仓库",
            ));
        }
        chosen.push(skill);
    }
    let git = GitSource::new(&preview.url, preview.lock.ref_.as_deref())?.with_scope(Scope::Skills);
    // 不联网前移版本；验证已预览快照后才开始复制。
    let snapshot = git.resolve(&skill_git_cache(data, &git.identity), Some(&preview.lock))?;
    if snapshot.content_digest != preview.lock.content_digest {
        return Err(Error::new(
            code::PRECONDITION_FAILED,
            "仓库快照已变化，请重新扫描",
        ));
    }
    let mut imported = Vec::new();
    let mut failed = Vec::new();
    for skill in chosen {
        let result = (|| {
            let dir = snapshot.root.join(&skill.repo_path);
            let check = import_preview_inner(data, &dir, Some(&skill.name), false)?;
            if !check.conflicts.is_empty() {
                return Err(Error::new(
                    code::SOURCE_CONFLICT,
                    "资源库已有同名 Skill，未覆盖",
                ));
            }
            let meta = crate::skill_source::git_meta(
                &git_discovery_label(&preview.url),
                &preview.url,
                &skill.repo_path,
                preview.lock.ref_.as_deref(),
                preview.lock.resolved_commit.as_deref(),
                "",
            );
            import_execute_with_source(data, &dir, Some(&skill.name), Some(meta))
        })();
        match result {
            Ok(report) => imported
                .push(json!({"id":skill.id,"name":skill.name,"resource_id":report.skill_id})),
            Err(e) => failed.push(json!({"id":skill.id,"name":skill.name,"error":e.message})),
        }
    }
    Ok(json!({"imported":imported,"failed":failed}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_collection_recognition_uses_transport_aliases() {
        let tmp = tempfile::tempdir().unwrap();
        let skill = tmp.path().join("external-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: fixture\ndescription: fixture\n---\nContent\n",
        )
        .unwrap();
        let mut registry = crate::collections::Registry::default();
        registry.sources.insert(
            "fixture".into(),
            crate::collections::Collection {
                id: "fixture".into(),
                name: "fixture".into(),
                url: "git@github.com:Owner/repo.git".into(),
                lock: SourceLock {
                    kind: "git".into(),
                    identity: "unused".into(),
                    ref_: None,
                    resolved_commit: None,
                    content_digest: "fixture".into(),
                    locked_at: "fixture".into(),
                },
                connection: None,
                migration: None,
                external_path: Some(skill.canonicalize().unwrap()),
            },
        );
        let data = tmp.path().join("data");
        let file = crate::collections::registry_path(&data);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, serde_json::to_vec(&registry).unwrap()).unwrap();
        for url in [
            "https://github.com/Owner/repo",
            "https://github.com/Owner/repo.git/",
            "ssh://git@github.com/Owner/repo.git",
        ] {
            assert!(installed_paths(&data, &repository_key(url))
                .unwrap()
                .contains(""));
        }
        assert!(
            installed_paths(&data, &repository_key("https://gitlab.com/Owner/repo.git"))
                .unwrap()
                .is_empty()
        );
        assert_ne!(
            repository_key("https://github.com/Owner/repo.git"),
            repository_key("https://github.com/owner/repo.git")
        );
        assert_ne!(
            repository_key("https://github.com/Owner/repo.git.git"),
            repository_key("https://github.com/Owner/repo.git")
        );
        assert_eq!(repository_path("."), "");
        assert_eq!(repository_path("./skills/path."), "skills/path.");
    }
}
