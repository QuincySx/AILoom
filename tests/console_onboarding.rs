//! AIL-047 集成验收：个人模式 onboarding 首次成功体验。
//! 无团队源、无网络、无现成声明、零 TOML：选目录 → 确认仓库/worktree → 选宿主
//! 与能力（导入 skill）→ 预览 → 应用 → 得到真实调用验证动作；公司跟踪文件不变。

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};

fn opts(tmp: &std::path::Path, port: u16) -> ConsoleOptions {
    ConsoleOptions {
        port,
        data_root: tmp.join("data"),
        open_browser: false,
    }
}

fn method(
    port: u16,
    verb: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> (u16, String) {
    let body_text = body.map(|b| b.to_string()).unwrap_or_default();
    let req = format!(
        "{verb} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{SESSION_HEADER}: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_text}",
        headers.iter().find(|(k,_)| *k==SESSION_HEADER).map(|(_,v)| v.to_string()).unwrap_or_default(),
        body_text.len(),
    );
    let mut buf = String::new();
    for _ in 0..3 {
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
        stream.write_all(req.as_bytes()).unwrap();
        buf.clear();
        let _ = stream.read_to_string(&mut buf);
        let status: u16 = buf
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if status != 0 {
            break; // 拿到完整响应
        }
        // 空响应（瞬时拥塞）：重试
        std::thread::sleep(Duration::from_millis(200));
    }
    let status: u16 = buf
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, buf)
}

fn jb(raw: &str) -> Value {
    let idx = raw.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    serde_json::from_str(&raw[idx..]).unwrap_or(Value::Null)
}

fn post(port: u16, path: &str, token: &str, body: Value) -> (u16, Value) {
    let (code, raw) = method(port, "POST", path, &[(SESSION_HEADER, token)], Some(&body));
    (code, jb(&raw))
}

fn wait_job(server: &ConsoleServer, id: &str) -> Value {
    for _ in 0..200 {
        if let Some(job) = server.state.jobs.lock().unwrap().get(id) {
            if !matches!(
                job.status,
                ailoom::console::jobs::JobStatus::Queued
                    | ailoom::console::jobs::JobStatus::Running
            ) {
                return serde_json::to_value(job).unwrap();
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    panic!("任务超时: {id}");
}

/// 首次使用六步闭环（与前端 JS 的调用序列一一对应）。
#[test]
fn ail047_onboarding_first_run_closed_loop() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&opts(tmp.path(), 17910)).unwrap();
    let token = server.token.clone();

    // 前端页面可达：壳注入会话令牌并加载 ES module 应用（AIL-080 分层架构）；
    // 向导要素位于 /ui 模块（onboarding 页面与任务/验证组件）
    let (code, raw) = method(server.port, "GET", "/", &[], None);
    assert_eq!(code, 200);
    assert!(raw.contains(&token), "页面注入会话令牌");
    assert!(raw.contains("/ui/app.js"), "壳加载模块化应用");
    for asset in [
        "/ui/tokens.css",
        "/ui/pages/onboarding.js",
        "/ui/features/planPreview.js",
    ] {
        let (code, path) = method(server.port, "GET", asset, &[], None);
        assert_eq!(code, 200, "资产可达: {asset}");
        assert!(
            path.contains("text/css") || path.contains("javascript"),
            "正确 MIME: {asset}"
        );
    }
    assert!(
        ailoom::console::ui::PAGE_ONBOARDING_JS.contains("选择一个要使用 Skill / MCP 的项目")
            && ailoom::console::ui::PAGE_ONBOARDING_JS.contains("选择宿主和要使用的资源")
            && ailoom::console::ui::PLAN_PREVIEW_JS.contains("预览将要做的改动")
            && ailoom::console::ui::PLAN_PREVIEW_JS.contains("在宿主里真实验证")
            && ailoom::console::ui::APP_JS.contains("hashchange"),
        "向导与路由要素分布在对应模块内"
    );

    // 公司样仓：含已跟踪 AGENTS.md（用户自己还有未暂存修改）
    let repo = tmp.path().join("company-repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut git = Command::new("git");
    git.args(["init", "-q"]).current_dir(&repo);
    assert!(git.status().unwrap().success());
    std::fs::write(repo.join("AGENTS.md"), "# 公司规范\n\n- 提交信息用英文\n").unwrap();
    std::fs::write(repo.join("README.md"), "demo").unwrap();
    let mut g = Command::new("git");
    g.args([
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "add",
        "--",
        "AGENTS.md",
        "README.md",
    ])
    .current_dir(&repo);
    assert!(g.status().unwrap().success(), "git add 失败");
    let mut g = Command::new("git");
    g.args([
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "base",
    ])
    .current_dir(&repo);
    assert!(g.status().unwrap().success(), "git commit 失败");
    // 用户本地未提交修改（必须保持原样）
    std::fs::write(
        repo.join("AGENTS.md"),
        "# 公司规范\n\n- 提交信息用英文\n- 用户的本地未提交修改\n",
    )
    .unwrap();
    let agents_content = std::fs::read(repo.join("AGENTS.md")).unwrap();

    // Step 1：批准目录
    let (code, v) = post(
        server.port,
        "/api/fs/approve",
        &token,
        json!({ "path": tmp.path() }),
    );
    assert_eq!(code, 200, "{v}");

    // Step 2：发现仓库 → 归组 + 工作树
    let (code, v) = post(
        server.port,
        "/api/repo/discover",
        &token,
        json!({ "path": repo }),
    );
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["kind"], "git");
    assert_eq!(v["worktrees"].as_array().unwrap().len(), 1);

    // Step 3：宿主探测（只读）+ 导入 skill + 选宿主/能力
    let (code, v) = post(server.port, "/api/hosts/detect", &token, json!({}));
    assert_eq!(code, 200, "{v}");
    let skill_src = tmp.path().join("my-skills/onboard-flow");
    std::fs::create_dir_all(&skill_src).unwrap();
    std::fs::write(
        skill_src.join("SKILL.md"),
        "# onboard-flow\n\n回复固定短语：ONBOARD-MARKER-047\n",
    )
    .unwrap();
    // 预览（默认不复制）
    let (code, v) = post(
        server.port,
        "/api/library/import",
        &token,
        json!({ "dir": skill_src }),
    );
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["executed"], json!(false));
    // 导入
    let (code, v) = post(
        server.port,
        "/api/library/import",
        &token,
        json!({ "dir": skill_src, "execute": true }),
    );
    assert_eq!(code, 200, "{v}");
    assert_eq!(
        v["report"]["skill_id"],
        json!("personal/skill/personal/onboard-flow")
    );
    // 选宿主 + 启用技能
    let (code, _) = post(
        server.port,
        "/api/profile/select",
        &token,
        json!({ "host": "claude", "state": "enable", "root": repo }),
    );
    assert_eq!(code, 200);
    let (code, _) = post(
        server.port,
        "/api/profile/select",
        &token,
        json!({ "resource": "personal/skill/personal/onboard-flow", "state": "enable", "root": repo }),
    );
    assert_eq!(code, 200);

    // Step 4：预览
    let (code, v) = post(
        server.port,
        "/api/jobs/plan",
        &token,
        json!({ "root": repo }),
    );
    assert_eq!(code, 202, "{v}");
    let plan_id = v["job_id"].as_str().unwrap().to_string();
    let plan_job = wait_job(&server, &plan_id);
    assert_eq!(plan_job["status"], "success", "{plan_job}");

    // Step 5：应用
    let (code, v) = post(
        server.port,
        "/api/jobs/apply",
        &token,
        json!({ "plan_job_id": plan_id }),
    );
    assert_eq!(code, 202, "{v}");
    let apply_id = v["job_id"].as_str().unwrap().to_string();
    let apply_job = wait_job(&server, &apply_id);
    assert_eq!(apply_job["status"], "success", "{apply_job}");
    assert!(
        repo.join(".claude/skills/onboard-flow").exists(),
        "技能已部署"
    );

    // Step 6：验证状态不是「宿主已加载」，而是「需新会话/需批准」+ 下一步动作
    let ver = &apply_job["result"]["verification"];
    assert_eq!(
        ver["invocation"],
        json!("manual"),
        "真实调用由用户在宿主内执行"
    );
    let flow_item = ver["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["path"].as_str().unwrap_or("").contains("onboard-flow"))
        .unwrap();
    assert!(
        flow_item["host_state"] == "needs-new-session",
        "文件落盘 ≠ 宿主已加载: {flow_item}"
    );
    assert!(apply_job["result"]["next"]
        .as_str()
        .unwrap_or("")
        .contains("新会话"));

    // 全程公司文件零改动（含用户未提交修改）
    assert_eq!(
        std::fs::read(repo.join("AGENTS.md")).unwrap(),
        agents_content,
        "公司 AGENTS.md（含 unstaged 修改）逐字节不变"
    );
    let status = {
        let out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert!(
        status
            .lines()
            .all(|l| l.starts_with(" M") || l.starts_with("M ")),
        "git status 只有用户自己的修改，没有 AILoom 新增文件（exclude 收录）: {status}"
    );

    // 一键撤销本次工具改动
    let (code, v) = post(
        server.port,
        "/api/jobs/undo",
        &token,
        json!({ "id": apply_id }),
    );
    assert_eq!(code, 200, "{v}");
    assert!(
        !repo.join(".claude/skills/onboard-flow").exists(),
        "撤销后移除本次部署"
    );
    assert_eq!(
        std::fs::read(repo.join("AGENTS.md")).unwrap(),
        agents_content,
        "撤销后公司文件依旧不变"
    );

    server.shutdown();
    server.join();
}
