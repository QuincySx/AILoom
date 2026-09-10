//! 知识反馈、晋升与维护（AIL-028）测试。

use ailoom::appctx::AppContext;
use ailoom::knowledge::feedback::{
    build_maintenance_plan_from_usage, record_feedback, record_recall_hits, UsageRecord,
};

fn usage_record() -> UsageRecord {
    UsageRecord {
        schema_version: 1,
        usage: Default::default(),
        feedback: Default::default(),
    }
}

fn manual_ctx(tmp: &tempfile::TempDir) -> AppContext {
    let data = tmp.path().join("data");
    let workspace_root = tmp.path().join("biz");
    let workspace = ailoom::workspace::Workspace {
        workspace_root: workspace_root.clone(),
        repository_anchor: "path+abc".into(),
        anchor_key: "ck".into(),
        workspace_id: ailoom::ids::workspace_id_from_root(&workspace_root),
        is_git: true,
        declaration_path: None,
    };
    AppContext {
        data_root: data.clone(),
        layout: ailoom::paths::layout_for(&data, &workspace.workspace_id, &workspace.anchor_key),
        workspace,
        device: "dev".into(),
    }
}

#[test]
fn recall_usage_records_and_feedback_accumulates() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = manual_ctx(&tmp);
    // 真实召回记录使用量
    record_recall_hits(&ctx, &["team/learning/common/x".into()]).unwrap();
    record_recall_hits(&ctx, &["team/learning/common/x".into()]).unwrap();
    // 显式反馈（有用 1 次，无用 2 次）
    record_feedback(&ctx, "team/learning/common/x", true).unwrap();
    record_feedback(&ctx, "team/learning/common/x", false).unwrap();
    record_feedback(&ctx, "team/learning/common/x", false).unwrap();

    // 读取持久化状态验证
    let path = tmp.path().join("data").join("ws");
    let mut found = None;
    for entry in std::fs::read_dir(&path).unwrap().flatten() {
        let up = entry.path().join("knowledge-usage.json");
        if up.is_file() {
            found = Some(std::fs::read_to_string(&up).unwrap());
        }
    }
    let text = found.expect("使用记录已持久化");
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(record["usage"]["team/learning/common/x"], 2, "两次召回计数");
    assert_eq!(
        record["feedback"]["team/learning/common/x"],
        serde_json::json!([1, 2]),
        "有用1 无用2"
    );
}

#[test]
fn maintenance_candidates_require_explicit_negative_signal() {
    let mut usage = usage_record();
    usage.usage.insert("zero-use".into(), 0);
    usage.usage.insert("b".into(), 1);
    usage.feedback.insert("a".into(), (2, 1));
    let plan = build_maintenance_plan_from_usage(&usage);
    // 零使用的 a 不成为候选：低频不直接等同无价值
    assert!(
        plan.archive_candidates.iter().all(|c| c.id != "zero-use"),
        "低使用量仅为提示"
    );
    // 显式负反馈 → 候选
    usage.feedback.insert("b".into(), (1, 3));
    let plan2 = build_maintenance_plan_from_usage(&usage);
    assert_eq!(plan2.archive_candidates.len(), 1);
    assert_eq!(plan2.archive_candidates[0].id, "b");
    assert_eq!(plan2.archive_candidates[0].reason, "显式无用反馈多于有用");
    assert!(plan2.dry_run, "维护先预览");
}

#[test]
fn promotion_draft_preserves_learning_id() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = manual_ctx(&tmp);
    let p =
        ailoom::knowledge::feedback::promotion_draft(&ctx, "team/learning/common/x", "新规则文本")
            .unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(
        text.contains("source_learning: team/learning/common/x"),
        "晋升保留来源: {text}"
    );
    assert!(text.contains("rule-from-"));
}
