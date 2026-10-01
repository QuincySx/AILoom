//! AIL-132：控制台 API 契约——非法 JSON 400、已知路径错误方法 405、
//! 业务错误为 `{error, code, hint, context}`（error 不再拼 Display 文本），启动 URL 不带 token。

use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn send(port: u16, method: &str, path: &str, token: &str, body: &str) -> (u16, Value) {
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n{SESSION_HEADER}: {token}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
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

#[test]
fn console_api_contract() {
    let tmp = tempfile::tempdir().unwrap();
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 17980,
        data_root: tmp.path().join("data"),
        open_browser: false,
    })
    .unwrap();
    let (port, tok) = (server.port, server.token.clone());

    let (code, v) = send(port, "POST", "/api/fs/approve", &tok, "{not json");
    assert_eq!(code, 400, "{v}");
    assert!(v["error"].as_str().unwrap().contains("JSON"), "{v}");

    let (code, v) = send(port, "GET", "/api/fs/approve", &tok, "");
    assert_eq!(code, 405, "{v}");
    let (code, _) = send(port, "GET", "/api/does-not-exist", &tok, "");
    assert_eq!(code, 404);

    // 业务错误：结构化字段，error 不含 `[E` 前缀与 `| context:`。
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let approve = format!("{{\"path\":{:?}}}", repo.to_str().unwrap());
    assert_eq!(send(port, "POST", "/api/fs/approve", &tok, &approve).0, 200);
    let body = format!(
        "{{\"root\":{:?},\"resource\":\"bad\",\"state\":\"enable\"}}",
        repo.to_str().unwrap()
    );
    let (code, v) = send(port, "POST", "/api/profile/select", &tok, &body);
    assert_eq!(code, 400, "{v}");
    assert_eq!(v["code"], "E3004", "{v}");
    let msg = v["error"].as_str().unwrap();
    assert!(
        !msg.starts_with("[E") && !msg.contains("| context:"),
        "{msg}"
    );

    server.shutdown();
    server.join();
}
