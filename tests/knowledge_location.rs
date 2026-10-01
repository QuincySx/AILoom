use ailoom::knowledge::location::{self, Request};
use serde_json::Value;
use std::path::{Path, PathBuf};
fn req(action: &str, root: &Path, path: Option<&Path>) -> Request {
    Request {
        action: action.into(),
        root: root.into(),
        path: path.map(Path::to_path_buf),
        ..Default::default()
    }
}
fn git(p: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
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
        .current_dir(p)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into()
}
fn init(data: &Path, root: &Path, path: &Path) -> Value {
    location::run(data, &req("init", root, Some(path))).unwrap()
}
fn move_to(data: &Path, root: &Path, dest: &Path) -> Value {
    let mut r = req("move", root, Some(dest));
    let p = location::run(data, &r).unwrap();
    r.execute = true;
    r.expected = p["expected"].as_str().map(str::to_owned);
    location::run(data, &r).unwrap()
}
fn note(data: &Path, root: &Path, dir: &Path, name: &str, body: &str) {
    let f = dir.join("draft.md");
    std::fs::write(&f, body).unwrap();
    location::save(data, root, &f, name).unwrap();
}
#[test]
fn folder_move_preserves_content_and_cli_read_write_location() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let data = t.path().join("data");
    let first = root.join("knowledge");
    let next = t.path().join("shared/app-a");
    init(&data, &root, &first);
    note(
        &data,
        &root,
        t.path(),
        "lesson",
        "# 迁移知识\n数据库迁移必须验证内容",
    );
    let before = location::recall(&data, &root, "迁移", 5).unwrap();
    assert_eq!(before["results"].as_array().unwrap().len(), 1);
    let result = move_to(&data, &root.join("docs"), &next);
    assert_eq!(
        result["backup"],
        first.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(first.join("learnings/lesson.md").exists());
    note(&data, &root, t.path(), "after", "# 新位置\n迁移后的知识");
    assert!(next.join("learnings/after.md").exists());
    assert!(!first.join("learnings/after.md").exists());
    assert_eq!(
        location::recall(&data, &root, "迁移", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
#[test]
fn worktrees_and_subdirectories_share_binding() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["commit", "--allow-empty", "-qm", "initial"]);
    let wt = t.path().join("feature");
    git(
        &root,
        &["worktree", "add", "-qb", "feature", wt.to_str().unwrap()],
    );
    std::fs::create_dir(wt.join("docs")).unwrap();
    let data = t.path().join("data");
    let first = t.path().join("knowledge");
    init(&data, &root, &first);
    note(
        &data,
        &wt.join("docs"),
        t.path(),
        "feature",
        "# 共享\nWorktree 共享知识",
    );
    let next = t.path().join("moved");
    move_to(&data, &wt, &next);
    let a = location::run(&data, &req("status", &root, None)).unwrap();
    let b = location::run(&data, &req("status", &wt, None)).unwrap();
    assert_eq!(a["location"], b["location"]);
    assert_eq!(
        a["location"]["path"],
        next.canonicalize().unwrap().to_str().unwrap()
    );
}
#[test]
fn conflicts_stale_preview_and_nested_paths_do_not_switch() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let data = t.path().join("data");
    let first = t.path().join("first");
    init(&data, &root, &first);
    let target = t.path().join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep"), "mine").unwrap();
    assert!(location::run(&data, &req("move", &root, Some(&target))).is_err());
    assert!(location::run(&data, &req("move", &root, Some(&first.join("nested")))).is_err());
    let dest = t.path().join("new");
    let mut r = req("move", &root, Some(&dest));
    let preview = location::run(&data, &r).unwrap();
    note(&data, &root, t.path(), "late", "# Late note");
    r.execute = true;
    r.expected = preview["expected"].as_str().map(str::to_owned);
    assert!(location::run(&data, &r).is_err());
    assert_eq!(
        location::run(&data, &req("status", &root, None)).unwrap()["location"]["path"],
        first.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(!dest.exists());
    assert_eq!(
        std::fs::read_to_string(target.join("keep")).unwrap(),
        "mine"
    );
}
#[test]
fn shared_knowledge_moves_all_project_links_without_mixing_other_projects() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let roots: Vec<PathBuf> = (0..3)
        .map(|i| {
            let p = t.path().join(format!("p{i}"));
            std::fs::create_dir(&p).unwrap();
            p
        })
        .collect();
    let old = t.path().join("old");
    init(&data, &roots[0], &old);
    init(&data, &roots[1], &old);
    let separate = t.path().join("separate");
    init(&data, &roots[2], &separate);
    let dest = t.path().join("moved");
    let result = move_to(&data, &roots[0], &dest);
    assert_eq!(result["affected_projects"].as_array().unwrap().len(), 2);
    assert_eq!(
        location::run(&data, &req("status", &roots[1], None)).unwrap()["location"]["path"],
        dest.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        location::run(&data, &req("status", &roots[2], None)).unwrap()["location"]["path"],
        separate.canonicalize().unwrap().to_str().unwrap()
    );
}
#[cfg(unix)]
#[test]
fn symlinks_are_rejected_before_copy() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let data = t.path().join("data");
    let first = t.path().join("first");
    init(&data, &root, &first);
    std::os::unix::fs::symlink(&root, first.join("escape")).unwrap();
    let target = t.path().join("target");
    assert!(location::run(&data, &req("move", &root, Some(&target))).is_err());
    assert!(!target.exists());
}
#[test]
fn git_sync_round_trip_after_move_preserves_other_paths_and_reports_conflicts() {
    let t = tempfile::tempdir().unwrap();
    let bare = t.path().join("remote.git");
    git(t.path(), &["init", "--bare", "-q", bare.to_str().unwrap()]);
    let data = t.path().join("data");
    let root = t.path().join("p1");
    let root2 = t.path().join("p2");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&root2).unwrap();
    let a = t.path().join("a");
    let mut r = req("init", &root, Some(&a));
    r.remote = Some(bare.to_str().unwrap().into());
    r.subdir = Some("projects/a".into());
    location::run(&data, &r).unwrap();
    note(&data, &root, t.path(), "first", "# First\n共同知识");
    location::run(&data, &req("sync", &root, None)).unwrap();
    // These are separate local copies of the same remote knowledge, representing devices.
    let data2 = t.path().join("data2");
    let root3 = t.path().join("p3");
    std::fs::create_dir(&root3).unwrap();
    let c = t.path().join("c");
    r.root = root3.clone();
    r.path = Some(c.clone());
    location::run(&data2, &r).unwrap();
    location::run(&data2, &req("sync", &root3, None)).unwrap();
    let moved = t.path().join("outside/moved");
    move_to(&data, &root, &moved);
    note(&data, &root, t.path(), "second", "# Second");
    location::run(&data, &req("sync", &root, None)).unwrap();
    location::run(&data2, &req("sync", &root3, None)).unwrap();
    assert!(c.join("learnings/second.md").is_file());
    std::fs::write(moved.join("learnings/first.md"), "changed A").unwrap();
    std::fs::write(c.join("learnings/first.md"), "changed B").unwrap();
    location::run(&data, &req("sync", &root, None)).unwrap();
    assert!(location::run(&data2, &req("sync", &root3, None)).is_err());
    assert_eq!(
        std::fs::read_to_string(c.join("learnings/first.md")).unwrap(),
        "changed B"
    );
}
#[test]
fn cli_init_save_recall_and_maintenance_follow_location() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let data = t.path().join("data");
    let first = t.path().join("first");
    let next = t.path().join("next");
    let run = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_ailoom"))
            .args(["--json", "--data-root", data.to_str().unwrap()])
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    run(&["init", "--knowledge-path", first.to_str().unwrap()]);
    let f = t.path().join("note.md");
    std::fs::write(&f, "---\ntitle: 迁移测试\n---\n\n迁移后继续召回。").unwrap();
    run(&[
        "knowledge",
        "--action",
        "save",
        "--file",
        f.to_str().unwrap(),
        "--name",
        "demo",
    ]);
    run(&["recall", "--query", "迁移"]);
    run(&[
        "knowledge",
        "--action",
        "archive",
        "--id",
        "learnings/demo.md",
    ]);
    move_to(&data, &root, &next);
    assert_eq!(
        location::recall(&data, &root, "迁移", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    run(&[
        "knowledge",
        "--action",
        "restore",
        "--id",
        "learnings/demo.md",
    ]);
    assert_eq!(
        location::recall(&data, &root, "迁移", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    run(&["contribute", "--file", f.to_str().unwrap()]);
    assert_eq!(
        std::fs::read_dir(next.join("learnings")).unwrap().count(),
        2
    );
}
#[test]
fn console_uses_same_binding_and_requires_approved_destination() {
    use ailoom::console::{route, ConsoleOptions, ConsoleServer, Request as HttpRequest};
    use serde_json::json;
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let data = t.path().join("data");
    let dest = t.path().join("target");
    std::fs::create_dir(&dest).unwrap();
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 27931,
        data_root: data.clone(),
        open_browser: false,
    })
    .unwrap();
    let call = |path: &str, body: Value, token: bool| {
        let mut headers = vec![("host".into(), format!("127.0.0.1:{}", server.port))];
        if token {
            headers.push(("x-ailoom-session".into(), server.token.clone()));
        }
        route(
            &HttpRequest {
                method: "POST".into(),
                path: path.into(),
                query: vec![],
                headers,
                body,
            },
            &server.state,
        )
    };
    let body = json!({"action":"init","root":root,"path":dest});
    assert_eq!(call("/api/knowledge", body.clone(), false).status, 401);
    assert_eq!(call("/api/knowledge", body.clone(), true).status, 403);
    assert_eq!(
        call("/api/fs/approve", json!({"path":root}), true).status,
        200
    );
    assert_eq!(call("/api/knowledge", body.clone(), true).status, 403);
    assert_eq!(
        call("/api/fs/approve", json!({"path":dest}), true).status,
        200
    );
    assert_eq!(call("/api/knowledge", body, true).status, 200);
    assert!(location::is_initialized(&data, &root).unwrap());
    let next = t.path().join("moved");
    assert_eq!(
        call("/api/fs/approve", json!({"path":t.path()}), true).status,
        200
    );
    let preview = call(
        "/api/knowledge",
        json!({"action":"move","root":root,"path":next}),
        true,
    );
    assert_eq!(preview.status, 200);
    let v: Value = serde_json::from_str(&preview.body).unwrap();
    assert_eq!(call("/api/knowledge",json!({"action":"move","root":root,"path":next,"execute":true,"expected":v["expected"]}),true).status,200);
    assert_eq!(
        location::run(&data, &req("status", &root, None)).unwrap()["location"]["path"],
        next.canonicalize().unwrap().to_str().unwrap()
    );
    server.shutdown();
    server.join();
}
#[test]
fn failed_sync_retains_local_notes_and_does_not_claim_synced() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let data = t.path().join("data");
    let dest = t.path().join("knowledge");
    let mut r = req("init", &root, Some(&dest));
    r.remote = Some(t.path().join("missing.git").to_string_lossy().into());
    location::run(&data, &r).unwrap();
    note(&data, &root, t.path(), "offline", "# Offline knowledge");
    assert!(location::run(&data, &req("sync", &root, None)).is_err());
    assert!(dest.join("learnings/offline.md").is_file());
    assert_eq!(
        location::run(&data, &req("status", &root, None)).unwrap()["pending"],
        true
    );
}
#[test]
fn git_subfolders_are_isolated_and_business_branch_is_untouched() {
    let t = tempfile::tempdir().unwrap();
    let business = t.path().join("business");
    std::fs::create_dir(&business).unwrap();
    git(&business, &["init", "-qb", "main"]);
    std::fs::write(business.join("code.txt"), "code").unwrap();
    git(&business, &["add", "code.txt"]);
    git(&business, &["commit", "-qm", "initial"]);
    std::fs::write(business.join("code.txt"), "dirty code").unwrap();
    let before = git(&business, &["status", "--porcelain"]);
    let head = git(&business, &["rev-parse", "HEAD"]);
    let data = t.path().join("data");
    for n in ["a", "b"] {
        let root = t.path().join(n);
        std::fs::create_dir(&root).unwrap();
        let dest = t.path().join(format!("knowledge-{n}"));
        let mut r = req("init", &root, Some(&dest));
        r.remote = Some(business.to_string_lossy().into());
        r.subdir = Some(format!("projects/{n}"));
        location::run(&data, &r).unwrap();
        note(&data, &root, t.path(), "note", &format!("# Project {n}"));
        location::run(&data, &req("sync", &root, None)).unwrap();
    }
    assert_eq!(git(&business, &["status", "--porcelain"]), before);
    assert_eq!(git(&business, &["rev-parse", "HEAD"]), head);
    let files = git(
        &business,
        &["ls-tree", "-r", "--name-only", "ailoom-knowledge"],
    );
    assert!(files.contains("projects/a/learnings/note.md"));
    assert!(files.contains("projects/b/learnings/note.md"));
    assert!(!files.contains("code.txt"));
}
#[test]
fn migration_and_sync_are_one_flow_with_local_success_on_network_failure() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let data = t.path().join("data");
    let old = t.path().join("old");
    init(&data, &root, &old);
    note(&data, &root, t.path(), "note", "# Migration");
    let mut r = req("move", &root, Some(&t.path().join("new")));
    r.remote = Some(t.path().join("missing.git").to_string_lossy().into());
    r.sync_after = true;
    let p = location::run(&data, &r).unwrap();
    r.expected = p["expected"].as_str().map(str::to_owned);
    r.execute = true;
    let v = location::run(&data, &r).unwrap();
    assert_eq!(v["moved"], true);
    assert_eq!(v["synced"], false);
    assert!(v["sync_error"].is_string());
    assert!(old.join("learnings/note.md").exists());
    assert_eq!(
        location::recall(&data, &root, "Migration", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn project_creation_inspection_shows_defaults_without_registering() {
    let t = tempfile::tempdir().unwrap();
    for is_git in [false, true] {
        let root = t.path().join(if is_git {
            "git-project"
        } else {
            "folder-project"
        });
        std::fs::create_dir(&root).unwrap();
        if is_git {
            git(&root, &["init", "-b", "main"]);
        }
        let data = t
            .path()
            .join(if is_git { "git-data" } else { "folder-data" });
        let preview = location::run(&data, &req("status", &root, None)).unwrap();
        assert_eq!(preview["initialized"], false);
        assert_eq!(
            preview["project_root"],
            root.canonicalize().unwrap().to_str().unwrap()
        );
        assert!(
            !data.join("repos").exists(),
            "closing creation must not register a project"
        );
        let expected = PathBuf::from(preview["default_path"].as_str().unwrap());
        assert!(!expected.exists());
        let created = location::run(&data, &req("init", &root, Some(&expected))).unwrap();
        assert_eq!(created["project_id"], preview["project_id"]);
        assert_eq!(
            created["location"]["path"],
            expected.canonicalize().unwrap().to_str().unwrap()
        );
        assert!(data
            .join("repos")
            .join(created["project_id"].as_str().unwrap())
            .join("registry.json")
            .is_file());
        let repeated = location::run(&data, &req("init", &root, Some(&expected))).unwrap();
        assert_eq!(repeated["project_id"], created["project_id"]);
    }
}

#[test]
fn invalid_knowledge_choice_does_not_register_new_project() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let dest = t.path().join("occupied");
    let data = t.path().join("data");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&dest).unwrap();
    std::fs::write(dest.join("docs"), "user content").unwrap();
    assert!(location::run(&data, &req("init", &root, Some(&dest))).is_err());
    assert!(!data.join("repos").exists());
    assert!(!data.join("knowledge/bindings.json").exists());
    assert_eq!(
        std::fs::read_to_string(dest.join("docs")).unwrap(),
        "user content"
    );
}

#[test]
fn directory_detection_and_init_preserve_existing_git() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let repo = t.path().join("knowledge");
    let data = t.path().join("data");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "existing").unwrap();
    let before = git(&repo, &["status", "--porcelain"]);
    let child = repo.join("projects/app-a");
    let inspected = location::run(&data, &req("inspect", &root, Some(&child))).unwrap();
    assert_eq!(inspected["git"]["branch"], "main");
    assert_eq!(
        inspected["git"]["root"],
        repo.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(!child.exists());
    assert_eq!(before, git(&repo, &["status", "--porcelain"]));
    let created = location::run(&data, &req("init", &root, Some(&repo))).unwrap();
    assert!(created["location"]["remote"].is_null());
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "existing"
    );
    assert!(git(&repo, &["diff", "--cached", "--name-only"]).is_empty());
    assert!(!repo.join(".git/refs/heads/main").exists());
    assert!(location::run(&data, &req("inspect", &root, Some(&root))).unwrap()["git"].is_null());
}

/// AIL-149：迁移记录可追溯——带 schema_version，完成后状态为 switched，并记录新旧位置。
#[test]
fn move_writes_versioned_switched_record() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    let data = t.path().join("data");
    init(&data, &root, &root.join("knowledge"));
    let result = move_to(&data, &root, &t.path().join("moved"));
    assert!(result.get("record_warning").is_none(), "{result}");
    let record: Value =
        serde_json::from_slice(&std::fs::read(result["record"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(record["schema_version"], 1);
    assert_eq!(record["state"], "switched");
    assert!(
        record["old"]["path"].is_string() && record["next"]["path"].is_string(),
        "{record}"
    );
}
