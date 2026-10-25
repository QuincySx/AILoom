//! Declared package dependencies link project knowledge; they never merge its contents.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct PackageReference {
    pub package: String,
    pub required: String,
    pub manifest: PathBuf,
    pub field: String,
    pub provider_manifest: PathBuf,
    pub provider_version: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct RelatedProject {
    pub repo_id: String,
    pub project_id: String,
    pub knowledge_id: String,
    pub name: String,
    pub knowledge_path: PathBuf,
    pub index_path: PathBuf,
    pub packages: Vec<PackageReference>,
}

#[derive(Default)]
pub(super) struct Discovery {
    pub projects: Vec<RelatedProject>,
    pub warnings: Vec<String>,
}

/// Read only root + explicitly declared npm workspaces. This is metadata discovery,
/// not dependency installation or execution. Unsupported patterns are not guessed.
fn manifests(root: &Path) -> BTreeMap<PathBuf, Value> {
    fn read(root: &Path, rel: &Path) -> Option<Value> {
        let file = root.join(rel);
        if rel.components().any(|c| !matches!(c, Component::Normal(_)))
            || !file
                .canonicalize()
                .ok()?
                .starts_with(root.canonicalize().ok()?)
            || file.metadata().ok()?.len() > 1024 * 1024
        {
            return None;
        }
        serde_json::from_slice(&std::fs::read(file).ok()?).ok()
    }
    let mut result = BTreeMap::new();
    let root_path = PathBuf::from("package.json");
    let Some(package) = read(root, &root_path) else {
        return result;
    };
    let patterns = package["workspaces"]
        .as_array()
        .or_else(|| package["workspaces"]["packages"].as_array())
        .cloned()
        .unwrap_or_default();
    result.insert(root_path, package);
    for pattern in patterns.iter().take(128).filter_map(Value::as_str) {
        if pattern.contains(['\\', '!', '[', '{']) {
            continue;
        }
        let mut dirs = vec![PathBuf::new()];
        for part in Path::new(pattern).components() {
            let Component::Normal(part) = part else {
                dirs.clear();
                break;
            };
            let mut next = Vec::new();
            if part == "*" {
                for dir in &dirs {
                    let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
                        continue;
                    };
                    for e in entries.flatten().take(512) {
                        if !e.file_type().is_ok_and(|t| t.is_dir())
                            || e.file_name().to_string_lossy().starts_with('.')
                            || e.file_name() == "node_modules"
                        {
                            continue;
                        }
                        next.push(dir.join(e.file_name()));
                    }
                }
            } else if part.to_string_lossy().contains('*') {
                dirs.clear();
                break;
            } else {
                for dir in &dirs {
                    next.push(dir.join(part));
                }
            }
            dirs = next;
            if dirs.len() > 512 {
                dirs.clear();
                break;
            }
        }
        for dir in dirs {
            if result.len() >= 512 {
                break;
            }
            let rel = dir.join("package.json");
            if let Some(p) = read(root, &rel) {
                result.insert(rel, p);
            }
        }
    }
    result
}

pub(super) fn related(cat: &Catalog, project: &str, root: &Path) -> Discovery {
    let consumers = manifests(root);
    if consumers.is_empty() {
        return Discovery::default();
    }
    let mut result = Discovery::default();
    for (repo, b) in &cat.projects {
        let target = match ingest::project_key(repo, &b.project_root) {
            Ok(id) => id,
            Err(_) => {
                result.warnings.push(format!(
                    "{} 的项目身份无法读取，未建立依赖关联",
                    wiki::project_name(&b.project_root)
                ));
                continue;
            }
        };
        if target == project {
            continue;
        }
        let providers = manifests(&b.project_root);
        let mut packages = Vec::new();
        for (manifest, consumer) in &consumers {
            for field in [
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
            ] {
                let Some(deps) = consumer[field].as_object() else {
                    continue;
                };
                for (provider_manifest, provider) in &providers {
                    let Some(name) = provider["name"].as_str() else {
                        continue;
                    };
                    let Some(required) = deps.get(name).and_then(Value::as_str) else {
                        continue;
                    };
                    packages.push(PackageReference {
                        package: name.into(),
                        required: required.into(),
                        manifest: manifest.clone(),
                        field: field.into(),
                        provider_manifest: provider_manifest.clone(),
                        provider_version: provider["version"].as_str().map(String::from),
                    });
                }
            }
        }
        if !packages.is_empty() {
            result.projects.push(RelatedProject {
                repo_id: repo.clone(),
                project_id: target.clone(),
                knowledge_id: b.id.clone(),
                name: wiki::project_name(&b.project_root),
                knowledge_path: b.path.clone(),
                index_path: b.path.join(wiki::base(&target)).join("index.md"),
                packages,
            });
        }
    }
    result
}

pub(super) fn portable(discovery: &Discovery) -> Value {
    json!({"schema_version":1,"semantics":"declared-package-match; not proof of installed version or current compatibility","warnings":discovery.warnings,"dependencies":discovery.projects.iter().map(|p| json!({"project_id":p.project_id,"knowledge_id":p.knowledge_id,"name":p.name,"packages":p.packages})).collect::<Vec<_>>()})
}
