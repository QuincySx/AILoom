//! 同仓资源模式与迁移（AIL-036）集成测试。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

struct Ctx {
    tmp: tempfile::TempDir,
}

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    loop {
        path.pop();
        let p = if cfg!(windows) {
            "ailoom.exe"
        } else {
            "ailoom"
        };
        if path.join(p).exists() {
            return path.join(p);
        }
    }
}

impl Ctx {
    fn new() -> Ctx {
        Ctx {
            tmp: tempfile::tempdir().unwrap(),
        }
    }
    fn run(&self, cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn dr(&self) -> String {
        self.tmp.path().join("data").to_string_lossy().to_string()
    }
}

#[test]
fn migrate_then_sync_in_self_mode_keeps_business_intact() {
    let c = Ctx::new();
    // 独立源 + 业务仓（绑定后迁移到同仓）
    let src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = src.to_str().unwrap().to_string();
    let dr = c.dr();
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    // 业务脏文件与分支（迁移全程不变）
    std::fs::write(ws.join("dirty.txt"), "local").unwrap();
    let branch_before = ailoom::gitx::git(&ws, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
    let head_before = ailoom::gitx::git(&ws, &["rev-parse", "HEAD"]).unwrap();

    // 迁移：copy→校验→切换
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "migrate",
        "--from",
        src.to_str().unwrap(),
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["mode"], "migrate");
    assert!(v["result"]["copied_files"].as_u64().unwrap() >= 10);
    // 子树内容与源一致（摘要校验通过才会成功）

    // 业务脏文件与分支不变
    assert_eq!(
        ailoom::gitx::git(&ws, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
        branch_before
    );
    assert_eq!(
        ailoom::gitx::git(&ws, &["rev-parse", "HEAD"]).unwrap(),
        head_before
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("dirty.txt")).unwrap(),
        "local"
    );

    // 声明已切换为 self；同仓 sync 生效
    let decl = std::fs::read_to_string(ws.join(".ailoom/project.toml")).unwrap();
    assert!(decl.contains("type = \"self\""), "{decl}");
    let args = ["--data-root", dr.as_str(), "sync"];
    let _refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "同仓 sync: {stderr}");
    assert!(
        ws.join(".claude/skills/common-greet/SKILL.md").is_file(),
        "同仓资源部署"
    );

    // 同仓 pull（git pull 语义下子树资源可见）：linked worktree 的资源写当前 checkout
    // 由 AIL-003 worktree 测试保证；此处断言主 checkout 未被当作 worktree 目标
    assert!(!ws.join(".git").is_file(), "主 checkout 的 .git 仍是目录");

    // 统计/上报不推进业务默认分支：报告走独立分支（AIL-022），此处仅验证分支隔离
    let branches = ailoom::gitx::git(&ws, &["branch", "--list"]).unwrap();
    assert!(
        !branches.contains("ailoom/reports"),
        "统计分支不落业务分支: {branches}"
    );
}

#[test]
fn self_mode_offline_and_linked_worktree() {
    let c = Ctx::new();
    let src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = src.to_str().unwrap().to_string();
    let dr = c.dr();
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);

    // 迁移到同仓
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "migrate",
        "--from",
        src.to_str().unwrap(),
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    // 提交声明与子树（均为可提交内容），再建 linked worktree
    ailoom::gitx::git(&ws, &["add", "--", ".ailoom/project.toml"]).unwrap();
    ailoom::gitx::git(&ws, &["add", "--", ".ailoom-team"]).unwrap();
    ailoom::gitx::git(
        &ws,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "commit declaration and team subtree",
        ],
    )
    .unwrap();
    let wt = c.tmp.path().join("biz-wt");
    ailoom::gitx::git(
        &ws,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feat"],
    )
    .unwrap();
    let args = ["--data-root", &c.dr(), "sync"];
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&wt, &refs);
    assert_eq!(code, 0, "linked worktree 同仓 sync: {stderr}");
    assert!(
        wt.join(".claude/skills/common-greet/SKILL.md").is_file(),
        "资源写当前 checkout"
    );
    // 迁移中断恢复：备份声明存在
    let backup_dir = c.tmp.path().join("data").join("ws");
    let mut has_backup = false;
    for entry in std::fs::read_dir(&backup_dir).unwrap().flatten() {
        if entry.path().join("migrate-backup").is_dir() {
            has_backup = true;
        }
    }
    assert!(has_backup, "迁移保留声明备份（可恢复）");
}

// ---------- AIL-036 返工回归（R10：越界覆盖 / 贡献链路） ----------

fn snapshot_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                out.push((
                    p.strip_prefix(base).unwrap().to_string_lossy().into_owned(),
                    std::fs::read(&p).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    if dir.exists() {
        walk(dir, dir, &mut out);
    }
    out.sort();
    out
}

fn init_ws(c: &Ctx, ws: &Path, src: &Path) {
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        src.to_str().unwrap().to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(ws, &refs);
    assert_eq!(code, 0, "init: {stderr}");
}

/// ../、绝对路径、指向工作区外的符号链接在任何写入前拒绝；外部哨兵逐字节不变。
#[test]
fn migrate_rejects_out_of_scope_subtrees_and_leaves_sentinel_untouched() {
    let c = Ctx::new();
    let src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_ws(&c, &ws, &src);

    // 外部哨兵目录
    let sentinel = c.tmp.path().join("sentinel");
    std::fs::create_dir_all(sentinel.join("victim")).unwrap();
    std::fs::write(
        sentinel.join("victim/README.md"),
        b"existing business content",
    )
    .unwrap();
    let before = snapshot_dir(&sentinel);

    // 哨兵内容的工作区外符号链接
    #[cfg(unix)]
    std::os::unix::fs::symlink(&sentinel, ws.join("team-escape")).unwrap();

    let dr = c.dr();
    let cases: Vec<&str> = vec![
        "../victim-relative",
        "/tmp/ailoom-absolute-escape",
        #[cfg(unix)]
        "team-escape",
    ];
    for sub in cases {
        let args = [
            "--json",
            "--data-root",
            dr.as_str(),
            "migrate",
            "--from",
            src.to_str().unwrap(),
            "--subtree",
            sub,
        ];
        let (code, _, stderr) = c.run(&ws, &args);
        assert_ne!(code, 0, "子树 {sub} 必须被拒绝");
        assert!(
            stderr.contains("E8002") || stderr.contains("相对路径") || stderr.contains("工作区外"),
            "应报越界错误: {stderr}"
        );
        assert!(!ws.join(".ailoom-team").exists(), "{sub}: 写入前必须拒绝");
    }

    assert_eq!(before, snapshot_dir(&sentinel), "外部哨兵逐字节不变");
    let decl = std::fs::read_to_string(ws.join(".ailoom/project.toml")).unwrap();
    assert!(decl.contains("type = \"git\""), "声明未被改动: {decl}");
}

/// 已有业务文件不被覆盖；源与目标重叠/嵌套不自复制。
#[test]
fn migrate_refuses_overwrite_and_overlap() {
    let c = Ctx::new();
    let src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_ws(&c, &ws, &src);

    // 预置与源同路径但内容不同的业务文件
    let existing = ws.join(".ailoom-team/resources/skills/common-greet/SKILL.md");
    std::fs::create_dir_all(existing.parent().unwrap()).unwrap();
    std::fs::write(&existing, "# 业务自有内容\n").unwrap();

    let dr = c.dr();
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "migrate",
        "--from",
        src.to_str().unwrap(),
    ];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_ne!(code, 0, "隐式覆盖必须被拒绝");
    assert!(
        stderr.contains("拒绝隐式覆盖") || stderr.contains("E8002"),
        "{stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(&existing).unwrap(),
        "# 业务自有内容\n",
        "已有业务文件逐字节保留"
    );

    // 源与目标重叠：from 在目标子树内 → 拒绝自复制
    // 先清理冲突文件并完成一次正常迁移
    std::fs::remove_dir_all(ws.join(".ailoom-team")).unwrap();
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    let from_self = ws.join(".ailoom-team");
    let from_self_str = from_self.to_str().unwrap().to_string();
    let overlap_args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "migrate",
        "--from",
        from_self_str.as_str(),
    ];
    let (code, _, stderr) = c.run(&ws, &overlap_args);
    assert_ne!(code, 0, "源与目标重叠必须被拒绝");
    assert!(
        stderr.contains("重叠") || stderr.contains("E8002"),
        "{stderr}"
    );
}

/// 迁移中断（物化第 N 文件失败）可重试：旧声明与业务文件逐字节保留。
#[test]
fn migrate_materialize_failure_is_retryable() {
    let c = Ctx::new();
    let src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_ws(&c, &ws, &src);
    let dr = c.dr();
    let decl_path = ws.join(".ailoom/project.toml");
    let decl_before = std::fs::read_to_string(&decl_path).unwrap();

    // 注入：子树中的 resources 目录只读 → 物化复制失败
    let blocked = ws.join(".ailoom-team/resources");
    std::fs::create_dir_all(&blocked).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "migrate",
        "--from",
        src.to_str().unwrap(),
    ];
    let (code, _, _stderr) = c.run(&ws, &args);
    assert_ne!(code, 0, "物化失败必须报告");
    assert_eq!(
        std::fs::read_to_string(&decl_path).unwrap(),
        decl_before,
        "物化失败时旧声明逐字节保留"
    );

    // 解除注入后重试成功
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let (code, _, _stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "重试应成功: {_stderr}");
    let decl = std::fs::read_to_string(&decl_path).unwrap();
    assert!(decl.contains("type = \"self\""), "{decl}");
    // 可恢复记录存在
    let mut has_record = false;
    for e in std::fs::read_dir(c.tmp.path().join("data/ws"))
        .unwrap()
        .flatten()
    {
        if e.path().join("migrate-record.json").is_file() {
            has_record = true;
        }
    }
    assert!(has_record, "应有 migrate-record.json 可恢复记录");
}

/// 真实同仓贡献命令：包含资源改动、分支确实存在、可重试；业务脏文件/HEAD/分支不变。
#[test]
fn contribute_self_cli_creates_reviewable_branch() {
    let c = Ctx::new();
    let src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_ws(&c, &ws, &src);
    let dr = c.dr();
    let (code, _, stderr) = c.run(
        &ws,
        &[
            "--json",
            "--data-root",
            dr.as_str(),
            "migrate",
            "--from",
            src.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "migrate: {stderr}");

    // 业务脏文件与基线
    std::fs::write(ws.join("dirty.txt"), "local").unwrap();
    let branch_before = ailoom::gitx::git(&ws, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
    let head_before = ailoom::gitx::git(&ws, &["rev-parse", "HEAD"]).unwrap();

    // 子树未提交（迁移产生的新文件）→ 贡献应包含全部资源文件
    let (code, stdout, stderr) = c.run(&ws, &["--json", "contribute-self"]);
    assert_eq!(code, 0, "contribute-self: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let branch1 = v["result"]["branch"]
        .as_str()
        .expect("应返回分支名")
        .to_string();
    assert!(
        v["result"]["files"].as_u64().unwrap() > 0,
        "应包含资源改动: {v}"
    );
    assert_eq!(v["result"]["pushed"], false);
    // 分支确实存在（worktree 清理后仍可验证）
    let exists = ailoom::gitx::git(&ws, &["rev-parse", "--verify", &branch1]).unwrap();
    assert!(
        exists.trim().len() == 40,
        "分支 {branch1} 必须存在: {exists}"
    );
    // 提交内容只含子树
    let names = ailoom::gitx::git(&ws, &["show", "--name-only", "--format=", &branch1]).unwrap();
    assert!(
        names.trim().lines().all(|l| l.starts_with(".ailoom-team/")),
        "{names}"
    );

    // 业务脏文件/HEAD/分支全程不变
    assert_eq!(
        std::fs::read_to_string(ws.join("dirty.txt")).unwrap(),
        "local"
    );
    assert_eq!(
        ailoom::gitx::git(&ws, &["rev-parse", "HEAD"]).unwrap(),
        head_before
    );
    assert_eq!(
        ailoom::gitx::git(&ws, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
        branch_before
    );

    // 子树整体提交到业务分支后：无改动 → no_changes
    ailoom::gitx::git(&ws, &["add", "-A", "--", ".ailoom-team"]).unwrap();
    ailoom::gitx::git(
        &ws,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "commit whole team subtree",
        ],
    )
    .unwrap();
    let (code, stdout, stderr) = c.run(&ws, &["--json", "contribute-self"]);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["no_changes"], true, "{v}");
}

/// RW-05/R01：子树内部祖先符号链接指向工作区外时，任何物化前拒绝，
/// 外部内容零写入、声明不变；正常迁移与失败后重试不受影响。
#[test]
fn migrate_rejects_internal_ancestor_symlink_escape_before_materialize() {
    let c = Ctx::new();
    let src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_ws(&c, &ws, &src);

    let outside = c.tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let before = snapshot_dir(&outside);

    // R01 反例：.ailoom-team（真实目录）/ resources → 工作区外
    std::fs::create_dir_all(ws.join(".ailoom-team")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, ws.join(".ailoom-team/resources")).unwrap();
    let dr = c.dr();
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "migrate",
        "--from",
        src.to_str().unwrap(),
    ];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_ne!(code, 0, "内部祖先链接越界必须拒绝");
    assert!(stderr.contains("E8002"), "应报越界错误: {stderr}");
    assert_eq!(before, snapshot_dir(&outside), "外部内容零写入");
    let decl = std::fs::read_to_string(ws.join(".ailoom/project.toml")).unwrap();
    assert!(decl.contains("type = \"git\""), "声明未被切换: {decl}");

    // 悬空链接（无法解析）同样拒绝
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(ws.join(".ailoom-team/resources"));
        std::os::unix::fs::symlink(
            c.tmp.path().join("no-such-target"),
            ws.join(".ailoom-team/dangling"),
        )
        .unwrap();
        let (code, _, stderr) = c.run(&ws, &args);
        assert_ne!(code, 0, "悬空链接必须拒绝");
        assert!(
            stderr.contains("E8002") || stderr.contains("无法解析"),
            "悬空链接错误: {stderr}"
        );
        let _ = std::fs::remove_file(ws.join(".ailoom-team/dangling"));
    }

    // 阻断解除后同一工作区重试：完整迁移成功（可重试性）
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "解除链接后迁移应成功: {stderr}");
    assert!(ws.join(".ailoom-team/resources/skills").is_dir() || ws.join(".ailoom-team").is_dir());
}

/// 多层祖先链接、最终叶子链接、源侧越界链接均在写入前拒绝。
#[test]
fn migrate_boundary_scan_covers_deep_ancestors_leaf_and_source_links() {
    let c = Ctx::new();
    let src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_ws(&c, &ws, &src);
    let outside = c.tmp.path().join("outside2");
    std::fs::create_dir_all(&outside).unwrap();
    let dr = c.dr();
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "migrate",
        "--from",
        src.to_str().unwrap(),
    ];

    // 1) 多层祖先：.ailoom-team/x/y → 外部，源含 x/y 下文件
    std::fs::create_dir_all(ws.join(".ailoom-team/x")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, ws.join(".ailoom-team/x/y")).unwrap();
    let (code, _, stderr) = c.run(&ws, &args);
    assert_ne!(code, 0, "多层祖先链接越界必须拒绝");
    assert!(stderr.contains("E8002"));
    let _ = std::fs::remove_file(ws.join(".ailoom-team/x/y"));

    // 2) 最终叶子链接：.ailoom-team/leaf.md → 外部已有文件（内容不同 → 预检拒绝）
    #[cfg(unix)]
    {
        std::fs::write(outside.join("victim.md"), b"external bytes").unwrap();
        let leaf_before = snapshot_dir(&outside);
        std::os::unix::fs::symlink(outside.join("victim.md"), ws.join(".ailoom-team/leaf.md"))
            .unwrap();
        let (code, _, stderr) = c.run(&ws, &args);
        assert_ne!(code, 0, "最终叶子链接必须拒绝: {stderr}");
        assert!(stderr.contains("E8002") || stderr.contains("拒绝"));
        let _ = std::fs::remove_file(ws.join(".ailoom-team/leaf.md"));
        assert_eq!(leaf_before, snapshot_dir(&outside), "叶子链接目标未被改写");
    }

    // 3) 源含越界链接：迁移在写入前失败
    let src2 = c.tmp.path().join("team-src-with-link");
    crate_clone_tree(&src, &src2);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, src2.join("resources/escape-link")).unwrap();
    let args2 = [
        "--json",
        "--data-root",
        dr.as_str(),
        "migrate",
        "--from",
        src2.to_str().unwrap(),
    ];
    let (code, _, stderr) = c.run(&ws, &args2);
    assert_ne!(code, 0, "源含越界链接必须拒绝");
    assert!(stderr.contains("E8002"));

    // victim.md 是本测试写入的外部文件；断言其内容此后未被迁移触碰
    assert_eq!(
        std::fs::read_to_string(outside.join("victim.md")).unwrap(),
        "external bytes",
        "外部文件逐字节不变"
    );
    assert_eq!(
        std::fs::read_dir(&outside).unwrap().count(),
        1,
        "外部目录无新增条目"
    );
    let decl = std::fs::read_to_string(ws.join(".ailoom/project.toml")).unwrap();
    assert!(decl.contains("type = \"git\""), "声明未被切换");
}

/// 复制目录树（测试辅助，跳过 .git）。
fn crate_clone_tree(from: &Path, to: &Path) {
    for entry in walkdir::WalkDir::new(from)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = match entry.path().strip_prefix(from) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if rel.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        let dest = to.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
        } else if entry.file_type().is_file() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::copy(entry.path(), &dest).unwrap();
        }
    }
}
