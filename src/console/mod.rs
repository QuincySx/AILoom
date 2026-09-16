//! 本地 Web 控制台（AIL-046/050）：仅 loopback 的受限写 API 服务与任务队列。
//!
//! 安全模型：启动生成一次性会话 token（每实例独立，互不串会话）；写请求必须带
//! `X-AILoom-Session`；Origin/Host 白名单只接受本机回环地址（拒绝跨站与 DNS
//! rebinding 形式的未知 Host）；不通配 CORS；目录访问仅限用户显式批准的根，
//! canonicalize 后校验边界（拒绝符号链接逃逸）；不提供任意 shell/CLI 执行接口。
//! 业务调用全部复用内部服务（仓库发现/个人配置/计划/同步）。

use crate::error::Result;
use crate::ids::new_id;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const SESSION_HEADER: &str = "x-ailoom-session";

#[derive(Debug, Clone)]
pub struct ConsoleOptions {
    pub port: u16,
    pub data_root: PathBuf,
    pub open_browser: bool,
}

pub struct ServerState {
    pub port: u16,
    pub data_root: PathBuf,
    pub token: String,
    pub approved_roots: Mutex<Vec<PathBuf>>,
    /// 配置草稿（revision/ETag 防并发覆盖）
    pub draft: Mutex<Option<(u64, Value)>>,
    pub shutdown_flag: Mutex<bool>,
    /// 任务事件序列（AIL-050 任务进度在此追加，SSE 订阅读取）
    pub events: Mutex<Vec<Value>>,
    /// 任务账本（AIL-050）
    pub jobs: Mutex<std::collections::BTreeMap<String, jobs::Job>>,
}

/// 运行中的控制台句柄（测试与命令共用）。
pub struct ConsoleServer {
    pub port: u16,
    pub token: String,
    pub state: Arc<ServerState>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ConsoleServer {
    /// 启动（绑定成功即返回；端口占用自动递增重试）。
    pub fn start(opts: &ConsoleOptions) -> Result<ConsoleServer> {
        let mut last_err = None;
        for p in opts.port..opts.port.saturating_add(50) {
            match TcpListener::bind(("127.0.0.1", p)) {
                Ok(listener) => {
                    let token = new_id();
                    let state = Arc::new(ServerState {
                        port: p,
                        data_root: opts.data_root.clone(),
                        token: token.clone(),
                        approved_roots: Mutex::new(Vec::new()),
                        draft: Mutex::new(None),
                        shutdown_flag: Mutex::new(false),
                        events: Mutex::new(Vec::new()),
                        jobs: Mutex::new(Default::default()),
                    });
                    let st = Arc::clone(&state);
                    let thread = std::thread::spawn(move || serve(listener, st));
                    return Ok(ConsoleServer {
                        port: p,
                        token,
                        state,
                        thread: Some(thread),
                    });
                }
                Err(e) => {
                    last_err = Some((p, e));
                    continue;
                }
            }
        }
        let (p, e) = last_err.unwrap_or_else(|| (opts.port, std::io::Error::other("无可用端口")));
        Err(crate::error::Error::new(
            crate::error::code::INTERNAL,
            format!("端口 {p} 起连续 50 个端口均绑定失败: {e}"),
        ))
    }

    /// 打开浏览器（尽力而为，失败不影响服务）。
    pub fn open_browser(&self) {
        let url = format!("http://127.0.0.1:{}/?token={}", self.port, self.token);
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&url).output();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&url).output();
        }
        #[cfg(not(unix))]
        {
            let _ = url;
        }
    }

    pub fn shutdown(&self) {
        *self.state.shutdown_flag.lock().unwrap() = true;
        // 打一次空闲连接唤醒 accept 循环
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }

    /// 追加任务事件（AIL-050 任务进度推送入口）。
    pub fn push_event(&self, event: Value) {
        self.state.events.lock().unwrap().push(event);
    }

    pub fn join(mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ConsoleServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve(listener: TcpListener, state: Arc<ServerState>) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    loop {
        if *state.shutdown_flag.lock().unwrap() {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let st = Arc::clone(&state);
                std::thread::spawn(move || {
                    // macOS：accept 出的连接继承 listener 的非阻塞模式，
                    // 必须显式恢复阻塞，否则读请求 EAGAIN 会丢响应
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
                    // 单连接 handler panic 不允许拖垮服务：捕获并尝试回 500
                    let probe = stream.try_clone();
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match probe {
                            Ok(s) => {
                                let r = handle_conn(s, &st);
                                if let Err(e) = &r {
                                    crate::logging::warn(format!("控制台连接处理错误: {e}"));
                                }
                            }
                            Err(e) => crate::logging::warn(format!("连接克隆失败: {e}")),
                        }));
                    if result.is_err() {
                        crate::logging::warn("控制台连接处理发生内部错误（已忽略该请求）");
                        let _ = write_response(
                            stream,
                            &Response::json(500, json!({ "error": "内部错误" })),
                        );
                    }
                });
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
    pub extra_headers: Vec<(String, String)>,
}

impl Response {
    pub fn json(status: u16, v: Value) -> Response {
        Response {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_string(&v).unwrap_or_default(),
            extra_headers: Vec::new(),
        }
    }
}

fn handle_conn(stream: TcpStream, state: &Arc<ServerState>) -> std::io::Result<()> {
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
        serde_json::from_slice(&body_bytes).unwrap_or(Value::Null)
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

fn parse_target(target: &str) -> (String, Vec<(String, String)>) {
    let (path, qs) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (target.to_string(), None),
    };
    let mut query = Vec::new();
    if let Some(qs) = qs {
        for pair in qs.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            query.push((k.to_string(), v.to_string()));
        }
    }
    (path, query)
}

fn write_response(mut stream: TcpStream, resp: &Response) -> std::io::Result<()> {
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

// ---------------------------------------------------------------------------
// 路由与安全检查
// ---------------------------------------------------------------------------

pub mod jobs;
pub mod web;

fn loopback_host_allowed(host: &str, port: u16) -> bool {
    let h = host.to_ascii_lowercase();
    h == format!("127.0.0.1:{port}")
        || h == format!("localhost:{port}")
        || h == format!("[::1]:{port}")
}

fn origin_allowed(origin: &str, port: u16) -> bool {
    let o = origin.to_ascii_lowercase();
    o == format!("http://127.0.0.1:{port}")
        || o == format!("http://localhost:{port}")
        || o == format!("http://[::1]:{port}")
}

pub fn route(req: &Request, state: &Arc<ServerState>) -> Response {
    // Host 校验：拒绝 DNS rebinding 形式的未知 Host
    let host = req
        .headers
        .iter()
        .find(|(k, _)| k == "host")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    if !loopback_host_allowed(&host, state.port) {
        return Response::json(403, json!({ "error": "未知 Host（拒绝跨站请求）" }));
    }
    // 写请求：Origin 校验（存在时）+ 会话 token
    let is_write = req.method == "POST" || req.method == "PUT" || req.method == "DELETE";
    if is_write {
        if let Some((_, origin)) = req.headers.iter().find(|(k, _)| k == "origin") {
            if !origin_allowed(origin, state.port) {
                return Response::json(403, json!({ "error": "跨站 Origin 被拒绝" }));
            }
        }
        let token = req
            .headers
            .iter()
            .find(|(k, _)| k == SESSION_HEADER)
            .map(|(_, v)| v.as_str());
        if token != Some(state.token.as_str()) {
            return Response::json(401, json!({ "error": "缺少或错误的会话令牌" }));
        }
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => index_page(state),
        ("GET", "/api/state") => api_state(state),
        ("GET", "/api/events") => api_events_snapshot(state, req),
        ("POST", "/api/fs/approve") => api_fs_approve(state, req),
        ("GET", "/api/fs/list") => api_fs_list(state, req),
        ("POST", "/api/repo/discover") => api_repo_discover(state, req),
        ("POST", "/api/profile/select") => api_profile_select(state, req),
        ("POST", "/api/profile/instructions") => api_profile_instructions(state, req),
        ("GET", "/api/draft") => api_draft_get(state),
        ("PUT", "/api/draft") => api_draft_put(state, req),
        ("GET", "/api/capabilities") => Response::json(
            200,
            json!({ "capabilities": crate::adapters::capability::matrix() }),
        ),
        ("POST", "/api/preview/repo-default") => {
            // 仓库默认变更影响预览：对登记的每个 active 工作树分别出计划（无写入）
            let mut per_wt: Vec<serde_json::Value> = Vec::new();
            let repos = read_repos_summary(&state.data_root);
            for repo in repos.as_array().cloned().unwrap_or_default() {
                for (_wt_key, wt) in repo["worktrees"].as_object().cloned().unwrap_or_default() {
                    if wt["status"].as_str() != Some("active") {
                        continue;
                    }
                    let Some(path) = wt["path"].as_str().map(str::to_string) else {
                        continue;
                    };
                    if !Path::new(&path).is_dir() {
                        continue;
                    }
                    let prepared = match crate::commands::personal::prepare_personal(
                        Some(&state.data_root),
                        Some(Path::new(&path)),
                        None,
                        &state.data_root,
                    ) {
                        Ok(p) => p,
                        Err(e) => {
                            per_wt.push(json!({ "worktree": path, "error": e.to_string() }));
                            continue;
                        }
                    };
                    let pending = prepared
                        .plan
                        .actions
                        .iter()
                        .filter(|a| !matches!(a.action, crate::sync::plan::ActionKind::Noop))
                        .count();
                    per_wt.push(json!({
                        "worktree": path,
                        "branch": wt["branch"],
                        "pending": pending,
                        "summary": prepared.plan.summary(),
                        "skipped_company_files": prepared.skipped,
                    }));
                }
            }
            Response::json(
                200,
                json!({ "worktrees": per_wt, "note": "预览无写入；默认只应用到当前工作树" }),
            )
        }
        ("GET", "/api/effective") => {
            match crate::commands::personal::effective(None, None, None, &state.data_root) {
                Ok(v) => Response::json(200, v),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("GET", "/api/server-info") => Response::json(
            200,
            json!({
                "cwd": std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
                "data_root": state.data_root,
                "note": "cwd 是控制台进程的启动目录，可作为「当前目录」默认值",
            }),
        ),
        ("POST", "/api/hosts/detect") => api_hosts_detect(state),
        ("POST", "/api/library/import") => api_library_import(state, req),
        ("GET", "/api/library/list") => api_library_list(state),
        ("POST", "/api/library/delete") => {
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            let execute = req
                .body
                .get("execute")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if execute {
                match crate::personal_library::delete_execute(&state.data_root, id) {
                    Ok(()) => Response::json(200, json!({ "deleted": true, "id": id })),
                    Err(e) => Response::json(400, json!({ "error": e.to_string() })),
                }
            } else {
                match crate::personal_library::delete_preview(&state.data_root, id) {
                    Ok(v) => Response::json(200, v),
                    Err(e) => Response::json(400, json!({ "error": e.to_string() })),
                }
            }
        }
        ("GET", "/api/library/resource") => api_library_resource_get(state, req),
        ("PUT", "/api/library/resource") => api_library_resource_put(state, req),
        ("GET", "/api/workflows") => Response::json(
            200,
            json!({ "runs": crate::workflow::list(&state.data_root) }),
        ),
        ("GET", "/api/workflows/show") => {
            let Some(id) = req
                .query
                .iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.clone())
            else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            match crate::workflow::show(&state.data_root, &id) {
                Ok(r) => Response::json(200, json!(r)),
                Err(e) => Response::json(404, json!({ "error": e.to_string() })),
            }
        }
        ("GET", "/api/workflows/artifact") => {
            let Some(id) = req
                .query
                .iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.clone())
            else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            let Some(art) = req
                .query
                .iter()
                .find(|(k, _)| k == "artifact_id")
                .map(|(_, v)| v.clone())
            else {
                return Response::json(400, json!({ "error": "需要 artifact_id" }));
            };
            match crate::workflow::read_artifact(&state.data_root, &id, &art) {
                Ok(content) => Response::json(200, json!({ "content": content })),
                Err(e) => Response::json(404, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/workflows/new") => {
            let Some(name) = req.body.get("name").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 name" }));
            };
            match crate::workflow::create(&state.data_root, name) {
                Ok(r) => Response::json(200, json!(r)),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/workflows/bind") => {
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            let Some(bindings) = req.body.get("bindings").and_then(|v| v.as_array()) else {
                return Response::json(
                    400,
                    json!({ "error": "需要 bindings [[stage, resource_id]]" }),
                );
            };
            let pairs: Vec<(String, String)> = bindings
                .iter()
                .filter_map(|b| {
                    let a = b.get(0)?.as_str()?.to_string();
                    let r = b.get(1)?.as_str()?.to_string();
                    Some((a, r))
                })
                .collect();
            match crate::workflow::bind_pack(&state.data_root, id, &pairs) {
                Ok(r) => Response::json(200, json!(r)),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/workflows/artifact") => {
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            let stage = req.body.get("stage").and_then(|v| v.as_str()).unwrap_or("");
            let title = req
                .body
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("未命名");
            let Some(content) = req.body.get("content").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 content" }));
            };
            match crate::workflow::put_artifact(&state.data_root, id, stage, title, content) {
                Ok(a) => Response::json(200, json!(a)),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/workflows/reviewed") => {
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            match crate::workflow::mark_reviewed(&state.data_root, id) {
                Ok(r) => Response::json(200, json!(r)),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/jobs/plan") => api_jobs_plan(state, req),
        ("POST", "/api/jobs/apply") => api_jobs_apply(state, req),
        ("GET", "/api/jobs") => {
            let list = jobs::list_persisted(state);
            Response::json(200, json!({ "jobs": list }))
        }
        ("POST", "/api/jobs/undo") => {
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            match jobs::undo(state, id) {
                Ok(v) => Response::json(200, v),
                Err(e) => Response::json(400, json!({ "error": e })),
            }
        }
        ("POST", "/api/workflows/export") => {
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            let Some(art) = req.body.get("artifact_id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 artifact_id" }));
            };
            let Some(target) = req.body.get("target").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 target（导出目标文件路径）" }));
            };
            let execute = req
                .body
                .get("execute")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let ws_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if execute {
                match crate::workflow::export_execute(
                    &state.data_root,
                    id,
                    art,
                    &ws_root,
                    Path::new(target),
                ) {
                    Ok(r) => Response::json(200, json!({ "exported": true, "target": r.path })),
                    Err(e) => Response::json(400, json!({ "error": e.to_string() })),
                }
            } else {
                match crate::workflow::export_preview(&state.data_root, id, art, Path::new(target))
                {
                    Ok(v) => Response::json(200, v),
                    Err(e) => Response::json(400, json!({ "error": e.to_string() })),
                }
            }
        }
        ("POST", "/api/repo/relink") => {
            // 失联工作树重关联：新路径必须仍在同一仓库
            let Some(repo_id) = req.body.get("repo_id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 repo_id" }));
            };
            let Some(wt_id) = req.body.get("wt_id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 wt_id" }));
            };
            let Some(new_path) = req.body.get("new_path").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 new_path" }));
            };
            let mut reg =
                match crate::repo_registry::load_or_create_by_id(&state.data_root, repo_id) {
                    Ok(r) => r,
                    Err(e) => return Response::json(404, json!({ "error": e.to_string() })),
                };
            match reg.relink_worktree(wt_id, Path::new(new_path), &crate::ids::now_iso()) {
                Ok(()) => {
                    let save = reg.save(&state.data_root);
                    match save {
                        Ok(()) => Response::json(
                            200,
                            json!({ "relinked": true, "wt_id": wt_id, "path": new_path }),
                        ),
                        Err(e) => Response::json(500, json!({ "error": e.to_string() })),
                    }
                }
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/shutdown") => {
            *state.shutdown_flag.lock().unwrap() = true;
            Response::json(200, json!({ "shutting_down": true }))
        }
        ("GET", p) if p.starts_with("/api/jobs/") => {
            let id = p.trim_start_matches("/api/jobs/");
            match jobs::load_job(&state.data_root, id) {
                Some(job) => Response::json(200, json!(job)),
                None => Response::json(404, json!({ "error": "任务不存在" })),
            }
        }
        ("POST", p) if p.starts_with("/api/jobs/") && p.ends_with("/cancel") => {
            let id = p
                .trim_start_matches("/api/jobs/")
                .trim_end_matches("/cancel");
            if jobs::request_cancel(state, id) {
                Response::json(200, json!({ "cancel_requested": true }))
            } else {
                Response::json(404, json!({ "error": "任务不存在" }))
            }
        }
        (m, p) => Response::json(404, json!({ "error": format!("未知路径 {m} {p}") })),
    }
}

fn api_state(state: &Arc<ServerState>) -> Response {
    let repos = read_repos_summary(&state.data_root);
    let profile_path = crate::profile::PersonalProfile::profile_path(&state.data_root);
    Response::json(
        200,
        json!({
            "service": "ailoom-console",
            "schema_version": 1,
            "repos": repos,
            "profile_path": profile_path,
            "has_profile": profile_path.is_file(),
            "approved_roots": &*state.approved_roots.lock().unwrap(),
        }),
    )
}

fn read_repos_summary(data_root: &Path) -> Value {
    let dir = data_root.join("repos");
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let reg_file = e.path().join("registry.json");
            if let Ok(text) = std::fs::read_to_string(&reg_file) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    out.push(v);
                }
            }
        }
    }
    Value::Array(out)
}

fn api_events_snapshot(state: &Arc<ServerState>, req: &Request) -> Response {
    // token 允许经 query 传递（EventSource 无法设置自定义头）
    let token_ok = req
        .query
        .iter()
        .any(|(k, v)| k == "token" && v == &state.token);
    if !token_ok {
        return Response::json(401, json!({ "error": "缺少会话令牌" }));
    }
    let events = state.events.lock().unwrap().clone();
    Response::json(200, json!({ "events": events }))
}

fn api_fs_approve(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(path) = req.body.get("path").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 path" }));
    };
    let p = Path::new(path);
    if !p.is_dir() {
        return Response::json(400, json!({ "error": "路径不是目录" }));
    }
    let canon = match p.canonicalize() {
        Ok(c) => c,
        Err(e) => return Response::json(400, json!({ "error": format!("路径不可用: {e}") })),
    };
    let mut roots = state.approved_roots.lock().unwrap();
    if !roots.contains(&canon) {
        roots.push(canon.clone());
    }
    Response::json(200, json!({ "approved": canon }))
}

fn ensure_within_roots(state: &ServerState, target: &Path) -> std::result::Result<PathBuf, String> {
    let canon = target
        .canonicalize()
        .map_err(|e| format!("路径不可用: {e}"))?;
    let roots = state.approved_roots.lock().unwrap();
    for r in roots.iter() {
        if canon.starts_with(r) {
            return Ok(canon);
        }
    }
    Err("路径不在已批准根内（拒绝越界与符号链接逃逸）".to_string())
}

fn api_fs_list(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(path) = req
        .query
        .iter()
        .find(|(k, _)| k == "path")
        .map(|(_, v)| v.clone())
    else {
        return Response::json(400, json!({ "error": "需要 path" }));
    };
    match ensure_within_roots(state, Path::new(&path)) {
        Ok(canon) => {
            let mut items = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&canon) {
                for e in entries.flatten().take(500) {
                    let ft = match e.file_type() {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    items.push(json!({
                        "name": e.file_name().to_string_lossy(),
                        "dir": ft.is_dir(),
                    }));
                }
            }
            items.sort_by(|a, b| {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["name"].as_str().unwrap_or(""))
            });
            Response::json(200, json!({ "path": canon, "items": items }))
        }
        Err(e) => Response::json(403, json!({ "error": e })),
    }
}

fn api_repo_discover(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(path) = req.body.get("path").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 path" }));
    };
    if let Err(e) = ensure_within_roots(state, Path::new(path)) {
        return Response::json(403, json!({ "error": e }));
    }
    match crate::repo_registry::classify_path(Path::new(path)) {
        Ok(crate::repo_registry::PathClass::Git(d)) => {
            // 发现即登记（与 CLI 行为一致）
            let mut reg =
                match crate::repo_registry::RepoRegistry::load_or_create(&state.data_root, &d) {
                    Ok(r) => r,
                    Err(e) => return Response::json(500, json!({ "error": e.to_string() })),
                };
            reg.refresh_worktrees(&d, &crate::ids::now_iso());
            let _ = reg.save(&state.data_root);
            Response::json(
                200,
                json!({
                    "kind": "git",
                    "repo_id": d.identity.repo_id,
                    "repo_root": d.identity.repo_root,
                    "origin": d.identity.origin_normalized,
                    "worktrees": d.worktrees,
                    "current_worktree": d.current_worktree,
                    "start": d.start,
                }),
            )
        }
        Ok(crate::repo_registry::PathClass::NonGit { root }) => {
            Response::json(200, json!({ "kind": "nongit", "root": root }))
        }
        Err(e) => Response::json(400, json!({ "error": e.to_string() })),
    }
}

fn api_profile_select(state: &Arc<ServerState>, req: &Request) -> Response {
    let args = crate::commands::personal::SelectArgs {
        resource: req
            .body
            .get("resource")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        host: req
            .body
            .get("host")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        state: req
            .body
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        subproject: req
            .body
            .get("subproject")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        worktree: req
            .body
            .get("worktree")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    };
    if args.state.is_empty() {
        return Response::json(400, json!({ "error": "需要 state" }));
    }
    match crate::commands::personal::select(&args, &state.data_root) {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::json(400, json!({ "error": e.to_string() })),
    }
}

fn api_profile_instructions(state: &Arc<ServerState>, req: &Request) -> Response {
    // 内容按不可信文本处理：仅作为 Markdown 保存/渲染，绝不注入命令或模板
    let clear = req
        .body
        .get("clear")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo = match crate::repo_registry::discover_repo(&cwd) {
        Ok(r) => r,
        Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
    };
    let repo_id = repo.identity.repo_id.clone();
    let result = if clear {
        crate::personal_instructions::clear_entry(&state.data_root, &repo_id, None)
            .map(|_| json!({ "cleared": true }))
    } else {
        match req.body.get("content").and_then(|v| v.as_str()) {
            Some(content) => {
                crate::personal_instructions::save_entry(&state.data_root, &repo_id, None, content)
                    .map(|_| json!({ "saved": true }))
            }
            None => Err(crate::error::Error::new(
                crate::error::code::USAGE,
                "需要 content 或 clear",
            )),
        }
    };
    match result {
        Ok(mut v) => {
            v["repo_id"] = json!(repo_id);
            Response::json(200, v)
        }
        Err(e) => Response::json(400, json!({ "error": e.to_string() })),
    }
}

fn api_jobs_plan(state: &Arc<ServerState>, req: &Request) -> Response {
    // root 必须在已批准根内（写目标的边界检查）
    let Some(root) = req.body.get("root").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 root" }));
    };
    if let Err(e) = ensure_within_roots(state, Path::new(root)) {
        return Response::json(403, json!({ "error": e }));
    }
    let scope = req
        .body
        .get("scope")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let idempotency_key = req
        .body
        .get("idempotency_key")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    match jobs::spawn_plan(state, PathBuf::from(root), scope, idempotency_key) {
        Ok(id) => Response::json(202, json!({ "job_id": id })),
        Err(e) => Response::json(400, json!({ "error": e.to_string() })),
    }
}

fn api_jobs_apply(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(plan_job_id) = req.body.get("plan_job_id").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 plan_job_id" }));
    };
    let idempotency_key = req
        .body
        .get("idempotency_key")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    match jobs::spawn_apply(state, plan_job_id, idempotency_key) {
        Ok(id) => Response::json(202, json!({ "job_id": id })),
        Err(e) => Response::json(400, json!({ "error": e })),
    }
}

/// 宿主版本探测：仅在用户显式点击时调用；只运行 `--version`（只读、无副作用）。
fn api_hosts_detect(_state: &Arc<ServerState>) -> Response {
    let probe = |name: &str| -> Value {
        let out = std::process::Command::new(name)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => json!({
                "installed": true,
                "version": String::from_utf8_lossy(&o.stdout).trim().to_string(),
            }),
            _ => json!({ "installed": false }),
        }
    };
    Response::json(
        200,
        json!({
            "claude": probe("claude"),
            "codex": probe("codex"),
            "note": "探测仅运行 --version；MCP 连接/包安装/Hook 启用等动作绝不由页面自动触发",
        }),
    )
}

/// 个人库导入：预览默认；execute=true 才复制。目录必须在已批准根内。
fn api_library_import(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(dir) = req.body.get("dir").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 dir" }));
    };
    if let Err(e) = ensure_within_roots(state, Path::new(dir)) {
        return Response::json(403, json!({ "error": e }));
    }
    let execute = req
        .body
        .get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let name = req.body.get("name").and_then(|v| v.as_str());
    let result = if execute {
        crate::personal_library::import_execute(&state.data_root, Path::new(dir), name)
            .map(|r| json!({ "executed": true, "report": r }))
    } else {
        crate::personal_library::import_preview(&state.data_root, Path::new(dir), name)
            .map(|p| json!({ "executed": false, "preview": p }))
    };
    match result {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::json(400, json!({ "error": e.to_string() })),
    }
}

fn library_entries(state: &Arc<ServerState>) -> Response {
    let lib = crate::personal_library::library_root(&state.data_root);
    if !lib.is_dir() {
        return Response::json(200, json!({ "path": lib, "entries": [] }));
    }
    let manifest = match crate::manifest::TeamManifest::load_from(&lib) {
        Ok(m) => m,
        Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
    };
    let entries =
        match crate::resource::enumerate(&lib, &manifest, crate::personal_library::LIBRARY_TEAM_ID)
        {
            Ok(e) => e,
            Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
        };
    let items: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "id": e.id.to_string(),
                "kind": e.id.kind.as_str(),
                "name": e.id.name,
                "namespace": e.id.namespace,
                "description": e.description,
                "path": e.path,
            })
        })
        .collect();
    Response::json(200, json!({ "path": lib, "entries": items }))
}

fn api_library_list(state: &Arc<ServerState>) -> Response {
    library_entries(state)
}

/// 读资源正文 + 指纹（并发保护）。
fn api_library_resource_get(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(id) = req
        .query
        .iter()
        .find(|(k, _)| k == "id")
        .map(|(_, v)| v.clone())
    else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    let lib = crate::personal_library::library_root(&state.data_root);
    let manifest = match crate::manifest::TeamManifest::load_from(&lib) {
        Ok(m) => m,
        Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
    };
    let entries =
        match crate::resource::enumerate(&lib, &manifest, crate::personal_library::LIBRARY_TEAM_ID)
        {
            Ok(e) => e,
            Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
        };
    let Some(entry) = entries.iter().find(|e| e.id.to_string() == id) else {
        return Response::json(404, json!({ "error": "资源不存在" }));
    };
    // skill 编辑 SKILL.md；其余编辑正文文件
    let file = if entry.id.kind == crate::resource::ResourceKind::Skill {
        lib.join(&entry.path).join("SKILL.md")
    } else {
        lib.join(&entry.path)
    };
    match std::fs::read_to_string(&file) {
        Ok(content) => {
            let fingerprint = crate::ids::sha256_hex(content.as_bytes());
            Response::json(
                200,
                json!({ "id": id, "content": content, "fingerprint": fingerprint }),
            )
        }
        Err(e) => Response::json(400, json!({ "error": format!("读取失败: {e}") })),
    }
}

/// 保存资源正文：base_fingerprint 不一致 → 409（保护外部编辑，不覆盖）。
fn api_library_resource_put(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    let Some(content) = req.body.get("content").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 content" }));
    };
    let base = req.body.get("base_fingerprint").and_then(|v| v.as_str());
    let lib = crate::personal_library::library_root(&state.data_root);
    let manifest = match crate::manifest::TeamManifest::load_from(&lib) {
        Ok(m) => m,
        Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
    };
    let entries =
        match crate::resource::enumerate(&lib, &manifest, crate::personal_library::LIBRARY_TEAM_ID)
        {
            Ok(e) => e,
            Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
        };
    let Some(entry) = entries.iter().find(|e| e.id.to_string() == id) else {
        return Response::json(404, json!({ "error": "资源不存在" }));
    };
    let file = if entry.id.kind == crate::resource::ResourceKind::Skill {
        lib.join(&entry.path).join("SKILL.md")
    } else {
        lib.join(&entry.path)
    };
    // 保存前重解析校验（frontmatter 合法性），失败不落盘
    if entry.id.kind == crate::resource::ResourceKind::Skill {
        let parsed = crate::resource::parse_frontmatter(content);
        if parsed.is_err() {
            return Response::json(400, json!({ "error": parsed.err().unwrap().to_string() }));
        }
    }
    let current = std::fs::read_to_string(&file).unwrap_or_default();
    let current_fp = crate::ids::sha256_hex(current.as_bytes());
    if let Some(base) = base {
        if base != current_fp {
            return Response::json(
                409,
                json!({
                    "error": "文件已被外部修改，拒绝覆盖（草稿保留在你的编辑器中）",
                    "current_fingerprint": current_fp,
                }),
            );
        }
    }
    let write = crate::sync_common::atomic_write(&file, content.as_bytes());
    if write.is_err() {
        return Response::json(500, json!({ "error": write.err().unwrap().to_string() }));
    }
    Response::json(
        200,
        json!({ "saved": true, "fingerprint": crate::ids::sha256_hex(content.as_bytes()) }),
    )
}

fn draft_disk_path(data_root: &Path) -> PathBuf {
    data_root.join("console").join("draft.json")
}

fn api_draft_get(state: &Arc<ServerState>) -> Response {
    // 内存优先；服务重启后从磁盘恢复（只恢复输入草稿，不重放动作）
    {
        let mut cur = state.draft.lock().unwrap();
        if cur.is_none() {
            let p = draft_disk_path(&state.data_root);
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    let rev = v.get("revision").and_then(|r| r.as_u64()).unwrap_or(0);
                    let draft = v.get("draft").cloned();
                    if let Some(d) = draft {
                        *cur = Some((rev, d));
                    }
                }
            }
        }
    }
    let draft = state.draft.lock().unwrap().clone();
    match draft {
        Some((rev, v)) => Response::json(200, json!({ "revision": rev, "draft": v })),
        None => Response::json(200, json!({ "revision": 0, "draft": null })),
    }
}

fn api_draft_put(state: &Arc<ServerState>, req: &Request) -> Response {
    let base = req.body.get("base_revision").and_then(|v| v.as_u64());
    let draft = req.body.get("draft").cloned();
    let Some(draft) = draft else {
        return Response::json(400, json!({ "error": "需要 draft" }));
    };
    let mut cur = state.draft.lock().unwrap();
    let current_rev = cur.as_ref().map(|(r, _)| *r).unwrap_or(0);
    // 并发保存保护：过期 base → 冲突并返回当前草稿（用户草稿不丢失）
    if let Some(base) = base {
        if base != current_rev {
            return Response::json(
                409,
                json!({
                    "error": "草稿已被其他会话修改",
                    "current_revision": current_rev,
                    "draft": cur.as_ref().map(|(_, d)| d).cloned(),
                }),
            );
        }
    }
    let new_rev = current_rev + 1;
    *cur = Some((new_rev, draft.clone()));
    drop(cur);
    // 落盘：服务重启后可恢复（无执行动作，不会自动重放）
    {
        let p = draft_disk_path(&state.data_root);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = crate::sync_common::atomic_write(
            &p,
            serde_json::to_vec_pretty(&json!({ "revision": new_rev, "draft": draft }))
                .unwrap_or_default()
                .as_slice(),
        );
    }
    Response::json(200, json!({ "revision": new_rev }))
}

fn index_page(state: &Arc<ServerState>) -> Response {
    Response {
        status: 200,
        content_type: "text/html; charset=utf-8",
        body: web::index_html(&state.token),
        extra_headers: Vec::new(),
    }
}

/// 启动控制台的命令入口（阻塞，直到 shutdown 请求）。
pub fn run_blocking(opts: &ConsoleOptions) -> Result<()> {
    let server = ConsoleServer::start(opts)?;
    crate::logging::info(format!(
        "控制台已启动：http://127.0.0.1:{}/?token={}（仅本机可访问；Ctrl+C 或 POST /api/shutdown 停止）",
        server.port, server.token
    ));
    if opts.open_browser {
        server.open_browser();
    }
    loop {
        if *server.state.shutdown_flag.lock().unwrap() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    crate::logging::info("控制台已停止");
    Ok(())
}
