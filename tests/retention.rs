//! 事件留存、数据导出与清理（AIL-037）测试。

use ailoom::appctx::AppContext;
use ailoom::data::{run as data_run, DataArgs};
use ailoom::events::schema::{Event, TokenSnapshot};
use std::path::Path;

fn ev(ctx: &AppContext, sid: &str, n: usize) -> Event {
    Event {
        schema_version: 1,
        event_id: format!("e{n}-{sid}"),
        session_id: sid.into(),
        workspace_id: ctx.workspace.workspace_id.clone(),
        device_id: "dev".into(),
        tool: "claude".into(),
        time: "2026-09-09T00:00:00Z".into(),
        kind: "stop".into(),
        tool_name: None,
        exit_code: None,
        duration_ms: None,
        prompt_len: None,
        prompt_hash: None,
        tokens: Some(TokenSnapshot {
            input: n as u64,
            output: 0,
            cache_read: 0,
            cache_creation: 0,
        }),
        dedup_key: None,
    }
}

fn manual_ctx(tmp: &tempfile::TempDir) -> AppContext {
    // discover 需要 git 根：临时目录初始化为 git 仓库
    ailoom::gitx::git_init(tmp.path(), false).unwrap();
    let data = tmp.path().join("data");
    let workspace_root = tmp.path().canonicalize().unwrap();
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
fn rotate_archives_and_aggregation_keeps_cumulative_values() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = manual_ctx(&tmp);
    ailoom::paths::ensure_layout(&ctx.layout).unwrap();
    // 写入事件（两条累计快照）
    ailoom::events::store::append_event(&ctx.layout.events_file, &ev(&ctx, "s1", 1)).unwrap();
    ailoom::events::store::append_event(&ctx.layout.events_file, &ev(&ctx, "s1", 2)).unwrap();

    // 轮转：阈值极小触发归档
    let args = DataArgs {
        action: "rotate".into(),
        out: None,
        max_size_mb: 0.000001,
        dry_run: false,
        root: Some(tmp.path().to_path_buf()),
    };
    let v = data_run(&args, true, Some(&tmp.path().join("data"))).unwrap();
    assert_eq!(v["rotated"], true);
    let archive = v["archive"].as_str().unwrap().to_string();
    // 聚合读取全部 events*.jsonl：会话累计值不丢
    let (events, bad) = ailoom::events::store::read_events(Path::new(&archive)).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(bad, 0);
    let m = ailoom::events::aggregate::aggregate_session(
        &ctx.workspace.workspace_id,
        "s1",
        &events,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(m.tokens.input, Some(2), "轮转不丢会话累计值");
}

#[test]
fn export_is_reimportable_and_excludes_nothing_needed() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = manual_ctx(&tmp);
    ailoom::paths::ensure_layout(&ctx.layout).unwrap();
    ailoom::events::store::append_event(&ctx.layout.events_file, &ev(&ctx, "s1", 5)).unwrap();
    let out = tmp.path().join("export-dir");
    let args = DataArgs {
        action: "export".into(),
        out: Some(out.clone()),
        max_size_mb: 10.0,
        dry_run: false,
        root: Some(tmp.path().to_path_buf()),
    };
    let v = data_run(&args, true, Some(&tmp.path().join("data"))).unwrap();
    let exported: Vec<String> = v["exported"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(
        exported.iter().any(|p| p.ends_with("session-metrics.json")),
        "{exported:?}"
    );
    // 导出可重新读入审计
    let metrics: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("session-metrics.json")).unwrap())
            .unwrap();
    assert_eq!(metrics["sessions"][0]["session_id"], "s1");
}

#[test]
fn cleanup_refuses_when_pending_report_and_scoped_by_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = manual_ctx(&tmp);
    ailoom::paths::ensure_layout(&ctx.layout).unwrap();
    // 模拟待上报批次
    let pending = ctx.layout.ws_dir.join("report-checkpoint.json");
    std::fs::write(
        &pending,
        r#"{"schema_version":1,"pushed_batches":[],"pending":{"b1":{}}}"#,
    )
    .unwrap();
    let args = DataArgs {
        action: "cleanup".into(),
        out: None,
        max_size_mb: 10.0,
        dry_run: false,
        root: Some(tmp.path().to_path_buf()),
    };
    let err = data_run(&args, true, Some(&tmp.path().join("data"))).unwrap_err();
    assert_eq!(err.code, "E8001", "未确认上报批次拒绝清理");
    // dry_run 模式同样拒绝（防丢优先）
    let args = DataArgs {
        dry_run: true,
        ..args
    };
    assert!(data_run(&args, true, Some(&tmp.path().join("data"))).is_err());
}

#[test]
fn cleanup_only_touches_own_cache_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = manual_ctx(&tmp);
    ailoom::paths::ensure_layout(&ctx.layout).unwrap();
    // 两个缓存目录：A 归属当前源，B 归属其它
    let cache = ctx.data_root.join("cache");
    let a = cache.join("aaaaaaaaaaaaaaaa");
    let b = cache.join("bbbbbbbbbbbbbbbb");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let args = DataArgs {
        action: "cleanup".into(),
        out: None,
        max_size_mb: 10.0,
        dry_run: true,
        root: Some(tmp.path().to_path_buf()),
    };
    let v = data_run(&args, true, Some(&tmp.path().join("data"))).unwrap();
    let would: Vec<String> = v["would_remove"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(would.iter().any(|p| p.contains("aaaaaaaaaaaaaaaa")), "{v}");
    assert!(would.iter().any(|p| p.contains("bbbbbbbbbbbbbbbb")), "{v}");
    // dry_run 不删除
    assert!(a.is_dir() && b.is_dir());
}
