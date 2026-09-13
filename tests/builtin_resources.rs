//! 内置资源（AIL-017）集成测试：部署/关闭/卸载/手动入口。

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

fn setup_ws(c: &Ctx) -> PathBuf {
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    let url = common::file_url(&src);
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
    let args = ["--data-root", &c.dr(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    ws
}

#[test]
fn builtins_deployed_with_sync() {
    let c = Ctx::new();
    let ws = setup_ws(&c);
    // 召回 Agent（Claude）
    let agent = ws.join(".claude/agents/ailoom-recall.md");
    assert!(agent.is_file(), "召回 Agent 未部署");
    let text = std::fs::read_to_string(&agent).unwrap();
    assert!(text.contains("name: ailoom-recall"));
    assert!(text.contains("ailoom recall"), "Agent 指导使用手动命令");
    assert!(text.contains("软约束"), "明确标注软约束");
    // 经验总结 Skill
    let skill = ws.join(".claude/skills/ailoom-share-learning/SKILL.md");
    assert!(skill.is_file());
    let skill_text = std::fs::read_to_string(&skill).unwrap();
    assert!(
        skill_text.contains("ailoom contribute --file"),
        "先成文件再走贡献命令"
    );
    assert!(
        skill_text.contains("不上传完整会话"),
        "明确不自动上传全文会话边界"
    );
    // Codex 手动入口片段
    let agents = std::fs::read_to_string(ws.join("AGENTS.md")).unwrap();
    assert!(agents.contains("ailoom recall"), "Codex 手动入口片段");
}

#[test]
fn no_builtin_flag_skips_deployment() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    let url = common::file_url(&src);
    let dr = c.dr();
    let args = [
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
        "--no-builtin".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "sync"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !ws.join(".claude/agents/ailoom-recall.md").exists(),
        "关闭后不部署内置 Agent"
    );
    assert!(!ws.join(".claude/skills/ailoom-share-learning").exists());
}

#[test]
fn uninstall_removes_builtins_too() {
    let c = Ctx::new();
    let ws = setup_ws(&c);
    let (code, _, stderr) = c.run(&ws, &["--data-root", &c.dr(), "uninstall", "--execute"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !ws.join(".claude/agents/ailoom-recall.md").exists(),
        "内置 Agent 随卸载移除"
    );
    assert!(!ws.join(".claude/skills/ailoom-share-learning").exists());
}

#[test]
fn recall_manual_entry_always_available() {
    let c = Ctx::new();
    let ws = setup_ws(&c);
    // 有匹配
    let (code, stdout, stderr) = c.run(
        &ws,
        &[
            "--json",
            "--data-root",
            &c.dr(),
            "recall",
            "--query",
            "缓存",
            "--limit",
            "3",
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(!v["result"]["results"].as_array().unwrap().is_empty());
    // 无匹配：明确空态，不报错
    let (code, stdout, _) = c.run(
        &ws,
        &[
            "--json",
            "--data-root",
            &c.dr(),
            "recall",
            "--query",
            "zzz-none xyz",
            "--limit",
            "3",
        ],
    );
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(v["result"]["results"].as_array().unwrap().is_empty());
}
