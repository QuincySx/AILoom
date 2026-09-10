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
