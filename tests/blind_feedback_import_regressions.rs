//! Regressions from the independent CLI blind test (R04 and R06).
mod common;

use ailoom::appctx::AppContext;
use ailoom::knowledge::feedback::{record_feedback, record_recall_hits};
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Barrier};

fn cli(root: &Path, data: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ailoom"))
        .args(["--json", "--data-root", data.to_str().unwrap()])
        .args(args)
        .current_dir(cwd)
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("CLAUDE_CONFIG_DIR", root.join("claude-config"))
        .env("PI_CODING_AGENT_DIR", root.join("pi-config"))
        .env("AILOOM_REPORTING", "off")
        .env("AILOOM_AUTO_SYNC", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

fn success(out: Output) -> Value {
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["result"].clone()
}

fn manual_context(root: &Path) -> AppContext {
    let workspace_root = root.join("business");
    let workspace = ailoom::workspace::Workspace {
        workspace_id: ailoom::ids::workspace_id_from_root(&workspace_root),
        workspace_root,
        repository_anchor: "path+feedback-regression".into(),
        anchor_key: "feedback-regression".into(),
        is_git: false,
        declaration_path: None,
    };
    let data_root = root.join("data");
    AppContext {
        layout: ailoom::paths::layout_for(
            &data_root,
            &workspace.workspace_id,
            &workspace.anchor_key,
        ),
        data_root,
        workspace,
        device: "synthetic-device".into(),
    }
}

#[test]
fn stable_feedback_identity_rejects_direction_and_learning_conflicts_without_mutation() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = manual_context(tmp.path());
    record_recall_hits(&ctx, &["lesson-a".into(), "lesson-b".into()]).unwrap();
    record_feedback(&ctx, "lesson-a", true, Some("stable-event")).unwrap();
    let path = ctx.layout.ws_dir.join("knowledge-usage.json");
    let original = std::fs::read(&path).unwrap();
    record_feedback(&ctx, "lesson-a", true, Some("stable-event")).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    for (id, useful) in [("lesson-a", false), ("lesson-b", true)] {
        let err = record_feedback(&ctx, id, useful, Some("stable-event")).unwrap_err();
        assert_eq!(err.code, ailoom::error::code::KNOWLEDGE_STATE_CONFLICT);
        assert!(err.message.contains("stable-event"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    record_feedback(&ctx, "lesson-a", true, Some("stable-event")).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let state: Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(state["feedback"]["lesson-a"], json!([1, 0]));
    assert!(state["feedback"].get("lesson-b").is_none());
}

#[test]
fn concurrent_recall_and_distinct_feedback_preserve_all_counters() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = Arc::new(manual_context(tmp.path()));
    record_recall_hits(&ctx, &["lesson-a".into()]).unwrap();
    let barrier = Arc::new(Barrier::new(16));
    let handles: Vec<_> = (0..16)
        .map(|i| {
            let ctx = Arc::clone(&ctx);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                if i % 2 == 0 {
                    record_recall_hits(&ctx, &["lesson-a".into()])
                } else {
                    record_feedback(&ctx, "lesson-a", true, Some(&format!("event-{i}")))
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap().unwrap();
    }
    let state: Value = serde_json::from_slice(
        &std::fs::read(ctx.layout.ws_dir.join("knowledge-usage.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["usage"]["lesson-a"], 9);
    assert_eq!(state["feedback"]["lesson-a"], json!([8, 0]));
    assert_eq!(state["feedback_events"].as_object().unwrap().len(), 8);
}

#[test]
fn concurrent_feedback_cli_returns_atomic_duplicate_and_conflict_results() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let data = root.join("data");
    let ws = root.join("business");
    let kb = root.join("knowledge");
    std::fs::create_dir_all(&ws).unwrap();
    let init = success(cli(
        root,
        &data,
        &ws,
        &[
            "knowledge",
            "--action",
            "init",
            "--path",
            kb.to_str().unwrap(),
        ],
    ));
    let lesson = root.join("lesson.md");
    std::fs::write(&lesson, "---\ntitle: cache lesson\n---\n# cache lesson\n").unwrap();
    success(cli(
        root,
        &data,
        &ws,
        &[
            "knowledge",
            "--action",
            "save",
            "--file",
            lesson.to_str().unwrap(),
            "--name",
            "lesson",
        ],
    ));
    success(cli(
        root,
        &data,
        &ws,
        &["knowledge", "--action", "recall", "--query", "cache"],
    ));
    let path = data
        .join("knowledge/state")
        .join(init["location"]["id"].as_str().unwrap())
        .join("knowledge-usage.json");
    for mixed in [false, true] {
        let barrier = Arc::new(Barrier::new(8));
        let outputs = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let barrier = Arc::clone(&barrier);
                    let data = &data;
                    let ws = &ws;
                    scope.spawn(move || {
                        let mut args = vec![
                            "knowledge",
                            "--action",
                            "feedback",
                            "--id",
                            "learnings/lesson.md",
                            "--feedback-id",
                            if mixed { "mixed-event" } else { "same-event" },
                        ];
                        if !mixed || i % 2 == 0 {
                            args.push("--useful");
                        }
                        barrier.wait();
                        cli(root, data, ws, &args)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        let mut accepted = 0;
        let mut new_votes = 0;
        let mut conflicts = 0;
        for out in outputs {
            if out.status.success() {
                accepted += 1;
                if !success(out)["duplicated"].as_bool().unwrap() {
                    new_votes += 1;
                }
            } else {
                let error: Value = serde_json::from_slice(&out.stderr).unwrap();
                assert_eq!(error["code"], ailoom::error::code::KNOWLEDGE_STATE_CONFLICT);
                conflicts += 1;
            }
        }
        assert_eq!(new_votes, 1);
        assert_eq!(accepted, if mixed { 4 } else { 8 });
        assert_eq!(conflicts, if mixed { 4 } else { 0 });
    }
    let state: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let counts = state["feedback"]["learnings/lesson.md"].as_array().unwrap();
    assert_eq!(counts.iter().map(|n| n.as_u64().unwrap()).sum::<u64>(), 2);
    assert_eq!(state["feedback_events"].as_object().unwrap().len(), 2);
}

fn assert_import_commit(bare: &Path, result: &Value, imported: u64) {
    assert_eq!(result["imported"], imported);
    assert_eq!(result["no_changes"], false);
    let commit = result["commit"].as_str().unwrap();
    let head = ailoom::gitx::git(bare, &["rev-parse", result["branch"].as_str().unwrap()]).unwrap();
    assert_eq!(head, commit);
    let diff = ailoom::gitx::git(
        bare,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", commit],
    )
    .unwrap();
    let paths: Vec<_> = diff.lines().collect();
    assert_eq!(result["committed_paths"], json!(paths));
    assert_eq!(paths.len() as u64, imported);
}

#[test]
fn import_reports_actual_doc_and_learning_commits_and_in_place_updates_on_local_bare_git() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let bare = root.join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let team = common::make_team_source(root);
    ailoom::gitx::git(&team, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(&team, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
    ailoom::gitx::git(&team, &["push", "-q", "-u", "origin", "main"]).unwrap();
    let ws = common::make_business_repo(root, "business");
    let data = root.join("data");
    success(cli(
        root,
        &data,
        &ws,
        &["init", "--url", bare.to_str().unwrap(), "--project", "a"],
    ));
    let docs = root.join("import-docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("a.md"), "# cache one\nfirst body\n").unwrap();
    std::fs::write(docs.join("b.md"), "# cache two\nsecond body\n").unwrap();
    let args = |kind| {
        vec![
            "import",
            "--dir",
            docs.to_str().unwrap(),
            "--target",
            "project:a",
            "--kind",
            kind,
            "--execute",
        ]
    };
    let first = success(cli(root, &data, &ws, &args("doc")));
    assert_import_commit(&bare, &first, 2);
    let again = success(cli(root, &data, &ws, &args("doc")));
    assert_eq!(again["imported"], 0);
    assert_eq!(again["skipped_unchanged"], 2);
    assert_eq!(
        ailoom::gitx::git(&bare, &["rev-parse", first["branch"].as_str().unwrap()]).unwrap(),
        first["commit"].as_str().unwrap()
    );
    std::fs::write(docs.join("a.md"), "# cache one\nupdated body\n").unwrap();
    let update = success(cli(root, &data, &ws, &args("doc")));
    assert_import_commit(&bare, &update, 1);
    assert_eq!(update["committed_paths"][0], first["committed_paths"][0]);
    let learning = success(cli(root, &data, &ws, &args("learning")));
    assert_import_commit(&bare, &learning, 2);
    assert!(learning["committed_paths"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p.as_str().unwrap().starts_with("resources/learnings/")));
}
