use ailoom::native_files::{self, Request};
use serde_json::{json, Value};
use std::path::Path;
fn call(data: &Path, home: &Path, root: &Path, body: Value) -> ailoom::error::Result<Value> {
    let mut body = body;
    body["scope"] = json!("project");
    body["root"] = json!(root);
    native_files::run(
        data,
        home,
        &serde_json::from_value::<Request>(body).unwrap(),
    )
}
#[test]
fn native_crud_preserves_conditions_and_rejects_stale_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let data = root.join("data");
    let home = root.join("home");
    std::fs::create_dir(&home).unwrap();
    let raw = "---\npaths:\n  - src/**\ncustom: preserve\n---\n原生规则\n";
    let base = json!({"action":"save","target":"claude-rules","name":"ui/style.md","content":raw,"expected":"missing"});
    call(&data, &home, &root, base.clone()).unwrap();
    assert!(call(&data, &home, &root, base).is_err());
    let v = call(
        &data,
        &home,
        &root,
        json!({"action":"read","target":"claude-rules","name":"ui/style.md"}),
    )
    .unwrap();
    assert_eq!(v["content"], raw);
    let list = call(&data, &home, &root, json!({"action":"list"})).unwrap();
    assert!(list["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "ui/style.md"));
    call(&data,&home,&root,json!({"action":"save","target":"claude-rules","name":"ui/style.md","content":"changed","expected":v["expected"]})).unwrap();
    assert!(call(&data,&home,&root,json!({"action":"delete","target":"claude-rules","name":"ui/style.md","expected":v["expected"]})).is_err());
    let v = call(
        &data,
        &home,
        &root,
        json!({"action":"read","target":"claude-rules","name":"ui/style.md"}),
    )
    .unwrap();
    call(&data,&home,&root,json!({"action":"delete","target":"claude-rules","name":"ui/style.md","expected":v["expected"]})).unwrap();
    assert!(!root.join(".claude/rules/ui/style.md").exists());
    assert_eq!(
        std::fs::read_dir(data.join("native-files/backups"))
            .unwrap()
            .count(),
        2
    );
    for name in ["../../outside.md", "/outside.md", "secret.json"] {
        assert!(call(
            &data,
            &home,
            &root,
            json!({"action":"read","target":"claude-rules","name":name})
        )
        .is_err());
    }
}
#[test]
fn global_and_project_are_separate() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let data = root.join("data");
    let home = root.join("home");
    std::fs::create_dir(&home).unwrap();
    let r:Request=serde_json::from_value(json!({"scope":"global","action":"save","target":"cursor-agents","name":"review.md","content":"---\nname: review\ndescription: Review code\n---\nReview","expected":"missing"})).unwrap();
    native_files::run(&data, &home, &r).unwrap();
    assert!(home.join(".cursor/agents/review.md").exists());
    assert!(
        call(&data, &home, &root, json!({"action":"list"})).unwrap()["files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[cfg(unix)]
#[test]
fn links_cannot_escape_native_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let rules = root.join(".claude/rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(root.join("secret.md"), "outside").unwrap();
    std::os::unix::fs::symlink(root.join("secret.md"), rules.join("escape.md")).unwrap();
    assert!(call(
        &root.join("data"),
        &root,
        &root,
        json!({"action":"read","target":"claude-rules","name":"escape.md"})
    )
    .is_err());
    let list = call(&root.join("data"), &root, &root, json!({"action":"list"})).unwrap();
    assert!(!list["warnings"].as_array().unwrap().is_empty());
}
#[test]
fn skill_scan_includes_all_adapter_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for host in [
        ".claude",
        ".agents",
        ".codex",
        ".cursor",
        ".grok",
        ".pi",
        ".opencode",
    ] {
        let dir = root.join(host).join("skills/topic/example");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: example\ndescription: example\n---\n",
        )
        .unwrap();
    }
    let v = ailoom::commands::scan_skills::scan_project_skills(&root, None, &root.join("store"));
    assert_eq!(v["items"].as_array().unwrap().len(), 7);
}

#[cfg(unix)]
#[test]
fn project_internal_skillshare_links_are_discovered_once() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let skill = root.join(".skillshare/skills/example");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: example\ndescription: shared\n---\n",
    )
    .unwrap();
    for host in [".claude", ".agents"] {
        std::fs::create_dir(root.join(host)).unwrap();
        std::os::unix::fs::symlink("../.skillshare/skills", root.join(host).join("skills"))
            .unwrap();
    }
    let v = ailoom::commands::scan_skills::scan_project_skills(&root, None, &root.join("store"));
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["management"], "unmanaged");
    assert_eq!(items[0]["name"], "example");
}

#[test]
fn codex_native_toml_is_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let content="name = \"review\"\ndescription = \"Review\"\ndeveloper_instructions = \"Read only\"\nsandbox_mode = \"read-only\"\n";
    call(&root.join("data"),&root,&root,json!({"action":"save","target":"codex-agents","name":"review.toml","content":content,"expected":"missing"})).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(".codex/agents/review.toml")).unwrap(),
        content
    );
    assert!(!root.join(".codex/config.toml").exists());
}
