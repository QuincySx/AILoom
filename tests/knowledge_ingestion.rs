mod common;

use ailoom::knowledge::{
    location::{self, ingest, Request},
    organize,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn init(data: &Path, root: &Path, library: &Path) {
    common::isolate_in_process_roots();
    location::run(
        data,
        &Request {
            action: "init".into(),
            root: root.into(),
            path: Some(library.into()),
            ..Default::default()
        },
    )
    .unwrap();
}
fn source(data: &Path, root: &Path, directory: &Path) -> String {
    let directory = directory.canonicalize().unwrap();
    ingest::status(data, root).unwrap()["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["path"].as_str() == directory.to_str())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn capture(data: &Path, root: &Path, id: &str, paths: &[&str]) -> Value {
    let preview = ingest::preview(data, root, id).unwrap();
    ingest::apply(
        data,
        root,
        id,
        preview["expected"].as_str().unwrap(),
        &paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
    )
    .unwrap()
}
fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn incremental_capture_keeps_history_and_recalls_only_current_revision() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let data = t.path().join("data");
    let library = t.path().join("knowledge");
    std::fs::create_dir(&root).unwrap();
    init(&data, &root, &library);
    let id = source(&data, &root, &root);
    let original = "# Connection\nquartzalpha original lesson";
    std::fs::write(root.join("lesson.md"), original).unwrap();
    let first = capture(&data, &root, &id, &["lesson.md"]);
    assert_eq!(first["new_documents"], 1);
    assert_eq!(first["revision_count"], 1);
    let again = capture(&data, &root, &id, &["lesson.md"]);
    assert_eq!(again["new_documents"], 0);
    assert_eq!(again["revision_count"], 1);
    std::fs::write(
        root.join("lesson.md"),
        "# Connection\nquartzbeta revised lesson",
    )
    .unwrap();
    let next = capture(&data, &root, &id, &["lesson.md"]);
    assert_eq!(next["revision_count"], 2);
    assert_eq!(next["document_count"], 1);
    assert!(
        location::recall(&data, &root, "quartzalpha", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let hits = location::recall(&data, &root, "quartzbeta", 5).unwrap();
    assert_eq!(hits["results"][0]["origins"][0]["document"], "lesson.md");
    assert_eq!(hits["results"][0]["evidence_state"], "source_record");
    assert_eq!(organize::status(&data, &root).unwrap()["source_count"], 1);
    std::fs::write(root.join("lesson.md"), original).unwrap();
    let reverted = capture(&data, &root, &id, &["lesson.md"]);
    assert_eq!(reverted["revision_count"], 3);
    assert_eq!(reverted["new_documents"], 0);
    assert_eq!(
        std::fs::read_to_string(root.join("lesson.md")).unwrap(),
        original
    );
    assert_eq!(
        location::recall(&data, &root, "quartzalpha", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        location::recall(&data, &root, "quartzbeta", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let snapshots = walkdir::WalkDir::new(library.join("sources"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().len() == 64)
        })
        .count();
    assert_eq!(snapshots, 2, "historical originals are retained");
}

#[test]
fn all_worktrees_and_managed_children_feed_one_library_with_multiple_origins() {
    let t = tempfile::tempdir().unwrap();
    let root = common::make_business_repo(t.path(), "main");
    let wt = t.path().join("feature");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ],
    );
    for p in [&root, &wt] {
        std::fs::create_dir_all(p.join("packages/app")).unwrap();
        std::fs::write(
            p.join("packages/app/lesson.md"),
            "# Shared\nnebulaarchive shared maintenance lesson",
        )
        .unwrap();
    }
    let data = t.path().join("data");
    let library = t.path().join("knowledge");
    init(&data, &root, &library);
    let discovery = ailoom::repo_registry::discover_repo(&root).unwrap();
    let mut registry =
        ailoom::repo_registry::RepoRegistry::resolve_or_create(&data, &discovery).unwrap();
    registry.refresh_worktrees(&discovery, &ailoom::ids::now_iso());
    registry.subprojects.insert(
        "packages/app".into(),
        ailoom::repo_registry::RegistrySubproject {
            rel: "packages/app".into(),
            registered_at: ailoom::ids::now_iso(),
        },
    );
    registry.save(&data).unwrap();
    let state = ingest::status(&data, &wt.join("packages/app")).unwrap();
    assert_eq!(state["sources"].as_array().unwrap().len(), 4);
    let main = source(&data, &root, &root);
    let child = source(&data, &root, &wt.join("packages/app"));
    let a = capture(&data, &root, &main, &["packages/app/lesson.md"]);
    let b = capture(&data, &wt, &child, &["packages/app/lesson.md"]);
    assert_eq!(a["new_documents"], 1);
    assert_eq!(b["new_documents"], 0);
    assert_eq!(b["reused_documents"], 1);
    assert_eq!(b["document_count"], 1);
    let hits = location::recall(&data, &root, "nebulaarchive", 5).unwrap();
    assert_eq!(hits["results"].as_array().unwrap().len(), 1);
    assert_eq!(hits["results"][0]["origins"].as_array().unwrap().len(), 2);
    let branch = hits["results"][0]["origins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["branch"] == "feature")
        .unwrap();
    assert!(branch["commit"].is_string());
    assert_eq!(branch["worktree_dirty"], true);
    std::fs::rename(&wt, t.path().join("retired-worktree")).unwrap();
    assert_eq!(
        location::recall(&data, &root, "nebulaarchive", 5).unwrap()["results"][0]["origins"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "archived copies survive loss of the original worktree"
    );
    assert_eq!(organize::status(&data, &root).unwrap()["source_count"], 1);
}

#[test]
fn preview_is_bound_to_documents_and_manifest_and_rejects_unlisted_paths() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let data = t.path().join("data");
    let library = t.path().join("knowledge");
    std::fs::create_dir(&root).unwrap();
    init(&data, &root, &library);
    std::fs::write(root.join("a.md"), "# First").unwrap();
    std::fs::write(root.join("b.md"), "# Second").unwrap();
    let id = source(&data, &root, &root);
    let preview = ingest::preview(&data, &root, &id).unwrap();
    let expected = preview["expected"].as_str().unwrap();
    for paths in [
        vec![],
        vec![PathBuf::from("../outside.md")],
        vec![root.join("a.md")],
    ] {
        assert!(ingest::apply(&data, &root, &id, expected, &paths).is_err());
    }
    assert!(!library.join(".ailoom/sources.json").exists());
    std::fs::write(root.join("a.md"), "# Changed").unwrap();
    assert!(ingest::apply(&data, &root, &id, expected, &["a.md".into()]).is_err());
    assert!(!library.join(".ailoom/sources.json").exists());
    let preview = ingest::preview(&data, &root, &id).unwrap();
    capture(&data, &root, &id, &["b.md"]);
    assert!(ingest::apply(
        &data,
        &root,
        &id,
        preview["expected"].as_str().unwrap(),
        &["a.md".into()]
    )
    .is_err());
    assert!(ingest::preview(&data, &root, "another-project-source").is_err());
}

#[test]
fn scan_skips_caches_nested_repositories_non_text_and_knowledge_itself() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let data = t.path().join("data");
    let library = root.join("knowledge");
    std::fs::create_dir(&root).unwrap();
    init(&data, &root, &library);
    for dir in ["node_modules", "target", ".codex", ".ssh", "nested"] {
        std::fs::create_dir(root.join(dir)).unwrap();
        std::fs::write(root.join(dir).join("skip.md"), "# Excluded").unwrap();
    }
    git(&root.join("nested"), &["init", "-q"]);
    std::fs::write(root.join("bad.md"), [0xff, 0xfe]).unwrap();
    std::fs::write(root.join("huge.md"), vec![b'a'; 4 * 1024 * 1024 + 1]).unwrap();
    std::fs::write(root.join("empty.md"), []).unwrap();
    std::fs::write(root.join("keep.MARKDOWN"), "# Keep").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("nested/skip.md"), root.join("linked.md")).unwrap();
    let id = source(&data, &root, &root);
    let preview = ingest::preview(&data, &root, &id).unwrap();
    assert_eq!(preview["documents"].as_array().unwrap().len(), 1);
    assert_eq!(preview["documents"][0]["path"], "keep.MARKDOWN");
    assert_eq!(preview["skipped"].as_array().unwrap().len(), 3);
    capture(&data, &root, &id, &["keep.MARKDOWN"]);
    assert_eq!(
        ingest::preview(&data, &root, &id).unwrap()["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ingest::status(&data, &root).unwrap()["knowledge_path"],
        library.canonicalize().unwrap().to_str().unwrap()
    );
}

#[test]
fn imported_documents_are_scoped_to_independent_repository_libraries() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let library = t.path().join("shared");
    let a = t.path().join("a");
    let b = t.path().join("b");
    for root in [&a, &b] {
        std::fs::create_dir(root).unwrap();
        init(&data, root, &library.join(root.file_name().unwrap()));
    }
    std::fs::write(a.join("note.md"), "# A\nprivatequartzalpha").unwrap();
    std::fs::write(b.join("note.md"), "# B\nprivatequartzbeta").unwrap();
    capture(&data, &a, &source(&data, &a, &a), &["note.md"]);
    capture(&data, &b, &source(&data, &b, &b), &["note.md"]);
    assert!(
        location::recall(&data, &b, "privatequartzalpha", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(organize::status(&data, &a).unwrap()["source_count"], 1);
    assert_eq!(organize::status(&data, &b).unwrap()["source_count"], 1);
}

#[test]
fn corrupt_current_snapshot_prevents_further_capture_without_partial_manifest() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let data = t.path().join("data");
    let library = t.path().join("knowledge");
    std::fs::create_dir(&root).unwrap();
    init(&data, &root, &library);
    std::fs::write(root.join("note.md"), "# Original\ncorruptcheck").unwrap();
    let id = source(&data, &root, &root);
    capture(&data, &root, &id, &["note.md"]);
    let hit = location::recall(&data, &root, "corruptcheck", 5).unwrap();
    let snapshot = hit["results"][0]["source_path"].as_str().unwrap();
    std::fs::write(snapshot, "# Tampered").unwrap();
    std::fs::write(root.join("next.md"), "# Next").unwrap();
    assert!(ingest::preview(&data, &root, &id).is_err());
    assert!(ingest::apply(&data, &root, &id, "anything", &["next.md".into()]).is_err());
    assert!(location::recall(&data, &root, "corruptcheck", 5).is_err());
}

#[test]
fn provenance_only_changes_are_selected_updates_and_git_status_errors_are_not_clean() {
    let t = tempfile::tempdir().unwrap();
    let root = common::make_business_repo(t.path(), "main");
    let data = t.path().join("data");
    init(&data, &root, &t.path().join("knowledge"));
    std::fs::write(root.join("note.md"), "# Stable body").unwrap();
    let id = source(&data, &root, &root);
    capture(&data, &root, &id, &["note.md"]);
    git(&root, &["add", "note.md"]);
    git(&root, &["commit", "-qm", "record note"]);
    let p = ingest::preview(&data, &root, &id).unwrap();
    assert_eq!(
        p["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["path"] == "note.md")
            .unwrap()["state"],
        "context_updated"
    );
    let result = capture(&data, &root, &id, &["note.md"]);
    assert_eq!(result["new_documents"], 0);
    assert_eq!(result["revision_count"], 2);
    std::fs::write(root.join(".git/index"), b"invalid index").unwrap();
    assert!(ingest::preview(&data, &root, &id)
        .unwrap_err()
        .to_string()
        .contains("Git 状态"));
}

#[test]
fn cache_named_project_is_scanned_but_nested_bare_repositories_are_not() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("build");
    let data = t.path().join("data");
    std::fs::create_dir(&root).unwrap();
    init(&data, &root, &t.path().join("knowledge"));
    std::fs::write(root.join("lesson.md"), "# Include").unwrap();
    std::fs::create_dir(root.join("dist")).unwrap();
    std::fs::write(root.join("dist/skip.md"), "# Skip").unwrap();
    let bare = root.join("nested.git");
    std::fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare", "-q"]);
    std::fs::write(bare.join("skip.md"), "# Skip nested bare").unwrap();
    let p = ingest::preview(&data, &root, &source(&data, &root, &root)).unwrap();
    assert_eq!(p["documents"].as_array().unwrap().len(), 1);
    assert_eq!(p["documents"][0]["path"], "lesson.md");
}

#[test]
fn worktree_knowledge_copies_and_marked_libraries_never_become_new_originals() {
    let t = tempfile::tempdir().unwrap();
    let root = common::make_business_repo(t.path(), "main");
    let wt = t.path().join("feature");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ],
    );
    let data = t.path().join("data");
    init(&data, &root, &root.join("knowledge"));
    for folder in [
        "knowledge/docs/organized",
        "other-library/docs/organized",
        "other-library/resources",
        "other-library/imports",
        "other-library/projects",
    ] {
        std::fs::create_dir_all(wt.join(folder)).unwrap();
        std::fs::write(
            wt.join(folder).join("not-original.md"),
            "# Generated or archived material\nMust not become fresh evidence",
        )
        .unwrap();
    }
    std::fs::copy(
        root.join("knowledge/.ailoom/library.json"),
        wt.join("other-library/ailoom-knowledge.json"),
    )
    .unwrap();
    std::fs::write(
        wt.join("actual-lesson.md"),
        "# Real lesson\nPreserve ordering",
    )
    .unwrap();
    let p = ingest::preview(&data, &root, &source(&data, &root, &wt)).unwrap();
    let paths: Vec<_> = p["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["README.md", "actual-lesson.md"]);
}
