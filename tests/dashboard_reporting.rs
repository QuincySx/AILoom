//! 本地看板（AIL-021）与团队统计（AIL-022）集成测试。

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    fn run_env(&self, cwd: &Path, args: &[&str], k: &str, v: &str) -> (i32, String, String) {
        let out = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .env(k, v)
            .output()
            .unwrap();
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

fn setup_ws_with_events(c: &Ctx) -> (PathBuf, PathBuf) {
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
    let url = bare.to_str().unwrap().to_string();
    let dr = c.dr();
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    // 采集两个事件
    for (ev, sid) in [("session-start", "s-board"), ("stop", "s-board")] {
        let payload = format!(r#"{{"session_id":"{sid}","cwd":"{}"}}"#, ws.display());
        let args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            ev.to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run_stdin(&ws, &refs, &payload);
        assert_eq!(code, 0, "{stderr}");
    }
    (ws, bare)
}

impl Ctx {
    fn run_stdin(&self, cwd: &Path, args: &[&str], stdin_payload: &str) -> (i32, String, String) {
        let mut child = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
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
}

#[test]
fn dashboard_state_matches_cli_metrics() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    // 起看板（后台）：使用随机空闲端口，避免并行/残留进程干扰
    let dr = c.dr();
    let port_probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_probe.local_addr().unwrap().port();
    drop(port_probe);
    let mut server = Command::new(bin())
        .args([
            "--json",
            "--data-root",
            dr.as_str(),
            "dashboard",
            "--port",
            &port.to_string(),
        ])
        .current_dir(&ws)
        .envs(common::isolated_child_env(c.tmp.path()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // 等待端口就绪
    let mut connected = false;
    for _ in 0..40 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            connected = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(connected, "看板端口未就绪");
    // API 状态
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /api/state HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    let json_start = body.find('{').expect("JSON 响应");
    let state: serde_json::Value = serde_json::from_str(&body[json_start..]).unwrap();
    let sessions = state["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{state}");
    assert_eq!(sessions[0]["session_id"], "s-board");

    // 与 CLI 统计一致
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "session",
        "--action",
        "metrics",
        "--session",
        "s-board",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let cli_metrics: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        cli_metrics["result"]["stop_count"],
        sessions[0]["stop_count"]
    );
    assert_eq!(
        cli_metrics["result"]["prompt_count"],
        sessions[0]["prompt_count"]
    );

    // 页面可访问且有本地视图声明 + HTML 转义函数
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut page = String::new();
    stream.read_to_string(&mut page).unwrap();
    assert!(page.contains("AILoom 本地看板"));
    assert!(
        page.contains("不代表全团队实时状态"),
        "看板不暗示远端成员实时在线"
    );
    assert!(page.contains("esc("), "用户可控字段 HTML 转义");

    server.kill().unwrap();
    let _ = server.wait();
}

#[test]
fn port_conflict_gives_diagnosis() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let dr = c.dr();
    // 占用端口的 listener
    let listener = std::net::TcpListener::bind("127.0.0.1:17999").unwrap();
    let out = Command::new(bin())
        .args(["--data-root", dr.as_str(), "dashboard", "--port", "17999"])
        .current_dir(&ws)
        .env("HOME", c.tmp.path().join("home"))
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0), "端口占用必须报错");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("17999") || stderr.contains("绑定失败"),
        "{stderr}"
    );
    drop(listener);
}

#[test]
fn report_push_requires_explicit_enable() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "report",
        "--action",
        "push",
    ];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 17, "默认关闭必须拒绝");
    assert!(stderr.contains("E8001"), "{stderr}");
}

#[test]
fn report_push_creates_report_branch_and_resource_lock_untouched() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let src = c.tmp.path().join("team-src");
    let lock_before =
        std::fs::read_to_string(ws.join(".ailoom/machine/sources.lock.json")).unwrap();

    let dr = c.dr();
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr,
        "report".to_string(),
        "--action".to_string(),
        "push".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run_env(&ws, &refs, "AILOOM_REPORTING", "1");
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let branch = v["result"]["branch"].as_str().unwrap().to_string();
    assert!(branch.starts_with("ailoom/reports"));

    // 远端报告分支存在且包含 reports/ 路径
    ailoom::gitx::git(&src, &["fetch", "-q", "origin"]).unwrap();
    let listing = ailoom::gitx::git(
        &src,
        &["ls-tree", "-r", "--name-only", &format!("origin/{branch}")],
    )
    .unwrap();
    assert!(listing.contains("reports/sessions/"), "{listing}");
    // 默认共享无 prompt 正文
    assert!(!listing.contains("summary-body"));

    // 源锁不变：统计提交不推进资源 revision
    let lock_after = std::fs::read_to_string(ws.join(".ailoom/machine/sources.lock.json")).unwrap();
    assert_eq!(lock_before, lock_after);

    // checkpoint 记录批次（幂等依据）
    let data_ws = c.tmp.path().join("data").join("ws");
    let mut found = false;
    for entry in std::fs::read_dir(&data_ws).unwrap().flatten() {
        let cp = entry.path().join("report-checkpoint.json");
        if cp.is_file() {
            let text = std::fs::read_to_string(&cp).unwrap();
            assert!(text.contains("pushed_batches"));
            found = true;
        }
    }
    assert!(found);
}

#[test]
fn report_digest_shows_missing_sources() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let dr = c.dr();
    // 团队汇总读取远端报告分支：先推送本工作区数据
    let push = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "report".to_string(),
        "--action".to_string(),
        "push".to_string(),
    ];
    let refs: Vec<&str> = push.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run_env(&ws, &refs, "AILOOM_REPORTING", "1");
    assert_eq!(code, 0, "{stderr}");
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "report",
        "--action",
        "digest",
    ];
    let (code, stdout, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["scope"], "team");
    assert_eq!(v["result"]["session_count"], 1);
    assert!(
        v["result"]["report_branches_merged"].as_u64().unwrap() >= 1,
        "应合并远端报告分支"
    );
    assert!(
        v["result"]["missing_sources"]["unavailable_token_sessions"]
            .as_u64()
            .unwrap()
            >= 1,
        "缺失来源可见"
    );
    assert!(v["result"]["totals"]["prompt_count"].as_u64().is_some());
}

// ---------- AIL-022 返工回归（R09：批次幂等/补传/团队汇总） ----------

fn push_report(c: &Ctx, ws: &Path) -> (i32, String, String) {
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "report".to_string(),
        "--action".to_string(),
        "push".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    c.run_env(ws, &refs, "AILOOM_REPORTING", "1")
}

fn report_status(c: &Ctx, ws: &Path) -> serde_json::Value {
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "report".to_string(),
        "--action".to_string(),
        "status".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    serde_json::from_str(stdout.trim()).unwrap()
}

/// 推送成功但响应丢失（checkpoint 未更新）后重试：同 batch_id、远端无重复批次、计数不翻倍。
#[test]
fn report_push_idempotent_on_lost_response() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let src = c.tmp.path().join("team-src");

    let (code, stdout, stderr) = push_report(&c, &ws);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let batch1 = v["result"]["batch_id"].as_str().unwrap().to_string();

    // 模拟"响应丢失"：推送已成功但本地确认丢失
    let cp = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(ailoom::ids::workspace_id_from_root(
            &ws.canonicalize().unwrap(),
        ))
        .join("report-checkpoint.json");
    std::fs::remove_file(&cp).unwrap();

    // 重试：同内容 → 同 batch_id，推送同内容到同分支
    let (code, stdout, stderr) = push_report(&c, &ws);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v["result"]["batch_id"].as_str().unwrap(),
        batch1,
        "内容派生批次身份稳定"
    );

    // 远端该分支上每个批次文件只出现一次（路径确定性 → 无重复批次）
    ailoom::gitx::git(&src, &["fetch", "-q", "origin"]).unwrap();
    let branch = format!("ailoom/reports-{}", &batch1[1..9]);
    let listing = ailoom::gitx::git(
        &src,
        &["ls-tree", "-r", "--name-only", &format!("origin/{branch}")],
    )
    .unwrap();
    let batch_files: Vec<&str> = listing.lines().filter(|l| l.contains("batches/")).collect();
    assert_eq!(batch_files.len(), 1, "批次文件唯一: {listing:?}");

    // 确认已写入 checkpoint 后再 push：幂等跳过，不再推送
    let (code, stdout, _) = push_report(&c, &ws);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["already_pushed"], true, "{v}");
}

/// 离线失败冻结批次 → 恢复后 retry 实际补发并清除 pending；重启仍可恢复。
#[test]
fn report_pending_retry_repushes_and_clears() {
    let c = Ctx::new();
    let (ws, bare) = setup_ws_with_events(&c);
    let dr = c.dr();

    // 远端不可达
    let hidden = bare.with_extension("hidden");
    std::fs::rename(&bare, &hidden).unwrap();
    let (code, _, stderr) = push_report(&c, &ws);
    assert_ne!(code, 0, "推送失败必须报告: {stderr}");
    let status = report_status(&c, &ws);
    assert_eq!(
        status["result"]["pending"].as_object().map(|o| o.len()),
        Some(1),
        "失败批次冻结进 pending: {status}"
    );
    let batch_id = status["result"]["pending"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();

    // 恢复远端后补传
    std::fs::rename(&hidden, &bare).unwrap();
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "report".to_string(),
        "--action".to_string(),
        "retry".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run_env(&ws, &refs, "AILOOM_REPORTING", "1");
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["retried"], 1);
    assert_eq!(v["result"]["batches"][0].as_str().unwrap(), batch_id);

    // pending 清除、pushed 与确认水位线更新；重启（新进程）后依然如此
    let status = report_status(&c, &ws);
    assert_eq!(
        status["result"]["pending"].as_object().map(|o| o.len()),
        Some(0)
    );
    assert!(
        status["result"]["pushed_batches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b.as_str() == Some(batch_id.as_str())),
        "{status}"
    );
    assert!(
        status["result"]["confirmed_event_count"].as_u64().unwrap() > 0,
        "确认水位线更新: {status}"
    );
    let (code, stdout, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["retried"], 0, "重复 retry 无事可做");
}

/// 两工作区/设备的报告合并正确；同 session 更新不反复累计（累计快照取最大值语义）。
#[test]
fn team_digest_merges_workspaces_without_double_counting() {
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

    let hook_prompt = |c: &Ctx, ws: &Path, sid: &str, text: &str| {
        // payload 必须有差异（AIL-018 稳定事件身份：完全相同的 payload 视为重投被去重）
        let payload = format!(
            r#"{{"session_id":"{sid}","cwd":"{}","prompt":"{text}"}}"#,
            ws.display()
        );
        let args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            c.dr(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            "UserPromptSubmit".to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run_stdin(ws, &refs, &payload);
        assert_eq!(code, 0, "{stderr}");
    };
    let bind = |c: &Ctx, ws: &Path| {
        let args = [
            "--json".to_string(),
            "--data-root".to_string(),
            c.dr(),
            "init".to_string(),
            "--url".to_string(),
            bare.to_str().unwrap().to_string(),
            "--project".to_string(),
            "a".to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run(ws, &refs);
        assert_eq!(code, 0, "{stderr}");
    };

    let ws_a = common::make_business_repo(c.tmp.path(), "biz-a");
    let ws_b = common::make_business_repo(c.tmp.path(), "biz-b");
    bind(&c, &ws_a);
    bind(&c, &ws_b);
    hook_prompt(&c, &ws_a, "s-a", "第一条提示");
    hook_prompt(&c, &ws_b, "s-b", "B 的一条提示");

    // A、B 各自推送（同一团队源远端）
    let (code, _, stderr) = push_report(&c, &ws_a);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = push_report(&c, &ws_b);
    assert_eq!(code, 0, "{stderr}");

    // A 的同 session 再补一次 prompt 后再推送：累计快照从 1 → 2
    hook_prompt(&c, &ws_a, "s-a", "第二条提示");
    let (code, _, stderr) = push_report(&c, &ws_a);
    assert_eq!(code, 0, "{stderr}");

    // 团队 digest：合并两个工作区，session 独立计数；s-a 取最大快照 2 而不是 1+2
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "report".to_string(),
        "--action".to_string(),
        "digest".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&ws_a, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["scope"], "team", "{v}");
    assert_eq!(v["result"]["session_count"], 2, "两工作区独立会话: {v}");
    assert_eq!(
        v["result"]["totals"]["prompt_count"].as_u64().unwrap(),
        3,
        "s-a 取累计快照最大值 2 + s-b 的 1，不反复累计"
    );
    let sessions = v["result"]["sessions"].as_array().unwrap();
    let sa = sessions
        .iter()
        .find(|s| s["session_id_hash"].is_string() && s["prompt_count"].as_u64() == Some(2))
        .expect("s-a 快照为 2");
    assert!(
        sa["devices"]
            .as_array()
            .map(|d| !d.is_empty())
            .unwrap_or(false),
        "设备维度可见: {sa}"
    );
}

// ---------- AIL-021 返工回归（R08：SSE 头解析/跨进程更新） ----------

fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    body
}

fn spawn_dashboard(c: &Ctx, ws: &Path, port: u16) -> std::process::Child {
    let dr = c.dr();
    Command::new(bin())
        .args([
            "--json",
            "--data-root",
            dr.as_str(),
            "dashboard",
            "--port",
            &port.to_string(),
        ])
        .current_dir(ws)
        .envs(common::isolated_child_env(c.tmp.path()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

fn wait_port(port: u16) {
    for _ in 0..60 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("看板端口 {port} 未就绪");
}

/// 读 SSE：返回在 bounded 时间内收到的全部文本（含首个 snapshot）。
fn sse_read(port: u16, path: &str, with_last_event_id: Option<&str>, secs: u64) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(secs)))
        .unwrap();
    let last = with_last_event_id
        .map(|id| format!("Last-Event-ID: {id}\r\n"))
        .unwrap_or_default();
    stream
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n{last}\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    buf
}

fn snapshot_count(text: &str) -> usize {
    text.matches("event: snapshot").count()
}

/// 普通 EventSource（无 Last-Event-ID）也在有界时间内收到初始快照。
#[test]
fn sse_initial_snapshot_without_last_event_id() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let port_probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_probe.local_addr().unwrap().port();
    drop(port_probe);
    let mut server = spawn_dashboard(&c, &ws, port);
    wait_port(port);

    let text = sse_read(port, "/api/events", None, 5);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
    assert_eq!(snapshot_count(&text), 1, "必须立即收到一个快照: {text}");
    assert!(text.contains("s-board"), "{text}");

    server.kill().unwrap();
    let _ = server.wait();
}

/// 另一进程追加事件后，SSE 客户端在约定延迟内看到新数据；轮转后历史不消失。
#[test]
fn sse_detects_cross_process_append_and_rotation_keeps_history() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let port_probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_probe.local_addr().unwrap().port();
    drop(port_probe);
    let mut server = spawn_dashboard(&c, &ws, port);
    wait_port(port);

    // 后台 SSE 客户端持续读取
    let reader = std::thread::spawn(move || sse_read(port, "/api/events", None, 8));

    // 等待首快照送达后再触发独立进程追加事件
    std::thread::sleep(std::time::Duration::from_millis(800));
    let dr = c.dr();
    let payload = format!(r#"{{"session_id":"s-late","cwd":"{}"}}"#, ws.display());
    let args = vec![
        "--json".to_string(),
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
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run_stdin(&ws, &refs, &payload);
    assert_eq!(code, 0, "{stderr}");

    // 轮转（独立进程）：历史进归档，统一读取不丢
    let (code, _, stderr) = c.run(
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
    assert_eq!(code, 0, "{stderr}");

    let text = reader.join().unwrap();
    assert!(snapshot_count(&text) >= 2, "跨进程更新应推送新快照: {text}");
    assert!(text.contains("s-late"), "新会话可见: {text}");
    assert!(text.contains("s-board"), "轮转后历史仍在: {text}");

    // 重连（带旧 Last-Event-ID 或不带）后状态与 /api/state 一致
    let state_body = http_get(port, "/api/state");
    let json = &state_body[state_body.find('{').unwrap()..];
    let state: serde_json::Value = serde_json::from_str(json).unwrap();
    let ids: Vec<&str> = state["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session_id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"s-late") && ids.contains(&"s-board"),
        "{state}"
    );
    let reconnect = sse_read(port, "/api/events", Some("0"), 5);
    assert_eq!(snapshot_count(&reconnect), 1, "重连立即收到重建快照");
    for id in ["s-late", "s-board"] {
        assert!(reconnect.contains(id), "重连快照含 {id}: {reconnect}");
    }

    server.kill().unwrap();
    let _ = server.wait();
}

/// 空数据工作区：/api/state 返回空会话列表；SSE 首快照也是空列表。
#[test]
fn dashboard_empty_workspace_shows_empty_state() {
    let c = Ctx::new();
    // 无事件的工作区（本地路径源）
    let team_src = common::make_team_source(c.tmp.path());
    let ws = common::make_business_repo(c.tmp.path(), "biz-empty");
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        team_src.to_str().unwrap().to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    let port_probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_probe.local_addr().unwrap().port();
    drop(port_probe);
    let mut server = spawn_dashboard(&c, &ws, port);
    wait_port(port);

    let state_body = http_get(port, "/api/state");
    let json = &state_body[state_body.find('{').unwrap()..];
    let state: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(state["sessions"].as_array().unwrap().len(), 0, "{state}");
    let snap = sse_read(port, "/api/events", None, 5);
    assert_eq!(snapshot_count(&snap), 1);
    assert!(snap.contains("\"sessions\":[]"), "{snap}");

    server.kill().unwrap();
    let _ = server.wait();
}

/// RW-06/R02：真实 CLI 注入同名 session 的不同设备事件——metrics 列表两条独立、
/// 单 session 查询给出歧义解释、/api/state 同样独立（与 CLI 同一聚合语义）。
#[test]
fn same_session_across_devices_distinct_in_metrics_and_dashboard() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let dr = c.dr();

    // 设备 2：改写 device-id 后以同名 session 采集（同一工作区、同一宿主）
    let device_file = c.tmp.path().join("data").join("device-id");
    let dev1 = std::fs::read_to_string(&device_file).unwrap();
    std::fs::write(&device_file, "device-two-rw06").unwrap();
    for ev in ["session-start", "stop"] {
        // payload 加区分字段：不同设备事件内容不同（event_id 去重按内容）
        let payload = format!(
            r#"{{"session_id":"s-board","cwd":"{}","device_hint":"two"}}"#,
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
            ev.to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run_stdin(&ws, &refs, &payload);
        assert_eq!(code, 0, "{stderr}");
    }
    std::fs::write(&device_file, dev1).unwrap();

    // metrics 列表：s-board 两条独立（device_id 不同），不合并
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "session",
        "--action",
        "metrics",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let sessions = v["result"]["sessions"].as_array().unwrap();
    let board: Vec<&serde_json::Value> = sessions
        .iter()
        .filter(|s| s["session_id"] == "s-board")
        .collect();
    assert_eq!(board.len(), 2, "跨设备同名会话应独立: {v}");
    let devices: std::collections::BTreeSet<String> = board
        .iter()
        .map(|s| s["device_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(devices.len(), 2, "两条会话设备不同: {v}");

    // 单 session 查询：歧义可解释
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "session",
        "--action",
        "metrics",
        "--session",
        "s-board",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["identity_ambiguous"], true, "{v}");
    assert_eq!(
        v["result"]["identities"].as_array().map(|a| a.len()),
        Some(2),
        "{v}"
    );

    // 看板 /api/state：s-board 两条独立（与 CLI 同一聚合语义）
    let port_probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_probe.local_addr().unwrap().port();
    drop(port_probe);
    let mut server = Command::new(bin())
        .args([
            "--json",
            "--data-root",
            &c.dr(),
            "dashboard",
            "--port",
            &port.to_string(),
        ])
        .current_dir(&ws)
        .envs(common::isolated_child_env(c.tmp.path()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut connected = false;
    for _ in 0..40 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            connected = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(connected, "看板端口未就绪");
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /api/state HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    let json_start = body.find('{').expect("JSON 响应");
    let state: serde_json::Value = serde_json::from_str(&body[json_start..]).unwrap();
    let board_rows: Vec<&serde_json::Value> = state["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["session_id"] == "s-board")
        .collect();
    assert_eq!(board_rows.len(), 2, "看板同样按完整身份独立: {state}");
    server.kill().unwrap();
    let _ = server.wait();
}

/// RW-07/R03：状态按最新生命周期事件变化——Stop 后继续输入恢复 running；
/// 最新事件为 Stop（含进程重启后）为 idle；仅有 session-start 为 running。
#[test]
fn dashboard_state_follows_latest_lifecycle_event() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let dr = c.dr();

    let hook = |ev: &str, sid: &str| {
        // 事件去重按内容：尾部 hint 使每次注入内容不同（模拟真实不同事件）
        let hint = format!(
            "{}-{}",
            ev,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 100000
        );
        let payload = format!(
            r#"{{"session_id":"{sid}","cwd":"{}","hint":"{}"}}"#,
            ws.display(),
            hint
        );
        let args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            ev.to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run_stdin(&ws, &refs, &payload);
        assert_eq!(code, 0, "{stderr}");
    };

    // s-board：session-start → stop（现有种子）→ 再来一次 prompt：应恢复 running
    hook("UserPromptSubmit", "s-board");
    // s2：只有 stop → idle
    hook("Stop", "s2");
    // s3：只有 session-start → running
    hook("SessionStart", "s3");

    let port_probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_probe.local_addr().unwrap().port();
    drop(port_probe);
    let mut server = Command::new(bin())
        .args([
            "--json",
            "--data-root",
            &c.dr(),
            "dashboard",
            "--port",
            &port.to_string(),
        ])
        .current_dir(&ws)
        .envs(common::isolated_child_env(c.tmp.path()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut connected = false;
    for _ in 0..40 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            connected = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(connected, "看板端口未就绪");
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /api/state HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    let json_start = body.find('{').expect("JSON 响应");
    let state: serde_json::Value = serde_json::from_str(&body[json_start..]).unwrap();
    server.kill().unwrap();
    let _ = server.wait();

    let state_of = |sid: &str| -> Vec<String> {
        state["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["session_id"] == sid)
            .map(|s| s["state"].as_str().unwrap_or("").to_string())
            .collect()
    };
    assert_eq!(
        state_of("s-board"),
        vec!["running"],
        "Stop 后 prompt 恢复 running: {state}"
    );
    assert_eq!(
        state_of("s2"),
        vec!["idle"],
        "最新事件为 Stop → idle: {state}"
    );
    assert_eq!(
        state_of("s3"),
        vec!["running"],
        "仅 session-start → running: {state}"
    );
}

/// RW-07：SSE 初始快照 + 指纹变化推送 + 断线重连。跨进程 hook 事件使
/// /api/events 在不刷新页面的情况下从 idle → running → idle。
fn read_sse_frames(stream: &mut TcpStream, frames: usize) -> String {
    use std::io::Read;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(8)))
        .unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while buf.windows(2).filter(|w| w == b"\n\n").count() < frames {
        let n = stream.read(&mut chunk).unwrap_or_else(|e| {
            panic!(
                "SSE 读取超时/断开: {e}; 已读 {} 字节: {}",
                buf.len(),
                String::from_utf8_lossy(&buf)
            )
        });
        assert!(n > 0, "SSE 流提前结束");
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn dashboard_sse_pushes_state_transitions_without_page_refresh() {
    let c = Ctx::new();
    let (ws, _bare) = setup_ws_with_events(&c);
    let dr = c.dr();

    let hook = |ev: &str, sid: &str| {
        // 事件去重按内容：尾部 hint 使每次注入内容不同（模拟真实不同事件）
        let hint = format!(
            "{}-{}",
            ev,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 100000
        );
        let payload = format!(
            r#"{{"session_id":"{sid}","cwd":"{}","hint":"{}"}}"#,
            ws.display(),
            hint
        );
        let args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            dr.clone(),
            "hook".to_string(),
            "--tool".to_string(),
            "claude".to_string(),
            "--event".to_string(),
            ev.to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run_stdin(&ws, &refs, &payload);
        assert_eq!(code, 0, "{stderr}");
    };
    // sse1：Start→Stop（当前 idle）
    hook("SessionStart", "sse1");
    hook("Stop", "sse1");

    let port_probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_probe.local_addr().unwrap().port();
    drop(port_probe);
    let mut server = Command::new(bin())
        .args([
            "--json",
            "--data-root",
            &c.dr(),
            "dashboard",
            "--port",
            &port.to_string(),
        ])
        .current_dir(&ws)
        .envs(common::isolated_child_env(c.tmp.path()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut connected = false;
    for _ in 0..40 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            connected = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(connected, "看板端口未就绪");

    let state_in_frame = |frame: &str| -> serde_json::Value {
        let data_line = frame
            .lines()
            .find(|l| l.starts_with("data: "))
            .expect("SSE data 行");
        serde_json::from_str::<serde_json::Value>(&data_line[6..]).unwrap()
    };
    let state_of = |v: &serde_json::Value, sid: &str| -> String {
        v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["session_id"] == sid)
            .map(|s| s["state"].as_str().unwrap_or("").to_string())
            .unwrap_or_default()
    };

    // 订阅：初始快照（无论是否带 Last-Event-ID）→ sse1 idle
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /api/events HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let frame1 = read_sse_frames(&mut stream, 1);
    let v1 = state_in_frame(&frame1);
    assert_eq!(state_of(&v1, "sse1"), "idle", "初始快照: {frame1}");

    // 独立进程 hook：新 Prompt → SSE 推送 running（页面不刷新）
    hook("UserPromptSubmit", "sse1");
    let frame2 = read_sse_frames(&mut stream, 1);
    let v2 = state_in_frame(&frame2);
    assert_eq!(
        state_of(&v2, "sse1"),
        "running",
        "SSE 应推送 running: {frame2}"
    );

    // 再 Stop → SSE 推送 idle
    hook("Stop", "sse1");
    let frame3 = read_sse_frames(&mut stream, 1);
    let v3 = state_in_frame(&frame3);
    assert_eq!(state_of(&v3, "sse1"), "idle", "SSE 应推送 idle: {frame3}");
    drop(stream);

    // 断线重连：新连接立即获得全量快照（含最新状态），服务器正常接受
    let mut stream2 = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream2
        .write_all(b"GET /api/events HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let reframed = read_sse_frames(&mut stream2, 1);
    let v4 = state_in_frame(&reframed);
    assert_eq!(state_of(&v4, "sse1"), "idle", "重连快照: {reframed}");
    drop(stream2);

    server.kill().unwrap();
    let _ = server.wait();
}

// ---------- AIL-022 返工回归（RW-08/R04+R05） ----------

/// R04：先制造真实 pending（离线推送失败冻结），关闭 reporting 后 retry/push
/// 均不发生远端写入（裸远端 refs 前后不变）；开关恢复后 retry 补传成功。
/// R05：同会话身份跨日（不同日期文件名）重报在 digest 中合并为一条取最大值，
/// 不重复累计。
#[test]
fn report_retry_gated_by_switch_and_crossday_snapshot_merged_once() {
    let c = Ctx::new();
    let (ws, bare) = setup_ws_with_events(&c);
    let dr = c.dr();

    let hook_prompt = |text: &str| {
        let payload = format!(
            r#"{{"session_id":"s-rw08","cwd":"{}","prompt":"{text}"}}"#,
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
            "UserPromptSubmit".to_string(),
            "--root".to_string(),
            ws.to_str().unwrap().to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = c.run_stdin(&ws, &refs, &payload);
        assert_eq!(code, 0, "{stderr}");
    };

    let remote_refs = || {
        ailoom::gitx::git(
            &bare,
            &["for-each-ref", "refs/heads", "--format=%(refname)"],
        )
        .unwrap()
    };

    // 基线推送成功（reporting 显式开启）
    let (code, _, stderr) = push_report(&c, &ws);
    assert_eq!(code, 0, "{stderr}");

    // 制造真实 pending：追加事件 → 断远端 → 推送失败冻结 → 恢复远端
    let hidden = c.tmp.path().join("origin.git-hidden");
    ailoom::gitx::git(&bare, &["config", "--get", "core.bare"]).unwrap(); // 触碰确认存在
    hook_prompt("pending 事件一");
    std::fs::rename(&bare, &hidden).unwrap();
    let push_args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "report".to_string(),
        "--action".to_string(),
        "push".to_string(),
    ];
    let push_refs: Vec<&str> = push_args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run_env(&ws, &push_refs, "AILOOM_REPORTING", "1");
    assert_ne!(code, 0, "远端不可达推送必须失败");
    std::fs::rename(&hidden, &bare).unwrap();
    let refs_after_pending = remote_refs();

    // 关闭 reporting：retry 不得发生远端写入（refs 前后不变），pending 保留
    let retry_args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "report".to_string(),
        "--action".to_string(),
        "retry".to_string(),
    ];
    let refs: Vec<&str> = retry_args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["retried"], 0, "{v}");
    assert_eq!(v["result"]["reporting_enabled"], false, "{v}");
    assert_eq!(remote_refs(), refs_after_pending, "关闭期间远端零写入");
    // push 同样被开关拒绝
    let (code, _, _) = c.run_env(&ws, &push_refs, "AILOOM_REPORTING", "0");
    assert_ne!(code, 0, "关闭时 push 必须拒绝");
    assert_eq!(remote_refs(), refs_after_pending, "远端仍零写入");

    // 开关恢复：retry 补传成功，pending 清空
    let (code, stdout, stderr) = c.run_env(&ws, &refs, "AILOOM_REPORTING", "1");
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["retried"], 1, "补传应成功: {v}");
    let status_args = [
        "--json",
        "--data-root",
        &c.dr(),
        "report",
        "--action",
        "status",
    ];
    let refs: Vec<&str> = status_args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v["result"]["pending"].as_object().map(|o| o.is_empty()),
        Some(true),
        "{v}"
    );

    // 追加事件后再推一次：今日快照 prompt=2（真实增量体现在新快照）
    hook_prompt("pending 事件二");
    let (code, push_out, stderr) = push_report(&c, &ws);
    assert_eq!(code, 0, "{stderr}");
    let branch: String = serde_json::from_str::<serde_json::Value>(push_out.trim()).unwrap()
        ["result"]["branch"]
        .as_str()
        .unwrap()
        .to_string();

    // R05 跨日合并：在报告分支手工加入昨日同名身份文件（较低计数），
    // digest 必须合并为一条取最大值（不重复累计）
    let wid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    let device = std::fs::read_to_string(c.tmp.path().join("data").join("device-id"))
        .unwrap()
        .trim()
        .to_string();
    let sidh = ailoom::ids::sha256_prefix(format!("{wid}|{device}|claude|s-rw08").as_bytes(), 16);
    // 取远端报告分支中今天的文件，复制为昨日文件名并把计数改低
    let clone = c.tmp.path().join("digest-clone");
    ailoom::gitx::git(
        &bare,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            clone.to_str().unwrap(),
            "--branch",
            &branch,
        ],
    )
    .unwrap();
    let listing = ailoom::gitx::git(&clone, &["ls-tree", "-r", "--name-only", "HEAD"]).unwrap();
    let today_file = listing
        .lines()
        .find(|l| l.starts_with("reports/sessions/") && l.ends_with(&format!("-{sidh}.json")))
        .expect("s-rw08 的今日报告文件存在")
        .to_string();
    let yesterday_file = format!("reports/sessions/2020-01-01-{sidh}.json");
    let body = ailoom::gitx::git(&clone, &["show", &format!("HEAD:{today_file}")]).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&body).unwrap();
    if let Some(rec) = v["record"].as_object_mut() {
        rec.insert("prompt_count".into(), serde_json::json!(1));
    } // 昨日快照较低（1），今日快照为 2 → 合并取最大值 2
    std::fs::write(
        clone.join(&yesterday_file),
        serde_json::to_string_pretty(&v).unwrap(),
    )
    .unwrap();
    ailoom::gitx::git(&clone, &["add", "-A"]).unwrap();
    ailoom::gitx::git(
        &clone,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "yesterday copy",
        ],
    )
    .unwrap();
    ailoom::gitx::git(&clone, &["push", "-q", "origin", "HEAD"]).unwrap();

    // digest：同身份跨日两条文件 → 一条记录，计数取最大值（不重复累计）
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "report",
        "--action",
        "digest",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let rows: Vec<&serde_json::Value> = v["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["session_id_hash"] == sidh.as_str())
        .collect();
    assert_eq!(rows.len(), 1, "跨日同身份只合并为一条: {v}");
    let prompts = rows[0]["prompt_count"].as_u64().unwrap();
    assert_eq!(prompts, 2, "取累计快照最大值 2 而非相加 3: {v}");
    assert!(
        rows[0]["dates"]
            .as_array()
            .map(|d| d.len() == 2)
            .unwrap_or(false),
        "两个上报日期都可见: {}",
        rows[0]["dates"]
    );
}
