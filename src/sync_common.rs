//! 跨卡共享的文件操作原语：临时文件暂存 + 同文件系统 rename 的原子写。

use crate::error::{code, Error, Result};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// 进程内写序号：并发原子写同一目标时，临时文件名必须互不相同，
/// 否则先完成的 rename 会“偷走”后一个请求的 tmp 文件（ENOENT）。
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// 预览只在扫描后确认时使用；超过这个时间的预览文件在写新预览时清理。
pub const PREVIEW_RETENTION: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

/// 写入一份预览（`<dir>/<id>.json`），并清理同目录下过期的预览文件。
pub fn write_preview(dir: &Path, id: &str, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    if let Some(cutoff) = std::time::SystemTime::now().checked_sub(PREVIEW_RETENTION) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let stale = path.extension().is_some_and(|e| e == "json")
                && entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t < cutoff);
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    atomic_write(&dir.join(format!("{id}.json")), bytes)
}

/// 原子写：先写同目录临时文件并 flush+sync，再 rename 覆盖目标。
pub fn atomic_write(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target.parent().ok_or_else(|| {
        Error::new(
            code::INTERNAL,
            format!("目标缺少父目录: {}", target.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let tmp = parent.join(format!(
        ".{}.tmp-{}-{}-{}",
        target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into()),
        std::process::id(),
        seq,
        nanos
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::new(code::WRITE_FAILED, format!("原子替换失败: {e}"))
            .context(serde_json::json!({ "target": target.display().to_string() }))
    })
}

/// 删除目录树，但拒绝删除明显危险的根（盘根、HOME、空路径）。
pub fn remove_dir_all_guarded(target: &Path) -> Result<()> {
    let bad = target.as_os_str().is_empty()
        || target.parent().is_none()
        || std::env::var_os("HOME")
            .map(|h| target == std::path::Path::new(&h))
            .unwrap_or(false);
    if bad {
        return Err(Error::new(
            code::INTERNAL,
            format!("拒绝删除可疑目录: {}", target.display()),
        ));
    }
    if target.exists() {
        std::fs::remove_dir_all(target)?;
    }
    Ok(())
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn writing_a_preview_prunes_only_expired_preview_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("previews");
        std::fs::create_dir_all(&dir).unwrap();
        let old =
            std::time::SystemTime::now() - PREVIEW_RETENTION - std::time::Duration::from_secs(60);
        for name in ["expired.json", "expired.lock"] {
            let file = std::fs::File::create(dir.join(name)).unwrap();
            file.set_modified(old).unwrap();
        }
        std::fs::write(dir.join("recent.json"), b"{}").unwrap();
        write_preview(&dir, "new", b"{}").unwrap();
        assert!(!dir.join("expired.json").exists());
        assert!(dir.join("expired.lock").exists(), "只清理预览文件");
        assert!(dir.join("recent.json").exists());
        assert!(dir.join("new.json").exists());
    }
}
