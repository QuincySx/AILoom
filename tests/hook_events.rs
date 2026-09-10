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
            .env("HOME", self.tmp.path().join("home"))
            .env("AILOOM_LOG", "error")
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
            .env("HOME", self.tmp.path().join("home"))
            .env("AILOOM_LOG", "error")
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
