//! 机器数据根与工作区数据布局（契约 §3）。所有根目录可注入。

use crate::error::{code, Error, Result};
use std::path::{Path, PathBuf};

/// 解析机器数据根：显式参数 > AILOOM_DATA_ROOT > 平台默认。
/// 默认实现不触碰 HOME 之外的目录；测试必须显式注入。
pub fn resolve_data_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(env_root) = std::env::var("AILOOM_DATA_ROOT") {
        if !env_root.is_empty() {
            return Ok(PathBuf::from(env_root));
        }
    }
    home_data_root().ok_or_else(|| {
        Error::new(
            code::REFUSE_GLOBAL_WRITE,
            "无法确定机器数据根目录，且未显式指定 --data-root/AILOOM_DATA_ROOT",
        )
        .fix("使用 --data-root <DIR> 显式指定")
    })
}

fn home_data_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let home = PathBuf::from(home);
    #[cfg(target_os = "macos")]
    {
        Some(
            home.join("Library")
                .join("Application Support")
                .join("ailoom"),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("ailoom"));
            }
        }
        Some(home.join(".local").join("state").join("ailoom"))
    }
}

/// 工作区级数据布局。root/cache/<anchor_key> 为仓库共享；ws/<id> 为工作区私有。
#[derive(Debug, Clone)]
pub struct WsLayout {
    pub data_root: PathBuf,
    pub cache_root: PathBuf,
    pub ws_dir: PathBuf,
    pub binding_path: PathBuf,
    pub managed_manifest_path: PathBuf,
    pub journal_dir: PathBuf,
    pub events_dir: PathBuf,
    pub events_file: PathBuf,
    pub index_dir: PathBuf,
    pub summary_dir: PathBuf,
}

pub fn layout_for(data_root: &Path, workspace_id: &str, anchor_key: &str) -> WsLayout {
    let cache_root = data_root.join("cache").join(anchor_key);
    let ws_dir = data_root.join("ws").join(workspace_id);
    let events_dir = ws_dir.join("events");
    let binding_path = ws_dir.join("binding.json");
    let managed_manifest_path = ws_dir.join("managed-manifest.json");
    let journal_dir = ws_dir.join("journal");
    let events_file = events_dir.join("events.jsonl");
    let index_dir = ws_dir.join("index");
    let summary_dir = ws_dir.join("summary");
    WsLayout {
        data_root: data_root.to_path_buf(),
        cache_root,
        ws_dir,
        binding_path,
        managed_manifest_path,
        journal_dir,
        events_dir,
        events_file,
        index_dir,
        summary_dir,
    }
}

/// 确保布局中的目录存在（只创建 AILoom 自己的数据目录）。
pub fn ensure_layout(layout: &WsLayout) -> Result<()> {
    for dir in [
        &layout.cache_root,
        &layout.ws_dir,
        &layout.journal_dir,
        &layout.events_dir,
        &layout.index_dir,
        &layout.summary_dir,
    ] {
        std::fs::create_dir_all(dir).map_err(|e| {
            Error::new(
                code::INTERNAL,
                format!("无法创建数据目录 {}: {e}", dir.display()),
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_partitions_by_anchor_and_workspace() {
        let root = Path::new("/tmp/dr");
        let l1 = layout_for(root, "aaaa", "ck1");
        let l2 = layout_for(root, "bbbb", "ck1");
        let l3 = layout_for(root, "aaaa", "ck2");
        assert_eq!(l1.cache_root, l2.cache_root, "同 anchor 共享缓存");
        assert_ne!(l1.ws_dir, l2.ws_dir, "不同 workspace 分区隔离");
        assert_ne!(l1.cache_root, l3.cache_root, "不同 anchor 缓存隔离");
    }

    #[test]
    fn explicit_root_wins_over_env() {
        std::env::set_var("AILOOM_DATA_ROOT", "/tmp/env-root");
        let p = resolve_data_root(Some(Path::new("/tmp/explicit"))).unwrap();
        assert_eq!(p, Path::new("/tmp/explicit"));
        std::env::remove_var("AILOOM_DATA_ROOT");
    }
}
