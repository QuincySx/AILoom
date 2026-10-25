//! Reset generated Wiki and put a legacy library into the readable directory layout.
use super::*;

pub(super) fn run(
    data: &Path,
    request: &Request,
    cat: &Catalog,
    repo: &str,
    root: &Path,
) -> Result<Value> {
    let b = binding(cat, repo)?;
    let source = checked_tree(b)?;
    if root.starts_with(&b.path) {
        return Err(fail(
            "知识库目录包含项目代码，请先将知识库迁移到独立目录，再重建",
        ));
    }
    let declaration = super::super::portable::read(root)?.ok_or_else(|| fail("项目声明缺失"))?;
    let project = &declaration.project_id;
    if !wiki::single_owner(cat, b, &source, project) {
        return Err(fail("请先 separate 拆分旧共享库，再重建 Wiki"));
    }
    separate::validate_owner(&source, project)?;
    let expected = crate::ids::sha256_hex(&serde_json::to_vec(
        &json!({"binding":b,"source":digest(&source)}),
    )?);
    let parent = b
        .path
        .parent()
        .ok_or_else(|| fail("知识库不能位于文件系统根目录"))?;
    let name = b.path.file_name().unwrap().to_string_lossy();
    let backup = parent.join(format!(".{name}-before-rebuild-{}", &expected[..12]));
    if !request.execute {
        return Ok(
            json!({"preview":true,"expected":expected,"path":b.path,"backup":backup,"message":"清空旧生成 Wiki；原文和恢复配置保留，首页与模块正文使用普通 Markdown。"}),
        );
    }
    if request.expected.as_deref() != Some(&expected) {
        return Err(fail("知识库已变化，请重新预览"));
    }
    if backup.exists() {
        return Err(fail("重建备份位置已存在，请先核对上一轮操作"));
    }
    let staging = parent.join(format!(".{name}-rebuild-{}", crate::ids::new_id()));
    let old_project = layout::project_dir(&b.path, project);
    let mut next = Tree::new();
    let mut moved = BTreeMap::<PathBuf, PathBuf>::new();
    for (path, bytes) in &source {
        if path == layout::marker(&source)
            || wiki::navigation(&source, path)
            || path.starts_with("wiki")
            || path.starts_with("docs/organized")
            || path == Path::new(".ailoom/wiki.json")
            || path == Path::new(".ailoom/homepage.json")
        {
            continue;
        }
        let target = if let Ok(rel) = path.strip_prefix(&old_project) {
            if rel.starts_with("learnings") {
                Path::new("sources/notes").join(rel.strip_prefix("learnings")?)
            } else {
                Path::new(".ailoom/project").join(rel)
            }
        } else if let Ok(rel) = path.strip_prefix(Path::new(ingest::PREFIX).join(project)) {
            if rel == Path::new("index.md") {
                continue;
            }
            if rel == Path::new("manifest.json") {
                PathBuf::from(".ailoom/sources.json")
            } else {
                Path::new("sources").join(rel)
            }
        } else if path == Path::new("ailoom.toml") {
            PathBuf::from(".ailoom/ailoom.toml")
        } else if path.starts_with("sources")
            || path.starts_with(".ailoom")
            || [
                ".gitignore",
                ".gitattributes",
                "LICENSE",
                "LICENSE.md",
                ".editorconfig",
            ]
            .contains(&path.to_string_lossy().as_ref())
        {
            path.clone()
        } else if path.extension().is_some_and(|e| e == "md")
            && wiki::source_owned(path, bytes, project, true)
        {
            Path::new("sources/notes").join(path)
        } else {
            Path::new(".ailoom/legacy").join(path)
        };
        if next.insert(target.clone(), bytes.clone()).is_some() {
            return Err(fail("重建后的文档路径冲突，原目录未变更"));
        }
        moved.insert(path.clone(), target);
    }
    next.insert(
        layout::HIDDEN_MARKER.into(),
        serde_json::to_vec_pretty(&json!({"schema_version":1,"id":b.id,"project_id":project}))?,
    );
    let mut aliases: BTreeMap<PathBuf, PathBuf> = source
        .get(Path::new(".ailoom/path-aliases.json"))
        .map(|b| serde_json::from_slice(b))
        .transpose()?
        .unwrap_or_default();
    for (old, new) in &moved {
        if old != new {
            aliases.insert(old.clone(), new.clone());
        }
    }
    next.insert(
        ".ailoom/path-aliases.json".into(),
        serde_json::to_vec_pretty(&aliases)?,
    );
    let state = PathBuf::from(".ailoom/project/state.json");
    if let Some(bytes) = next.get_mut(&state) {
        let mut value: Value = serde_json::from_slice(bytes)?;
        if let Some(sources) = value["sources"].as_object_mut() {
            for rel in sources.values_mut() {
                if let Some(old) = rel.as_str() {
                    if let Ok(tail) = Path::new(old).strip_prefix(&old_project) {
                        *rel = json!(Path::new(".ailoom/project").join(tail));
                    }
                }
            }
        }
        *bytes = serde_json::to_vec_pretty(&value)?;
    }
    // Catalog uses the staged marker to choose the readable layout.
    write_tree(&staging, &next)?;
    std::fs::create_dir_all(staging.join("wiki"))?;
    std::fs::create_dir_all(staging.join("sources/notes"))?;
    let mut staged = b.clone();
    staged.path = staging.clone();
    next.extend(wiki::catalog(&next, cat, &staged)?);
    write_tree(&staging, &next)?;
    if scan(&staging)? != next || checked_tree(b)? != source {
        return Err(fail("复制核对失败或原知识库已变化，尚未切换"));
    }
    let record = data
        .join("knowledge/rebuilds")
        .join(format!("{}.json", crate::ids::new_id()));
    crate::sync_common::atomic_write(
        &record,
        &serde_json::to_vec_pretty(
            &json!({"state":"prepared","path":b.path,"backup":backup,"staging":staging}),
        )?,
    )?;
    std::fs::rename(&b.path, &backup)?;
    if let Err(e) = std::fs::rename(&staging, &b.path) {
        std::fs::rename(&backup, &b.path)?;
        return Err(e.into());
    }
    let switched = (|| {
        if backup.join(".git").exists() {
            std::fs::rename(backup.join(".git"), b.path.join(".git"))?;
        }
        let mut old = b.clone();
        old.path = b.path.join(&old_project);
        let mut new = b.clone();
        new.path = b.path.join(".ailoom/project");
        super::super::state::relocate(data, &old, &new)?;
        Ok(())
    })();
    if let Err(e) = switched {
        if b.path.join(".git").exists() {
            std::fs::rename(b.path.join(".git"), backup.join(".git"))?;
        }
        std::fs::rename(&b.path, &staging)?;
        std::fs::rename(&backup, &b.path)?;
        return Err(e);
    }
    let checkpoint = data
        .join("knowledge/checkpoints")
        .join(project)
        .join(format!("{repo}.json"));
    let old_state = source.get(&old_project.join("state.json"));
    let current_base = std::fs::read_to_string(&checkpoint).ok();
    let mut warnings = Vec::new();
    if let Some(bytes) = next.get(&state) {
        if old_state
            .is_some_and(|old| current_base.as_deref() == Some(&crate::ids::sha256_hex(old)))
        {
            if crate::sync_common::atomic_write(
                &checkpoint,
                crate::ids::sha256_hex(bytes).as_bytes(),
            )
            .is_err()
            {
                warnings.push("重建完成，恢复配置基线待核对".to_string());
            }
        } else {
            warnings.push("原恢复配置尚未应用到本机，请先 recover 再 checkpoint".into());
        }
    }
    if crate::sync_common::atomic_write(
        &record,
        &serde_json::to_vec_pretty(&json!({"state":"switched","path":b.path,"backup":backup}))?,
    )
    .is_err()
    {
        warnings.push("重建完成，操作记录收尾失败".into());
    }
    Ok(
        json!({"reset":true,"path":b.path,"index":b.path.join("README.md"),"backup":backup,"knowledge_id":b.id,"project_id":project,"preserved_files":moved.len(),"warnings":warnings}),
    )
}

/// Apply path changes lazily under the same locks as feedback. The alias record
/// switches atomically with the directory, so crashes cannot revive archived notes.
pub(super) fn remap_feedback(data: &Path, b: &Binding) -> Result<()> {
    let path = b.path.join(".ailoom/path-aliases.json");
    if !path.is_file() {
        return Ok(());
    }
    let meta = path.symlink_metadata()?;
    if meta.file_type().is_symlink()
        || b.path
            .join(".ailoom")
            .symlink_metadata()?
            .file_type()
            .is_symlink()
        || meta.len() > 8 * 1024 * 1024
    {
        return Err(fail("知识路径映射无效"));
    }
    let bytes = std::fs::read(path)?;
    let digest = crate::ids::sha256_hex(&bytes);
    let applied = data
        .join("knowledge/state")
        .join(&b.id)
        .join("path-aliases-applied");
    if std::fs::read_to_string(&applied).ok().as_deref() == Some(&digest) {
        return Ok(());
    }
    let aliases: BTreeMap<String, String> = serde_json::from_slice(&bytes)?;
    let state = data.join("knowledge/state").join(&b.id);
    let archive = state.join("archived-learnings.json");
    if archive.is_file() {
        let old = std::fs::read(&archive)?;
        let mut ids: Vec<String> = serde_json::from_slice(&old)?;
        for id in &mut ids {
            if let Some(new) = aliases.get(id) {
                *id = new.clone();
            }
        }
        ids.sort();
        ids.dedup();
        let next = serde_json::to_vec_pretty(&ids)?;
        if old != next {
            crate::sync_common::atomic_write(&archive, &next)?;
        }
    }
    crate::source::with_file_lock(&state.join("knowledge-usage.lock"), || {
        let path = state.join("knowledge-usage.json");
        if !path.is_file() {
            return Ok(());
        }
        let old = std::fs::read(&path)?;
        let mut usage: super::super::feedback::UsageRecord = serde_json::from_slice(&old)?;
        for (old, new) in &aliases {
            if old == new {
                continue;
            }
            if let Some(n) = usage.usage.remove(old) {
                let entry = usage.usage.entry(new.clone()).or_default();
                *entry = entry.saturating_add(n);
            }
            if let Some((good, bad)) = usage.feedback.remove(old) {
                let entry = usage.feedback.entry(new.clone()).or_default();
                entry.0 = entry.0.saturating_add(good);
                entry.1 = entry.1.saturating_add(bad);
            }
        }
        for value in usage.feedback_events.values_mut() {
            if let Some((id, direction)) = value.rsplit_once(':') {
                if let Some(new) = aliases.get(id) {
                    *value = format!("{new}:{direction}");
                }
            }
        }
        let next = serde_json::to_vec_pretty(&usage)?;
        if old != next {
            crate::sync_common::atomic_write(&path, &next)?;
        }
        Ok(())
    })?;
    crate::sync_common::atomic_write(&applied, digest.as_bytes())
}
