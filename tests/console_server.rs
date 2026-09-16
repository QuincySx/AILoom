//! AIL-046 集成验收：本地控制台服务生命周期与受限 API 安全。
//! loopback 绑定、端口占用恢复、多实例会话隔离、Origin/Host/token 校验、
//! 目录边界（含符号链接逃逸）、草稿 revision 冲突、干净关停。

use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

fn opts(tmp: &std::path::Path, port: u16) -> ConsoleOptions {
    ConsoleOptions {
        port,
        data_root: tmp.join("data"),
        open_browser: false,
    }
}

/// 端口占用恢复：第二个实例自动换端口；两实例 token 独立（不串会话）。
#[test]
fn ail046_port_recovery_and_session_isolation() {
    let tmp = tempfile::tempdir().unwrap();
    let s1 = ConsoleServer::start(&opts(tmp.path(), 17890)).unwrap();
    let s2 = ConsoleServer::start(&opts(tmp.path(), 17890)).unwrap();
    assert_ne!(s1.port, s2.port, "端口占用自动让位");
    assert_ne!(s1.token, s2.token, "每实例独立会话令牌");
    // s1 的令牌不能用于 s2 的写接口
    let resp = post(
        s2.port,
        "/api/fs/approve",
        &[
            ("host", &format!("127.0.0.1:{}", s2.port)),
            (SESSION_HEADER, &s1.token),
        ],
        json!({ "path": tmp.path().join("approved") }),
    );
    assert_eq!(resp.0, 401, "跨实例令牌必须被拒");
    s1.shutdown();
    s2.shutdown();
    s1.join();
    s2.join();
}

fn raw_request(port: u16, req: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    let status: u16 = buf
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, buf)
}

fn method(
    port: u16,
    verb: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> (u16, String) {
    let body_text = body.map(|b| b.to_string()).unwrap_or_default();
    let has_host = headers.iter().any(|(k, _)| *k == "host");
    let mut req = if has_host {
        format!(
            "{verb} {path} HTTP/1.1\r\nContent-Length: {}\r\nConnection: close\r\n",
            body_text.len()
        )
    } else {
        format!("{verb} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n", body_text.len())
    };
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(&body_text);
    raw_request(port, &req)
}

fn post(port: u16, path: &str, headers: &[(&str, &str)], body: Value) -> (u16, String) {
    method(port, "POST", path, headers, Some(&body))
}

fn json_body(raw: &str) -> Value {
    let idx = raw.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    serde_json::from_str(&raw[idx..]).unwrap_or(Value::Null)
}

/// 安全矩阵：Host 白名单、Origin 跨站拒绝、写接口 token 必需、目录越界与 symlink 逃逸拒绝。
#[test]
fn ail046_security_matrix() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&opts(tmp.path(), 17895)).unwrap();
    let auth = [(SESSION_HEADER, server.token.as_str())];

    // 未知 Host（DNS rebinding 形态）拒绝
    let (code, _) = method(
        server.port,
        "GET",
        "/api/state",
        &[("host", "attacker.example:1")],
        None,
    );
    assert_eq!(code, 403, "未知 Host 拒绝");

    // localhost Host 合法
    let (code, raw) = method(
        server.port,
        "GET",
        "/api/state",
        &[("host", &format!("localhost:{}", server.port))],
        None,
    );
    assert_eq!(code, 200);
    assert!(json_body(&raw)["service"] == "ailoom-console");

    // 跨站 Origin 写请求拒绝
    let (code, _) = post(
        server.port,
        "/api/fs/approve",
        &[
            ("origin", &format!("http://evil.example:{}", server.port)),
            (SESSION_HEADER, server.token.as_str()),
        ],
        json!({ "path": tmp.path().join("x") }),
    );
    assert_eq!(code, 403, "跨站 Origin 拒绝");

    // 缺 token 的写请求拒绝
    let (code, _) = post(
        server.port,
        "/api/fs/approve",
        &[("origin", &format!("http://127.0.0.1:{}", server.port))],
        json!({ "path": tmp.path().join("x") }),
    );
    assert_eq!(code, 401, "写接口必须带会话令牌");

    // 批准目录 → 列出；未批准路径拒绝
    let approved = tmp.path().join("approved");
    std::fs::create_dir_all(approved.join("sub")).unwrap();
    let (code, _) = post(
        server.port,
        "/api/fs/approve",
        &auth,
        json!({ "path": approved }),
    );
    assert_eq!(code, 200);
    let (code, raw) = method(
        server.port,
        "GET",
        &format!("/api/fs/list?path={}", approved.display()),
        &[],
        None,
    );
    assert_eq!(code, 200);
    assert!(raw.contains("sub"));

    // 根外路径拒绝
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let (code, raw) = method(
        server.port,
        "GET",
        &format!("/api/fs/list?path={}", outside.display()),
        &[],
        None,
    );
    assert_eq!(code, 403, "未批准根拒绝");
    assert!(json_body(&raw)["error"]
        .as_str()
        .unwrap_or("")
        .contains("批准"));

    // 符号链接逃逸拒绝：approved 内的链接指向 outside
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, approved.join("escape")).unwrap();
        let (code, _) = method(
            server.port,
            "GET",
            &format!("/api/fs/list?path={}", approved.join("escape").display()),
            &[],
            None,
        );
        assert_eq!(code, 403, "符号链接逃逸拒绝");
    }

    // 不通配 CORS
    let (_, raw) = method(server.port, "GET", "/api/state", &[], None);
    assert!(
        !raw.to_ascii_lowercase()
            .contains("access-control-allow-origin: *"),
        "不允许通配 CORS"
    );

    server.shutdown();
    server.join();
}

/// 仓库发现 + 登记 + 草稿 revision 冲突 + 干净关停。
#[test]
fn ail046_discover_draft_conflict_and_shutdown() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&opts(tmp.path(), 17897)).unwrap();
    let auth = [(SESSION_HEADER, server.token.as_str())];

    // 批准并发现一个 git 仓库
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut git = std::process::Command::new("git");
    git.args(["init", "-q"]).current_dir(&repo);
    assert!(git.status().unwrap().success());
    let (code, _) = post(
        server.port,
        "/api/fs/approve",
        &auth,
        json!({ "path": tmp.path() }),
    );
    assert_eq!(code, 200);
    let (code, raw) = post(
        server.port,
        "/api/repo/discover",
        &auth,
        json!({ "path": repo }),
    );
    assert_eq!(code, 200, "{raw}");
    let v = json_body(&raw);
    assert_eq!(v["kind"], "git");
    assert!(v["repo_id"].as_str().unwrap_or("").starts_with("repo-"));
    // 发现即登记
    assert!(server
        .state
        .data_root
        .join("repos")
        .join(v["repo_id"].as_str().unwrap())
        .join("registry.json")
        .is_file());

    // 草稿：base=0 保存 → rev 1；旧 base 再保存 → 409 且返回当前草稿
    let (code, _) = method(
        server.port,
        "PUT",
        "/api/draft",
        &auth,
        Some(&json!({ "base_revision": 0, "draft": { "step": 2 } })),
    );
    assert_eq!(code, 200);
    let (code, raw) = method(
        server.port,
        "PUT",
        "/api/draft",
        &auth,
        Some(&json!({ "base_revision": 0, "draft": { "step": 3 } })),
    );
    assert_eq!(code, 409, "过期保存返回冲突");
    let v = json_body(&raw);
    assert_eq!(v["current_revision"], json!(1));
    assert_eq!(v["draft"]["step"], json!(2), "冲突时保留当前草稿");
    let (code, raw) = method(server.port, "GET", "/api/draft", &auth, None);
    assert_eq!(code, 200);
    assert_eq!(json_body(&raw)["revision"], json!(1));

    // 个人能力选择经受限 API 写入 profile
    let (code, _) = post(
        server.port,
        "/api/profile/select",
        &auth,
        json!({ "host": "claude", "state": "enable" }),
    );
    assert_eq!(code, 200, "无登记仓库时给 400；此处已登记");
    let _ = code;

    // 能力矩阵可读（宿主×资源支持级别）
    let (code, raw) = method(server.port, "GET", "/api/capabilities", &[], None);
    assert_eq!(code, 200);
    assert!(
        raw.contains(".agents/skills"),
        "能力矩阵包含 Codex 原生路径"
    );

    // 干净关停：shutdown 后 accept 循环退出
    let (code, _) = post(server.port, "/api/shutdown", &auth, json!({}));
    assert_eq!(code, 200);
    server.join();
}

/// 非授权目录的 repo/discover 拒绝（受限读的第一道边界）。
#[test]
fn ail046_repo_discover_requires_approved_root() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&opts(tmp.path(), 17899)).unwrap();
    let auth = [(SESSION_HEADER, server.token.as_str())];
    let repo = tmp.path().join("secret-repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut git = std::process::Command::new("git");
    git.args(["init", "-q"]).current_dir(&repo);
    let _ = git.status();
    let (code, raw) = post(
        server.port,
        "/api/repo/discover",
        &auth,
        json!({ "path": repo }),
    );
    assert_eq!(code, 403, "未批准根内的仓库发现拒绝: {raw}");
    server.shutdown();
    server.join();
}

/// PathBuf 引用保持（避免未使用告警的辅助）。
#[allow(dead_code)]
fn _touch(p: PathBuf) -> PathBuf {
    p
}

// ---------------------------------------------------------------------------
// AIL-050：计划/应用任务、指纹绑定、宿主验证、可恢复撤销
// ---------------------------------------------------------------------------

use ailoom::console::jobs;

fn wait_job(server: &ConsoleServer, id: &str, timeout: Duration) -> Value {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(job) = server.state.jobs.lock().unwrap().get(id) {
            if !matches!(
                job.status,
                ailoom::console::jobs::JobStatus::Queued
                    | ailoom::console::jobs::JobStatus::Running
            ) {
                return serde_json::to_value(job).unwrap();
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("任务超时未完成: {id}");
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// 完整链路：plan → apply → 部署落盘 → undo 恢复原状；幂等键复用任务。
#[test]
fn ail050_plan_apply_verify_undo_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&opts(tmp.path(), 17811)).unwrap();
    let auth = [(SESSION_HEADER, server.token.as_str())];

    // 准备仓库 + 个人库 skill + 宿主选择
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut git = std::process::Command::new("git");
    git.args(["init", "-q"]).current_dir(&repo);
    assert!(git.status().unwrap().success());
    let _ = post(
        server.port,
        "/api/fs/approve",
        &auth,
        json!({ "path": tmp.path() }),
    );
    let (code, _) = post(
        server.port,
        "/api/repo/discover",
        &auth,
        json!({ "path": repo }),
    );
    assert_eq!(code, 200);
    let (code, _) = post(
        server.port,
        "/api/profile/select",
        &auth,
        json!({ "host": "claude", "state": "enable" }),
    );
    assert_eq!(code, 200);
    // 导入 skill
    let skill_src = tmp.path().join("skills/job-flow");
    std::fs::create_dir_all(&skill_src).unwrap();
    std::fs::write(skill_src.join("SKILL.md"), "# job-flow\n\n说明\n").unwrap();
    let (code, _, stderr) = {
        let out = std::process::Command::new(ailoom_bin())
            .args([
                "--data-root",
                server.state.data_root.to_str().unwrap(),
                "library",
                "--action",
                "import",
                "--dir",
                skill_src.to_str().unwrap(),
                "--execute",
            ])
            .current_dir(&repo)
            .envs(isolate_env(tmp.path()))
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    assert_eq!(code, 0, "import: {stderr}");
    let _ = post(
        server.port,
        "/api/profile/select",
        &auth,
        json!({ "resource": "personal/skill/personal/job-flow", "state": "enable" }),
    );

    // plan 任务
    let (code, raw) = post(
        server.port,
        "/api/jobs/plan",
        &auth,
        json!({ "root": repo, "idempotency_key": "k1" }),
    );
    assert_eq!(code, 202, "{raw}");
    let plan_id = json_body(&raw)["job_id"].as_str().unwrap().to_string();
    // 幂等：同键再次请求返回同一任务
    let (code, raw) = post(
        server.port,
        "/api/jobs/plan",
        &auth,
        json!({ "root": repo, "idempotency_key": "k1" }),
    );
    assert_eq!(code, 202);
    assert_eq!(json_body(&raw)["job_id"].as_str().unwrap(), plan_id);

    let plan_job = wait_job(&server, &plan_id, Duration::from_secs(20));
    assert_eq!(plan_job["status"], "success", "{plan_job}");
    assert!(
        plan_job["result"]["pending"].as_u64().unwrap_or(0) > 0,
        "计划有待执行动作"
    );

    // apply 任务（引用 plan）
    let (code, raw) = post(
        server.port,
        "/api/jobs/apply",
        &auth,
        json!({ "plan_job_id": plan_id, "idempotency_key": "a1" }),
    );
    assert_eq!(code, 202, "{raw}");
    let apply_id = json_body(&raw)["job_id"].as_str().unwrap().to_string();
    let apply_job = wait_job(&server, &apply_id, Duration::from_secs(30));
    assert_eq!(apply_job["status"], "success", "{apply_job}");
    assert!(
        repo.join(".claude/skills/job-flow").exists(),
        "skill 已部署"
    );
    // 宿主验证状态：文件落盘 ≠ 宿主已加载
    let ver = &apply_job["result"]["verification"]["items"];
    assert!(
        ver.as_array().unwrap().iter().any(|i| {
            i["path"].as_str().unwrap_or("").contains("job-flow")
                && (i["host_state"] == "needs-new-session" || i["host_state"] == "deployed")
        }),
        "{ver}"
    );

    // 重连：任务可从磁盘恢复读取
    let disk = jobs::load_job(&server.state.data_root, &apply_id);
    assert!(disk.is_some(), "任务持久化");

    // undo：恢复到应用前
    let (code, raw) = post(
        server.port,
        "/api/jobs/undo",
        &auth,
        json!({ "id": apply_id }),
    );
    assert_eq!(code, 200, "{raw}");
    assert!(
        !repo.join(".claude/skills/job-flow").exists(),
        "撤销后 skill 移除"
    );
    let v = json_body(&raw);
    assert!(v["conflicts"].as_array().unwrap().is_empty(), "{v}");

    server.shutdown();
    server.join();
}

/// 指纹绑定：计划后修改配置 → apply 拒绝（旧计划不写错目标）。
#[test]
fn ail050_stale_plan_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&opts(tmp.path(), 17812)).unwrap();
    let auth = [(SESSION_HEADER, server.token.as_str())];
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut git = std::process::Command::new("git");
    git.args(["init", "-q"]).current_dir(&repo);
    let _ = git.status();
    let _ = post(
        server.port,
        "/api/fs/approve",
        &auth,
        json!({ "path": tmp.path() }),
    );
    let _ = post(
        server.port,
        "/api/repo/discover",
        &auth,
        json!({ "path": repo }),
    );

    let (code, raw) = post(
        server.port,
        "/api/jobs/plan",
        &auth,
        json!({ "root": repo }),
    );
    assert_eq!(code, 202, "{raw}");
    let plan_id = json_body(&raw)["job_id"].as_str().unwrap().to_string();
    wait_job(&server, &plan_id, Duration::from_secs(20));

    // 修改配置（profile）→ 指纹过期
    let (code, _) = post(
        server.port,
        "/api/profile/select",
        &auth,
        json!({ "host": "claude", "state": "enable" }),
    );
    assert_eq!(code, 200);

    let (code, raw) = post(
        server.port,
        "/api/jobs/apply",
        &auth,
        json!({ "plan_job_id": plan_id }),
    );
    assert_eq!(code, 202);
    let apply_id = json_body(&raw)["job_id"].as_str().unwrap().to_string();
    let apply_job = wait_job(&server, &apply_id, Duration::from_secs(20));
    assert_eq!(apply_job["status"], "failed", "{apply_job}");
    assert_eq!(apply_job["error"], "stale-plan", "旧计划必须被拒绝");

    server.shutdown();
    server.join();
}

/// 取消语义（确定性）：cancel_requested 在执行前置位 → cancelled。
#[test]
fn ail050_cancel_at_safe_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&opts(tmp.path(), 17813)).unwrap();
    let id = "apply-cancel-test".to_string();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let job = jobs::Job {
        id: id.clone(),
        kind: jobs::JobKind::Apply,
        status: jobs::JobStatus::Queued,
        created_at: ailoom::ids::now_iso(),
        updated_at: ailoom::ids::now_iso(),
        root: repo,
        scope: None,
        fingerprint: None,
        plan_job_id: None,
        idempotency_key: None,
        progress: vec![],
        result: Value::Null,
        undo: None,
        error: None,
        cancel_requested: true,
    };
    server.state.jobs.lock().unwrap().insert(id.clone(), job);
    jobs::run_apply_job(&server.state, &id);
    let job = server.state.jobs.lock().unwrap().get(&id).cloned().unwrap();
    assert_eq!(job.status, jobs::JobStatus::Cancelled, "取消在安全边界生效");
    server.shutdown();
    server.join();
}

fn ailoom_bin() -> PathBuf {
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

fn isolate_env(root: &std::path::Path) -> Vec<(String, String)> {
    vec![
        (
            "HOME".into(),
            root.join("home").to_string_lossy().into_owned(),
        ),
        (
            "USERPROFILE".into(),
            root.join("home").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".into(),
            root.join("xdg-data").to_string_lossy().into_owned(),
        ),
        (
            "XDG_STATE_HOME".into(),
            root.join("xdg-state").to_string_lossy().into_owned(),
        ),
        ("AILOOM_LOG".into(), "error".into()),
    ]
}

/// AIL-048：仓库默认变更影响预览（多工作树分别出计划，无写入）。
#[test]
fn ail048_repo_default_preview_across_worktrees() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&opts(tmp.path(), 17815)).unwrap();
    let auth = [(SESSION_HEADER, server.token.as_str())];
    let main = tmp.path().join("main");
    std::fs::create_dir_all(&main).unwrap();
    let mut git = std::process::Command::new("git");
    git.args(["init", "-q"]).current_dir(&main);
    assert!(git.status().unwrap().success());
    std::fs::write(main.join("r.txt"), "r").unwrap();
    let mut git = std::process::Command::new("git");
    git.args(["-c", "user.name=t", "-c", "user.email=t@t", "add", "."])
        .current_dir(&main);
    let _ = git.status();
    let mut git = std::process::Command::new("git");
    git.args([
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "b",
    ])
    .current_dir(&main);
    let _ = git.status();
    let wt2 = tmp.path().join("wt2");
    let mut git = std::process::Command::new("git");
    git.args(["worktree", "add", "-q", wt2.to_str().unwrap(), "-b", "b2"])
        .current_dir(&main);
    assert!(git.status().unwrap().success());

    let _ = post(
        server.port,
        "/api/fs/approve",
        &auth,
        json!({ "path": tmp.path() }),
    );
    let (code, _) = post(
        server.port,
        "/api/repo/discover",
        &auth,
        json!({ "path": main }),
    );
    assert_eq!(code, 200);
    // 主 worktree 部署一个 skill（当前工作树落点）
    let skill_src = tmp.path().join("skills/preview-flow");
    std::fs::create_dir_all(&skill_src).unwrap();
    std::fs::write(skill_src.join("SKILL.md"), "# preview-flow\n").unwrap();
    let _ = post(
        server.port,
        "/api/library/import",
        &auth,
        json!({ "dir": skill_src, "execute": true }),
    );
    let (code, _) = post(
        server.port,
        "/api/profile/select",
        &auth,
        json!({ "host": "claude", "state": "enable" }),
    );
    assert_eq!(code, 200);
    let (code, _) = post(
        server.port,
        "/api/profile/select",
        &auth,
        json!({ "resource": "personal/skill/personal/preview-flow", "state": "enable" }),
    );
    assert_eq!(code, 200);

    // 仓库默认影响预览：列出两个工作树的待执行数，且不写入
    let (code, raw) = post(server.port, "/api/preview/repo-default", &auth, json!({}));
    assert_eq!(code, 200, "{raw}");
    let v = json_body(&raw);
    let wts = v["worktrees"].as_array().unwrap();
    assert!(wts.len() >= 2, "两棵工作树都在预览中: {v}");
    let main_entry = wts
        .iter()
        .find(|w| w["worktree"].as_str().unwrap_or("").contains("main"))
        .expect("main 在预览中");
    eprintln!("PREVIEW={}", serde_json::to_string(&v).unwrap());
    assert!(
        main_entry["pending"].as_u64().unwrap() >= 1,
        "main 有待执行部署"
    );
    // 预览无写入
    assert!(
        !main.join(".claude/skills/preview-flow").exists(),
        "预览不写盘"
    );
    assert!(
        !wt2.join(".claude/skills/preview-flow").exists(),
        "wt2 不被预览写入"
    );

    server.shutdown();
    server.join();
}
