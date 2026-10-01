//! 资源清单与项目/角色解析器（AIL-006）集成测试。
//! 复用 tests/common 的契约 §7 场景源仓库。

mod common;

use ailoom::manifest::TeamManifest;
use ailoom::resolver::{resolve, ResolveRequest};
use std::path::Path;

fn resolve_at(
    src: &Path,
    projects: &[&str],
    roles: &[&str],
) -> Result<ailoom::resolver::DesiredSet, ailoom::error::Error> {
    let manifest = TeamManifest::load_from(src)?;
    let projects: Vec<String> = projects.iter().map(|s| s.to_string()).collect();
    let roles: Vec<String> = roles.iter().map(|s| s.to_string()).collect();
    resolve(ResolveRequest {
        snapshot_root: src,
        manifest: &manifest,
        source: "team",
        identity: "git+test",
        revision: Some("cafe1234".into()),
        content_digest: "sha256:test".into(),
        active_projects: &projects,
        active_roles: &roles,
    })
}

fn names(set: &ailoom::resolver::DesiredSet, kind: &str) -> Vec<String> {
    set.selected
        .iter()
        .filter(|s| s.kind == kind)
        .map(|s| s.name.clone())
        .collect()
}

#[test]
fn dev_plus_a_gets_common_dev_a() {
    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(tmp.path());
    let set = resolve_at(&src, &["a"], &["dev"]).unwrap();
    let skills = names(&set, "skill");
    assert!(skills.contains(&"common-greet".to_string()), "{skills:?}");
    assert!(skills.contains(&"dev-tooling".to_string()), "{skills:?}");
    assert!(skills.contains(&"a-deploy".to_string()), "{skills:?}");
    assert!(!skills.contains(&"b-deploy".to_string()), "{skills:?}");
    assert!(!skills.contains(&"pm-checklist".to_string()), "{skills:?}");
    // 经验：A + shared，不含 B
    let learnings = names(&set, "learning");
    assert!(learnings.contains(&"a-postmortem".to_string()));
    assert!(learnings.contains(&"shared-lessons".to_string()));
    assert!(!learnings.contains(&"b-postmortem".to_string()));
    // 原因可解释
    let dev_skill = set
        .selected
        .iter()
        .find(|s| s.name == "dev-tooling")
        .unwrap();
    assert_eq!(dev_skill.reason, "role:dev");
    let a_skill = set.selected.iter().find(|s| s.name == "a-deploy").unwrap();
    assert_eq!(a_skill.reason, "project:a");
    let shared_skill = set
        .selected
        .iter()
        .find(|s| s.name == "common-greet")
        .unwrap();
    assert_eq!(shared_skill.reason, "shared");
    // 被排除项也可解释
    assert!(
        set.excluded
            .iter()
            .any(|e| e.id.ends_with("/skill/team-lib/b-deploy")),
        "{:?}",
        set.excluded
    );
}

#[test]
fn pm_plus_b_gets_common_pm_b() {
    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(tmp.path());
    let set = resolve_at(&src, &["b"], &["pm"]).unwrap();
    let skills = names(&set, "skill");
    // 按 ResourceId 排序：common namespace 在前
    assert_eq!(
        skills,
        vec![
            "common-greet".to_string(),
            "pm-checklist".to_string(),
            "b-deploy".to_string()
        ]
    );
    let learnings = names(&set, "learning");
    assert!(learnings.contains(&"b-postmortem".to_string()));
    assert!(!learnings.contains(&"a-postmortem".to_string()));
}

#[test]
fn zero_projects_gets_shared_only() {
    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(tmp.path());
    let set = resolve_at(&src, &[], &[]).unwrap();
    let skills = names(&set, "skill");
    assert_eq!(
        skills,
        vec!["common-greet".to_string()],
        "零项目只能拿到 shared"
    );
    let learnings = names(&set, "learning");
    assert_eq!(learnings, vec!["shared-lessons".to_string()]);
}

#[test]
fn roles_never_expand_learning_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(tmp.path());
    // dev 角色无论怎么组合都不给 b 的经验
    let set = resolve_at(&src, &["a"], &["dev", "pm"]).unwrap();
    let learnings = names(&set, "learning");
    assert!(
        !learnings.contains(&"b-postmortem".to_string()),
        "角色不得扩大项目经验范围"
    );
}

#[test]
fn both_projects_union_for_skills_but_learning_ambiguous_excluded_from_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(tmp.path());
    let set = resolve_at(&src, &["a", "b"], &[]).unwrap();
    let skills = names(&set, "skill");
    assert!(skills.contains(&"a-deploy".to_string()) && skills.contains(&"b-deploy".to_string()));
    // learning 只能属于一个项目：a 与 b 各自命中自己的
    let learnings = names(&set, "learning");
    assert!(learnings.contains(&"a-postmortem".to_string()));
    assert!(learnings.contains(&"b-postmortem".to_string()));
}

#[test]
fn duplicate_skill_name_in_source_is_rejected_at_parse() {
    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(tmp.path());
    // 单源内目录名唯一 → 同名技能即 name/目录名不一致，解析层直接拒绝；
    // 跨源目标冲突（E3006）由 resolver::check_target_conflicts 单测覆盖。
    let dup = src.join("resources/skills/common-greet-dup");
    std::fs::create_dir_all(&dup).unwrap();
    std::fs::write(
        dup.join("SKILL.md"),
        "---\nname: common-greet\ndescription: 同名\nshared: true\nnamespace: team-lib\n---\n\n重复\n",
    )
    .unwrap();
    let err = resolve_at(&src, &[], &[]).unwrap_err();
    assert_eq!(err.code, "E3002", "{err}");
}

#[test]
fn invalid_fixtures_fail_with_expected_codes() {
    fn err_for(dir: &Path, m: &TeamManifest) -> ailoom::error::Error {
        let empty: Vec<String> = Vec::new();
        resolve(ResolveRequest {
            snapshot_root: dir,
            manifest: m,
            source: "team",
            identity: "git+test",
            revision: None,
            content_digest: "d".into(),
            active_projects: &empty,
            active_roles: &empty,
        })
        .unwrap_err()
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/fixtures/contract/invalid");
    // unknown-namespace → E3004
    let m = TeamManifest::load_from(&fixtures.join("unknown-namespace")).unwrap();
    assert_eq!(
        err_for(&fixtures.join("unknown-namespace"), &m).code,
        "E3004"
    );
    // ownerless-resource → E3005
    let m = TeamManifest::load_from(&fixtures.join("ownerless-resource")).unwrap();
    assert_eq!(
        err_for(&fixtures.join("ownerless-resource"), &m).code,
        "E3005"
    );
    // learning-multi-project → E3002
    let m = TeamManifest::load_from(&fixtures.join("learning-multi-project")).unwrap();
    assert_eq!(
        err_for(&fixtures.join("learning-multi-project"), &m).code,
        "E3002"
    );
}

#[test]
fn symlink_escape_in_skill_dir_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(tmp.path());
    let secret = tmp.path().join("outside-secret");
    std::fs::write(&secret, "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, src.join("resources/skills/common-greet/evil")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&secret, src.join("resources/skills/common-greet/evil"))
        .unwrap();
    let desired = resolve_at(&src, &[], &[]).unwrap();
    assert!(!desired
        .selected
        .iter()
        .any(|item| item.name == "common-greet"));
    assert!(desired.selected.iter().any(|item| item.kind == "rule"));
}

#[test]
fn tags_do_not_change_identity_or_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let src = common::make_team_source(tmp.path());
    // 给 common-greet 增加 tags 字段（未来字段，向前兼容）
    let p = src.join("resources/skills/common-greet/SKILL.md");
    let text = std::fs::read_to_string(&p).unwrap();
    let patched = text.replace(
        "namespace: common",
        "namespace: common\ntags: [greeting, v2]",
    );
    std::fs::write(&p, patched).unwrap();
    let set = resolve_at(&src, &[], &[]).unwrap();
    let skills = names(&set, "skill");
    assert!(skills.contains(&"common-greet".to_string()));
    let entry = set
        .selected
        .iter()
        .find(|s| s.name == "common-greet")
        .unwrap();
    assert_eq!(
        entry.entry.meta.tags,
        vec!["greeting".to_string(), "v2".to_string()]
    );
    assert_eq!(
        entry.id.to_string(),
        "team/skill/common/common-greet",
        "身份不含 tags"
    );
}
