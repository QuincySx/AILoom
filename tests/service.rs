//! Real CLI subprocesses, isolated data roots; never change the user's login services.
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn command(root: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ailoom"));
    c.env("AILOOM_STORE_ROOT", root.join("store"))
        .env_remove("XDG_DATA_HOME");
    c.arg("--data-root").arg(root).arg("--json").args(args);
    c
}
fn run(root: &Path, args: &[&str]) -> Value {
    let out = command(root, args).output().unwrap();
    assert!(
        out.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["result"].clone()
}
struct Fixture {
    root: PathBuf,
    _tmp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        Self {
            root: tmp.path().join("data space"),
            _tmp: tmp,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = command(&self.root, &["service", "stop"]).output();
    }
}
fn post(port: u16, token: &str, path: &str, body: Value) -> (u16, Value) {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = body.to_string();
    write!(socket,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-AILoom-Session: {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        serde_json::from_str(body).unwrap(),
    )
}
fn assert_output(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["result"].clone()
}
#[test]
fn cli_is_independent_and_web_reuses_the_detached_process() {
    let f = Fixture::new();
    assert_eq!(run(&f.root, &["service", "status"])["state"], "stopped");
    assert!(!f.root.join("service").exists());
    run(&f.root, &["version"]);
    assert!(!f.root.join("service").exists());
    let project = f._tmp.path().join("project");
    fs::create_dir(&project).unwrap();
    let knowledge = f._tmp.path().join("knowledge");
    run(
        &f.root,
        &[
            "knowledge",
            "--action",
            "init",
            "--root",
            project.to_str().unwrap(),
            "--path",
            knowledge.to_str().unwrap(),
        ],
    );
    assert!(
        !f.root.join("service").exists(),
        "Knowledge CLI must not launch a server"
    );
    let start = run(&f.root, &["service", "start", "--port", "0"]);
    assert_eq!(start["state"], "running");
    assert_eq!(start["autostart"]["enabled"], false);
    let web = run(&f.root, &["web", "--no-open"]);
    assert_eq!(web["pid"], start["pid"]);
    assert_eq!(web["reused"], true);
    assert!(web["url"]
        .as_str()
        .unwrap()
        .starts_with("http://127.0.0.1:"));
    assert!(start.get("token").is_none());
    assert!(start.get("url").is_none());
    let runtime: Value =
        serde_json::from_slice(&fs::read(f.root.join("service/runtime.json")).unwrap()).unwrap();
    let port = runtime["port"].as_u64().unwrap() as u16;
    assert_eq!(post(port, "wrong", "/api/service/probe", json!({})).0, 401);
    assert_eq!(
        post(
            port,
            "wrong",
            "/api/service/autostart",
            json!({"enabled":true})
        )
        .0,
        401
    );
    assert_eq!(post(port, "wrong", "/api/shutdown", json!({})).0, 401);
    assert_eq!(
        post(
            port,
            runtime["token"].as_str().unwrap(),
            "/api/service/status",
            json!({})
        )
        .1["running"],
        true
    );
    let log = fs::read_to_string(f.root.join("service/service.log")).unwrap();
    assert!(
        !log.contains(runtime["token"].as_str().unwrap()),
        "background log must not expose session token"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(f.root.join("service/runtime.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(f.root.join("service"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    assert_eq!(run(&f.root, &["service", "stop"])["state"], "stopped");
    assert!(!f.root.join("service/runtime.json").exists());
    assert_eq!(run(&f.root, &["service", "stop"])["state"], "stopped");
    run(
        &f.root,
        &[
            "knowledge",
            "--action",
            "status",
            "--root",
            project.to_str().unwrap(),
        ],
    );
    assert_eq!(run(&f.root, &["service", "status"])["running"], false);
}
#[test]
fn simultaneous_starts_and_port_collision_use_one_instance() {
    let f = Fixture::new();
    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = occupied.local_addr().unwrap().port();
    // 65535 has no fallback port; ephemeral ports on tested platforms are lower.
    if port == u16::MAX {
        return;
    }
    let args = ["service", "start", "--port", &port.to_string()];
    let mut a = command(&f.root, &args);
    let mut b = command(&f.root, &args);
    let a = a
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let b = b
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let a = assert_output(a.wait_with_output().unwrap());
    let b = assert_output(b.wait_with_output().unwrap());
    assert_eq!(a["pid"], b["pid"]);
    assert_ne!(a["port"], json!(port));
    assert_ne!(a["reused"], b["reused"]);
    let foreground = command(&f.root, &["console", "--no-open", "--port", "0"])
        .output()
        .unwrap();
    assert!(!foreground.status.success());
    assert_eq!(run(&f.root, &["service", "status"])["pid"], a["pid"]);
}
#[test]
fn stale_pid_is_not_killed_and_new_start_recovers_after_crash() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("service")).unwrap();
    fs::write(f.root.join("service/runtime.json"),json!({"pid":std::process::id(),"port":1,"token":uuid::Uuid::new_v4().to_string(),"data_root":f.root.canonicalize().unwrap()}).to_string()).unwrap();
    assert_eq!(run(&f.root, &["service", "stop"])["state"], "stopped");
    let mut process = command(&f.root, &["service", "run", "--port", "0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if run(&f.root, &["service", "status"])["state"] == "running" {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    process.kill().unwrap();
    process.wait().unwrap();
    assert_eq!(run(&f.root, &["service", "status"])["state"], "stopped");
    assert_eq!(
        run(&f.root, &["service", "start", "--port", "0"])["state"],
        "running"
    );
}
#[test]
fn different_data_roots_are_independent() {
    let a = Fixture::new();
    let b = Fixture::new();
    let av = run(&a.root, &["service", "start", "--port", "0"]);
    let bv = run(&b.root, &["service", "start", "--port", "0"]);
    assert_ne!(av["pid"], bv["pid"]);
    assert_ne!(av["port"], bv["port"]);
    run(&a.root, &["service", "stop"]);
    assert_eq!(run(&b.root, &["service", "status"])["state"], "running");
}
#[test]
fn graceful_stop_drains_a_request_and_rejects_new_writes() {
    use ailoom::console::{ConsoleOptions, ConsoleServer};
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 0,
        data_root: tmp.path().to_owned(),
        open_browser: false,
    })
    .unwrap();
    let mut slow = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    // Hold an accepted request open while requesting shutdown on another connection.
    write!(
        slow,
        "GET /api/server-info HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n",
        server.port
    )
    .unwrap();
    assert_eq!(
        post(server.port, &server.token, "/api/shutdown", json!({})).0,
        200
    );
    assert_eq!(
        post(
            server.port,
            &server.token,
            "/api/fs/approve",
            json!({"path":tmp.path()})
        )
        .0,
        503
    );
    slow.write_all(b"\r\n").unwrap();
    let mut reply = String::new();
    slow.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1"));
    let started = Instant::now();
    server.join();
    assert!(started.elapsed() < Duration::from_secs(3));
}
#[cfg(unix)]
#[test]
fn terminal_signals_stop_foreground_cleanly() {
    let f = Fixture::new();
    let mut process = command(&f.root, &["console", "--no-open", "--port", "0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if run(&f.root, &["service", "status"])["state"] == "running" {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe {
        libc::kill(process.id() as i32, libc::SIGTERM);
    }
    loop {
        if let Some(status) = process.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!f.root.join("service/runtime.json").exists());
}

#[test]
fn graceful_stop_waits_for_background_jobs() {
    use ailoom::console::{
        jobs::{Job, JobStatus},
        ConsoleOptions, ConsoleServer,
    };
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 0,
        data_root: tmp.path().to_owned(),
        open_browser: false,
    })
    .unwrap();
    let job:Job=serde_json::from_value(json!({"id":"drain-test","kind":"apply","status":"running","created_at":"now","updated_at":"now","root":tmp.path(),"scope":null,"progress":[],"result":null,"cancel_requested":false})).unwrap();
    server
        .state
        .jobs
        .lock()
        .unwrap()
        .insert(job.id.clone(), job);
    let state = server.state.clone();
    server.shutdown();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        server.join();
        tx.send(()).unwrap();
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "must not exit with an unfinished job"
    );
    state
        .jobs
        .lock()
        .unwrap()
        .get_mut("drain-test")
        .unwrap()
        .status = JobStatus::Success;
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    worker.join().unwrap();
}

/// 升级回归：同一数据目录下由旧版本启动的服务（不写当前的运行记录、不持有运行锁）。
/// 此前 status / stop 只看运行记录，误报「已停止」，start 还会对同一数据目录起第二个服务。
#[cfg(unix)]
#[test]
fn service_started_by_an_older_version_is_reported_not_silently_ignored() {
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let f = Fixture::new();
    fs::create_dir_all(&f.root).unwrap();
    let root = f.root.canonicalize().unwrap();
    // 模拟旧版本进程：命令行与真实服务一致（数据目录带空格），但什么都不注册
    let argv0 = format!(
        "/old/ailoom --data-root {} service run --port 47999",
        root.display()
    );
    let legacy = KillOnDrop(
        Command::new("bash")
            .arg("-c")
            .arg(format!("exec -a \"{argv0}\" sleep 60"))
            .spawn()
            .unwrap(),
    );
    let pid = legacy.0.id();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        let v = run(&f.root, &["service", "status"]);
        if v["state"] == "unmanaged" || Instant::now() > deadline {
            break v;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(status["state"], "unmanaged", "{status}");
    assert_eq!(status["pid"], pid, "{status}");
    assert_eq!(status["port"], 47999, "{status}");

    for args in [
        &["service", "stop"][..],
        &["service", "start", "--port", "0"][..],
    ] {
        let out = command(&f.root, args).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} 不能假装成功: {stderr}");
        assert!(
            stderr.contains(&pid.to_string()) && stderr.contains("\"fix\""),
            "{args:?}: {stderr}"
        );
    }
    assert!(
        !f.root.join("service/runtime.json").exists(),
        "没有起第二个服务"
    );

    // 其他数据目录不受影响；旧进程结束后恢复正常
    let other = Fixture::new();
    assert_eq!(run(&other.root, &["service", "status"])["state"], "stopped");
    drop(legacy);
    assert_eq!(run(&f.root, &["service", "status"])["state"], "stopped");
}

/// 升级用：restart 换成新进程、沿用原端口；没在运行时等同于 start。
#[test]
fn restart_replaces_the_process_and_keeps_the_port() {
    let f = Fixture::new();
    let first = run(&f.root, &["service", "start", "--port", "0"]);
    let restarted = run(&f.root, &["service", "restart"]);
    assert_eq!(restarted["state"], "running", "{restarted}");
    assert_eq!(restarted["restarted"], true);
    assert_eq!(restarted["port"], first["port"], "沿用原端口");
    assert_ne!(restarted["pid"], first["pid"], "换成新进程");
    run(&f.root, &["service", "stop"]);
    let cold = run(&f.root, &["service", "restart", "--port", "0"]);
    assert_eq!(cold["state"], "running");
    assert_eq!(cold["restarted"], false);
}
