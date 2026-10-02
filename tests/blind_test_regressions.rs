//! 2026-10-02 盲测缺陷：嵌套 Git 边界与跨宿主文本片段同步幂等。
mod common;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    fn data(&self) -> PathBuf {
        self.tmp.path().join("data")
    }

    fn run(&self, root: &Path, args: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_ailoom"))
            .args(["--json", "--data-root"])
            .arg(self.data())
            .args(args)
            .arg("--root")
            .arg(root)
            .current_dir(root)
            .env("XDG_DATA_HOME", self.tmp.path().join("xdg-data"))
            .env("XDG_STATE_HOME", self.tmp.path().join("xdg-state"))
            .env("CLAUDE_CONFIG_DIR", self.tmp.path().join("claude"))
            .env("PI_CODING_AGENT_DIR", self.tmp.path().join("pi"))
            .env("AILOOM_REPORTING", "off")
            .env("AILOOM_AUTO_SYNC", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        value["result"].clone()
    }

    fn select(&self, root: &Path, kind: &str, value: &str, state: &str) {
        self.run(
            root,
            &[
                "personal", "--action", "select", kind, value, "--state", state,
            ],
        );
    }

    fn rule(&self) -> String {
        ailoom::personal_library::create_definition(
            &self.data(),
            "rule",
            "chinese",
            "中文规则",
            "规则 V1。\n\n",
        )
        .unwrap()
    }
}

#[test]
fn nested_repository_plan_sync_and_undo_stay_inside_child() {
    let f = Fixture::new();
    let parent = common::make_business_repo(f.tmp.path(), "parent");
    let source = common::make_team_source(&f.tmp.path().join("source"));
    f.run(
        &parent,
        &[
            "init",
            "--url",
            source.to_str().unwrap(),
            "--project",
            "a",
            "--target",
            "claude",
        ],
    );
    let declaration = parent.join(".ailoom/project.toml");
    let parent_binding = std::fs::read(&declaration).unwrap();
    let child = common::make_business_repo(&parent, "child");
    let rule = f.rule();
    f.select(&child, "--host", "grok", "enable");
    f.select(&child, "--resource", &rule, "enable");
    let plan = f.run(&child, &["personal", "--action", "plan"]);
    assert_eq!(
        plan["scope_dir"],
        child.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(
        plan["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["resource_id"].as_str().unwrap().starts_with("personal/")),
        "父团队资源不得进入子仓库: {plan}"
    );
    let synced = f.run(&child, &["personal", "--action", "sync"]);
    assert_eq!(synced["ok"], true);
    assert!(child.join(".grok/rules/ailoom-personal.md").is_file());
    assert!(!parent.join(".grok/rules/ailoom-personal.md").exists());
    assert!(!parent.join(".claude/skills/common-greet").exists());
    assert_eq!(std::fs::read(&declaration).unwrap(), parent_binding);
    f.run(
        &child,
        &[
            "personal",
            "--action",
            "undo",
            "--id",
            synced["job_id"].as_str().unwrap(),
        ],
    );
    assert!(!child.join(".grok/rules/ailoom-personal.md").exists());
    assert_eq!(std::fs::read(&declaration).unwrap(), parent_binding);
}

#[test]
fn rules_and_instructions_repeat_update_undo_and_protect_user_edits() {
    for (host, target) in [
        ("grok", ".grok/rules/ailoom-personal.md"),
        ("pi", ".pi/APPEND_SYSTEM.md"),
        ("opencode", "AGENTS.md"),
        ("codex", "AGENTS.md"),
    ] {
        let f = Fixture::new();
        let root = common::make_business_repo(f.tmp.path(), host);
        let rule = f.rule();
        f.select(&root, "--host", host, "enable");
        f.select(&root, "--resource", &rule, "enable");
        let instructions = f.tmp.path().join("instructions.md");
        std::fs::write(&instructions, "个人说明。\n\n").unwrap();
        // 三个失败宿主测试 Rules + 说明组合；Codex 对齐原盲测的 Rules 单独对照。
        if host != "codex" {
            f.run(
                &root,
                &[
                    "personal",
                    "--action",
                    "instructions",
                    "--file",
                    instructions.to_str().unwrap(),
                    "--worktree",
                ],
            );
        }
        let synced = f.run(&root, &["personal", "--action", "sync"]);
        assert_eq!(synced["ok"], true, "{host}: {synced}");
        assert!(synced["skipped_conflicts"].as_array().unwrap().is_empty());
        let repeat = f.run(&root, &["personal", "--action", "plan"]);
        let actions = repeat["actions"].as_array().unwrap();
        assert!(!actions.is_empty());
        assert!(
            actions.iter().all(|a| a["action"] == "noop"),
            "{host}: {repeat}"
        );
        let file = root.join(target);
        let v1 = std::fs::read_to_string(&file).unwrap();
        if host != "codex" {
            assert!(v1.contains("个人说明"));
        }
        let definition = f.data().join("library/resources/rules/chinese.md");
        let original = std::fs::read_to_string(&definition).unwrap();
        std::fs::write(&definition, original.replace("规则 V1", "规则 V2")).unwrap();
        let update = f.run(&root, &["personal", "--action", "sync"]);
        assert!(
            update["skipped_conflicts"].as_array().unwrap().is_empty(),
            "{host}: {update}"
        );
        assert!(std::fs::read_to_string(&file).unwrap().contains("规则 V2"));
        f.run(
            &root,
            &[
                "personal",
                "--action",
                "undo",
                "--id",
                update["job_id"].as_str().unwrap(),
            ],
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            v1,
            "{host}: undo 恢复完整片段"
        );
        std::fs::write(&definition, original).unwrap();
        f.run(&root, &["personal", "--action", "sync"]);
        let edited = v1.replace("规则 V1", "用户本机编辑");
        std::fs::write(&file, &edited).unwrap();
        let conflict = f.run(&root, &["personal", "--action", "sync"]);
        assert!(
            conflict["skipped_conflicts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == target),
            "{host}: {conflict}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            edited,
            "{host}: 本机修改必须保留"
        );
    }
}
