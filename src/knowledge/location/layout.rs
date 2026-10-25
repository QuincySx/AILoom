//! File locations for the readable library layout; legacy libraries remain readable.
use super::*;

pub const HIDDEN_MARKER: &str = ".ailoom/library.json";
pub(super) fn marker(tree: &Tree) -> &Path {
    if tree.contains_key(Path::new(HIDDEN_MARKER)) {
        Path::new(HIDDEN_MARKER)
    } else {
        Path::new(MARKER)
    }
}
pub(crate) fn marker_at(root: &Path) -> PathBuf {
    if root.join(HIDDEN_MARKER).is_file() {
        root.join(HIDDEN_MARKER)
    } else {
        root.join(MARKER)
    }
}
pub(super) fn modern(tree: &Tree) -> bool {
    tree.contains_key(Path::new(HIDDEN_MARKER))
}
pub(crate) fn project_dir(root: &Path, project: &str) -> PathBuf {
    if root.join(HIDDEN_MARKER).is_file() {
        PathBuf::from(".ailoom/project")
    } else {
        Path::new("projects").join(project)
    }
}
pub(super) fn source_index(tree: &Tree, project: &str) -> PathBuf {
    if modern(tree) {
        "sources/README.md".into()
    } else {
        Path::new(ingest::PREFIX).join(project).join("index.md")
    }
}
pub(crate) fn source_index_at(root: &Path, project: &str) -> PathBuf {
    if root.join(HIDDEN_MARKER).is_file() {
        root.join("sources/README.md")
    } else {
        root.join(ingest::PREFIX).join(project).join("index.md")
    }
}
pub(super) fn manifest(tree: &Tree, project: &str) -> PathBuf {
    if modern(tree) {
        ".ailoom/sources.json".into()
    } else {
        Path::new(ingest::PREFIX)
            .join(project)
            .join("manifest.json")
    }
}
pub(super) fn source(tree: &Tree, project: &str, hash: &str) -> PathBuf {
    if modern(tree) {
        Path::new("sources").join(format!("{hash}.md"))
    } else {
        Path::new(ingest::PREFIX)
            .join(project)
            .join(format!("{hash}.md"))
    }
}

pub(super) fn validate_project(tree: &Tree, project: &str) -> Result<()> {
    if modern(tree) {
        let value: Value = serde_json::from_slice(&tree[marker(tree)])?;
        if value["project_id"].as_str().is_some_and(|id| id != project) {
            return Err(fail("知识库属于其他仓库"));
        }
    }
    Ok(())
}
