//! Split a legacy multi-repository library by ownership, preserving the old library.
use super::*;

pub(super) fn exclusive(
    cat: &Catalog,
    repo: &str,
    root: &Path,
    path: &Path,
    knowledge_id: Option<&str>,
) -> Result<()> {
    if cat.projects.iter().any(|(id, b)| {
        id != repo
            && !same_project(root, &b.project_root)
            && (path.starts_with(&b.path)
                || b.path.starts_with(path)
                || knowledge_id == Some(b.id.as_str()))
    }) {
        return Err(fail(
            "该项目目录已被其他仓库使用；可选择同一知识库仓库下的另一个项目文件夹",
        ));
    }
    Ok(())
}

fn same_project(a: &Path, b: &Path) -> bool {
    let identity = |p| {
        super::super::portable::read(p)
            .ok()
            .flatten()
            .map(|d| d.project_id)
    };
    identity(a).is_some_and(|id| identity(b).as_ref() == Some(&id))
}

pub(super) fn validate_owner(tree: &Tree, project: &str) -> Result<()> {
    let marker: Value = serde_json::from_slice(
        tree.get(layout::marker(tree))
            .ok_or_else(|| fail("知识库标识缺失"))?,
    )?;
    if marker["project_id"]
        .as_str()
        .is_some_and(|id| id != project)
        || !wiki::tree_single_owner(tree, project)
    {
        return Err(fail(
            "项目目录包含其他仓库的身份或资料；请按项目分文件夹，旧格式可用 separate 提取当前项目",
        ));
    }
    Ok(())
}

fn explicit_owner(bytes: &[u8], project: &str) -> bool {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| crate::resource::split_frontmatter(text).ok().flatten())
        .and_then(|(fm, _)| serde_yaml::from_str::<Value>(&fm).ok())
        .is_some_and(|meta| meta["project_id"] == project)
}

pub(super) fn run(
    data: &Path,
    r: &Request,
    cat: &mut Catalog,
    repo: &str,
    root: &Path,
) -> Result<Value> {
    let old = binding(cat, repo)?.clone();
    let source = checked_tree(&old)?;
    let old_declaration = super::super::portable::read(root)?
        .ok_or_else(|| fail("项目声明缺失，请先恢复仓库的 .ailoom/knowledge.json"))?;
    let project = &old_declaration.project_id;
    let mut declarations = BTreeMap::from([(root.to_path_buf(), old_declaration.clone())]);
    if let Ok(discovery) = crate::repo_registry::discover_repo(root) {
        for worktree in discovery
            .worktrees
            .iter()
            .filter(|w| w.deployable() && w.path.is_dir())
        {
            if let Some(d) = super::super::portable::read(&worktree.path)? {
                if d.project_id == *project && d.knowledge_id == old.id {
                    declarations.insert(worktree.path.clone(), d);
                }
            }
        }
    }
    let dest = destination(
        r.path
            .as_deref()
            .ok_or_else(|| fail("请选择独立知识库目录"))?,
    )?;
    if old.path.starts_with(&dest) || dest.starts_with(&old.path) {
        return Err(fail("独立目录不能与旧知识库相同或嵌套，旧库将保留为备份"));
    }
    exclusive(cat, repo, root, &dest, None)?;
    if dest.exists() && (!dest.is_dir() || !scan(&dest)?.is_empty()) {
        return Err(fail("目标目录已有内容，请选择空目录"));
    }
    let modern = layout::modern(&source);
    let prefixes = [
        Path::new("projects").join(project),
        Path::new("imports/worktrees").join(project),
        wiki::base(project),
        layout::project_dir(&old.path, project),
        if modern {
            PathBuf::from("sources")
        } else {
            PathBuf::from("projects").join(project)
        },
    ];
    let legacy = Path::new("docs/organized").join(format!("{project}.md"));
    let mut files: Tree = source
        .iter()
        .filter(|(p, bytes)| {
            (modern
                && (p.starts_with("wiki")
                    || **p == Path::new("README.md")
                    || **p == Path::new(".ailoom/wiki.json")))
                || prefixes.iter().any(|prefix| p.starts_with(prefix))
                || **p == legacy
                || (wiki::source_owned(p, bytes, project, false) && explicit_owner(bytes, project))
        })
        .map(|(p, v)| (p.clone(), v.clone()))
        .collect();
    files.remove(&Path::new("projects").join(project).join("relations.json"));
    if modern {
        if let Some(bytes) = source.get(Path::new(".ailoom/path-aliases.json")) {
            files.insert(".ailoom/path-aliases.json".into(), bytes.clone());
        }
        if let Some(bytes) = source.get(Path::new(".ailoom/sources.json")) {
            files.insert(".ailoom/sources.json".into(), bytes.clone());
        }
    }
    let unassigned: Vec<_> = source
        .iter()
        .filter(|(p, bytes)| {
            p.extension().is_some_and(|e| e == "md")
                && !files.contains_key(*p)
                && wiki::source_owned(p, bytes, project, true)
        })
        .map(|(p, _)| p.clone())
        .collect();
    let state = layout::project_dir(&old.path, project).join("state.json");
    let checkpoint = data
        .join("knowledge/checkpoints")
        .join(project)
        .join(format!("{repo}.json"));
    let baseline = std::fs::read_to_string(&checkpoint).ok();
    let recovery_required = source
        .get(&state)
        .is_some_and(|bytes| baseline.as_deref() != Some(&crate::ids::sha256_hex(bytes)));
    let expected = crate::ids::sha256_hex(&serde_json::to_vec(
        &json!({"old":old,"destination":dest,"source":digest(&source),"declarations":declarations,"baseline":baseline}),
    )?);
    if !r.execute {
        return Ok(
            json!({"preview":true,"expected":expected,"to":dest,"from":old.path,"files":files.len(),"affected_projects":[repo],"separation":true,"unassigned_sources":unassigned}),
        );
    }
    if r.expected.as_deref() != Some(&expected) {
        return Err(fail("资料或绑定已变化，请重新预览拆分"));
    }
    let next = Binding {
        id: crate::ids::new_id(),
        project_root: root.into(),
        path: dest.clone(),
        remote: None,
        branch: default_branch(),
        subdir: String::new(),
        baseline: BTreeMap::new(),
    };
    for (p, bytes) in &mut files {
        if (p.starts_with(wiki::base(project)) || (modern && p.starts_with("wiki")))
            && p.extension().is_some_and(|e| e == "md")
        {
            let text =
                String::from_utf8(bytes.clone()).map_err(|_| fail("Wiki 页面不是 UTF-8 文本"))?;
            *bytes = text
                .replace(
                    &format!("knowledge_id: {}", old.id),
                    &format!("knowledge_id: {}", next.id),
                )
                .replace(
                    &format!("知识库 ID：`{}`", old.id),
                    &format!("知识库 ID：`{}`", next.id),
                )
                .replace("[全部项目]", "[本仓库知识入口]")
                .into_bytes();
        }
    }
    if modern {
        if let Some(bytes) = source.get(Path::new(".ailoom/wiki.json")) {
            let mut hashes: BTreeMap<PathBuf, String> = serde_json::from_slice(bytes)?;
            for (path, hash) in &mut hashes {
                if source
                    .get(path)
                    .is_some_and(|b| crate::ids::sha256_hex(b) == *hash)
                {
                    if let Some(bytes) = files.get(path) {
                        *hash = crate::ids::sha256_hex(bytes);
                    }
                }
            }
            files.insert(
                ".ailoom/wiki.json".into(),
                serde_json::to_vec_pretty(&hashes)?,
            );
        }
    }
    if let Some(bytes) = files.get_mut(&state) {
        let mut value: Value = serde_json::from_slice(bytes)?;
        value["knowledge_id"] = json!(next.id);
        *bytes = serde_json::to_vec_pretty(&value)?;
    }
    files.insert(
        layout::marker(&source).into(),
        serde_json::to_vec_pretty(&json!({"schema_version":1,"id":next.id,"project_id":project}))?,
    );
    let manifest = json!({"schema_version":1,"team_id":format!("knowledge-{project}"),"projects":{project:{"name":wiki::project_name(root)}},"namespaces":{"known":["common"],"shared":[]}});
    files.insert(
        (if modern {
            ".ailoom/ailoom.toml"
        } else {
            "ailoom.toml"
        })
        .into(),
        toml::to_string_pretty(&manifest)?.into_bytes(),
    );
    let projected = Catalog {
        schema_version: 1,
        repository_path: cat.repository_path.clone(),
        repositories: cat.repositories.clone(),
        projects: BTreeMap::from([(repo.into(), next.clone())]),
    };
    files.extend(wiki::catalog(&files, &projected, &next)?);
    let record = data
        .join("knowledge/separations")
        .join(format!("{}.json", next.id));
    crate::sync_common::atomic_write(
        &record,
        &serde_json::to_vec_pretty(
            &json!({"schema_version":1,"state":"copying","old":old,"next":next,"project_id":project,"declaration":old_declaration,"source_digest":digest(&source)}),
        )?,
    )?;
    write_tree(&dest, &files)?;
    if scan(&dest)? != files || checked_tree(&old)? != source {
        return Err(fail(
            "复制核对失败或旧库已变化；旧绑定保持有效，复制目录保留供检查",
        ));
    }
    rebuild::remap_feedback(data, &old)?;
    copy_feedback(data, &old, &next, &files)?;
    super::super::state::relocate_project(data, &old, &next, project)?;
    // Update existing declarations of this repository's worktrees, retaining rollback data.
    for (path, previous) in &declarations {
        let mut declaration = previous.clone();
        declaration.knowledge_id = next.id.clone();
        declaration.source = super::super::portable::Source::Directory;
        if let Err(error) = super::super::portable::write(path, &declaration) {
            for (path, previous) in &declarations {
                let _ = super::super::portable::write(path, previous);
            }
            let _ = super::super::state::relocate_project(data, &next, &old, project);
            return Err(error);
        }
    }
    cat.projects.insert(repo.into(), next.clone());
    if let Err(error) = persist(data, cat) {
        cat.projects.insert(repo.into(), old.clone());
        for (path, previous) in &declarations {
            let _ = super::super::portable::write(path, previous);
        }
        let _ = super::super::state::relocate_project(data, &next, &old, project);
        return Err(error);
    }
    let mut warnings = Vec::new();
    if let Some(bytes) = files.get(&state).filter(|_| !recovery_required) {
        if crate::sync_common::atomic_write(&checkpoint, crate::ids::sha256_hex(bytes).as_bytes())
            .is_err()
        {
            warnings.push("拆分已完成，恢复配置基线未更新；旧库仍保留".to_string());
        }
    }
    if recovery_required {
        warnings.push("原恢复配置尚未应用到本机；请对新目录运行 knowledge --action recover 后再保存恢复配置。当前副本已保留，未将空配置标记为可覆盖基线。".into());
    }
    if crate::sync_common::atomic_write(&record,&serde_json::to_vec_pretty(&json!({"schema_version":1,"state":"switched","old":old,"next":next,"project_id":project}))?).is_err() {warnings.push("拆分已完成，迁移记录收尾失败".into());}
    Ok(
        json!({"separated":true,"location":next,"project_id":project,"backup":old.path,"affected_projects":[repo],"unassigned_sources":unassigned,"recovery_required":recovery_required,"warnings":warnings}),
    )
}

fn copy_feedback(data: &Path, old: &Binding, next: &Binding, files: &Tree) -> Result<()> {
    let from = data.join("knowledge/state").join(&old.id);
    let to = data.join("knowledge/state").join(&next.id);
    let owned = |id: &str| files.contains_key(Path::new(id));
    if let Ok(bytes) = std::fs::read(from.join("archived-learnings.json")) {
        let mut ids: Vec<String> = serde_json::from_slice(&bytes)?;
        ids.retain(|id| owned(id));
        crate::sync_common::atomic_write(
            &to.join("archived-learnings.json"),
            &serde_json::to_vec_pretty(&ids)?,
        )?;
    }
    if let Ok(bytes) = std::fs::read(from.join("knowledge-usage.json")) {
        let mut usage: super::super::feedback::UsageRecord = serde_json::from_slice(&bytes)?;
        usage.usage.retain(|id, _| owned(id));
        usage.feedback.retain(|id, _| owned(id));
        usage
            .feedback_events
            .retain(|_, v| v.rsplit_once(':').is_some_and(|(id, _)| owned(id)));
        crate::sync_common::atomic_write(
            &to.join("knowledge-usage.json"),
            &serde_json::to_vec_pretty(&usage)?,
        )?;
    }
    Ok(())
}
