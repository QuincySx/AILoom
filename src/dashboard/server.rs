//! 本地实时看板（AIL-021）：loopback HTTP + SSE，复用聚合统计；不含任何遥测外发。

use crate::appctx::AppContext;
use crate::error::Result;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// 看板状态版本号（SSE 游标）。
static STATE_VERSION: AtomicU64 = AtomicU64::new(0);

pub struct DashboardArgs {
    pub port: u16,
    pub root: Option<PathBuf>,
}

/// 启动看板服务器（阻塞）。仅绑定 127.0.0.1；服务端做工作区过滤。
pub fn run(args: &DashboardArgs, data_root: Option<&std::path::Path>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let addr = format!("127.0.0.1:{}", args.port);
    let listener = TcpListener::bind(&addr).map_err(|e| {
        crate::error::Error::new(
            crate::error::code::INTERNAL,
            format!("端口 {} 绑定失败: {e}", args.port),
        )
        .fix("换一个端口（--port）；确认没有其它 ailoom dashboard 在运行")
    })?;
    crate::logging::info(format!("看板已启动：http://{addr}（仅本机可访问）"));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let ctx_ref = ctx.layout.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, &ctx_ref);
                });
            }
            Err(e) => crate::logging::warn(format!("连接错误: {e}")),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, layout: &crate::paths::WsLayout) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    if method != "GET" {
        return respond(
            &mut stream,
            405,
            "Method Not Allowed",
            "text/plain",
            "仅支持 GET",
        );
    }
    match path {
        "/" | "/index.html" => {
            let body = render_page();
            respond(&mut stream, 200, "OK", "text/html; charset=utf-8", &body)
        }
        "/api/state" => {
            let state = build_state(layout);
            let body = serde_json::to_string_pretty(&state).unwrap_or_default();
            respond(&mut stream, 200, "OK", "application/json", &body)
        }
        "/api/events" => {
            // SSE：游标重连——客户端带 Last-Event-ID 或缺省立即推快照
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n")
                .unwrap_or_default();
            let _ = stream.flush();
            let last_event_id = reader
                .lines()
                .take(20)
                .find_map(|l| {
                    l.ok().and_then(|line| {
                        line.strip_prefix("Last-Event-ID:")
                            .map(|v| v.trim().to_string())
                    })
                })
                .and_then(|v| v.parse::<u64>().ok());
            let last = last_event_id.unwrap_or(0);
            let mut cursor = STATE_VERSION.load(Ordering::SeqCst);
            if cursor > last {
                // 补齐：立即推送当前全量快照
                let state = build_state(layout);
                let body = serde_json::to_string(&state).unwrap_or_default();
                let _ = stream.write_all(
                    format!("id: {cursor}\nevent: snapshot\ndata: {body}\n\n").as_bytes(),
                );
                let _ = stream.flush();
            }
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let v = STATE_VERSION.load(Ordering::SeqCst);
                if v != cursor {
                    cursor = v;
                    let state = build_state(layout);
                    let body = serde_json::to_string(&state).unwrap_or_default();
                    if stream
                        .write_all(
                            format!("id: {cursor}\nevent: snapshot\ndata: {body}\n\n").as_bytes(),
                        )
                        .is_err()
                    {
                        break; // 客户端断开
                    }
                    let _ = stream.flush();
                }
            }
            Ok(())
        }
        _ => respond(&mut stream, 404, "Not Found", "text/plain", "未知路径"),
    }
}

/// 服务端构建状态（过滤在本端执行）；复用 AIL-019 聚合，不在前端另算。
fn build_state(layout: &crate::paths::WsLayout) -> serde_json::Value {
    let heuristic = crate::events::aggregate::HeuristicConfig::default();
    let cfg = crate::events::friction::FrictionConfig::load(&layout.ws_dir);
    let (events, bad) = crate::events::store::read_events(&layout.events_file).unwrap_or_default();
    // workspace_id 由事件携带：按事件自身的工作区分组聚合（互不混淆）
    let mut by_session: std::collections::BTreeMap<
        (String, String),
        Vec<crate::events::schema::Event>,
    > = Default::default();
    for e in &events {
        by_session
            .entry((e.workspace_id.clone(), e.session_id.clone()))
            .or_default()
            .push(e.clone());
    }
    let mut sessions: std::collections::BTreeMap<String, crate::events::aggregate::SessionMetrics> =
        Default::default();
    for ((wid, sid), evs) in &by_session {
        if let Ok(m) = crate::events::aggregate::aggregate_session(wid, sid, evs, &heuristic) {
            sessions.insert(format!("{wid}/{sid}"), m);
        }
    }
    let list: Vec<serde_json::Value> = sessions
        .values()
        .map(|m| {
            serde_json::json!({
                "session_id": m.session_id,
                "workspace_id": m.workspace_id,
                "state": session_state(m, &events),
                "prompt_count": m.prompt_count,
                "tool_calls": m.tool_calls,
                "stop_count": m.stop_count,
                "tool_errors": m.tool_errors,
                "interventions": m.interventions,
                "friction_score": crate::events::friction::friction_score(m, &cfg),
                "tokens": {
                    "input": m.tokens.input,
                    "output": m.tokens.output,
                    "availability": m.tokens_availability,
                },
                "last_event_at": m.last_event_at,
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": 1,
        "bad_event_lines": bad,
        "sessions": list,
        "note": "本地视图：仅本工作区采集数据，不代表全团队实时状态",
    })
}

/// 会话状态：running（有 start 无 stop）/idle/unknown。区分回答结束与进程退出是不可能的（Stop≠退出），以事件推断。
fn session_state(
    m: &crate::events::aggregate::SessionMetrics,
    _events: &[crate::events::schema::Event],
) -> String {
    if m.last_event_at.is_some() && m.stop_count > 0 {
        "idle".into()
    } else if m.started_at.is_some() {
        "running".into()
    } else {
        "unknown".into()
    }
}

fn render_page() -> String {
    r#"<!doctype html>
<html lang="zh"><head><meta charset="utf-8"><title>AILoom 本地看板</title>
<style>body{font-family:system-ui;max-width:960px;margin:2rem auto;padding:0 1rem}
table{border-collapse:collapse;width:100%}td,th{border:1px solid #ddd;padding:6px 10px;text-align:left}
.unavailable{color:#b8860b}</style></head>
<body>
<h1>AILoom 本地看板</h1>
<p>本页仅展示本工作区采集的数据，不代表全团队实时状态。</p>
<table id="t"><thead><tr><th>会话</th><th>状态</th><th>prompts</th><th>tools</th><th>errors</th><th>interventions</th><th>tokens in</th></tr></thead><tbody></tbody></table>
<script>
const tb = document.querySelector('#t tbody');
function esc(s){const d=document.createElement('div');d.textContent=String(s??'');return d.innerHTML;}
function render(s){
  tb.innerHTML = (s.sessions||[]).map(x=>'<tr>'+
    '<td>'+esc(x.session_id)+'</td><td>'+esc(x.state)+'</td><td>'+esc(x.prompt_count)+'</td>'+
    '<td>'+esc(x.tool_calls)+'</td><td>'+esc(x.tool_errors)+'</td><td>'+esc(x.interventions)+'</td>'+
    '<td class="'+(x.tokens.availability==='unavailable'?'unavailable':'')+'">'+
    (x.tokens.input==null?'unavailable':esc(x.tokens.input))+'</td></tr>').join('');
}
fetch('/api/state').then(r=>r.json()).then(render);
const es = new EventSource('/api/events');
es.addEventListener('snapshot', e => render(JSON.parse(e.data)));
</script></body></html>"#.to_string()
}

fn respond(
    stream: &mut TcpStream,
    code: u16,
    status: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

/// 版本推进（事件写入方调用，推进 SSE 游标）。
pub fn bump_version() {
    STATE_VERSION.fetch_add(1, Ordering::SeqCst);
}
