//! Explicit discovery returns repository-scoped entry points, never knowledge bodies.
use super::*;

fn name_key(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn url_key(value: &str) -> String {
    let key = crate::repository_identity::key(value);
    if key.starts_with("github.com/") {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

/// A root package manifest may explicitly declare the canonical repository while
/// Git uses a private SSH host alias. This is evidence, not a guessed host mapping.
fn declared_repository(root: &Path) -> Option<String> {
    let path = root.join("package.json");
    let meta = path.symlink_metadata().ok()?;
    if !meta.is_file() || meta.len() > 128 * 1024 {
        return None;
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let repository = &value["repository"];
    let url = repository
        .as_str()
        .or_else(|| repository["url"].as_str())?
        .trim();
    let url = url.strip_prefix("git+").unwrap_or(url);
    if !super::super::portable::portable_url(url)
        || crate::repository_identity::validate_url(url).is_err()
        || crate::gitx::redact_credentials(url) != url
    {
        return None;
    }
    Some(url.to_string())
}

pub(super) fn locate(data: &Path, query: &str, limit: usize) -> Result<Value> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 512 {
        return Err(Error::new(
            code::USAGE,
            "--query 需要仓库名称、Git 地址或本地路径（最多 512 字）",
        ));
    }
    let cat = load(data)?;
    let by_url = query.contains("://") || (query.contains('@') && query.contains(':'));
    let path = Path::new(query);
    let by_path = !by_url
        && (path.is_absolute()
            || query.starts_with("./")
            || query.starts_with("../")
            || (query.contains('/') && path.exists()));
    if !by_path && !by_url && name_key(query).is_empty() {
        return Err(Error::new(
            code::USAGE,
            "请提供有效的仓库名称、Git 地址或本地路径",
        ));
    }
    if by_url {
        crate::repository_identity::validate_url(query)?;
    }
    let query_path = if by_path {
        path.canonicalize().ok()
    } else {
        None
    };
    let requested_repo = query_path
        .as_ref()
        .filter(|p| p.is_dir())
        .and_then(|p| crate::repo_registry::discover_repo(p).ok());
    let mappings = crate::repository_identity::connections(data).unwrap_or_default();
    let canonical = |url: &str| {
        let mapped: std::collections::BTreeSet<_> = mappings
            .iter()
            .filter_map(|c| {
                if c.url == url {
                    Some(url_key(&c.repository_url))
                } else {
                    c.host_mapping
                        .as_ref()
                        .and_then(|m| crate::repository_identity::mapped_url(url, m).ok())
                        .map(|u| url_key(&u))
                }
            })
            .collect();
        if mapped.len() == 1 {
            mapped.into_iter().next().unwrap()
        } else {
            url_key(url)
        }
    };
    let mut matches = Vec::new();
    let mut warnings = Vec::new();
    for (repo, b) in &cat.projects {
        let name = wiki::project_name(&b.project_root);
        let declaration = match super::super::portable::read(&b.project_root) {
            Ok(value) => value,
            Err(_) => {
                warnings.push(format!("{name} 的项目身份无法读取，未作为候选"));
                continue;
            }
        };
        if declaration.as_ref().is_some_and(|d| d.knowledge_id != b.id) {
            warnings.push(format!("{name} 的知识库绑定与项目声明不一致，未作为候选"));
            continue;
        }
        let project_id = declaration
            .as_ref()
            .map(|d| d.project_id.as_str())
            .unwrap_or(repo);
        let mut urls = Vec::new();
        let declared = declared_repository(&b.project_root);
        if crate::repo_registry::find_git_root(&b.project_root).is_some() {
            for remote in git(&b.project_root, &["remote"])
                .unwrap_or_default()
                .lines()
                .take(32)
            {
                if let Ok(url) = git(&b.project_root, &["remote", "get-url", remote]) {
                    urls.push(url.trim().to_string());
                }
            }
        }
        let kind = if by_url {
            if urls.iter().any(|u| canonical(u) == canonical(query)) {
                Some((0, "git_url"))
            } else {
                declared
                    .as_ref()
                    .is_some_and(|u| canonical(u) == canonical(query))
                    .then_some((1, "declared_repository_url"))
            }
        } else if by_path {
            let direct = query_path
                .as_ref()
                .is_some_and(|p| p.starts_with(&b.project_root));
            let same_repo = requested_repo.as_ref().is_some_and(|requested| {
                crate::repo_registry::discover_repo(&b.project_root)
                    .is_ok_and(|d| d.identity.common_dir == requested.identity.common_dir)
            });
            (if requested_repo.is_some() {
                same_repo
            } else {
                direct
            })
            .then_some((0, "local_path"))
        } else if query == repo || query == project_id {
            Some((0, "project_id"))
        } else {
            let key = name_key(query);
            let name_key_value = name_key(&name);
            if key == name_key_value {
                Some((1, "name"))
            } else if name_key_value.contains(&key) {
                Some((2, "partial_name"))
            } else {
                None
            }
        };
        let Some((rank, kind)) = kind else { continue };
        let index = b.path.join(wiki::base(project_id)).join("index.md");
        let source_index = b
            .path
            .join("imports/worktrees")
            .join(project_id)
            .join("index.md");
        matches.push((rank,json!({"repo_id":repo,"project_id":project_id,"name":name,"match":kind,
            "repository_root":b.project_root,"repository_urls":urls.iter().filter(|u|crate::gitx::redact_credentials(u)==**u).collect::<Vec<_>>(),
            "declared_repository_url":declared,
            "knowledge_id":b.id,"knowledge_path":b.path,"available":b.path.is_dir(),
            "wiki_index":index.is_file().then_some(index),"source_index":source_index.is_file().then_some(source_index),
            "recall_command":["ailoom","--json","knowledge","--action","recall","--root",b.project_root.to_string_lossy().as_ref(),"--query","<要查询的问题>"]
        })));
    }
    matches.sort_by(|(a, x), (b, y)| {
        a.cmp(b)
            .then(x["repo_id"].as_str().cmp(&y["repo_id"].as_str()))
    });
    let total = matches.len();
    let candidates: Vec<_> = matches
        .into_iter()
        .take(limit.clamp(1, 100))
        .map(|(_, v)| v)
        .collect();
    Ok(
        json!({"status":if total==0 {"not_found"} else if total==1 {"found"} else {"ambiguous"},
        "query":query,"total":total,"candidates":candidates,"warnings":warnings,
        "scope":"本机已登记仓库；仅返回入口，未读取或合并知识正文。多个候选时用 Git 地址或本地路径进一步定位。"}),
    )
}
