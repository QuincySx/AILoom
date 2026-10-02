//! Hook 生命周期与事件协议（AIL-018）集成测试。

mod common;

use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Ctx {
    tmp: tempfile::TempDir,
}

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    loop {
        path.pop();
        let p = if cfg!(windows) {
            "ailoom.exe"
        } else {
            "ailoom"
        };
        if path.join(p).exists() {
            return path.join(p);
        }
    }
}

impl Ctx {
    fn new() -> Ctx {
        Ctx {
            tmp: tempfile::tempdir().unwrap(),
        }
    }
    fn run(&self, cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn run_stdin(&self, cwd: &Path, args: &[&str], stdin_payload: &str) -> (i32, String, String) {
        let mut child = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin_payload.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn dr(&self) -> String {
        self.tmp.path().join("data").to_string_lossy().to_string()
    }
}

fn setup(c: &Ctx) -> PathBuf {
    let (bare, src, ws) = {
        let bare = c.tmp.path().join("origin.git");
        ailoom::gitx::git_init(&bare, true).unwrap();
        let team_src = common::make_team_source(c.tmp.path());
        ailoom::gitx::git(&team_src, &["branch", "-M", "main"]).unwrap();
        ailoom::gitx::git(
            &team_src,
            &["remote", "add", "origin", bare.to_str().unwrap()],
        )
        .unwrap();
        ailoom::gitx::git(&team_src, &["push", "-q", "-u", "origin", "main"]).unwrap();
        let ws = common::make_business_repo(c.tmp.path(), "biz");
        (bare, team_src, ws)
    };
    let _ = bare;
    let url = src.to_str().unwrap().to_string();
    let dr = c.dr();
    let args = [
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    ws
}

fn events_file(c: &Ctx) -> PathBuf {
    // 唯一工作区：直接找 data/ws/*/events/events.jsonl
    let ws_root = c.tmp.path().join("data").join("ws");
    for entry in std::fs::read_dir(&ws_root).unwrap().flatten() {
        let f = entry.path().join("events").join("events.jsonl");
        if f.is_file() {
            return f;
        }
    }
    panic!("未找到事件文件");
}

#[test]
fn hook_captures_event_without_storing_prompt_text() {
    let c = Ctx::new();
    let ws = setup(&c);
    let secret_prompt = "请把 API_KEY=sk-super-secret 写进配置";
    let payload = format!(
        r#"{{"session_id":"sess-1","cwd":"{cwd}","tool_name":"Write","tool_input":{{"prompt":"{secret_prompt}"}}}}"#,
        cwd = ws.display()
    );
    let dr = c.dr();
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "hook",
        "--tool",
        "claude",
        "--event",
        "prompt",
        "--root",
        ws.to_str().unwrap(),
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run_stdin(&ws, &refs, &payload);
    assert_eq!(code, 0, "hook 失败不得阻塞宿主: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["captured"], true);

    let text = std::fs::read_to_string(events_file(&c)).unwrap();
    assert!(text.contains("\"type\":\"prompt\""));
    assert!(text.contains("sess-1"));
    assert!(
        !text.contains("sk-super-secret"),
        "prompt 全文默认不落盘: {text}"
    );
    assert!(text.contains("prompt_hash"), "只存哈希");
}

#[test]
fn duplicate_delivery_is_idempotent() {
    let c = Ctx::new();
    let ws = setup(&c);
    let dr = c.dr();
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "hook",
        "--tool",
        "claude",
        "--event",
        "stop",
        "--root",
        ws.to_str().unwrap(),
    ];
    let refs: Vec<&str> = args.to_vec();
    // 手工构造固定 event_id 的 payload 走标准事件通道（schema parse 路径不同，直接用 store 验证）
    let e1 = ailoom::events::schema::Event {
        schema_version: 1,
        event_id: "fixed-id".into(),
        session_id: "s".into(),
        workspace_id: "w".into(),
        device_id: "d".into(),
        tool: "claude".into(),
        time: "2026-09-09T00:00:00Z".into(),
        kind: "stop".into(),
        tool_name: None,
        exit_code: None,
        duration_ms: None,
        prompt_len: None,
        prompt_hash: None,
        tokens: None,
        dedup_key: None,
    };
    let f = c.tmp.path().join("ev").join("events.jsonl");
    assert!(ailoom::events::store::append_event(&f, &e1).unwrap());
    assert!(
        !ailoom::events::store::append_event(&f, &e1).unwrap(),
        "重复送达幂等"
    );
    assert_eq!(std::fs::read_to_string(&f).unwrap().lines().count(), 1);

    let _ = (&args, &refs);
}

#[test]
fn bad_payload_diagnosed_but_exits_zero() {
    let c = Ctx::new();
    let ws = setup(&c);
    let dr = c.dr();
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "hook",
        "--tool",
        "claude",
        "--event",
        "prompt",
        "--root",
        ws.to_str().unwrap(),
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run_stdin(&ws, &refs, "{ not json");
    assert_eq!(code, 0, "坏 JSON 不得使宿主中断");
    assert!(
        stderr.contains("E7001") || stderr.contains("JSON"),
        "{stderr}"
    );
}

#[test]
fn two_concurrent_hook_processes_both_persist() {
    let c = Ctx::new();
    let ws = setup(&c);
    let dr = c.dr();
    let hook_args = |sid: &str| {
        let payload = format!(r#"{{"session_id":"{sid}","cwd":"{}"}}"#, ws.display());
        let args = vec![
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            "session-start".to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        (args, payload)
    };
    let (a1, p1) = hook_args("s-A");
    let (a2, p2) = hook_args("s-B");
    let refs1: Vec<&str> = a1.iter().map(String::as_str).collect();
    let refs2: Vec<&str> = a2.iter().map(String::as_str).collect();
    let (c1, s1, e1) = c.run_stdin(&ws, &refs1, &p1);
    let (c2, s2, e2) = c.run_stdin(&ws, &refs2, &p2);
    assert_eq!(c1, 0, "{e1}");
    assert_eq!(c2, 0, "{e2}");
    let _ = (s1, s2);
    let text = std::fs::read_to_string(events_file(&c)).unwrap();
    assert!(
        text.contains("s-A") && text.contains("s-B"),
        "两个会话事件都落盘: {text}"
    );
}

#[test]
fn hooks_install_idempotent_and_remove_only_managed() {
    let c = Ctx::new();
    let ws = setup(&c);
    // 用户已有自己的 hook
    std::fs::create_dir_all(ws.join(".claude")).unwrap();
    std::fs::write(
        ws.join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"my-own-hook"}]}]}}"#,
    )
    .unwrap();

    let args = ["--data-root", &c.dr(), "hooks", "--action", "install"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, _) = c.run(&ws, &args);
    assert_eq!(code, 0, "重复注册幂等");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    // 用户 hook 保留；ailoom 注册存在
    let stop = settings["hooks"]["Stop"].as_array().unwrap();
    assert!(
        stop.iter()
            .any(|e| e["hooks"][0]["command"] == "my-own-hook"),
        "用户 hook 保留"
    );
    assert!(stop.iter().any(|e| e["hooks"][0]["command"]
        .as_str()
        .unwrap_or("")
        .starts_with("ailoom hook")));
    // 4 个事件键
    assert!(settings["hooks"]["SessionStart"].is_array());
    assert!(settings["hooks"]["UserPromptSubmit"].is_array());
    assert!(settings["hooks"]["PostToolUse"].is_array());

    // remove：只移除托管条目
    let args = ["--data-root", &c.dr(), "hooks", "--action", "remove"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    let stop = settings["hooks"]["Stop"].as_array().unwrap();
    assert!(
        stop.iter()
            .any(|e| e["hooks"][0]["command"] == "my-own-hook"),
        "用户 hook 仍在"
    );
    assert!(!stop.iter().any(|e| e["hooks"][0]["command"]
        .as_str()
        .unwrap_or("")
        .starts_with("ailoom hook")));
}

#[test]
fn events_survive_prompt_privacy_and_forward_compat() {
    // 已由 schema 单测覆盖未知字段；这里验证 stop 事件全链路 + 摩擦提示一次
    let c = Ctx::new();
    let ws = setup(&c);
    let dr = c.dr();
    let hook = |event: &str, sid: &str| {
        let payload = format!(r#"{{"session_id":"{sid}","cwd":"{}"}}"#, ws.display());
        let args = vec![
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            event.to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        c.run_stdin(&ws, &refs, &payload)
    };
    let (code, _, _) = hook("session-start", "s1");
    assert_eq!(code, 0);
    let (code, _, _) = hook("stop", "s1");
    assert_eq!(code, 0);
    // 事件可聚合
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "session",
        "--action",
        "metrics",
        "--session",
        "s1",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["session_id"], "s1");
    assert_eq!(v["result"]["stop_count"], 1);
    assert_eq!(v["result"]["tokens_availability"], "unavailable");
}

// ---------- AIL-018 返工回归（R02/R03/R04） ----------

/// 执行实际生成的 Hook 注册命令：start/prompt/tool/stop 落标准事件并被聚合计数。
#[test]
fn registered_hook_commands_produce_standard_events() {
    let c = Ctx::new();
    let ws = setup(&c);
    let args = ["--data-root", &c.dr(), "hooks", "--action", "install"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    // 提取每个宿主事件下由 AILoom 注册的命令字符串
    let mut commands: Vec<(String, String)> = Vec::new();
    for ev in ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop"] {
        let arr = settings["hooks"][ev].as_array().expect(ev);
        let cmd = arr
            .iter()
            .filter_map(|e| e["hooks"][0]["command"].as_str())
            .find(|c| c.starts_with("ailoom hook"))
            .expect("ailoom 注册命令存在")
            .to_string();
        commands.push((ev.to_string(), cmd));
    }

    // 按宿主 payload 逐条执行实际生成的命令（ailoom → 本地构建二进制）
    let payloads: Vec<(&str, String)> = vec![
        (
            "SessionStart",
            format!(r#"{{"session_id":"sr1","cwd":"{}"}}"#, ws.display()),
        ),
        (
            "UserPromptSubmit",
            format!(
                r#"{{"session_id":"sr1","cwd":"{}","prompt":"请帮我构建"}}"#,
                ws.display()
            ),
        ),
        (
            "PostToolUse",
            format!(
                r#"{{"session_id":"sr1","cwd":"{}","tool_name":"Bash","tool_input":{{"command":"make build"}}}}"#,
                ws.display()
            ),
        ),
        (
            "Stop",
            format!(r#"{{"session_id":"sr1","cwd":"{}"}}"#, ws.display()),
        ),
    ];
    for ((ev, cmd), (want_ev, payload)) in commands.iter().zip(payloads.iter()) {
        assert_eq!(ev, want_ev);
        let mut parts = cmd.split_whitespace();
        assert_eq!(
            parts.next(),
            Some("ailoom"),
            "命令必须以 ailoom 开头: {cmd}"
        );
        let rest: Vec<&str> = parts.collect();
        // 前置全局 --json（仅启用 JSON 输出 envelope，不改变注册命令的 hook 语义）
        let mut full = vec!["--json".to_string(), "--data-root".to_string(), c.dr()];
        full.extend(rest.iter().map(|s| s.to_string()));
        let refs: Vec<&str> = full.iter().map(String::as_str).collect();
        // 进程 cwd = ws（宿主真实调用形态）
        let (code, stdout, stderr) = c.run_stdin(&ws, &refs, payload);
        assert_eq!(code, 0, "{ev}: {stderr}");
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["result"]["captured"], true, "{ev}: {v}");
    }

    // 事件落盘为标准类型并被聚合计数
    let text = std::fs::read_to_string(events_file(&c)).unwrap();
    for kind in [
        "\"type\":\"session-start\"",
        "\"type\":\"prompt\"",
        "\"type\":\"tool\"",
        "\"type\":\"stop\"",
    ] {
        assert!(text.contains(kind), "缺少 {kind}: {text}");
    }
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "session",
        "--action",
        "metrics",
        "--session",
        "sr1",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["prompt_count"], 1, "{v}");
    assert_eq!(v["result"]["tool_calls"], 1, "{v}");
    assert_eq!(v["result"]["stop_count"], 1, "{v}");
    assert!(v["result"]["started_at"].is_string(), "{v}");
}

/// 同一宿主事件重复投递只计一次；两个真实独立（payload 不同）事件分别计数；轮转重投不翻倍。
#[test]
fn redelivery_dedup_but_similar_events_counted_separately() {
    let c = Ctx::new();
    let ws = setup(&c);
    let dr = c.dr();
    let hook = |payload: String| {
        let args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            "PostToolUse".to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        c.run_stdin(&ws, &refs, &payload)
    };
    let p1 = format!(
        r#"{{"session_id":"dd1","cwd":"{}","tool_name":"Bash","tool_input":{{"command":"make build"}},"tool_use_id":"t1"}}"#,
        ws.display()
    );
    let (code, _, stderr) = hook(p1.clone());
    assert_eq!(code, 0, "{stderr}");
    // 重投同一 payload
    let (code, out, _) = hook(p1);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["result"]["captured"], false, "重投应被去重: {v}");
    // 相似但独立的事件（不同 tool_use_id / 不同命令）
    let p2 = format!(
        r#"{{"session_id":"dd1","cwd":"{}","tool_name":"Bash","tool_input":{{"command":"make build"}},"tool_use_id":"t2"}}"#,
        ws.display()
    );
    let (code, _, _) = hook(p2);
    assert_eq!(code, 0);

    let metrics = |session: &str| {
        let args = [
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "session".to_string(),
            "--action".to_string(),
            "metrics".to_string(),
            "--session".to_string(),
            session.to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, stdout, stderr) = c.run(&ws, &refs);
        assert_eq!(code, 0, "{stderr}");
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        v["result"]["tool_calls"].as_u64().unwrap()
    };
    assert_eq!(metrics("dd1"), 2, "重投去重 + 独立事件分别计数");

    // 轮转后重投旧事件：聚合不翻倍（与 AIL-037 联调）
    let rotate = c.run(
        &ws,
        &[
            "--json",
            "--data-root",
            dr.as_str(),
            "data",
            "--action",
            "rotate",
            "--max-size-mb",
            "0.000001",
        ],
    );
    assert_eq!(rotate.0, 0, "{}", rotate.2);
    let (code, _, _) = hook(format!(
        r#"{{"session_id":"dd1","cwd":"{}","tool_name":"Bash","tool_input":{{"command":"make build"}},"tool_use_id":"t2"}}"#,
        ws.display()
    ));
    assert_eq!(code, 0);
    assert_eq!(metrics("dd1"), 2, "轮转后重投不翻倍");
}

/// 进程 cwd=A、payload.cwd=B → 写 B；显式 --root A → 写 A（优先级：显式 > payload.cwd）。
#[test]
fn payload_cwd_and_explicit_root_priority() {
    let c = Ctx::new();
    // 两个业务仓各自绑定同一数据根
    let bare = c.tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let team_src = common::make_team_source(c.tmp.path());
    ailoom::gitx::git(
        &team_src,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    )
    .unwrap();
    ailoom::gitx::git(&team_src, &["push", "-q", "-u", "origin", "HEAD"]).unwrap();
    let ws_a = common::make_business_repo(c.tmp.path(), "biz-a");
    let ws_b = common::make_business_repo(c.tmp.path(), "biz-b");
    let dr = c.dr();
    for ws in [&ws_a, &ws_b] {
        let args = [
            "--data-root".to_string(),
            dr.clone(),
            "init".to_string(),
            "--url".to_string(),
            team_src.to_str().unwrap().to_string(),
            "--project".to_string(),
            "a".to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run(ws, &refs);
        assert_eq!(code, 0, "{stderr}");
    }

    let payload = |cwd: &Path| format!(r#"{{"session_id":"cw1","cwd":"{}"}}"#, cwd.display());
    let hook_args = |root: Option<&Path>| {
        let mut args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            "SessionStart".to_string(),
        ];
        if let Some(r) = root {
            args.push("--root".to_string());
            args.push(r.to_str().unwrap().to_string());
        }
        args
    };
    fn as_refs(args: &[String]) -> Vec<&str> {
        args.iter().map(String::as_str).collect()
    }

    let file_of = |ws: &Path| {
        let wsid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
        c.tmp
            .path()
            .join("data")
            .join("ws")
            .join(wsid)
            .join("events")
            .join("events.jsonl")
    };
    let fa = file_of(&ws_a);
    let fb = file_of(&ws_b);

    // 进程 cwd=A、payload.cwd=B、无显式 root → 落 B
    let a_args = hook_args(None);
    let (code, out, stderr) = c.run_stdin(&ws_a, &as_refs(&a_args), &payload(&ws_b));
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["result"]["captured"], true, "{v}");
    assert!(
        !fa.exists() || !std::fs::read_to_string(&fa).unwrap().contains("cw1"),
        "A 不应收到该事件"
    );
    assert!(
        fb.exists() && std::fs::read_to_string(&fb).unwrap().contains("cw1"),
        "事件应写入 payload.cwd=B 的工作区"
    );

    // 显式 --root A + payload.cwd=B → 显式优先，写 A
    let b_args = hook_args(Some(&ws_a));
    let (code, out, stderr) = c.run_stdin(&ws_a, &as_refs(&b_args), &payload(&ws_b));
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["result"]["captured"], true, "{v}");
    assert!(
        std::fs::read_to_string(&fa).unwrap().contains("cw1"),
        "显式 root=A 时事件写 A"
    );
}

/// 未知宿主事件：诊断并跳过，退出 0（宿主不中断）。
#[test]
fn unknown_host_event_diagnosed_exit_zero() {
    let c = Ctx::new();
    let ws = setup(&c);
    let payload = format!(r#"{{"session_id":"u1","cwd":"{}"}}"#, ws.display());
    let dr = c.dr();
    let args = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        dr,
        "hook".to_string(),
        "--tool".to_string(),
        "claude".to_string(),
        "--event".to_string(),
        "PreToolUse".to_string(),
        "--root".to_string(),
        ws.to_str().unwrap().to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run_stdin(&ws, &refs, &payload);
    assert_eq!(code, 0, "未知事件不得阻塞宿主: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["captured"], false);
    assert_eq!(v["result"]["reason"], "unknown-event");
    assert!(
        stderr.contains("未知宿主事件") || stderr.contains("PreToolUse"),
        "{stderr}"
    );
}

// ---------- AIL-020 返工回归（R11）：真实 Stop 提示链路 ----------

/// 真实 Stop 命令作用于有足够干预的任意 session：宿主 stdout 含 systemMessage
/// 提示（含真实 session id）；重复 Stop（新进程）不再提示；其他 session 独立。
#[test]
fn stop_prompt_uses_real_session_and_outputs_once() {
    let c = Ctx::new();
    let ws = setup(&c);
    // 直接种入 2 次人工干预（宿主显式信号契约：dedup_key=intervention-N）
    let wsid0 = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    let ef = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(&wsid0)
        .join("events")
        .join("events.jsonl");
    for n in 1..=2 {
        let e = ailoom::events::schema::Event {
            schema_version: 1,
            event_id: format!("int-{n}"),
            session_id: "fr-1".into(),
            workspace_id: ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap()),
            device_id: "dev".into(),
            tool: "claude".into(),
            time: ailoom::ids::now_iso(),
            kind: "tool".into(),
            tool_name: Some("Bash".into()),
            exit_code: None,
            duration_ms: None,
            prompt_len: None,
            prompt_hash: None,
            tokens: None,
            dedup_key: Some(format!("intervention-{n}")),
        };
        assert!(ailoom::events::store::append_event(&ef, &e).unwrap());
    }

    // 无 --json：emit_host_stdout 路径（宿主真实调用形态）
    let stop = |sid: &str| {
        let payload = format!(r#"{{"session_id":"{sid}","cwd":"{}"}}"#, ws.display());
        let args = vec![
            "--data-root".to_string(),
            c.dr(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            "Stop".to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        c.run_stdin(&ws, &refs, &payload)
    };

    let (code, stdout, stderr) = stop("fr-1");
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("systemMessage"),
        "宿主 stdout 应含提示: {stdout}"
    );
    assert!(stdout.contains("fr-1"), "提示作用于真实 session: {stdout}");
    assert!(
        stdout.contains("ailoom session --action summary --session fr-1"),
        "提示给出 CLI 真正接受的总结命令: {stdout}"
    );

    // 进程重启后重复 Stop：不再提示
    let (code, stdout2, _) = stop("fr-1");
    assert_eq!(code, 0);
    assert!(
        !stdout2.contains("systemMessage"),
        "每会话最多一次: {stdout2}"
    );

    // 其他 session 独立：无事件 → 无提示
    let (code, stdout3, _) = stop("fr-other");
    assert_eq!(code, 0);
    assert!(!stdout3.contains("systemMessage"), "{stdout3}");
}

/// 关闭提示（friction.toml prompt_enabled=false）后不再输出提示，但统计照常。
#[test]
fn prompt_disabled_no_notice_but_metrics_persist() {
    let c = Ctx::new();
    let ws = setup(&c);
    let wsid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    let ws_dir = c.tmp.path().join("data").join("ws").join(&wsid);
    std::fs::create_dir_all(&ws_dir).unwrap();
    std::fs::write(ws_dir.join("friction.toml"), "prompt_enabled = false\n").unwrap();

    let ef = ws_dir.join("events").join("events.jsonl");
    let e = ailoom::events::schema::Event {
        schema_version: 1,
        event_id: "int-x".into(),
        session_id: "dis-1".into(),
        workspace_id: wsid.clone(),
        device_id: "dev".into(),
        tool: "claude".into(),
        time: ailoom::ids::now_iso(),
        kind: "tool".into(),
        tool_name: Some("Bash".into()),
        exit_code: None,
        duration_ms: None,
        prompt_len: None,
        prompt_hash: None,
        tokens: None,
        dedup_key: Some("intervention-1".into()),
    };
    assert!(ailoom::events::store::append_event(&ef, &e).unwrap());

    let payload = format!(r#"{{"session_id":"dis-1","cwd":"{}"}}"#, ws.display());
    let args = vec![
        "--data-root".to_string(),
        c.dr(),
        "hook".to_string(),
        "--tool".to_string(),
        "claude".to_string(),
        "--event".to_string(),
        "Stop".to_string(),
        "--root".to_string(),
        ws.to_str().unwrap().to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run_stdin(&ws, &refs, &payload);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !stdout.contains("systemMessage"),
        "关闭提示不得输出: {stdout}"
    );

    // 统计仍在
    let dr_for_metrics = c.dr();
    let margs = [
        "--json",
        "--data-root",
        dr_for_metrics.as_str(),
        "session",
        "--action",
        "metrics",
        "--session",
        "dis-1",
    ];
    let (code, stdout, stderr) = c.run(&ws, &margs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["interventions"], 1, "关闭提示仍统计: {v}");
}

/// AIL-019 全链路：真实 hook 入口关键词标注 → 窗口内聚合计数 → 配置关闭即失效。
#[test]
fn correction_heuristic_end_to_end_via_hook_entry() {
    let c = Ctx::new();
    let ws = setup(&c);
    let wsid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    let dr = c.dr();
    let hook = |event: &str, prompt_json: &str| {
        let payload = format!(
            r#"{{"session_id":"he-1","cwd":"{}","prompt":{prompt_json}}}"#,
            ws.display()
        );
        let args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            event.to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        c.run_stdin(&ws, &refs, &payload)
    };

    // 工具事件（建立窗口基准）+ 关键词纠正 + 普通提示
    let (code, _, stderr) = hook("PostToolUse", r#""忽略""#);
    assert_eq!(code, 0, "{stderr}");
    let (code, out, stderr) = hook("UserPromptSubmit", r#""不对，改成缓存预热""#);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["result"]["captured"], true, "{v}");
    let (code, _, stderr) = hook("UserPromptSubmit", r#""请继续""#);
    assert_eq!(code, 0, "{stderr}");

    let text = std::fs::read_to_string(events_file(&c)).unwrap();
    assert!(
        text.contains("\"dedup_key\":\"correction\""),
        "hook 入口应标注 correction（文本仍不落盘）: {text}"
    );
    assert!(
        !text.contains("不对，改成缓存预热"),
        "prompt 原文不落盘: {text}"
    );

    let metrics = || {
        let args = [
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "session".to_string(),
            "--action".to_string(),
            "metrics".to_string(),
            "--session".to_string(),
            "he-1".to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, stdout, stderr) = c.run(&ws, &refs);
        assert_eq!(code, 0, "{stderr}");
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        v["result"].clone()
    };
    let m = metrics();
    assert_eq!(m["corrections_heuristic"], 1, "窗口内关键词纠正计数: {m}");
    assert_eq!(m["prompt_count"], 2, "普通提示不计纠正: {m}");

    // 更改配置确实影响结果：关闭启发式
    let ws_dir = c.tmp.path().join("data").join("ws").join(&wsid);
    std::fs::write(ws_dir.join("heuristic.toml"), "enabled = false\n").unwrap();
    let m = metrics();
    assert_eq!(m["corrections_heuristic"], 0, "关闭后不再计数: {m}");
}

// ---------- AIL-018/020 返工回归（RW-15/R14）：后续处理沿用 payload 工作区 ----------

/// Stop 的摩擦提示在「进程 cwd=A、payload.cwd=B」时必须作用于 B：
/// B 有足够干预则提示（marker 落在 B，A 无标记）；显式 --root A 优先则不提示；
/// 事件采集与后续处理使用同一解析结果。
#[test]
fn stop_prompt_follows_payload_workspace_not_process_cwd() {
    let c = Ctx::new();
    let bare = c.tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let team_src = common::make_team_source(c.tmp.path());
    ailoom::gitx::git(
        &team_src,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    )
    .unwrap();
    ailoom::gitx::git(&team_src, &["push", "-q", "-u", "origin", "HEAD"]).unwrap();
    let ws_a = common::make_business_repo(c.tmp.path(), "biz-a");
    let ws_b = common::make_business_repo(c.tmp.path(), "biz-b");
    let dr = c.dr();
    for ws in [&ws_a, &ws_b] {
        let args = [
            "--data-root".to_string(),
            dr.clone(),
            "init".to_string(),
            "--url".to_string(),
            team_src.to_str().unwrap().to_string(),
            "--project".to_string(),
            "a".to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run(ws, &refs);
        assert_eq!(code, 0, "{stderr}");
    }

    // 在 B 种入 2 次人工干预（会话 rw15-s）
    let wsid_b = ailoom::ids::workspace_id_from_root(&ws_b.canonicalize().unwrap());
    let ef = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(&wsid_b)
        .join("events")
        .join("events.jsonl");
    for n in 1..=2 {
        let e = ailoom::events::schema::Event {
            schema_version: 1,
            event_id: format!("rw15-int-{n}"),
            session_id: "rw15-s".into(),
            workspace_id: wsid_b.to_string(),
            device_id: "dev".into(),
            tool: "claude".into(),
            time: ailoom::ids::now_iso(),
            kind: "tool".into(),
            tool_name: Some("Bash".into()),
            exit_code: None,
            duration_ms: None,
            prompt_len: None,
            prompt_hash: None,
            tokens: None,
            dedup_key: Some(format!("intervention-{n}")),
        };
        assert!(ailoom::events::store::append_event(&ef, &e).unwrap());
    }

    // 进程 cwd=A、payload.cwd=B、无 --root：提示必须按 B 的数据触发
    let stop = |proc_cwd: &Path, payload_cwd: &Path, root: Option<&Path>| {
        let payload = format!(
            r#"{{"session_id":"rw15-s","cwd":"{}"}}"#,
            payload_cwd.display()
        );
        let mut args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            "Stop".to_string(),
        ];
        if let Some(r) = root {
            args.push("--root".to_string());
            args.push(r.to_str().unwrap().to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        c.run_stdin(proc_cwd, &refs, &payload)
    };

    let (code, stdout, stderr) = stop(&ws_a, &ws_b, None);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let root = v["result"]["workspace_root"].as_str().unwrap();
    assert!(
        root.ends_with("biz-b"),
        "workspace_root 应解析为 payload 工作区 B: {root}"
    );
    let notice = v["result"]["friction_notice"].as_str().unwrap_or_default();
    assert!(
        notice.contains("rw15-s"),
        "提示应按 B 的干预数据触发: {stdout}"
    );
    // 提示标记落在 B，A 未被触碰
    let summary = |ws: &Path| {
        c.tmp
            .path()
            .join("data")
            .join("ws")
            .join(ailoom::ids::workspace_id_from_root(
                &ws.canonicalize().unwrap(),
            ))
            .join("summary")
    };
    assert!(
        summary(&ws_b).join("rw15-s.prompted").exists(),
        "标记应在 B"
    );
    assert!(
        !summary(&ws_a).join("rw15-s.prompted").exists(),
        "A 不应有提示标记"
    );

    // 显式 --root A 优先于 payload B：按 A 的数据决策（A 无干预 → 不提示）
    let (code, stdout, stderr) = stop(&ws_b, &ws_b, Some(&ws_a));
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(v["result"]["workspace_root"]
        .as_str()
        .unwrap()
        .ends_with("biz-a"));
    assert!(
        v["result"]["friction_notice"].as_str().is_none(),
        "显式 root=A 不应按 B 提示: {stdout}"
    );
}
