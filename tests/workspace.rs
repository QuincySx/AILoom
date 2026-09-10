//! 工作区识别与数据分区（AIL-003）集成测试。

use ailoom::gitx::{git, git_commit_all, git_init};
use ailoom::paths::{ensure_layout, layout_for};
use ailoom::workspace::{discover, AILOOM_DIR, DECLARATION_FILE};

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
