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
/// L01（事务性）：先在暂存区完成复制+元数据修正+完整校验，再原子发布到库内；
/// 任一步失败 → 清理暂存，个人库保持原状（不会被半成品锁死）。
pub fn import_execute(
    data_root: &Path,
    source_dir: &Path,
    name_override: Option<&str>,
) -> Result<ImportReport> {
    import_execute_with_source(data_root, source_dir, name_override, None)
}

/// AIL-063：带来源元数据的导入（local/git/provider）；None = 本地路径自动标注。
pub fn import_execute_with_source(
    data_root: &Path,
    source_dir: &Path,
    name_override: Option<&str>,
    source_meta: Option<crate::skill_source::SkillSourceMeta>,
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
        let existing_source =
            crate::skill_source::existing_source_note(Path::new(&preview.target_dir));
        return Err(Error::new(
            code::SOURCE_CONFLICT,
            format!(
                "导入冲突：{}；与库内差异文件: {:?}{}",
                preview.conflicts.join("; "),
                changed_files,
                existing_source
                    .map(|n| format!("；{n}"))
                    .unwrap_or_default()
            ),
        )
        .fix("换一个 --name 另存为新副本；或先删除库内同名技能再导入（更新语义）"));
    }
    ensure_library(data_root)?;
    let lib = library_root(data_root);
    let target = PathBuf::from(preview.target_dir.clone());
    // 暂存区：库内 .staging/<id>（与 resources/ 同文件系统，可原子 rename）
    let staging_root = lib.join(".staging");
    std::fs::create_dir_all(&staging_root)?;
    let staging = staging_root.join(format!("{}-{}", preview.skill_name, crate::ids::new_id()));
    let rollback = |staging: &Path| {
        let _ = std::fs::remove_dir_all(staging);
    };
    let mut copied = 0usize;
    for rel in &preview.files {
        let from = src.join(rel);
        let to = staging.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::copy(&from, &to).is_err() {
            rollback(&staging);
            return Err(Error::new(
                code::WRITE_FAILED,
                format!("复制源文件失败: {rel}"),
            ));
        }
        copied += 1;
    }
    // 补齐 frontmatter 元数据（在暂存副本上；保留既有字段与正文）
    let skill_md = staging.join("SKILL.md");
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
    // 暂存副本必须能被真实解析器完整解析（L01：先验证后发布）
    if let Err(e) = validate_staged_skill(&staging, &preview.skill_name) {
        rollback(&staging);
        return Err(e);
    }
    // 原子发布：同文件系统 rename；目标存在性已在 preview 阶段检查
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Err(e) = std::fs::rename(&staging, &target) {
        rollback(&staging);
        return Err(Error::new(
            code::WRITE_FAILED,
            format!("发布到个人库失败（库保持原状）: {e}"),
        ));
    }
    // 发布后整库校验（资源身份/归属/namespace 全链路）；失败则回滚本次发布
    let manifest = TeamManifest::load_from(&lib)?;
    let entries = match enumerate(&lib, &manifest, LIBRARY_TEAM_ID) {
        Ok(e) => e,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&target);
            return Err(e);
        }
    };
    let expected = format!(
        "{LIBRARY_TEAM_ID}/skill/{LIBRARY_NAMESPACE}/{}",
        preview.skill_name
    );
    if !entries.iter().any(|e| e.id.to_string() == expected) {
        let _ = std::fs::remove_dir_all(&target);
        return Err(Error::new(
            code::OWNERLESS_RESOURCE,
            format!("导入后资源校验失败：未解析到 {expected}（已回滚，库保持原状）"),
        ));
    }
    // 记录导入摘要元数据（版本/文件指纹，供更新对比）
    let imported_digest = skill_dir_digest(&target);
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
        &target.join(crate::skill_source::IMPORT_META_FILE),
        serde_json::to_vec_pretty(&meta)
            .unwrap_or_default()
            .as_slice(),
    );
    // 来源元数据（AIL-063）合并写入（必须在汇总 meta 之后，避免被覆盖）
    let source =
        source_meta.unwrap_or_else(|| crate::skill_source::local_meta(&src, &imported_digest));
    let mut source = source;
    source.imported_digest = imported_digest;
    source.fetched_at = crate::ids::now_iso();
    crate::skill_source::write_meta(&target, &source)?;
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

/// 暂存副本的发布前校验：frontmatter 合法、name 与目录一致、可被枚举器接受。
fn validate_staged_skill(staged: &Path, name: &str) -> Result<()> {
    let text = std::fs::read_to_string(staged.join("SKILL.md"))?;
    let (meta, _) = parse_frontmatter(&text)?;
    let m = meta.ok_or_else(|| {
        Error::new(
            code::MANIFEST_MISSING_FIELD,
            "发布前校验失败：frontmatter 缺失",
        )
    })?;
    if m.name.as_deref() != Some(name) {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!(
                "发布前校验失败：frontmatter name({:?}) 与目录名({name}) 不一致",
                m.name
            ),
        ));
    }
    if m.namespace.as_deref() != Some(LIBRARY_NAMESPACE) {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!(
                "发布前校验失败：frontmatter namespace 应为 {LIBRARY_NAMESPACE}（实际 {:?}）",
                m.namespace
            ),
        ));
    }
    Ok(())
}

/// 按需要给 SKILL.md 补 frontmatter 字段；已有 frontmatter 时只补缺失键。
/// 最终 `name` 必须等于库内目录名（资源解析要求一致），不同则改写该行。
/// L01：注入字段前确保 YAML 段以换行结束，杜绝 `description: …namespace: …` 拼接。
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
    if let Some(rest) = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
    {
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
        // 关键修复：YAML 段与新注入字段之间必须有换行分隔
        let mut yaml = yaml;
        if !yaml.is_empty() && !yaml.ends_with('\n') {
            yaml.push('\n');
        }
        Ok(format!("---\n{yaml}{extras}{body}"))
    } else {
        Ok(format!("---\n{extras}---\n\n{raw}"))
    }
}

fn sha256_of(b: &[u8]) -> String {
    crate::ids::sha256_hex(b)
}

/// 技能目录内容摘要（排除来源元数据文件本身，AIL-063/066 统一口径）。
fn skill_dir_digest(dir: &Path) -> String {
    let mut parts: Vec<(String, String)> = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() == crate::skill_source::IMPORT_META_FILE {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let content = std::fs::read(entry.path()).unwrap_or_default();
        parts.push((rel, sha256_of(&content)));
    }
    parts.sort();
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    for (rel, hash) in &parts {
        hasher.update(rel.as_bytes());
        hasher.update(hash.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
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

// ---------------------------------------------------------------------------
// 宽容列表（AIL-062）：单个坏条目不使整个库列表与修复入口失效
// ---------------------------------------------------------------------------

/// 列表条目（宽容模式；与控制台返回的 JSON 字段一致）。
#[derive(Debug, Serialize)]
pub struct TolerantEntry {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub path: String,
}

/// 坏条目及其定位信息（错误落到具体文件）。
#[derive(Debug, Serialize)]
pub struct LibraryIssue {
    pub path: String,
    pub error: String,
}

/// 逐项扫描个人库：可解析的条目正常返回；坏文件记为 issue（带文件级错误），
/// 不让一个坏条目锁死整个列表/编辑/删除入口。
pub fn list_tolerant(data_root: &Path) -> (Vec<TolerantEntry>, Vec<LibraryIssue>) {
    let lib = library_root(data_root);
    let mut entries = Vec::new();
    let mut issues = Vec::new();
    let manifest = match TeamManifest::load_from(&lib) {
        Ok(m) => m,
        Err(e) => {
            issues.push(LibraryIssue {
                path: lib.join(MANIFEST_FILE).display().to_string(),
                error: format!("清单不可读: {e}"),
            });
            return (entries, issues);
        }
    };
    let paths = manifest.effective_paths();
    // skills：目录含 SKILL.md
    let skills_dir = lib.join(&paths.skills);
    if skills_dir.is_dir() {
        for dir in std::fs::read_dir(&skills_dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = dir.path();
            let fname = dir.file_name().to_string_lossy().to_string();
            if !path.is_dir() || fname.starts_with('.') {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            let rel = path
                .strip_prefix(&lib)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| fname.clone());
            match std::fs::read_to_string(&skill_md)
                .map_err(|e| format!("缺少/不可读 SKILL.md: {e}"))
                .and_then(|raw| {
                    parse_frontmatter(&raw)
                        .map(|(m, _)| (m, raw))
                        .map_err(|e| e.to_string())
                }) {
                Ok((Some(meta), _)) if meta.name.as_deref() == Some(fname.as_str()) => {
                    entries.push(TolerantEntry {
                        id: format!("{LIBRARY_TEAM_ID}/skill/{}/{}", namespace_of(&meta), fname),
                        kind: "skill".into(),
                        name: fname.clone(),
                        namespace: namespace_of(&meta),
                        description: meta.description.unwrap_or_default(),
                        path: rel,
                    });
                }
                Ok((Some(meta), _)) => issues.push(LibraryIssue {
                    path: rel,
                    error: format!("SKILL.md name `{:?}` 与目录名 `{fname}` 不一致", meta.name),
                }),
                Ok((None, _)) => issues.push(LibraryIssue {
                    path: rel,
                    error: "SKILL.md 缺少 frontmatter".into(),
                }),
                Err(e) => issues.push(LibraryIssue {
                    path: rel,
                    error: e,
                }),
            }
        }
    }
    // markdown 类与 toml 类：逐文件捕获错误
    let md_kinds = [
        ("rule", &paths.rules),
        ("doc", &paths.docs),
        ("learning", &paths.learnings),
    ];
    for (kind, dir_path) in md_kinds {
        let base = lib.join(dir_path);
        if !base.is_dir() {
            continue;
        }
        collect_file_entries(&base, &lib, kind, true, &mut entries, &mut issues);
    }
    let toml_kinds = [
        ("agent", &paths.agents),
        ("mcp", &paths.mcp),
        ("env", &paths.env),
        ("hook", &paths.hooks),
        ("package", &paths.packages),
    ];
    for (kind, dir_path) in toml_kinds {
        let base = lib.join(dir_path);
        if !base.is_dir() {
            continue;
        }
        collect_file_entries(&base, &lib, kind, false, &mut entries, &mut issues);
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    (entries, issues)
}

fn namespace_of(meta: &crate::resource::RawMeta) -> String {
    meta.namespace
        .clone()
        .unwrap_or_else(|| LIBRARY_NAMESPACE.to_string())
}

fn collect_file_entries(
    base: &Path,
    lib: &Path,
    kind: &str,
    markdown: bool,
    entries: &mut Vec<TolerantEntry>,
    issues: &mut Vec<LibraryIssue>,
) {
    for file in std::fs::read_dir(base).into_iter().flatten().flatten() {
        let fp = file.path();
        if !fp.is_file() {
            continue;
        }
        let fname = file.file_name().to_string_lossy().to_string();
        if fname.starts_with('.') {
            continue;
        }
        let ext = if markdown { ".md" } else { ".toml" };
        if !fname.ends_with(ext) {
            continue;
        }
        let rel = fp
            .strip_prefix(lib)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| fname.clone());
        let stem = fname.trim_end_matches(ext).to_string();
        let parsed = std::fs::read_to_string(&fp)
            .map_err(|e| format!("不可读: {e}"))
            .and_then(|raw| {
                if markdown {
                    parse_frontmatter(&raw)
                        .map(|(m, _)| m)
                        .map_err(|e| e.to_string())
                } else {
                    toml::from_str::<toml::Value>(&raw)
                        .map(|_| None)
                        .map_err(|e| format!("TOML 解析失败: {e}"))
                }
            });
        match parsed {
            Ok(_) => {
                let namespace = LIBRARY_NAMESPACE.to_string();
                entries.push(TolerantEntry {
                    id: format!("{LIBRARY_TEAM_ID}/{kind}/{namespace}/{stem}"),
                    kind: kind.to_string(),
                    name: stem.clone(),
                    namespace,
                    description: String::new(),
                    path: rel,
                });
            }
            Err(e) => issues.push(LibraryIssue {
                path: rel,
                error: e,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Git/远程来源导入与上游更新（AIL-064/066）
// ---------------------------------------------------------------------------

/// skill git 导入缓存根（与团队源缓存隔离，互不干扰）。
fn skill_git_cache(data_root: &Path, identity: &str) -> PathBuf {
    data_root
        .join("cache")
        .join("skill-import")
        .join(crate::ids::cache_key_from_identity(identity))
}

#[derive(Debug, serde::Serialize)]
pub struct GitImportPreview {
    pub discovery_entry: String,
    pub repo_url: String,
    pub repo_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    pub skill_name: String,
    pub files: Vec<String>,
    pub scripts: Vec<String>,
    pub conflicts: Vec<String>,
    /// 库内同名 skill 的既有来源（同名不同来源不混同）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_source: Option<String>,
    /// 多 skill 仓库时列出候选路径（供用户选择，不默默全导）
    pub candidates: Vec<String>,
    pub note: String,
}

fn find_skill_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && entry.file_name() == "SKILL.md" {
            if let Some(parent) = entry.path().parent() {
                out.push(parent.to_path_buf());
            }
        }
    }
    out.sort();
    out
}

fn resolve_skill_dir_in_snapshot(
    snap_root: &Path,
    repo_path: Option<&str>,
) -> Result<(PathBuf, Vec<String>)> {
    let explicit: Option<PathBuf> = repo_path.map(|p| {
        let rel = p.trim_matches('/');
        if rel.is_empty() {
            snap_root.to_path_buf()
        } else {
            snap_root.join(rel)
        }
    });
    if let Some(dir) = explicit {
        if !dir.starts_with(snap_root) {
            return Err(Error::new(
                code::PATH_TRAVERSAL,
                format!("repo-path 越界: {}", dir.display()),
            ));
        }
        if !dir.join("SKILL.md").is_file() {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("指定路径不含 SKILL.md（不是 skill 根）: {}", dir.display()),
            ));
        }
        return Ok((dir, Vec::new()));
    }
    let dirs = find_skill_dirs(snap_root);
    match dirs.len() {
        0 => Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            "仓库内未发现 SKILL.md（请用 --path 指定 skill 子目录）",
        )),
        1 => Ok((dirs[0].clone(), Vec::new())),
        _ => {
            let cands: Vec<String> = dirs
                .iter()
                .filter_map(|d| d.strip_prefix(snap_root).ok())
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            Err(Error::new(
                code::USAGE,
                format!(
                    "该仓库包含多个 skill（{} 个）；请用 --path 选择其一: {:?}",
                    cands.len(),
                    cands
                ),
            ))
        }
    }
}

/// Git 仓库/子目录导入预览（AIL-064）：克隆进隔离缓存（不执行仓库内任何脚本），
/// 解析 commit，列出文件/脚本/冲突/既有来源。`repo_path` 为 None 且仓库含多个
/// skill 时返回候选列表错误供选择。
pub fn git_import_preview(
    data_root: &Path,
    url: &str,
    repo_path: Option<&str>,
    ref_: Option<&str>,
    name_override: Option<&str>,
) -> Result<GitImportPreview> {
    git_import_preview_labeled(data_root, url, repo_path, ref_, name_override, None)
}

/// 带发现入口标签的预览（AIL-065：入口与实际来源同时记录）。
pub fn git_import_preview_labeled(
    data_root: &Path,
    url: &str,
    repo_path: Option<&str>,
    ref_: Option<&str>,
    name_override: Option<&str>,
    discovery_label: Option<&str>,
) -> Result<GitImportPreview> {
    let src = crate::source::GitSource::new(url, ref_)?;
    let cache = skill_git_cache(data_root, &src.identity);
    let snap = src.resolve(&cache, None)?;
    let (skill_dir, _cands) =
        resolve_skill_dir_in_snapshot(&snap.root, repo_path).map_err(|e| {
            if e.code == code::USAGE {
                let cands: Vec<String> = find_skill_dirs(&snap.root)
                    .iter()
                    .filter_map(|d| d.strip_prefix(&snap.root).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                return e.context(serde_json::json!({ "candidates": cands }));
            }
            e
        })?;
    let repo_path_rel = skill_dir
        .strip_prefix(&snap.root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let preview = import_preview(data_root, &skill_dir, name_override)?;
    let target = PathBuf::from(preview.target_dir.clone());
    Ok(GitImportPreview {
        discovery_entry: discovery_label
            .map(str::to_string)
            .unwrap_or_else(|| format!("github:{url}")),
        repo_url: src.url.clone(),
        repo_path: repo_path_rel,
        ref_: ref_.map(str::to_string),
        resolved_commit: snap.resolved_commit.clone(),
        skill_name: preview.skill_name,
        files: preview.files,
        scripts: preview.scripts,
        conflicts: preview.conflicts,
        existing_source: crate::skill_source::existing_source_note(&target),
        candidates: Vec::new(),
        note: "预览不复制不执行；导入仅复制文件，绝不运行仓库内脚本".into(),
    })
}

/// Git 仓库/子目录导入执行：复用事务性本地导入 + git 来源元数据。
pub fn git_import_execute(
    data_root: &Path,
    url: &str,
    repo_path: Option<&str>,
    ref_: Option<&str>,
    name_override: Option<&str>,
) -> Result<ImportReport> {
    git_import_execute_labeled(data_root, url, repo_path, ref_, name_override, None)
}

/// 带发现入口标签的导入执行（AIL-065）。
pub fn git_import_execute_labeled(
    data_root: &Path,
    url: &str,
    repo_path: Option<&str>,
    ref_: Option<&str>,
    name_override: Option<&str>,
    discovery_label: Option<&str>,
) -> Result<ImportReport> {
    let preview = git_import_preview_labeled(
        data_root,
        url,
        repo_path,
        ref_,
        name_override,
        discovery_label,
    )?;
    if !preview.conflicts.is_empty() {
        return Err(Error::new(
            code::SOURCE_CONFLICT,
            format!(
                "导入冲突：{}；{}",
                preview.conflicts.join("; "),
                preview
                    .existing_source
                    .clone()
                    .unwrap_or_else(|| "同名目录已存在".into())
            ),
        )
        .fix("换一个 --name 另存为新副本；或先删除库内同名技能再导入（更新语义）"));
    }
    let src = crate::source::GitSource::new(url, ref_)?;
    let cache = skill_git_cache(data_root, &src.identity);
    let snap = src.resolve(&cache, None)?;
    let skill_dir = snap.root.join(preview.repo_path.trim_start_matches('/'));
    let meta = crate::skill_source::git_meta(
        &preview.discovery_entry,
        &preview.repo_url,
        &preview.repo_path,
        ref_,
        snap.resolved_commit.as_deref(),
        "",
    );
    import_execute_with_source(data_root, &skill_dir, name_override, Some(meta))
}

/// 上游检查（AIL-066）：比较 导入基线/本地现状/上游最新；只检查，不应用。
pub fn check_update(data_root: &Path, name: &str) -> Result<crate::skill_source::UpdateStatus> {
    use crate::skill_source::UpdateStatus;
    let skill_dir = library_root(data_root)
        .join("resources")
        .join("skills")
        .join(name);
    if !skill_dir.is_dir() {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("库内不存在 skill: {name}"),
        ));
    }
    let meta = crate::skill_source::read_meta(&skill_dir);
    let Some(meta) = meta else {
        return Ok(UpdateStatus {
            skill: name.into(),
            source_kind: "unknown".into(),
            discovery_entry: String::new(),
            state: "not-applicable".into(),
            imported_digest: String::new(),
            local_digest: None,
            upstream_digest: None,
            upstream_commit: None,
            note: Some("旧数据无来源记录：按本地管理，不伪称能上游更新".into()),
        });
    };
    if !meta.updatable() {
        return Ok(UpdateStatus {
            skill: name.into(),
            source_kind: meta.source_kind.clone(),
            discovery_entry: meta.discovery_entry.clone(),
            state: "not-applicable".into(),
            imported_digest: meta.imported_digest,
            local_digest: None,
            upstream_digest: None,
            upstream_commit: None,
            note: Some("本地来源：无上游可检查".into()),
        });
    }
    let local_digest = skill_dir_digest(&skill_dir);
    let url = meta.repo_url.clone().unwrap_or_default();
    let src = crate::source::GitSource::new(&url, meta.ref_.as_deref())?;
    let cache = skill_git_cache(data_root, &src.identity);
    let snap = src.resolve(&cache, None)?;
    let upstream_dir = snap.root.join(meta.repo_path.as_deref().unwrap_or(""));
    if !upstream_dir.join("SKILL.md").is_file() {
        return Ok(UpdateStatus {
            skill: name.into(),
            source_kind: meta.source_kind,
            discovery_entry: meta.discovery_entry,
            state: "upstream-missing".into(),
            imported_digest: meta.imported_digest,
            local_digest: Some(local_digest),
            upstream_digest: None,
            upstream_commit: snap.resolved_commit,
            note: Some("上游 skill 目录已不存在（可能删除/改名）".into()),
        });
    }
    let upstream_digest = skill_dir_digest(&upstream_dir);
    let state = match (
        local_digest == meta.imported_digest,
        upstream_digest == meta.imported_digest,
    ) {
        (true, true) => "up-to-date",
        (false, true) => "local-modified",
        (true, false) => "upstream-new",
        (false, false) => "conflict",
    };
    Ok(UpdateStatus {
        skill: name.into(),
        source_kind: meta.source_kind.clone(),
        discovery_entry: meta.discovery_entry.clone(),
        state: state.into(),
        imported_digest: meta.imported_digest,
        local_digest: Some(local_digest),
        upstream_digest: Some(upstream_digest),
        upstream_commit: snap.resolved_commit,
        note: Some(format!(
            "入口 {}（只检查未应用；检查不等于更新）",
            meta.discovery_entry
        )),
    })
}

/// 更新执行（AIL-066）：本地未改才允许替换；更新前备份旧版到库内
/// .updates-backup/；来源元数据随新内容推进（锁定版本显式前进）。
pub fn update_execute(data_root: &Path, name: &str) -> Result<serde_json::Value> {
    let status = check_update(data_root, name)?;
    crate::skill_source::ensure_updatable(&status)?;
    if status.state == "up-to-date" {
        // ensure_updatable 已报「无变化」；此处不可达，双保险
        return Ok(serde_json::json!({ "updated": false, "state": status.state }));
    }
    let meta = crate::skill_source::read_meta(
        &library_root(data_root).join("resources/skills").join(name),
    )
    .ok_or_else(|| Error::new(code::UNKNOWN_REFERENCE, "来源元数据缺失"))?;
    let url = meta.repo_url.clone().unwrap_or_default();
    let src = crate::source::GitSource::new(&url, meta.ref_.as_deref())?;
    let cache = skill_git_cache(data_root, &src.identity);
    let snap = src.resolve(&cache, None)?;
    let upstream_dir = snap.root.join(meta.repo_path.as_deref().unwrap_or(""));
    let target = library_root(data_root).join("resources/skills").join(name);
    // 备份旧版（可恢复）
    let backup_dir = library_root(data_root).join(".updates-backup");
    std::fs::create_dir_all(&backup_dir)?;
    let backup = backup_dir.join(format!("{name}-{}", crate::ids::now_iso().replace(':', "")));
    copy_tree(&target, &backup)?;
    // 替换内容
    std::fs::remove_dir_all(&target)?;
    std::fs::create_dir_all(&target)?;
    copy_tree(&upstream_dir, &target)?;
    // 推进来源元数据（新 commit/摘要；发现入口保留）
    let mut new_meta = meta.clone();
    new_meta.resolved_commit = snap.resolved_commit.clone();
    new_meta.imported_digest = skill_dir_digest(&target);
    new_meta.fetched_at = crate::ids::now_iso();
    crate::skill_source::write_meta(&target, &new_meta)?;
    // 发布前整库校验失败则回滚
    let lib = library_root(data_root);
    let manifest = TeamManifest::load_from(&lib)?;
    if enumerate(&lib, &manifest, LIBRARY_TEAM_ID).is_err() {
        let _ = std::fs::remove_dir_all(&target);
        copy_tree(&backup, &target)?;
        return Err(Error::new(
            code::WRITE_FAILED,
            "更新后库校验失败，已回滚旧版",
        ));
    }
    Ok(serde_json::json!({
        "updated": true,
        "skill": name,
        "from_commit": meta.resolved_commit,
        "to_commit": new_meta.resolved_commit,
        "backup": backup,
        "note": "更新仅写入个人库；部署需重新预览+应用",
    }))
}

/// 递归复制（跳过符号链接；导入/更新不引入链接）。
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry
            .path()
            .strip_prefix(src)
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let to = dst.join(&rel);
        let ft = entry.file_type();
        if ft.is_dir() {
            std::fs::create_dir_all(&to)?;
        } else if ft.is_file() {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 通过发现入口导入（AIL-065）：解析提供方 → 实际 GitHub 仓库 → 复用 git 导入；
/// 有 skill 名提示时按目录名/资源名匹配（不猜 API）。
pub fn import_via_discovery(
    data_root: &Path,
    entry: &str,
    name_override: Option<&str>,
    execute: bool,
) -> Result<serde_json::Value> {
    let pref = crate::skill_source::normalize_discovery_entry(entry)?;
    if execute {
        let r = git_import_execute_labeled(
            data_root,
            &pref.repo_url,
            None,
            None,
            name_override.or(pref.hint_name.as_deref()),
            Some(&pref.discovery_entry),
        )?;
        Ok(serde_json::json!({
            "executed": true,
            "discovery_entry": pref.discovery_entry,
            "provider": pref.provider,
            "repo_url": pref.repo_url,
            "report": r,
        }))
    } else {
        match git_import_preview(
            data_root,
            &pref.repo_url,
            None,
            None,
            name_override.or(pref.hint_name.as_deref()),
        ) {
            Ok(p) => Ok(serde_json::json!({
                "executed": false,
                "discovery_entry": pref.discovery_entry,
                "provider": pref.provider,
                "repo_url": pref.repo_url,
                "preview": p,
            })),
            // 提示名匹配失败（仓库布局未知）→ 列出候选供 --name/--path 指定
            Err(e) => Ok(serde_json::json!({
                "executed": false,
                "discovery_entry": pref.discovery_entry,
                "provider": pref.provider,
                "repo_url": pref.repo_url,
                "error": e.to_string(),
                "candidates": e.context.get("candidates"),
                "note": "按提示名未直接命中；可用 --name <skill名> 或改用 --url+--path 明确指定",
            })),
        }
    }
}
