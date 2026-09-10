//! 会话、Token 与人工干预聚合（AIL-019）+ 摩擦提示（AIL-020）单元/集成测试。

use ailoom::events::aggregate::{aggregate_session, parse_claude_transcript, HeuristicConfig};
use ailoom::events::friction::{build_share_record, friction_score, should_prompt, FrictionConfig};
use ailoom::events::schema::{Event, TokenSnapshot};

fn ev(
    kind: &str,
    sid: &str,
    tool_name: Option<&str>,
    exit_code: Option<i64>,
    dedup: Option<&str>,
    tokens: Option<TokenSnapshot>,
) -> Event {
    Event {
        schema_version: 1,
        event_id: ailoom::ids::new_id(),
        session_id: sid.into(),
        workspace_id: "wsA".into(),
        device_id: "dev".into(),
        tool: "claude".into(),
        time: "2026-09-09T00:00:00Z".into(),
        kind: kind.into(),
        tool_name: tool_name.map(str::to_string),
        exit_code,
        duration_ms: None,
        prompt_len: None,
        prompt_hash: None,
        tokens,
        dedup_key: dedup.map(str::to_string),
    }
}

fn cfg() -> HeuristicConfig {
    HeuristicConfig::default()
}

#[test]
fn cumulative_tokens_take_max_not_sum() {
    let events = vec![
        ev(
            "stop",
            "s1",
            None,
            None,
            None,
            Some(TokenSnapshot {
                input: 100,
                output: 50,
                cache_read: 0,
                cache_creation: 0,
            }),
        ),
        ev(
            "stop",
            "s1",
            None,
            None,
            None,
            Some(TokenSnapshot {
                input: 150,
                output: 80,
                cache_read: 0,
                cache_creation: 0,
            }),
        ),
    ];
    let m = aggregate_session("wsA", "s1", &events, &cfg()).unwrap();
    assert_eq!(m.tokens.input, Some(150), "累计快照 100/150 → 150 不是 250");
    assert_eq!(m.tokens.output, Some(80));
    // 延迟刷新可补齐：更低值不回退
    let events2 = vec![
        events[0].clone(),
        ev(
            "stop",
            "s1",
            None,
            None,
            None,
            Some(TokenSnapshot {
                input: 120,
                output: 60,
                cache_read: 0,
                cache_creation: 0,
            }),
        ),
        events[1].clone(),
    ];
    let m2 = aggregate_session("wsA", "s1", &events2, &cfg()).unwrap();
    assert_eq!(m2.tokens.input, Some(150), "乱序/延迟刷新取最大");
}

#[test]
fn missing_tokens_reported_unavailable_not_zero() {
    let events = vec![
        ev("session-start", "s2", None, None, None, None),
        ev("stop", "s2", None, None, None, None),
    ];
    let m = aggregate_session("wsA", "s2", &events, &cfg()).unwrap();
    assert_eq!(m.tokens_availability, "unavailable");
    assert_eq!(m.tokens.input, None, "缺失用 null 表示而非 0");
}

#[test]
fn tool_errors_and_human_interventions_counted_separately() {
    let events = vec![
        ev("tool", "s3", Some("Bash"), Some(1), None, None),
        ev("tool", "s3", Some("Bash"), Some(0), None, None),
        ev(
            "tool",
            "s3",
            Some("Bash"),
            Some(2),
            Some("intervention-1"),
            None,
        ),
    ];
    let m = aggregate_session("wsA", "s3", &events, &cfg()).unwrap();
    assert_eq!(m.tool_calls, 3);
    assert_eq!(m.tool_errors, 2, "exit_code 非 0 为工具错误");
    assert_eq!(m.interventions, 1, "人工干预单独计数");
}

#[test]
fn workspaces_with_same_session_string_do_not_mix() {
    let mut e = ev("stop", "same-sid", None, None, None, None);
    e.workspace_id = "wsB".into();
    let events = vec![ev("stop", "same-sid", None, None, None, None), e];
    let a = aggregate_session("wsA", "same-sid", &events, &cfg()).unwrap();
    let b = aggregate_session("wsB", "same-sid", &events, &cfg()).unwrap();
    assert_eq!(a.workspace_id, "wsA");
    assert_eq!(b.workspace_id, "wsB");
    assert_eq!(a.stop_count, 1);
    assert_eq!(b.stop_count, 1);
}

#[test]
fn transcript_ingestion_produces_token_snapshots() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("t.jsonl");
    std::fs::write(
        &f,
        concat!(
            r#"{"sessionId":"s9","message":{"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            "\n",
            r#"{"sessionId":"s9","message":{"usage":{"input_tokens":25,"output_tokens":9,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            "\n",
            "not json\n"
        ),
    )
    .unwrap();
    let events = parse_claude_transcript(&f, "wsA", "dev").unwrap();
    assert_eq!(events.len(), 2, "坏行跳过");
    let m = aggregate_session("wsA", "s9", &events, &cfg()).unwrap();
    assert_eq!(m.tokens.input, Some(25), "聚合取最大：延迟刷新补齐语义");
    assert_eq!(m.tokens.output, Some(9));
}

// ---------- AIL-020 摩擦提示 ----------

fn metrics_with(
    interventions: u64,
    tool_errors: u64,
    tool_calls: u64,
) -> ailoom::events::aggregate::SessionMetrics {
    let mut m = ailoom::events::aggregate::SessionMetrics {
        session_id: "s".into(),
        workspace_id: "wsA".into(),
        tool: "claude".into(),
        ..Default::default()
    };
    m.interventions = interventions;
    m.tool_errors = tool_errors;
    m.tool_calls = tool_calls;
    m
}

#[test]
fn high_volume_clean_session_never_prompts() {
    let cfg = FrictionConfig::default();
    let m = metrics_with(0, 0, 500);
    assert!(!should_prompt(&m, &cfg), "500 次无错误调用不触发");
    assert_eq!(friction_score(&m, &cfg), 0);
}

#[test]
fn interventions_beyond_threshold_trigger_once_then_state_persists() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = FrictionConfig::default();
    let m = metrics_with(3, 1, 20);
    assert!(should_prompt(&m, &cfg));
    assert!(!ailoom::events::friction::was_prompted(tmp.path(), "s1"));
    ailoom::events::friction::mark_prompted(tmp.path(), "s1").unwrap();
    assert!(
        ailoom::events::friction::was_prompted(tmp.path(), "s1"),
        "提示状态持久化：进程重启后不重复提示"
    );
}

#[test]
fn prompt_disabled_still_counts() {
    let cfg = FrictionConfig {
        prompt_enabled: false,
        ..Default::default()
    };
    let m = metrics_with(5, 5, 30);
    assert!(!should_prompt(&m, &cfg), "关闭提示不触发");
    assert!(
        friction_score(&m, &cfg) >= cfg.min_score,
        "关闭提示仍统计分数"
    );
}

#[test]
fn share_record_contains_counts_only() {
    let m = metrics_with(1, 1, 5);
    let share = build_share_record(&m);
    let text = serde_json::to_string(&share).unwrap();
    assert!(
        !text.to_lowercase().contains("prompt\"") || !text.contains("prompt_text"),
        "无 prompt 正文"
    );
    assert!(share["prompt_count"].as_u64().is_some(), "有计数");
    assert!(share.get("summary_body").is_none() && share.get("transcript").is_none());
}
