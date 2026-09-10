//! 跨卡共享的文件操作原语：临时文件暂存 + 同文件系统 rename 的原子写。

use crate::error::{code, Error, Result};
use std::io::Write;
use std::path::Path;

/// 原子写：先写同目录临时文件并 flush+sync，再 rename 覆盖目标。
pub fn atomic_write(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target.parent().ok_or_else(|| {
        Error::new(
            code::INTERNAL,
            format!("目标缺少父目录: {}", target.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into()),
        std::process::id()
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
