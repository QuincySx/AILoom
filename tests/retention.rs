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
    // discover 需要 git 根：临时目录初始化为 git 仓库；
    // 直接用 discover 保证 workspace_id/anchor_key 与 data_run/逐入口完全一致
    ailoom::gitx::git_init(tmp.path(), false).unwrap();
    AppContext::discover(Some(&tmp.path().join("data")), tmp.path(), Some(tmp.path())).unwrap()
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
    assert!(Path::new(&archive).is_file(), "归档文件存在: {archive}");

    // 轮转后统一读取入口（活动+归档合并、去重）：历史事件不丢
    let (events, bad) = ailoom::events::store::read_all_events(&ctx.layout.events_dir).unwrap();
    assert_eq!(events.len(), 2, "轮转后统一读取应包含归档历史");
    assert_eq!(bad, 0);

    // 正常 session 入口（metrics）轮转后会话与累计值不变（不直接传归档路径绕过入口）
    let metrics = ailoom::commands::session::run(
        &ailoom::commands::session::SessionArgs {
            action: "metrics".into(),
            session: Some("s1".into()),
            file: None,
            share: None,
            root: Some(tmp.path().to_path_buf()),
        },
        true,
        Some(&tmp.path().join("data")),
    )
    .unwrap_or_else(|e| panic!("session metrics 失败: {e}"));
    assert_eq!(metrics["session_id"], "s1", "{metrics}");
    assert_eq!(
        metrics["tokens"]["input"], 2,
        "轮转不丢会话累计值: {metrics}"
    );
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
    // 两个缓存目录：A 归属当前源 anchor，B 归属其它源
    let cache = ctx.data_root.join("cache");
    let a = cache.join(&ctx.workspace.anchor_key);
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
    assert!(
        would
            .iter()
            .any(|p| p.contains(ctx.workspace.anchor_key.as_str())),
        "own anchor cache 应在范围内: {v}"
    );
    assert!(
        !would.iter().any(|p| p.contains("bbbbbbbbbbbbbbbb")),
        "其他工作区/其他 anchor 的缓存不得进入清理范围: {v}"
    );
    // dry_run 不删除
    assert!(a.is_dir() && b.is_dir());
}

// ---------- AIL-037 返工回归（R06/R07） ----------

use ailoom::data::{run as data_run2, DataArgs as DataArgs2};

fn data_args(tmp: &std::path::Path, action: &str, dry_run: bool) -> DataArgs2 {
    DataArgs2 {
        action: action.into(),
        out: None,
        max_size_mb: 10.0,
        dry_run,
        root: Some(tmp.to_path_buf()),
    }
}

fn write_events(ctx: &AppContext, ids: &[&str]) {
    for (i, id) in ids.iter().enumerate() {
        let mut e = ev(ctx, "s1", i + 1);
        e.event_id = (*id).into();
        ailoom::events::store::append_event(&ctx.layout.events_file, &e).unwrap();
    }
}

/// 轮转后重投旧事件不翻倍；连续轮转归档名唯一、追加事件不丢。
#[test]
fn rotate_reinjection_no_double_count_and_unique_archives() {
    let tmp = tempfile::tempdir().unwrap();
    ailoom::gitx::git_init(tmp.path(), false).unwrap();
    let ctx =
        AppContext::discover(Some(&tmp.path().join("data")), tmp.path(), Some(tmp.path())).unwrap();
    ailoom::paths::ensure_layout(&ctx.layout).unwrap();
    write_events(&ctx, &["e1", "e2"]);

    // 轮转 1：阈值极小
    let mut args = data_args(tmp.path(), "rotate", false);
    args.max_size_mb = 0.000001;
    let v = data_run2(&args, true, Some(&tmp.path().join("data"))).unwrap();
    assert_eq!(v["rotated"], true);

    // 重投旧事件 e1（append_event 的 event_id 去重只看活动文件；统一读取层再次去重）
    let mut dup = ev(&ctx, "s1", 1);
    dup.event_id = "e1".into();
    ailoom::events::store::append_event(&ctx.layout.events_file, &dup).unwrap();
    let mut fresh = ev(&ctx, "s1", 9);
    fresh.event_id = "e9".into();
    ailoom::events::store::append_event(&ctx.layout.events_file, &fresh).unwrap();

    // 轮转 2（连续）：归档不覆盖，追加事件进入新活动文件
    let v2 = data_run2(&args, true, Some(&tmp.path().join("data"))).unwrap();
    assert_eq!(v2["rotated"], true);
    assert_ne!(
        v["archive"].as_str().unwrap(),
        v2["archive"].as_str().unwrap()
    );

    // 统一读取：e1 只出现一次（去重），e2/e9 都在，历史无丢失
    let (events, bad) = ailoom::events::store::read_all_events(&ctx.layout.events_dir).unwrap();
    assert_eq!(bad, 0);
    let ids: Vec<&str> = events.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(
        ids.iter().filter(|i| **i == "e1").count(),
        1,
        "重投不翻倍: {ids:?}"
    );
    for want in ["e1", "e2", "e9"] {
        assert!(ids.contains(&want), "缺少 {want}: {ids:?}");
    }
    // 归档文件两份都存在
    assert!(std::path::Path::new(v["archive"].as_str().unwrap()).is_file());
    assert!(std::path::Path::new(v2["archive"].as_str().unwrap()).is_file());
}

/// 两工作区共享数据根：cleanup A 不动 B；同 anchor 的另一 worktree 缓存被保护；未上报归档不删。
#[test]
fn cleanup_scope_and_unreported_protection() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    // 工作区 A、B：两个独立 git 仓（不同 anchor）
    let wsa = tmp.path().join("ws-a");
    let wsb = tmp.path().join("ws-b");
    ailoom::gitx::git_init(&wsa, false).unwrap();
    ailoom::gitx::git_init(&wsb, false).unwrap();
    let ctx_a = AppContext::discover(Some(&data), &wsa, Some(&wsa)).unwrap();
    let ctx_b = AppContext::discover(Some(&data), &wsb, Some(&wsb)).unwrap();
    assert_ne!(ctx_a.workspace.anchor_key, ctx_b.workspace.anchor_key);
    ailoom::paths::ensure_layout(&ctx_a.layout).unwrap();
    ailoom::paths::ensure_layout(&ctx_b.layout).unwrap();

    // 各自缓存与事件
    std::fs::create_dir_all(&ctx_a.layout.cache_root).unwrap();
    std::fs::write(ctx_a.layout.cache_root.join("blob"), b"a").unwrap();
    std::fs::create_dir_all(&ctx_b.layout.cache_root).unwrap();
    std::fs::write(ctx_b.layout.cache_root.join("blob"), b"b").unwrap();
    write_events(&ctx_a, &["a-e1"]);
    write_events(&ctx_b, &["b-e1"]);

    // A 轮转产生归档（未上报 → 无确认水位线）
    let mut rot = data_args(&wsa, "rotate", false);
    rot.max_size_mb = 0.000001;
    let v = data_run2(&rot, true, Some(&data)).unwrap();
    assert_eq!(v["rotated"], true);
    let archive_path = v["archive"].as_str().unwrap().to_string();

    // cleanup A（dry-run 与执行一致）
    let dry = data_run2(&data_args(&wsa, "cleanup", true), true, Some(&data)).unwrap();
    let would: Vec<String> = dry["would_remove"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(
        !would
            .iter()
            .any(|p| p.contains(&ctx_b.workspace.anchor_key)),
        "B 的缓存不得进入 A 的清理计划: {dry}"
    );
    assert!(
        !would.iter().any(|p| p.starts_with(archive_path.as_str())),
        "未上报归档不得进入清理计划: {dry}"
    );

    let exec = data_run2(&data_args(&wsa, "cleanup", false), true, Some(&data)).unwrap();
    let removed: Vec<String> = exec["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert_eq!(removed, would, "dry-run 计划与执行必须一致");

    // B 完整：缓存逐字节不变、事件可读
    assert!(ctx_b.layout.cache_root.join("blob").is_file(), "B 缓存保留");
    assert_eq!(
        std::fs::read(ctx_b.layout.cache_root.join("blob")).unwrap(),
        b"b"
    );
    let (b_events, _) = ailoom::events::store::read_all_events(&ctx_b.layout.events_dir).unwrap();
    assert_eq!(b_events.len(), 1, "B 事件完整");

    // A：未上报归档保留；缓存已清
    assert!(
        std::path::Path::new(&archive_path).is_file(),
        "未上报归档不得被删"
    );
    assert!(!ctx_a.layout.cache_root.exists(), "自己的缓存已清");

    // 确认上报后（checkpoint 水位线覆盖全部事件）再 cleanup：归档可清
    let confirmed: Vec<String> = {
        let (events, _) = ailoom::events::store::read_all_events(&ctx_a.layout.events_dir).unwrap();
        events.iter().map(|e| e.event_id.clone()).collect()
    };
    let cp = serde_json::json!({
        "schema_version": 1,
        "pushed_batches": ["b1"],
        "pending": {},
        "confirmed_event_ids": confirmed,
    });
    std::fs::write(
        ctx_a.layout.ws_dir.join("report-checkpoint.json"),
        serde_json::to_vec_pretty(&cp).unwrap(),
    )
    .unwrap();
    let v = data_run2(&data_args(&wsa, "cleanup", false), true, Some(&data)).unwrap();
    let removed: Vec<String> = v["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(
        removed.contains(&archive_path),
        "确认上传后归档才可清理: {v}"
    );
    assert!(!std::path::Path::new(&archive_path).is_file());
}

/// 同 anchor 两个 worktree：cleanup A 跳过共享缓存（B 离线仍可读锁定内容）。
#[test]
fn cleanup_skips_cache_shared_by_sibling_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let wsa = tmp.path().join("ws-a");
    ailoom::gitx::git_init(&wsa, false).unwrap();
    ailoom::gitx::git(
        &wsa,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    )
    .unwrap();
    let ctx_a = AppContext::discover(Some(&data), &wsa, Some(&wsa)).unwrap();
    ailoom::paths::ensure_layout(&ctx_a.layout).unwrap();
    std::fs::create_dir_all(&ctx_a.layout.cache_root).unwrap();
    std::fs::write(ctx_a.layout.cache_root.join("locked-content"), b"offline").unwrap();

    // 同仓 worktree B（同 anchor）
    let wsb = tmp.path().join("ws-b");
    ailoom::gitx::git(
        &wsa,
        &["worktree", "add", "-q", wsb.to_str().unwrap(), "-b", "feat"],
    )
    .unwrap();
    let ctx_b = AppContext::discover(Some(&data), &wsb, Some(&wsb)).unwrap();
    assert_eq!(ctx_a.workspace.anchor_key, ctx_b.workspace.anchor_key);
    // 模拟 init 写入 B 的绑定（共享引用检测读取 binding.json 的 repository_anchor）
    std::fs::create_dir_all(ctx_b.layout.binding_path.parent().unwrap()).unwrap();
    std::fs::write(
        &ctx_b.layout.binding_path,
        serde_json::json!({
            "schema_version": 1,
            "workspace_root": wsb.display().to_string(),
            "repository_anchor": ctx_a.workspace.repository_anchor,
            "workspace_id": ctx_b.workspace.workspace_id,
        })
        .to_string(),
    )
    .unwrap();

    let v = data_run2(&data_args(&wsa, "cleanup", true), true, Some(&data)).unwrap();
    assert_eq!(v["skipped_shared_cache"], 1, "{v}");
    let would: Vec<String> = v["would_remove"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(
        !would
            .iter()
            .any(|p| p.contains(&ctx_a.workspace.anchor_key)),
        "共享缓存不得进入清理计划: {v}"
    );
    let _ = data_run2(&data_args(&wsa, "cleanup", false), true, Some(&data)).unwrap();
    assert_eq!(
        std::fs::read(ctx_b.layout.cache_root.join("locked-content")).unwrap(),
        b"offline",
        "B 离线仍能读取锁定内容"
    );
}

/// 导出含活动+归档与聚合基线；坏行文件跳过报告不清删；dry-run 无副作用。
#[test]
fn export_includes_archives_and_bad_lines_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let ws = tmp.path().join("ws");
    ailoom::gitx::git_init(&ws, false).unwrap();
    let ctx = AppContext::discover(Some(&data), &ws, Some(&ws)).unwrap();
    ailoom::paths::ensure_layout(&ctx.layout).unwrap();
    write_events(&ctx, &["e1"]);

    // 轮转出归档 + 手工加坏行
    let mut rot = data_args(&ws, "rotate", false);
    rot.max_size_mb = 0.000001;
    data_run2(&rot, true, Some(&data)).unwrap();
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.layout.events_file)
        .unwrap();
    writeln!(f, "{{not-json").unwrap();
    drop(f);

    let out = tmp.path().join("export-dir");
    let mut args = data_args(&ws, "export", false);
    args.out = Some(out.clone());
    let v = data_run2(&args, true, Some(&data)).unwrap();
    let exported: Vec<String> = v["exported"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(
        exported.iter().any(|p| p.ends_with("session-metrics.json")),
        "聚合基线导出: {exported:?}"
    );
    assert!(
        exported.iter().any(|p| p.contains("events-archive-")),
        "归档导出: {exported:?}"
    );
    // 聚合基线基于活动+归档：e1 在内（坏行只计数）
    let metrics: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("session-metrics.json")).unwrap())
            .unwrap();
    assert_eq!(metrics["bad_event_lines"], 1, "{metrics}");
    assert_eq!(metrics["sessions"][0]["session_id"], "s1");
}

/// RW-09/R06：采集→轮转→受控报告确认→cleanup 后，正常 session 入口的累计值
/// 与清理前一致（基线消费，不读归档）；重投已清理事件不重复计数；新增事件
/// 正确累加；基线损坏时拒绝破坏性清理并可重试。
#[test]
fn cleanup_persists_cumulative_baseline_and_entries_stay_consistent() {
    let tmp = tempfile::tempdir().unwrap();
    ailoom::gitx::git_init(tmp.path(), false).unwrap();
    let ctx =
        AppContext::discover(Some(&tmp.path().join("data")), tmp.path(), Some(tmp.path())).unwrap();
    ailoom::paths::ensure_layout(&ctx.layout).unwrap();

    let mk_prompt = |ctx: &AppContext, id: &str, n: u64| {
        let mut e = ev(ctx, "s1", n as usize);
        e.event_id = id.into();
        e.kind = "prompt".into();
        ailoom::events::store::append_event(&ctx.layout.events_file, &e).unwrap();
    };
    let metrics = || -> serde_json::Value {
        ailoom::commands::session::run(
            &ailoom::commands::session::SessionArgs {
                action: "metrics".into(),
                session: Some("s1".into()),
                file: None,
                share: None,
                root: Some(tmp.path().to_path_buf()),
            },
            true,
            Some(&tmp.path().join("data")),
        )
        .unwrap_or_else(|e| panic!("session metrics 失败: {e}"))
    };

    mk_prompt(&ctx, "e1", 1);
    mk_prompt(&ctx, "e2", 2);
    assert_eq!(metrics()["prompt_count"], 2, "清理前累计");

    // 轮转：e1/e2 进入归档
    let mut rot = data_args(tmp.path(), "rotate", false);
    rot.max_size_mb = 0.000001;
    let v = data_run2(&rot, true, Some(&tmp.path().join("data"))).unwrap();
    assert_eq!(v["rotated"], true);

    // 受控报告确认（写确认水位线：e1/e2/e3 已确认）
    mk_prompt(&ctx, "e3", 3);
    let cp = serde_json::json!({
        "schema_version": 1,
        "pushed_batches": ["b-confirm"],
        "pending": {},
        "confirmed_event_ids": ["e1", "e2", "e3"],
    });
    std::fs::write(
        ctx.layout.ws_dir.join("report-checkpoint.json"),
        serde_json::to_string_pretty(&cp).unwrap(),
    )
    .unwrap();

    // cleanup：删除已确认归档（删除前基线持久化）
    let cl = data_args(tmp.path(), "cleanup", false);
    let v = data_run2(&cl, true, Some(&tmp.path().join("data"))).unwrap();
    assert!(
        !v["removed"].as_array().unwrap().is_empty(),
        "归档应被清理: {v}"
    );

    // 清理后正常 session 入口：累计一致（基线 2 + 实时 e3 1 = 3）
    let after = metrics();
    assert_eq!(after["prompt_count"], 3, "清理后累计一致: {after}");

    // 重投已清理事件（同 id、不同内容）→ 不重复计数
    mk_prompt(&ctx, "e1", 7);
    let after2 = metrics();
    assert_eq!(after2["prompt_count"], 3, "重投已清算事件不重复: {after2}");

    // 新事件 → 正确累加（基线 2 + 实时 e3/e4 2 = 4）
    mk_prompt(&ctx, "e4", 9);
    let after3 = metrics();
    assert_eq!(after3["prompt_count"], 4, "新增事件累加: {after3}");

    // 基线损坏：再轮转出新的已确认归档后，cleanup 拒绝（可修复重试）
    mk_prompt(&ctx, "e5", 5);
    let mut rot2 = data_args(tmp.path(), "rotate", false);
    rot2.max_size_mb = 0.000001;
    let v = data_run2(&rot2, true, Some(&tmp.path().join("data"))).unwrap();
    assert_eq!(v["rotated"], true);
    let cp = serde_json::json!({
        "schema_version": 1,
        "pushed_batches": ["b-confirm"],
        "pending": {},
        "confirmed_event_ids": ["e1", "e2", "e3", "e4", "e5"],
    });
    std::fs::write(
        ctx.layout.ws_dir.join("report-checkpoint.json"),
        serde_json::to_string_pretty(&cp).unwrap(),
    )
    .unwrap();
    std::fs::write(ctx.layout.ws_dir.join("metrics-baseline.json"), "{corrupt").unwrap();
    let err = data_run2(
        &data_args(tmp.path(), "cleanup", false),
        true,
        Some(&tmp.path().join("data")),
    );
    assert!(err.is_err(), "基线损坏必须拒绝破坏性清理");
    // 修复基线后重试成功
    let identities = serde_json::json!({
        "schema_version": 1,
        "identities": {},
        "accounted_event_ids": ["e1", "e2", "e3", "e4"],
    });
    std::fs::write(
        ctx.layout.ws_dir.join("metrics-baseline.json"),
        serde_json::to_string_pretty(&identities).unwrap(),
    )
    .unwrap();
    let v = data_run2(
        &data_args(tmp.path(), "cleanup", false),
        true,
        Some(&tmp.path().join("data")),
    )
    .unwrap_or_else(|e| panic!("修复后 cleanup 应成功: {e}"));
    assert!(!v["removed"].as_array().unwrap().is_empty(), "{v}");
}
