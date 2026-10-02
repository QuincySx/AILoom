//! 资源源抽象：Git 源与本地目录源（AIL-004）。
//! 快照只读；缓存按规范化源身份隔离；锁定版本只有显式 refresh 才前进。

pub mod git;
pub mod local;

pub use git::GitSource;
pub use local::LocalSource;

use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// 规范化源身份（缓存 key 输入）
    pub identity: String,
    /// Git 源为 resolved commit；本地源为 None
    pub resolved_commit: Option<String>,
    /// 快照内容树摘要
    pub content_digest: String,
    /// 只读快照根目录（Git 源=缓存快照；本地源=源目录本身）
    pub root: PathBuf,
    pub ref_: Option<String>,
    pub locked_at: String,
    /// 本地源目录可变，内容可能与摘要不一致
    pub mutable: bool,
}

/// 源锁文件 `.ailoom/machine/sources.lock.json`（契约 §4.5）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcesLock {
    pub schema_version: u32,
    pub sources: BTreeMap<String, SourceLock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLock {
    #[serde(rename = "type")]
    pub kind: String,
    pub identity: String,
    /// 契约 §4.5 字段名是 `ref`；早期版本写成 `ref_`，读取时兼容
    #[serde(
        rename = "ref",
        alias = "ref_",
        skip_serializing_if = "Option::is_none"
    )]
    pub ref_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    pub content_digest: String,
    pub locked_at: String,
}

impl SourcesLock {
    pub fn new() -> Self {
        SourcesLock {
            schema_version: 1,
            sources: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Option<SourcesLock>> {
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| Error::new(code::SOURCE_CACHE_CORRUPT, format!("锁文件不可读: {e}")))?;
        let lock: SourcesLock = serde_json::from_str(&text).map_err(|e| {
            Error::new(
                code::SOURCE_CACHE_CORRUPT,
                format!("锁文件损坏（拒绝猜测修复，可删除后重新 init）: {e}"),
            )
            .context(serde_json::json!({ "path": path.display().to_string() }))
        })?;
        if lock.schema_version != 1 {
            return Err(Error::new(
                code::SCHEMA_VERSION,
                format!("不支持的锁文件 schema_version: {}", lock.schema_version),
            ));
        }
        Ok(Some(lock))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        crate::sync_common::atomic_write(path, json.as_bytes())?;
        Ok(())
    }
}

impl Default for SourcesLock {
    fn default() -> Self {
        Self::new()
    }
}

/// 快照内容树摘要：按相对路径排序，逐文件 hash(rel_path \0 content)。
/// 跳过 `.git` 与快照标记文件本身。
pub fn tree_digest(root: &Path) -> Result<String> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .map_err(|e| Error::new(code::INTERNAL, format!("walkdir 前缀错误: {e}")))?;
        if rel.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        if rel == std::path::Path::new(git::SNAPSHOT_MARKER) {
            continue;
        }
        paths.push(rel.to_path_buf());
    }
    paths.sort();
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    for rel in paths {
        let content = std::fs::read(root.join(&rel))
            .map_err(|e| Error::new(code::SOURCE_CACHE_CORRUPT, format!("快照文件不可读: {e}")))?;
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update([0u8]);
        hasher.update(&content);
    }
    Ok(format!("sha256:{}", crate::ids::hex(&hasher.finalize())))
}

/// 带互斥文件锁执行闭包（用于并发 fetch 串行化）。
pub fn with_file_lock<T>(lock_path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => break,
            Err(_) => {
                if std::time::Instant::now() > deadline {
                    return Err(Error::new(
                        code::LOCK_HELD,
                        format!("等待源锁超时: {}", lock_path.display()),
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    let result = f();
    let _ = fs2::FileExt::unlock(&file);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_digest_is_order_stable_and_content_sensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir_all(a.join("sub")).unwrap();
        std::fs::write(a.join("sub/f.txt"), "hello").unwrap();
        std::fs::write(a.join("top.md"), "# t").unwrap();
        let d1 = tree_digest(&a).unwrap();

        let b = tmp.path().join("b");
        std::fs::create_dir_all(b.join("sub")).unwrap();
        std::fs::write(b.join("top.md"), "# t").unwrap();
        std::fs::write(b.join("sub/f.txt"), "hello").unwrap();
        let d2 = tree_digest(&b).unwrap();
        assert_eq!(d1, d2, "摘要与文件创建顺序无关");

        std::fs::write(b.join("sub/f.txt"), "changed").unwrap();
        let d3 = tree_digest(&b).unwrap();
        assert_ne!(d2, d3, "内容变化必须改变摘要");
    }

    #[test]
    fn corrupted_lock_file_is_reported_not_repaired() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("sources.lock.json");
        std::fs::write(&p, "{ not json").unwrap();
        let err = SourcesLock::load(&p).unwrap_err();
        assert_eq!(err.code, code::SOURCE_CACHE_CORRUPT);
    }
}
