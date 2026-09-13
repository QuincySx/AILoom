//! 资源贡献与审核发布（AIL-014）集成测试：全部使用本地裸仓，无真实远端/gh。

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

/// 本地裸远端 + 团队源仓库（充当用户可编辑克隆）；返回 (bare, team_src)。
fn setup_remote(c: &Ctx) -> (PathBuf, PathBuf) {
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
    (bare, team_src)
}

fn init_ws(c: &Ctx, ws: &Path, url: &str) {
    let dr = c.dr();
    let args = [
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url.to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(ws, &refs);
    assert_eq!(code, 0, "{stderr}");
}

#[test]
fn push_creates_branch_on_remote_without_touching_business_repo() {
    let c = Ctx::new();
    let (bare, user_clone) = setup_remote(&c);
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = common::file_url(&bare);
    init_ws(&c, &ws, &url);

    // 业务仓库制造脏状态（验证全程不变）
    std::fs::write(ws.join("dirty.txt"), "local change").unwrap();
    let branch_before = ailoom::gitx::git(&ws, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
    let head_before = ailoom::gitx::git(&ws, &["rev-parse", "HEAD"]).unwrap();

    // 用户在克隆里修改资源
    let skill = user_clone.join("resources/skills/common-greet/SKILL.md");
    std::fs::write(&skill, "---\nname: common-greet\ndescription: 更新版\nshared: true\nnamespace: common\n---\n\n新问候\n").unwrap();

    let dr = c.dr();
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "push",
        "--from",
        user_clone.to_str().unwrap(),
        "--provider",
        "manual",
        "--message",
        "更新问候技能",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let branch = v["result"]["branch"].as_str().unwrap().to_string();
    assert!(branch.starts_with("ailoom/contribute/"));
    assert_eq!(
        v["result"]["pr_url"],
        serde_json::Value::Null,
        "manual 模式不出 PR"
    );
    assert!(v["result"]["manual_review"]
        .as_str()
        .unwrap()
        .contains("init --refresh"));

    // 远端分支内容 = 用户修改
    ailoom::gitx::git(&user_clone, &["fetch", "-q", "origin"]).unwrap();
    let content = ailoom::gitx::git(
        &user_clone,
        &[
            "show",
            &format!("origin/{branch}:resources/skills/common-greet/SKILL.md"),
        ],
    )
    .unwrap();
    assert!(content.contains("新问候"));

    // 业务仓库不变
    assert_eq!(
        ailoom::gitx::git(&ws, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
        branch_before
    );
    assert_eq!(
        ailoom::gitx::git(&ws, &["rev-parse", "HEAD"]).unwrap(),
        head_before
    );
    assert!(ws.join("dirty.txt").exists(), "业务脏文件保留");
}

#[test]
fn machine_data_never_staged() {
    let c = Ctx::new();
    let (bare, user_clone) = setup_remote(&c);
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = common::file_url(&bare);
    init_ws(&c, &ws, &url);

    // 克隆里放机器数据 + 白名单外文件
    std::fs::create_dir_all(user_clone.join(".ailoom/machine")).unwrap();
    std::fs::write(
        user_clone.join(".ailoom/machine/secret.json"),
        "{\"token\":\"x\"}",
    )
    .unwrap();
    std::fs::write(user_clone.join("resources/stolen.log"), "log").unwrap();
    std::fs::write(
        user_clone.join("resources/skills/common-greet/SKILL.md"),
        "---\nname: common-greet\ndescription: v2\nshared: true\nnamespace: common\n---\n\nv2\n",
    )
    .unwrap();

    let dr = c.dr();
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "push",
        "--from",
        user_clone.to_str().unwrap(),
        "--provider",
        "manual",
        "--message",
        "v2",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let paths: Vec<String> = v["result"]["committed_paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert!(
        paths
            .iter()
            .all(|p| !p.contains("machine") && !p.contains(".log")),
        "{paths:?}"
    );
    assert!(paths.contains(&"resources/skills/common-greet/SKILL.md".to_string()));
}

#[test]
fn retry_reuses_changeset_and_branch() {
    let c = Ctx::new();
    let (bare, user_clone) = setup_remote(&c);
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = common::file_url(&bare);
    init_ws(&c, &ws, &url);

    let skill = user_clone.join("resources/skills/common-greet/SKILL.md");
    let write_v = |n: &str| {
        std::fs::write(
            &skill,
            format!("---\nname: common-greet\ndescription: {n}\nshared: true\nnamespace: common\n---\n\n{n}\n"),
        )
        .unwrap();
    };

    let dr = c.dr();
    let push_args = |clone: &Path, msg: &str| {
        vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "push".to_string(),
            "--from".to_string(),
            clone.to_str().unwrap().to_string(),
            "--provider".to_string(),
            "manual".to_string(),
            "--message".to_string(),
            msg.to_string(),
        ]
    };

    write_v("v2");
    let args = push_args(&user_clone, "v2");
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v1: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let cs1 = v1["result"]["changeset_id"].as_str().unwrap().to_string();
    let branch1 = v1["result"]["branch"].as_str().unwrap().to_string();

    write_v("v3");
    let args = push_args(&user_clone, "v3");
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v2: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v2["result"]["changeset_id"].as_str().unwrap(),
        cs1,
        "重试复用变更集"
    );
    assert_eq!(
        v2["result"]["branch"].as_str().unwrap(),
        branch1,
        "重试复用分支（无重复 PR）"
    );

    // 远端分支 tip = v3
    ailoom::gitx::git(&user_clone, &["fetch", "-q", "origin"]).unwrap();
    let content = ailoom::gitx::git(
        &user_clone,
        &[
            "show",
            &format!("origin/{branch1}:resources/skills/common-greet/SKILL.md"),
        ],
    )
    .unwrap();
    assert!(content.contains("v3"));
}

#[test]
fn remote_diverged_surfaces_diverged_error() {
    let c = Ctx::new();
    let (bare, user_clone) = setup_remote(&c);
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = common::file_url(&bare);
    init_ws(&c, &ws, &url);

    // 直接在远端造一个同名分支（他人已推），使后续推送非快进
    let skill = user_clone.join("resources/skills/common-greet/SKILL.md");
    let write_v = |n: &str| {
        std::fs::write(
            &skill,
            format!("---\nname: common-greet\ndescription: {n}\nshared: true\nnamespace: common\n---\n\n{n}\n"),
        )
        .unwrap();
    };
    let dr = c.dr();
    let push = |msg: &str| {
        let args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "push".to_string(),
            "--from".to_string(),
            user_clone.to_str().unwrap().to_string(),
            "--provider".to_string(),
            "manual".to_string(),
            "--message".to_string(),
            msg.to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        c.run(&ws, &refs)
    };

    write_v("v2");
    let (code, stdout, _) = push("v2");
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let branch = v["result"]["branch"].as_str().unwrap().to_string();

    // 模拟他人推了不同提交到该分支：在另一个克隆里改并强推
    let other = c.tmp.path().join("other-clone");
    ailoom::gitx::git(
        c.tmp.path(),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    )
    .unwrap();
    // 基于远端分支当前 tip（我们的 v2 提交）再造一个新提交 → 远端领先于本地
    ailoom::gitx::git(&other, &["fetch", "-q", "origin"]).unwrap();
    ailoom::gitx::git(
        &other,
        &["checkout", "-q", "-B", &branch, &format!("origin/{branch}")],
    )
    .unwrap();
    std::fs::write(
        other.join("resources/rules/other.md"),
        "---\nname: other-x\ndescription: x\nshared: true\nnamespace: common\n---\n\nx\n",
    )
    .unwrap();
    ailoom::gitx::git(&other, &["add", "--", "resources/rules/other.md"]).unwrap();
    ailoom::gitx::git(
        &other,
        &[
            "-c",
            "user.name=o",
            "-c",
            "user.email=o@o",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "other",
        ],
    )
    .unwrap();
    // 我们的 base 是 main 旧提交，他人的分支包含不同历史 → 普通推送被拒
    ailoom::gitx::git(&other, &["push", "-q", "origin", &branch]).unwrap();

    // 本地再次修改并推同分支（基于旧 base）→ 远端领先 → 非 FF 拒绝 → E8101
    write_v("v3-after-user-edit");
    let (code, _, stderr) = push("v3-after-user-edit");
    assert_ne!(code, 0);
    assert!(
        stderr.contains("E8101") || stderr.contains("分叉") || stderr.contains("rejected"),
        "{stderr}"
    );
}
