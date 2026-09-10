//! 同步执行、锁与失败恢复（AIL-008）集成测试。

use ailoom::adapters::common::{Artifact, ArtifactBody};
use ailoom::sync::apply::{apply, recover, ApplyReport};
use ailoom::sync::journal::JournalRun;
use ailoom::sync::manifest::ManagedManifest;
use ailoom::sync::plan::{build_plan, SyncPlan};

fn art(path: &str, content: &str) -> Artifact {
    Artifact {
        resource_id: format!("team/skill/common/{}", path.replace(".md", "")),
        target_tool: "claude".into(),
        kind: "skill".into(),
        path: path.into(),
        body: ArtifactBody::Full {
            content: content.into(),
        },
    }
}

struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            tmp: tempfile::tempdir().unwrap(),
        }
    }
    fn ws(&self) -> std::path::PathBuf {
        self.tmp.path().join("ws")
    }
    fn lock_dir(&self) -> std::path::PathBuf {
        self.tmp.path().join("lock")
    }
    fn journal(&self) -> std::path::PathBuf {
        self.tmp.path().join("journal")
    }
    fn manifest(&self) -> ManagedManifest {
        ManagedManifest::new("ws1")
    }
}

fn plan_for(env: &Env, artifacts: &[Artifact]) -> SyncPlan {
    let ws = env.ws();
    std::fs::create_dir_all(&ws).unwrap();
    build_plan(
        artifacts,
        &ManagedManifest::new("ws1"),
        &ws,
        "git+test",
        Some("r1".into()),
    )
    .unwrap()
}

#[test]
fn happy_path_creates_files_and_advances_manifest() {
    let env = Env::new();
    let artifacts = vec![art("a.md", "A"), art("sub/b.md", "B")];
    let plan = plan_for(&env, &artifacts);
    let mut managed = env.manifest();
    let report = apply(
        &plan,
        &artifacts,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.applied.len(), 2);
    assert_eq!(std::fs::read_to_string(env.ws().join("a.md")).unwrap(), "A");
    assert_eq!(
        std::fs::read_to_string(env.ws().join("sub/b.md")).unwrap(),
        "B"
    );
    assert_eq!(managed.deployed_revision.as_deref(), Some("r1"));
    assert!(managed.items.contains_key("a.md"));
    // journal 无残留
    assert!(JournalRun::find_pending(&env.journal()).unwrap().is_empty());
}

#[test]
fn repeated_apply_is_noop() {
    let env = Env::new();
    let artifacts = vec![art("a.md", "A")];
    let mut managed = env.manifest();
    let plan1 = plan_for(&env, &artifacts);
    let r1 = apply(
        &plan1,
        &artifacts,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r1.ok);
    // 第二次同步：全部 noop
    let plan2 = build_plan(
        &artifacts,
        &managed,
        &env.ws(),
        "git+test",
        Some("r1".into()),
    )
    .unwrap();
    let r2 = apply(
        &plan2,
        &artifacts,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r2.ok);
    assert_eq!(r2.noop, 1, "重复同步无额外变化");
    assert!(r2.applied.is_empty());
}

#[test]
fn concurrent_apply_only_one_wins_lock() {
    let env = Env::new();
    let ws = env.ws();
    let lock_dir = env.lock_dir();
    let journal = env.journal();

    let results: Vec<bool> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for i in 0..2 {
            let ws = ws.clone();
            let lock_dir = lock_dir.clone();
            let journal = journal.clone();
            handles.push(s.spawn(move || {
                let artifacts = vec![art("a.md", "A")];
                let mut managed = ManagedManifest::new("ws1");
                let plan = build_plan(
                    &artifacts,
                    &ManagedManifest::new("ws1"),
                    &ws,
                    "git+test",
                    Some("r1".into()),
                )
                .unwrap();
                let report = apply(
                    &plan,
                    &artifacts,
                    &mut managed,
                    &ws,
                    &lock_dir,
                    &journal,
                    &format!("dev{i}"),
                );
                report.map(|r| r.ok).unwrap_or(false)
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(
        results.iter().filter(|r| **r).count(),
        1,
        "两个并发 apply 只有一个成功: {results:?}"
    );
    assert_eq!(std::fs::read_to_string(ws.join("a.md")).unwrap(), "A");
}

#[test]
fn user_change_after_plan_rejects_execution() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    std::fs::write(env.ws().join("a.md"), "old").unwrap();
    let mut managed = env.manifest();
    // 先部署一次
    let artifacts = vec![art("a.md", "old")];
    let plan1 = build_plan(&artifacts, &managed, &env.ws(), "git+test", None).unwrap();
    apply(
        &plan1,
        &artifacts,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    // 用户修改
    std::fs::write(env.ws().join("a.md"), "user-edit").unwrap();
    // 新计划（期望 new）：plan 阶段会判 conflict → apply 跳过
    let artifacts2 = vec![art("a.md", "new")];
    let plan2 = build_plan(&artifacts2, &managed, &env.ws(), "git+test", None).unwrap();
    let r = apply(
        &plan2,
        &artifacts2,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r.ok);
    assert_eq!(r.skipped_conflicts.len(), 1, "用户修改被保留为冲突");
    assert_eq!(
        std::fs::read_to_string(env.ws().join("a.md")).unwrap(),
        "user-edit",
        "冲突目标不被覆盖"
    );
}

#[test]
fn injected_failure_leaves_journal_and_recovery_restores() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    let mut managed = env.manifest();
    // 首轮部署 a.md=A-old（由 sync 创建，进入托管清单）
    let artifacts1 = vec![art("a.md", "A-old")];
    let plan1 = build_plan(&artifacts1, &managed, &env.ws(), "git+test", None).unwrap();
    let r1 = apply(
        &plan1,
        &artifacts1,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r1.ok && r1.applied.len() == 1);

    // 故意制造第 2 项写入失败：sub 是文件不是目录 → sub/c.md 写入失败
    std::fs::write(env.ws().join("sub"), "not a dir").unwrap();
    let artifacts2 = vec![art("a.md", "A-new"), art("sub/c.md", "C")];
    let plan2 = build_plan(&artifacts2, &managed, &env.ws(), "git+test", None).unwrap();
    // 排序后 sub/c.md 在 a.md 前？路径排序 "a.md" < "sub/c.md"：先成功 a.md 再失败 sub
    let r2 = apply(
        &plan2,
        &artifacts2,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(!r2.ok, "注入失败必须报告");
    assert!(r2.pending_journal.is_some(), "失败留下恢复点");
    assert_eq!(
        std::fs::read_to_string(env.ws().join("a.md")).unwrap(),
        "A-new",
        "已写入项保留（回滚前）"
    );

    // 崩溃后下次可发现 journal
    let pending = JournalRun::find_pending(&env.journal()).unwrap();
    assert_eq!(pending.len(), 1);

    // 恢复：回滚仍属于本次写入的文件
    let recovered = recover(&env.journal(), &env.ws()).unwrap();
    assert_eq!(recovered, vec!["a.md".to_string()], "恢复到 A-old");
    assert_eq!(
        std::fs::read_to_string(env.ws().join("a.md")).unwrap(),
        "A-old"
    );
    assert!(
        JournalRun::find_pending(&env.journal()).unwrap().is_empty(),
        "恢复后 journal 清理"
    );

    // 恢复后再次同步可成功
    std::fs::remove_file(env.ws().join("sub")).unwrap();
    let plan3 = build_plan(&artifacts2, &managed, &env.ws(), "git+test", None).unwrap();
    let r3 = apply(
        &plan3,
        &artifacts2,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r3.ok, "{r3:?}");
}

#[test]
fn recovery_protects_later_user_edits() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    let mut managed = env.manifest();
    let artifacts = vec![art("a.md", "A-new")];
    let plan = plan_for(&env, &artifacts);
    let r = apply(
        &plan,
        &artifacts,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r.ok);
    // 模拟失败：手动放一个 journal 残留
    let mut run = JournalRun::start(&env.journal(), "fake-crash").unwrap();
    run.append(&ailoom::sync::journal::JournalEntry {
        seq: 0,
        item_key: "a.md".into(),
        path: "a.md".into(),
        action: "update".into(),
        resource_id: "x".into(),
        written_hash: format!("sha256:{}", ailoom::ids::sha256_hex(b"stale")),
        backup_file: None,
        backup_hash: None,
        done: true,
    })
    .unwrap();
    // 用户随后修改 a.md
    std::fs::write(env.ws().join("a.md"), "user-edit").unwrap();
    let recovered = recover(&env.journal(), &env.ws()).unwrap();
    assert!(recovered.is_empty(), "用户编辑不得被恢复覆盖");
    assert_eq!(
        std::fs::read_to_string(env.ws().join("a.md")).unwrap(),
        "user-edit"
    );
}

#[test]
fn pending_journal_blocks_new_apply() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    let mut run = JournalRun::start(&env.journal(), "stale").unwrap();
    run.append(&ailoom::sync::journal::JournalEntry {
        seq: 0,
        item_key: "x.md".into(),
        path: "x.md".into(),
        action: "create".into(),
        resource_id: "x".into(),
        written_hash: "sha256:0".into(),
        backup_file: None,
        backup_hash: None,
        done: true,
    })
    .unwrap();
    let artifacts = vec![art("a.md", "A")];
    let plan = plan_for(&env, &artifacts);
    let mut managed = env.manifest();
    let err = apply(
        &plan,
        &artifacts,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap_err();
    assert_eq!(err.code, "E4005");
    // 删除 journal 目录后可恢复同步（模拟已处理）
    std::fs::remove_dir_all(env.journal().join("stale")).unwrap();
    let r = apply(
        &plan,
        &artifacts,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r.ok);
}

fn _type_assertions(_r: &ApplyReport) {}
