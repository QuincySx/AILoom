//! Code repository addresses are lookup keys; UUIDs remain the ownership boundary.
use crate::error::{code, Error, Result};
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

pub(crate) fn valid_url(url: &str) -> bool {
    url.len() <= 512
        && super::portable::portable_url(url)
        && crate::repository_identity::validate_url(url).is_ok()
        && crate::gitx::redact_credentials(url) == url
}

pub(crate) fn key(url: &str) -> String {
    let key = crate::repository_identity::key(url);
    if key.starts_with("github.com/") {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

/// Explicit package metadata can identify a repository behind a private SSH alias.
pub(crate) fn declared_repository(root: &Path) -> Option<String> {
    let path = root.join("package.json");
    let meta = path.symlink_metadata().ok()?;
    if !meta.is_file() || meta.len() > 128 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let repository = &value["repository"];
    let url = repository
        .as_str()
        .or_else(|| repository["url"].as_str())?
        .trim();
    let url = url.strip_prefix("git+").unwrap_or(url);
    valid_url(url).then(|| url.to_string())
}

pub(crate) fn repository_urls(root: &Path, saved: &[String]) -> Vec<String> {
    let mut urls: BTreeSet<_> = saved.iter().filter(|url| valid_url(url)).cloned().collect();
    if let Some(url) = declared_repository(root) {
        urls.insert(url);
    }
    if crate::repo_registry::find_git_root(root).is_some() {
        for remote in super::location::git(root, &["remote"])
            .unwrap_or_default()
            .lines()
            .take(32)
        {
            if let Ok(url) = super::location::git(root, &["remote", "get-url", remote]) {
                let url = url.trim();
                if valid_url(url) {
                    urls.insert(url.into());
                }
            }
        }
    }
    urls.into_iter().take(64).collect()
}

#[derive(Deserialize)]
pub(crate) struct ProjectIdentity {
    pub schema_version: u32,
    pub project_id: String,
    pub knowledge_id: String,
    pub name: String,
    #[serde(default)]
    pub repository_urls: Vec<String>,
}

/// Discover only modern, project-owned directories. Never follow metadata links.
pub(crate) fn read(path: &Path) -> Result<Option<ProjectIdentity>> {
    let conflict = || {
        Error::new(
            code::KNOWLEDGE_STATE_CONFLICT,
            "项目知识身份记录无效或与标识不一致",
        )
    };
    for relative in [
        ".ailoom",
        ".ailoom/project",
        ".ailoom/project/project.json",
        ".ailoom/library.json",
    ] {
        if path
            .join(relative)
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(conflict());
        }
    }
    let read_json = |relative: &str| -> Result<Option<Vec<u8>>> {
        let file = path.join(relative);
        match file.metadata() {
            Ok(meta) if meta.is_file() && meta.len() <= 65536 => Ok(Some(std::fs::read(file)?)),
            Ok(_) => Err(conflict()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    };
    let Some(marker) = read_json(".ailoom/library.json")? else {
        return Ok(None);
    };
    let Some(bytes) = read_json(".ailoom/project/project.json")? else {
        return Ok(None);
    };
    let identity: ProjectIdentity = serde_json::from_slice(&bytes)?;
    let marker: serde_json::Value = serde_json::from_slice(&marker)?;
    if identity.schema_version != 1
        || marker["schema_version"] != 1
        || marker["id"] != identity.knowledge_id
        || marker["project_id"] != identity.project_id
        || uuid::Uuid::parse_str(&identity.project_id).is_err()
        || uuid::Uuid::parse_str(&identity.knowledge_id).is_err()
        || identity.name.is_empty()
        || identity.name.chars().any(char::is_control)
        || identity.repository_urls.len() > 64
        || identity.repository_urls.iter().any(|url| !valid_url(url))
    {
        return Err(conflict());
    }
    Ok(Some(identity))
}
