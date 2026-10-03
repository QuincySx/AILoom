use ailoom::{
    knowledge::{
        location::{self, Request},
        portable::{self, Source},
    },
    profile::{PersonalProfile, RepoProfile, ScopeSelection, TriState},
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
fn run(data: &Path, root: &Path, action: &str, path: Option<&Path>) -> Value {
    location::run(
        data,
        &Request {
            action: action.into(),
            root: root.into(),
            path: path.map(Into::into),
            ..Default::default()
        },
    )
    .unwrap()
}
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in walkdir::WalkDir::new(from).min_depth(1) {
        let e = e.unwrap();
        let p = to.join(e.path().strip_prefix(from).unwrap());
        if e.file_type().is_dir() {
            std::fs::create_dir_all(p).unwrap();
        } else {
            std::fs::copy(e.path(), p).unwrap();
        }
    }
}
fn recover(data: &Path, root: &Path, path: Option<&Path>) -> Value {
    let p = run(data, root, "recover", path);
    location::run(
        data,
        &Request {
            action: "recover".into(),
            root: root.into(),
            path: path.map(Into::into),
            execute: true,
            expected: p["expected"].as_str().map(Into::into),
            ..Default::default()
        },
    )
    .unwrap()
}
fn git(root: &Path, args: &[&str]) -> String {
    let result = std::process::Command::new("git")
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
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8_lossy(&result.stdout).trim().into()
}
#[test]
fn project_relative_recovery_keeps_uuid_after_machine_and_path_change() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    let da = t.path().join("da");
    let db = t.path().join("db");
    std::fs::create_dir(&a).unwrap();
    let old = run(&da, &a, "init", Some(&a.join("knowledge")));
    let declaration = std::fs::read_to_string(a.join(portable::DECLARATION)).unwrap();
    assert!(!declaration.contains(t.path().to_str().unwrap()));
    copy(&a, &b);
    let status = run(&db, &b, "status", None);
    assert_eq!(status["initialized"], false);
    assert!(status["recovery"].is_object());
    assert!(!db.join("repos").exists(), "inspection must not register");
    let new = recover(&db, &b, None);
    assert_eq!(old["location"]["id"], new["location"]["id"]);
    assert_eq!(
        portable::read(&a).unwrap().unwrap().project_id,
        portable::read(&b).unwrap().unwrap().project_id
    );
    assert_ne!(old["project_id"], new["project_id"]);
    assert_eq!(run(&db, &b, "status", None)["initialized"], true);
}
#[test]
fn external_folder_recovers_selected_skill_rule_agent_and_instructions_offline() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    let da = t.path().join("da");
    let db = t.path().join("db");
    let ka = t.path().join("ka");
    let kb = t.path().join("kb");
    std::fs::create_dir(&a).unwrap();
    let init = run(&da, &a, "init", Some(&ka));
    let repo = init["project_id"].as_str().unwrap();
    let lib = ailoom::personal_library::ensure_library(&da).unwrap().path;
    std::fs::create_dir_all(lib.join("resources/skills/search")).unwrap();
    std::fs::write(
        lib.join("resources/skills/search/SKILL.md"),
        "---\nname: search\ndescription: Search\nnamespace: personal\nshared: true\n---\n# Search",
    )
    .unwrap();
    std::fs::write(
        lib.join("resources/rules/style.md"),
        "---\nnamespace: personal\nshared: true\n---\n# Style",
    )
    .unwrap();
    std::fs::write(lib.join("resources/agents/reviewer.toml"),"name = \"reviewer\"\nnamespace = \"personal\"\nshared = true\ndescription = \"Review\"\ninstructions = \"Review code\"\n").unwrap();
    let mut p = PersonalProfile::load_or_default(&da).unwrap();
    p.repos.insert(
        repo.into(),
        RepoProfile {
            default: Some(ScopeSelection {
                hosts: BTreeMap::from([("claude".into(), TriState::Enable)]),
                resources: [
                    "personal/skill/personal/search",
                    "personal/rule/personal/style",
                    "personal/agent/personal/reviewer",
                ]
                .into_iter()
                .map(|s| (s.into(), TriState::Enable))
                .collect(),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    p.save(&da).unwrap();
    ailoom::personal_instructions::save_entry(&da, repo, None, "# Project conventions").unwrap();
    copy(&a, &b);
    copy(&ka, &kb);
    assert!(matches!(
        portable::read(&a).unwrap().unwrap().source,
        Source::Directory
    ));
    recover(&db, &b, Some(&kb));
    let status = run(&db, &b, "status", None);
    let new_repo = status["project_id"].as_str().unwrap();
    let p = PersonalProfile::load_or_default(&db).unwrap();
    let selection = p.repos[new_repo].default.as_ref().unwrap();
    assert_eq!(selection.resources.len(), 3);
    assert_eq!(
        ailoom::personal_instructions::load_entry(&db, new_repo, None).unwrap(),
        "# Project conventions"
    );
    let registry = ailoom::collections::load(&db).unwrap();
    let entries: Vec<_> = registry
        .sources
        .values()
        .flat_map(|s| ailoom::collections::catalog(&db, s).unwrap().entries)
        .collect();
    assert_eq!(entries.len(), 3);
    assert!(entries
        .iter()
        .all(|e| selection.resources.contains_key(&e.id.to_string())));
    assert!(
        !b.join(".claude").exists(),
        "restore must not deploy automatically"
    );
    run(&db, &b, "checkpoint", None); // republishing a restored package remains stable
    let plan =
        ailoom::commands::personal::prepare_personal(Some(&db), Some(&b), None, &db).unwrap();
    assert!(plan
        .artifacts
        .iter()
        .any(|a| a.path.ends_with("agents/reviewer.md")));
    assert!(plan
        .artifacts
        .iter()
        .any(|a| a.path.ends_with("skills/search")));
    let applied = ailoom::commands::personal::apply_prepared_personal(&plan).unwrap();
    assert!(applied.ok);
    assert!(b.join(".claude/agents/reviewer.md").exists());
    assert!(b.join(".claude/skills/search/SKILL.md").exists());
    let moved = t.path().join("moved-knowledge");
    let preview = run(&db, &b, "move", Some(&moved));
    location::run(
        &db,
        &Request {
            action: "move".into(),
            root: b.clone(),
            path: Some(moved.clone()),
            execute: true,
            expected: preview["expected"].as_str().map(Into::into),
            ..Default::default()
        },
    )
    .unwrap();
    let moved = moved.canonicalize().unwrap();
    assert!(ailoom::collections::load(&db)
        .unwrap()
        .sources
        .values()
        .all(|s| s.external_path.as_ref().unwrap().starts_with(&moved)));
    let updated =
        ailoom::commands::personal::prepare_personal(Some(&db), Some(&b), None, &db).unwrap();
    assert!(
        ailoom::commands::personal::apply_prepared_personal(&updated)
            .unwrap()
            .ok
    );
    assert!(std::fs::read_link(b.join(".claude/skills/search"))
        .unwrap()
        .starts_with(&moved));
}
#[test]
fn wrong_identity_stale_preview_and_symlink_are_rejected_without_binding() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    let da = t.path().join("da");
    let db = t.path().join("db");
    let ka = t.path().join("ka");
    let kb = t.path().join("kb");
    std::fs::create_dir(&a).unwrap();
    run(&da, &a, "init", Some(&ka));
    copy(&a, &b);
    copy(&ka, &kb);
    let p = run(&db, &b, "recover", Some(&kb));
    std::fs::write(kb.join("docs/new.md"), "# New").unwrap();
    let mut r = Request {
        action: "recover".into(),
        root: b.clone(),
        path: Some(kb.clone()),
        execute: true,
        expected: p["expected"].as_str().map(Into::into),
        ..Default::default()
    };
    assert!(location::run(&db, &r).is_err());
    assert!(!db.join("knowledge/bindings.json").exists());
    std::fs::write(
        kb.join("ailoom-knowledge.json"),
        serde_json::to_vec(
            &serde_json::json!({"schema_version":1,"id":uuid::Uuid::new_v4().to_string()}),
        )
        .unwrap(),
    )
    .unwrap();
    r.execute = false;
    assert!(location::run(&db, &r).is_err());
    #[cfg(unix)]
    {
        std::fs::remove_file(b.join(portable::DECLARATION)).unwrap();
        std::os::unix::fs::symlink(a.join(portable::DECLARATION), b.join(portable::DECLARATION))
            .unwrap();
        assert!(location::run(
            &db,
            &Request {
                action: "status".into(),
                root: b,
                ..Default::default()
            }
        )
        .is_err());
    }
}
#[test]
fn worktrees_share_identity_and_git_hint_tracks_repo_subdirectory() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("main");
    let wt = t.path().join("feature");
    let data = t.path().join("data");
    let kb = t.path().join("knowledge-repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&kb).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["commit", "--allow-empty", "-m", "init"]);
    git(
        &root,
        &["worktree", "add", "-b", "feature", wt.to_str().unwrap()],
    );
    git(&kb, &["init", "-q", "-b", "knowledge"]);
    git(
        &kb,
        &[
            "remote",
            "add",
            "origin",
            "git@example.com:team/knowledge.git",
        ],
    );
    let a = run(&data, &wt, "init", Some(&kb.join("apps/a")));
    let b = run(&data, &root, "status", None);
    assert_eq!(a["location"]["id"], b["location"]["id"]);
    let d = portable::read(&root).unwrap().unwrap();
    match d.source {
        Source::Git {
            url,
            branch,
            subdir,
        } => {
            assert_eq!(url, "git@example.com:team/knowledge.git");
            assert_eq!(branch.as_deref(), Some("knowledge"));
            assert_eq!(subdir, "apps/a");
        }
        _ => panic!("missing Git locator"),
    }
    let before = d.project_id;
    git(
        &kb,
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.com/team/renamed.git",
        ],
    );
    run(&data, &root, "checkpoint", None);
    assert_eq!(portable::read(&root).unwrap().unwrap().project_id, before);
}
#[test]
fn concurrent_device_state_does_not_silently_overwrite() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let data = t.path().join("data");
    let kb = t.path().join("kb");
    std::fs::create_dir(&root).unwrap();
    run(&data, &root, "init", Some(&kb));
    let d = portable::read(&root).unwrap().unwrap();
    let state = kb.join("projects").join(d.project_id).join("state.json");
    let mut other: Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    other["profile"] = serde_json::json!({"default":{"hosts":{"codex":"enable"}}});
    std::fs::write(&state, serde_json::to_vec(&other).unwrap()).unwrap();
    assert!(location::run(
        &data,
        &Request {
            action: "checkpoint".into(),
            root,
            ..Default::default()
        }
    )
    .is_err());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(state).unwrap()).unwrap(),
        other
    );
}
#[test]
fn git_clone_restores_subdirectory_without_commits_or_hooks() {
    struct Daemon(std::process::Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let data = t.path().join("data");
    let repo = t.path().join("kb.git");
    let dest = t.path().join("checkout");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let url = format!("git://127.0.0.1:{port}/kb.git");
    git(&repo, &["remote", "add", "origin", &url]);
    run(&data, &root, "init", Some(&repo.join("apps/one")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "knowledge"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    // git 会再启动 git-daemon；直接持有监听进程，避免只终止外层 git 后遗留子进程。
    let daemon_exe = Path::new(&git(&repo, &["--exec-path"]))
        .join(format!("git-daemon{}", std::env::consts::EXE_SUFFIX));
    let daemon = Daemon(
        std::process::Command::new(daemon_exe)
            .args([
                "--export-all",
                "--listen=127.0.0.1",
                &format!("--port={port}"),
                &format!("--base-path={}", t.path().display()),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut ready = false;
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(ready);
    let cloned = run(&data, &root, "clone", Some(&dest));
    assert_eq!(
        cloned["path"],
        dest.canonicalize()
            .unwrap()
            .join("apps/one")
            .to_str()
            .unwrap()
    );
    assert_eq!(git(&dest, &["rev-parse", "HEAD"]), head);
    assert!(git(&dest, &["status", "--porcelain"]).is_empty());
    assert!(location::run(
        &data,
        &Request {
            action: "clone".into(),
            root: root.clone(),
            path: Some(dest),
            ..Default::default()
        }
    )
    .is_err());
    // A restored Git resource keeps its update source while remaining usable offline.
    let source = t.path().join("skills.git");
    std::fs::create_dir(&source).unwrap();
    git(&source, &["init", "-q", "-b", "main"]);
    let skill = "---\nname: remote-skill\ndescription: Remote\n---\nVersion one";
    std::fs::write(source.join("SKILL.md"), skill).unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "v1"]);
    let source_url = format!("git://127.0.0.1:{port}/skills.git");
    let preview =
        ailoom::collections::preview(&data, "Remote skills", &source_url, Some("main"), None)
            .unwrap();
    let added =
        ailoom::collections::apply_preview(&data, preview["preview_id"].as_str().unwrap()).unwrap();
    let source_id = added["source"]["id"].as_str().unwrap();
    let sources = ailoom::collections::load(&data).unwrap();
    let resource = ailoom::collections::catalog(&data, &sources.sources[source_id])
        .unwrap()
        .entries[0]
        .id
        .to_string();
    let status = run(&data, &root, "status", None);
    let repo_id = status["project_id"].as_str().unwrap();
    let mut profile = PersonalProfile::load_or_default(&data).unwrap();
    profile.repos.insert(
        repo_id.into(),
        RepoProfile {
            default: Some(ScopeSelection {
                resources: BTreeMap::from([(resource, TriState::Enable)]),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    profile.save(&data).unwrap();
    run(&data, &root, "checkpoint", None);
    let new_root = t.path().join("new-project");
    let new_kb = t.path().join("new-kb");
    let new_data = t.path().join("new-data");
    copy(&root, &new_root);
    copy(&repo.join("apps/one"), &new_kb);
    recover(&new_data, &new_root, Some(&new_kb));
    let restored = ailoom::collections::load(&new_data).unwrap();
    let remote = restored.sources.values().next().unwrap();
    assert_eq!(remote.url, source_url);
    assert!(remote.external_path.is_some());
    let portable_id = remote.id.clone();
    std::fs::write(
        source.join("SKILL.md"),
        skill.replace("Version one", "Version two"),
    )
    .unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "v2"]);
    let check = ailoom::collections::check_updates(&new_data, Some(&portable_id)).unwrap();
    assert_eq!(check["items"][0]["state"], "available");
    let update_token = check["items"][0]["preview"]["preview_id"].as_str().unwrap();
    ailoom::collections::apply_preview(&new_data, update_token).unwrap();
    let updated = ailoom::collections::load(&new_data).unwrap();
    assert!(updated.sources[&portable_id].external_path.is_none());
    assert!(
        ailoom::collections::catalog(&new_data, &updated.sources[&portable_id])
            .unwrap()
            .entries[0]
            .raw
            .as_ref()
            .unwrap()
            .contains("Version two")
    );
    run(&new_data, &new_root, "checkpoint", None);
    drop(daemon);
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "测试结束后 Git daemon 必须释放监听端口 {port}"
    );
}
#[test]
fn missing_local_binding_location_can_be_recovered() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let data = t.path().join("data");
    let kb = t.path().join("kb");
    let moved = t.path().join("moved");
    std::fs::create_dir(&root).unwrap();
    let original = run(&data, &root, "init", Some(&kb));
    std::fs::rename(&kb, &moved).unwrap();
    let status = run(&data, &root, "status", None);
    assert_eq!(status["initialized"], false);
    assert!(status["recovery"].is_object());
    let restored = recover(&data, &root, Some(&moved));
    assert_eq!(restored["location"]["id"], original["location"]["id"]);
}

#[test]
fn separate_checkouts_on_one_machine_have_independent_conflict_baselines() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    let data = t.path().join("data");
    let kb = t.path().join("knowledge");
    std::fs::create_dir(&a).unwrap();
    let first = run(&data, &a, "init", Some(&kb));
    copy(&a, &b);
    let second = recover(&data, &b, Some(&kb));
    ailoom::personal_instructions::save_entry(
        &data,
        first["project_id"].as_str().unwrap(),
        None,
        "First checkout edit",
    )
    .unwrap();
    assert!(ailoom::personal_instructions::save_entry(
        &data,
        second["project_id"].as_str().unwrap(),
        None,
        "Stale checkout edit"
    )
    .is_err());
    let d = portable::read(&a).unwrap().unwrap();
    let state: Value = serde_json::from_slice(
        &std::fs::read(kb.join("projects").join(d.project_id).join("state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["instructions"]["project"], "First checkout edit");
}
