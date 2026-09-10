//! 同步计划引擎（AIL-007）测试：所有权矩阵逐行验证 + plan 无写入 + 确定性。

use ailoom::adapters::common::{upsert_fragment, Artifact, ArtifactBody};
use ailoom::sync::manifest::{ManagedItem, ManagedManifest};
use ailoom::sync::plan::{build_plan, ActionKind};
use std::path::Path;

fn artifact_full(rid: &str, path: &str, content: &str) -> Artifact {
    Artifact {
        resource_id: rid.into(),
        target_tool: "claude".into(),
        kind: "skill".into(),
        path: path.into(),
        body: ArtifactBody::Full {
            content: content.into(),
        },
    }
}

fn manifest_with(ws_id: &str, key: &str, rid: &str, hash: &str) -> ManagedManifest {
    let mut m = ManagedManifest::new(ws_id);
    m.items.insert(
        key.into(),
        ManagedItem {
            resource_id: rid.into(),
            target_tool: "claude".into(),
            kind: "skill".into(),
            content_hash: hash.into(),
            deployed_at: "2026-09-09T00:00:00Z".into(),
        },
    );
    m
}

fn h(content: &str) -> String {
    format!("sha256:{}", ailoom::ids::sha256_hex(content.as_bytes()))
}

fn tree_snapshot(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let content = std::fs::read(entry.path()).unwrap_or_default();
            out.push((
                entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                ailoom::ids::sha256_hex(&content),
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn untouched_target_with_new_source_content_is_update() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(ws.join("sub")).unwrap();
    std::fs::write(ws.join("sub/f.md"), "old").unwrap();
    let managed = manifest_with("ws1", "sub/f.md", "team/skill/common/a", &h("old"));

    let art = artifact_full("team/skill/common/a", "sub/f.md", "new");
    let plan = build_plan(&[art], &managed, &ws, "git+test", Some("r1".into())).unwrap();
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].action, ActionKind::Update);
    assert_eq!(
        plan.actions[0].precondition_hash.as_deref(),
        Some(h("old").as_str())
    );
}

#[test]
fn user_modified_target_is_conflict_not_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    // 源没变（期望仍为 old），但用户把目标改成了 mine
    std::fs::write(ws.join("f.md"), "mine").unwrap();
    let managed = manifest_with("ws1", "f.md", "team/skill/common/a", &h("old"));
    let art = artifact_full("team/skill/common/a", "f.md", "old");
    let plan = build_plan(&[art], &managed, &ws, "git+test", None).unwrap();
    assert_eq!(plan.actions[0].action, ActionKind::Conflict);
}

#[test]
fn deleted_source_resource_planned_for_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("gone.md"), "old").unwrap();
    let managed = manifest_with("ws1", "gone.md", "team/skill/common/gone", &h("old"));
    // 期望产物为空（源删除）
    let plan = build_plan(&[], &managed, &ws, "git+test", None).unwrap();
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].action, ActionKind::Delete);
    assert_eq!(
        plan.actions[0].manifest_hash.as_deref(),
        Some(h("old").as_str())
    );
}

#[test]
fn deleted_source_but_user_modified_keeps_and_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("gone.md"), "user-edit").unwrap();
    let managed = manifest_with("ws1", "gone.md", "team/skill/common/gone", &h("old"));
    let plan = build_plan(&[], &managed, &ws, "git+test", None).unwrap();
    assert_eq!(plan.actions[0].action, ActionKind::Conflict);
}

#[test]
fn untracked_same_content_is_not_adopted() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("f.md"), "same").unwrap();
    let art = artifact_full("team/skill/common/a", "f.md", "same");
    let plan = build_plan(&[art], &ManagedManifest::new("ws1"), &ws, "git+test", None).unwrap();
    assert_eq!(
        plan.actions[0].action,
        ActionKind::Conflict,
        "内容相同也不自动接管"
    );
}

#[test]
fn missing_target_with_manifest_is_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let managed = manifest_with("ws1", "f.md", "team/skill/common/a", &h("old"));
    let art = artifact_full("team/skill/common/a", "f.md", "new");
    let plan = build_plan(&[art], &managed, &ws, "git+test", None).unwrap();
    assert_eq!(plan.actions[0].action, ActionKind::Restore);
}

#[test]
fn create_and_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("exists.md"), "same").unwrap();
    let managed = manifest_with("ws1", "exists.md", "team/skill/common/b", &h("same"));
    let new_art = artifact_full("team/skill/common/a", "new.md", "content");
    let same_art = artifact_full("team/skill/common/b", "exists.md", "same");
    let plan = build_plan(&[new_art, same_art], &managed, &ws, "git+test", None).unwrap();
    // 动作按 path 排序：exists.md 在前（Noop），new.md 在后（Create）
    let a = &plan.actions[0];
    assert_eq!(a.action, ActionKind::Noop, "{:?}", plan.actions);
    let b = &plan.actions[1];
    assert_eq!(b.action, ActionKind::Create);
}

#[test]
fn plan_makes_no_file_changes_and_is_deterministic() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("f.md"), "old").unwrap();
    let managed = manifest_with("ws1", "f.md", "team/skill/common/a", &h("old"));

    let mk_arts = || {
        vec![
            artifact_full("team/skill/common/b", "b.md", "B"),
            artifact_full("team/skill/common/a", "a.md", "A"),
        ]
    };
    let before = tree_snapshot(&ws);
    let p1 = build_plan(&mk_arts(), &managed, &ws, "git+test", Some("r1".into())).unwrap();
    let after = tree_snapshot(&ws);
    assert_eq!(before, after, "plan 运行前后文件树完全相同");

    let p2 = build_plan(&mk_arts(), &managed, &ws, "git+test", Some("r1".into())).unwrap();
    let j1 = serde_json::to_string(&p1).unwrap();
    let j2 = serde_json::to_string(&p2).unwrap();
    // created_at 之外全部一致：比较去掉 created_at 的 JSON
    let strip = |s: &str| s.split("\"created_at\"").next().unwrap_or("").to_string();
    assert_eq!(strip(&j1), strip(&j2), "同一计划输入产生稳定输出");
    assert_eq!(p1.actions.len(), 3, "两个 create + 一个 update");
}

#[test]
fn fragment_deletion_only_removes_managed_part() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let rid = "team/rule/common/std";
    let file_text = upsert_fragment("# 用户标题\n", rid, "托管规则");
    std::fs::write(ws.join("AGENTS.md"), &file_text).unwrap();

    let mut managed = ManagedManifest::new("ws1");
    managed.items.insert(
        "AGENTS.md#fragment".into(),
        ManagedItem {
            resource_id: rid.into(),
            target_tool: "codex".into(),
            kind: "rule".into(),
            content_hash: h("托管规则"),
            deployed_at: "2026-09-09T00:00:00Z".into(),
        },
    );
    // 源不再产出该规则
    let plan = build_plan(&[], &managed, &ws, "git+test", None).unwrap();
    assert_eq!(plan.actions[0].action, ActionKind::Delete);
    assert_eq!(plan.actions[0].path, "AGENTS.md");
}

#[test]
fn json_entry_merge_plan_compares_entry_only() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    // 用户配置含无关字段
    std::fs::write(
        ws.join(".mcp.json"),
        r#"{"mcpServers":{"mine":{"command":"x"}},"other":1}"#,
    )
    .unwrap();
    let entry = serde_json::json!({"command":"uvx","args":["a"]});
    let art = Artifact {
        resource_id: "team/mcp/common/files".into(),
        target_tool: "claude".into(),
        kind: "mcp".into(),
        path: ".mcp.json".into(),
        body: ArtifactBody::JsonPointer {
            pointer: "/mcpServers/team-files".into(),
            value: entry.clone(),
        },
    };
    // 文件未托管但条目缺失 → create（条目级合并，保留 mine 与无关字段）
    let plan = build_plan(
        std::slice::from_ref(&art),
        &ManagedManifest::new("ws1"),
        &ws,
        "git+test",
        None,
    )
    .unwrap();
    assert_eq!(
        plan.actions[0].action,
        ActionKind::Create,
        "结构化条目按条目粒度主张"
    );

    // 托管过且条目未变 → noop
    let mut managed = ManagedManifest::new("ws1");
    managed.items.insert(
        ".mcp.json#json:/mcpServers/team-files".into(),
        ManagedItem {
            resource_id: "team/mcp/common/files".into(),
            target_tool: "claude".into(),
            kind: "mcp".into(),
            content_hash: art.desired_hash().unwrap(),
            deployed_at: "2026-09-09T00:00:00Z".into(),
        },
    );
    // 模拟已部署：文件里写上该条目
    let mut value: serde_json::Value =
        serde_json::from_str(r#"{"mcpServers":{"mine":{"command":"x"}},"other":1}"#).unwrap();
    value["mcpServers"]["team-files"] = entry;
    std::fs::write(
        ws.join(".mcp.json"),
        serde_json::to_string_pretty(&value).unwrap(),
    )
    .unwrap();
    let plan = build_plan(&[art], &managed, &ws, "git+test", None).unwrap();
    assert_eq!(
        plan.actions[0].action,
        ActionKind::Noop,
        "{:?}",
        plan.actions[0]
    );
}
