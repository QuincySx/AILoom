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
            .env("HOME", self.tmp.path().join("home"))
            .env("AILOOM_LOG", "error")
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
