//! 工作区识别（AIL-003）：从 cwd 向上发现声明（.ailoom/project.toml）与 Git 根；
//! 区分 repository anchor（共享缓存 key）与 workspace root（资源落点 + workspace_id）。

use crate::error::{code, Error, Result};
use crate::gitx;
use crate::ids::{sha256_prefix, workspace_id_from_root};
use std::path::{Path, PathBuf};

pub const AILOOM_DIR: &str = ".ailoom";
pub const DECLARATION_FILE: &str = "project.toml";

#[derive(Debug, Clone, serde::Serialize)]
pub struct Workspace {
    /// 当前 checkout/worktree 的规范绝对路径：资源写入落点
    pub workspace_root: PathBuf,
    /// 共享缓存 key 的仓库身份（规范化远端 URL 或主 .git 路径哈希），不是资源落点
    pub repository_anchor: String,
    pub anchor_key: String,
    pub workspace_id: String,
    pub is_git: bool,
    /// 已存在的可提交声明路径（可能尚未 init）
    pub declaration_path: Option<PathBuf>,
}

/// 发现工作区。`explicit_root` 优先（显式 --root 或测试注入），否则从 cwd 向上找。
/// 优先级：当前 Git 边界内最近的 `.ailoom/project.toml` 声明 > 最近的 Git 根。
/// 到达 Git 根即停止，避免嵌套仓库或 linked worktree 继承外部声明。两者都无 → E1001。
pub fn discover(cwd: &Path, explicit_root: Option<&Path>) -> Result<Workspace> {
    let start = match explicit_root {
        Some(root) => canonicalize(root).map_err(|e| {
            Error::new(code::WORKSPACE_INVALID, format!("显式根不可用: {e}"))
                .context(serde_json::json!({ "root": root.display().to_string() }))
        })?,
        None => canonicalize(cwd)
            .map_err(|e| Error::new(code::WORKSPACE_INVALID, format!("cwd 不可用: {e}")))?,
    };

    let mut dir: Option<&Path> = Some(start.as_path());
    while let Some(current) = dir {
        let declaration = current.join(AILOOM_DIR).join(DECLARATION_FILE);
        if declaration.is_file() {
            return build_workspace(current, true);
        }
        if current.join(".git").exists() {
            return build_workspace(current, false);
        }
        dir = current.parent();
    }

    Err(Error::new(
        code::WORKSPACE_ROOT_NOT_FOUND,
        "未找到 .ailoom/project.toml 声明或 Git 仓库根",
    )
    .context(serde_json::json!({ "start": start.display().to_string() }))
    .fix("在仓库根执行 ailoom init，或用 --root 显式指定工作区根"))
}

fn build_workspace(root: &Path, has_declaration: bool) -> Result<Workspace> {
    // AIL-039：Git 身份与配置作用域分离。声明目录只是配置作用域（workspace_root），
    // Git 身份由所在仓库决定——子目录没有自身 .git 也不降为非 Git。
    let is_git = find_git_root_up(root).is_some();
    let anchor = repository_anchor(root, is_git)?;
    let anchor_key = sha256_prefix(anchor.as_bytes(), 16);
    Ok(Workspace {
        workspace_root: root.to_path_buf(),
        repository_anchor: anchor,
        anchor_key,
        workspace_id: workspace_id_from_root(root),
        is_git,
        declaration_path: if has_declaration {
            Some(root.join(AILOOM_DIR).join(DECLARATION_FILE))
        } else {
            None
        },
    })
}

/// Repository anchor：有远端 → `git+<规范化URL>`；无远端 → `path+<commondir哈希>`。
/// anchor 只用于共享缓存，绝不作为资源写入位置。
fn repository_anchor(root: &Path, is_git: bool) -> Result<String> {
    if !is_git {
        return Ok(format!(
            "nongit+{}",
            sha256_prefix(root.to_string_lossy().as_bytes(), 32)
        ));
    }
    if let Some(url) = gitx::git_optional(root, &["remote", "get-url", "origin"]) {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return Ok(gitx::normalize_remote_url(trimmed));
        }
    }
    // 无远端：用主 worktree 的 git common dir 路径哈希，同一仓库的各 worktree 共享
    let common = gitx::git_optional(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .unwrap_or_default();
    let base = if common.trim().is_empty() {
        root.join(".git").to_string_lossy().to_string()
    } else {
        common.trim().to_string()
    };
    Ok(format!("path+{}", sha256_prefix(base.as_bytes(), 32)))
}

/// 从 root 向上找最近的 `.git`（目录或 linked-worktree 文件）；声明目录本身无
/// `.git` 时，其 Git 身份来自所在仓库（AIL-039）。
pub fn find_git_root_up(root: &Path) -> Option<PathBuf> {
    let mut dir = Some(root.to_path_buf());
    while let Some(current) = dir {
        if current.join(".git").exists() {
            return Some(current);
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    None
}

/// 规范化路径（解析符号链接）。平台大小写语义保持原样：不做大小写折叠。
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitx::{git, git_commit_all, git_init};

    /// macOS 会把 /var 解析为 /private/var：断言一律比较规范化后的路径。
    fn canon(p: &Path) -> PathBuf {
        canonicalize(p).unwrap()
    }

    fn discover_at(dir: &Path) -> Result<Workspace> {
        discover(dir, None)
    }

    #[test]
    fn root_and_deep_subdir_same_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        git_init(&repo, false).unwrap();
        let deep = repo.join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();
        let at_root = discover_at(&repo).unwrap();
        let at_deep = discover_at(&deep).unwrap();
        assert_eq!(at_root.workspace_root, at_deep.workspace_root);
        assert_eq!(at_root.workspace_id, at_deep.workspace_id);
        assert!(at_root.is_git);
    }

    #[test]
    fn no_root_is_error_not_global_default() {
        let tmp = tempfile::tempdir().unwrap();
        let err = discover_at(tmp.path()).unwrap_err();
        assert_eq!(err.code, code::WORKSPACE_ROOT_NOT_FOUND);
    }

    #[test]
    fn spaces_and_chinese_paths_work() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("项目 目录 with space");
        std::fs::create_dir_all(&repo).unwrap();
        git_init(&repo, false).unwrap();
        let ws = discover_at(&repo).unwrap();
        assert!(ws
            .workspace_root
            .to_string_lossy()
            .contains("项目 目录 with space"));
    }

    #[test]
    fn symlinks_resolve_to_same_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        git_init(&repo, false).unwrap();
        let link = tmp.path().join("repo-link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&repo, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&repo, &link).unwrap();
        let a = discover_at(&repo).unwrap();
        let b = discover_at(&link).unwrap();
        assert_eq!(
            a.workspace_id, b.workspace_id,
            "符号链接别名归一到同一 workspace"
        );
    }

    #[test]
    fn paths_are_not_lowercased() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("MiXeD-Case-Repo");
        std::fs::create_dir_all(&repo).unwrap();
        git_init(&repo, false).unwrap();
        let ws = discover_at(&repo).unwrap();
        // 不把路径转小写：规范化后保留原大小写
        assert!(
            ws.workspace_root.ends_with("MiXeD-Case-Repo"),
            "大小写被改变: {}",
            ws.workspace_root.display()
        );
        assert_eq!(ws.workspace_root, canon(&repo));
    }

    #[test]
    fn declaration_inside_repo_binds_nearest_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("monorepo");
        git_init(&repo, false).unwrap();
        let nested = repo.join("services/x");
        std::fs::create_dir_all(nested.join(AILOOM_DIR)).unwrap();
        std::fs::write(
            nested.join(AILOOM_DIR).join(DECLARATION_FILE),
            "schema_version = 1\n",
        )
        .unwrap();
        let ws = discover_at(&nested).unwrap();
        assert_eq!(
            ws.workspace_root,
            canon(&nested),
            "最近声明优先于外部 Git 根"
        );
        let from_other = discover_at(&repo).unwrap();
        assert_eq!(from_other.workspace_root, canon(&repo));
        assert_ne!(ws.workspace_id, from_other.workspace_id);
    }

    #[test]
    fn worktrees_share_anchor_but_have_distinct_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        git_init(&main, false).unwrap();
        std::fs::write(main.join("seed.txt"), "seed").unwrap();
        git_commit_all(&main, "seed", &["seed.txt"]).unwrap();
        let wt = tmp.path().join("wt2");
        git(
            &main,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wt2"],
        )
        .unwrap();
        let ws_main = discover_at(&main).unwrap();
        let ws_wt = discover_at(&wt).unwrap();
        assert_ne!(
            ws_main.workspace_id, ws_wt.workspace_id,
            "两个 worktree 必须是不同 workspace"
        );
        assert_eq!(
            ws_main.repository_anchor, ws_wt.repository_anchor,
            "同仓库 worktree 共享 anchor"
        );
        assert_eq!(ws_main.anchor_key, ws_wt.anchor_key);
        // 主 checkout 不作为 worktree 的资源目标
        assert_ne!(ws_main.workspace_root, ws_wt.workspace_root);
    }

    #[test]
    fn declaration_discovery_stops_at_nested_git_root() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        git_init(&parent, false).unwrap();
        std::fs::create_dir_all(parent.join(AILOOM_DIR)).unwrap();
        std::fs::write(
            parent.join(AILOOM_DIR).join(DECLARATION_FILE),
            "schema_version = 1\n",
        )
        .unwrap();
        let same_repo = parent.join("services/ordinary/deep");
        std::fs::create_dir_all(&same_repo).unwrap();
        assert_eq!(
            discover_at(&same_repo).unwrap().workspace_root,
            canon(&parent)
        );

        let child = parent.join("child");
        git_init(&child, false).unwrap();
        let deep = child.join("src/deep");
        std::fs::create_dir_all(&deep).unwrap();
        for ws in [
            discover_at(&deep).unwrap(),
            discover(tmp.path(), Some(&child)).unwrap(),
        ] {
            assert_eq!(ws.workspace_root, canon(&child));
            assert!(ws.declaration_path.is_none(), "子仓库不得继承父声明");
        }
        std::fs::create_dir_all(child.join(AILOOM_DIR)).unwrap();
        std::fs::write(
            child.join(AILOOM_DIR).join(DECLARATION_FILE),
            "schema_version = 1\n",
        )
        .unwrap();
        assert_eq!(
            discover_at(&deep).unwrap().declaration_path,
            Some(canon(&child).join(AILOOM_DIR).join(DECLARATION_FILE))
        );
    }

    #[test]
    fn linked_worktree_does_not_inherit_enclosing_declaration() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        git_init(&main, false).unwrap();
        std::fs::write(main.join("seed.txt"), "seed").unwrap();
        git_commit_all(&main, "seed", &["seed.txt"]).unwrap();
        let parent = tmp.path().join("parent");
        std::fs::create_dir_all(parent.join(AILOOM_DIR)).unwrap();
        std::fs::write(
            parent.join(AILOOM_DIR).join(DECLARATION_FILE),
            "schema_version = 1\n",
        )
        .unwrap();
        let wt = parent.join("wt");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                wt.to_str().unwrap(),
                "-b",
                "nested-wt",
            ],
        )
        .unwrap();
        assert!(wt.join(".git").is_file());
        let ws = discover_at(&wt).unwrap();
        assert_eq!(ws.workspace_root, canon(&wt));
        assert!(ws.declaration_path.is_none());
        assert_eq!(
            ws.repository_anchor,
            discover_at(&main).unwrap().repository_anchor
        );
    }

    #[test]
    fn explicit_root_takes_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        git_init(&repo, false).unwrap();
        let other = tempfile::tempdir().unwrap();
        let ws = discover(other.path(), Some(&repo)).unwrap();
        assert_eq!(ws.workspace_root, canon(&repo));
    }
}
