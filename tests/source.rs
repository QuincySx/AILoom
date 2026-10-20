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
fn refresh_uses_selected_connection_and_tracks_changed_default_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("first");
    let second = tmp.path().join("second");
    let cache = tmp.path().join("cache");
    for (dir, text) in [(&first, "first"), (&second, "second")] {
        git_init(dir, false).unwrap();
        write_file(dir, "a.txt", text);
        git_commit_all(dir, text, &["a.txt"]).unwrap();
    }
    ailoom::gitx::git(&second, &["branch", "-m", "develop"]).unwrap();
    let old = GitSource::new(first.to_str().unwrap(), None).unwrap();
    let old_snapshot = old.refresh(&cache).unwrap();
    let locked = lock_entry(
        &old,
        old_snapshot.resolved_commit.as_deref().unwrap(),
        &old_snapshot.content_digest,
    );
    let next = GitSource::new(second.to_str().unwrap(), None).unwrap();
    let snapshot = next.refresh(&cache).unwrap();
    assert_eq!(
        std::fs::read_to_string(snapshot.root.join("a.txt")).unwrap(),
        "second"
    );
    assert_eq!(
        ailoom::gitx::git(&cache.join("repo"), &["remote", "get-url", "origin"]).unwrap(),
        second.to_str().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(
            old.resolve(&cache, Some(&locked))
                .unwrap()
                .root
                .join("a.txt")
        )
        .unwrap(),
        "first",
        "old immutable content remains available"
    );
    let missing = GitSource::new(second.to_str().unwrap(), Some("main")).unwrap();
    assert!(
        missing.refresh(&cache).is_err(),
        "new connection must not resolve a stale clone-local main branch"
    );
    let old_commit = old_snapshot.resolved_commit.as_deref().unwrap();
    assert!(
        GitSource::new(second.to_str().unwrap(), Some(old_commit))
            .unwrap()
            .refresh(&cache)
            .is_err(),
        "old cached objects are not evidence of the new upstream containing a commit"
    );
    ailoom::gitx::git(&second, &["tag", "v1"]).unwrap();
    assert_eq!(
        GitSource::new(second.to_str().unwrap(), Some("v1"))
            .unwrap()
            .refresh(&cache)
            .unwrap()
            .resolved_commit,
        snapshot.resolved_commit
    );
    assert_eq!(
        GitSource::new(
            second.to_str().unwrap(),
            snapshot.resolved_commit.as_deref()
        )
        .unwrap()
        .refresh(&cache)
        .unwrap()
        .resolved_commit,
        snapshot.resolved_commit
    );
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
    assert!(err.message.contains("资源源目录不存在"), "{err}");
    assert!(err.message.contains("no-such-origin"));
    assert!(!err.fix.as_deref().unwrap_or_default().contains("恢复网络"));
    // 已锁定但缓存丢失也要保留真实修复建议，不能换成网络故障。
    let lock = lock_entry(&src, &"a".repeat(40), "unused");
    let locked = src.resolve(&cache, Some(&lock)).unwrap_err();
    assert_eq!(locked.message, err.message);
    assert_eq!(locked.fix, err.fix);
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

#[test]
fn skill_scope_snapshot_migrates_legacy_full_snapshot_and_keeps_locked_digest() {
    use ailoom::source::git::Scope;
    let tmp = tempfile::tempdir().unwrap();
    let up = tmp.path().join("up");
    let cache = tmp.path().join("cache");
    git_init(&up, false).unwrap();
    write_file(&up, "skills/a/SKILL.md", "---\nname: a\n---\n");
    write_file(&up, "skills/a/nested/SKILL.md", "---\nname: n\n---\n");
    write_file(&up, "vendor/big.txt", "irrelevant");
    git_commit_all(&up, "init", &["skills", "vendor"]).unwrap();

    // 旧版本写下的整树快照与锁
    let full = GitSource::new(up.to_str().unwrap(), None).unwrap();
    let legacy = full.refresh(&cache).unwrap();
    assert!(legacy.root.join("vendor/big.txt").is_file());
    let lock = lock_entry(
        &full,
        legacy.resolved_commit.as_deref().unwrap(),
        &legacy.content_digest,
    );

    let scoped = full.clone().with_scope(Scope::Skills);
    let snap = scoped.resolve(&cache, Some(&lock)).unwrap();
    assert_eq!(snap.root, legacy.root);
    assert!(!snap.root.join("vendor").exists(), "无关文件不再物化");
    assert!(snap.root.join("skills/a/nested/SKILL.md").is_file());
    assert_eq!(
        snap.content_digest, lock.content_digest,
        "同一 commit 的锁摘要保持不变"
    );
    let again = scoped.resolve(&cache, Some(&lock)).unwrap();
    assert_eq!(again.content_digest, lock.content_digest);

    // 无锁的刷新给出范围快照自身的摘要，后续按该摘要锁定同样可用。
    let fresh = scoped.refresh(&cache).unwrap();
    assert_ne!(fresh.content_digest, lock.content_digest);
    let fresh_lock = lock_entry(
        &scoped,
        fresh.resolved_commit.as_deref().unwrap(),
        &fresh.content_digest,
    );
    assert_eq!(
        scoped
            .resolve(&cache, Some(&fresh_lock))
            .unwrap()
            .content_digest,
        fresh.content_digest
    );

    // 复用时仍校验已物化内容：篡改即报缓存损坏。
    write_file(&snap.root, "skills/a/SKILL.md", "tampered");
    let err = scoped.resolve(&cache, Some(&fresh_lock)).unwrap_err();
    assert_eq!(err.code, ailoom::error::code::SOURCE_CACHE_CORRUPT);

    // 整树范围的使用方不受影响：范围快照会被重新物化为整树。
    let other = tmp.path().join("cache-full");
    let full_snap = full.refresh(&other).unwrap();
    assert!(full_snap.root.join("vendor/big.txt").is_file());
}
