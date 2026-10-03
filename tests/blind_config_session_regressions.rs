//! 独立盲测后的配置所有权、源替换与 transcript 重导回归。
mod common;

use serde_json::{json, Value};
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
    fn run(&self, ws: &Path, args: &[&str]) -> Value {
        let output = self.execute(ws, args);
        assert!(
            output.status.success(),
            "{args:?}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"].clone()
    }
    fn execute(&self, ws: &Path, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_ailoom"))
            .args(["--json", "--data-root"])
            .arg(self.tmp.path().join("data"))
            .args(args)
            .arg("--root")
            .arg(ws)
            .current_dir(ws)
            .env("XDG_DATA_HOME", self.tmp.path().join("xdg-data"))
            .env("XDG_STATE_HOME", self.tmp.path().join("xdg-state"))
            .env("CLAUDE_CONFIG_DIR", self.tmp.path().join("claude"))
            .env("AILOOM_AUTO_SYNC", "0")
            .env("AILOOM_REPORTING", "off")
            .output()
            .unwrap()
    }
    fn setup(&self) -> (PathBuf, PathBuf) {
        let ws = common::make_business_repo(self.tmp.path(), "biz");
        let source = common::make_team_source_full(&self.tmp.path().join("source"));
        std::fs::create_dir_all(ws.join(".codex")).unwrap();
        std::fs::write(ws.join(".codex/config.toml"), "model = 'user-model'\n[skills]\nuser_setting = 'keep'\n[[skills.config]]\npath = '/user/skill/SKILL.md'\nenabled = false\n").unwrap();
        self.run(
            &ws,
            &[
                "init",
                "--local-path",
                "../source/team-src",
                "--project",
                "a",
                "--role",
                "dev",
                "--target",
                "codex",
                "--no-builtin",
            ],
        );
        self.run(&ws, &["sync"]);
        (ws, source)
    }
    fn manifest(&self, ws: &Path) -> PathBuf {
        self.tmp
            .path()
            .join("data/ws")
            .join(ailoom::ids::workspace_id_from_root(
                &ws.canonicalize().unwrap(),
            ))
            .join("managed-manifest.json")
    }
}

#[test]
fn duplicate_skill_config_identity_protects_all_user_entries() {
    let f = Fixture::new();
    let (ws, _) = f.setup();
    let path = ws.join(".codex/config.toml");
    let mut config = std::fs::read_to_string(&path).unwrap();
    config.push_str("\n[[skills.config]]\npath = '.agents/skills/common-greet/SKILL.md'\nenabled = false\nuser_note = 'preserve duplicate'\n");
    std::fs::write(&path, &config).unwrap();
    for args in [vec!["plan"], vec!["uninstall", "--execute"]] {
        let output = f.execute(&ws, &args);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("重复"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), config);
        assert!(ws.join(".agents/skills/common-greet/SKILL.md").exists());
    }
    assert_eq!(f.run(&ws, &["doctor"])["ok"], false);
}

#[test]
fn codex_skill_and_mcp_ownership_survives_noop_and_uninstall() {
    let f = Fixture::new();
    let (ws, _) = f.setup();
    let config = ws.join(".codex/config.toml");
    let before = std::fs::read(&config).unwrap();
    let doctor = f.run(&ws, &["doctor"]);
    assert_eq!(doctor["ok"], true, "{doctor}");
    let plan = f.run(&ws, &["plan"]);
    assert!(
        plan["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["action"] == "noop" || a["action"] == "unsupported"),
        "{plan}"
    );
    f.run(&ws, &["sync"]);
    assert_eq!(std::fs::read(&config).unwrap(), before);
    let removed = f.run(&ws, &["uninstall", "--execute"]);
    assert!(
        removed["kept_conflicts"].as_array().unwrap().is_empty(),
        "{removed}"
    );
    let remaining: toml::Value = std::fs::read_to_string(&config).unwrap().parse().unwrap();
    assert_eq!(remaining["model"].as_str(), Some("user-model"));
    assert_eq!(remaining["skills"]["user_setting"].as_str(), Some("keep"));
    let entries = remaining["skills"]["config"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{remaining}");
    assert_eq!(entries[0]["path"].as_str(), Some("/user/skill/SKILL.md"));
    assert!(!ws.join(".agents/skills/common-greet").exists());
}

#[test]
fn legacy_whole_config_manifest_migrates_without_claiming_user_fields() {
    let f = Fixture::new();
    let (ws, _) = f.setup();
    let path = f.manifest(&ws);
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let items = manifest["items"].as_object_mut().unwrap();
    let keys: Vec<_> = items
        .keys()
        .filter(|key| key.starts_with(".codex/config.toml#tomlarr:skills.config:"))
        .cloned()
        .collect();
    let mut legacy = items[&keys[0]].clone();
    for key in keys {
        items.remove(&key);
    }
    // 模拟旧版 skills Full 写入后 MCP 又改了整文件，旧清单哈希已经失效。
    legacy["content_hash"] = json!("sha256:old-pre-mcp-full-file");
    items.insert(".codex/config.toml".into(), legacy);
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let doctor = f.run(&ws, &["doctor"]);
    assert_eq!(doctor["ok"], true, "{doctor}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "doctor 的兼容读取不写盘"
    );
    f.run(&ws, &["sync"]);
    let migrated: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(migrated["items"][".codex/config.toml"].is_null());
    let result = f.run(&ws, &["uninstall", "--execute"]);
    assert!(
        result["kept_conflicts"].as_array().unwrap().is_empty(),
        "{result}"
    );
    assert!(std::fs::read_to_string(ws.join(".codex/config.toml"))
        .unwrap()
        .contains("user-model"));
}

#[test]
fn reingest_transcript_is_idempotent_and_distinct_messages_survive() {
    let f = Fixture::new();
    let ws = common::make_business_repo(f.tmp.path(), "biz");
    let file = f.tmp.path().join("transcript.jsonl");
    let record = |uuid, time| json!({"sessionId":"s", "uuid":uuid, "timestamp":time, "message":{"id":uuid, "usage":{"input_tokens":10,"output_tokens":5}}});
    let first = record("one", "2026-10-03T01:00:00Z");
    std::fs::write(&file, format!("{first}\n")).unwrap();
    assert_eq!(
        f.run(
            &ws,
            &[
                "session",
                "--action",
                "ingest",
                "--file",
                file.to_str().unwrap()
            ]
        )["appended"],
        1
    );
    let repeated = f.run(
        &ws,
        &[
            "session",
            "--action",
            "ingest",
            "--file",
            file.to_str().unwrap(),
        ],
    );
    assert_eq!(repeated["appended"], 0);
    assert_eq!(repeated["deduplicated"], 1);
    let second = record("two", "2026-10-03T01:01:00Z");
    std::fs::write(&file, format!("{first}\n{second}\n")).unwrap();
    let appended = f.run(
        &ws,
        &[
            "session",
            "--action",
            "ingest",
            "--file",
            file.to_str().unwrap(),
        ],
    );
    assert_eq!(appended["appended"], 1);
    assert_eq!(appended["deduplicated"], 1);
    let metrics = f.run(&ws, &["session", "--action", "metrics", "--session", "s"]);
    assert_eq!(metrics["stop_count"], 2, "{metrics}");
}

#[test]
fn explicit_init_source_replacement_updates_lock_without_refresh() {
    let f = Fixture::new();
    let (ws, _) = f.setup();
    let new_source = common::make_team_source(&f.tmp.path().join("new-source"));
    let url = common::file_url(&new_source);
    let result = f.run(&ws, &["init", "--url", &url]);
    assert!(result["source"]["resolved_commit"].is_string(), "{result}");
    let lock: Value = serde_json::from_slice(
        &std::fs::read(ws.join(".ailoom/machine/sources.lock.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(lock["sources"]["team"]["type"], "git");
    f.run(&ws, &["plan"]);
}
