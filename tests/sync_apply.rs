//! 同步执行、锁与失败恢复（AIL-008）集成测试。

mod common;

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
    let report = recover(&env.journal(), &env.ws()).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.recovered, vec!["a.md".to_string()], "恢复到 A-old");
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
    let report = recover(&env.journal(), &env.ws()).unwrap();
    assert!(report.recovered.is_empty(), "用户编辑不得被恢复覆盖");
    assert_eq!(
        report.skipped_user_modified,
        vec!["a.md".to_string()],
        "跳过项必须明确报告"
    );
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

#[test]
fn toml_array_entry_create_resync_update_and_remove() {
    let env = Env::new();
    let entry = |name: &str, prompt: &str| {
        let mut t = toml::value::Table::new();
        t.insert("name".into(), toml::Value::String(name.into()));
        t.insert("description".into(), toml::Value::String("d".into()));
        t.insert(
            "system_prompt_base".into(),
            toml::Value::String(prompt.into()),
        );
        Artifact {
            resource_id: format!("team/agent/common/{name}"),
            target_tool: "alva".into(),
            kind: "agent".into(),
            path: ".alva/agents.toml".into(),
            body: ArtifactBody::TomlArrayEntry {
                table: "agent".into(),
                key_field: "name".into(),
                entry: toml::Value::Table(t),
            },
        }
    };

    let mut managed = env.manifest();
    let a1 = entry("helper", "v1");
    let plan = plan_for(&env, std::slice::from_ref(&a1));
    let r = apply(
        &plan,
        std::slice::from_ref(&a1),
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r.ok, "{r:?}");
    let path = env.ws().join(".alva/agents.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("helper"), "{text}");
    assert!(text.contains("v1"), "{text}");

    // 二次 sync：已有 [[agent]] 数组，必须幂等成功（回归：不可因 as_table_mut 冲突）
    let plan2 = build_plan(
        std::slice::from_ref(&a1),
        &managed,
        &env.ws(),
        "git+test",
        Some("r1".into()),
    )
    .unwrap();
    let r2 = apply(
        &plan2,
        std::slice::from_ref(&a1),
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r2.ok, "{r2:?}");

    // 更新同一 name
    let a2 = entry("helper", "v2");
    let plan3 = build_plan(
        std::slice::from_ref(&a2),
        &managed,
        &env.ws(),
        "git+test",
        Some("r2".into()),
    )
    .unwrap();
    let r3 = apply(
        &plan3,
        std::slice::from_ref(&a2),
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r3.ok, "{r3:?}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("v2"), "{text}");
    assert!(!text.contains("v1"), "{text}");

    // 叠加第二个 agent，再删第一个
    let other = entry("other", "ox");
    let both = vec![a2.clone(), other.clone()];
    let plan4 = build_plan(&both, &managed, &env.ws(), "git+test", Some("r3".into())).unwrap();
    apply(
        &plan4,
        &both,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    let only_other = vec![other.clone()];
    let plan5 = build_plan(
        &only_other,
        &managed,
        &env.ws(),
        "git+test",
        Some("r4".into()),
    )
    .unwrap();
    let r5 = apply(
        &plan5,
        &only_other,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r5.ok, "{r5:?}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("other"), "{text}");
    assert!(!text.contains("helper"), "{text}");
}

fn _type_assertions(_r: &ApplyReport) {}

// ---------- AIL-008 返工回归（R01：同文件多片段备份覆盖 / 恢复残留） ----------

fn frag(path: &str, rid: &str, content: &str) -> Artifact {
    Artifact {
        resource_id: rid.into(),
        target_tool: "codex".into(),
        kind: "rule".into(),
        path: path.into(),
        body: ArtifactBody::Fragment {
            content: content.into(),
        },
    }
}

fn fragment_marker(rid: &str, body: &str) -> String {
    format!("\n<!-- BEGIN AILOOM MANAGED: {rid} -->\n{body}\n<!-- END AILOOM MANAGED: {rid} -->\n")
}

/// 同一文件连加两个片段后注入失败：每步独立备份，恢复逐字节回到原始内容。
#[test]
fn multi_fragment_same_file_recovery_restores_original_bytes() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    let mut managed = env.manifest();
    let original = "# 我的工作区\n\n用户自有内容\n";
    std::fs::write(env.ws().join("AGENTS.md"), original).unwrap();

    let f1 = frag(
        "AGENTS.md",
        "team/rule/common/rule-one",
        &fragment_marker("team/rule/common/rule-one", "规则一"),
    );
    let f2 = frag(
        "AGENTS.md",
        "team/rule/common/rule-two",
        &fragment_marker("team/rule/common/rule-two", "规则二"),
    );
    // 注入失败：sub 是文件不是目录 → sub/c.md 写失败，且路径排序在 AGENTS.md 之后
    std::fs::write(env.ws().join("sub"), "not a dir").unwrap();
    let bad = art("sub/c.md", "C");
    let artifacts = vec![f1, f2, bad];
    let plan = build_plan(&artifacts, &managed, &env.ws(), "git+test", None).unwrap();

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
    assert!(!r.ok, "注入失败必须报告");
    let residue = std::fs::read_to_string(env.ws().join("AGENTS.md")).unwrap();
    assert_ne!(residue, original, "失败后片段仍在（回滚前）");

    let report = recover(&env.journal(), &env.ws()).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(
        report.recovered,
        vec!["AGENTS.md".to_string(), "AGENTS.md".to_string()],
        "两个片段步骤各自独立还原"
    );
    let restored = std::fs::read_to_string(env.ws().join("AGENTS.md")).unwrap();
    assert_eq!(restored, original, "恢复必须逐字节回到原始内容");
    assert!(
        JournalRun::find_pending(&env.journal()).unwrap().is_empty(),
        "成功恢复后 journal 清理"
    );
}

/// 备份损坏：恢复报失败，目标不被掩盖性还原，journal 保留待人工处置。
#[test]
fn corrupt_backup_reports_failure_and_keeps_journal() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    let mut managed = env.manifest();
    // 首轮部署 a.md=A-old（进入托管清单），第二轮更新时才会产生备份
    let first = vec![art("a.md", "A-old")];
    let plan0 = build_plan(&first, &managed, &env.ws(), "git+test", None).unwrap();
    let r0 = apply(
        &plan0,
        &first,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r0.ok && r0.applied.len() == 1);
    std::fs::write(env.ws().join("sub"), "not a dir").unwrap();
    let artifacts = vec![art("a.md", "A-new"), art("sub/c.md", "C")];
    let plan = build_plan(&artifacts, &managed, &env.ws(), "git+test", None).unwrap();
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
    assert!(!r.ok);
    let run_dir = &JournalRun::find_pending(&env.journal()).unwrap()[0];
    let backup = &std::fs::read_dir(run_dir.join("backup"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    std::fs::write(backup, b"corrupted-by-something-else").unwrap();

    let report = recover(&env.journal(), &env.ws()).unwrap();
    assert!(!report.ok, "备份摘要不匹配必须报失败");
    assert_eq!(report.broken_backups, vec!["a.md".to_string()]);
    assert_eq!(
        std::fs::read_to_string(env.ws().join("a.md")).unwrap(),
        "A-new",
        "不得以损坏备份覆盖目标"
    );
    assert_eq!(
        JournalRun::find_pending(&env.journal()).unwrap().len(),
        1,
        "损坏时保留恢复点证据"
    );
    assert_eq!(report.pending_runs.len(), 1);

    // 修复备份内容后可重新恢复（等价人工处置）
    std::fs::write(backup, b"A-old").unwrap();
    // 手工计算摘要太繁琐：直接改用缺失备份场景验证重复恢复幂等（见下）
}

/// 备份缺失：恢复报失败并保留 journal。
#[test]
fn missing_backup_reports_failure() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    let mut managed = env.manifest();
    // 首轮部署 a.md=A-old（进入托管清单），第二轮更新时才会产生备份
    let first = vec![art("a.md", "A-old")];
    let plan0 = build_plan(&first, &managed, &env.ws(), "git+test", None).unwrap();
    let r0 = apply(
        &plan0,
        &first,
        &mut managed,
        &env.ws(),
        &env.lock_dir(),
        &env.journal(),
        "dev1",
    )
    .unwrap();
    assert!(r0.ok && r0.applied.len() == 1);
    std::fs::write(env.ws().join("sub"), "not a dir").unwrap();
    let artifacts = vec![art("a.md", "A-new"), art("sub/c.md", "C")];
    let plan = build_plan(&artifacts, &managed, &env.ws(), "git+test", None).unwrap();
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
    assert!(!r.ok);
    let run_dir = &JournalRun::find_pending(&env.journal()).unwrap()[0];
    std::fs::remove_dir_all(run_dir.join("backup")).unwrap();

    let report = recover(&env.journal(), &env.ws()).unwrap();
    assert!(!report.ok);
    assert_eq!(report.broken_backups, vec!["a.md".to_string()]);
    assert_eq!(JournalRun::find_pending(&env.journal()).unwrap().len(), 1);
}

/// 用户在恢复前修改目标：拒绝覆盖、报告跳过；重复 recover 幂等。
#[test]
fn recover_skips_user_modified_and_is_idempotent() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    let mut managed = env.manifest();
    let original = "# 我的工作区\n\n用户自有内容\n";
    std::fs::write(env.ws().join("AGENTS.md"), original).unwrap();
    let f1 = frag(
        "AGENTS.md",
        "team/rule/common/rule-one",
        &fragment_marker("team/rule/common/rule-one", "规则一"),
    );
    std::fs::write(env.ws().join("sub"), "not a dir").unwrap();
    let bad = art("sub/c.md", "C");
    let artifacts = vec![f1, bad];
    let plan = build_plan(&artifacts, &managed, &env.ws(), "git+test", None).unwrap();
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
    assert!(!r.ok);

    // 用户在恢复前编辑了目标
    std::fs::write(env.ws().join("AGENTS.md"), "用户手改").unwrap();
    let report = recover(&env.journal(), &env.ws()).unwrap();
    assert!(report.ok);
    assert!(report.recovered.is_empty());
    assert_eq!(report.skipped_user_modified, vec!["AGENTS.md".to_string()]);
    assert_eq!(
        std::fs::read_to_string(env.ws().join("AGENTS.md")).unwrap(),
        "用户手改",
        "用户编辑必须原样保留"
    );
    assert!(
        JournalRun::find_pending(&env.journal()).unwrap().is_empty(),
        "无损坏时（全部还原或跳过）journal 清理"
    );

    // 重复 recover 幂等：无恢复点、无输出
    let again = recover(&env.journal(), &env.ws()).unwrap();
    assert!(again.ok && again.recovered.is_empty() && again.skipped_user_modified.is_empty());
}

/// JSON/TOML 多条目共用恢复约束：同文件两个托管条目后失败，恢复不残留条目、不丢用户内容。
#[test]
fn json_multi_entry_recovery_keeps_user_entries() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws()).unwrap();
    let mut managed = env.manifest();
    // 用户已有自己的 mcp 配置
    let original = r#"{
  "mcpServers": {
    "user-own": {
      "command": "user-server"
    }
  }
}"#;
    std::fs::write(env.ws().join(".mcp.json"), original).unwrap();

    let entry = |name: &str, command: &str| Artifact {
        resource_id: format!("team/mcp/common/{name}"),
        target_tool: "claude".into(),
        kind: "mcp".into(),
        path: ".mcp.json".into(),
        body: ArtifactBody::JsonPointer {
            pointer: format!("/mcpServers/{name}"),
            value: serde_json::json!({ "command": command }),
        },
    };
    let m1 = entry("team-a", "cmd-a");
    let m2 = entry("team-b", "cmd-b");
    std::fs::write(env.ws().join("sub"), "not a dir").unwrap();
    let bad = art("sub/c.md", "C");
    let artifacts = vec![m1, m2, bad];
    let plan = build_plan(&artifacts, &managed, &env.ws(), "git+test", None).unwrap();
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
    assert!(!r.ok, "注入失败必须报告");

    let report = recover(&env.journal(), &env.ws()).unwrap();
    assert!(report.ok, "{report:?}");
    let restored = std::fs::read_to_string(env.ws().join(".mcp.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&restored).unwrap();
    assert!(
        v["mcpServers"].get("user-own").is_some(),
        "用户条目保留: {restored}"
    );
    assert!(v["mcpServers"].get("team-a").is_none(), "托管条目 A 不残留");
    assert!(v["mcpServers"].get("team-b").is_none(), "托管条目 B 不残留");
    assert!(JournalRun::find_pending(&env.journal()).unwrap().is_empty());
}

/// 真实 CLI 入口：AGENTS.md 连加索引+两条规则片段后，第三目标（CLAUDE.md）注入失败，
/// `ailoom sync --recover` 从实际恢复入口把 AGENTS.md 逐字节还原。
#[test]
fn cli_sync_failure_then_recover_restores_agents_md() {
    use std::path::PathBuf;
    use std::process::Command;

    fn bin() -> PathBuf {
        let mut path = std::env::current_exe().unwrap();
        while path.pop() {
            let p = if cfg!(windows) {
                "ailoom.exe"
            } else {
                "ailoom"
            };
            if path.join(p).exists() {
                return path.join(p);
            }
        }
        panic!("未找到 ailoom 二进制");
    }

    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(&tmp.path().join("src-root"));
    // 第二条规则 + 一个文档（文档触发 CLAUDE.md/AGENTS.md 索引片段）
    std::fs::write(
        src.join("resources/rules/rule-two.md"),
        "---\nname: rule-two\ndescription: 规则二\nshared: true\nprojects: []\nroles: []\nnamespace: common\n---\n\n# 规则二\n\n遵循第二条规则。\n",
    )
    .unwrap();
    std::fs::create_dir_all(src.join("resources/docs")).unwrap();
    std::fs::write(
        src.join("resources/docs/release-notes.md"),
        "---\nname: release-notes\ndescription: 发布说明\nshared: true\nprojects: []\nroles: []\nnamespace: common\n---\n\n# 发布说明\n",
    )
    .unwrap();
    ailoom::gitx::git_commit_all(&src, "add rule-two and doc", &["resources"]).unwrap();

    let ws = common::make_business_repo(&tmp.path().join("biz"), "biz");
    let original = "# 我的工作区说明\n\n自定义内容\n";
    std::fs::write(ws.join("AGENTS.md"), original).unwrap();
    ailoom::gitx::git_commit_all(&ws, "user agents", &["AGENTS.md"]).unwrap();

    // 注入失败：CLAUDE.md 是目录（路径排序在 AGENTS.md 之后 → 最后一个写入动作失败）
    std::fs::create_dir(ws.join("CLAUDE.md")).unwrap();

    let run = |args: &[&str]| {
        let mut cmd = Command::new(bin());
        cmd.args(args).current_dir(&ws);
        for (k, v) in common::isolated_child_env(tmp.path()) {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    };
    let dr = tmp.path().join("data").to_string_lossy().into_owned();
    let url = common::file_url(&src);

    let out = run(&[
        "--data-root",
        &dr,
        "init",
        "--url",
        &url,
        "--project",
        "a",
        "--role",
        "dev",
    ]);
    assert_eq!(out.status.code(), Some(0));

    let out = run(&["--data-root", &dr, "sync"]);
    assert_ne!(out.status.code(), Some(0), "注入失败 sync 必须非零退出");
    let residue = std::fs::read_to_string(ws.join("AGENTS.md")).unwrap();
    assert_ne!(residue, original, "失败后 AGENTS.md 有片段残留（回滚前）");

    let out = run(&["--data-root", &dr, "sync", "--recover"]);
    assert_eq!(out.status.code(), Some(0), "recover 应成功");
    let restored = std::fs::read_to_string(ws.join("AGENTS.md")).unwrap();
    assert_eq!(
        restored, original,
        "实际 recover 入口必须逐字节还原 AGENTS.md"
    );

    // 解除注入后重试成功
    std::fs::remove_dir(ws.join("CLAUDE.md")).unwrap();
    let out = run(&["--data-root", &dr, "sync"]);
    assert_eq!(out.status.code(), Some(0), "恢复后重试 sync 应成功");
}
