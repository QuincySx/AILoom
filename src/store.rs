//! SkillStore：按源仓库分桶存放 Skill 实体；Workspace 只挂链接。
//! 新布局：`<store>/<host>/<repo>/revisions/<version>/<skill>/`。
//! source_key 保留历史编码；旧目录不自动删除，项目下次同步时逐个切换到可读布局。
//! Store 根解析见 [`crate::paths::resolve_store_root`]。

use crate::error::{code, Error, Result};
use crate::ids::sha256_hex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use crate::paths::resolve_store_root;

/// 规范化源 identity：去凭据、统一 git/https、去 `.git`、整体小写（稳定同仓同 key）。
pub fn normalize_identity(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }
    // 本地路径 / file://
    if s.starts_with("file://")
        || s.starts_with('/')
        || (!s.contains("://") && !s.contains('@') && !s.contains(':'))
    {
        let p = s.strip_prefix("file://").unwrap_or(s);
        let abs = PathBuf::from(p);
        let canon = abs.canonicalize().unwrap_or(abs);
        return format!("local:{}", canon.display()).to_ascii_lowercase();
    }
    // git@host:path/repo.git
    if let Some(rest) = s.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            let path = path.trim_start_matches('/').trim_end_matches(".git");
            return format!("https://{host}/{path}").to_ascii_lowercase();
        }
    }
    // https://user:pass@host/path 或 ssh://
    let mut s = s.to_string();
    if let Some(scheme_end) = s.find("://") {
        let after = &s[scheme_end + 3..];
        if let Some(at) = after.rfind('@') {
            let scheme = &s[..=scheme_end + 2];
            s = format!("{scheme}{}", &after[at + 1..]);
        }
    }
    let s = s.trim_end_matches('/').trim_end_matches(".git");
    s.to_ascii_lowercase()
}

/// 可逆：规范化 identity → base64url（无 padding）。
pub fn source_key(identity: &str) -> String {
    let norm = normalize_identity(identity);
    base64url_encode(norm.as_bytes())
}

pub fn source_key_decode(key: &str) -> Result<String> {
    let bytes = base64url_decode(key).map_err(|e| {
        Error::new(code::INTERNAL, format!("source_key 解码失败: {e}"))
            .fix("检查 store 下分桶目录名是否被改坏")
    })?;
    String::from_utf8(bytes)
        .map_err(|e| Error::new(code::INTERNAL, format!("source_key 非 UTF-8: {e}")))
}

/// 可读的来源路径；旧 source_key 仅用于识别历史目录，不再作为新实体目录名。
pub fn source_bucket(store_root: &Path, identity: &str) -> PathBuf {
    let (source, revision) = identity.rsplit_once('#').unwrap_or((identity, "working"));
    let norm = normalize_identity(source.strip_prefix("git+").unwrap_or(source));
    let remote = norm
        .split_once("://")
        .filter(|(scheme, _)| matches!(*scheme, "https" | "http" | "ssh"));
    let mut bucket = store_root.to_path_buf();
    if let Some((_, rest)) = remote {
        let (host, path) = rest.split_once('/').unwrap_or((rest, "repository"));
        bucket.push(path_component(host));
        for part in path.trim_matches('/').split('/') {
            bucket.push(path_component(part));
        }
    } else {
        let label = source
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("source");
        bucket.push("local");
        bucket.push(format!(
            "{}-{}",
            path_component(label),
            &sha256_hex(source.as_bytes())[..12]
        ));
    }
    bucket.join("revisions").join(path_component(revision))
}

// 编码分隔符、点目录和大小写，避免逃逸以及大小写不敏感文件系统中的碰撞。
fn path_component(value: &str) -> String {
    if value.is_empty() {
        return "%00".into();
    }
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'-'
                || b == b'_'
                || (b == b'.' && value != "." && value != "..")
            {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// 仓内 skill 路径相对 skills 根；拒绝逃逸。
pub fn skill_rel_under_root(entry_path: &Path, skills_root: &Path) -> Result<PathBuf> {
    let rel = match entry_path.strip_prefix(skills_root) {
        Ok(r) if !r.as_os_str().is_empty() => r.to_path_buf(),
        _ => entry_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_default(),
    };
    if rel.as_os_str().is_empty()
        || rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::new(
            code::PATH_TRAVERSAL,
            format!(
                "非法 skill store 相对路径: {} (skills_root={})",
                entry_path.display(),
                skills_root.display()
            ),
        ));
    }
    Ok(rel)
}

fn base64url_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = data.len() - i;
    if rem == 1 {
        let n = (data[i] as u32) << 16;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
    } else if rem == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
    }
    out
}

fn base64url_decode(s: &str) -> std::result::Result<Vec<u8>, String> {
    fn val(c: u8) -> std::result::Result<u32, String> {
        Ok(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(format!("invalid base64url char: {}", c as char)),
        })
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let n = (val(bytes[i])? << 18)
            | (val(bytes[i + 1])? << 12)
            | (val(bytes[i + 2])? << 6)
            | val(bytes[i + 3])?;
        out.push(((n >> 16) & 0xff) as u8);
        out.push(((n >> 8) & 0xff) as u8);
        out.push((n & 0xff) as u8);
        i += 4;
    }
    let rem = bytes.len() - i;
    if rem == 2 {
        let n = (val(bytes[i])? << 18) | (val(bytes[i + 1])? << 12);
        out.push(((n >> 16) & 0xff) as u8);
    } else if rem == 3 {
        let n = (val(bytes[i])? << 18) | (val(bytes[i + 1])? << 12) | (val(bytes[i + 2])? << 6);
        out.push(((n >> 16) & 0xff) as u8);
        out.push(((n >> 8) & 0xff) as u8);
    } else if rem == 1 {
        return Err("truncated base64url".into());
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMeta {
    pub identity: String,
    pub key: String,
}

/// Skill 实体目录：`<store>/<key>/<相对 skills 根>/`
pub fn skill_entity_dir(store_root: &Path, identity: &str, skill_rel: &Path) -> PathBuf {
    source_bucket(store_root, identity).join(skill_rel)
}

pub fn write_source_meta(store_root: &Path, identity: &str) -> Result<()> {
    let norm = normalize_identity(identity);
    let key = source_key(&norm);
    let meta_dir = source_bucket(store_root, identity).join(".meta");
    std::fs::create_dir_all(&meta_dir)?;
    let meta = SourceMeta {
        identity: norm,
        key,
    };
    crate::sync_common::atomic_write(
        &meta_dir.join("SOURCE.json"),
        serde_json::to_vec_pretty(&meta)?.as_slice(),
    )
}

/// 将源仓中的 skill 目录同步到 store 实体目录；返回实体绝对路径与内容摘要。
pub fn materialize_skill_dir(
    store_root: &Path,
    identity: &str,
    skill_rel: &Path,
    src_dir: &Path,
) -> Result<(PathBuf, String)> {
    if !src_dir.is_dir() {
        return Err(Error::new(
            code::RENDER_FAILED,
            format!("技能源目录不存在: {}", src_dir.display()),
        ));
    }
    write_source_meta(store_root, identity)?;
    let dest = skill_entity_dir(store_root, identity, skill_rel);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| {
            Error::new(
                code::WRITE_FAILED,
                format!("无法清理 store 旧实体 {}: {e}", dest.display()),
            )
        })?;
    }
    copy_dir_recursive(src_dir, &dest)?;
    let digest = dir_digest(&dest)?;
    Ok((dest, digest))
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let rel = path.strip_prefix(src).unwrap();
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(path, &target).map_err(|e| {
                Error::new(
                    code::WRITE_FAILED,
                    format!(
                        "复制到 store 失败 {} → {}: {e}",
                        path.display(),
                        target.display()
                    ),
                )
            })?;
        }
    }
    Ok(())
}

pub fn dir_digest(dir: &Path) -> Result<String> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();
    files.sort();
    let mut buf = Vec::new();
    for f in files {
        let rel = f.strip_prefix(dir).unwrap().to_string_lossy();
        buf.extend_from_slice(rel.as_bytes());
        buf.push(0);
        buf.extend_from_slice(
            &std::fs::read(&f)
                .map_err(|e| Error::new(code::RENDER_FAILED, format!("读 store 文件失败: {e}")))?,
        );
        buf.push(0);
    }
    Ok(format!("sha256:{}", sha256_hex(&buf)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_git_ssh_and_https_same_key() {
        let a = normalize_identity("git@github.com:Org/Repo.git");
        let b = normalize_identity("https://github.com/Org/Repo.git");
        assert_eq!(a, b);
        assert_eq!(a, "https://github.com/org/repo");
        assert_eq!(source_key(&a), source_key(&b));
    }

    #[test]
    fn source_key_roundtrip() {
        let id = "https://github.com/acme/skills";
        let key = source_key(id);
        let back = source_key_decode(&key).unwrap();
        assert_eq!(back, normalize_identity(id));
    }

    #[test]
    fn layout_is_key_then_skill_under_skills_root() {
        let root = PathBuf::from("/tmp/ailoom-store-test");
        let id = "https://github.com/acme/skills";
        let skills_root = Path::new("resources/skills");
        let entry = Path::new("resources/skills/inking/line-art");
        let rel = skill_rel_under_root(entry, skills_root).unwrap();
        assert_eq!(rel, PathBuf::from("inking/line-art"));
        let p = skill_entity_dir(&root, id, &rel);
        let s = p.to_string_lossy();
        let key = "github.com/acme/skills/revisions/working";
        assert!(!s.contains("/sources/"));
        assert_eq!(p, root.join(&key).join("inking/line-art"));
        assert_eq!(
            source_bucket(&root, id).join(".meta").join("SOURCE.json"),
            root.join(&key).join(".meta").join("SOURCE.json")
        );
    }

    #[test]
    fn readable_git_identity_keeps_version_and_contains_traversal() {
        let root = Path::new("/tmp/store");
        assert_eq!(
            source_bucket(root, "git+https://github.com/acme/skills.git#abc123"),
            root.join("github.com/acme/skills/revisions/abc123")
        );
        assert_ne!(
            source_bucket(root, "git+https://github.com/acme/skills#v1"),
            source_bucket(root, "git+https://github.com/acme/skills#v2")
        );
        let p = source_bucket(root, "https://example.com/../../evil#../bad");
        assert!(p.starts_with(root));
        assert!(!p
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)));
        assert_ne!(
            source_bucket(root, "/a/skills"),
            source_bucket(root, "/b/skills")
        );
    }
}
