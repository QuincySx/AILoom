//! AIL-152：全局 Skill 网页接口。
//! 服务在测试进程内运行，全局部署写的是本进程的 HOME：本文件只有一个测试，
//! 启动服务前把 HOME / XDG 指向临时目录并清除 CLAUDE_CONFIG_DIR，绝不写真实用户目录。

use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn call(
    port: u16,
    token: Option<&str>,
    verb: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let auth = token
        .map(|t| format!("{SESSION_HEADER}: {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{verb} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n{auth}\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    let status = buf
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let idx = buf.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    (
        status,
        serde_json::from_str(&buf[idx..]).unwrap_or(Value::Null),
    )
}

#[cfg(unix)]
#[test]
fn global_skill_api_selects_applies_takes_over_and_restores() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_DATA_HOME", tmp.path().join("xdg-data"));
    std::env::set_var("XDG_STATE_HOME", tmp.path().join("xdg-state"));
    std::env::remove_var("CLAUDE_CONFIG_DIR");
    let data = tmp.path().join("data");

    let src = tmp.path().join("skills/web-skill");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: web-skill\ndescription: d\n---\nbody\n",
    )
    .unwrap();
    ailoom::personal_library::import_execute(&data, &src, None).unwrap();
    // 全局目录里已有的同名外部条目
    let foreign = tmp.path().join("foreign/web-skill");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(foreign.join("SKILL.md"), "---\nname: web-skill\n---\nX\n").unwrap();
    std::os::unix::fs::symlink(&foreign, home.join(".claude/skills/web-skill")).unwrap();

    let server = ConsoleServer::start(&ConsoleOptions {
        port: 0,
        data_root: data.clone(),
        open_browser: false,
    })
    .unwrap();
    let (port, token) = (server.port, server.token.clone());
    let t = Some(token.as_str());
    let id = "personal/skill/personal/web-skill";

    let (code, v) = call(port, t, "GET", "/api/global/skills", None);
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["skills"][0]["id"], id, "{v}");
    assert_eq!(v["skills"][0]["global"], false);
    let revision = v["revision"].as_u64().unwrap();

    // 写接口都要求会话令牌；过期 revision 被拒绝
    for path in [
        "/api/global/select",
        "/api/global/apply",
        "/api/global/takeover",
        "/api/global/restore",
    ] {
        assert_eq!(
            call(port, None, "POST", path, Some(json!({}))).0,
            401,
            "{path}"
        );
    }
    let (code, _) = call(
        port,
        t,
        "POST",
        "/api/global/select",
        Some(json!({"skill": id, "enabled": true, "base_revision": revision + 7})),
    );
    assert_eq!(code, 409);
    let (code, v) = call(
        port,
        t,
        "POST",
        "/api/global/select",
        Some(json!({"skill": id, "enabled": true, "base_revision": revision})),
    );
    assert_eq!(code, 200, "{v}");
    let (code, v) = call(
        port,
        t,
        "POST",
        "/api/global/select",
        Some(json!({"skill": "personal/skill/personal/none", "enabled": true})),
    );
    assert_eq!(code, 400, "{v}");

    let (code, plan) = call(port, t, "GET", "/api/global/plan", None);
    assert_eq!(code, 200, "{plan}");
    assert_eq!(plan["has_conflicts"], true, "{plan}");
    let (code, v) = call(port, t, "POST", "/api/global/apply", Some(json!({})));
    assert_eq!(code, 200, "{v}");
    assert!(home.join(".agents/skills/web-skill/SKILL.md").is_file());
    assert_eq!(
        std::fs::read_link(home.join(".claude/skills/web-skill")).unwrap(),
        foreign
    );

    let (_, status) = call(port, t, "GET", "/api/global/skills", None);
    let f = &status["foreign"][0];
    assert_eq!(f["conflicts_with"], id, "{status}");
    let (code, v) = call(
        port,
        t,
        "POST",
        "/api/global/takeover",
        Some(json!({"target": "claude", "name": "../evil"})),
    );
    assert_eq!(code, 400, "{v}");
    let (code, v) = call(
        port,
        t,
        "POST",
        "/api/global/takeover",
        Some(json!({"target": "claude", "name": "web-skill"})),
    );
    assert_eq!(code, 200, "{v}");
    let archive = v["archived"]["id"].as_str().unwrap().to_string();
    call(port, t, "POST", "/api/global/apply", Some(json!({})));
    assert!(home.join(".claude/skills/web-skill/SKILL.md").is_file());
    let (code, _) = call(
        port,
        t,
        "POST",
        "/api/global/restore",
        Some(json!({"id": archive})),
    );
    assert_eq!(code, 409, "原位置被占用时拒绝还原");

    call(
        port,
        t,
        "POST",
        "/api/global/select",
        Some(json!({"skill": id, "enabled": false})),
    );
    call(port, t, "POST", "/api/global/apply", Some(json!({})));
    let (code, v) = call(
        port,
        t,
        "POST",
        "/api/global/restore",
        Some(json!({"id": archive})),
    );
    assert_eq!(code, 200, "{v}");
    assert_eq!(
        std::fs::read_link(home.join(".claude/skills/web-skill")).unwrap(),
        foreign
    );
    assert!(!home.join(".codex").exists(), "不写宿主全局配置");

    server.shutdown();
    server.join();
}
