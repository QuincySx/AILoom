mod common;
use ailoom::{
    collections, personal_library, repository_identity, skill_source,
    source_rebinding::{self, Change, Target},
};
use serde_json::Value;
use std::path::{Path, PathBuf};

struct Fixture {
    _tmp: tempfile::TempDir,
    data: PathBuf,
    repo: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        common::isolate_in_process_roots();
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("upstream");
        let data = tmp.path().join("data");
        write(&repo.join("skills/alpha"), "alpha", "one");
        write(&repo.join("skills/beta"), "beta", "one");
        ailoom::gitx::git_init(&repo, false).unwrap();
        ailoom::gitx::git_commit_all(&repo, "one", &["skills"]).unwrap();
        Self {
            _tmp: tmp,
            data,
            repo,
        }
    }
    fn change(&self, url: &str, mode: &str) -> Change {
        Change {
            url: url.into(),
            repository_url: None,
            ref_: Some("main".into()),
            repo_path: Some("skills/alpha".into()),
            mode: Some(mode.into()),
            host_mapping: None,
        }
    }
    fn skill(&self) -> Target {
        Target {
            kind: "skill".into(),
            id: "personal/skill/personal/alpha".into(),
        }
    }
    fn installed(&self) -> PathBuf {
        personal_library::library_root(&self.data).join("resources/skills/alpha")
    }
    fn local(&self) {
        personal_library::import_execute(&self.data, &self.repo.join("skills/alpha"), None)
            .unwrap();
    }
    fn remote(&self) {
        let p = personal_library::git_catalog::preview(
            &self.data,
            self.repo.to_str().unwrap(),
            Some("main"),
        )
        .unwrap();
        personal_library::git_catalog::install(&self.data, token(&p), &["skills/alpha".into()])
            .unwrap();
    }
    fn clone_repo(&self) -> PathBuf {
        let repo = self._tmp.path().join("fork");
        ailoom::gitx::git(
            self._tmp.path(),
            &["clone", self.repo.to_str().unwrap(), repo.to_str().unwrap()],
        )
        .unwrap();
        repo
    }
    fn collection(&self) -> Target {
        let p = collections::preview(
            &self.data,
            "fixture",
            self.repo.to_str().unwrap(),
            Some("main"),
            None,
        )
        .unwrap();
        let v = collections::apply_preview(&self.data, token(&p)).unwrap();
        Target {
            kind: "collection".into(),
            id: v["source"]["id"].as_str().unwrap().into(),
        }
    }
}
fn token(v: &Value) -> &str {
    v["preview_id"].as_str().unwrap()
}
fn write(dir: &Path, name: &str, version: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: fixture\n---\n{version}\n"),
    )
    .unwrap();
}
fn metadata(f: &Fixture) -> skill_source::SkillSourceMeta {
    skill_source::read_meta(&f.installed()).unwrap()
}

#[test]
fn local_association_selects_remote_and_preserves_uncommitted_content_and_references() {
    let f = Fixture::new();
    f.local();
    let old = metadata(&f);
    let text = std::fs::read(f.installed().join("SKILL.md")).unwrap();
    let mut profile = ailoom::profile::PersonalProfile::load_or_default(&f.data).unwrap();
    profile.global.skills.insert(f.skill().id.clone());
    profile.save(&f.data).unwrap();
    let profile_bytes = std::fs::read(f.data.join("profile/profile.toml")).unwrap();
    ailoom::gitx::git(
        &f.repo,
        &["remote", "add", "origin", f.repo.to_str().unwrap()],
    )
    .unwrap();
    ailoom::gitx::git(
        &f.repo,
        &["remote", "add", "upstream", "https://github.com/other/fork"],
    )
    .unwrap();
    write(
        &f.repo.join("skills/alpha"),
        "alpha",
        "uncommitted development",
    );
    let discovered = source_rebinding::discover(&f.data, &f.skill()).unwrap();
    assert_eq!(discovered["remotes"].as_array().unwrap().len(), 2);
    assert_eq!(discovered["repo_path"], "skills/alpha");
    let p = source_rebinding::preview(
        &f.data,
        &f.skill(),
        &f.change(f.repo.to_str().unwrap(), "local"),
    )
    .unwrap();
    assert_eq!(p["relationship"], "associate");
    assert_eq!(p["references"].as_array().unwrap().len(), 1);
    assert_eq!(metadata(&f), old, "preview is cancelable");
    source_rebinding::apply(&f.data, token(&p), None).unwrap();
    let next = metadata(&f);
    assert_eq!(next.source_kind, "local");
    assert_eq!(next.local_dir(), old.local_dir());
    assert_eq!(next.imported_digest, old.imported_digest);
    assert_eq!(next.fetched_at, old.fetched_at);
    assert_eq!(std::fs::read(f.installed().join("SKILL.md")).unwrap(), text);
    assert_eq!(
        std::fs::read(f.data.join("profile/profile.toml")).unwrap(),
        profile_bytes
    );
    assert_eq!(
        personal_library::check_update(&f.data, "alpha")
            .unwrap()
            .state,
        "upstream-new"
    );
}

#[test]
fn changed_remote_rejects_confirmation_and_keeps_source() {
    let f = Fixture::new();
    f.local();
    let old = metadata(&f);
    ailoom::gitx::git(
        &f.repo,
        &["remote", "add", "origin", f.repo.to_str().unwrap()],
    )
    .unwrap();
    let p = source_rebinding::preview(
        &f.data,
        &f.skill(),
        &f.change(f.repo.to_str().unwrap(), "local"),
    )
    .unwrap();
    ailoom::gitx::git(
        &f.repo,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/changed/repo",
        ],
    )
    .unwrap();
    assert!(source_rebinding::apply(&f.data, token(&p), None).is_err());
    assert_eq!(metadata(&f), old);
}

#[test]
fn explicit_alias_maps_repository_but_retains_machine_transport() {
    let f = Fixture::new();
    f.local();
    let url = "git@quincy-github:Owner/skills.git";
    ailoom::gitx::git(&f.repo, &["remote", "add", "origin", url]).unwrap();
    let mut change = f.change(url, "local");
    change.host_mapping = Some(repository_identity::HostMapping {
        alias: "quincy-github".into(),
        hostname: "github.com".into(),
        user: Some("git".into()),
        port: Some(22),
    });
    let p = source_rebinding::preview(&f.data, &f.skill(), &change).unwrap();
    source_rebinding::apply(&f.data, token(&p), None).unwrap();
    let next = metadata(&f);
    assert_eq!(next.connection.unwrap().url, url);
    assert_eq!(
        next.repo_url.as_deref(),
        Some("https://github.com/Owner/skills.git")
    );
    assert_eq!(
        repository_identity::key_for(&f.data, url).unwrap(),
        repository_identity::key("https://github.com/Owner/skills")
    );
    assert_ne!(
        repository_identity::key_for(&f.data, "git@github-lookalike:Owner/skills").unwrap(),
        repository_identity::key("https://github.com/Owner/skills")
    );
    assert_ne!(
        repository_identity::key_for(&f.data, "other@quincy-github:Owner/skills").unwrap(),
        repository_identity::key("https://github.com/Owner/skills")
    );
}

#[test]
fn confirmed_replacement_preserves_collection_snapshot_until_explicit_update() {
    let f = Fixture::new();
    let target = f.collection();
    let fork = f.clone_repo();
    let old = collections::load(&f.data)
        .unwrap()
        .sources
        .remove(&target.id)
        .unwrap();
    let old_ids: Vec<_> = collections::catalog(&f.data, &old)
        .unwrap()
        .entries
        .iter()
        .map(|e| e.id.to_string())
        .collect();
    let old_update =
        collections::preview(&f.data, &old.name, &old.url, Some("main"), Some(&old.id)).unwrap();
    write(&fork.join("skills/alpha"), "alpha", "two");
    ailoom::gitx::git_commit_all(&fork, "two", &["skills"]).unwrap();
    let p = source_rebinding::preview(
        &f.data,
        &target,
        &f.change(fork.to_str().unwrap(), "remote"),
    )
    .unwrap();
    assert_eq!(p["relationship"], "unverified");
    assert!(source_rebinding::apply(&f.data, token(&p), None).is_err());
    source_rebinding::apply(&f.data, token(&p), Some("replace")).unwrap();
    let source = collections::load(&f.data)
        .unwrap()
        .sources
        .remove(&target.id)
        .unwrap();
    assert_eq!(source.id, old.id);
    assert_eq!(source.url, old.url);
    assert_eq!(
        serde_json::to_value(source.lock.clone()).unwrap(),
        serde_json::to_value(old.lock.clone()).unwrap()
    );
    assert_eq!(source.update_url(), fork.to_str().unwrap());
    let cat = collections::catalog(&f.data, &source).unwrap();
    assert_eq!(
        cat.entries
            .iter()
            .map(|e| e.id.to_string())
            .collect::<Vec<_>>(),
        old_ids
    );
    assert!(
        std::fs::read_to_string(cat.snapshot.root.join("skills/alpha/SKILL.md"))
            .unwrap()
            .contains("one")
    );
    assert!(collections::apply_preview(&f.data, token(&old_update)).is_err());
    let checked = collections::check_updates(&f.data, Some(&target.id)).unwrap();
    assert_eq!(checked["items"][0]["state"], "available");
    collections::apply_preview(&f.data, token(&checked["items"][0]["preview"])).unwrap();
    let source = collections::load(&f.data)
        .unwrap()
        .sources
        .remove(&target.id)
        .unwrap();
    assert_eq!(source.url, fork.to_str().unwrap());
    assert_eq!(
        source.lock.identity,
        ailoom::gitx::normalize_remote_url(&source.url)
    );
    assert!(std::fs::read_to_string(
        collections::catalog(&f.data, &source)
            .unwrap()
            .snapshot
            .root
            .join("skills/alpha/SKILL.md")
    )
    .unwrap()
    .contains("two"));
}

#[test]
fn personal_rebinding_invalidates_update_install_and_other_binding_previews() {
    let f = Fixture::new();
    f.remote();
    let fork = f.clone_repo();
    write(&f.repo.join("skills/alpha"), "alpha", "two");
    ailoom::gitx::git_commit_all(&f.repo, "two", &["skills"]).unwrap();
    let update = personal_library::check_update(&f.data, "alpha").unwrap();
    assert_eq!(update.state, "upstream-new");
    let scan =
        personal_library::git_catalog::preview(&f.data, f.repo.to_str().unwrap(), Some("main"))
            .unwrap();
    let old = metadata(&f);
    let content = std::fs::read(f.installed().join("SKILL.md")).unwrap();
    let p = source_rebinding::preview(
        &f.data,
        &f.skill(),
        &f.change(fork.to_str().unwrap(), "remote"),
    )
    .unwrap();
    source_rebinding::apply(&f.data, token(&p), Some("same")).unwrap();
    assert_eq!(metadata(&f).resolved_commit, old.resolved_commit);
    assert_eq!(metadata(&f).imported_digest, old.imported_digest);
    assert_eq!(
        std::fs::read(f.installed().join("SKILL.md")).unwrap(),
        content
    );
    assert_eq!(
        personal_library::cached_update(&f.data, "alpha")
            .unwrap()
            .unwrap()
            .state,
        "stale"
    );
    assert!(personal_library::update_execute_checked(
        &f.data,
        "alpha",
        update.preview_id.as_deref().unwrap()
    )
    .is_err());
    assert!(
        personal_library::git_catalog::install(&f.data, token(&scan), &["skills/beta".into()])
            .is_err()
    );
    assert!(source_rebinding::apply(&f.data, token(&p), Some("same")).is_err());
}

#[test]
fn failed_validation_and_changed_library_content_do_not_rebind() {
    let f = Fixture::new();
    f.remote();
    let old = metadata(&f);
    let mut invalid = f.change(f.repo.to_str().unwrap(), "remote");
    invalid.repo_path = Some("../outside".into());
    assert!(source_rebinding::preview(&f.data, &f.skill(), &invalid).is_err());
    invalid.repo_path = Some("missing".into());
    assert!(source_rebinding::preview(&f.data, &f.skill(), &invalid).is_err());
    invalid.url = f
        ._tmp
        .path()
        .join("missing-repository")
        .to_string_lossy()
        .into();
    assert!(source_rebinding::preview(&f.data, &f.skill(), &invalid).is_err());
    assert_eq!(metadata(&f), old);
    let p = source_rebinding::preview(
        &f.data,
        &f.skill(),
        &f.change(f.repo.to_str().unwrap(), "remote"),
    )
    .unwrap();
    write(&f.installed(), "alpha", "local edit while previewing");
    assert!(source_rebinding::apply(&f.data, token(&p), None).is_err());
    assert_eq!(metadata(&f), old);
}

#[test]
fn changing_registry_after_preview_is_rejected() {
    let f = Fixture::new();
    let target = f.collection();
    let old = collections::load(&f.data)
        .unwrap()
        .sources
        .remove(&target.id)
        .unwrap();
    let p = source_rebinding::preview(
        &f.data,
        &target,
        &f.change(f.repo.to_str().unwrap(), "remote"),
    )
    .unwrap();
    let fork = f.clone_repo();
    let other =
        collections::preview(&f.data, "another", fork.to_str().unwrap(), None, None).unwrap();
    collections::apply_preview(&f.data, token(&other)).unwrap();
    assert!(source_rebinding::apply(&f.data, token(&p), None).is_err());
    let source = collections::load(&f.data)
        .unwrap()
        .sources
        .remove(&target.id)
        .unwrap();
    assert_eq!(source.connection, old.connection);
    assert_eq!(
        serde_json::to_value(source.lock.clone()).unwrap(),
        serde_json::to_value(old.lock.clone()).unwrap()
    );
}

#[test]
fn console_rejects_whitespace_local_urls_and_implicit_local_mode_without_approval() {
    use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let f = Fixture::new();
    f.local();
    ailoom::gitx::git(
        &f.repo,
        &["remote", "add", "origin", f.repo.to_str().unwrap()],
    )
    .unwrap();
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 17994,
        data_root: f.data.clone(),
        open_browser: false,
    })
    .unwrap();
    let request = |route: &str, body: Value| {
        let body = body.to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        write!(stream,"POST {route} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{SESSION_HEADER}: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",server.port,server.token,body.len()).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        let (header, body) = text.split_once("\r\n\r\n").unwrap();
        (
            header
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u16>()
                .unwrap(),
            serde_json::from_str::<Value>(body).unwrap(),
        )
    };
    for url in [
        format!(" {}", f.repo.display()),
        format!(" file://{}", f.repo.display()),
    ] {
        assert_eq!(request("/api/source-binding",serde_json::json!({"action":"preview","target":f.skill(),"change":{"url":url,"mode":"remote","repo_path":"skills/alpha"}})).0,403);
    }
    assert_eq!(request("/api/source-binding",serde_json::json!({"action":"preview","target":f.skill(),"change":{"url":"https://github.com/example/skills","repo_path":"skills/alpha"}})).0,403);
    assert_eq!(
        request("/api/fs/approve", serde_json::json!({"path":f.repo})).0,
        200
    );
    let (status, p) = request(
        "/api/source-binding",
        serde_json::json!({"action":"preview","target":f.skill(),"change":f.change(f.repo.to_str().unwrap(),"local")}),
    );
    assert_eq!(status, 200, "{p}");
    assert_eq!(
        request(
            "/api/source-binding",
            serde_json::json!({"action":"apply","preview_id":p["preview_id"]})
        )
        .0,
        200
    );
    server.shutdown();
    server.join();
}

#[cfg(unix)]
#[test]
fn symlink_skill_file_cannot_escape_validation_snapshot() {
    let f = Fixture::new();
    f.remote();
    let old = metadata(&f);
    let fork = f.clone_repo();
    let sentinel = f._tmp.path().join("sentinel.md");
    std::fs::write(&sentinel, "synthetic fixture, must not become a skill").unwrap();
    std::fs::remove_file(fork.join("skills/alpha/SKILL.md")).unwrap();
    std::os::unix::fs::symlink(&sentinel, fork.join("skills/alpha/SKILL.md")).unwrap();
    ailoom::gitx::git_commit_all(&fork, "symlink", &["skills"]).unwrap();
    assert!(source_rebinding::preview(
        &f.data,
        &f.skill(),
        &f.change(fork.to_str().unwrap(), "remote")
    )
    .is_err());
    assert_eq!(metadata(&f), old);
}

#[test]
fn identical_commit_in_another_repository_is_not_identity_evidence() {
    let f = Fixture::new();
    let target = f.collection();
    let fork = f.clone_repo();
    assert_eq!(
        ailoom::gitx::git(&f.repo, &["rev-parse", "HEAD"]).unwrap(),
        ailoom::gitx::git(&fork, &["rev-parse", "HEAD"]).unwrap()
    );
    let p = source_rebinding::preview(
        &f.data,
        &target,
        &f.change(fork.to_str().unwrap(), "remote"),
    )
    .unwrap();
    assert_eq!(p["relationship"], "unverified");
    assert!(source_rebinding::apply(&f.data, token(&p), None).is_err());
}

#[test]
fn local_path_normalization_is_identical_at_preview_and_save() {
    let f = Fixture::new();
    f.local();
    ailoom::gitx::git(
        &f.repo,
        &["remote", "add", "origin", f.repo.to_str().unwrap()],
    )
    .unwrap();
    let mut change = f.change(f.repo.to_str().unwrap(), "local");
    change.repo_path = Some("./skills/./alpha/".into());
    let p = source_rebinding::preview(&f.data, &f.skill(), &change).unwrap();
    assert_eq!(p["after"]["repo_path"], "skills/alpha");
    source_rebinding::apply(&f.data, token(&p), None).unwrap();
    assert_eq!(metadata(&f).repo_path.as_deref(), Some("skills/alpha"));
}

#[test]
fn portable_checkpoint_excludes_machine_paths_and_identity_aliases() {
    use ailoom::knowledge::location::{self, Request};
    let f = Fixture::new();
    f.local();
    let url = "git@quincy-github:Owner/skills.git";
    ailoom::gitx::git(&f.repo, &["remote", "add", "origin", url]).unwrap();
    let mut change = f.change(url, "local");
    change.host_mapping = Some(repository_identity::HostMapping {
        alias: "quincy-github".into(),
        hostname: "github.com".into(),
        user: Some("git".into()),
        port: Some(22),
    });
    let p = source_rebinding::preview(&f.data, &f.skill(), &change).unwrap();
    source_rebinding::apply(&f.data, token(&p), None).unwrap();
    let target = f.collection();
    let mut change = f.change(f.repo.to_str().unwrap(), "remote");
    change.repository_url = Some("https://github.com/Owner/skills.git".into());
    change.ref_ = None;
    let p = source_rebinding::preview(&f.data, &target, &change).unwrap();
    source_rebinding::apply(&f.data, token(&p), Some("same")).unwrap();
    let root = f._tmp.path().join("business-project");
    let knowledge = root.join("knowledge");
    std::fs::create_dir(&root).unwrap();
    let init = location::run(
        &f.data,
        &Request {
            action: "init".into(),
            root: root.clone(),
            path: Some(knowledge.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let repo = init["project_id"].as_str().unwrap();
    let source = collections::load(&f.data)
        .unwrap()
        .sources
        .remove(&target.id)
        .unwrap();
    let selected = collections::catalog(&f.data, &source).unwrap().entries[0]
        .id
        .to_string();
    let mut profile = ailoom::profile::PersonalProfile::load_or_default(&f.data).unwrap();
    profile.repos.insert(
        repo.into(),
        ailoom::profile::RepoProfile {
            default: Some(ailoom::profile::ScopeSelection {
                resources: [
                    (f.skill().id, ailoom::profile::TriState::Enable),
                    (selected, ailoom::profile::TriState::Enable),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    profile.save(&f.data).unwrap();
    location::run(
        &f.data,
        &Request {
            action: "checkpoint".into(),
            root,
            ..Default::default()
        },
    )
    .unwrap();
    let mut metadata_seen = false;
    let mut origin_seen = false;
    for file in walkdir::WalkDir::new(&knowledge) {
        let file = file.unwrap();
        if !file.file_type().is_file() || file.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert!(
            !text.contains("quincy-github"),
            "alias leaked into {}",
            file.path().display()
        );
        assert!(
            !text.contains(f.repo.to_str().unwrap()),
            "machine path leaked into {}",
            file.path().display()
        );
        if file.file_name() == skill_source::IMPORT_META_FILE {
            metadata_seen = true;
            let v: Value = serde_json::from_str(&text).unwrap();
            assert!(v["source_dir"].is_null());
            assert_eq!(
                v["source"]["repo_url"],
                "https://github.com/Owner/skills.git"
            );
            assert!(v["source"]["connection"].is_null());
        }
        if file.file_name() == "ailoom-origin.json" {
            origin_seen = true;
            let v: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                v["connection"]["url"],
                "https://github.com/Owner/skills.git"
            );
            assert!(v["connection"]["ref"].is_null());
            assert!(v["connection"]["host_mapping"].is_null());
        }
    }
    assert!(metadata_seen && origin_seen);
}
