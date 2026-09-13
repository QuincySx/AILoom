//! 资源身份与枚举（AIL-006）：从锁定快照解析统一资源清单。
//! 每个 entry 携带稳定 ResourceId、归属元数据与可解释的来源路径。

use crate::error::{code, Error, Result};
use crate::manifest::{valid_name, TeamManifest};
use serde::{Deserialize, Serialize};
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Skill,
    Rule,
    Doc,
    Agent,
    Mcp,
    Learning,
    Env,
    Hook,
    Package,
}

impl ResourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceKind::Skill => "skill",
            ResourceKind::Rule => "rule",
            ResourceKind::Doc => "doc",
            ResourceKind::Agent => "agent",
            ResourceKind::Mcp => "mcp",
            ResourceKind::Learning => "learning",
            ResourceKind::Env => "env",
            ResourceKind::Hook => "hook",
            ResourceKind::Package => "package",
        }
    }
}

/// 稳定资源身份：source/kind/namespace/name（契约 §1）。tags 等未来字段不参与身份。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceId {
    pub source: String,
    pub kind: ResourceKind,
    pub namespace: String,
    pub name: String,
}

impl std::fmt::Display for ResourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}/{}/{}",
            self.source,
            self.kind.as_str(),
            self.namespace,
            self.name
        )
    }
}

/// 归属元数据（契约 §2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMeta {
    #[serde(default)]
    pub shared: bool,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    pub namespace: String,
    /// 未来字段：不影响身份与选择语义（显式忽略即向前兼容）
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 统一资源条目。skill 的 path 为目录，其余为文件（相对快照根）。
#[derive(Debug, Clone)]
pub struct ResourceEntry {
    pub id: ResourceId,
    pub meta: ResourceMeta,
    pub path: String,
    pub description: String,
    /// 原始文件全文（SKILL.md 或 .toml），适配卡直接复用，避免二次解析
    pub raw: Option<String>,
}

impl ResourceEntry {
    /// 逻辑目标键：同 key 不同 id 的选中资源即为目标路径冲突（E3006）。
    /// learning 不部署，无目标键。
    pub fn target_key(&self) -> Option<String> {
        match self.id.kind {
            ResourceKind::Learning => None,
            kind => Some(format!("{}:{}", kind.as_str(), self.id.name)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RawMeta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub shared: Option<bool>,
    #[serde(default)]
    pub projects: Option<Vec<String>>,
    #[serde(default)]
    pub roles: Option<Vec<String>>,
    #[serde(default)]
    pub namespace: Option<String>,
    /// learning 专用：单项目
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// 切分 frontmatter：返回 Some((yaml 原文, 正文))；无 frontmatter → None。
pub fn split_frontmatter(text: &str) -> Result<Option<(String, String)>> {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return Ok(None);
    };
    let end = rest.find("\n---").ok_or_else(|| {
        Error::new(
            code::MANIFEST_MISSING_FIELD,
            "frontmatter 未闭合（缺少结束 ---）",
        )
    })?;
    let yaml_part = &rest[..end];
    // 跳过关闭行 "\n---\n"（兼容 \r\n）
    let mut after = (end + 4).min(rest.len());
    if rest[after..].starts_with('\r') {
        after += 1;
    }
    if rest[after..].starts_with('\n') {
        after += 1;
    }
    let body = rest[after..].to_string();
    Ok(Some((yaml_part.to_string(), body)))
}

/// 解析 Markdown frontmatter（`---\n…\n---\n`），返回 (元数据YAML, 正文)。
pub fn parse_frontmatter(text: &str) -> Result<(Option<RawMeta>, String)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"));
    if rest.is_none() {
        return Ok((None, text.to_string()));
    }
    let rest = rest.unwrap();
    let end = rest.find("\n---").ok_or_else(|| {
        Error::new(
            code::MANIFEST_MISSING_FIELD,
            "frontmatter 未闭合（缺少结束 ---）",
        )
    })?;
    let yaml_part = &rest[..end];
    let body_start = end + rest[end..].find('\n').map(|i| i + 1).unwrap_or(0);
    let body = rest[body_start.min(rest.len())..].to_string();
    let meta: RawMeta = serde_yaml::from_str(yaml_part).map_err(|e| {
        Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("frontmatter 解析失败: {e}"),
        )
    })?;
    Ok((Some(meta), body))
}

fn parse_meta_block(text: &str, is_markdown: bool, context: &str) -> Result<RawMeta> {
    if is_markdown {
        let (meta, _) = parse_frontmatter(text)?;
        Ok(meta.unwrap_or(RawMeta {
            name: None,
            description: None,
            shared: None,
            projects: None,
            roles: None,
            namespace: None,
            project: None,
            tags: None,
        }))
    } else {
        toml::from_str(text).map_err(|e| {
            Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("资源 TOML 解析失败: {e}"),
            )
            .context(serde_json::json!({"resource": context}))
        })
    }
}

fn build_entry(
    source: &str,
    kind: ResourceKind,
    manifest: &TeamManifest,
    meta: RawMeta,
    name: &str,
    path: String,
    raw: Option<String>,
) -> Result<ResourceEntry> {
    let name = meta.name.clone().unwrap_or_else(|| name.to_string());
    if !valid_name(&name) {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("资源 name 非法（需 [a-z0-9][a-z0-9._-]{{0,63}}）: {name}"),
        )
        .context(serde_json::json!({ "path": path })));
    }
    let namespace = meta.namespace.clone().ok_or_else(|| {
        Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("资源缺少 namespace: {name}"),
        )
        .context(serde_json::json!({ "path": path }))
    })?;
    if !manifest.namespaces.known.contains(&namespace) {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("资源 `{name}` 引用未声明的 namespace `{namespace}`"),
        )
        .context(serde_json::json!({ "known": manifest.namespaces.known })));
    }
    let projects = meta.projects.clone().unwrap_or_default();
    let roles = meta.roles.clone().unwrap_or_default();
    let mut projects = projects;
    let mut roles = roles;
    if let Some(single) = &meta.project {
        if kind != ResourceKind::Learning {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("仅 learning 允许 `project` 单数字段: {name}"),
            ));
        }
        projects.push(single.clone());
    }
    projects.sort();
    roles.sort();
    // 重复项
    check_dup(&projects, &name, "projects")?;
    check_dup(&roles, &name, "roles")?;
    // 引用校验
    for p in &projects {
        manifest
            .require_project(p)
            .map_err(|e| e.context(serde_json::json!({ "resource": name, "path": path })))?;
    }
    for r in &roles {
        manifest
            .require_role(r)
            .map_err(|e| e.context(serde_json::json!({ "resource": name, "path": path })))?;
    }
    let shared = meta.shared.unwrap_or(false);
    if !shared && projects.is_empty() && roles.is_empty() {
        return Err(Error::new(
            code::OWNERLESS_RESOURCE,
            format!("资源 `{name}` 无归属：shared=false 且未声明 projects/roles"),
        )
        .context(serde_json::json!({ "path": path }))
        .fix("在资源元数据声明 shared: true 或 projects/roles"));
    }
    // learning 特殊约束（契约 §2.3）
    if kind == ResourceKind::Learning {
        if shared && !projects.is_empty() {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("经验 `{name}` 不能同时声明 shared 与 project"),
            ));
        }
        if projects.len() > 1 {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("经验 `{name}` 归属不能多于一个项目（多项目知识请在贡献时明确目标）"),
            ));
        }
    }
    Ok(ResourceEntry {
        id: ResourceId {
            source: source.to_string(),
            kind,
            namespace: namespace.clone(),
            name: name.clone(),
        },
        meta: ResourceMeta {
            shared,
            projects,
            roles,
            namespace,
            tags: meta.tags.unwrap_or_default(),
        },
        path,
        description: meta.description.unwrap_or_default(),
        raw,
    })
}

/// 渲染产物提交前按资源契约复验（AIL-015）：Markdown 资源必须能被资源解析器
/// 重新解析并通过归属/namespace/name 合法性检查，非法输出在进入贡献链路前拒绝。
pub fn validate_rendered_markdown(
    manifest: &TeamManifest,
    kind: ResourceKind,
    name: &str,
    content: &str,
    path: &str,
) -> Result<ResourceEntry> {
    let meta = parse_meta_block(content, true, path)?;
    build_entry(
        "contribute",
        kind,
        manifest,
        meta,
        name,
        path.to_string(),
        Some(content.to_string()),
    )
}

fn check_dup(items: &[String], resource: &str, field: &str) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for i in items {
        if !seen.insert(i.as_str()) {
            return Err(Error::new(
                code::DUPLICATE_ITEM,
                format!("资源 `{resource}` 的 {field} 含重复项: {i}"),
            ));
        }
    }
    Ok(())
}

/// 拒绝资源目录中出现任何符号链接（外跳 symlink 防御，AIL-006 必测）。
fn reject_symlinks(dir: &Path, context: &str) -> Result<()> {
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_symlink() {
            return Err(Error::new(
                code::PATH_TRAVERSAL,
                format!("资源目录含符号链接，已拒绝: {}", entry.path().display()),
            )
            .context(serde_json::json!({ "resource_root": context })));
        }
    }
    Ok(())
}

/// 枚举快照内全部资源（先 schema 校验再逐项解析；结果按 ResourceId 排序）。
pub fn enumerate(
    snapshot_root: &Path,
    manifest: &TeamManifest,
    source: &str,
) -> Result<Vec<ResourceEntry>> {
    let paths = manifest.effective_paths();
    let mut entries = Vec::new();

    // skills：目录含 SKILL.md
    let skills_dir = snapshot_root.join(&paths.skills);
    if skills_dir.is_dir() {
        for dir in std::fs::read_dir(&skills_dir)?.flatten() {
            let path = dir.path();
            if !path.is_dir() || dir.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            reject_symlinks(&path, dir.file_name().to_string_lossy().as_ref())?;
            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() {
                return Err(Error::new(
                    code::MANIFEST_MISSING_FIELD,
                    format!(
                        "技能目录缺少 SKILL.md: {}",
                        dir.file_name().to_string_lossy()
                    ),
                )
                .context(serde_json::json!({ "dir": path.display().to_string() })));
            }
            let dir_name = dir.file_name().to_string_lossy().to_string();
            let raw = std::fs::read_to_string(&skill_md)?;
            let (meta, _) = parse_frontmatter(&raw)?;
            let meta = meta.unwrap_or(RawMeta {
                name: None,
                description: None,
                shared: None,
                projects: None,
                roles: None,
                namespace: None,
                project: None,
                tags: None,
            });
            if let Some(n) = &meta.name {
                if *n != dir_name {
                    return Err(Error::new(
                        code::MANIFEST_MISSING_FIELD,
                        format!("SKILL.md name `{n}` 与目录名 `{dir_name}` 不一致"),
                    ));
                }
            }
            let rel = path
                .strip_prefix(snapshot_root)?
                .to_string_lossy()
                .to_string();
            entries.push(build_entry(
                source,
                ResourceKind::Skill,
                manifest,
                meta,
                &dir_name,
                rel,
                Some(raw),
            )?);
        }
    }

    // markdown 类：rules / docs / learnings
    for (kind, dir_path) in [
        (ResourceKind::Rule, &paths.rules),
        (ResourceKind::Doc, &paths.docs),
        (ResourceKind::Learning, &paths.learnings),
    ] {
        let base = snapshot_root.join(dir_path);
        if !base.is_dir() {
            continue;
        }
        for file in WalkDir::new(&base)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !file.file_type().is_file() {
                continue;
            }
            let fname = file.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') || !fname.ends_with(".md") {
                continue;
            }
            let raw = std::fs::read_to_string(file.path())?;
            let meta = parse_meta_block(&raw, true, &fname)?;
            let stem = fname.trim_end_matches(".md").to_string();
            let rel = file
                .path()
                .strip_prefix(snapshot_root)?
                .to_string_lossy()
                .to_string();
            entries.push(build_entry(
                source,
                kind,
                manifest,
                meta,
                &stem,
                rel,
                Some(raw),
            )?);
        }
    }

    // toml 类：agents / mcp / env / hooks / packages
    for (kind, dir_path) in [
        (ResourceKind::Agent, &paths.agents),
        (ResourceKind::Mcp, &paths.mcp),
        (ResourceKind::Env, &paths.env),
        (ResourceKind::Hook, &paths.hooks),
        (ResourceKind::Package, &paths.packages),
    ] {
        let base = snapshot_root.join(dir_path);
        if !base.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&base)?.flatten() {
            let fp = file.path();
            if !fp.is_file() {
                continue;
            }
            let fname = file.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') || !fname.ends_with(".toml") {
                continue;
            }
            let raw = std::fs::read_to_string(&fp)?;
            let meta = parse_meta_block(&raw, false, &fname)?;
            let stem = fname.trim_end_matches(".toml").to_string();
            let rel = fp
                .strip_prefix(snapshot_root)?
                .to_string_lossy()
                .to_string();
            entries.push(build_entry(
                source,
                kind,
                manifest,
                meta,
                &stem,
                rel,
                Some(raw),
            )?);
        }
    }

    entries.sort_by(|a, b| a.id.cmp(&b.id));
    // 同源内 (kind, name) 跨 namespace 的目标冲突在 resolver 报；这里只查完全同 id（不可能，名字即来源）
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_parses_and_body_preserved() {
        let text = "---\nname: x\ndescription: 你好\nshared: true\nprojects: [a]\nroles: []\nnamespace: common\n---\n\n正文第一行\n";
        let (meta, body) = parse_frontmatter(text).unwrap();
        let m = meta.unwrap();
        assert_eq!(m.name.as_deref(), Some("x"));
        assert_eq!(m.projects.as_ref().unwrap(), &vec!["a".to_string()]);
        assert!(body.contains("正文第一行"));
    }

    #[test]
    fn no_frontmatter_returns_none() {
        let (meta, body) = parse_frontmatter("纯正文").unwrap();
        assert!(meta.is_none());
        assert_eq!(body, "纯正文");
    }
}
