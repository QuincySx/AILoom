//! 工作区级同步锁（AIL-008）：owner 记录、flock 互斥、释放校验 owner。

use crate::error::{code, Error, Result};
use crate::ids::now_iso;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockOwner {
    pub pid: u32,
    pub device_id: String,
    pub created_at: String,
}

pub struct SyncLock {
    path: PathBuf,
    file: std::fs::File,
    owner: LockOwner,
}

impl std::fmt::Debug for SyncLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncLock")
            .field("path", &self.path)
            .field("owner", &self.owner)
            .finish()
    }
}

impl SyncLock {
    /// 尝试获取（非阻塞）。已被他人持有 → E4003。
    pub fn acquire(lock_dir: &Path, device_id: &str) -> Result<SyncLock> {
        std::fs::create_dir_all(lock_dir)?;
        let path = lock_dir.join("sync.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(&path)?;
        file.try_lock_exclusive().map_err(|_| {
            let holder = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<LockOwner>(&t).ok());
            Error::new(code::LOCK_HELD, "工作区同步锁被其他进程持有")
                .context(
                    serde_json::json!({ "lock": path.display().to_string(), "holder": holder }),
                )
                .fix("等待其它 ailoom 进程完成，或确认其已退出后删除锁文件")
        })?;
        let owner = LockOwner {
            pid: std::process::id(),
            device_id: device_id.to_string(),
            created_at: now_iso(),
        };
        std::fs::write(&path, serde_json::to_vec_pretty(&owner)?)?;
        Ok(SyncLock { path, file, owner })
    }

    pub fn owner(&self) -> &LockOwner {
        &self.owner
    }

    /// 释放：校验锁文件内容仍是本次 owner，防止删除后来者的锁。
    pub fn release(self) -> Result<()> {
        let result = (|| -> Result<()> {
            let current = std::fs::read_to_string(&self.path)?;
            let owner: LockOwner = serde_json::from_str(&current)
                .map_err(|e| Error::new(code::INTERNAL, format!("锁文件损坏: {e}")))?;
            if owner.pid != self.owner.pid || owner.device_id != self.owner.device_id {
                return Err(
                    Error::new(code::LOCK_HELD, "锁文件已被其他 owner 覆盖，拒绝删除").context(
                        serde_json::json!({
                            "mine": self.owner,
                            "current": owner,
                        }),
                    ),
                );
            }
            let _ = std::fs::remove_file(&self.path);
            Ok(())
        })();
        let _ = fs2::FileExt::unlock(&self.file);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_fails_while_held() {
        let dir = tempfile::tempdir().unwrap();
        let l1 = SyncLock::acquire(dir.path(), "dev1").unwrap();
        let err = SyncLock::acquire(dir.path(), "dev2").unwrap_err();
        assert_eq!(err.code, "E4003");
        l1.release().unwrap();
        let l2 = SyncLock::acquire(dir.path(), "dev2").unwrap();
        l2.release().unwrap();
    }

    #[test]
    fn release_refuses_when_owner_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let l1 = SyncLock::acquire(dir.path(), "dev1").unwrap();
        // 模拟新 owner 覆盖锁内容
        let new_owner = LockOwner {
            pid: 999999,
            device_id: "dev2".into(),
            created_at: now_iso(),
        };
        let path = dir.path().join("sync.lock");
        std::fs::write(&path, serde_json::to_vec_pretty(&new_owner).unwrap()).unwrap();
        let err = l1.release().unwrap_err();
        assert_eq!(err.code, "E4003");
        // 新 owner 的锁文件仍在
        assert!(path.exists(), "旧进程释放不能删新 owner 锁");
    }
}
