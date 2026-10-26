//! Explicit discovery returns repository-scoped entry points, never knowledge bodies.
use super::super::identity::{self, declared_repository};
use super::*;

fn name_key(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub(super) fn locate(
    data: &Path,
    query: &str,
    limit: usize,
    repository: Option<&Path>,
) -> Result<Value> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 512 {
        return Err(Error::new(
            code::USAGE,
            "--query 需要仓库名称、Git 地址或本地路径（最多 512 字）",
        ));
    }
    let cat = load(data)?;
    let selected_repository = repository.map(Path::canonicalize).transpose()?;
    if selected_repository
        .as_ref()
        .is_some_and(|root| !root.is_dir())
    {
        return Err(fail("请选择知识库仓库目录"));
    }
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
                    Some(identity::key(&c.repository_url))
                } else {
                    c.host_mapping
                        .as_ref()
                        .and_then(|m| crate::repository_identity::mapped_url(url, m).ok())
                        .map(|u| identity::key(&u))
                }
            })
            .collect();
        if mapped.len() == 1 {
            mapped.into_iter().next().unwrap()
        } else {
            identity::key(url)
        }
    };
    let mut matches = Vec::new();
    let mut warnings = Vec::new();
    for (repo, b) in &cat.projects {
        if selected_repository
            .as_ref()
            .is_some_and(|root| !b.path.starts_with(root))
        {
            continue;
        }
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
        let saved = match identity::read(&b.path) {
            Ok(saved) => saved,
            Err(_) => {
                warnings.push(format!("{name} 的知识身份无法核对，未作为候选"));
                continue;
            }
        };
        if saved.as_ref().is_some_and(|s| {
            s.knowledge_id != b.id
                || declaration
                    .as_ref()
                    .is_some_and(|d| d.project_id != s.project_id)
        }) {
            warnings.push(format!("{name} 的知识身份与本机绑定不一致，未作为候选"));
            continue;
        }
        let project_id = declaration
            .as_ref()
            .map(|d| d.project_id.as_str())
            .or_else(|| saved.as_ref().map(|s| s.project_id.as_str()))
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
                    if identity::valid_url(url.trim()) {
                        urls.push(url.trim().to_string());
                    }
                }
            }
        }
        let saved_urls: Vec<_> = declaration
            .as_ref()
            .map(|d| d.repository_urls.as_slice())
            .unwrap_or_default()
            .iter()
            .chain(
                saved
                    .as_ref()
                    .map(|s| s.repository_urls.as_slice())
                    .unwrap_or_default(),
            )
            .cloned()
            .collect();
        let kind = if by_url {
            if urls.iter().any(|u| canonical(u) == canonical(query)) {
                Some((0, "git_url"))
            } else if declared
                .as_ref()
                .is_some_and(|u| canonical(u) == canonical(query))
            {
                Some((1, "declared_repository_url"))
            } else {
                saved_urls
                    .iter()
                    .any(|u| canonical(u) == canonical(query))
                    .then_some((1, "saved_repository_url"))
            }
        } else if by_path {
            let knowledge_path = query_path.as_ref().is_some_and(|p| p.starts_with(&b.path));
            let direct = query_path
                .as_ref()
                .is_some_and(|p| p.starts_with(&b.project_root));
            let same_repo = requested_repo.as_ref().is_some_and(|requested| {
                crate::repo_registry::discover_repo(&b.project_root)
                    .is_ok_and(|d| d.identity.common_dir == requested.identity.common_dir)
            });
            (knowledge_path
                || if requested_repo.is_some() {
                    same_repo
                } else {
                    direct
                })
            .then_some((0, "local_path"))
        } else if query == repo || query == project_id || query == b.id {
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
        let index = if b.path.join("README.md").is_file() {
            b.path.join("README.md")
        } else {
            b.path.join(wiki::base(project_id)).join("index.md")
        };
        let source_index = layout::source_index_at(&b.path, project_id);
        urls.extend(saved_urls);
        urls.sort();
        urls.dedup();
        let code_available = b.project_root.is_dir();
        let command = if code_available {
            vec![
                "ailoom",
                "--json",
                "knowledge",
                "--action",
                "recall",
                "--root",
                b.project_root.to_str().unwrap_or(""),
                "--query",
                "<要查询的问题>",
            ]
        } else {
            vec![
                "ailoom",
                "--json",
                "knowledge",
                "--action",
                "recall",
                "--path",
                b.path.to_str().unwrap_or(""),
                "--query",
                "<要查询的问题>",
            ]
        };
        matches.push((rank,json!({"repo_id":repo,"project_id":project_id,"name":name,"match":kind,
            "repository_root":code_available.then_some(&b.project_root),"repository_urls":urls.iter().filter(|u|identity::valid_url(u)).collect::<Vec<_>>(),"code_available":code_available,
            "declared_repository_url":declared,
            "knowledge_id":b.id,"knowledge_path":b.path,"available":b.path.is_dir(),
            "wiki_index":index.is_file().then_some(index),"source_index":source_index.is_file().then_some(source_index),
            "recall_command":command
        })));
    }
    let roots: std::collections::BTreeSet<_> = if let Some(root) = selected_repository {
        [root].into_iter().collect()
    } else {
        cat.repositories
            .values()
            .cloned()
            .chain([repository::default_root(data, &cat)])
            .collect()
    };
    let mut seen: std::collections::BTreeSet<_> =
        cat.projects.values().map(|b| b.path.clone()).collect();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&root)
            .max_depth(8)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                e.depth() == 0
                    || (e.file_type().is_dir()
                        && !e.file_name().to_string_lossy().starts_with('.')
                        && !e
                            .path()
                            .parent()
                            .is_some_and(|p| p.join(layout::HIDDEN_MARKER).is_file()))
            })
            .take(10000)
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    warnings.push("部分知识目录无法读取，未作为候选".into());
                    continue;
                }
            };
            if !seen.insert(entry.path().into()) {
                continue;
            }
            let identity = match identity::read(entry.path()) {
                Ok(Some(identity)) => identity,
                Ok(None) => continue,
                Err(_) => {
                    warnings.push(format!(
                        "{} 的项目身份无法核对，未作为候选",
                        entry.path().display()
                    ));
                    continue;
                }
            };
            let kind = if by_url {
                identity
                    .repository_urls
                    .iter()
                    .any(|url| canonical(url) == canonical(query))
                    .then_some((1, "saved_repository_url"))
            } else if by_path {
                query_path
                    .as_ref()
                    .is_some_and(|p| p.starts_with(entry.path()))
                    .then_some((0, "knowledge_path"))
            } else if query == identity.project_id || query == identity.knowledge_id {
                Some((0, "project_id"))
            } else if name_key(query) == name_key(&identity.name) {
                Some((1, "name"))
            } else if name_key(&identity.name).contains(&name_key(query)) {
                Some((2, "partial_name"))
            } else {
                None
            };
            let Some((rank, kind)) = kind else { continue };
            let path = entry.path();
            matches.push((rank,json!({"repo_id":null,"project_id":identity.project_id,"knowledge_id":identity.knowledge_id,"name":identity.name,"match":kind,
                "repository_root":null,"repository_urls":identity.repository_urls,"declared_repository_url":null,"code_available":false,
                "knowledge_path":path,"available":true,"wiki_index":path.join("README.md").is_file().then(||path.join("README.md")),
                "source_index":path.join("sources/README.md").is_file().then(||path.join("sources/README.md")),
                "recall_command":["ailoom","--json","knowledge","--action","recall","--path",path,"--query","<要查询的问题>"]
            })));
        }
    }
    matches.sort_by(|(a, x), (b, y)| {
        a.cmp(b)
            .then(x["repo_id"].as_str().cmp(&y["repo_id"].as_str()))
            .then(x["project_id"].as_str().cmp(&y["project_id"].as_str()))
            .then(
                x["knowledge_path"]
                    .as_str()
                    .cmp(&y["knowledge_path"].as_str()),
            )
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
        "scope":"本机已登记项目及所选知识仓库内的项目身份；仅返回入口，未读取或合并正文。多个候选时用项目 ID、Git 地址或路径进一步定位；未联网搜索远端。"}),
    )
}
