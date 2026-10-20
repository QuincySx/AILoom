mod common;
use ailoom::personal_library::{self, git_catalog};
use serde_json::Value;
use std::path::{Path, PathBuf};

struct Fixture {
    tmp: tempfile::TempDir,
    repo: PathBuf,
    data: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        common::isolate_in_process_roots();
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("upstream");
        let data = tmp.path().join("data");
        for (path, name) in [
            ("skills/alpha", "alpha"),
            ("packages/tool/deep/skills/beta", "beta"),
        ] {
            write_skill(&repo.join(path), name, "version one");
        }
        ailoom::gitx::git_init(&repo, false).unwrap();
        ailoom::gitx::git_commit_all(&repo, "fixture", &["skills", "packages"]).unwrap();
        Self { tmp, repo, data }
    }
    fn preview(&self) -> Value {
        git_catalog::preview(&self.data, self.repo.to_str().unwrap(), Some("main")).unwrap()
    }
    fn token<'a>(&self, value: &'a Value) -> &'a str {
        value["preview_id"].as_str().unwrap()
    }
    fn installed(&self, name: &str) -> PathBuf {
        personal_library::library_root(&self.data)
            .join("resources/skills")
            .join(name)
    }
}
fn write_skill(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: fixture {name}\n---\n{body}\n"),
    )
    .unwrap();
}

#[test]
fn scan_is_recursive_and_cancel_does_not_install_or_register() {
    let f = Fixture::new();
    let preview = f.preview();
    assert_eq!(preview["skills"].as_array().unwrap().len(), 2);
    assert!(!personal_library::library_root(&f.data)
        .join("ailoom.toml")
        .exists());
    assert!(!f.data.join("profile/profile.toml").exists());
    assert!(!f.data.join("collections/registry.json").exists());
    let result =
        git_catalog::install(&f.data, f.token(&preview), &["skills/alpha".into()]).unwrap();
    assert_eq!(result["imported"].as_array().unwrap().len(), 1);
    assert!(f.installed("alpha").join("SKILL.md").exists());
    assert!(!f.installed("beta").exists());
    let next = f.preview();
    assert_eq!(next["skills"][0]["installed"], true);
    assert_eq!(next["skills"][1]["installed"], false);
}

#[test]
fn installation_uses_previewed_commit_and_retains_update_branch() {
    let f = Fixture::new();
    let preview = f.preview();
    write_skill(&f.repo.join("skills/alpha"), "alpha", "version two");
    ailoom::gitx::git_commit_all(&f.repo, "upstream update", &["skills"]).unwrap();
    git_catalog::install(&f.data, f.token(&preview), &["skills/alpha".into()]).unwrap();
    let text = std::fs::read_to_string(f.installed("alpha").join("SKILL.md")).unwrap();
    assert!(text.contains("version one"));
    assert!(!text.contains("version two"));
    let meta = ailoom::skill_source::read_meta(&f.installed("alpha")).unwrap();
    assert_eq!(meta.ref_.as_deref(), Some("main"));
    assert_eq!(
        serde_json::to_value(meta.resolved_commit).unwrap(),
        preview["resolved_commit"]
    );
    let update = personal_library::check_update(&f.data, "alpha").unwrap();
    assert_eq!(update.state, "upstream-new");
}

#[test]
fn unpreviewed_selections_are_rejected_and_late_conflicts_never_overwrite() {
    let f = Fixture::new();
    let preview = f.preview();
    assert!(git_catalog::install(
        &f.data,
        f.token(&preview),
        &["skills/alpha".into(), "../outside".into()]
    )
    .is_err());
    assert!(!f.installed("alpha").exists());
    assert!(git_catalog::install(
        &f.data,
        f.token(&preview),
        &["skills/alpha".into(), "skills/alpha".into()]
    )
    .is_err());
    let local = f.tmp.path().join("local-beta");
    write_skill(&local, "beta", "local copy");
    personal_library::import_execute(&f.data, &local, None).unwrap();
    let result = git_catalog::install(
        &f.data,
        f.token(&preview),
        &[
            "skills/alpha".into(),
            "packages/tool/deep/skills/beta".into(),
        ],
    )
    .unwrap();
    assert_eq!(result["imported"].as_array().unwrap().len(), 1);
    assert_eq!(result["failed"].as_array().unwrap().len(), 1);
    assert!(
        std::fs::read_to_string(f.installed("beta").join("SKILL.md"))
            .unwrap()
            .contains("local copy")
    );
    let rescan = f.preview();
    assert_eq!(rescan["skills"][0]["installed"], true);
    assert!(rescan["skills"][1]["conflict"].is_string());
    assert!(git_catalog::install(&f.data, f.token(&rescan), &["skills/alpha".into()]).is_err());
}

#[test]
fn collection_resources_count_as_installed_but_new_upstream_skills_do_not() {
    let f = Fixture::new();
    let collection = ailoom::collections::preview(
        &f.data,
        "fixture",
        f.repo.to_str().unwrap(),
        Some("main"),
        None,
    )
    .unwrap();
    ailoom::collections::apply_preview(&f.data, collection["preview_id"].as_str().unwrap())
        .unwrap();
    let before = f.preview();
    assert!(before["skills"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["installed"] == true));
    write_skill(&f.repo.join("skills/gamma"), "gamma", "new skill");
    ailoom::gitx::git_commit_all(&f.repo, "add gamma", &["skills"]).unwrap();
    let after = f.preview();
    assert_eq!(after["skills"].as_array().unwrap().len(), 3);
    assert_eq!(after["skills"][2]["name"], "gamma");
    assert_eq!(after["skills"][2]["installed"], false);
    git_catalog::install(&f.data, f.token(&after), &["skills/gamma".into()]).unwrap();
    assert!(f.installed("gamma").join("SKILL.md").exists());
    assert_eq!(ailoom::collections::load(&f.data).unwrap().sources.len(), 1);
}

#[test]
fn console_requires_local_approval_and_installs_only_selected_skills() {
    use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let f = Fixture::new();
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 17992,
        data_root: f.data.clone(),
        open_browser: false,
    })
    .unwrap();
    let request = |method: &str, route: &str, body: Value| {
        let body = body.to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        write!(stream, "{method} {route} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{SESSION_HEADER}: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", server.port, server.token, body.len()).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        let (headers, body) = text.split_once("\r\n\r\n").unwrap();
        let status: u16 = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_str::<Value>(body).unwrap())
    };
    let input = serde_json::json!({"url": f.repo, "ref": "main"});
    assert_eq!(
        request("POST", "/api/library/git-catalog", input.clone()).0,
        403
    );
    assert_eq!(
        request("GET", "/api/library/git-catalog", Value::Null).0,
        405
    );
    assert_eq!(
        request(
            "POST",
            "/api/fs/approve",
            serde_json::json!({"path": f.repo})
        )
        .0,
        200
    );
    let (status, preview) = request("POST", "/api/library/git-catalog", input);
    assert_eq!(status, 200, "{preview}");
    assert!(!f.installed("alpha").exists());
    let (status, result) = request(
        "POST",
        "/api/library/git-catalog",
        serde_json::json!({
            "execute":true,"preview_id":preview["preview_id"],"selected":["packages/tool/deep/skills/beta"]
        }),
    );
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["imported"].as_array().unwrap().len(), 1);
    assert_eq!(result["failed"].as_array().unwrap().len(), 0);
    assert!(f.installed("beta").join("SKILL.md").exists());
    assert!(!f.installed("alpha").exists());
    server.shutdown();
    server.join();
}

#[test]
fn metadata_write_failure_does_not_publish_and_does_not_block_retry() {
    let f = Fixture::new();
    let reserved = f.repo.join("skills/alpha/.ailoom-import.json");
    std::fs::create_dir_all(&reserved).unwrap();
    std::fs::write(
        reserved.join("child.txt"),
        "reserved metadata path is a directory",
    )
    .unwrap();
    ailoom::gitx::git_commit_all(&f.repo, "invalid metadata fixture", &["skills"]).unwrap();
    let preview = f.preview();
    let first = git_catalog::install(&f.data, f.token(&preview), &["skills/alpha".into()]).unwrap();
    assert_eq!(first["imported"].as_array().unwrap().len(), 0);
    assert_eq!(first["failed"].as_array().unwrap().len(), 1);
    assert!(!f.installed("alpha").exists());
    let second =
        git_catalog::install(&f.data, f.token(&preview), &["skills/alpha".into()]).unwrap();
    assert_eq!(second["failed"].as_array().unwrap().len(), 1);
    assert!(!second["failed"][0]["error"]
        .as_str()
        .unwrap()
        .contains("已有同名"));
    assert!(!f.installed("alpha").exists());
}

/// 只写入 Git 索引、不在磁盘上落地的无关文件：快速构造“数万文件”的大仓库。
fn commit_irrelevant_files(f: &Fixture, count: usize) {
    use std::io::Write;
    let filler = f.tmp.path().join("filler.txt");
    std::fs::write(&filler, "irrelevant\n").unwrap();
    let blob =
        ailoom::gitx::git(&f.repo, &["hash-object", "-w", filler.to_str().unwrap()]).unwrap();
    let mut child = std::process::Command::new("git")
        .args(["update-index", "--index-info"])
        .current_dir(&f.repo)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for i in 0..count {
        writeln!(
            stdin,
            "100644 {blob}\tvendor/pkg{:04}/file{:03}.txt",
            i / 100,
            i % 100
        )
        .unwrap();
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
    ailoom::gitx::git_commit_all(&f.repo, "vendor", &[]).unwrap();
}

fn snapshot_files(cache: &Path) -> Vec<String> {
    walkdir::WalkDir::new(cache)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e
                .path()
                .strip_prefix(cache)
                .unwrap()
                .to_string_lossy()
                .to_string();
            let (_, inner) = rel.split_once("/snapshots/")?;
            Some(inner.split_once('/')?.1.to_string())
        })
        .collect()
}

#[test]
fn large_repository_materializes_and_hashes_only_skill_directories() {
    let f = Fixture::new();
    commit_irrelevant_files(&f, 20_000);
    let preview = f.preview();
    assert_eq!(preview["skills"].as_array().unwrap().len(), 2);
    let mut files = snapshot_files(&f.data.join("cache/skill-import"));
    files.sort();
    assert_eq!(
        files,
        [
            ".ailoom-snapshot.json",
            "packages/tool/deep/skills/beta/SKILL.md",
            "skills/alpha/SKILL.md"
        ]
    );
    let result =
        git_catalog::install(&f.data, f.token(&preview), &["skills/alpha".into()]).unwrap();
    assert_eq!(result["imported"].as_array().unwrap().len(), 1, "{result}");
    let imported = personal_library::git_import_execute(
        &f.data,
        f.repo.to_str().unwrap(),
        Some("packages/tool/deep/skills/beta"),
        None,
        None,
    )
    .unwrap();
    assert_eq!(imported.files_copied, 1);

    let collection = ailoom::collections::preview(
        &f.data,
        "large",
        f.repo.to_str().unwrap(),
        Some("main"),
        None,
    )
    .unwrap();
    assert_eq!(collection["resources"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot_files(&f.data.join("collections/cache")).len(), 3);
    ailoom::collections::apply_preview(&f.data, collection["preview_id"].as_str().unwrap())
        .unwrap();
    let listed = ailoom::collections::list(&f.data).unwrap();
    assert_eq!(
        listed["sources"][0]["resources"].as_array().unwrap().len(),
        2
    );
}
