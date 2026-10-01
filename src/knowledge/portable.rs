//! Portable identity and location hints. Absolute paths belong only to local bindings.
use super::location::{self, Binding};
use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

pub const DECLARATION: &str = ".ailoom/knowledge.json";
fn fail(message: impl Into<String>) -> Error {
    Error::new(code::KNOWLEDGE_STATE_CONFLICT, message)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Declaration {
    pub schema_version: u32,
    pub project_id: String,
    pub knowledge_id: String,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    Project {
        path: String,
    },
    Git {
        url: String,
        branch: Option<String>,
        subdir: String,
    },
    Directory,
}
pub(crate) fn portable_url(url: &str) -> bool {
    ["https://", "http://", "ssh://", "git://"]
        .iter()
        .any(|prefix| url.starts_with(prefix))
        || (!url.contains("://")
            && !url.contains("::")
            && !url.starts_with('-')
            && url.split_once(':').is_some_and(|(host, path)| {
                host.contains('@') && !host.contains('/') && !path.is_empty()
            }))
}
fn relative(value: &str) -> Result<()> {
    if value.is_empty()
        || Path::new(value)
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        || value.contains('\\')
    {
        return Err(fail("恢复配置中的相对目录无效"));
    }
    Ok(())
}
fn safe_file(root: &Path) -> Result<PathBuf> {
    for p in [root.join(".ailoom"), root.join(DECLARATION)] {
        if p.symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(fail("恢复配置不能是符号链接"));
        }
    }
    Ok(root.join(DECLARATION))
}
pub fn read(root: &Path) -> Result<Option<Declaration>> {
    let p = safe_file(root)?;
    let bytes = match std::fs::read(p) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if bytes.len() > 65536 {
        return Err(fail("恢复配置过大"));
    }
    let d: Declaration = serde_json::from_slice(&bytes)?;
    if d.schema_version != 1 {
        return Err(Error::new(code::SCHEMA_VERSION, "不支持的恢复配置版本"));
    }
    for id in [&d.project_id, &d.knowledge_id] {
        uuid::Uuid::parse_str(id).map_err(|_| fail("恢复配置中的 UUID 无效"))?;
    }
    match &d.source {
        Source::Project { path } => relative(path)?,
        Source::Git {
            url,
            subdir,
            branch,
        } => {
            relative(subdir)?;
            if !portable_url(url) {
                return Err(fail("恢复来源需要 HTTPS、SSH 或 Git 地址"));
            }
            crate::source::GitSource::new(url, branch.as_deref())?;
            if crate::gitx::redact_credentials(url) != *url {
                return Err(fail("Git 地址不能包含凭据"));
            }
            if branch
                .as_ref()
                .is_some_and(|v| v.starts_with('-') || v.is_empty())
            {
                return Err(fail("恢复分支无效"));
            }
        }
        Source::Directory => {}
    }
    Ok(Some(d))
}
pub fn write(root: &Path, d: &Declaration) -> Result<()> {
    crate::sync_common::atomic_write(&safe_file(root)?, &serde_json::to_vec_pretty(d)?)
}
pub fn describe(b: &Binding) -> Result<Declaration> {
    let old = read(&b.project_root)?;
    if old.as_ref().is_some_and(|d| d.knowledge_id != b.id) {
        return Err(fail("项目声明指向另一知识库，请恢复已有知识库"));
    }
    let source = if let Ok(path) = b.path.strip_prefix(&b.project_root) {
        Source::Project {
            path: if path.as_os_str().is_empty() {
                ".".into()
            } else {
                path.to_string_lossy().into()
            },
        }
    } else {
        let mut info = location::inspect_directory(&b.path)?;
        // Never publish a redacted credential placeholder as a usable clone URL.
        if let Ok(raw) = location::git(&b.path, &["remote", "get-url", "origin"]) {
            if crate::gitx::redact_credentials(raw.trim()) != raw.trim() {
                info["git"]["remote"] = Value::Null;
            }
        }
        match (info["git"]["root"].as_str(), info["git"]["remote"].as_str()) {
            (Some(root), Some(url))
                if crate::source::GitSource::new(url, None).is_ok()
                    && portable_url(url)
                    && crate::gitx::redact_credentials(url) == url =>
            {
                let rel = b
                    .path
                    .strip_prefix(root)
                    .map_err(|_| fail("Git 根目录不包含知识库"))?;
                let branch =
                    location::git(&b.path, &["symbolic-ref", "--quiet", "--short", "HEAD"])
                        .ok()
                        .map(|s| s.trim().into());
                Source::Git {
                    url: url.into(),
                    branch,
                    subdir: if rel.as_os_str().is_empty() {
                        ".".into()
                    } else {
                        rel.to_string_lossy().into()
                    },
                }
            }
            _ => Source::Directory,
        }
    };
    Ok(Declaration {
        schema_version: 1,
        project_id: old.map(|d| d.project_id).unwrap_or_else(crate::ids::new_id),
        knowledge_id: b.id.clone(),
        source,
    })
}
pub fn suggested(root: &Path, d: &Declaration) -> Result<Option<PathBuf>> {
    match &d.source {
        Source::Project { path } => {
            let p = location::destination(&root.join(path))?;
            if !p.starts_with(root) {
                return Err(fail("项目内知识库路径越出项目"));
            }
            Ok(Some(p))
        }
        _ => Ok(None),
    }
}
pub fn recovery(root: &Path) -> Result<Value> {
    Ok(match read(root)? {
        Some(d) => json!({"declaration":d,"suggested_path":suggested(root,&d)?}),
        None => Value::Null,
    })
}
/// Explicit clone only. Never replaces an existing checkout, never installs hooks.
pub fn clone_repository(root: &Path, dest: &Path) -> Result<Value> {
    let d = read(root)?.ok_or_else(|| fail("项目没有恢复配置"))?;
    let Source::Git {
        url,
        branch,
        subdir,
    } = &d.source
    else {
        return Err(fail("该知识库没有可克隆的 Git 地址"));
    };
    let dest = location::destination(dest)?;
    if dest.exists() {
        return Err(fail(
            "克隆目录已存在；请选择新目录，或直接关联现有知识库文件夹",
        ));
    }
    let parent = dest.parent().ok_or_else(|| fail("无效克隆目录"))?;
    std::fs::create_dir_all(parent)?;
    let mut args = vec![
        "-c",
        "protocol.file.allow=never",
        "-c",
        "filter.lfs.required=false",
        "clone",
        "--no-checkout",
        "--no-recurse-submodules",
    ];
    if let Some(branch) = branch {
        args.extend(["--branch", branch]);
    }
    args.extend(["--", url, dest.to_str().ok_or_else(|| fail("无效路径"))?]);
    location::git(parent, &args)?;
    // Read blobs directly: no checkout filters, hooks, or submodule initialization.
    let listing = location::git(&dest, &["ls-tree", "-rz", "--full-tree", "HEAD"])?;
    let mut size = 0usize;
    for (i, record) in listing.split('\0').filter(|r| !r.is_empty()).enumerate() {
        let (header, rel) = record
            .split_once('\t')
            .ok_or_else(|| fail("无效 Git 文件清单"))?;
        relative(rel)?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3
            || !matches!(fields[0], "100644" | "100755")
            || fields[1] != "blob"
            || Path::new(rel).components().any(|p| p.as_os_str() == ".git")
        {
            return Err(fail(
                "仓库包含链接或子模块；已保留克隆目录，请在本机准备好知识库后关联",
            ));
        }
        let output = std::process::Command::new("git")
            .args(["cat-file", "blob", fields[2]])
            .current_dir(&dest)
            .output()?;
        if !output.status.success() {
            return Err(fail("无法读取克隆文件"));
        }
        size += output.stdout.len();
        if i >= 10000 || size > 128 * 1024 * 1024 {
            return Err(fail("克隆内容超过恢复限制；已保留目录"));
        }
        let file = dest.join(rel);
        crate::sync_common::atomic_write(&file, &output.stdout)?;
        #[cfg(unix)]
        if fields[0] == "100755" {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    location::git(&dest, &["reset", "--mixed", "HEAD"])?;
    let path = location::destination(&dest.join(subdir))?;
    if !path.starts_with(&dest) {
        return Err(fail("知识库子目录越出克隆目录"));
    }
    let marker: Value =
        serde_json::from_slice(&std::fs::read(path.join("ailoom-knowledge.json"))?)?;
    if marker["id"].as_str() != Some(&d.knowledge_id) {
        return Err(fail("克隆完成，但知识库 ID 不匹配；已保留目录供检查"));
    }
    Ok(json!({"cloned":true,"path":path,"knowledge_id":d.knowledge_id}))
}
