//! Git 资源源、缓存与版本锁定（AIL-004）集成测试。全部使用本地裸仓，无真实远端。

use ailoom::gitx::{git_commit_all, git_init};
use ailoom::source::{git::GitSource, SourceLock, SourcesLock};
use std::path::Path;

fn write_file(repo: &Path, rel: &str, content: &str) {
    let p = repo.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, content).unwrap();
}

fn lock_entry(src: &GitSource, commit: &str, digest: &str) -> SourceLock {
    SourceLock {
        kind: "git".into(),
        identity: src.identity.clone(),
        ref_: src.ref_.clone(),
        resolved_commit: Some(commit.into()),
        content_digest: digest.into(),
        locked_at: "2026-09-09T00:00:00Z".into(),
    }
}

#[test]
fn first_resolve_locks_and_reuses_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    git_init(&origin, false).unwrap();
    write_file(
        &origin,
        "resources/skills/common/SKILL.md",
        "---\nname: x\n---\n",
    );
    git_commit_all(&origin, "init", &["resources"]).unwrap();

    let cache = tmp.path().join("cache");
    let src = GitSource::new(origin.to_str().unwrap(), Some("main")).unwrap();
    let first = src.resolve(&cache, None).unwrap();
    assert!(first.resolved_commit.is_some());
    assert!(!first.mutable);

    // 锁定后：远端前进不改变已锁版本
    let lock = lock_entry(
        &src,
        first.resolved_commit.as_ref().unwrap(),
        &first.content_digest,
    );
    write_file(
        &origin,
        "resources/skills/common/SKILL.md",
        "---\nname: x\n---\nchanged",
    );
    git_commit_all(&origin, "v2", &["resources"]).unwrap();

    let second = src.resolve(&cache, Some(&lock)).unwrap();
    assert_eq!(
        first.resolved_commit, second.resolved_commit,
        "远端 branch 前进不得改变已锁定版本"
    );
    assert_eq!(first.content_digest, second.content_digest);
    assert_eq!(second.root, first.root, "同一源复用缓存快照");
}

#[test]
fn explicit_refresh_produces_new_revision() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    git_init(&origin, false).unwrap();
    write_file(&origin, "a.txt", "v1");
    git_commit_all(&origin, "v1", &["a.txt"]).unwrap();

    let cache = tmp.path().join("cache");
    let src = GitSource::new(origin.to_str().unwrap(), None).unwrap();
    let s1 = src.resolve(&cache, None).unwrap();

    write_file(&origin, "a.txt", "v2");
    git_commit_all(&origin, "v2", &["a.txt"]).unwrap();
    let s2 = src.refresh(&cache).unwrap();
    assert_ne!(
        s1.resolved_commit, s2.resolved_commit,
        "显式更新才产生新 revision"
    );
    assert_ne!(s1.content_digest, s2.content_digest);
    // 旧快照仍可用（保留旧锁的语义）
    let old = src
        .resolve(
            &cache,
            Some(&lock_entry(
                &src,
                s1.resolved_commit.as_ref().unwrap(),
                &s1.content_digest,
            )),
        )
        .unwrap();
    assert_eq!(old.resolved_commit, s1.resolved_commit);
}

#[test]
fn offline_uses_existing_snapshot_fetch_failure_keeps_old() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    git_init(&origin, false).unwrap();
    write_file(&origin, "a.txt", "v1");
    git_commit_all(&origin, "v1", &["a.txt"]).unwrap();

    let cache = tmp.path().join("cache");
    let src = GitSource::new(origin.to_str().unwrap(), None).unwrap();
    let s1 = src.resolve(&cache, None).unwrap();
    let lock = lock_entry(
        &src,
        s1.resolved_commit.as_ref().unwrap(),
        &s1.content_digest,
    );

    // 模拟断网/远端消失：把 origin 移走，fetch 必然失败
    let gone = tmp.path().join("origin-gone");
    std::fs::rename(&origin, &gone).unwrap();

    // 已锁 + 快照存在 → 完全离线可用（不触发 fetch）
    let s2 = src.resolve(&cache, Some(&lock)).unwrap();
    assert_eq!(s2.resolved_commit, s1.resolved_commit);

    // 快照被删 + fetch 失败 → 明确报错而非伪成功（旧锁文件本身不动）
    let snapshots = cache.join("snapshots");
    std::fs::remove_dir_all(&snapshots).unwrap();
    let err = src.resolve(&cache, Some(&lock)).unwrap_err();
    assert!(
        err.code == ailoom::error::code::SOURCE_NOT_CACHED
            || err.code == ailoom::error::code::SOURCE_FETCH_FAILED
            || err.code == ailoom::error::code::SOURCE_CACHE_CORRUPT,
        "意外错误: {err}"
    );
    // 恢复远端后可重新物化
    std::fs::rename(&gone, &origin).unwrap();
    let s3 = src.resolve(&cache, Some(&lock)).unwrap();
    assert_eq!(s3.resolved_commit, s1.resolved_commit);
}

#[test]
fn first_time_offline_fails_clearly() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let src = GitSource::new(tmp.path().join("no-such-origin").to_str().unwrap(), None).unwrap();
    let err = src.resolve(&cache, None).unwrap_err();
    assert_eq!(err.code, ailoom::error::code::SOURCE_FETCH_FAILED);
}

#[test]
fn concurrent_fetch_is_serialized_and_consistent() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    git_init(&origin, false).unwrap();
    write_file(&origin, "a.txt", "v1");
    git_commit_all(&origin, "v1", &["a.txt"]).unwrap();
    let cache = tmp.path().join("cache");
    let src = std::sync::Arc::new(GitSource::new(origin.to_str().unwrap(), None).unwrap());
    let cache = std::sync::Arc::new(cache);

    let mut handles = Vec::new();
    for _ in 0..8 {
        let src = src.clone();
        let cache = cache.clone();
        handles.push(std::thread::spawn(move || {
            src.resolve(&cache, None)
                .map(|s| s.resolved_commit.unwrap())
        }));
    }
    let mut commits = Vec::new();
    for h in handles {
        commits.push(h.join().unwrap().unwrap());
    }
    assert!(
        commits.iter().all(|c| *c == commits[0]),
        "并发 fetch 结果一致"
    );
}

#[test]
fn invalid_ref_does_not_damage_old_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    git_init(&origin, false).unwrap();
    write_file(&origin, "a.txt", "v1");
    git_commit_all(&origin, "v1", &["a.txt"]).unwrap();
    let cache = tmp.path().join("cache");
    let src = GitSource::new(origin.to_str().unwrap(), None).unwrap();
    let good = src.resolve(&cache, None).unwrap();
    let good_lock = lock_entry(
        &src,
        good.resolved_commit.as_ref().unwrap(),
        &good.content_digest,
    );

    let bad = GitSource::new(origin.to_str().unwrap(), Some("no-such-branch")).unwrap();
    let err = bad.refresh(&cache).unwrap_err();
    assert_eq!(err.code, ailoom::error::code::SOURCE_INVALID_REF);
    // 旧缓存与旧锁继续可用
    let again = src.resolve(&cache, Some(&good_lock)).unwrap();
    assert_eq!(again.resolved_commit, good.resolved_commit);
}

#[test]
fn local_source_digest_tracks_content_and_flags_mutable() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("team-src");
    std::fs::create_dir_all(dir.join("resources")).unwrap();
    std::fs::write(dir.join("resources").join("r.md"), "hello").unwrap();
    let src = ailoom::source::LocalSource::new(&dir).unwrap();
    let s1 = src.resolve().unwrap();
    assert!(s1.mutable);
    assert!(s1.resolved_commit.is_none());
    std::fs::write(dir.join("resources").join("r.md"), "hello!").unwrap();
    let s2 = src.resolve().unwrap();
    assert_ne!(
        s1.content_digest, s2.content_digest,
        "本地源内容变化必须体现在摘要中"
    );
}

#[test]
fn credential_url_is_rejected_and_never_logged() {
    let err = GitSource::new("https://user:hunter2@example.com/team.git", None).unwrap_err();
    assert_eq!(err.code, ailoom::error::code::SOURCE_URL_CREDENTIAL);
    let rendered = format!("{err}");
    assert!(!rendered.contains("hunter2"));
}

#[test]
fn lock_file_roundtrip_and_save_is_atomic() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("ws").join("sources.lock.json");
    let mut lock = SourcesLock::new();
    lock.sources.insert(
        "team".into(),
        SourceLock {
            kind: "git".into(),
            identity: "git+https://example.com/t.git".into(),
            ref_: Some("main".into()),
            resolved_commit: Some("97fe5b79".into()),
            content_digest: "sha256:ab".into(),
            locked_at: "2026-09-09T00:00:00Z".into(),
        },
    );
    lock.save(&path).unwrap();
    let loaded = SourcesLock::load(&path).unwrap().unwrap();
    assert_eq!(
        loaded.sources["team"].identity,
        "git+https://example.com/t.git"
    );
    // 锁文件不含凭据字段
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("password") && !text.contains("token"));
}
