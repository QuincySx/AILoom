//! 本地目录源（AIL-004）：无锁定 commit，内容摘要标识当前状态并声明可变性。

use super::Snapshot;
use crate::error::Result;
use crate::gitx;
use crate::ids::now_iso;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocalSource {
    pub identity: String,
    pub path: PathBuf,
}

impl LocalSource {
    pub fn new(path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let identity = format!(
            "local+{}",
            gitx::normalize_remote_url(&path.to_string_lossy())
        );
        Ok(LocalSource { identity, path })
    }

    /// 本地源不落缓存：直接以源目录为只读视图；可变性由 mutable 标记。
    /// 说明可变性：摘要只代表此刻内容，两次 resolve 可能不同。
    pub fn resolve(&self) -> Result<Snapshot> {
        if !self.path.is_dir() {
            return Err(crate::error::Error::new(
                crate::error::code::SOURCE_CACHE_CORRUPT,
                format!("本地源目录不存在: {}", self.path.display()),
            ));
        }
        let digest = super::tree_digest(&self.path)?;
        Ok(Snapshot {
            identity: self.identity.clone(),
            resolved_commit: None,
            content_digest: digest,
            root: self.path.clone(),
            ref_: None,
            locked_at: now_iso(),
            mutable: true,
        })
    }
}
