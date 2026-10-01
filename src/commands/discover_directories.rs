//! Optional, read-only discovery of child configuration nodes. No file content
//! is opened, no symlink is followed, and no candidate is registered here.

use crate::adapters::discovery::{directory_markers, AgentDirectory};
use crate::error::{code, Error, Result};
use serde_json::{json, Value};
use std::path::Path;

pub fn discover(root: &Path, relative: Option<&str>, depth: usize) -> Result<Value> {
    scan(root, relative, depth, &directory_markers(), 5000)
}

fn scan(
    root: &Path,
    relative: Option<&str>,
    depth: usize,
    markers: &[AgentDirectory],
    budget: usize,
) -> Result<Value> {
    if !matches!(depth, 2 | 3) {
        return Err(Error::new(code::USAGE, "扫描深度只能是 2 或 3 层"));
    }
    let root = root.canonicalize()?;
    let mut base = root.clone();
    if let Some(rel) = relative.filter(|r| !r.is_empty()) {
        crate::manifest::validate_relative_path("sub", rel)?;
        for part in Path::new(rel).components() {
            base.push(part);
            let meta = base.symlink_metadata()?;
            if !meta.is_dir() || meta.is_symlink() || base.join(".git").symlink_metadata().is_ok() {
                return Err(Error::new(
                    code::ILLEGAL_PATH,
                    "扫描起点不能跨符号链接或嵌套 Git 仓库",
                ));
            }
        }
    }
    let walker = walkdir::WalkDir::new(&base)
        .follow_links(false)
        .max_depth(depth)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            entry.file_type().is_dir()
                && !name.starts_with('.')
                && !super::scan_skills::SCAN_SKIP_DIRS.contains(&name.as_ref())
                && entry.path().join(".git").symlink_metadata().is_err()
        });
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut visited = 0;
    let mut truncated = false;
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        if visited == budget {
            truncated = true;
            break;
        }
        visited += 1;
        let agents: Vec<_> = markers
            .iter()
            .filter(|marker| {
                entry
                    .path()
                    .join(marker.directory)
                    .symlink_metadata()
                    .map(|m| m.is_dir() && !m.is_symlink())
                    .unwrap_or(false)
            })
            .collect();
        if !agents.is_empty() {
            let path = entry
                .path()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            candidates.push(json!({"path":path,"agents":agents}));
        }
    }
    Ok(
        json!({"candidates":candidates,"depth":depth,"visited":visited,"truncated":truncated,"errors":errors}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_only_marked_directories_with_relative_depth_and_registry_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        for dir in [
            ".claude",
            "plain",
            "a/.claude",
            "a/b/.codex",
            "a/b/c/.agents",
            "a/b/c/d/.claude",
            "custom/.future-agent",
            "node_modules/pkg/.claude",
            ".cache/pkg/.claude",
            "nested/.git",
            "nested/docs/.claude",
        ] {
            std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
        }
        let mut markers = directory_markers();
        markers.push(AgentDirectory {
            agent: "future",
            label: "Future Agent",
            directory: ".future-agent",
        });
        let v = scan(tmp.path(), None, 2, &markers, 5000).unwrap();
        let paths: Vec<_> = v["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, vec!["a", "a/b", "custom"]);
        let v = scan(tmp.path(), Some("a"), 3, &markers, 5000).unwrap();
        let paths: Vec<_> = v["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, vec!["a/b", "a/b/c", "a/b/c/d"]);
        assert!(discover(tmp.path(), Some("../escape"), 3).is_err());
        assert!(discover(tmp.path(), Some("nested/docs"), 3).is_err());
        assert!(discover(tmp.path(), None, 99).is_err());
        assert_eq!(
            scan(tmp.path(), None, 3, &markers, 1).unwrap()["truncated"],
            true
        );
    }

    #[cfg(unix)]
    #[test]
    fn ignores_linked_directories_and_linked_agent_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::create_dir_all(outside.join(".claude")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
        std::os::unix::fs::symlink(outside.join(".claude"), root.join("web/.claude")).unwrap();
        assert_eq!(discover(&root, None, 3).unwrap()["candidates"], json!([]));
        assert!(discover(&root, Some("linked"), 3).is_err());
    }
}
