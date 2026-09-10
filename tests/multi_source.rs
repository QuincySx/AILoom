//! 多源订阅、标签与来源锁（AIL-025）集成测试。

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

/// 主源 + 带标签的额外源（common-rs 技能 tag=rust、rule 无 tag）。
fn setup_two_sources(c: &Ctx) -> (PathBuf, PathBuf, PathBuf) {
    let bare1 = c.tmp.path().join("origin1.git");
    ailoom::gitx::git_init(&bare1, true).unwrap();
    let src1 = common::make_team_source(c.tmp.path());
    ailoom::gitx::git(&src1, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(&src1, &["remote", "add", "origin", bare1.to_str().unwrap()]).unwrap();
    ailoom::gitx::git(&src1, &["push", "-q", "-u", "origin", "main"]).unwrap();

    let bare2 = c.tmp.path().join("origin2.git");
    ailoom::gitx::git_init(&bare2, true).unwrap();
    let src2 = c.tmp.path().join("extra-src");
    std::fs::create_dir_all(src2.join("resources/skills/common-rust")).unwrap();
    std::fs::create_dir_all(src2.join("resources/rules")).unwrap();
    std::fs::write(
        src2.join("ailoom.toml"),
        "schema_version = 1\nteam_id = \"extra\"\n[projects.a]\nname = \"A\"\n[namespaces]\nknown = [\"common\"]\nshared = [\"common\"]\n",
    )
    .unwrap();
    std::fs::write(
        src2.join("resources/skills/common-rust/SKILL.md"),
        "---\nname: common-rust\ndescription: Rust 技能\nshared: true\nnamespace: common\ntags: [rust]\n---\n\nRust 规范\n",
    )
    .unwrap();
    std::fs::write(
        src2.join("resources/rules/no-tag-rule.md"),
        "---\nname: no-tag-rule\ndescription: 无标签规则\nshared: true\nnamespace: common\n---\n\n内容\n",
    )
    .unwrap();
    ailoom::gitx::git_init(&src2, false).unwrap();
    ailoom::gitx::git_commit_all(&src2, "init extra", &["ailoom.toml", "resources"]).unwrap();
    ailoom::gitx::git(&src2, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(&src2, &["remote", "add", "origin", bare2.to_str().unwrap()]).unwrap();
    ailoom::gitx::git(&src2, &["push", "-q", "-u", "origin", "main"]).unwrap();

    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url1 = bare1.to_str().unwrap().to_string();
    let dr = c.dr();
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url1,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    (ws, src1, src2)
}

fn add_extra_source(ws: &Path, es: &str) {
    let p = ws.join(".ailoom/project.toml");
    let mut text = std::fs::read_to_string(&p).unwrap();
    text.push_str(&format!("\n[[extra_sources]]\nname = \"extra\"\ntype = \"git\"\nurl = \"{es}\"\nref = \"main\"\nprojects = [\"a\"]\nroles = []\ntags = [\"rust\"]\n"));
    std::fs::write(&p, text).unwrap();
}

fn sync(c: &Ctx, ws: &Path) -> (i32, String, String) {
    let args = ["--data-root", &c.dr(), "sync"];
    let refs: Vec<&str> = args.to_vec();
    c.run(ws, &refs)
}

#[test]
fn tags_subscription_selects_only_matching_and_conflicts_detected() {
    let c = Ctx::new();
    let (ws, src1, src2) = setup_two_sources(&c);
    let url2 = src2.to_str().unwrap().to_string();

    // 订阅 rust 标签
    add_extra_source(&ws, &url2);
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        src1.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "init 校验额外源: {stderr}");
    let (code, _, stderr) = sync(&c, &ws);
    assert_eq!(code, 0, "{stderr}");

    // tags 订阅：rust 技能入选，无标签规则排除
    assert!(
        ws.join(".claude/skills/common-rust/SKILL.md").is_file(),
        "tag 命中资源部署"
    );
    assert!(
        !ws.join(".claude/rules/no-tag-rule.md").exists(),
        "无标签资源被订阅过滤"
    );
    // 主源资源不受影响
    assert!(ws.join(".claude/skills/common-greet/SKILL.md").is_file());

    // 跨源同名冲突：额外源追加与主源同名的技能 → E3006
    std::fs::create_dir_all(src2.join("resources/skills/common-greet")).unwrap();
    std::fs::write(
        src2.join("resources/skills/common-greet/SKILL.md"),
        "---\nname: common-greet\ndescription: 同名\nshared: true\nnamespace: common\ntags: [rust]\n---\n\n冲突\n",
    )
    .unwrap();
    common::commit_only(&src2, "conflict skill");
    ailoom::gitx::git(&src2, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        src1.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let (code, _, stderr) = sync(&c, &ws);
    assert_eq!(code, 12, "跨源目标冲突退出 12");
    assert!(stderr.contains("E3006"), "{stderr}");
}

#[test]
fn duplicate_identity_rejected_at_init() {
    let c = Ctx::new();
    let (ws, src1, _src2) = setup_two_sources(&c);
    // 额外源与主源同一 URL（循环/重复订阅）
    add_extra_source(&ws, src1.to_str().unwrap());
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        src1.to_str().unwrap().to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 11, "重复订阅必须拒绝（源/Git 类）");
    assert!(stderr.contains("E2006"), "{stderr}");
}

#[test]
fn extra_source_offline_keeps_locked_version_primary_unaffected() {
    let c = Ctx::new();
    let (ws, src1, src2) = setup_two_sources(&c);
    add_extra_source(&ws, src2.to_str().unwrap());
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        src1.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = sync(&c, &ws);
    assert_eq!(code, 0, "{stderr}");

    // 额外源远端消失：锁仍在，离线复用快照，sync 正常
    let gone = c.tmp.path().join("origin2-moved.git");
    std::fs::rename(&src2, &gone).unwrap(); // src2 是 origin2 的工作仓（非裸）：fetch 失败但有锁
    let args = ["--data-root", &c.dr(), "sync"];
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "额外源离线：已锁快照继续可用 {stderr}");
    assert!(ws.join(".claude/skills/common-rust/SKILL.md").is_file());
    // 主源资源不受影响
    assert!(ws.join(".claude/skills/common-greet/SKILL.md").is_file());
}
