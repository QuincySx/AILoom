//! 控制台 HTTP 底层：请求解析、百分号解码、响应写出与 Host / Origin 校验。

use super::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Request {
    /// 查询参数（同名取第一个）。
    pub fn query_param(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
    pub extra_headers: Vec<(String, String)>,
}

impl Response {
    /// 业务错误的统一响应：`{error, code, hint, context}`。`error` 只放面向用户的说明，
    /// 不再拼接 `[E….] … | context:` 这类 Display 文本。
    pub fn error(status: u16, e: &crate::error::Error) -> Response {
        Response::json(
            status,
            json!({ "error": e.message, "code": e.code, "hint": e.fix, "context": e.context }),
        )
    }

    pub fn json(status: u16, v: Value) -> Response {
        Response {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_string(&v).unwrap_or_default(),
            extra_headers: Vec::new(),
        }
    }
}

pub(super) fn handle_conn(stream: TcpStream, state: &Arc<ServerState>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = parse_target(&target);
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for _ in 0..128 {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim().to_string();
            if k == "content-length" {
                content_length = v.parse().unwrap_or(0);
            }
            headers.push((k, v));
        }
    }
    let mut body_bytes = vec![0u8; content_length.min(1 << 20)];
    if content_length > 0 {
        reader.read_exact(&mut body_bytes)?;
    }
    let body: Value = if body_bytes.is_empty() {
        Value::Null
    } else {
        match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                let resp =
                    Response::json(400, json!({ "error": format!("请求体不是合法 JSON：{e}") }));
                return write_response(stream, &resp);
            }
        }
    };
    let req = Request {
        method,
        path,
        query,
        headers,
        body,
    };
    let resp = route(&req, state);
    write_response(stream, &resp)
}

pub(super) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 && i + 2 <= bytes.len() - 1 + 1 {
            let hex = |b: u8| -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            };
            if i + 2 < bytes.len() {
                if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push(h * 16 + l);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(super) fn parse_target(target: &str) -> (String, Vec<(String, String)>) {
    let (path, qs) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (target.to_string(), None),
    };
    let mut query = Vec::new();
    if let Some(qs) = qs {
        for pair in qs.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            query.push((percent_decode(k), percent_decode(v)));
        }
    }
    (path, query)
}

pub(super) fn write_response(mut stream: TcpStream, resp: &Response) -> std::io::Result<()> {
    let status_text = match resp.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {ct}\r\nContent-Length: {len}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
        status = resp.status,
        status_text = status_text,
        ct = resp.content_type,
        len = resp.body.len()
    );
    for (k, v) in &resp.extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(resp.body.as_bytes())?;
    stream.flush()
}

pub(super) fn loopback_host_allowed(host: &str, port: u16) -> bool {
    let h = host.to_ascii_lowercase();
    h == format!("127.0.0.1:{port}")
        || h == format!("localhost:{port}")
        || h == format!("[::1]:{port}")
}

pub(super) fn origin_allowed(origin: &str, port: u16) -> bool {
    let o = origin.to_ascii_lowercase();
    o == format!("http://127.0.0.1:{port}")
        || o == format!("http://localhost:{port}")
        || o == format!("http://[::1]:{port}")
}
