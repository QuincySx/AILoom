mod common;
use ailoom::knowledge::{
    feedback,
    location::{self, Request},
    portable,
};
use serde_json::{json, Value};
use std::path::Path;

fn request(action: &str, root: &Path) -> Request {
    Request {
        action: action.into(),
        root: root.into(),
        ..Default::default()
    }
}
fn confirmed(data: &Path, r: Request) -> Value {
    let preview = location::run(data, &r).unwrap();
    location::run(
        data,
        &Request {
            execute: true,
            expected: preview["expected"].as_str().map(String::from),
            ..r
        },
    )
    .unwrap()
}

#[test]
fn rebuilding_keeps_originals_instructions_archives_feedback_and_git_while_removing_old_wiki() {
    common::isolate_in_process_roots();
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let kb = t.path().join("knowledge");
    let data = t.path().join("data");
    std::fs::create_dir(&root).unwrap();
    let init = location::run(
        &data,
        &Request {
            path: Some(kb.clone()),
            ..request("init", &root)
        },
    )
    .unwrap();
    common::legacy_knowledge_library(&kb);
    let d = portable::read(&root).unwrap().unwrap();
    let repo = init["project_id"].as_str().unwrap();
    ailoom::personal_instructions::save_entry(&data, repo, None, "KEEP PROJECT CONTRACT").unwrap();
    let original = "# 原始决策\noriginal-readme-unique\n外部配额要求串行处理。\n";
    std::fs::write(kb.join("README.md"), original).unwrap();
    let note = t.path().join("note.md");
    std::fs::write(&note, "# 旧经验\narchiveduniquelesson").unwrap();
    location::save(&data, &root, &note, "decision").unwrap();
    let recalled = location::recall(&data, &root, "archiveduniquelesson", 5).unwrap();
    let old_id = recalled["results"][0]["id"].as_str().unwrap();
    let ctx = location::maintenance_context(&data, &root)
        .unwrap()
        .unwrap();
    feedback::record_feedback(&ctx, old_id, true, Some("feedback-once")).unwrap();
    feedback::archive(&ctx, old_id).unwrap();
    let legacy_wiki = kb.join("wiki").join(&d.project_id);
    std::fs::create_dir_all(&legacy_wiki).unwrap();
    std::fs::write(legacy_wiki.join("index.md"), "old generated wiki").unwrap();
    std::fs::write(kb.join(".gitignore"), "temporary/\n").unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&kb)
        .status()
        .unwrap()
        .success());
    let preview = location::run(&data, &request("reset-wiki", &root)).unwrap();
    assert_eq!(
        std::fs::read_to_string(kb.join("README.md")).unwrap(),
        original
    );
    let reset = location::run(
        &data,
        &Request {
            execute: true,
            expected: preview["expected"].as_str().map(String::from),
            ..request("reset-wiki", &root)
        },
    )
    .unwrap();
    assert_eq!(reset["knowledge_id"], d.knowledge_id);
    assert!(reset["warnings"].as_array().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(kb.join("sources/notes/README.md")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read(kb.join("sources/notes/decision.md")).unwrap(),
        std::fs::read(note).unwrap()
    );
    assert!(!kb.join("projects").exists());
    assert!(!kb.join("imports").exists());
    assert!(!kb.join("wiki").join(&d.project_id).exists());
    assert!(kb.join("wiki").is_dir());
    assert!(kb.join(".git").exists());
    assert_eq!(
        std::fs::read_to_string(kb.join(".gitignore")).unwrap(),
        "temporary/\n"
    );
    assert!(
        location::recall(&data, &root, "archiveduniquelesson", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let usage: Value = serde_json::from_slice(
        &std::fs::read(
            data.join("knowledge/state")
                .join(&d.knowledge_id)
                .join("knowledge-usage.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(usage["usage"]["sources/notes/decision.md"], 1);
    assert_eq!(
        usage["feedback"]["sources/notes/decision.md"],
        json!([1, 0])
    );
    assert_eq!(
        usage["feedback_events"]["feedback-once"],
        "sources/notes/decision.md:useful"
    );
    location::run(&data, &request("checkpoint", &root)).unwrap();
    let state: Value =
        serde_json::from_slice(&std::fs::read(kb.join(".ailoom/project/state.json")).unwrap())
            .unwrap();
    assert_eq!(state["instructions"]["project"], "KEEP PROJECT CONTRACT");
    assert!(Path::new(reset["backup"].as_str().unwrap())
        .join("wiki")
        .join(&d.project_id)
        .join("index.md")
        .is_file());
}

#[test]
fn separating_a_modern_library_preserves_handwritten_pages_and_homepage() {
    common::isolate_in_process_roots();
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let kb = t.path().join("library");
    let data = t.path().join("data");
    std::fs::create_dir(&root).unwrap();
    location::run(
        &data,
        &Request {
            path: Some(kb.clone()),
            ..request("init", &root)
        },
    )
    .unwrap();
    let home = "# 手工首页\n[人工模块](wiki/index.md)\n";
    let body = "# 人工模块\nhandwritten-separation-contract\n";
    std::fs::write(kb.join("README.md"), home).unwrap();
    std::fs::write(kb.join("wiki/index.md"), body).unwrap();
    let destination = t.path().join("new-library");
    confirmed(
        &data,
        Request {
            path: Some(destination.clone()),
            ..request("separate", &root)
        },
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("README.md")).unwrap(),
        home
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("wiki/index.md")).unwrap(),
        body
    );
    assert_eq!(
        location::recall(&data, &root, "handwritten-separation-contract", 5).unwrap()["results"][0]
            ["id"],
        "wiki/index.md"
    );
}
