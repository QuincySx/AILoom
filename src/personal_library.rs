//! 个人资源库（AIL-043）：无需团队/远端即可本地使用的资源源。
//! 复用 source manifest（ailoom.toml + resources/ 布局），首次自动生成；
//! 从用户指定目录导入 skill 时预览→复制，原目录只读、不执行任何脚本。

use crate::error::{code, Error, Result};
use crate::ids::now_iso;
use crate::manifest::{valid_name, TeamManifest, MANIFEST_FILE};
use crate::resource::{enumerate, parse_frontmatter, ResourceKind};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const LIBRARY_TEAM_ID: &str = "personal";
pub const LIBRARY_NAMESPACE: &str = "personal";

pub fn library_root(data_root: &Path) -> PathBuf {
    data_root.join("library")
}

fn manifest_text() -> String {
    format!(
        "# AILoom 个人资源库（自动生成，可手工扩展）\n\
         schema_version = 1\n\
         team_id = \"{LIBRARY_TEAM_ID}\"\n\
         [namespaces]\n\
         known = [\"{LIBRARY_NAMESPACE}\"]\n\
         shared = [\"{LIBRARY_NAMESPACE}\"]\n"
    )
}

#[derive(Debug, Serialize)]
pub struct LibraryInit {
    pub path: PathBuf,
    pub created: bool,
}

/// 确保个人库存在（幂等、离线、零输入）：生成合法 manifest + resources 目录骨架，
/// 并登记到 profile（仓外）。已存在的合法库不改动。
pub fn ensure_library(data_root: &Path) -> Result<LibraryInit> {
    let root = library_root(data_root);
    let manifest_file = root.join(MANIFEST_FILE);
    let created = !manifest_file.is_file();
    if created {
        std::fs::create_dir_all(&root)?;
        crate::sync_common::atomic_write(&manifest_file, manifest_text().as_bytes())?;
        for dir in [
            "skills",
            "rules",
            "docs",
            "agents",
            "mcp",
            "learnings",
            "env",
            "hooks",
            "packages",
        ] {
            std::fs::create_dir_all(root.join("resources").join(dir))?;
        }
        // 生成即自校验：manifest 必须能被真实加载器解析
        TeamManifest::load_from(&root)?;
    } else {
        // 已有库必须仍是合法源（避免静默坏库）
        TeamManifest::load_from(&root)?;
    }
    // 登记（覆盖为当前路径；幂等）
    let mut profile = crate::profile::PersonalProfile::load_or_default(data_root)?;
    let changed = match &profile.library {
        Some(l) => l.path != root,
        None => true,
    };
    if changed {
        profile.library = Some(crate::profile::LibraryRef { path: root.clone() });
        profile.save(data_root)?;
    }
    Ok(LibraryInit {
        path: root,
        created,
    })
}

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub source_dir: String,
    pub skill_name: String,
    /// 源 SKILL.md frontmatter 里的原始 name（与 skill_name 不同 = 需要改名对齐）
    pub frontmatter_name: Option<String>,
    pub description: String,
    pub files: Vec<String>,
    pub scripts: Vec<String>,
    /// 与现有库的同名资源冲突（默认拒绝，除非 --force 语义由调用方决定）
    pub conflicts: Vec<String>,
    /// 导入时将自动补充的元数据说明
    pub metadata_to_add: Vec<String>,
    pub target_dir: String,
}

fn scan_files(
    dir: &Path,
    base: &Path,
    out: &mut Vec<String>,
    scripts: &mut Vec<String>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let p = entry.path();
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            return Err(Error::new(
                code::PATH_TRAVERSAL,
                format!("源目录含符号链接，拒绝导入: {}", p.display()),
            ));
        }
        let rel = p
            .strip_prefix(base)
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_default();
        if ft.is_dir() {
            scan_files(&p, base, out, scripts)?;
        } else {
            out.push(rel.clone());
            let name = entry.file_name().to_string_lossy().to_string();
            let parent = p
                .parent()
                .and_then(|x| x.file_name())
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_default();
            if parent == "scripts" || name.ends_with(".sh") || name.ends_with(".py") {
                scripts.push(rel);
            }
        }
    }
    out.sort();
    Ok(())
}

/// 校验 Markdown 相对链接不逃逸技能目录（绝对路径与 `..` 拒绝；外链 URL 允许）。
fn check_link_boundaries(skill_md_text: &str) -> Result<()> {
    for cand in markdown_link_targets(skill_md_text) {
        if cand.starts_with("http://")
            || cand.starts_with("https://")
            || cand.starts_with('#')
            || cand.starts_with("mailto:")
        {
            continue;
        }
        let path = Path::new(&cand);
        let path = path.strip_prefix("./").unwrap_or(path);
        if cand.starts_with('/')
            || cand.starts_with('~')
            || path.components().any(|c| c.as_os_str() == "..")
        {
            return Err(Error::new(
                code::PATH_TRAVERSAL,
                format!("SKILL.md 链接目标逃逸技能目录，拒绝导入: {cand}"),
            ));
        }
    }
    Ok(())
}

fn markdown_link_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find(')') {
            out.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out
}

/// 导入预览：只读源目录，不复制、不执行。
pub fn import_preview(
    data_root: &Path,
    source_dir: &Path,
    name_override: Option<&str>,
) -> Result<ImportPreview> {
    let source_dir = source_dir
        .canonicalize()
        .map_err(|e| Error::new(code::WORKSPACE_INVALID, format!("源目录不可用: {e}")))?;
    let skill_md = source_dir.join("SKILL.md");
    if !skill_md.is_file() {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!(
                "源目录缺少 SKILL.md，不是可导入的技能目录: {}",
                source_dir.display()
            ),
        ));
    }
    let raw = std::fs::read_to_string(&skill_md)?;
    let (meta, _) = parse_frontmatter(&raw)?;
    let dir_name = source_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let fm_name = meta.as_ref().and_then(|m| m.name.clone());
    let name = name_override
        .map(str::to_string)
        .or_else(|| fm_name.clone())
        .unwrap_or_else(|| dir_name.clone());
    if !valid_name(&name) {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("技能名非法（需 [a-z0-9][a-z0-9._-]{{0,63}}）: {name}"),
        ));
    }
    check_link_boundaries(&raw)?;
    let mut files = Vec::new();
    let mut scripts = Vec::new();
    scan_files(&source_dir, &source_dir, &mut files, &mut scripts)?;
    // 冲突检查：库内同名（skill 身份 + 目标目录）
    let lib = ensure_library(data_root)?;
    let target = lib.path.join("resources").join("skills").join(&name);
    let mut conflicts = Vec::new();
    if target.exists() {
        conflicts.push(format!(
            "personal/skill/{LIBRARY_NAMESPACE}/{name}（库内已存在同名目录）"
        ));
    }
    let mut metadata_to_add = Vec::new();
    let meta_ref = meta.as_ref();
    if meta_ref.and_then(|m| m.name.clone()).is_none() {
        metadata_to_add.push("frontmatter.name".into());
    }
    if meta_ref.and_then(|m| m.namespace.clone()).is_none() {
        metadata_to_add.push(format!("frontmatter.namespace = {LIBRARY_NAMESPACE}"));
    }
    if meta_ref.and_then(|m| m.shared).is_none() {
        metadata_to_add.push("frontmatter.shared = true".into());
    }
    Ok(ImportPreview {
        source_dir: source_dir.display().to_string(),
        skill_name: name.clone(),
        frontmatter_name: fm_name,
        description: meta_ref
            .and_then(|m| m.description.clone())
            .unwrap_or_default(),
        files,
        scripts,
        conflicts,
        metadata_to_add,
        target_dir: target.display().to_string(),
    })
}

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub skill_id: String,
    pub target_dir: PathBuf,
    pub files_copied: usize,
    pub scripts: Vec<String>,
    /// 脚本只是复制，从未执行；声明在报告中避免误会
    pub scripts_executed: bool,
    pub note: String,
    /// 源信息（更新 diff 用）
    pub source_dir: String,
    /// 冲突时各文件的差异摘要（库内已有 vs 源）
    pub changed_files: Vec<String>,
}

/// 导入执行：复制进个人库并补齐元数据；原目录只读；脚本从不执行。
pub fn import_execute(
    data_root: &Path,
    source_dir: &Path,
    name_override: Option<&str>,
) -> Result<ImportReport> {
    let preview = import_preview(data_root, source_dir, name_override)?;
    let src = source_dir
        .canonicalize()
        .unwrap_or_else(|_| source_dir.to_path_buf());
    // 冲突时计算文件级差异摘要（供“更新有 diff”的预览；不自动覆盖）
    let mut changed_files = Vec::new();
    if !preview.conflicts.is_empty() {
        let existing = PathBuf::from(preview.target_dir.clone());
        for rel in &preview.files {
            let src_hash = std::fs::read(src.join(rel)).map(|b| sha256_of(&b));
            let dst_hash = std::fs::read(existing.join(rel)).map(|b| sha256_of(&b));
            match (src_hash, dst_hash) {
                (Ok(a), Ok(b)) if a != b => changed_files.push(rel.clone()),
                (Ok(_), Err(_)) => {}
                (Err(_), Ok(_)) => changed_files.push(format!("{rel}（库内多出）")),
                _ => {}
            }
        }
        return Err(Error::new(
            code::SOURCE_CONFLICT,
            format!(
                "导入冲突：{}；与库内差异文件: {:?}",
                preview.conflicts.join("; "),
                changed_files
            ),
        )
        .fix("换一个 --name 另存为新副本；或先删除库内同名技能再导入（更新语义）"));
    }
    ensure_library(data_root)?;
    let target = PathBuf::from(preview.target_dir.clone());
    std::fs::create_dir_all(&target)?;
    let mut copied = 0usize;
    for rel in &preview.files {
        let from = src.join(rel);
        let to = target.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&from, &to)?;
        copied += 1;
    }
    // 补齐 frontmatter 元数据（保留既有字段与正文；改名时对齐 name 行）
    let skill_md = target.join("SKILL.md");
    let raw = std::fs::read_to_string(&skill_md)?;
    let rename_needed = preview
        .frontmatter_name
        .as_ref()
        .map(|n| n != &preview.skill_name)
        .unwrap_or(false);
    let mut meta_needed = preview.metadata_to_add.clone();
    if rename_needed {
        meta_needed.push("frontmatter.rename".into());
    }
    let fixed = add_frontmatter_meta(&raw, &preview.skill_name, &meta_needed)?;
    if fixed != raw {
        crate::sync_common::atomic_write(&skill_md, fixed.as_bytes())?;
    }
    // 用真实资源解析器校验导入结果（资源身份/归属/namespace 全链路）
    let lib = library_root(data_root);
    let manifest = TeamManifest::load_from(&lib)?;
    let entries = enumerate(&lib, &manifest, LIBRARY_TEAM_ID)?;
    let expected = format!(
        "{LIBRARY_TEAM_ID}/skill/{LIBRARY_NAMESPACE}/{}",
        preview.skill_name
    );
    if !entries.iter().any(|e| e.id.to_string() == expected) {
        return Err(Error::new(
            code::OWNERLESS_RESOURCE,
            format!("导入后资源校验失败：未解析到 {expected}"),
        ));
    }
    // 记录来源元数据（版本/摘要），供后续更新对比
    let meta = serde_json::json!({
        "source_dir": src.display().to_string(),
        "imported_at": now_iso(),
        "files": preview
            .files
            .iter()
            .map(|f| {
                let digest = std::fs::read(target.join(f)).ok().map(|b| sha256_of(&b));
                (f.clone(), digest)
            })
            .collect::<std::collections::BTreeMap<_, _>>(),
    });
    let _ = crate::sync_common::atomic_write(
        &target.join(".ailoom-import.json"),
        serde_json::to_vec_pretty(&meta)
            .unwrap_or_default()
            .as_slice(),
    );
    Ok(ImportReport {
        skill_id: expected,
        target_dir: target,
        files_copied: copied,
        scripts: preview.scripts,
        scripts_executed: false,
        source_dir: src.display().to_string(),
        changed_files,
        note: format!(
            "导入时间 {now}；脚本仅复制未执行；源目录未修改",
            now = now_iso()
        ),
    })
}

/// 按需要给 SKILL.md 补 frontmatter 字段；已有 frontmatter 时只补缺失键。
/// 最终 `name` 必须等于库内目录名（资源解析要求一致），不同则改写该行。
fn add_frontmatter_meta(raw: &str, name: &str, to_add: &[String]) -> Result<String> {
    if to_add.is_empty() {
        return Ok(raw.to_string());
    }
    let needs_name = to_add.iter().any(|x| x == "frontmatter.name");
    let needs_rename = to_add.iter().any(|x| x == "frontmatter.rename");
    let mut extras = String::new();
    if needs_name {
        extras.push_str(&format!("name: {name}\n"));
    }
    if to_add.iter().any(|x| x.contains("namespace")) {
        extras.push_str(&format!("namespace: {LIBRARY_NAMESPACE}\n"));
    }
    if to_add.iter().any(|x| x == "frontmatter.shared = true") {
        extras.push_str("shared: true\n");
    }
    if let Some(rest) = raw.strip_prefix("---\n") {
        let end = rest.find("\n---").ok_or_else(|| {
            Error::new(code::MANIFEST_MISSING_FIELD, "SKILL.md frontmatter 未闭合")
        })?;
        let yaml = &rest[..end];
        let body = &rest[end..];
        // 既有 name 行与目标名不一致时改写（导入重命名），其余行原样保留
        let yaml =
            if !needs_name && (needs_rename || yaml.contains("name:")) && yaml.contains("name:") {
                let mut replaced = false;
                let lines: Vec<String> = yaml
                    .lines()
                    .map(|l| {
                        if !replaced && l.trim_start().starts_with("name:") {
                            replaced = true;
                            format!("name: {name}")
                        } else {
                            l.to_string()
                        }
                    })
                    .collect();
                if !replaced {
                    // name 在多行结构里（无法安全就地改）→ 追加一行保持一致
                    lines
                        .into_iter()
                        .chain(std::iter::once(format!("name: {name}")))
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    lines.join("\n")
                }
            } else {
                yaml.to_string()
            };
        Ok(format!("---\n{yaml}{extras}{body}"))
    } else {
        Ok(format!("---\n{extras}---\n\n{raw}"))
    }
}

fn sha256_of(b: &[u8]) -> String {
    crate::ids::sha256_hex(b)
}

/// 删除库内资源前的影响预览：列出个人配置中启用该资源的作用域。
pub fn delete_preview(data_root: &Path, resource_id: &str) -> Result<serde_json::Value> {
    let profile = crate::profile::PersonalProfile::load_or_default(data_root)?;
    let mut affected = Vec::new();
    for (repo_id, repo) in &profile.repos {
        let mut scan = |sel: Option<&crate::profile::ScopeSelection>, scope: String| {
            if let Some(s) = sel {
                if s.resources
                    .get(resource_id)
                    .map(|tr| *tr != crate::profile::TriState::Inherit)
                    .unwrap_or(false)
                {
                    affected.push(format!("{repo_id}/{scope}"));
                }
            }
        };
        scan(repo.default.as_ref(), "default".into());
        for sp in &repo.subprojects {
            scan(Some(&sp.selection), format!("subproject:{}", sp.path));
        }
        for (wt, sel) in &repo.worktrees {
            scan(Some(sel), format!("worktree:{wt}"));
        }
        for (wt, sps) in &repo.wt_subprojects {
            for sp in sps {
                scan(Some(&sp.selection), format!("wt:{wt}/{}", sp.path));
            }
        }
    }
    let lib = library_root(data_root);
    let manifest = TeamManifest::load_from(&lib)?;
    let entries = enumerate(&lib, &manifest, LIBRARY_TEAM_ID)?;
    let exists = entries.iter().any(|e| e.id.to_string() == resource_id);
    Ok(serde_json::json!({
        "resource_id": resource_id,
        "exists": exists,
        "affected_scopes": affected,
        "note": if exists { "确认后删除库内副本；已启用它的作用域需改为 inherit/禁用或改绑其他资源" } else { "资源不存在" },
    }))
}

/// 删除库内资源（目录级；仅个人库自有内容）。
pub fn delete_execute(data_root: &Path, resource_id: &str) -> Result<()> {
    let lib = library_root(data_root);
    let manifest = TeamManifest::load_from(&lib)?;
    let entries = enumerate(&lib, &manifest, LIBRARY_TEAM_ID)?;
    let entry = entries
        .iter()
        .find(|e| e.id.to_string() == resource_id)
        .ok_or_else(|| {
            Error::new(
                code::UNKNOWN_REFERENCE,
                format!("资源不存在: {resource_id}"),
            )
        })?;
    let full = lib
        .join(&entry.path)
        .canonicalize()
        .unwrap_or_else(|_| lib.join(&entry.path));
    if full.is_dir() {
        std::fs::remove_dir_all(&full)?;
    } else if full.is_file() {
        std::fs::remove_file(&full)?;
        if let Some(p) = full.parent() {
            let _ = std::fs::remove_dir(p);
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct LibraryListing {
    pub path: PathBuf,
    pub skills: Vec<String>,
}

/// 列出个人库内的技能（经真实解析器）。
pub fn list(data_root: &Path) -> Result<LibraryListing> {
    let lib = library_root(data_root);
    if !lib.is_dir() {
        return Ok(LibraryListing {
            path: lib,
            skills: Vec::new(),
        });
    }
    let manifest = TeamManifest::load_from(&lib)?;
    let entries = enumerate(&lib, &manifest, LIBRARY_TEAM_ID)?;
    let skills = entries
        .iter()
        .filter(|e| e.id.kind == ResourceKind::Skill)
        .map(|e| e.id.name.clone())
        .collect();
    Ok(LibraryListing { path: lib, skills })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(dir: &Path, name: &str, with_meta: bool) {
        let d = dir.join(name);
        std::fs::create_dir_all(d.join("references")).unwrap();
        std::fs::create_dir_all(d.join("scripts")).unwrap();
        let fm = if with_meta {
            format!("---\nname: {name}\ndescription: 测试技能\nshared: true\nnamespace: {LIBRARY_NAMESPACE}\n---\n\n# {name}\n\n[清单](references/list.md)\n[官网](https://example.com)\n")
        } else {
            format!(
                "# {name}\n\n这是一个没有任何 frontmatter 的技能。\n\n[清单](references/list.md)\n"
            )
        };
        std::fs::write(d.join("SKILL.md"), fm).unwrap();
        std::fs::write(d.join("references/list.md"), "- a\n").unwrap();
        std::fs::write(d.join("scripts/run.sh"), "echo hello\n").unwrap();
    }

    #[test]
    fn ensure_library_is_idempotent_and_offline() {
        let data = tempfile::tempdir().unwrap();
        let a = ensure_library(data.path()).unwrap();
        assert!(a.created);
        assert!(a.path.join(MANIFEST_FILE).is_file());
        let b = ensure_library(data.path()).unwrap();
        assert!(!b.created, "幂等");
        // 登记进 profile
        let p = crate::profile::PersonalProfile::load_or_default(data.path()).unwrap();
        assert_eq!(p.library.as_ref().unwrap().path, a.path);
    }

    #[test]
    fn import_without_frontmatter_gets_metadata_and_validates() {
        let data = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        make_skill(src.path(), "my-flow", false);
        let preview = import_preview(data.path(), &src.path().join("my-flow"), None).unwrap();
        assert_eq!(preview.skill_name, "my-flow");
        assert!(preview
            .metadata_to_add
            .contains(&format!("frontmatter.namespace = {LIBRARY_NAMESPACE}")));
        assert!(preview.scripts.contains(&"scripts/run.sh".to_string()));
        let report = import_execute(data.path(), &src.path().join("my-flow"), None).unwrap();
        assert_eq!(
            report.skill_id,
            format!("{LIBRARY_TEAM_ID}/skill/{LIBRARY_NAMESPACE}/my-flow")
        );
        assert!(!report.scripts_executed);
        // 原目录逐字节未动（frontmatter 只补在副本上）
        let orig = std::fs::read_to_string(src.path().join("my-flow/SKILL.md")).unwrap();
        assert!(!orig.contains("shared:"), "源目录 SKILL.md 未被改写");
        let copy = std::fs::read_to_string(report.target_dir.join("SKILL.md")).unwrap();
        assert!(copy.contains("shared: true"));
        let listing = list(data.path()).unwrap();
        assert_eq!(listing.skills, vec!["my-flow"]);
        // 同名再导入 → 冲突拒绝
        let err = import_execute(data.path(), &src.path().join("my-flow"), None).unwrap_err();
        assert_eq!(err.code, code::SOURCE_CONFLICT);
    }

    #[test]
    fn escaping_link_and_symlink_rejected() {
        let data = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let d = src.path().join("bad-links");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), "# x\n\n[逃逸](../../../etc/passwd)\n").unwrap();
        let err = import_preview(data.path(), &d, None).unwrap_err();
        assert_eq!(err.code, code::PATH_TRAVERSAL, "`..` 链接拒绝");

        let d2 = src.path().join("bad-symlink");
        std::fs::create_dir_all(&d2).unwrap();
        std::fs::write(d2.join("SKILL.md"), "# x\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", d2.join("references")).unwrap();
        let err2 = import_preview(data.path(), &d2, None).unwrap_err();
        assert_eq!(err2.code, code::PATH_TRAVERSAL, "符号链接拒绝");
    }

    #[test]
    fn name_override_and_invalid_name() {
        let data = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        make_skill(src.path(), "orig-name", true);
        let report =
            import_execute(data.path(), &src.path().join("orig-name"), Some("renamed")).unwrap();
        assert_eq!(
            report.skill_id,
            format!("{LIBRARY_TEAM_ID}/skill/{LIBRARY_NAMESPACE}/renamed")
        );
        let d = src.path().join("Bad Name");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), "# x\n").unwrap();
        let err = import_preview(data.path(), &d, None).unwrap_err();
        assert_eq!(err.code, code::MANIFEST_MISSING_FIELD, "非法名拒绝");
    }
}
