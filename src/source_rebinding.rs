//! Preview and atomically change a source binding without replacing installed content.
use crate::error::{code, Error, Result};
use crate::repository_identity::{self, GitConnection, HostMapping};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub kind: String,
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub url: String,
    #[serde(default)]
    pub repository_url: Option<String>,
    #[serde(rename = "ref", default)]
    pub ref_: Option<String>,
    #[serde(default)]
    pub repo_path: Option<String>,
    /// local keeps the working directory; remote uses immutable fetched snapshots.
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub host_mapping: Option<HostMapping>,
}
#[derive(Serialize, Deserialize)]
struct Preview {
    target: Target,
    before: Value,
    digest: String,
    registry_revision: u64,
    stamp: String,
    next: Value,
    relationship: String,
    /// Pinned candidate used to validate the new connection, not applied as content.
    validation: Option<crate::source::SourceLock>,
}

fn skill_path(data: &Path, id: &str) -> Result<PathBuf> {
    let name = id
        .strip_prefix("personal/skill/personal/")
        .ok_or_else(|| Error::new(code::USAGE, "仅个人 Skill 支持关联 Git"))?;
    if !crate::manifest::valid_name(name) {
        return Err(Error::new(code::USAGE, "Skill 名称无效"));
    }
    let path = crate::personal_library::library_root(data)
        .join("resources/skills")
        .join(name);
    if !path.is_dir() || path.symlink_metadata()?.file_type().is_symlink() {
        return Err(Error::new(code::ILLEGAL_PATH, "外部维护目录不能变更来源"));
    }
    Ok(path)
}

fn current(data: &Path, target: &Target) -> Result<(Value, String)> {
    match target.kind.as_str() {
        "skill" => {
            let path = skill_path(data, &target.id)?;
            let source = crate::skill_source::read_meta(&path).ok_or_else(|| {
                Error::new(code::USAGE, "此 Skill 没有已登记来源，请重新导入来源后管理")
            })?;
            Ok((
                serde_json::to_value(source)?,
                crate::personal_library::skill_dir_digest(&path)?,
            ))
        }
        "collection" => {
            let source = crate::collections::load(data)?
                .sources
                .remove(&target.id)
                .ok_or_else(|| Error::new(code::UNKNOWN_REFERENCE, "来源不存在"))?;
            if source.external_path.is_some() && !source.id.starts_with("collection-portable-") {
                return Err(Error::new(code::USAGE, "此来源由原目录维护"));
            }
            let catalog = crate::collections::catalog(data, &source)?;
            Ok((
                serde_json::to_value(source)?,
                catalog.snapshot.content_digest,
            ))
        }
        _ => Err(Error::new(code::USAGE, "来源类型无效")),
    }
}

/// Read only the working repository's root, branch and named remotes. No SSH configuration access.
pub fn discover(data: &Path, target: &Target) -> Result<Value> {
    let (before, _) = current(data, target)?;
    let local = if target.kind == "skill" {
        serde_json::from_value::<crate::skill_source::SkillSourceMeta>(before.clone())?.local_dir()
    } else {
        None
    };
    let mut result = json!({"target":target,"source":before,"remotes":[],"local_dir":local});
    if let Some(local) = local {
        let root = crate::gitx::rev_parse_path(&local, &["--show-toplevel"])?;
        let local = local.canonicalize()?;
        let relative = local
            .strip_prefix(&root)
            .map_err(|_| Error::new(code::PATH_TRAVERSAL, "Skill 不在 Git 仓库内"))?;
        let names = crate::gitx::git(&root, &["remote"])?;
        let remotes: Vec<_> = names
            .lines()
            .filter_map(|name| {
                let url = crate::gitx::git_optional(&root, &["remote", "get-url", name])?;
                // Embedded credentials must not be returned to the UI.
                repository_identity::validate_url(&url).ok()?;
                Some(json!({"name":name,"url":url}))
            })
            .collect();
        result["remotes"] = json!(remotes);
        result["git_root"] = json!(root);
        result["repo_path"] = json!(relative.to_string_lossy().replace('\\', "/"));
        result["ref"] = json!(crate::gitx::git_optional(
            &root,
            &["symbolic-ref", "--short", "HEAD"]
        ));
    }
    Ok(result)
}

fn relative_path(raw: &str) -> Result<PathBuf> {
    let path = Path::new(raw);
    if path.is_absolute()
        || path.components().any(|p| {
            matches!(
                p,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(Error::new(code::PATH_TRAVERSAL, "Skill 仓内路径不能越界"));
    }
    Ok(path
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .collect())
}

fn validate_skill(root: &Path, relative: &str) -> Result<()> {
    let path = root.join(relative_path(relative)?).canonicalize()?;
    if !path.starts_with(root.canonicalize()?) || !path.join("SKILL.md").is_file() {
        return Err(Error::new(
            code::ILLEGAL_PATH,
            "该仓内路径没有可用的 SKILL.md",
        ));
    }
    let file = path.join("SKILL.md");
    if file.symlink_metadata()?.file_type().is_symlink() {
        return Err(Error::new(code::ILLEGAL_PATH, "SKILL.md 不能是符号链接"));
    }
    let text = std::fs::read_to_string(file)?;
    crate::resource::parse_frontmatter(&text)?;
    Ok(())
}

pub fn preview(data: &Path, target: &Target, change: &Change) -> Result<Value> {
    let registry_revision = crate::collections::load(data)?.revision;
    let stamp = repository_identity::binding_stamp(data)?;
    let (before, digest) = current(data, target)?;
    let url = change.url.trim();
    repository_identity::validate_url(url)?;
    let mapped = change
        .host_mapping
        .as_ref()
        .map(|m| repository_identity::mapped_url(url, m))
        .transpose()?;
    let repository_url = change
        .repository_url
        .as_deref()
        .filter(|u| !u.trim().is_empty())
        .map(str::trim)
        .map(str::to_string)
        .or(mapped.clone())
        .unwrap_or_else(|| url.into());
    repository_identity::validate_url(&repository_url)?;
    if let Some(mapped) = mapped {
        if repository_identity::key(&mapped) != repository_identity::key(&repository_url) {
            return Err(Error::new(
                code::USAGE,
                "仓库地址与 SSH HostName 映射不一致",
            ));
        }
    }
    let ref_ = change
        .ref_
        .as_ref()
        .filter(|r| !r.trim().is_empty())
        .map(|r| r.trim().to_string());
    let connection = GitConnection {
        url: url.into(),
        repository_url: repository_url.clone(),
        ref_: ref_.clone(),
        host_mapping: change.host_mapping.clone(),
        revision: crate::ids::new_id(),
    };
    let old_url = before["connection"]["repository_url"]
        .as_str()
        .or(before["repo_url"].as_str())
        .or(before["url"].as_str());
    let relationship = match old_url {
        None => "associate",
        Some(old)
            if change.host_mapping.is_none()
                && change.repository_url.as_deref().is_none_or(|u| {
                    u.trim().is_empty()
                        || repository_identity::key(u) == repository_identity::key(url)
                })
                && repository_identity::key_for(data, old)?
                    == repository_identity::key_for(data, &repository_url)? =>
        {
            "same-address"
        }
        Some(_) => "unverified",
    };
    let mut validation = None;
    let mode = change
        .mode
        .as_deref()
        .unwrap_or(if before["source_kind"] == "local" {
            "local"
        } else {
            "remote"
        });
    if mode != "local" && mode != "remote" {
        return Err(Error::new(code::USAGE, "请选择跟随本地开发或远程版本"));
    }
    let next = if target.kind == "skill" {
        let mut source: crate::skill_source::SkillSourceMeta =
            serde_json::from_value(before.clone())?;
        let path = change
            .repo_path
            .as_deref()
            .unwrap_or(source.repo_path.as_deref().unwrap_or(""));
        let path = relative_path(path)?.to_string_lossy().replace('\\', "/");
        if mode == "local" {
            let discovery = discover(data, target)?;
            if discovery["repo_path"].as_str()
                != Some(path.trim_end_matches('/').trim_start_matches("./"))
            {
                return Err(Error::new(
                    code::USAGE,
                    "跟随本地开发时须使用检测到的 Skill 仓内路径",
                ));
            }
            if !discovery["remotes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["url"] == url)
            {
                return Err(Error::new(
                    code::PRECONDITION_FAILED,
                    "本地 remote 已变化，请重新检测并选择",
                ));
            }
            validate_skill(Path::new(discovery["git_root"].as_str().unwrap()), &path)?;
        } else {
            let git = crate::source::GitSource::new(url, ref_.as_deref())?
                .with_scope(crate::source::git::Scope::Skills);
            let cache = crate::personal_library::skill_git_cache(data, &git.identity);
            let snapshot = git.refresh(&cache)?;
            validate_skill(&snapshot.root, &path)?;
            validation = Some(crate::source::SourceLock {
                kind: "git".into(),
                identity: git.identity,
                ref_: git.ref_,
                resolved_commit: snapshot.resolved_commit,
                content_digest: snapshot.content_digest,
                locked_at: snapshot.locked_at,
            });
            source.source_kind = "git".into();
            source.discovery_entry = repository_url.clone();
        }
        source.repo_url = Some(repository_url.clone());
        source.repo_path = Some(path);
        source.ref_ = ref_;
        source.connection = Some(connection);
        // imported digest, fetched time and installed commit describe retained content.
        serde_json::to_value(source)?
    } else {
        if mode != "remote" {
            return Err(Error::new(code::USAGE, "合集使用远程快照更新"));
        }
        let git = crate::source::GitSource::new(url, ref_.as_deref())?
            .with_scope(crate::source::git::Scope::Catalog);
        let cache = data
            .join("collections/cache")
            .join(crate::ids::cache_key_from_identity(&git.identity));
        let snapshot = git.refresh(&cache)?;
        validation = Some(crate::source::SourceLock {
            kind: "git".into(),
            identity: git.identity.clone(),
            ref_: git.ref_.clone(),
            resolved_commit: snapshot.resolved_commit.clone(),
            content_digest: snapshot.content_digest.clone(),
            locked_at: snapshot.locked_at.clone(),
        });
        crate::collections::enumerate(snapshot, &target.id)?;
        let mut source: crate::collections::Collection = serde_json::from_value(before.clone())?;
        source.connection = Some(connection);
        serde_json::to_value(source)?
    };
    let references = crate::collections::references(
        data,
        if target.kind == "collection" {
            format!("{}/", target.id)
        } else {
            target.id.clone()
        }
        .as_str(),
    )?;
    let token = crate::ids::new_id();
    let stored = Preview {
        target: target.clone(),
        before: before.clone(),
        digest,
        registry_revision,
        stamp,
        next,
        relationship: relationship.into(),
        validation,
    };
    crate::sync_common::write_preview(
        &data.join("source-bindings/previews"),
        &token,
        &serde_json::to_vec_pretty(&stored)?,
    )?;
    Ok(
        json!({"preview_id":token,"relationship":relationship,"before":before,"after":stored.next,"references":references,"mode":mode,"candidate_commit":stored.validation.as_ref().and_then(|v|v.resolved_commit.as_ref()),"note":"保存来源后继续使用当前内容；检查更新后再决定是否升级。"}),
    )
}

pub(crate) fn with_binding_lock<T>(data: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    crate::source::with_file_lock(&data.join("source-bindings/operation.lock"), f)
}

pub fn apply(data: &Path, token: &str, relationship: Option<&str>) -> Result<Value> {
    with_binding_lock(data, || apply_locked(data, token, relationship))
}

fn apply_locked(data: &Path, token: &str, relationship: Option<&str>) -> Result<Value> {
    if uuid::Uuid::parse_str(token).is_err() {
        return Err(Error::new(code::USAGE, "来源预览 ID 无效"));
    }
    let preview: Preview = serde_json::from_slice(&std::fs::read(
        data.join("source-bindings/previews")
            .join(format!("{token}.json")),
    )?)?;
    if preview.relationship == "unverified" && !matches!(relationship, Some("same" | "replace")) {
        return Err(Error::new(
            code::PRECONDITION_FAILED,
            "无法核实是否同一仓库，请明确确认换地址或替换上游",
        ));
    }
    crate::personal_library::update::with_update_lock(data, || {
        crate::source::with_file_lock(&data.join("collections/registry.lock"), || {
            let mut registry = crate::collections::load(data)?;
            if registry.revision != preview.registry_revision
                || repository_identity::binding_stamp(data)? != preview.stamp
            {
                return Err(Error::new(
                    code::PRECONDITION_FAILED,
                    "来源已变化，请重新预览",
                ));
            }
            let (current, digest) = current(data, &preview.target)?;
            if current != preview.before || digest != preview.digest {
                return Err(Error::new(
                    code::PRECONDITION_FAILED,
                    "资源在预览后已变化，请重新预览",
                ));
            }
            if let Some(lock) = &preview.validation {
                let connection: GitConnection =
                    serde_json::from_value(preview.next["connection"].clone())?;
                let skill = preview.target.kind == "skill";
                let git =
                    crate::source::GitSource::new(&connection.url, connection.ref_.as_deref())?
                        .with_scope(if skill {
                            crate::source::git::Scope::Skills
                        } else {
                            crate::source::git::Scope::Catalog
                        });
                let cache = if skill {
                    crate::personal_library::skill_git_cache(data, &git.identity)
                } else {
                    data.join("collections/cache")
                        .join(crate::ids::cache_key_from_identity(&git.identity))
                };
                let snapshot = git.resolve(&cache, Some(lock))?;
                if snapshot.content_digest != lock.content_digest {
                    return Err(Error::new(
                        code::SOURCE_CACHE_CORRUPT,
                        "来源验证快照已变化，请重新预览",
                    ));
                }
            } else {
                // Local remotes may change while the confirmation dialog is open.
                let source: crate::skill_source::SkillSourceMeta =
                    serde_json::from_value(preview.next.clone())?;
                let discovery = discover(data, &preview.target)?;
                if !discovery["remotes"].as_array().unwrap().iter().any(|r| {
                    Some(r["url"].as_str().unwrap_or(""))
                        == source.connection.as_ref().map(|c| c.url.as_str())
                }) || discovery["repo_path"].as_str() != source.repo_path.as_deref()
                {
                    return Err(Error::new(
                        code::PRECONDITION_FAILED,
                        "本地 Git 关联已变化，请重新检测",
                    ));
                }
            }
            if preview.target.kind == "skill" {
                let next = serde_json::from_value(preview.next)?;
                crate::skill_source::write_meta(&skill_path(data, &preview.target.id)?, &next)?;
            } else {
                let next = serde_json::from_value(preview.next)?;
                registry.sources.insert(preview.target.id.clone(), next);
                registry.revision += 1;
                crate::sync_common::atomic_write(
                    &crate::collections::registry_path(data),
                    &serde_json::to_vec_pretty(&registry)?,
                )?;
            }
            Ok(
                json!({"saved":true,"target":preview.target,"note":"来源已保存，当前资源和项目引用已保留。请重新检查更新。"}),
            )
        })
    })
}
