//! 工作区识别与数据分区（AIL-003）集成测试。
//! AIL-039 追加：仓库身份/worktree/子项目发现（文件后半部分）。

use ailoom::gitx::{git, git_commit_all, git_init};
use ailoom::ids::workspace_id_from_root;
use ailoom::paths::{ensure_layout, layout_for};
use ailoom::repo_registry::{
    check_subproject, classify_path, discover_repo, list_worktrees, registry_path,
    suggest_origin_links, PathClass, RepoRegistry, WorktreeStatus,
};
use ailoom::workspace::{discover, find_git_root_up, AILOOM_DIR, DECLARATION_FILE};
use std::path::{Path, PathBuf};

#[test]
fn layout_binding_and_cache_partitioning() {
    let tmp = tempfile::tempdir().unwrap();
    let data_root = tmp.path().join("data");
    let main = tmp.path().join("main");
    git_init(&main, false).unwrap();
    std::fs::write(main.join("f.txt"), "v").unwrap();
    git_commit_all(&main, "init", &["f.txt"]).unwrap();
    let wt = tmp.path().join("wt-b");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            wt.to_str().unwrap(),
            "-b",
            "feature-b",
        ],
    )
    .unwrap();

    let ws_main = discover(&main, None).unwrap();
    let ws_wt = discover(&wt, None).unwrap();

    let l_main = layout_for(&data_root, &ws_main.workspace_id, &ws_main.anchor_key);
    let l_wt = layout_for(&data_root, &ws_wt.workspace_id, &ws_wt.anchor_key);

    assert_eq!(l_main.cache_root, l_wt.cache_root, "同仓库共享缓存 root");
    assert_ne!(
        l_main.binding_path, l_wt.binding_path,
        "binding 按工作区隔离"
    );
    assert_ne!(l_main.events_file, l_wt.events_file, "事件按工作区隔离");

    ensure_layout(&l_main).unwrap();
    ensure_layout(&l_wt).unwrap();
    assert!(l_main.binding_path.parent().unwrap().is_dir());
    assert!(l_main.cache_root.is_dir());
}

#[test]
fn two_worktrees_different_bindings_do_not_interfere() {
    // 绑定文件写入各自的 ws 目录：模拟 A/B 两个 worktree 绑定不同项目
    let tmp = tempfile::tempdir().unwrap();
    let data_root = tmp.path().join("data");
    let main = tmp.path().join("main");
    git_init(&main, false).unwrap();
    std::fs::write(main.join("f.txt"), "v").unwrap();
    git_commit_all(&main, "init", &["f.txt"]).unwrap();
    let wt = tmp.path().join("wt-b");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            wt.to_str().unwrap(),
            "-b",
            "feature-b",
        ],
    )
    .unwrap();
    let ws_main = discover(&main, None).unwrap();
    let ws_wt = discover(&wt, None).unwrap();
    assert_ne!(ws_main.workspace_id, ws_wt.workspace_id);

    let l_a = layout_for(&data_root, &ws_main.workspace_id, &ws_main.anchor_key);
    let l_b = layout_for(&data_root, &ws_wt.workspace_id, &ws_wt.anchor_key);
    ensure_layout(&l_a).unwrap();
    ensure_layout(&l_b).unwrap();
    std::fs::write(&l_a.binding_path, r#"{"projects":["a"]}"#).unwrap();
    std::fs::write(&l_b.binding_path, r#"{"projects":["b"]}"#).unwrap();
    let read_a = std::fs::read_to_string(&l_a.binding_path).unwrap();
    let read_b = std::fs::read_to_string(&l_b.binding_path).unwrap();
    assert!(read_a.contains("\"a\"") && !read_a.contains("\"b\""));
    assert!(read_b.contains("\"b\"") && !read_b.contains("\"a\""));
}

#[test]
fn discovery_fails_without_git_or_declaration() {
    let tmp = tempfile::tempdir().unwrap();
    let err = discover(tmp.path(), None).unwrap_err();
    assert_eq!(err.code, "E1001");
    // 识别失败绝不静默写全局：不创建任何全局文件
    assert!(!tmp.path().join("binding.json").exists());
}

#[test]
fn declaration_root_resources_write_to_current_checkout() {
    // 声明所在目录即 workspace_root：linked worktree 内声明 → 资源写当前 checkout
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    git_init(&main, false).unwrap();
    std::fs::write(main.join("f.txt"), "v").unwrap();
    git_commit_all(&main, "init", &["f.txt"]).unwrap();
    let wt = tmp.path().join("wt-c");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            wt.to_str().unwrap(),
            "-b",
            "feature-c",
        ],
    )
    .unwrap();
    // 在 worktree 内显式声明
    let decl_dir = wt.join(AILOOM_DIR);
    std::fs::create_dir_all(&decl_dir).unwrap();
    std::fs::write(decl_dir.join(DECLARATION_FILE), "schema_version = 1\n").unwrap();
    let ws = discover(&wt, None).unwrap();
    let wt_canon = wt.canonicalize().unwrap();
    assert_eq!(
        ws.workspace_root, wt_canon,
        "资源落点必须是当前 checkout，而不是主 checkout"
    );
    assert!(ws.declaration_path.is_some());
}

#[test]
fn cache_key_stable_across_root_path_changes_of_same_remote() {
    // 同一远端不同本地路径 → 同一 anchor（缓存复用）
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin.git");
    git_init(&origin, true).unwrap();
    let clone1 = tmp.path().join("clone-one");
    let clone2 = tmp.path().join("clone two"); // 含空格
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone1.to_str().unwrap(),
        ],
    )
    .unwrap();
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone2.to_str().unwrap(),
        ],
    )
    .unwrap();
    let a = discover(&clone1, None).unwrap();
    let b = discover(&clone2, None).unwrap();
    assert_eq!(a.repository_anchor, b.repository_anchor);
    assert_ne!(a.workspace_id, b.workspace_id);
}

// ---------------------------------------------------------------------------
// AIL-039：Git 仓库身份、Worktree 与子项目发现
// ---------------------------------------------------------------------------

fn ail039_canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

fn ail039_seed_repo(main: &Path) {
    git_init(main, false).unwrap();
    std::fs::write(main.join("seed.txt"), "seed").unwrap();
    git_commit_all(main, "seed", &["seed.txt"]).unwrap();
}

/// 从主 Worktree、linked worktree、子目录声明处发现同一仓库；
/// 子目录没有自身 .git 也不降为非 Git（anchor 用 Git 身份）。
#[test]
fn ail039_same_repo_from_main_wt_linked_wt_and_subdir_declaration() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    ail039_seed_repo(&main);
    let wt = tmp.path().join("feature");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            wt.to_str().unwrap(),
            "-b",
            "feature",
        ],
    )
    .unwrap();
    // 子目录声明（无自身 .git）
    let decl_dir = main.join("services/api");
    std::fs::create_dir_all(decl_dir.join(AILOOM_DIR)).unwrap();
    std::fs::write(
        decl_dir.join(AILOOM_DIR).join(DECLARATION_FILE),
        "schema_version = 1\n",
    )
    .unwrap();

    let d_main = discover_repo(&main).unwrap();
    let d_wt = discover_repo(&wt).unwrap();
    let d_sub = discover_repo(&decl_dir).unwrap();
    assert_eq!(
        d_main.identity.repo_id, d_wt.identity.repo_id,
        "linked worktree 同仓归组"
    );
    assert_eq!(
        d_main.identity.repo_id, d_sub.identity.repo_id,
        "子目录声明处同仓"
    );

    // workspace 层：声明子目录 is_git=true、anchor 不再是 nongit（Git 身份与作用域分离）
    let ws_decl = discover(&decl_dir, None).unwrap();
    assert!(ws_decl.is_git, "子目录声明不降为非 Git");
    assert_eq!(
        ws_decl.workspace_root,
        ail039_canon(&decl_dir),
        "作用域仍绑定声明目录"
    );
    let ws_root = discover(&main, None).unwrap();
    assert_eq!(
        ws_decl.repository_anchor, ws_root.repository_anchor,
        "同仓共享 anchor"
    );
    assert_ne!(
        ws_decl.workspace_id, ws_root.workspace_id,
        "声明目录是独立作用域"
    );
    assert_eq!(
        ws_decl.workspace_id,
        workspace_id_from_root(&ws_decl.workspace_root)
    );
    // linked worktree：workspace_id 不同、anchor 相同
    let ws_wt = discover(&wt, None).unwrap();
    assert_ne!(ws_wt.workspace_id, ws_root.workspace_id);
    assert_eq!(ws_wt.repository_anchor, ws_root.repository_anchor);
}

/// 相同 common-dir 自动归组；同 origin 独立 clone 只建议关联；无远端仍用 Git 身份。
#[test]
fn ail039_common_dir_groups_and_same_origin_only_suggested() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("origin.git");
    git_init(&bare, true).unwrap();
    let seed = tmp.path().join("seed");
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    )
    .unwrap();
    std::fs::write(seed.join("f.txt"), "f").unwrap();
    git_commit_all(&seed, "f", &["f.txt"]).unwrap();
    git(&seed, &["push", "-q", "origin", "HEAD"]).unwrap();
    let c1 = tmp.path().join("clone-one");
    let c2 = tmp.path().join("clone-two");
    git(
        tmp.path(),
        &["clone", "-q", bare.to_str().unwrap(), c1.to_str().unwrap()],
    )
    .unwrap();
    git(
        tmp.path(),
        &["clone", "-q", bare.to_str().unwrap(), c2.to_str().unwrap()],
    )
    .unwrap();

    let d1 = discover_repo(&c1).unwrap();
    let d2 = discover_repo(&c2).unwrap();
    assert_ne!(
        d1.identity.repo_id, d2.identity.repo_id,
        "独立 clone 不误并"
    );
    assert_eq!(
        d1.identity.origin_normalized, d2.identity.origin_normalized,
        "同 origin 只是关联证据"
    );

    let data = tempfile::tempdir().unwrap();
    for d in [&d1, &d2] {
        let mut reg = RepoRegistry::load_or_create(data.path(), d).unwrap();
        reg.refresh_worktrees(d, "t0");
        reg.save(data.path()).unwrap();
    }
    let suggestions = suggest_origin_links(data.path()).unwrap();
    assert_eq!(suggestions.len(), 1, "同 origin 仅给一条关联建议");
    let (a, b, origin) = &suggestions[0];
    assert_ne!(a, b);
    assert_eq!(origin, d1.identity.origin_normalized.as_ref().unwrap());

    // 无远端仓库：仍用 Git 身份且稳定
    let noremote = tmp.path().join("local-only");
    ail039_seed_repo(&noremote);
    let d3 = discover_repo(&noremote).unwrap();
    assert!(d3.identity.origin_normalized.is_none());
    assert_eq!(
        d3.identity.repo_id,
        discover_repo(&noremote).unwrap().identity.repo_id
    );
    assert_ne!(d3.identity.repo_id, d1.identity.repo_id);
}

/// worktree 状态：bare 不可部署、detached/locked 可辨；失联登记 + 重关联保 id。
#[test]
fn ail039_worktree_states_and_registry_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    ail039_seed_repo(&main);
    let wtd = tmp.path().join("detached-wt");
    git(
        &main,
        &["worktree", "add", "-q", wtd.to_str().unwrap(), "--detach"],
    )
    .unwrap();
    git(
        &main,
        &[
            "worktree",
            "lock",
            wtd.to_str().unwrap(),
            "--reason",
            "冻结中",
        ],
    )
    .unwrap();

    let d = discover_repo(&main).unwrap();
    let info = d
        .worktrees
        .iter()
        .find(|w| w.path == ail039_canon(&wtd))
        .expect("linked worktree 被枚举");
    assert!(info.is_detached, "detached 可辨");
    assert_eq!(
        info.locked_reason.as_deref(),
        Some("冻结中"),
        "locked 原因保留"
    );
    assert!(
        info.deployable(),
        "locked 但非 bare：仍是潜在落点（写前另检）"
    );
    assert!(d.worktrees.iter().any(|w| w.path == ail039_canon(&main)));
    assert_eq!(list_worktrees(&main).unwrap().len(), 2);

    // 登记生命周期：登记 → 移走失联 → 移回重关联（id 不变）
    let data = tempfile::tempdir().unwrap();
    let mut reg = RepoRegistry::load_or_create(data.path(), &d).unwrap();
    let added = reg.refresh_worktrees(&d, "t1");
    assert_eq!(added.len(), 2, "主 + linked 都登记（bare 除外）");
    reg.save(data.path()).unwrap();

    let moved = tmp.path().join("moved-wt");
    std::fs::rename(&wtd, &moved).unwrap();
    let d2 = discover_repo(&main).unwrap();
    let mut reg2 = RepoRegistry::load_or_create(data.path(), &d2).unwrap();
    reg2.refresh_worktrees(&d2, "t2");
    let miss = reg2
        .worktrees
        .values()
        .find(|e| e.status == WorktreeStatus::Missing)
        .expect("移走后标失联")
        .clone();
    assert_eq!(miss.first_seen, "t1", "保留首次登记");
    reg2.save(data.path()).unwrap();

    std::fs::rename(&moved, &wtd).unwrap();
    let mut reg3 = RepoRegistry::load_or_create(data.path(), &d2).unwrap();
    reg3.relink_worktree(data.path(), &miss.id, &wtd, "t3")
        .unwrap();
    assert_eq!(reg3.worktrees[&miss.id].status, WorktreeStatus::Active);
    assert_eq!(
        reg3.worktrees[&miss.id].first_seen, "t1",
        "重关联不改登记 id"
    );
    assert!(
        registry_path(data.path(), &d.identity.repo_id).is_file(),
        "登记落盘在仓外数据区"
    );
}

/// 子项目作用域校验：仓内相对路径可登记；`..`/绝对/仓库根/Git 边界拒绝；
/// 模板路径允许当前 worktree 未命中（不自动创建业务目录）；非 Git 目录走路径模式。
#[test]
fn ail039_subproject_scope_boundaries() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("mono");
    ail039_seed_repo(&main);
    std::fs::create_dir_all(main.join("web/docs")).unwrap();
    std::fs::write(main.join("web/docs/a.md"), "a").unwrap();
    git_commit_all(&main, "docs", &["web/docs/a.md"]).unwrap();
    // 嵌套独立仓库（非子模块）
    let nested = main.join("vendor/lib");
    std::fs::create_dir_all(&nested).unwrap();
    git_init(&nested, false).unwrap();

    let d = discover_repo(&main).unwrap();
    assert!(check_subproject(&d, "web").ok);
    assert!(check_subproject(&d, "web/docs").ok);
    let wt2 = tmp.path().join("wt-tpl");
    git(
        &main,
        &["worktree", "add", "-q", wt2.to_str().unwrap(), "-b", "tpl"],
    )
    .unwrap();
    assert!(
        check_subproject(&discover_repo(&wt2).unwrap(), "web/docs").ok,
        "模板路径允许当前 worktree 未命中"
    );
    for bad in ["..", "/etc", ".", "vendor/lib", "web/../.."] {
        assert!(!check_subproject(&d, bad).ok, "应拒绝: {bad}");
    }

    // 非 Git 目录：路径模式，不当 Git；Git 探测错误不允许静默降级
    let plain = tmp.path().join("plain-dir");
    std::fs::create_dir_all(&plain).unwrap();
    match classify_path(&plain).unwrap() {
        PathClass::NonGit { root } => assert_eq!(root, ail039_canon(&plain)),
        PathClass::Git(_) => panic!("plain-dir 不应判为 Git"),
    }
    let fake = tmp.path().join("fake-git");
    std::fs::create_dir_all(&fake).unwrap();
    std::fs::write(fake.join(".git"), "gitdir: /nonexistent/x").unwrap();
    let err = discover_repo(&fake).unwrap_err();
    assert!(
        err.code.starts_with("E2"),
        "探测错误应报 Git 段错误而非非 Git: {err}"
    );
    assert_eq!(
        find_git_root_up(&main.join("web/docs")).as_deref(),
        Some(main.as_path())
    );
}

/// porcelain -z 无损：含换行的路径不会被记录分隔符截断（AIL-039）。
#[test]
fn ail039_worktree_path_with_newline_survives_porcelain_z() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    ail039_seed_repo(&main);
    let weird = tmp.path().join("weird\nline");
    std::fs::create_dir_all(&weird).unwrap();
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            weird.to_str().unwrap(),
            "-b",
            "weird",
        ],
    )
    .unwrap();
    let d = discover_repo(&main).unwrap();
    let found = d
        .worktrees
        .iter()
        .find(|w| w.path.to_string_lossy().contains('\n'))
        .expect("含换行的 worktree 路径被无损枚举");
    assert_eq!(found.branch.as_deref(), Some("refs/heads/weird"));
}
