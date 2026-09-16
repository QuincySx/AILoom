//! AIL-040 集成验收：仓外个人配置、作用域继承与 v1 兼容。
//! 个人 profile 落在机器数据区（仓外）；与 repo_registry 登记联动；
//! 团队声明语义不变；损坏的 profile 显式报错。

#![allow(clippy::field_reassign_with_default, clippy::cloned_ref_to_slice_refs)]

use ailoom::gitx::{git, git_commit_all, git_init};
use ailoom::ids::now_iso;
use ailoom::profile::{
    resolve_effective, LibraryRef, PersonalProfile, RepoProfile, ResolveScopeRequest,
    ScopeSelection, SubprojectSelection, TriState,
};
use ailoom::repo_registry::{discover_repo, RepoRegistry};
use std::path::PathBuf;

fn canon(p: &std::path::Path) -> PathBuf {
    p.canonicalize().unwrap()
}

fn seed_repo(main: &std::path::Path) {
    git_init(main, false).unwrap();
    std::fs::write(main.join("seed.txt"), "seed").unwrap();
    git_commit_all(main, "seed", &["seed.txt"]).unwrap();
}

fn sel(resources: &[(&str, TriState)]) -> ScopeSelection {
    ScopeSelection {
        hosts: Default::default(),
        resources: resources.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    }
}

/// 个人配置写入仓外数据区；项目根不新增任何个人文件；真实仓库登记联动解析。
#[test]
fn profile_stored_outside_repo_and_resolves_with_registry() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    seed_repo(&main);
    let data_root = tmp.path().join("data"); // 仓外机器数据区

    // 发现 + 登记（AIL-039 接口）
    let d = discover_repo(&main).unwrap();
    let mut reg = RepoRegistry::load_or_create(&data_root, &d).unwrap();
    reg.refresh_worktrees(&d, &now_iso());
    reg.save(&data_root).unwrap();
    let wt_id = reg
        .worktrees
        .values()
        .find(|w| w.path == canon(&main))
        .unwrap()
        .id
        .clone();

    // 写个人配置：仓库默认启用个人 skill、禁用团队 skill
    let mut profile = PersonalProfile::new();
    profile.library = Some(LibraryRef {
        path: data_root.join("library"),
    });
    let mut repo = RepoProfile::default();
    repo.default = Some(sel(&[
        ("personal/skill/common/myflow", TriState::Enable),
        ("team/skill/common/deploy", TriState::Disable),
    ]));
    profile.repos.insert(d.identity.repo_id.clone(), repo);
    profile.save(&data_root).unwrap();

    // profile 落盘在数据区，项目根没有任何新文件
    assert!(PersonalProfile::profile_path(&data_root).is_file());
    assert!(!main.join("profile.toml").exists());
    assert_eq!(
        list_new_files(&main),
        Vec::<PathBuf>::new(),
        "公司仓未新增文件"
    );

    // 从盘加载并解析：团队层启用 deploy（模拟 v1 解析结果），个人层禁用它
    let loaded = PersonalProfile::load_or_default(&data_root).unwrap();
    assert_eq!(
        loaded.library.as_ref().unwrap().path,
        data_root.join("library")
    );
    let out = resolve_effective(ResolveScopeRequest {
        profile: &loaded,
        repo_id: &d.identity.repo_id,
        worktree_id: &wt_id,
        active_rel: None,
        team_enabled: &[
            "team/skill/common/deploy".to_string(),
            "team/skill/common/lint".to_string(),
        ],
        team_hosts: &["claude".to_string()],
    });
    assert!(
        !out.resources["team/skill/common/deploy"].deployed,
        "个人禁用覆盖团队启用"
    );
    assert!(
        out.resources["team/skill/common/lint"].deployed,
        "未表态资源保持团队层值"
    );
    assert!(
        out.resources["personal/skill/common/myflow"].deployed,
        "个人库资源独立启用"
    );
    assert!(out.hosts["claude"].enabled);
}

/// 无 profile 文件 = 纯 v1 行为（个人模式零门槛；团队声明语义不变）。
#[test]
fn missing_profile_keeps_pure_v1_semantics() {
    let tmp = tempfile::tempdir().unwrap();
    let data_root = tmp.path().join("data");
    let profile = PersonalProfile::load_or_default(&data_root).unwrap();
    assert!(profile.repos.is_empty());
    let out = resolve_effective(ResolveScopeRequest {
        profile: &profile,
        repo_id: "repo-0123456789abcdef",
        worktree_id: "wt",
        active_rel: None,
        team_enabled: &["team/skill/common/a".to_string()],
        team_hosts: &["codex".to_string()],
    });
    assert!(out.resources["team/skill/common/a"].deployed);
    assert_eq!(
        out.resources["team/skill/common/a"]
            .origin
            .as_ref()
            .unwrap(),
        &ailoom::profile::ScopeOrigin::TeamDeclaration
    );
    assert!(out.hosts["codex"].enabled);
}

/// 损坏的 profile 显式报错（不静默当空配置）；schema_version 不符拒绝。
#[test]
fn corrupted_profile_errors_loudly() {
    let tmp = tempfile::tempdir().unwrap();
    let data_root = tmp.path().join("data");
    std::fs::create_dir_all(PersonalProfile::profile_path(&data_root).parent().unwrap()).unwrap();
    std::fs::write(
        PersonalProfile::profile_path(&data_root),
        "not [valid toml ===",
    )
    .unwrap();
    let err = PersonalProfile::load_or_default(&data_root).unwrap_err();
    assert_eq!(err.code, "E3001");

    std::fs::write(
        PersonalProfile::profile_path(&data_root),
        "schema_version = 99\n",
    )
    .unwrap();
    let err2 = PersonalProfile::load_or_default(&data_root).unwrap_err();
    assert_eq!(err2.code, "E3001", "未知 schema_version 拒绝");
}

/// 子项目模板跨 worktree 生效：某工作树无该目录时显示未匹配（不部署该子项目层）。
#[test]
fn subproject_template_missing_dir_is_unmatched_not_created() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    seed_repo(&main);
    std::fs::create_dir_all(main.join("web/docs")).unwrap();
    std::fs::write(main.join("web/docs/a.md"), "a").unwrap();
    git_commit_all(&main, "docs", &["web/docs/a.md"]).unwrap();
    let wt = tmp.path().join("wt-clean");
    git(
        &main,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "clean"],
    )
    .unwrap();

    let data_root = tmp.path().join("data");
    let d_main = discover_repo(&main).unwrap();
    let mut reg = RepoRegistry::load_or_create(&data_root, &d_main).unwrap();
    reg.refresh_worktrees(&d_main, &now_iso());
    let wt_clean_id = reg
        .worktrees
        .values()
        .find(|w| w.path == canon(&wt))
        .unwrap()
        .id
        .clone();

    let mut profile = PersonalProfile::new();
    let mut repo = RepoProfile::default();
    repo.subprojects.push(SubprojectSelection {
        path: "web/docs".into(),
        selection: sel(&[("team/skill/common/a", TriState::Disable)]),
    });
    profile.repos.insert(d_main.identity.repo_id.clone(), repo);
    profile.save(&data_root).unwrap();
    let loaded = PersonalProfile::load_or_default(&data_root).unwrap();

    // 主工作树：web/docs 命中 → 禁用生效
    let out_main = resolve_effective(ResolveScopeRequest {
        profile: &loaded,
        repo_id: &d_main.identity.repo_id,
        worktree_id: "unused-wt-main",
        active_rel: Some("web/docs"),
        team_enabled: &["team/skill/common/a".to_string()],
        team_hosts: &[],
    });
    assert!(!out_main.resources["team/skill/common/a"].deployed);

    // clean worktree：web/docs 不存在 → 未匹配，模板层不生效，团队层保持
    let out_clean = resolve_effective(ResolveScopeRequest {
        profile: &loaded,
        repo_id: &d_main.identity.repo_id,
        worktree_id: &wt_clean_id,
        active_rel: Some("elsewhere"),
        team_enabled: &["team/skill/common/a".to_string()],
        team_hosts: &[],
    });
    assert!(
        out_clean.resources["team/skill/common/a"].deployed,
        "未命中的子项目模板不改变有效值（也不自动创建业务目录）"
    );
}

fn list_new_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    // seed_repo 只应留下 seed.txt 与 .git
    walk(root, &mut out);
    out.retain(|p| !p.ends_with("seed.txt") && !p.to_string_lossy().contains("/.git"));
    out
}

/// 契约 fixture：docs/fixtures/contract/valid/profile-v1.toml 必须能被解析器接受
/// （AIL-040 §11.2 接口与 fixture 同步落地）。
#[test]
fn contract_fixture_profile_v1_parses() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path =
        std::path::Path::new(manifest_dir).join("docs/fixtures/contract/valid/profile-v1.toml");
    let profile = PersonalProfile::load(&path).unwrap();
    let repo = &profile.repos["repo-0123456789abcdef"];
    let d = repo.default.as_ref().unwrap();
    assert_eq!(d.hosts["claude"], TriState::Enable);
    assert_eq!(d.hosts["codex"], TriState::Disable);
    assert_eq!(
        d.resources["personal/skill/common/my-flow"],
        TriState::Enable
    );
    assert_eq!(d.resources["team/skill/common/deploy"], TriState::Disable);
    assert_eq!(d.resources["team/skill/common/lint"], TriState::Inherit);
    assert_eq!(repo.subprojects[0].path, "web");
    assert_eq!(
        repo.worktrees["wt123"].resources["team/skill/common/deploy"],
        TriState::Disable
    );
    assert_eq!(repo.wt_subprojects["wt123"][0].path, "web/docs");
}
