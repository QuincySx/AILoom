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
    fn run_env(&self, cwd: &Path, args: &[&str], k: &str, v: &str) -> (i32, String, String) {
        let out = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .env("HOME", self.tmp.path().join("home"))
            .env("AILOOM_LOG", "error")
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
            .env("HOME", self.tmp.path().join("home"))
            .env("AILOOM_LOG", "error")
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
        .env("HOME", c.tmp.path().join("home"))
        .env("AILOOM_LOG", "error")
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
    assert_eq!(v["result"]["session_count"], 1);
    assert!(
        v["result"]["missing_sources"]["unavailable_token_sessions"]
            .as_u64()
            .unwrap()
            >= 1,
        "缺失来源可见"
    );
    assert!(v["result"]["totals"]["prompt_count"].as_u64().is_some());
}
