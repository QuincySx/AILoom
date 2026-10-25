mod common;
use ailoom::knowledge::{
    location::{self, Request},
    organize, portable,
};
use serde_json::{json, Value};
use std::path::Path;

fn init(data: &Path, root: &Path, path: &Path) {
    location::run(
        data,
        &Request {
            action: "init".into(),
            root: root.into(),
            path: Some(path.into()),
            ..Default::default()
        },
    )
    .unwrap();
}
fn cli(data: &Path, cwd: &Path, args: &[&str]) -> Value {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ailoom"))
        .args(["--json", "--data-root", data.to_str().unwrap()])
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["result"].clone()
}
fn git(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_locates_url_path_and_partial_name_from_outside_the_repository() {
    common::isolate_in_process_roots();
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let sdk = t.path().join("projects/hardware-js-sdk");
    std::fs::create_dir_all(&sdk).unwrap();
    git(&sdk, &["init", "-q"]);
    git(
        &sdk,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:Example/Hardware-SDK.git",
        ],
    );
    let library = t.path().join("sdk-knowledge");
    init(&data, &sdk, &library);
    let id = portable::read(&sdk).unwrap().unwrap().knowledge_id;
    for query in [
        "JS SDK",
        "https://github.com/example/hardware-sdk",
        sdk.to_str().unwrap(),
        "./projects/hardware-js-sdk",
        "projects/hardware-js-sdk",
    ] {
        let found = cli(
            &data,
            t.path(),
            &["knowledge", "--action", "locate", "--query", query],
        );
        assert_eq!(found["status"], "found", "{found}");
        assert_eq!(found["candidates"][0]["knowledge_id"], id);
        assert_eq!(
            found["candidates"][0]["knowledge_path"],
            library.canonicalize().unwrap().to_str().unwrap()
        );
        assert!(found["candidates"][0].get("body").is_none());
        assert!(found["candidates"][0].get("documents").is_none());
    }
    // Root metadata supplies explicit canonical identity when Git uses a host alias.
    git(
        &sdk,
        &[
            "remote",
            "set-url",
            "origin",
            "git@work-github:Example/Hardware-SDK.git",
        ],
    );
    std::fs::write(
        sdk.join("package.json"),
        r#"{"repository":{"type":"git","url":"git+https://github.com/Example/Hardware-SDK.git"}}"#,
    )
    .unwrap();
    let declared = location::locate(&data, "https://github.com/example/hardware-sdk", 5).unwrap();
    assert_eq!(declared["status"], "found");
    assert_eq!(
        declared["candidates"][0]["match"],
        "declared_repository_url"
    );
    assert_eq!(
        declared["candidates"][0]["declared_repository_url"],
        "https://github.com/Example/Hardware-SDK.git"
    );
    // Credentials in metadata never become lookup evidence or returned URLs.
    std::fs::write(
        sdk.join("package.json"),
        r#"{"repository":"https://user:password@github.com/Example/Hardware-SDK.git"}"#,
    )
    .unwrap();
    assert_eq!(
        location::locate(&data, "https://github.com/example/hardware-sdk", 5).unwrap()["status"],
        "not_found"
    );
    let other = t.path().join("hardware-js-sdk-experiment");
    std::fs::create_dir(&other).unwrap();
    init(&data, &other, &t.path().join("other-knowledge"));
    let found = location::locate(&data, "js-sdk", 1).unwrap();
    assert_eq!(found["status"], "ambiguous");
    assert_eq!(found["total"], 2);
    assert_eq!(found["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(
        location::locate(&data, "https://github.com/other/hardware-sdk", 5).unwrap()["status"],
        "not_found"
    );
    assert_eq!(
        location::locate(&data, "unknown-sdk", 5).unwrap()["status"],
        "not_found"
    );
    assert!(location::locate(&data, "", 5).is_err());
    assert!(location::locate(&data, "---", 5).is_err());
}

#[test]
fn old_shared_library_splits_one_owner_at_a_time_without_copying_foreign_content() {
    common::isolate_in_process_roots();
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let a = t.path().join("app");
    let b = t.path().join("sdk");
    for root in [&a, &b] {
        std::fs::create_dir(root).unwrap();
    }
    let shared = t.path().join("old-shared");
    let old_b = t.path().join("old-sdk");
    init(&data, &a, &shared);
    init(&data, &b, &old_b);
    let da = portable::read(&a).unwrap().unwrap();
    let mut db = portable::read(&b).unwrap().unwrap();
    let old_a_id = da.knowledge_id.clone();
    // Reproduce persisted state written by the older multi-project library version.
    let path = data.join("knowledge/bindings.json");
    let mut cat: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let b_id = cat["projects"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, v)| Path::new(v["project_root"].as_str().unwrap()) == b.canonicalize().unwrap())
        .unwrap()
        .0
        .clone();
    cat["projects"][&b_id]["id"] = json!(old_a_id);
    cat["projects"][&b_id]["path"] = json!(shared);
    std::fs::write(&path, serde_json::to_vec_pretty(&cat).unwrap()).unwrap();
    std::fs::write(
        shared.join("ailoom-knowledge.json"),
        serde_json::to_vec(&json!({"schema_version":1,"id":old_a_id})).unwrap(),
    )
    .unwrap();
    db.knowledge_id = old_a_id.clone();
    portable::write(&b, &db).unwrap();
    let b_state = Path::new("projects")
        .join(&db.project_id)
        .join("state.json");
    let mut state: Value =
        serde_json::from_slice(&std::fs::read(old_b.join(&b_state)).unwrap()).unwrap();
    state["knowledge_id"] = json!(old_a_id);
    std::fs::create_dir_all(shared.join(&b_state).parent().unwrap()).unwrap();
    std::fs::write(shared.join(&b_state), serde_json::to_vec(&state).unwrap()).unwrap();
    let a_id = location::run(
        &data,
        &Request {
            action: "status".into(),
            root: a.clone(),
            ..Default::default()
        },
    )
    .unwrap()["project_id"]
        .as_str()
        .unwrap()
        .to_string();
    ailoom::personal_instructions::save_entry(
        &data,
        &a_id,
        None,
        "KEEP RESTORED PROJECT INSTRUCTION",
    )
    .unwrap();
    for (root, text) in [(&a, "# app-private-knowledge"), (&b, "# sdksecretunique")] {
        let file = t.path().join("note.md");
        std::fs::write(&file, text).unwrap();
        location::save(&data, root, &file, "note").unwrap();
    }
    let owned = format!(
        "---\nproject_id: {}\n---\n# historical-app-decision-unique",
        da.project_id
    );
    std::fs::write(shared.join("owned.md"), &owned).unwrap();
    std::fs::write(shared.join("ambiguous.md"), "# ownership unknown").unwrap();
    let before: Vec<_> = walkdir::WalkDir::new(&shared)
        .into_iter()
        .map(Result::unwrap)
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            (
                e.path().strip_prefix(&shared).unwrap().to_path_buf(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    let original_b = cat["projects"][&b_id].clone();
    // A fresh machine can register legacy ownership for splitting without restoring shared state.
    let restored_data = t.path().join("restored-data");
    let restored_a = t.path().join("restored-app");
    let restored_b = t.path().join("restored-sdk");
    for (root, d) in [(&restored_a, &da), (&restored_b, &db)] {
        std::fs::create_dir(root).unwrap();
        portable::write(root, d).unwrap();
        let request = Request {
            action: "recover".into(),
            root: root.clone(),
            path: Some(shared.clone()),
            ..Default::default()
        };
        let preview = location::run(&restored_data, &request).unwrap();
        let recovered = location::run(
            &restored_data,
            &Request {
                execute: true,
                expected: preview["expected"].as_str().map(String::from),
                ..request
            },
        )
        .unwrap();
        assert_eq!(recovered["needs_separation"], true);
    }
    let fresh_dest = t.path().join("restored-independent");
    let split = Request {
        action: "separate".into(),
        root: restored_a.clone(),
        path: Some(fresh_dest.clone()),
        ..Default::default()
    };
    let preview = location::run(&restored_data, &split).unwrap();
    let result = location::run(
        &restored_data,
        &Request {
            execute: true,
            expected: preview["expected"].as_str().map(String::from),
            ..split
        },
    )
    .unwrap();
    assert_eq!(result["recovery_required"], true);
    let checkpoint = Request {
        action: "checkpoint".into(),
        root: restored_a.clone(),
        ..Default::default()
    };
    assert!(location::run(&restored_data, &checkpoint).is_err());
    let state_path = fresh_dest
        .join("projects")
        .join(&da.project_id)
        .join("state.json");
    let preserved = std::fs::read(&state_path).unwrap();
    assert!(String::from_utf8_lossy(&preserved).contains("KEEP RESTORED PROJECT INSTRUCTION"));
    let recovery = Request {
        action: "recover".into(),
        root: restored_a,
        path: Some(fresh_dest),
        ..Default::default()
    };
    let preview = location::run(&restored_data, &recovery).unwrap();
    location::run(
        &restored_data,
        &Request {
            execute: true,
            expected: preview["expected"].as_str().map(String::from),
            ..recovery
        },
    )
    .unwrap();
    location::run(&restored_data, &checkpoint).unwrap();
    let recovered: Value = serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(
        recovered["instructions"]["project"],
        "KEEP RESTORED PROJECT INSTRUCTION"
    );
    let dest = t.path().join("app-knowledge");
    let preview = cli(
        &data,
        t.path(),
        &[
            "knowledge",
            "--action",
            "separate",
            "--root",
            a.to_str().unwrap(),
            "--path",
            dest.to_str().unwrap(),
        ],
    );
    assert!(!dest.exists());
    assert_eq!(preview["unassigned_sources"], json!(["ambiguous.md"]));
    assert_eq!(preview["affected_projects"].as_array().unwrap().len(), 1);
    let result = cli(
        &data,
        t.path(),
        &[
            "knowledge",
            "--action",
            "separate",
            "--root",
            a.to_str().unwrap(),
            "--path",
            dest.to_str().unwrap(),
            "--execute",
            "--expected",
            preview["expected"].as_str().unwrap(),
        ],
    );
    let next = portable::read(&a).unwrap().unwrap();
    assert_eq!(next.project_id, da.project_id);
    assert_ne!(next.knowledge_id, da.knowledge_id);
    assert!(!dest.join("projects").join(&db.project_id).exists());
    assert!(!dest.join("ambiguous.md").exists());
    assert_eq!(
        std::fs::read_to_string(dest.join("owned.md")).unwrap(),
        owned
    );
    assert!(result["separated"].as_bool().unwrap());
    let current: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(current["projects"][&b_id], original_b);
    assert!(
        location::recall(&data, &a, "sdksecretunique", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(organize::status(&data, &a).unwrap()["source_count"], 2);
    for (p, bytes) in before {
        assert_eq!(std::fs::read(shared.join(p)).unwrap(), bytes);
    }
    let sdk_dest = t.path().join("sdk-knowledge");
    let r = Request {
        action: "separate".into(),
        root: b.clone(),
        path: Some(sdk_dest),
        ..Default::default()
    };
    let preview = location::run(&data, &r).unwrap();
    location::run(
        &data,
        &Request {
            execute: true,
            expected: preview["expected"].as_str().map(String::from),
            ..r
        },
    )
    .unwrap();
    let sdk = portable::read(&b).unwrap().unwrap();
    assert_ne!(sdk.knowledge_id, next.knowledge_id);
    assert_ne!(sdk.knowledge_id, old_a_id);
    assert!(location::run(
        &data,
        &Request {
            action: "checkpoint".into(),
            root: a.clone(),
            ..Default::default()
        }
    )
    .is_ok());
    let found = location::locate(&data, "sdk", 5).unwrap();
    assert_eq!(found["candidates"][0]["knowledge_id"], sdk.knowledge_id);
}

#[test]
fn another_repository_cannot_adopt_an_existing_library_through_sync_or_recovery() {
    common::isolate_in_process_roots();
    let t = tempfile::tempdir().unwrap();
    let bare = t.path().join("remote.git");
    git(t.path(), &["init", "--bare", "-q", bare.to_str().unwrap()]);
    let a = t.path().join("app");
    let b = t.path().join("sdk");
    let da = t.path().join("data-a");
    let db = t.path().join("data-b");
    let ka = t.path().join("knowledge-a");
    let kb = t.path().join("knowledge-b");
    for (data, root, path) in [(&da, &a, &ka), (&db, &b, &kb)] {
        std::fs::create_dir(root).unwrap();
        location::run(
            data,
            &Request {
                action: "init".into(),
                root: root.clone(),
                path: Some(path.clone()),
                remote: Some(bare.to_string_lossy().into()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    location::run(
        &da,
        &Request {
            action: "sync".into(),
            root: a.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    let before = std::fs::read(kb.join("ailoom-knowledge.json")).unwrap();
    let declaration = portable::read(&b).unwrap().unwrap();
    let error = location::run(
        &db,
        &Request {
            action: "sync".into(),
            root: b.clone(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("其他仓库"), "{error}");
    assert_eq!(
        std::fs::read(kb.join("ailoom-knowledge.json")).unwrap(),
        before
    );
    assert_eq!(
        portable::read(&b).unwrap().unwrap().knowledge_id,
        declaration.knowledge_id
    );
    // Even a matching knowledge UUID is insufficient when the marker belongs to another project.
    let mut wrong = declaration;
    wrong.knowledge_id = portable::read(&a).unwrap().unwrap().knowledge_id;
    portable::write(&b, &wrong).unwrap();
    assert!(location::run(
        &t.path().join("fresh-data"),
        &Request {
            action: "recover".into(),
            root: b,
            path: Some(ka),
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn separation_updates_existing_worktree_declarations_and_shared_repository_binding() {
    common::isolate_in_process_roots();
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let root = t.path().join("repository");
    let worktree = t.path().join("feature-worktree");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    init(&data, &root, &t.path().join("old-library"));
    git(&root, &["add", ".ailoom/knowledge.json"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "Portable project identity",
        ],
    );
    git(
        &root,
        &[
            "worktree",
            "add",
            "-qb",
            "feature",
            worktree.to_str().unwrap(),
        ],
    );
    let before = portable::read(&root).unwrap().unwrap();
    assert_eq!(
        portable::read(&worktree).unwrap().unwrap().knowledge_id,
        before.knowledge_id
    );
    let request = Request {
        action: "separate".into(),
        root: root.clone(),
        path: Some(t.path().join("independent")),
        ..Default::default()
    };
    let preview = location::run(&data, &request).unwrap();
    location::run(
        &data,
        &Request {
            execute: true,
            expected: preview["expected"].as_str().map(String::from),
            ..request
        },
    )
    .unwrap();
    let after = portable::read(&root).unwrap().unwrap();
    let branch = portable::read(&worktree).unwrap().unwrap();
    assert_ne!(before.knowledge_id, after.knowledge_id);
    assert_eq!(branch.project_id, before.project_id);
    assert_eq!(branch.knowledge_id, after.knowledge_id);
    let status = location::run(
        &data,
        &Request {
            action: "status".into(),
            root: worktree,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(status["location"]["id"], after.knowledge_id);
    assert_eq!(status["needs_separation"], false);
}
