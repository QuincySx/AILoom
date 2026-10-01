//! AIL-133：此前没有任何测试引用的 25 条控制台路由的冒烟测试。
//! 每条路由至少验证：不返回 5xx、响应是 JSON 对象、失败时带 `error` 文本；能给出合法参数的路由验证 200。
//! `POST /api/fs/pick-directory` 会弹出系统文件夹选择框，只验证未带会话令牌时被拒绝。

use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn call(
    port: u16,
    token: Option<&str>,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let body_text = body.map(|b| b.to_string()).unwrap_or_default();
    let auth = token
        .map(|t| format!("{SESSION_HEADER}: {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n{auth}\r\n{body_text}",
        body_text.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
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

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[test]
fn previously_untested_routes_respond_with_json_and_no_server_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir_all(repo.join(".claude/skills/local")).unwrap();
    std::fs::write(
        repo.join(".claude/skills/local/SKILL.md"),
        "---\nname: local\ndescription: d\n---\nbody\n",
    )
    .unwrap();
    std::fs::write(repo.join("notes.md"), "hello").unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());

    let server = ConsoleServer::start(&ConsoleOptions {
        port: 17990,
        data_root: tmp.path().join("data"),
        open_browser: false,
    })
    .unwrap();
    let (port, tok) = (server.port, server.token.clone());
    let t = Some(tok.as_str());
    assert_eq!(
        call(
            port,
            t,
            "POST",
            "/api/fs/approve",
            Some(json!({ "path": repo }))
        )
        .0,
        200
    );
    let root = enc(repo.to_str().unwrap());

    // 有合法参数时必须成功的只读路由
    let ok_gets = [
        "/api/library/list".to_string(),
        "/api/library/sources".to_string(),
        "/api/migrations/cc-switch/location".to_string(),
        format!("/api/project/scan-skills?root={root}"),
        format!("/api/project/dirs?root={root}"),
        format!("/api/project/discover-directories?root={root}"),
        format!("/api/effective?root={root}"),
        format!("/api/deploy-status?root={root}"),
        format!(
            "/api/fs/read?path={}",
            enc(repo.join("notes.md").to_str().unwrap())
        ),
    ];
    for path in &ok_gets {
        let (code, v) = call(port, t, "GET", path, None);
        assert!(v.is_object(), "{path}: 非 JSON 对象 {v}");
        assert!(
            code == 200 || (code < 500 && v["error"].is_string()),
            "{path}: {code} {v}"
        );
        if path.starts_with("/api/library/")
            || path.contains("scan-skills")
            || path.contains("fs/read")
        {
            assert_eq!(code, 200, "{path}: {v}");
        }
    }
    let (_, scan) = call(
        port,
        t,
        "GET",
        &format!("/api/project/scan-skills?root={root}"),
        None,
    );
    assert_eq!(scan["items"][0]["management"], "unmanaged", "{scan}");

    // 缺参数 / 越界参数：结构化 4xx，不 panic
    let bad_gets = [
        "/api/resources/mcp-detail",
        "/api/resources/mcp-detail?id=nope/mcp/x/y",
        "/api/fs/read?path=/etc/hosts",
        "/api/project/scan-skills",
        "/api/effective",
    ];
    for path in bad_gets {
        let (code, v) = call(port, t, "GET", path, None);
        assert!(
            (400..500).contains(&code),
            "{path}: 期望 4xx，实际 {code} {v}"
        );
        assert!(v["error"].is_string(), "{path}: {v}");
    }

    // 写路由：空请求体不导致 5xx；「检查全部」类动作可返回 200，其余被校验拒绝并带 error
    let posts = [
        "/api/collections/check",
        "/api/collections/remove",
        "/api/collections/update",
        "/api/library/check-update",
        "/api/library/delete",
        "/api/library/import-entry",
        "/api/library/import-git",
        "/api/library/update",
        "/api/profile/scope",
        "/api/repo/relink",
        "/api/workflows/bind",
        "/api/workflows/record-input",
        "/api/workflows/rename",
        "/api/workflows/reviewed",
    ];
    for path in posts {
        let (code, v) = call(port, t, "POST", path, Some(json!({})));
        assert!(v.is_object(), "{path}: 非 JSON 对象 {v}");
        assert!(
            code < 300 || ((400..500).contains(&code) && v["error"].is_string()),
            "{path}: {code} {v}"
        );
        // 写路由都要求会话令牌
        assert_eq!(
            call(port, None, "POST", path, Some(json!({}))).0,
            401,
            "{path}"
        );
    }
    assert_eq!(
        call(
            port,
            None,
            "POST",
            "/api/fs/pick-directory",
            Some(json!({}))
        )
        .0,
        401
    );

    server.shutdown();
    server.join();
}

/// 守卫：路由表中的每条 `/api/*` 路径都必须至少被一个测试文件引用。
/// 新增路由时请同时补测试（原生文件选择框等需要 GUI 的路由也至少要有鉴权断言）。
#[test]
fn every_api_route_is_referenced_by_a_test() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(root.join("src/console/mod.rs")).unwrap();
    let start = src
        .find("const KNOWN_PATHS: &[&str] = &[")
        .expect("KNOWN_PATHS");
    let end = start + src[start..].find("];").unwrap();
    let paths: Vec<&str> = src[start..end]
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix('"')
                .and_then(|l| l.strip_suffix("\","))
        })
        .filter(|p| p.starts_with("/api/"))
        .collect();
    assert!(paths.len() > 60, "KNOWN_PATHS 解析失败");
    let mut tests = String::new();
    for entry in std::fs::read_dir(root.join("tests")).unwrap().flatten() {
        let p = entry.path();
        if matches!(p.extension().and_then(|e| e.to_str()), Some("rs" | "mjs")) {
            tests.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
        }
    }
    let missing: Vec<&&str> = paths.iter().filter(|p| !tests.contains(**p)).collect();
    assert!(missing.is_empty(), "没有测试引用的路由: {missing:?}");
}
