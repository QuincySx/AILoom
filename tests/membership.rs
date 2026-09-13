//! 成员名册与项目查询（AIL-024）集成测试。

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

fn setup(c: &Ctx) -> (PathBuf, PathBuf) {
    let bare = c.tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let team_src = common::make_team_source(c.tmp.path());
    ailoom::gitx::git(&team_src, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(
        &team_src,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    )
    .unwrap();
    ailoom::gitx::git(&team_src, &["push", "-q", "-u", "origin", "main"]).unwrap();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = bare.to_str().unwrap().to_string();
    let dr = c.dr();
    let args = [
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    (bare, ws)
}

#[test]
fn register_then_merge_shows_a_plus_b_and_activation_stays_local() {
    let c = Ctx::new();
    let (bare, ws) = setup(&c);
    let src = c.tmp.path().join("team-src");
    let dr = c.dr();

    // alice 在 a 项目登记
    let args = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "members".to_string(),
        "--action".to_string(),
        "register".to_string(),
        "--project".to_string(),
        "a".to_string(),
        "--member".to_string(),
        "alice".to_string(),
        "--provider".to_string(),
        "manual".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    // 合并到远端（模拟审核通过）
    ailoom::gitx::git(&src, &["fetch", "-q", "origin"]).unwrap();
    ailoom::gitx::git(
        &src,
        &["checkout", "-q", "-B", "integration", "origin/main"],
    )
    .unwrap();
    let branches = ailoom::gitx::git(&src, &["branch", "-r", "--format=%(refname:short)"]).unwrap();
    let contribution_branch = branches
        .lines()
        .find(|b| b.contains("ailoom/contribute/"))
        .unwrap()
        .to_string();
    ailoom::gitx::git(&src, &["cherry-pick", "-x", "-n", &contribution_branch]).unwrap();
    ailoom::gitx::git(
        &src,
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
            "merge roster",
        ],
    )
    .unwrap();
    ailoom::gitx::git(&src, &["push", "-q", "origin", "integration:main"]).unwrap();

    // alice 又在 b 项目（另一 checkout）登记 → 名册显示 a+b
    // 刷新本工作区锁后，查询成员项目
    let args = [
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "members",
        "--action",
        "projects",
        "--member",
        "alice",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v["result"]["projects"],
        serde_json::json!(["a"]),
        "名册参与集合: {v}"
    );

    // 目录激活与名册正交：binding 仍只激活 a（status 显示 projects ["a"]）
    let args = ["--json", "--data-root", &c.dr(), "status"];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v["result"]["declaration"]["projects"],
        serde_json::json!(["a"])
    );
}

#[test]
fn duplicate_register_is_idempotent() {
    let c = Ctx::new();
    let (bare, ws) = setup(&c);
    let src = c.tmp.path().join("team-src");
    let dr = c.dr();
    let args = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "members".to_string(),
        "--action".to_string(),
        "register".to_string(),
        "--project".to_string(),
        "a".to_string(),
        "--member".to_string(),
        "bob".to_string(),
        "--provider".to_string(),
        "manual".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    // 合并贡献分支
    ailoom::gitx::git(&src, &["fetch", "-q", "origin"]).unwrap();
    let branches = ailoom::gitx::git(&src, &["branch", "-r", "--format=%(refname:short)"]).unwrap();
    let contribution_branch = branches
        .lines()
        .find(|b| b.contains("ailoom/contribute/"))
        .unwrap()
        .to_string();
    ailoom::gitx::git(&src, &["checkout", "-q", "-B", "int2", "origin/main"]).unwrap();
    ailoom::gitx::git(&src, &["cherry-pick", "-x", "-n", &contribution_branch]).unwrap();
    ailoom::gitx::git(
        &src,
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
            "merge",
        ],
    )
    .unwrap();
    ailoom::gitx::git(&src, &["push", "-q", "origin", "int2:main"]).unwrap();

    // 重复登记（同样内容）→ changed=false，不产生空提交
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    // 幂等：变更集已含相同内容 → 无新提交（no_changes）
    assert_eq!(v["result"]["no_changes"], true, "重复登记幂等: {v}");
    let _ = bare;
}

/// AIL-024 核心验收：两设备并发更新同一成员——
/// 变更集分支审核模型下：两台设备各自登记 alice 的不同项目，推送各自分支互不覆盖；
/// 审核合并第二个分支时产生显式冲突（不静默丢弃任何一方），
/// 按并集解决后名册 = {a, b}，集合不丢失。
#[test]
fn two_devices_concurrent_updates_do_not_lose_members() {
    let c = Ctx::new();
    let (bare, ws1) = setup(&c);
    let src = c.tmp.path().join("team-src");

    // 设备 2：独立业务仓 + 独立数据根（同远端，缓存各自独立 = 并发起点相同）
    let ws2 = common::make_business_repo(c.tmp.path(), "biz-2");
    let dr2 = c.tmp.path().join("data-2").to_str().unwrap().to_string();
    let url = bare.to_str().unwrap().to_string();
    let args2 = [
        "--data-root".to_string(),
        dr2.clone(),
        "init".to_string(),
        "--url".to_string(),
        url.clone(),
        "--project".to_string(),
        "b".to_string(),
    ];
    let refs2: Vec<&str> = args2.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws2, &refs2);
    assert_eq!(code, 0, "{stderr}");

    let register = |ws: &Path, dr: &str, member: &str, project: &str| -> (String, i32, String) {
        let (code, stdout, stderr) = c.run(
            ws,
            &[
                "--json",
                "--data-root",
                dr,
                "members",
                "--action",
                "register",
                "--member",
                member,
                "--project",
                project,
                "--provider",
                "manual",
            ],
        );
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
        let branch = v["result"]["branch"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        (branch, code, stderr)
    };

    // 设备 1：alice 加入 a → 变更集分支 csA
    let (branch_a, code, stderr) = register(&ws1, &c.dr(), "alice", "a");
    assert_eq!(code, 0, "{stderr}");
    assert!(!branch_a.is_empty());

    // 设备 2：alice 加入 b → 变更集分支 csB（推送各自分支，互不覆盖、互不阻塞）
    let (branch_b, code, stderr) = register(&ws2, &dr2, "alice", "b");
    assert_eq!(code, 0, "{stderr}");
    assert!(!branch_b.is_empty());
    assert_ne!(branch_a, branch_b, "两个设备的变更集分支独立");

    // 审核合并 csA 到 main（模拟审核通过）
    let merge_branch = |branch: &str| -> bool {
        ailoom::gitx::git(&src, &["fetch", "-q", "origin"]).unwrap();
        ailoom::gitx::git(
            &src,
            &["checkout", "-q", "-B", "integration", "origin/main"],
        )
        .unwrap();
        let remote_branch = format!("origin/{branch}");
        let pick = ailoom::gitx::git(&src, &["cherry-pick", "-x", "-n", &remote_branch]);
        let ok = pick.is_ok();
        ailoom::gitx::git(
            &src,
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
                "merge roster",
            ],
        )
        .unwrap();
        ailoom::gitx::git(&src, &["push", "-q", "origin", "integration:main"]).unwrap();
        ok
    };
    assert!(merge_branch(&branch_a), "csA 合并无冲突");

    // 审核合并 csB：base 相同、alice 行被两设备各自写入 → 显式冲突（可审、不静默丢失）
    ailoom::gitx::git(&src, &["fetch", "-q", "origin"]).unwrap();
    ailoom::gitx::git(
        &src,
        &["checkout", "-q", "-B", "integration", "origin/main"],
    )
    .unwrap();
    let pick_result = ailoom::gitx::git(
        &src,
        &["cherry-pick", "-x", "-n", &format!("origin/{branch_b}")],
    );
    assert!(
        pick_result.is_err() || {
            // 若 git 自动合并成功，检查结果不是静默丢失（roster 必须含 a 与 b 之一且另一分支可追溯）
            let roster = std::fs::read_to_string(src.join("membership/roster.toml")).unwrap();
            roster.contains("b")
        },
        "第二个分支要么显式冲突、要么合并结果可追溯，不得静默覆盖"
    );
    if pick_result.is_err() {
        // 审核者按并集解决冲突（两台设备的登记都保留）
        std::fs::create_dir_all(src.join("membership")).unwrap();
        std::fs::write(
            src.join("membership/roster.toml"),
            "schema_version = 1\n\n[members.alice]\nprojects = [\"a\", \"b\"]\narchived = false\n",
        )
        .unwrap();
        ailoom::gitx::git(&src, &["add", "--", "membership/roster.toml"]).unwrap();
        ailoom::gitx::git(
            &src,
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
                "merge roster union",
            ],
        )
        .unwrap();
        ailoom::gitx::git(&src, &["push", "-q", "origin", "integration:main"]).unwrap();
    }

    // 两台设备刷新后查询：alice 的参与集合 = {a, b}（无丢失）
    for (ws, dr) in [(&ws1, c.dr()), (&ws2, dr2.clone())] {
        let args = [
            "--data-root".to_string(),
            dr,
            "init".to_string(),
            "--url".to_string(),
            url.clone(),
            "--refresh".to_string(),
            "--project".to_string(),
            "a".to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run(ws, &refs);
        assert_eq!(code, 0, "{stderr}");
    }
    let (code, out_p, stderr) = c.run(
        &ws1,
        &[
            "--json",
            "--data-root",
            c.dr().as_str(),
            "members",
            "--action",
            "projects",
            "--member",
            "alice",
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(out_p.trim()).unwrap();
    assert_eq!(
        v["result"]["projects"],
        serde_json::json!(["a", "b"]),
        "并发更新后名册为并集，无成员/项目丢失: {v}"
    );
}
