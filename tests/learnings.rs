//! 经验文档与项目归属（AIL-015）集成测试。

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
    /// 本地裸远端 + 团队源；绑定 ws 到该源。
    fn setup(&self, with_projects: &[&str]) -> (PathBuf, PathBuf) {
        let bare = self.tmp.path().join("origin.git");
        ailoom::gitx::git_init(&bare, true).unwrap();
        let team_src = common::make_team_source(self.tmp.path());
        ailoom::gitx::git(&team_src, &["branch", "-M", "main"]).unwrap();
        ailoom::gitx::git(
            &team_src,
            &["remote", "add", "origin", bare.to_str().unwrap()],
        )
        .unwrap();
        ailoom::gitx::git(&team_src, &["push", "-q", "-u", "origin", "main"]).unwrap();
        let ws = common::make_business_repo(self.tmp.path(), "biz");
        let url = bare.to_str().unwrap().to_string();
        let dr = self.dr();
        let mut args: Vec<String> =
            vec!["--data-root".into(), dr, "init".into(), "--url".into(), url];
        for p in with_projects {
            args.push("--project".into());
            args.push(p.to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = self.run(&ws, &refs);
        assert_eq!(code, 0, "{stderr}");
        (bare, ws)
    }
    fn contribute(&self, ws: &Path, learning: &Path, extra: &[&str]) -> (i32, String, String) {
        let dr = self.dr();
        let mut args: Vec<String> = vec![
            "--json".into(),
            "--data-root".into(),
            dr,
            "contribute".into(),
            "--file".into(),
            learning.to_str().unwrap().to_string(),
            "--provider".into(),
            "manual".into(),
        ];
        args.extend(extra.iter().map(|s| s.to_string()));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(ws, &refs)
    }
}

fn write_learning(path: &Path, title: &str, body: &str) {
    std::fs::write(
        path,
        format!("---\ntitle: {title}\ndescription: {title}\n---\n\n{body}\n"),
    )
    .unwrap();
}

#[test]
fn single_project_binding_defaults_to_it_and_marks_project() {
    let c = Ctx::new();
    let (bare, ws) = c.setup(&["a"]);
    let draft = c.tmp.path().join("draft.md");
    write_learning(&draft, "A 项目经验", "缓存预热脚本缺失导致事故");

    let (code, stdout, stderr) = c.contribute(&ws, &draft, &[]);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let branch = v["result"]["branch"].as_str().unwrap().to_string();

    // 远端分支文件带 project: a
    ailoom::gitx::git(&team_src_dir(&c), &["fetch", "-q", "origin"]).unwrap();
    let id = v["result"]["learning_id"].as_str().unwrap();
    let content = ailoom::gitx::git(
        &team_src_dir(&c),
        &[
            "show",
            &format!("origin/{branch}:resources/learnings/{id}.md"),
        ],
    )
    .unwrap();
    assert!(content.contains("project: a"), "{content}");
    assert!(content.contains("shared: false"), "{content}");
    assert!(!content.contains("project: b"));
    let _ = bare;
}

fn team_src_dir(c: &Ctx) -> PathBuf {
    c.tmp.path().join("team-src")
}

#[test]
fn explicit_shared_required_and_respected() {
    let c = Ctx::new();
    let (_bare, ws) = c.setup(&["a"]);
    let draft = c.tmp.path().join("draft.md");
    write_learning(&draft, "全员经验", "值班表同步");

    // 不带 --shared：单项目绑定默认归入项目（非共享）
    let (code, stdout, _) = c.contribute(&ws, &draft, &[]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(!v["result"]["target"]
        .as_str()
        .unwrap()
        .contains("a-postmortem"));

    // 显式 --shared 才共享
    let draft2 = c.tmp.path().join("draft2.md");
    write_learning(&draft2, "全员发布经验", "发布前核对值班表");
    let (code, stdout, stderr) = c.contribute(&ws, &draft2, &["--shared"]);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let branch = v["result"]["branch"].as_str().unwrap().to_string();
    let id = v["result"]["learning_id"].as_str().unwrap();
    ailoom::gitx::git(&team_src_dir(&c), &["fetch", "-q", "origin"]).unwrap();
    let content = ailoom::gitx::git(
        &team_src_dir(&c),
        &[
            "show",
            &format!("origin/{branch}:resources/learnings/{id}.md"),
        ],
    )
    .unwrap();
    assert!(content.contains("shared: true"), "{content}");
}

#[test]
fn multi_project_binding_requires_explicit_target() {
    let c = Ctx::new();
    let (_bare, ws) = c.setup(&["a", "b"]);
    let draft = c.tmp.path().join("draft.md");
    write_learning(&draft, "两项目经验", "内容");

    let (code, _, stderr) = c.contribute(&ws, &draft, &[]);
    assert_eq!(code, 15, "多项目未指定目标必须失败");
    let e: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(e["code"], "E6002");
    assert!(e["message"].as_str().unwrap().contains("显式"), "{e}");

    // 显式 --project b 可成功
    let (code, _, stderr) = c.contribute(&ws, &draft, &["--project", "b"]);
    assert_eq!(code, 0, "{stderr}");
}

#[test]
fn retry_same_learning_does_not_duplicate() {
    let c = Ctx::new();
    let (_bare, ws) = c.setup(&["a"]);
    let draft = c.tmp.path().join("draft.md");
    write_learning(&draft, "重试经验", "同样内容");

    let (code, stdout, _) = c.contribute(&ws, &draft, &[]);
    assert_eq!(code, 0);
    let v1: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let id = v1["result"]["learning_id"].as_str().unwrap().to_string();

    let (code, stdout, stderr) = c.contribute(&ws, &draft, &[]);
    assert_eq!(code, 0, "{stderr}");
    let v2: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v2["result"]["learning_id"].as_str().unwrap(), id, "稳定 ID");
    assert_eq!(v2["result"]["deduplicated"], true, "重试不重复创建");
}

#[test]
fn empty_body_rejected_and_draft_preserved() {
    let c = Ctx::new();
    let (_bare, ws) = c.setup(&["a"]);
    let draft = c.tmp.path().join("draft.md");
    std::fs::write(&draft, "---\ntitle: 空经验\n---\n\n   \n").unwrap();
    let before = std::fs::read_to_string(&draft).unwrap();

    let (code, _, stderr) = c.contribute(&ws, &draft, &[]);
    assert_eq!(code, 15);
    let e: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(e["code"], "E6002");
    assert_eq!(
        std::fs::read_to_string(&draft).unwrap(),
        before,
        "提交失败保留草稿"
    );
}

#[test]
fn unknown_namespace_rejected() {
    let c = Ctx::new();
    let (_bare, ws) = c.setup(&["a"]);
    let draft = c.tmp.path().join("draft.md");
    write_learning(&draft, "命名空间测试", "内容");
    let (code, _, stderr) = c.contribute(&ws, &draft, &["--namespace", "ghost", "--project", "a"]);
    assert_eq!(code, 12);
    assert!(stderr.contains("E3004"), "{stderr}");
}
