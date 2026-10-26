//! One knowledge repository, with a readable directory for each source project.
use super::*;

pub(super) fn default_root(data: &Path, cat: &Catalog) -> PathBuf {
    cat.repository_path
        .clone()
        .unwrap_or_else(|| data.join("knowledge/repository"))
}

pub(super) fn directory_name(root: &Path) -> String {
    let name: String = wiki::project_name(root)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    if name.is_empty() || name.starts_with('.') {
        format!("project-{}", name.trim_start_matches('.'))
    } else {
        name
    }
}

pub(super) fn prepare(
    data: &Path,
    cat: &Catalog,
    root: &Path,
    r: &Request,
) -> Result<(Request, Option<PathBuf>)> {
    let mut next = r.clone();
    let repository = match &r.repository_path {
        Some(path) if matches!(r.action.as_str(), "init" | "move" | "separate") => {
            destination(path)?
        }
        Some(_) => {
            return Err(fail(
                "--repository 适用于 init、move 或 separate；恢复时请选择具体项目目录",
            ))
        }
        None if r.action == "init"
            && r.path.is_none()
            && super::super::portable::read(root)?.is_none() =>
        {
            destination(&default_root(data, cat))?
        }
        None => return Ok((next, None)),
    };
    if repository.join(layout::HIDDEN_MARKER).exists() {
        return Err(fail(
            "所选位置是一个项目的知识目录，请选择容纳各项目文件夹的知识库仓库",
        ));
    }
    let path = match &r.path {
        Some(path) => destination(path)?,
        None => cat
            .projects
            .values()
            .find(|b| {
                b.project_root == root && b.path.starts_with(&repository) && b.path != repository
            })
            .map(|b| b.path.clone())
            .unwrap_or_else(|| repository.join(directory_name(root))),
    };
    let path = destination(&path)?;
    if path == repository || !path.starts_with(&repository) {
        return Err(fail("本项目知识必须放在知识库仓库的项目子目录中"));
    }
    if root.starts_with(&path) {
        return Err(fail("项目知识目录不能占用当前代码项目或其上级目录"));
    }
    if r.remote.is_some() && r.subdir.is_none() {
        next.subdir = Some(
            path.strip_prefix(&repository)
                .unwrap()
                .to_string_lossy()
                .into(),
        );
    }
    next.path = Some(path);
    Ok((next, Some(repository)))
}

pub(super) fn status(data: &Path, cat: &Catalog, root: &Path, result: &mut Value) {
    let selected = result["location"]["id"]
        .as_str()
        .and_then(|id| cat.repositories.get(id));
    let repository = selected.cloned().unwrap_or_else(|| default_root(data, cat));
    let name = selected
        .and_then(|repository| {
            result["location"]["path"]
                .as_str()
                .and_then(|path| Path::new(path).strip_prefix(repository).ok())
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| directory_name(root));
    result["repository_path"] = json!(repository);
    result["directory_name"] = json!(name);
    result["storage_mode"] = json!(if result["initialized"] == true && selected.is_none() {
        "directory"
    } else {
        "repository"
    });
    // Recovery hints refer to the exact project directory, not the shared repository.
    if result["recovery"].is_null() || result["initialized"] == true {
        result["default_path"] = json!(repository.join(name));
    }
}

/// Update only our navigation block. Human text outside it remains untouched.
pub(super) fn navigation(cat: &Catalog, repository: &Path) -> Result<()> {
    let path = repository.join("README.md");
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(fail("知识库首页不能是符号链接"));
    }
    let old = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "# 开发知识库\n\n各项目的原文、知识正文和维护记录保存在各自文件夹中。人工与 AI 维护同一份 Markdown。\n".into(),
        Err(e) => return Err(e.into()),
    };
    let start = "<!-- ailoom:projects:start -->";
    let end = "<!-- ailoom:projects:end -->";
    let mut projects = BTreeMap::new();
    // A shared checkout can contain projects not yet registered on this machine.
    for entry in std::fs::read_dir(repository)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && layout::marker_at(&entry.path()).is_file()
            && entry.path().join("README.md").is_file()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            projects.insert(format!("{name}/README.md"), directory_name(&entry.path()));
        }
    }
    for b in cat.projects.values() {
        if let Ok(relative) = b.path.strip_prefix(repository) {
            if !relative.as_os_str().is_empty() {
                let link = relative
                    .join("README.md")
                    .to_string_lossy()
                    .replace('\\', "/");
                projects.insert(link, directory_name(&b.project_root));
            }
        }
    }
    let mut block = format!("{start}\n## 项目\n\n");
    for (link, name) in projects {
        let link = link
            .replace('%', "%25")
            .replace(' ', "%20")
            .replace('#', "%23")
            .replace('(', "%28")
            .replace(')', "%29");
        block.push_str(&format!("- [{name}]({link})\n"));
    }
    block.push_str(end);
    let text = match (old.find(start), old.find(end)) {
        (Some(a), Some(b)) if a < b => format!("{}{}{}", &old[..a], block, &old[b + end.len()..]),
        (None, None) => format!("{}\n\n{block}\n", old.trim_end()),
        _ => {
            return Err(fail(
                "知识库首页的项目导航标记不完整，请保留正文并修复标记后重试",
            ))
        }
    };
    crate::sync_common::atomic_write(&path, text.as_bytes())
}
