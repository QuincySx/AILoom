//! 机器数据根与工作区数据布局（契约 §3）。所有根目录可注入。
//!
//! 解析优先级（高 → 低）：
//! 1. CLI `--data-root`（仅数据根）
//! 2. 自有环境变量 `AILOOM_DATA_ROOT` / `AILOOM_STORE_ROOT`
//! 3. 已显式设置的 XDG 变量（`XDG_STATE_HOME` / `XDG_DATA_HOME`）
//! 4. 用户主目录下的 `~/.ailoom…`
//!
//! 未设置的 XDG 变量**不会**自动落到规范默认路径（如 `~/.local/state`）；
//! 只有变量本身非空时才采用 XDG。

use crate::error::{code, Error, Result};
use std::path::{Path, PathBuf};

/// 解析机器数据根：`--data-root` > `AILOOM_DATA_ROOT` > `XDG_STATE_HOME/ailoom` > `~/.ailoom`。
pub fn resolve_data_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Some(env_root) = non_empty_env("AILOOM_DATA_ROOT") {
        return Ok(PathBuf::from(env_root));
    }
    if let Some(xdg) = non_empty_env("XDG_STATE_HOME") {
        return Ok(PathBuf::from(xdg).join("ailoom"));
    }
    user_home()
        .map(|home| home.join(".ailoom"))
        .ok_or_else(|| {
            Error::new(
                code::REFUSE_GLOBAL_WRITE,
                "无法确定机器数据根目录：未设置 --data-root / AILOOM_DATA_ROOT / XDG_STATE_HOME，且无法解析 HOME",
            )
            .fix("使用 --data-root <DIR>，或设置 AILOOM_DATA_ROOT / XDG_STATE_HOME / HOME")
        })
}

/// 解析 SkillStore 根：`AILOOM_STORE_ROOT` > `XDG_DATA_HOME/ailoom/store` > `~/.ailoom/store`。
pub fn resolve_store_root() -> Result<PathBuf> {
    if let Some(env_root) = non_empty_env("AILOOM_STORE_ROOT") {
        return Ok(PathBuf::from(env_root));
    }
    if let Some(xdg) = non_empty_env("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join("ailoom").join("store"));
    }
    user_home()
        .map(|home| home.join(".ailoom").join("store"))
        .ok_or_else(|| {
            Error::new(
                code::REFUSE_GLOBAL_WRITE,
                "无法确定 SkillStore 根目录：未设置 AILOOM_STORE_ROOT / XDG_DATA_HOME，且无法解析 HOME",
            )
            .fix("设置 AILOOM_STORE_ROOT / XDG_DATA_HOME / HOME")
        })
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    })
}

fn user_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home));
    }
    std::env::var_os("USERPROFILE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        prev: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let prev = std::env::var_os(key);
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

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
    fn explicit_root_wins_over_env_and_xdg() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", Some("/tmp/env-root"));
        let _x = EnvGuard::set("XDG_STATE_HOME", Some("/tmp/xdg-state"));
        let p = resolve_data_root(Some(Path::new("/tmp/explicit"))).unwrap();
        assert_eq!(p, Path::new("/tmp/explicit"));
    }

    #[test]
    fn ailoom_data_root_beats_xdg_and_home() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", Some("/tmp/ailoom-data"));
        let _x = EnvGuard::set("XDG_STATE_HOME", Some("/tmp/xdg-state"));
        let _h = EnvGuard::set("HOME", Some("/tmp/fake-home"));
        let p = resolve_data_root(None).unwrap();
        assert_eq!(p, Path::new("/tmp/ailoom-data"));
    }

    #[test]
    fn xdg_state_home_beats_home_when_ailoom_unset() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", None);
        let _x = EnvGuard::set("XDG_STATE_HOME", Some("/tmp/xdg-state"));
        let _h = EnvGuard::set("HOME", Some("/tmp/fake-home"));
        let p = resolve_data_root(None).unwrap();
        assert_eq!(p, Path::new("/tmp/xdg-state/ailoom"));
    }

    #[test]
    fn home_dot_ailoom_when_no_ailoom_or_xdg() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", None);
        let _x = EnvGuard::set("XDG_STATE_HOME", None);
        let _h = EnvGuard::set("HOME", Some("/tmp/fake-home"));
        let p = resolve_data_root(None).unwrap();
        assert_eq!(p, Path::new("/tmp/fake-home/.ailoom"));
    }

    #[test]
    fn empty_xdg_does_not_count_as_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", None);
        let _x = EnvGuard::set("XDG_STATE_HOME", Some("   "));
        let _h = EnvGuard::set("HOME", Some("/tmp/fake-home"));
        let p = resolve_data_root(None).unwrap();
        assert_eq!(p, Path::new("/tmp/fake-home/.ailoom"));
    }

    #[test]
    fn store_priority_ailoom_then_xdg_then_home() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _s = EnvGuard::set("AILOOM_STORE_ROOT", Some("/tmp/store-override"));
        let _x = EnvGuard::set("XDG_DATA_HOME", Some("/tmp/xdg-data"));
        let _h = EnvGuard::set("HOME", Some("/tmp/fake-home"));
        assert_eq!(
            resolve_store_root().unwrap(),
            Path::new("/tmp/store-override")
        );

        drop(_s);
        let _s = EnvGuard::set("AILOOM_STORE_ROOT", None);
        assert_eq!(
            resolve_store_root().unwrap(),
            Path::new("/tmp/xdg-data/ailoom/store")
        );

        drop(_x);
        let _x = EnvGuard::set("XDG_DATA_HOME", None);
        assert_eq!(
            resolve_store_root().unwrap(),
            Path::new("/tmp/fake-home/.ailoom/store")
        );
    }
}
