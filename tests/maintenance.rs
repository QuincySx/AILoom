//! 知识反馈、晋升与维护（AIL-028）测试。

mod common;

use ailoom::appctx::AppContext;
use ailoom::knowledge::feedback::{
    build_maintenance_plan_from_usage, record_feedback, record_recall_hits, UsageRecord,
};

fn usage_record() -> UsageRecord {
    UsageRecord {
        schema_version: 1,
        usage: Default::default(),
        feedback: Default::default(),
        feedback_events: Default::default(),
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
    record_feedback(&ctx, "team/learning/common/x", true, None).unwrap();
    record_feedback(&ctx, "team/learning/common/x", false, None).unwrap();
    record_feedback(&ctx, "team/learning/common/x", false, None).unwrap();

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

/// AIL-028 反馈幂等：同一 feedback_id 重试不翻倍；不同事件各自计数。
#[test]
fn feedback_event_id_retry_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = manual_ctx(&tmp);
    record_recall_hits(&ctx, &["team/learning/common/x".into()]).unwrap();

    record_feedback(&ctx, "team/learning/common/x", false, Some("ci-event-1")).unwrap();
    // 同一事件重试（网络重发/CI 重跑）
    record_feedback(&ctx, "team/learning/common/x", false, Some("ci-event-1")).unwrap();
    record_feedback(&ctx, "team/learning/common/x", false, Some("ci-event-1")).unwrap();

    // 另一个独立事件
    record_feedback(&ctx, "team/learning/common/x", false, Some("ci-event-2")).unwrap();

    let path = tmp.path().join("data").join("ws");
    let mut found = None;
    for entry in std::fs::read_dir(&path).unwrap().flatten() {
        let up = entry.path().join("knowledge-usage.json");
        if up.is_file() {
            found = Some(std::fs::read_to_string(&up).unwrap());
        }
    }
    let record: serde_json::Value =
        serde_json::from_str(&found.expect("使用记录已持久化")).unwrap();
    assert_eq!(
        record["feedback"]["team/learning/common/x"],
        serde_json::json!([0, 2]),
        "同事件重试只计一次：0 有用 2 无用（两个独立事件）: {record}"
    );
}

/// AIL-028 归档/恢复闭环：归档 → 召回排除；恢复 → 召回可见。
#[test]
fn archive_restore_round_trip_via_recall() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let src = common::make_team_source(tmp.path());
    ailoom::gitx::git(&src, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(&src, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
    ailoom::gitx::git(&src, &["push", "-q", "-u", "origin", "main"]).unwrap();
    let ws = common::make_business_repo(tmp.path(), "biz");
    let dr = tmp.path().join("data").to_str().unwrap().to_string();

    let run = |args: &[&str]| {
        let mut bin_path = std::env::current_exe().unwrap();
        loop {
            if bin_path.join("ailoom").exists() {
                break;
            }
            if !bin_path.pop() {
                panic!("未找到 ailoom 二进制");
            }
        }
        let out = std::process::Command::new(bin_path.join("ailoom"))
            .args(args)
            .current_dir(&ws)
            .envs(common::isolated_child_env(tmp.path()))
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let init_args = [
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = init_args.iter().map(String::as_str).collect();
    let (code, _, stderr) = run(&refs);
    assert_eq!(code, 0, "{stderr}");

    let recall = |q: &str| run(&["--json", "--data-root", dr.as_str(), "recall", "--query", q]);

    // 共享经验 a-postmortem 可召回
    let (code, stdout, stderr) = recall("postmortem");
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let ids: Vec<&str> = v["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["id"].as_str())
        .collect();
    assert!(
        ids.iter().any(|i| i.contains("a-postmortem")),
        "归档前可召回: {v}"
    );

    // 归档：同查询不再返回该经验
    let (code, _, stderr) = run(&[
        "--json",
        "--data-root",
        dr.as_str(),
        "knowledge",
        "--action",
        "archive",
        "--id",
        "team/learning/team-lib/a-postmortem",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let (code, stdout, _) = recall("postmortem");
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let ids: Vec<&str> = v["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["id"].as_str())
        .collect();
    assert!(
        !ids.iter().any(|i| i.contains("a-postmortem")),
        "归档后召回排除: {v}"
    );

    // 恢复：重新可见（闭环）
    let (code, _, stderr) = run(&[
        "--json",
        "--data-root",
        dr.as_str(),
        "knowledge",
        "--action",
        "restore",
        "--id",
        "team/learning/team-lib/a-postmortem",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let (code, stdout, _) = recall("postmortem");
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let ids: Vec<&str> = v["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["id"].as_str())
        .collect();
    assert!(
        ids.iter().any(|i| i.contains("a-postmortem")),
        "恢复后重新可召回: {v}"
    );
}
