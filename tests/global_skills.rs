//! AIL-152：全局 Skill —— 部署到用户级目录、与非 AILoom 条目共存、接管与还原、项目内不重复部署。
//! 全部经 CLI 子进程；HOME / XDG 指向临时目录，CLAUDE_CONFIG_DIR 显式清除。
mod common;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Ctx {
    tmp: tempfile::TempDir,
}

impl Ctx {
    fn new() -> Ctx {
        let c = Ctx {
            tmp: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(c.home()).unwrap();
        c
    }
    fn home(&self) -> PathBuf {
        self.tmp.path().join("home")
    }
    fn dr(&self) -> String {
        self.tmp.path().join("data").to_string_lossy().to_string()
    }
    fn run_env(&self, cwd: &Path, args: &[&str], extra: &[(&str, &Path)]) -> (i32, Value, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ailoom"));
        cmd.args(["--json", "--data-root", &self.dr()])
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .env_remove("CLAUDE_CONFIG_DIR");
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        (
            out.status.code().unwrap_or(-1),
            v["result"].clone(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn run(&self, cwd: &Path, args: &[&str]) -> (i32, Value, String) {
        self.run_env(cwd, args, &[])
    }
    fn ok(&self, args: &[&str]) -> Value {
        let (code, v, stderr) = self.run(self.tmp.path(), args);
        assert_eq!(code, 0, "{args:?}: {stderr}");
        v
    }
    fn import(&self, name: &str) {
        let src = self.tmp.path().join("skills").join(name);
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} skill\n---\nbody {name}\n"),
        )
        .unwrap();
        self.ok(&[
            "library",
            "--action",
            "import",
            "--dir",
            src.to_str().unwrap(),
            "--execute",
        ]);
    }
    fn enable(&self, name: &str) {
        self.ok(&[
            "global",
            "--action",
            "select",
            "--skill",
            &format!("personal/skill/personal/{name}"),
            "--state",
            "enable",
        ]);
    }
}

#[cfg(unix)]
#[test]
fn global_skills_deploy_to_user_dirs_and_coexist_with_foreign_entries() {
    let c = Ctx::new();
    c.import("solo");
    c.import("both");
    // CC Switch 风格：全局目录里已有一个同名链接
    let other = c.tmp.path().join("cc-switch/both");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("SKILL.md"), "---\nname: both\n---\nOTHER\n").unwrap();
    let claude = c.home().join(".claude/skills");
    let agents = c.home().join(".agents/skills");
    std::fs::create_dir_all(&claude).unwrap();
    std::os::unix::fs::symlink(&other, claude.join("both")).unwrap();

    c.enable("solo");
    c.enable("both");
    let status = c.ok(&["global", "--action", "status"]);
    let foreign = status["foreign"].as_array().unwrap();
    assert!(
        foreign
            .iter()
            .any(|f| f["name"] == "both" && f["conflicts_with"] == "personal/skill/personal/both"),
        "{status}"
    );
    let v = c.ok(&["global", "--action", "sync"]);
    assert_eq!(
        v["skipped_conflicts"],
        serde_json::json!([".claude/skills/both"]),
        "{v}"
    );
    // AILoom 自己部署的条目不算「非托管」，也不能被接管
    let status = c.ok(&["global", "--action", "status"]);
    let foreign: Vec<String> = status["foreign"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            format!(
                "{}/{}",
                f["target"].as_str().unwrap(),
                f["name"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(foreign, vec!["claude/both".to_string()], "{status}");
    let (code, _, stderr) = c.run(
        c.tmp.path(),
        &[
            "global", "--action", "takeover", "--target", "agents", "--name", "solo",
        ],
    );
    assert_eq!(code, 13, "托管条目拒绝接管: {stderr}");
    assert!(agents.join("solo/SKILL.md").is_file());
    for dir in [&claude, &agents] {
        assert!(dir.join("solo/SKILL.md").is_file(), "{}", dir.display());
    }
    assert!(agents.join("both/SKILL.md").is_file());
    assert_eq!(
        std::fs::read_link(claude.join("both")).unwrap(),
        other,
        "外部链接保留"
    );
    assert!(!c.home().join(".codex").exists(), "不写宿主全局配置");
    assert!(c.ok(&["global", "--action", "plan"])["actions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| matches!(a["action"].as_str(), Some("noop" | "conflict"))));

    // 接管：外部链接本身移入归档（指向的目录不动），之后由 AILoom 部署
    let v = c.ok(&[
        "global", "--action", "takeover", "--target", "claude", "--name", "both",
    ]);
    let archive_id = v["archived"]["id"].as_str().unwrap().to_string();
    assert!(other.join("SKILL.md").is_file(), "链接指向的内容不动");
    c.ok(&["global", "--action", "sync"]);
    let body = std::fs::read_to_string(claude.join("both/SKILL.md")).unwrap();
    assert!(body.contains("body both"), "{body}");

    // 原位置被占用时不能还原；停用并同步后可以还原
    let (code, _, stderr) = c.run(
        c.tmp.path(),
        &["global", "--action", "restore", "--id", &archive_id],
    );
    assert_ne!(code, 0, "{stderr}");
    c.ok(&[
        "global",
        "--action",
        "select",
        "--skill",
        "personal/skill/personal/both",
        "--state",
        "disable",
    ]);
    c.ok(&["global", "--action", "sync"]);
    assert!(claude.join("both").symlink_metadata().is_err() && !agents.join("both").exists());
    c.ok(&["global", "--action", "restore", "--id", &archive_id]);
    assert_eq!(std::fs::read_link(claude.join("both")).unwrap(), other);

    // 关闭一个目标：只清理该目录里 AILoom 的条目
    c.ok(&[
        "global", "--action", "select", "--target", "claude", "--state", "disable",
    ]);
    c.ok(&["global", "--action", "sync"]);
    assert!(claude.join("solo").symlink_metadata().is_err());
    assert!(agents.join("solo/SKILL.md").is_file());
    assert_eq!(
        std::fs::read_link(claude.join("both")).unwrap(),
        other,
        "外部条目始终不动"
    );
}

#[cfg(unix)]
#[test]
fn project_skips_skills_that_are_deployed_globally() {
    let c = Ctx::new();
    c.import("g");
    c.import("p");
    let repo = c.tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());
    let personal = |args: &[&str]| {
        let mut full = vec!["personal"];
        full.extend_from_slice(args);
        let (code, v, stderr) = c.run(&repo, &full);
        assert_eq!(code, 0, "{args:?}: {stderr}");
        v
    };
    for args in [
        [
            "--action", "select", "--host", "claude", "--state", "enable",
        ],
        ["--action", "select", "--host", "codex", "--state", "enable"],
        [
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/g",
            "--state",
            "enable",
        ],
        [
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/p",
            "--state",
            "enable",
        ],
    ] {
        personal(&args);
    }
    let root = repo.to_str().unwrap().to_string();
    personal(&["--action", "sync", "--root", &root]);
    assert!(repo.join(".claude/skills/g").exists() && repo.join(".agents/skills/g").exists());

    // 只是全局勾选、还没同步：项目照常（以实际全局部署为准）
    c.enable("g");
    let plan = personal(&["--action", "plan", "--root", &root]);
    assert!(
        plan["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["action"] == "noop"),
        "{plan}"
    );

    c.ok(&["global", "--action", "sync"]);
    let plan = personal(&["--action", "plan", "--root", &root]);
    assert!(
        plan["notes"]
            .to_string()
            .contains("personal/skill/personal/g"),
        "{plan}"
    );
    personal(&["--action", "sync", "--root", &root]);
    assert!(
        repo.join(".claude/skills/g").symlink_metadata().is_err(),
        "项目内不重复部署"
    );
    assert!(repo.join(".agents/skills/g").symlink_metadata().is_err());
    assert!(
        repo.join(".claude/skills/p").exists(),
        "其他 Skill 不受影响"
    );
    // 个人模式不生成 Codex 配置；生成时（团队模式）不能再列出已全局部署的 Skill
    if let Ok(codex) = std::fs::read_to_string(repo.join(".codex/config.toml")) {
        assert!(!codex.contains("skills/g/"), "{codex}");
    }
    let eff = personal(&["--action", "effective", "--root", &root]);
    assert_eq!(
        eff["global_skills"],
        serde_json::json!(["personal/skill/personal/g"]),
        "{eff}"
    );

    // 全局停用并同步后，项目恢复部署
    c.ok(&[
        "global",
        "--action",
        "select",
        "--skill",
        "personal/skill/personal/g",
        "--state",
        "disable",
    ]);
    c.ok(&["global", "--action", "sync"]);
    personal(&["--action", "sync", "--root", &root]);
    assert!(repo.join(".claude/skills/g/SKILL.md").is_file());
    assert!(repo.join(".agents/skills/g/SKILL.md").is_file());
}

#[test]
fn global_selection_is_validated_and_blocks_deleting_the_source() {
    let c = Ctx::new();
    c.import("kept");
    let (code, _, stderr) = c.run(
        c.tmp.path(),
        &[
            "global",
            "--action",
            "select",
            "--skill",
            "personal/skill/personal/nope",
            "--state",
            "enable",
        ],
    );
    assert_eq!(code, 12, "{stderr}");
    assert!(
        stderr.contains("E3004") && stderr.contains("\"fix\""),
        "{stderr}"
    );
    c.enable("kept");
    let (code, _, stderr) = c.run(
        c.tmp.path(),
        &[
            "library",
            "--action",
            "delete",
            "--skill",
            "kept",
            "--execute",
        ],
    );
    assert_ne!(code, 0, "全局启用中的资源不能删除: {stderr}");
    let preview = c.ok(&["library", "--action", "delete", "--skill", "kept"]);
    assert!(preview["preview"]["exists"] == true, "{preview}");
}

#[cfg(unix)]
#[test]
fn claude_config_dir_outside_home_is_not_written() {
    let c = Ctx::new();
    c.import("solo");
    c.enable("solo");
    let outside = c.tmp.path().join("elsewhere/claude");
    std::fs::create_dir_all(&outside).unwrap();
    let (code, v, stderr) = c.run_env(
        c.tmp.path(),
        &["global", "--action", "sync"],
        &[("CLAUDE_CONFIG_DIR", outside.as_path())],
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(v["notes"].to_string().contains("不在用户目录下"), "{v}");
    assert!(!outside.join("skills").exists(), "用户目录外不写");
    assert!(c.home().join(".agents/skills/solo/SKILL.md").is_file());
}

/// 浏览器验收发现：`CLAUDE_CONFIG_DIR` 在用户目录内但尚未创建（首次使用），且路径未规范化
/// （macOS 临时目录 /var → /private/var）时，曾被误判为「不在用户目录下」而不部署。
#[cfg(unix)]
#[test]
fn missing_claude_config_dir_inside_home_is_still_deployed() {
    let c = Ctx::new();
    c.import("solo");
    c.enable("solo");
    let config = c.home().join("custom-claude");
    assert!(!config.exists());
    let (code, v, stderr) = c.run_env(
        c.tmp.path(),
        &["global", "--action", "sync"],
        &[("CLAUDE_CONFIG_DIR", config.as_path())],
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(config.join("skills/solo/SKILL.md").is_file(), "{v}");
}

/// 用户反馈：非托管条目只用一个「→」看不清链接本身在哪、真实内容在哪。
/// 状态要给出：是不是链接、沿链接解析到底的真实目录、链接套链接时中间经过的链接、失效链接。
#[cfg(unix)]
#[test]
fn foreign_entries_report_real_location_through_link_chains() {
    let c = Ctx::new();
    let real = c.tmp.path().join("cc-switch/skills/kb");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("SKILL.md"), "---\nname: kb\n---\nx\n").unwrap();
    let claude = c.home().join(".claude/skills");
    let agents = c.home().join(".agents/skills");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::create_dir_all(agents.join("plain")).unwrap();
    std::os::unix::fs::symlink(&real, claude.join("kb")).unwrap();
    // .agents → .claude → 真实目录（两跳）
    std::os::unix::fs::symlink(claude.join("kb"), agents.join("kb")).unwrap();
    std::os::unix::fs::symlink(c.tmp.path().join("gone"), agents.join("dead")).unwrap();
    let status = c.ok(&["global", "--action", "status"]);
    let find = |target: &str, name: &str| -> Value {
        status["foreign"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["target"] == target && f["name"] == name)
            .unwrap_or_else(|| panic!("{target}/{name}: {status}"))["location"]
            .clone()
    };
    let canon = |p: &Path| p.canonicalize().unwrap().to_string_lossy().to_string();
    let kb = find("agents", "kb");
    assert_eq!(kb["kind"], "link");
    assert_eq!(kb["real_path"], canon(&real).as_str(), "{kb}");
    assert_eq!(
        kb["via"].as_array().unwrap().len(),
        1,
        "中间经过 .claude 那一跳: {kb}"
    );
    let direct = find("claude", "kb");
    assert_eq!(direct["real_path"], canon(&real).as_str());
    assert_eq!(direct["via"], serde_json::json!([]));
    assert_eq!(find("agents", "plain")["kind"], "dir");
    assert_eq!(find("agents", "dead")["kind"], "broken_link");
}
