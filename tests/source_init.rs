//! 团队资源源脚手架（AIL-038）集成测试：
//! `ailoom source` 生成的骨架必须能直接被 `ailoom init --local-path` 绑定并同步。

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

/// 脚手架生成 → 自校验 → 本地源绑定 → 同步 → 示例资源真实部署。
#[test]
fn scaffold_then_bind_and_sync_end_to_end() {
    let c = Ctx::new();
    let src_dir = c.tmp.path().join("team-src");
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let dr = c.dr();

    // 1) 生成骨架（含示例资源 + git init）
    let (code, stdout, stderr) = c.run(
        c.tmp.path(),
        &[
            "--json",
            "--data-root",
            dr.as_str(),
            "source",
            "--dir",
            src_dir.to_str().unwrap(),
            "--team-id",
            "acme",
            "--project",
            "a",
            "--role",
            "dev",
            "--git",
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["team_id"], "acme");
    assert_eq!(v["result"]["git_inited"], true, "{v}");
    assert!(src_dir.join("ailoom.toml").is_file());
    assert!(src_dir
        .join("resources/skills/common-greet/SKILL.md")
        .is_file());
    assert!(src_dir.join("resources/rules/team-basics.md").is_file());
    assert!(src_dir.join(".git").is_dir(), "--git 应初始化仓库并提交");

    // 2) 直接以脚手架产物为本地源绑定业务仓
    let (code, _, stderr) = c.run(
        &ws,
        &[
            "--data-root",
            dr.as_str(),
            "init",
            "--local-path",
            "../team-src",
            "--project",
            "a",
        ],
    );
    assert_eq!(code, 0, "脚手架产物必须可绑定: {stderr}");

    // 3) 同步：示例技能/规则真实部署
    let (code, _, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "sync"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        ws.join(".claude/skills/common-greet/SKILL.md").is_file(),
        "示例技能应部署"
    );
    let agents = std::fs::read_to_string(ws.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("team/rule/common/team-basics"),
        "示例规则应以受管片段部署: {agents}"
    );
}

/// 防覆盖：已有 ailoom.toml 的目录默认拒绝；--force 合并；坏 team_id 拒绝。
#[test]
fn scaffold_rejects_overwrite_and_invalid_ids() {
    let c = Ctx::new();
    let src_dir = c.tmp.path().join("team-src");
    let dr = c.dr();
    let base = [
        "--json",
        "--data-root",
        dr.as_str(),
        "source",
        "--dir",
        src_dir.to_str().unwrap(),
        "--git",
    ];

    let (code, _, _) = c.run(c.tmp.path(), &base);
    assert_eq!(code, 0, "首次生成应成功");

    // 重复生成：拒绝覆盖
    let (code, _, stderr) = c.run(c.tmp.path(), &base);
    assert_ne!(code, 0, "已存在清单必须拒绝: {stderr}");
    assert!(stderr.contains("ailoom.toml"), "{stderr}");

    // --force 合并生成成功（示例文件已存在会被拒绝覆盖是预期，但清单保持可解析）
    let (code, _, stderr) = c.run(
        c.tmp.path(),
        &[
            "--json",
            "--data-root",
            c.dr().as_str(),
            "source",
            "--dir",
            src_dir.to_str().unwrap(),
            "--force",
        ],
    );
    assert_eq!(code, 0, "--force 应放行合并: {stderr}");

    // 非法 team_id
    let (code, _, stderr) = c.run(
        c.tmp.path(),
        &[
            "--json",
            "--data-root",
            c.dr().as_str(),
            "source",
            "--dir",
            c.tmp.path().join("t2").to_str().unwrap(),
            "--team-id",
            "Bad_Team!",
        ],
    );
    assert_ne!(code, 0, "非法 team_id 必须拒绝");
    assert!(stderr.contains("team_id"), "{stderr}");
}

/// --minimal：只生成清单骨架，不带示例资源。
#[test]
fn scaffold_minimal_has_no_examples() {
    let c = Ctx::new();
    let src_dir = c.tmp.path().join("minimal-src");
    let (code, stdout, stderr) = c.run(
        c.tmp.path(),
        &[
            "--json",
            "--data-root",
            c.dr().as_str(),
            "source",
            "--dir",
            src_dir.to_str().unwrap(),
            "--minimal",
            "--git",
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["resources_enumerated"], 0, "{v}");
    assert!(
        !src_dir.join("resources/skills/common-greet").exists(),
        "minimal 模式不带示例技能"
    );
    assert!(src_dir.join("resources/rules").is_dir(), "资源目录骨架仍在");
}
