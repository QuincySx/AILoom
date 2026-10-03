//! Git 本地 exclude（info/exclude）生命周期辅助（AIL-042）。
//!
//! 只管理 `# >>> ailoom-personal >>>` 托管块内的行：用户既有条目（含 common-dir
//! 共享 exclude）原样保留。多 Worktree 共用同一 common-dir 的 exclude，用数据区的
//! 引用计数决定何时真正移除某行。说明：exclude 对已跟踪文件无效，也不是强制
//! add 的防泄漏保证——它只让普通 `git add` 默认不收 AILoom 生成的新增文件。

use crate::error::{code, Error, Result};
use crate::ids::now_iso;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MARKER_BEGIN: &str = "# >>> ailoom-personal >>>";
pub const MARKER_END: &str = "# <<< ailoom-personal <<<";

/// 每仓库的 exclude 引用计数状态（数据区，仓外）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludeState {
    pub schema_version: u32,
    /// 旧格式：pattern → 计数。每次同步都 +1、从不减少，导致关闭宿主后条目永远残留；
    /// 新同步会把它迁移进 `owners`。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub refcount: BTreeMap<String, u32>,
    /// pattern → 当前部署了它的所有者（Worktree + 作用域）
    #[serde(default)]
    pub owners: BTreeMap<String, std::collections::BTreeSet<String>>,
    pub updated_at: String,
}

impl ExcludeState {
    fn live(&self) -> Vec<String> {
        let mut live: std::collections::BTreeSet<String> = self.refcount.keys().cloned().collect();
        live.extend(self.owners.keys().cloned());
        live.into_iter().collect()
    }
}

impl Default for ExcludeState {
    fn default() -> Self {
        ExcludeState {
            schema_version: 1,
            refcount: Default::default(),
            owners: Default::default(),
            updated_at: now_iso(),
        }
    }
}

pub fn exclude_file(common_dir: &Path) -> PathBuf {
    common_dir.join("info").join("exclude")
}

fn state_path(data_root: &Path, repo_id: &str) -> PathBuf {
    data_root
        .join("repos")
        .join(repo_id)
        .join("exclude-state.json")
}

fn load_state(data_root: &Path, repo_id: &str) -> Result<ExcludeState> {
    let p = state_path(data_root, repo_id);
    if !p.is_file() {
        return Ok(ExcludeState::default());
    }
    let text = std::fs::read_to_string(&p)?;
    serde_json::from_str(&text)
        .map_err(|e| Error::new(code::INTERNAL, format!("exclude-state.json 损坏: {e}")))
}

fn save_state(state: &ExcludeState, data_root: &Path, repo_id: &str) -> Result<()> {
    let p = state_path(data_root, repo_id);
    std::fs::create_dir_all(p.parent().unwrap_or_else(|| Path::new(".")))?;
    crate::sync_common::atomic_write(&p, serde_json::to_vec_pretty(state)?.as_slice())
}

/// 读取用户行（剔除原托管块，区间状态机）。
fn user_lines_of(text: &str) -> Vec<String> {
    let mut in_block = false;
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim();
        if t == MARKER_BEGIN {
            in_block = true;
            continue;
        }
        if t == MARKER_END {
            in_block = false;
            continue;
        }
        if !in_block {
            out.push(l.to_string());
        }
    }
    out
}

fn write_exclude_with(common_dir: &Path, patterns: &[String]) -> Result<()> {
    let file = exclude_file(common_dir);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = if file.is_file() {
        std::fs::read_to_string(&file)?
    } else {
        String::new()
    };
    let mut out = user_lines_of(&existing).join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    if !patterns.is_empty() {
        out.push_str(MARKER_BEGIN);
        out.push('\n');
        for p in patterns {
            out.push_str(p);
            out.push('\n');
        }
        out.push_str(MARKER_END);
        out.push('\n');
    }
    crate::sync_common::atomic_write(&file, out.as_bytes())
}

/// 为本次部署登记 exclude patterns（引用计数 +1；幂等）。
/// 同一 common-dir 的多个 Worktree 共享 exclude 文件，计数到 0 才移除行。
pub fn add_patterns(
    common_dir: &Path,
    data_root: &Path,
    repo_id: &str,
    patterns: &[String],
) -> Result<()> {
    let mut state = load_state(data_root, repo_id)?;
    for p in patterns {
        *state.refcount.entry(p.clone()).or_insert(0) += 1;
    }
    state.updated_at = now_iso();
    write_exclude_with(common_dir, &state.live())?;
    save_state(&state, data_root, repo_id)
}

/// 同步后对账：把 `owner`（Worktree + 作用域）名下的 pattern 设为本次实际部署的集合。
/// 关闭宿主或资源后，没有任何所有者引用的行随之移除；用户既有行永不触碰。
pub fn sync_patterns(
    common_dir: &Path,
    data_root: &Path,
    repo_id: &str,
    owner: &str,
    patterns: &[String],
) -> Result<()> {
    let mut state = load_state(data_root, repo_id)?;
    // 旧计数没有归属信息：交给当前所有者对账（其他 Worktree 下次同步时补回各自条目）
    for legacy in std::mem::take(&mut state.refcount).into_keys() {
        state
            .owners
            .entry(legacy)
            .or_default()
            .insert(owner.to_string());
    }
    for owners in state.owners.values_mut() {
        owners.remove(owner);
    }
    for p in patterns {
        state
            .owners
            .entry(p.clone())
            .or_default()
            .insert(owner.to_string());
    }
    state.owners.retain(|_, o| !o.is_empty());
    state.updated_at = now_iso();
    write_exclude_with(common_dir, &state.live())?;
    save_state(&state, data_root, repo_id)
}

/// 卸载时移除引用（计数 -1；到 0 移除行）。用户既有行永不触碰。
pub fn remove_patterns(
    common_dir: &Path,
    data_root: &Path,
    repo_id: &str,
    patterns: &[String],
) -> Result<()> {
    let mut state = load_state(data_root, repo_id)?;
    for p in patterns {
        if let Some(c) = state.refcount.get_mut(p) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                state.refcount.remove(p);
            }
        }
    }
    state.updated_at = now_iso();
    write_exclude_with(common_dir, &state.live())?;
    save_state(&state, data_root, repo_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 升级兼容：旧版本只记计数（每次同步 +1、从不减少）。新版本第一次同步时把旧条目
    /// 交给当前所有者对账：仍部署的保留，已不部署的移除，用户行不动。
    #[test]
    fn legacy_refcount_state_migrates_on_first_sync() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join("common");
        std::fs::create_dir_all(common.join("info")).unwrap();
        let data = tempfile::tempdir().unwrap();
        // 旧版本留下的状态与 exclude 文件
        add_patterns(
            &common,
            data.path(),
            "repo",
            &["keep.md".into(), "stale.md".into()],
        )
        .unwrap();
        add_patterns(
            &common,
            data.path(),
            "repo",
            &["keep.md".into(), "stale.md".into()],
        )
        .unwrap();
        let mut text = std::fs::read_to_string(exclude_file(&common)).unwrap();
        text.insert_str(0, "*.log\n");
        std::fs::write(exclude_file(&common), text).unwrap();

        sync_patterns(&common, data.path(), "repo", "wt:", &["keep.md".into()]).unwrap();
        let text = std::fs::read_to_string(exclude_file(&common)).unwrap();
        assert!(
            text.contains("keep.md") && !text.contains("stale.md"),
            "{text}"
        );
        assert!(text.contains("*.log"), "用户行保留: {text}");
        let state = load_state(data.path(), "repo").unwrap();
        assert!(state.refcount.is_empty(), "旧计数已迁移");
        sync_patterns(&common, data.path(), "repo", "wt:", &[]).unwrap();
        assert!(!std::fs::read_to_string(exclude_file(&common))
            .unwrap()
            .contains(MARKER_BEGIN));
    }

    #[test]
    fn user_lines_preserved_and_refcount_works() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join("common");
        std::fs::create_dir_all(common.join("info")).unwrap();
        std::fs::write(exclude_file(&common), "*.log\n# my own\n").unwrap();
        let data = tempfile::tempdir().unwrap();

        add_patterns(
            &common,
            data.path(),
            "repo-a",
            &[".claude/rules/ailoom-personal.md".to_string()],
        )
        .unwrap();
        add_patterns(
            &common,
            data.path(),
            "repo-a",
            &[".claude/rules/ailoom-personal.md".to_string()],
        )
        .unwrap();
        let text = std::fs::read_to_string(exclude_file(&common)).unwrap();
        assert!(
            text.contains("*.log") && text.contains("# my own"),
            "用户行保留: {text}"
        );
        assert_eq!(
            text.matches(".claude/rules/ailoom-personal.md").count(),
            1,
            "同一 pattern 只有一行"
        );

        remove_patterns(
            &common,
            data.path(),
            "repo-a",
            &[".claude/rules/ailoom-personal.md".to_string()],
        )
        .unwrap();
        let text = std::fs::read_to_string(exclude_file(&common)).unwrap();
        assert!(
            text.contains(".claude/rules/ailoom-personal.md"),
            "计数 1 → 行保留"
        );

        remove_patterns(
            &common,
            data.path(),
            "repo-a",
            &[".claude/rules/ailoom-personal.md".to_string()],
        )
        .unwrap();
        let text = std::fs::read_to_string(exclude_file(&common)).unwrap();
        assert!(
            !text.contains(".claude/rules/ailoom-personal.md"),
            "计数 0 → 行移除"
        );
        assert!(text.contains("*.log"), "用户行仍在");
        // 托管块整体消失
        assert!(!text.contains(MARKER_BEGIN));
    }

    #[test]
    fn stale_managed_block_is_replaced_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join("common");
        std::fs::create_dir_all(common.join("info")).unwrap();
        std::fs::write(
            exclude_file(&common),
            "keep.txt\n# >>> ailoom-personal >>>\nold/path.md\n# <<< ailoom-personal <<<\n",
        )
        .unwrap();
        let data = tempfile::tempdir().unwrap();
        add_patterns(&common, data.path(), "repo-b", &["new/path.md".to_string()]).unwrap();
        let text = std::fs::read_to_string(exclude_file(&common)).unwrap();
        assert!(text.contains("keep.txt"));
        assert!(text.contains("new/path.md"));
        assert!(!text.contains("old/path.md"), "旧托管行被替换: {text}");
        assert_eq!(text.matches(MARKER_BEGIN).count(), 1);
    }
}
