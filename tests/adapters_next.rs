//! 环境配置（AIL-031）、团队 Hook（AIL-032）、包依赖（AIL-033）集成测试。

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
    fn setup(&self) -> (PathBuf, PathBuf) {
        let bare = self.tmp.path().join("origin.git");
        ailoom::gitx::git_init(&bare, true).unwrap();
        let src = common::make_team_source(self.tmp.path());
        ailoom::gitx::git(&src, &["branch", "-M", "main"]).unwrap();
        ailoom::gitx::git(&src, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
        ailoom::gitx::git(&src, &["push", "-q", "-u", "origin", "main"]).unwrap();
        let ws = common::make_business_repo(self.tmp.path(), "biz");
        let url = bare.to_str().unwrap().to_string();
        let dr = self.dr();
        let args = [
            "--json".to_string(),
            "--data-root".to_string(),
            dr,
            "init".to_string(),
            "--url".to_string(),
            url,
            "--project".to_string(),
            "a".to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = self.run(&ws, &refs);
        assert_eq!(code, 0, "{stderr}");
        (bare, ws)
    }
    fn sync(&self, ws: &Path) -> (i32, String, String) {
        let dr = self.dr();
        let refs: Vec<&str> = vec!["--data-root", dr.as_str(), "sync"];
        self.run(ws, &refs)
    }
}

fn env_toml(with_secret: bool) -> String {
    let secret = if with_secret {
        "\n[secret_refs]\nTOKEN = \"$ENV:AILOOM_T\"\n"
    } else {
        ""
    };
    format!(
        "name = \"team-env\"\nshared = true\nnamespace = \"common\"\ntargets = [\"claude\"]\n\n[vars]\nAPI_BASE = \"https://x.internal\"\n{secret}"
    )
}

#[test]
fn env_literal_vars_deployed_secret_refs_rejected_without_plaintext() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/env")).unwrap();
    std::fs::write(src.join("resources/env/team-env.toml"), env_toml(true)).unwrap();
    common::commit_only(&src, "add env");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
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
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "{stderr}");

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    // 字面量部署；秘密引用不写明文
    assert_eq!(settings["env"]["API_BASE"], "https://x.internal");
    assert!(settings["env"].get("TOKEN").is_none(), "秘密引用不得写明文");
    // plan 中显式 unsupported
    let args = ["--json", "--data-root", &c.dr(), "plan"];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(v["result"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |a| a["action"] == "unsupported" && a["reason"].as_str().unwrap().contains("秘密引用")
        ));
}

#[test]
fn env_user_variable_not_overwritten_and_uninstall_removes_own() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    // 用户已有同名变量（不同值）→ 冲突保护
    std::fs::create_dir_all(ws.join(".claude")).unwrap();
    std::fs::write(
        ws.join(".claude/settings.json"),
        r#"{"env":{"API_BASE":"https://mine"}}"#,
    )
    .unwrap();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/env")).unwrap();
    std::fs::write(src.join("resources/env/team-env.toml"), env_toml(false)).unwrap();
    common::commit_only(&src, "add env");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["env"]["API_BASE"], "https://mine",
        "个人同名变量不静默覆盖"
    );
}

#[test]
fn culture_and_env_repeated_sync_no_duplicates() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/env")).unwrap();
    std::fs::write(src.join("resources/env/team-env.toml"), env_toml(false)).unwrap();
    // 文化文本 = 规则资源（受管片段）
    std::fs::write(
        src.join("resources/rules/team-culture.md"),
        "---\nname: team-culture\ndescription: 团队文化\nshared: true\nnamespace: common\n---\n\n- 坦诚沟通\n- 尊重评审\n",
    )
    .unwrap();
    common::commit_only(&src, "add culture and env");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let _ = c.sync(&ws);
    let _ = c.sync(&ws);
    let agents = std::fs::read_to_string(ws.join("AGENTS.md")).unwrap();
    assert_eq!(
        agents
            .matches("BEGIN AILOOM MANAGED: team/rule/common/team-culture")
            .count(),
        1,
        "文化重复同步无重复段落"
    );
}

#[test]
fn packages_check_reports_missing_and_install_gated() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/packages")).unwrap();
    std::fs::write(
        src.join("resources/packages/leftpad.toml"),
        "name = \"leftpad\"\necosystem = \"npm\"\nversion = \"1.3.0\"\nshared = true\nnamespace = \"common\"\n",
    )
    .unwrap();
    common::commit_only(&src, "add package");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
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

    // check：missing 报告
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "packages",
        "--action",
        "check",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let rows = v["result"]["packages"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["state"] == "missing"), "{v}");

    // install 无 --yes：拒绝（普通流程不擅自安装）
    let args = ["--data-root", &c.dr(), "packages", "--action", "install"];
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 2, "install 需显式 --yes");
    assert!(stderr.contains("--yes"), "{stderr}");
}

#[test]
fn team_hook_registered_and_user_hooks_preserved() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/hooks")).unwrap();
    std::fs::write(
        src.join("resources/hooks/team-stop.toml"),
        "name = \"team-stop\"\nevent = \"Stop\"\nmatcher = \"\"\ncommand = [\"echo\", \"team hook ran\"]\ntimeout_ms = 3000\nshared = true\nnamespace = \"common\"\ntargets = [\"claude\"]\n",
    )
    .unwrap();
    common::commit_only(&src, "add team hook");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    // 用户自己的 hook 在首次同步前就存在
    std::fs::create_dir_all(ws.join(".claude")).unwrap();
    std::fs::write(
        ws.join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"user-own"}]}]}}"#,
    )
    .unwrap();
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    let stop = settings["hooks"]["Stop"].as_array().unwrap();
    assert!(
        stop.iter().any(|e| e["hooks"][0]["command"] == "user-own"),
        "用户 hook 保留"
    );
    assert!(
        stop.iter().any(|e| e["hooks"][0]["command"]
            .as_str()
            .unwrap_or("")
            .starts_with("ailoom hooks exec --id")),
        "团队 hook 以 exec 包装注册"
    );
}
