//! 机器数据根与工作区数据布局（契约 §3，v1.1）。所有根目录可注入。
//!
//! 解析优先级（高 → 低）：
//! 1. CLI `--data-root`（仅数据根）
//! 2. 已显式设置的 XDG 变量（`XDG_STATE_HOME` / `XDG_DATA_HOME`；空值不算已设）
//! 3. 自有环境变量 `AILOOM_DATA_ROOT` / `AILOOM_STORE_ROOT`
//! 4. XDG 规范默认：`$HOME/.local/state/ailoom`（数据）、`$HOME/.local/share/ailoom/store`（Store）
//!
//! 兼容迁移（v1.1，v1.2 补充可达性）：旧默认 `~/.ailoom` 存在时，首次解析一次性把
//! `store` → `~/.local/share/ailoom/store`、`ws`/`cache` → `~/.local/state/ailoom`
//! 拆分迁移（其余内容如安装器的 `bin/` 不动）；`device-id` 复制到新数据根，
//! 设备身份跨升级连续。迁移成功后在旧 `store` 位置留一个指向新位置的兼容符号链接，
//! 使既有工作区里指向旧 store 的绝对 Skill 链接无需重新 sync 仍可达（RW-01/S01）。
//! 任何一步失败则整体回滚（含清理本次新建的空目录，避免下次误选空新根）、
//! 继续使用旧目录并告警——数据不可达优于静默孤儿。

use crate::error::{code, Error, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// 解析机器数据根：
/// `--data-root` > `$XDG_STATE_HOME/ailoom`（已设时）> `AILOOM_DATA_ROOT`
/// > 规范默认 `$HOME/.local/state/ailoom`（含旧 `~/.ailoom` 一次性迁移）。
pub fn resolve_data_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Some(xdg) = non_empty_env("XDG_STATE_HOME") {
        return Ok(PathBuf::from(xdg).join("ailoom"));
    }
    if let Some(env_root) = non_empty_env("AILOOM_DATA_ROOT") {
        return Ok(PathBuf::from(env_root));
    }
    default_data_root()
}

/// 解析 SkillStore 根：
/// `$XDG_DATA_HOME/ailoom/store`（已设时）> `AILOOM_STORE_ROOT`
/// > 规范默认 `$HOME/.local/share/ailoom/store`（含旧 `~/.ailoom/store` 一次性迁移）。
pub fn resolve_store_root() -> Result<PathBuf> {
    if let Some(xdg) = non_empty_env("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join("ailoom").join("store"));
    }
    if let Some(env_root) = non_empty_env("AILOOM_STORE_ROOT") {
        return Ok(PathBuf::from(env_root));
    }
    let home = user_home().ok_or_else(|| no_home_error("AILOOM_STORE_ROOT / XDG_DATA_HOME"))?;
    let migrated = ensure_legacy_migrated(&home);
    if !migrated {
        // 迁移失败回退：数据与 Store 保持同一个旧根，保证一致性
        return Ok(home.join(".ailoom").join("store"));
    }
    Ok(home
        .join(".local")
        .join("share")
        .join("ailoom")
        .join("store"))
}

fn default_data_root() -> Result<PathBuf> {
    let home = user_home()
        .ok_or_else(|| no_home_error("--data-root / AILOOM_DATA_ROOT / XDG_STATE_HOME"))?;
    let migrated = ensure_legacy_migrated(&home);
    if !migrated {
        // 迁移失败回退：继续使用旧根（数据保持可达）
        return Ok(home.join(".ailoom"));
    }
    Ok(home.join(".local").join("state").join("ailoom"))
}

fn no_home_error(overrides: &str) -> Error {
    Error::new(
        code::REFUSE_GLOBAL_WRITE,
        format!("无法确定机器数据根目录：未设置 {overrides}，且无法解析 HOME"),
    )
    .fix("使用 --data-root <DIR>，或设置 XDG_STATE_HOME / AILOOM_DATA_ROOT / HOME")
}

/// 旧默认目录一次性迁移（每进程至多执行一次；幂等）：
/// `~/.ailoom/store` → `~/.local/share/ailoom/store`，
/// `~/.ailoom/{ws,cache}` → `~/.local/state/ailoom/`；
/// 其余内容（如安装器的 `bin/`）原地保留。任一步失败回滚已移动部分，
/// 继续使用 `~/.ailoom` 并告警。
static LEGACY_MIGRATION_DONE: AtomicBool = AtomicBool::new(false);
/// true = 迁移失败，旧根 `~/.ailoom` 仍是权威位置（数据与 Store 都回退旧目录）
static FALLBACK_TO_LEGACY: AtomicBool = AtomicBool::new(false);

/// 返回 true = 旧目录不再是权威位置（已迁移/无旧目录/新旧并存时以新默认为准）。
fn ensure_legacy_migrated(home: &Path) -> bool {
    if LEGACY_MIGRATION_DONE.swap(true, Ordering::SeqCst) {
        return !FALLBACK_TO_LEGACY.load(Ordering::SeqCst);
    }
    let legacy = home.join(".ailoom");
    if !legacy.is_dir() {
        return true; // 无旧目录：直接使用规范默认
    }
    let state = home.join(".local").join("state").join("ailoom");
    let share = home.join(".local").join("share").join("ailoom");
    // 上次迁移留下的兼容符号链接（RW-01）= 已迁移完成，静默采用新默认
    if is_symlink(&legacy.join("store")) && (state.is_dir() || share.join("store").is_dir()) {
        return true;
    }
    if state.is_dir() || share.join("store").is_dir() {
        // 真正的新旧并存：仍要保住设备身份（旧 device-id 尚未带到新根时补上）
        copy_device_id(&legacy, &state);
        crate::logging::warn(format!(
            "检测到旧默认 {} 与新规范默认并存：继续使用新默认，旧目录未改动（可手动迁移后删除）",
            legacy.display()
        ));
        return true;
    }
    // 逐个子目录移动，任何失败回滚全部已移动部分并清掉本次新建的空目录
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new(); // (目标, 来源)
    let mut created: Vec<PathBuf> = Vec::new(); // 本次 create_dir_all 新建的根
    let mut store_moved = false;
    let plan: Vec<(&str, &Path)> = vec![
        ("store", share.as_path()),
        ("ws", state.as_path()),
        ("cache", state.as_path()),
    ];
    for (sub, target_base) in plan {
        let src = legacy.join(sub);
        if !src.is_dir() {
            continue;
        }
        let dst_parent = target_base;
        let dst = dst_parent.join(sub);
        if fs_create_dir_new(dst_parent, &mut created).is_err() || fs_rename(&src, &dst).is_err() {
            rollback(&moved, &created);
            FALLBACK_TO_LEGACY.store(true, Ordering::SeqCst);
            crate::logging::warn(format!(
                "旧默认目录 {} 迁移到 XDG 规范位置失败：继续使用旧目录（数据保持可达）",
                legacy.display()
            ));
            return false;
        }
        if sub == "store" {
            store_moved = true;
        }
        moved.push((dst, src));
    }
    // 设备身份随迁：旧 device-id 复制到新数据根（目标已存在则不覆盖）
    copy_device_id(&legacy, &state);
    // 兼容符号链接：旧 store 位置 → 新位置，既有绝对链接无需重新 sync 仍可达
    if store_moved {
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(share.join("store"), legacy.join("store")).is_err() {
                rollback(&moved, &created);
                FALLBACK_TO_LEGACY.store(true, Ordering::SeqCst);
                crate::logging::warn(format!(
                    "旧默认目录 {} 迁移后建立兼容链接失败：整体回退，继续使用旧目录",
                    legacy.display()
                ));
                return false;
            }
        }
    }
    if moved.is_empty() {
        return true; // 旧目录存在但没有可迁移的契约数据（如只有 bin/）
    }
    crate::logging::info(format!(
        "已把旧默认 {} 的 store/ws/cache 一次性迁移到 XDG 规范位置（~/.local/share/ailoom、~/.local/state/ailoom），旧 store 位置保留兼容链接",
        legacy.display()
    ));
    true
}

fn is_symlink(p: &Path) -> bool {
    std::fs::symlink_metadata(p)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// 设备身份跨升级连续（RW-01/S01）：`<legacy>/device-id` → `<state>/device-id`。
/// 只在目标缺失时复制（copy 而非 move：迁移回退时旧根仍持有原身份）。
fn copy_device_id(legacy: &Path, state: &Path) {
    let src = legacy.join("device-id");
    let dst = state.join("device-id");
    if src.is_file() && !dst.exists() && fs_create_dir_all(state).is_ok() {
        if let Ok(bytes) = std::fs::read(&src) {
            let _ = std::fs::write(&dst, bytes);
        }
    }
}

fn rollback(moved: &[(PathBuf, PathBuf)], created: &[PathBuf]) {
    for (dst, src) in moved.iter().rev() {
        let _ = fs_rename(dst, src);
    }
    // 清掉本次迁移新建的（此时应为空的）目录，避免下次启动把空新根当成并存并采用
    for dir in created.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
}

fn fs_create_dir_all(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)
}

/// create_dir_all，并记录本次新建的目录（回滚时清理，仅删空目录）。
fn fs_create_dir_new(p: &Path, created: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !p.exists() {
        created.push(p.to_path_buf());
    }
    std::fs::create_dir_all(p)
}

fn fs_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
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
pub(crate) fn reset_migration_state_for_test() {
    LEGACY_MIGRATION_DONE.store(false, Ordering::SeqCst);
    FALLBACK_TO_LEGACY.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

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
    fn explicit_root_wins_over_xdg_and_env() {
        let _lock = lock();
        let _x = EnvGuard::set("XDG_STATE_HOME", Some("/tmp/xdg-state"));
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", Some("/tmp/env-root"));
        let p = resolve_data_root(Some(Path::new("/tmp/explicit"))).unwrap();
        assert_eq!(p, Path::new("/tmp/explicit"));
    }

    /// v1.1：已设 XDG 变量高于自有 AILOOM_*。
    #[test]
    fn xdg_beats_ailoom_env() {
        let _lock = lock();
        let _x = EnvGuard::set("XDG_STATE_HOME", Some("/tmp/xdg-state"));
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", Some("/tmp/ailoom-data"));
        let _h = EnvGuard::set("HOME", Some("/tmp/fake-home"));
        let p = resolve_data_root(None).unwrap();
        assert_eq!(p, Path::new("/tmp/xdg-state/ailoom"));

        let _xd = EnvGuard::set("XDG_DATA_HOME", Some("/tmp/xdg-data"));
        let _s = EnvGuard::set("AILOOM_STORE_ROOT", Some("/tmp/ailoom-store"));
        assert_eq!(
            resolve_store_root().unwrap(),
            Path::new("/tmp/xdg-data/ailoom/store")
        );
    }

    /// XDG 未设（或为空）时用自有 AILOOM_*。
    #[test]
    fn ailoom_env_used_when_xdg_unset() {
        let _lock = lock();
        let _x = EnvGuard::set("XDG_STATE_HOME", None);
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", Some("/tmp/ailoom-data"));
        let _h = EnvGuard::set("HOME", Some("/tmp/fake-home"));
        assert_eq!(
            resolve_data_root(None).unwrap(),
            Path::new("/tmp/ailoom-data")
        );

        let _xd = EnvGuard::set("XDG_DATA_HOME", None);
        let _s = EnvGuard::set("AILOOM_STORE_ROOT", Some("/tmp/ailoom-store"));
        assert_eq!(
            resolve_store_root().unwrap(),
            Path::new("/tmp/ailoom-store")
        );
    }

    #[test]
    fn empty_xdg_does_not_count_as_set() {
        let _lock = lock();
        let _x = EnvGuard::set("XDG_STATE_HOME", Some("   "));
        let _a = EnvGuard::set("AILOOM_DATA_ROOT", None);
        let _h = EnvGuard::set("HOME", Some("/tmp/fake-home"));
        // 空 XDG → 视为未设 → 无 AILOOM_* → 规范默认
        assert_eq!(
            resolve_data_root(None).unwrap(),
            Path::new("/tmp/fake-home/.local/state/ailoom")
        );
    }

    /// v1.1：无任何覆盖时按 XDG 规范默认落盘。
    #[test]
    fn xdg_spec_defaults_when_no_overrides() {
        let _lock = lock();
        super::reset_migration_state_for_test();
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let _guards: Vec<EnvGuard> = [
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "AILOOM_DATA_ROOT",
            "AILOOM_STORE_ROOT",
        ]
        .iter()
        .map(|k| EnvGuard::set(k, None))
        .collect();
        let _h = EnvGuard::set("HOME", Some(home.to_str().unwrap()));
        assert_eq!(
            resolve_data_root(None).unwrap(),
            home.join(".local/state/ailoom")
        );
        assert_eq!(
            resolve_store_root().unwrap(),
            home.join(".local/share/ailoom/store")
        );
    }

    /// v1.1 迁移：旧默认 ~/.ailoom 存在时一次性拆分迁移（store→share，ws/cache→state）。
    #[test]
    fn legacy_home_migrates_to_xdg_spec_locations() {
        let _lock = lock();
        super::reset_migration_state_for_test();
        let _guards: Vec<EnvGuard> = [
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "AILOOM_DATA_ROOT",
            "AILOOM_STORE_ROOT",
        ]
        .iter()
        .map(|k| EnvGuard::set(k, None))
        .collect();
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let legacy = home.join(".ailoom");
        for d in [
            legacy.join("store/team/source"),
            legacy.join("ws/abc/events"),
            legacy.join("cache/ck"),
            legacy.join("bin"),
        ] {
            std::fs::create_dir_all(&d).unwrap();
        }
        std::fs::write(legacy.join("store/team/source/f.json"), b"{}").unwrap();
        std::fs::write(legacy.join("ws/abc/events/events.jsonl"), b"").unwrap();
        std::fs::write(legacy.join("bin/ailoom"), b"#!/bin/sh\n").unwrap();
        let _h = EnvGuard::set("HOME", Some(home.to_str().unwrap()));

        assert_eq!(
            resolve_data_root(None).unwrap(),
            home.join(".local/state/ailoom"),
            "数据根迁移到规范位置"
        );
        assert_eq!(
            resolve_store_root().unwrap(),
            home.join(".local/share/ailoom/store"),
            "Store 迁移到规范位置"
        );
        assert!(
            home.join(".local/share/ailoom/store/team/source/f.json")
                .is_file(),
            "store 内容随迁"
        );
        assert!(
            home.join(".local/state/ailoom/ws/abc/events/events.jsonl")
                .is_file(),
            "ws 内容随迁"
        );
        assert!(
            home.join(".local/state/ailoom/cache/ck").is_dir(),
            "cache 随迁"
        );
        assert!(
            legacy.join("bin/ailoom").is_file(),
            "非契约数据（bin/）原地保留"
        );
        // 幂等：再次解析稳定
        assert_eq!(
            resolve_data_root(None).unwrap(),
            home.join(".local/state/ailoom")
        );
    }

    /// 迁移失败（目标父目录不可创建）→ 整体回退，继续使用旧目录并保持数据可达。
    #[test]
    fn migration_failure_falls_back_to_legacy() {
        let _lock = lock();
        super::reset_migration_state_for_test();
        let _guards: Vec<EnvGuard> = [
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "AILOOM_DATA_ROOT",
            "AILOOM_STORE_ROOT",
        ]
        .iter()
        .map(|k| EnvGuard::set(k, None))
        .collect();
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let legacy = home.join(".ailoom");
        std::fs::create_dir_all(legacy.join("ws/abc")).unwrap();
        std::fs::write(legacy.join("ws/abc/events.jsonl"), b"").unwrap();
        // 阻断迁移：把 ~/.local 做成文件，create_dir_all 必然失败
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".local"), b"blocker").unwrap();
        let _h = EnvGuard::set("HOME", Some(home.to_str().unwrap()));

        assert_eq!(
            resolve_data_root(None).unwrap(),
            legacy,
            "迁移失败回退旧根（数据保持可达）"
        );
        assert!(
            legacy.join("ws/abc/events.jsonl").is_file(),
            "旧数据未被动过"
        );
        assert_eq!(
            resolve_store_root().unwrap(),
            legacy.join("store"),
            "Store 同样回退旧位置"
        );
    }

    /// RW-01/S01：迁移后旧 store 位置的兼容链接保持既有绝对链接可达，设备身份随迁。
    #[test]
    fn migration_keeps_legacy_links_reachable_and_device_identity() {
        let _lock = lock();
        super::reset_migration_state_for_test();
        let _guards: Vec<EnvGuard> = [
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "AILOOM_DATA_ROOT",
            "AILOOM_STORE_ROOT",
        ]
        .iter()
        .map(|k| EnvGuard::set(k, None))
        .collect();
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let legacy = home.join(".ailoom");
        for d in [
            legacy.join("store/team/source"),
            legacy.join("ws/abc/events"),
            legacy.join("bin"),
        ] {
            std::fs::create_dir_all(&d).unwrap();
        }
        std::fs::write(legacy.join("store/team/source/f.json"), b"{}").unwrap();
        std::fs::write(legacy.join("device-id"), b"dev-identity-001").unwrap();
        std::fs::write(legacy.join("bin/ailoom"), b"#!/bin/sh\n").unwrap();
        let _h = EnvGuard::set("HOME", Some(home.to_str().unwrap()));

        let _ = resolve_data_root(None).unwrap();
        let _ = resolve_store_root().unwrap();

        // 已部署工作区的旧绝对链接（经由 legacy/store/…）仍可达
        let old_link_target = legacy.join("store/team/source/f.json");
        assert!(old_link_target.is_file(), "旧绝对路径经兼容链接仍可达");
        assert_eq!(
            std::fs::read(&old_link_target).unwrap(),
            b"{}",
            "内容逐字节不变"
        );
        assert!(
            super::is_symlink(&legacy.join("store")),
            "旧 store 位置留兼容符号链接"
        );
        // 设备身份连续：新数据根的 device-id 与旧值一致
        let migrated_id =
            std::fs::read_to_string(home.join(".local/state/ailoom/device-id")).unwrap();
        assert_eq!(migrated_id, "dev-identity-001", "device-id 随迁不变");
        // bin 等非契约内容不被移动
        assert!(legacy.join("bin/ailoom").is_file());
        // 幂等：再次解析稳定在新默认
        assert_eq!(
            resolve_store_root().unwrap(),
            home.join(".local/share/ailoom/store")
        );
    }

    /// RW-01：迁移中段失败要清掉本次新建的空新根，下一个进程重试完整迁移，
    /// 不会把空新根误判成并存并采用。
    #[test]
    fn partial_failure_cleans_empty_roots_and_retry_completes() {
        let _lock = lock();
        super::reset_migration_state_for_test();
        let _guards: Vec<EnvGuard> = [
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "AILOOM_DATA_ROOT",
            "AILOOM_STORE_ROOT",
        ]
        .iter()
        .map(|k| EnvGuard::set(k, None))
        .collect();
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let legacy = home.join(".ailoom");
        for d in [
            legacy.join("store/team"),
            legacy.join("ws/abc"),
            legacy.join("cache/ck"),
        ] {
            std::fs::create_dir_all(&d).unwrap();
        }
        std::fs::write(legacy.join("store/team/s.json"), b"{}").unwrap();
        // share 可创建、state 被文件阻断 → store 已移动后 ws 步骤失败
        std::fs::create_dir_all(home.join(".local/share")).unwrap();
        std::fs::write(home.join(".local/state"), b"blocker").unwrap();
        let _h = EnvGuard::set("HOME", Some(home.to_str().unwrap()));

        // 进程 1：迁移失败 → 回退旧根，且不留半迁移状态
        assert_eq!(resolve_data_root(None).unwrap(), legacy);
        assert!(
            legacy.join("store/team/s.json").is_file(),
            "store 回滚回旧位置"
        );
        assert!(
            !home.join(".local/share/ailoom/store").exists(),
            "不残留已移动内容"
        );
        assert!(
            !home.join(".local/share/ailoom").exists(),
            "本次新建的空新根被清理"
        );

        // 进程 2：阻断解除后重试 → 完整迁移成功
        super::reset_migration_state_for_test();
        std::fs::remove_file(home.join(".local/state")).unwrap();
        assert_eq!(
            resolve_data_root(None).unwrap(),
            home.join(".local/state/ailoom"),
            "重试进程完成迁移，不误选空新根"
        );
        assert!(
            home.join(".local/state/ailoom/ws/abc").is_dir(),
            "ws 随重试迁移"
        );
        assert!(
            home.join(".local/share/ailoom/store/team/s.json").is_file(),
            "store 随重试迁移"
        );
    }
}
