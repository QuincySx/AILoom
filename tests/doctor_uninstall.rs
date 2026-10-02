//! doctor 与安全卸载（AIL-013）集成测试。

mod common;

use std::path::Path;

fn run(c: &common_test::Ctx, cwd: &Path, args: &[&str]) -> (i32, String, String) {
    c.run(cwd, args)
}

// 复用 adapters.rs 的 Ctx 结构（简单复制，避免跨测试文件耦合）
mod common_test {
    use std::path::Path;
    use std::process::Command;

    pub struct Ctx {
        pub tmp: tempfile::TempDir,
    }

    impl Ctx {
        pub fn new() -> Ctx {
            Ctx {
                tmp: tempfile::tempdir().unwrap(),
            }
        }
        pub fn run(&self, cwd: &Path, args: &[&str]) -> (i32, String, String) {
            let bin = {
                let mut path = std::env::current_exe().unwrap();
                loop {
                    path.pop();
                    let p = if cfg!(windows) {
                        "ailoom.exe"
                    } else {
                        "ailoom"
                    };
                    if path.join(p).exists() {
                        break path.join(p);
                    }
                }
            };
            let out = Command::new(bin)
                .args(args)
                .current_dir(cwd)
                .envs(crate::common::isolated_child_env(self.tmp.path()))
                .output()
                .unwrap();
            (
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        }
        pub fn dr(&self) -> String {
            self.tmp.path().join("data").to_string_lossy().to_string()
        }
    }
}

fn setup_ws(c: &common_test::Ctx) -> PathBuf {
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    let url = common::file_url(&src);
    let dr = c.dr();
    let args = vec![
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
        "--role".to_string(),
        "dev".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = run(c, &ws, &refs);
    assert_eq!(code, 0, "init: {stderr}");
    let dr = c.dr();
    let (code, _, stderr) = run(c, &ws, &["--data-root", dr.as_str(), "sync"]);
    assert_eq!(code, 0, "sync: {stderr}");
    ws
}

use std::path::PathBuf;

fn tree_snapshot(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            out.push((
                entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                ailoom::ids::sha256_hex(&std::fs::read(entry.path()).unwrap_or_default()),
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn doctor_checks_pass_and_modify_nothing() {
    let c = common_test::Ctx::new();
    let ws = setup_ws(&c);
    let before = tree_snapshot(&ws);
    let (code, stdout, stderr) = run(&c, &ws, &["--json", "--data-root", &c.dr(), "doctor"]);
    assert_eq!(code, 0, "{stderr}");
    let after = tree_snapshot(&ws);
    assert_eq!(before, after, "doctor 不修改任何状态");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["ok"], true, "checks: {}", v["result"]["checks"]);
    assert!(v["result"]["checks"].as_array().unwrap().len() >= 5);
}

#[test]
fn doctor_treats_personal_only_repo_as_healthy() {
    // 盲测回归：没有团队声明是纯个人模式的正常状态，不应判为问题、也不应要求 init
    let c = common_test::Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let (code, stdout, stderr) = run(&c, &ws, &["--json", "--data-root", &c.dr(), "doctor"]);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["ok"], true, "{v}");
    let checks = v["result"]["checks"].as_array().unwrap();
    let decl = checks.iter().find(|c| c["check"] == "declaration").unwrap();
    assert_eq!(decl["detail"]["mode"], "personal", "{decl}");
    assert!(
        !checks
            .iter()
            .any(|c| c["check"] == "binding" || c["check"] == "source-lock"),
        "个人模式不报团队绑定/源锁: {v}"
    );
}

#[test]
fn doctor_reports_corrupt_files_instead_of_failing() {
    let c = common_test::Ctx::new();
    let ws = setup_ws(&c);
    let lock = ws.join(".ailoom/machine/sources.lock.json");
    std::fs::write(&lock, "{broken").unwrap();
    let managed = walkdir::WalkDir::new(c.tmp.path().join("data"))
        .into_iter()
        .flatten()
        .find(|e| e.file_name() == "managed-manifest.json")
        .unwrap()
        .into_path();
    std::fs::write(&managed, "{broken").unwrap();
    let (code, stdout, stderr) = run(&c, &ws, &["--json", "--data-root", &c.dr(), "doctor"]);
    assert_eq!(code, 0, "体检本身不因损坏文件退出: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["ok"], false);
    for name in ["source-lock", "managed-manifest"] {
        let check = v["result"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["check"] == name)
            .unwrap_or_else(|| panic!("缺少 {name}: {v}"));
        assert_eq!(check["ok"], false, "{check}");
        assert!(check["fix"].is_string(), "{check}");
    }
    let (code, _, _) = run(
        &c,
        &ws,
        &["--json", "--data-root", &c.dr(), "doctor", "--strict"],
    );
    assert_ne!(code, 0, "--strict 按问题项返回非 0");
}

#[test]
fn uninstall_preview_then_execute_and_idempotent() {
    let c = common_test::Ctx::new();
    let ws = setup_ws(&c);
    assert!(ws.join(".claude/skills/common-greet/SKILL.md").is_file());

    // 预览：不删除
    let (code, stdout, _) = run(&c, &ws, &["--json", "--data-root", &c.dr(), "uninstall"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["mode"], "preview");
    assert!(
        ws.join(".claude/skills/common-greet/SKILL.md").is_file(),
        "预览不删除"
    );

    // 执行
    let (code, stdout, stderr) = run(
        &c,
        &ws,
        &["--json", "--data-root", &c.dr(), "uninstall", "--execute"],
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["ok"], true);
    assert!(
        !ws.join(".claude/skills/common-greet").exists(),
        "托管技能被移除"
    );
    assert!(
        !ws.join("AGENTS.md").exists()
            || !std::fs::read_to_string(ws.join("AGENTS.md"))
                .unwrap()
                .contains("AILOOM"),
        "片段被清理"
    );
    assert!(ws.join(".ailoom/project.toml").exists(), "用户声明保留");
    assert!(
        ws.join("app.txt").exists() || ws.join("README.md").exists(),
        "用户文件保留"
    );

    // 幂等：再次执行 OK 且零操作
    let (code, stdout, stderr) = run(
        &c,
        &ws,
        &["--json", "--data-root", &c.dr(), "uninstall", "--execute"],
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["ok"], true);
    assert!(v["result"]["removed"].as_array().unwrap().is_empty());
}

#[test]
fn uninstall_keeps_user_modified_targets() {
    let c = common_test::Ctx::new();
    let ws = setup_ws(&c);
    let target = ws.join(".claude/skills/common-greet/SKILL.md");
    std::fs::write(&target, "用户自己改过").unwrap();

    let (code, stdout, stderr) = run(
        &c,
        &ws,
        &["--json", "--data-root", &c.dr(), "uninstall", "--execute"],
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        !v["result"]["kept_conflicts"].as_array().unwrap().is_empty(),
        "{v}"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "用户自己改过",
        "手改内容保留"
    );
}

#[test]
fn uninstall_other_workspace_unaffected() {
    let c = common_test::Ctx::new();
    let ws1 = setup_ws(&c);
    // 第二个 checkout 同步
    let biz_origin = c.tmp.path().join("biz-origin");
    let _ = biz_origin;
    let ws2 = c.tmp.path().join("ws2");
    std::fs::create_dir_all(&ws2).unwrap();
    common::clone(&ws1, &ws2).unwrap();
    let src = c.tmp.path().join("src/team-src");
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
    let (code, _, stderr) = run(&c, &ws2, &refs);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = run(&c, &ws2, &["--data-root", &c.dr(), "sync"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(ws2.join(".claude/skills/common-greet/SKILL.md").is_file());

    // 卸载 ws1
    let (code, _, stderr) = run(
        &c,
        &ws1,
        &["--data-root", &c.dr(), "uninstall", "--execute"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(!ws1.join(".claude/skills/common-greet").exists());
    assert!(
        ws2.join(".claude/skills/common-greet/SKILL.md").is_file(),
        "其它工作区不受影响"
    );
}

#[test]
fn corrupted_managed_manifest_refuses_guess_delete() {
    let c = common_test::Ctx::new();
    let ws = setup_ws(&c);
    // 找到本工作区 manifest 并损坏
    let ws_root = c.tmp.path().join("data").join("ws");
    let mut corrupted = false;
    for entry in std::fs::read_dir(&ws_root).unwrap().flatten() {
        let mp = entry.path().join("managed-manifest.json");
        if mp.is_file() {
            std::fs::write(&mp, "{ corrupt").unwrap();
            corrupted = true;
        }
    }
    assert!(corrupted);
    let (code, _, stderr) = run(&c, &ws, &["--data-root", &c.dr(), "uninstall", "--execute"]);
    assert_ne!(code, 0, "损坏清单必须拒绝");
    assert!(
        stderr.contains("E9000") || stderr.contains("损坏"),
        "{stderr}"
    );
    assert!(
        ws.join(".claude/skills/common-greet/SKILL.md").is_file(),
        "不得猜删"
    );
}
