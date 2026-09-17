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
                    // S04：启动即载入持久化任务（成功任务/幂等键可继续操作；
                    // 中断任务显式标记，不自动重放执行动作）
                    let (jobs_map, interrupted) = jobs::load_all_jobs(&opts.data_root);
                    let loaded = jobs_map.len();
                    if loaded > 0 {
                        crate::logging::info(format!(
                            "已恢复 {loaded} 个持久化任务（其中 {} 个标记为中断）",
                            interrupted.len()
                        ));
                    }
                    let state = Arc::new(ServerState {
                        port: p,
                        data_root: opts.data_root.clone(),
                        token: token.clone(),
                        approved_roots: Mutex::new(Vec::new()),
                        draft: Mutex::new(None),
                        shutdown_flag: Mutex::new(false),
                        events: Mutex::new(Vec::new()),
                        jobs: Mutex::new(jobs_map),
                    });
                    for id in &interrupted {
                        state.events.lock().unwrap().push(json!({
                            "event": "job-interrupted", "id": id,
                            "note": "服务重启导致任务中断；不自动重放",
                        }));
                    }
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

fn percent_decode(s: &str) -> String {
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

fn parse_target(target: &str) -> (String, Vec<(String, String)>) {
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
pub mod ui;
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
        ("POST", "/api/fs/pick-directory") => api_pick_directory(),
        ("POST", "/api/projects/metadata") => api_project_metadata(state, req),
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
            // F01/U04：作用域解析跟随页面选择的仓库/工作树根（缺省服务 cwd）
            let root = req
                .query
                .iter()
                .find(|(k, _)| k == "root")
                .map(|(_, v)| PathBuf::from(v));
            if let Some(r) = &root {
                if let Err(e) = ensure_within_roots(state, r) {
                    return Response::json(403, json!({ "error": e }));
                }
            }
            match crate::commands::personal::effective(
                root.as_deref(),
                req.query
                    .iter()
                    .find(|(k, _)| k == "scope")
                    .map(|(_, v)| v.clone()),
                None,
                &state.data_root,
            ) {
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
        ("GET", "/api/migrations/cc-switch/location") => collection_response(
            crate::cc_switch::default_directory().map(|path| json!({"directory":path})),
        ),
        ("POST", "/api/migrations/cc-switch/read") => {
            if req.body["confirm_source_read"].as_bool() != Some(true) {
                return Response::json(
                    400,
                    json!({"error":"请在迁移界面点击扫描，确认读取 Skill 来源"}),
                );
            }
            let default = crate::cc_switch::default_directory();
            let directory = req.body["directory"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .or_else(|| default.as_ref().ok().cloned());
            let Some(directory) = directory else {
                return Response::json(400, json!({"error":"请选择 CC Switch 数据目录"}));
            };
            // 默认目录仅开放此固定来源查询，不加入通用文件读取授权根。
            if default.as_ref().ok() != Some(&directory) {
                if let Err(e) = ensure_within_roots(state, &directory) {
                    return Response::json(403, json!({"error":e}));
                }
            }
            collection_response(crate::cc_switch::scan_directory(
                &state.data_root,
                &directory,
            ))
        }
        ("POST", "/api/migrations/cc-switch/scan") => collection_response(crate::cc_switch::scan(
            &state.data_root,
            &req.body["manifest"],
        )),
        ("POST", "/api/migrations/cc-switch/preview") => {
            let selected: std::result::Result<Vec<String>, _> =
                serde_json::from_value(req.body["selected"].clone());
            match selected {
                Ok(selected) => collection_response(crate::cc_switch::prepare(
                    &state.data_root,
                    req.body["scan_id"].as_str().unwrap_or(""),
                    &selected,
                )),
                Err(_) => Response::json(400, json!({"error":"请选择来源记录"})),
            }
        }
        ("POST", "/api/migrations/cc-switch/apply") => {
            collection_response(crate::cc_switch::apply(
                &state.data_root,
                req.body["preview_id"].as_str().unwrap_or(""),
            ))
        }
        ("GET", "/api/library/list") => api_library_list(state),
        ("GET", "/api/collections") => {
            collection_response(crate::collections::list(&state.data_root))
        }
        ("POST", "/api/collections/preview") => {
            let url = req.body["url"].as_str().unwrap_or("");
            // 本机 Git 仓库也必须先批准目录；远程 URL 交 GitSource 校验。
            if let Some(path) = url
                .strip_prefix("file://")
                .or_else(|| (Path::new(url).is_absolute() || !url.contains(':')).then_some(url))
            {
                if let Err(e) = ensure_within_roots(state, Path::new(path)) {
                    return Response::json(403, json!({ "error": e }));
                }
            }
            collection_response(crate::collections::preview(
                &state.data_root,
                req.body["name"].as_str().unwrap_or(""),
                url,
                req.body["ref"].as_str(),
                req.body["source_id"].as_str(),
            ))
        }
        ("POST", "/api/collections/apply") => {
            collection_response(crate::collections::apply_preview(
                &state.data_root,
                req.body["preview_id"].as_str().unwrap_or(""),
            ))
        }
        ("POST", "/api/collections/check") => collection_response(
            crate::collections::check_updates(&state.data_root, req.body["source_id"].as_str()),
        ),
        ("POST", "/api/collections/update") => {
            let tokens: Vec<String> = req.body["preview_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            collection_response(crate::collections::apply_previews(
                &state.data_root,
                &tokens,
            ))
        }
        ("POST", "/api/collections/remove") => collection_response(crate::collections::remove(
            &state.data_root,
            req.body["source_id"].as_str().unwrap_or(""),
            req.body["execute"].as_bool().unwrap_or(false),
        )),
        ("GET", "/api/resources") => {
            let (local, issues) = crate::personal_library::list_tolerant(&state.data_root);
            let result = crate::collections::list(&state.data_root).map(|v| {
                let mut entries: Vec<Value> = local
                    .iter()
                    .map(|e| {
                        json!({
                            "id": e.id, "kind": e.kind, "description": e.description,
                            "source_name": "个人资源库", "readonly": false
                        })
                    })
                    .collect();
                let mut source_errors = Vec::new();
                for source in v["sources"].as_array().into_iter().flatten() {
                    entries.extend(
                        source["resources"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
                    if !source["error"].is_null() {
                        source_errors
                            .push(json!({ "source": source["name"], "error": source["error"] }));
                    }
                }
                json!({ "entries": entries, "issues": issues, "source_errors": source_errors })
            });
            collection_response(result)
        }
        ("POST", "/api/library/import-git") => {
            // AIL-064：GitHub/远程仓库导入（预览默认；execute 才复制）
            let Some(url) = req.body.get("url").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 url" }));
            };
            let repo_path = req.body.get("path").and_then(|v| v.as_str());
            let ref_ = req.body.get("ref").and_then(|v| v.as_str());
            let name = req.body.get("name").and_then(|v| v.as_str());
            let execute = req
                .body
                .get("execute")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let result = if execute {
                crate::personal_library::git_import_execute(
                    &state.data_root,
                    url,
                    repo_path,
                    ref_,
                    name,
                )
                .map(|r| json!({ "executed": true, "report": r }))
            } else {
                crate::personal_library::git_import_preview(
                    &state.data_root,
                    url,
                    repo_path,
                    ref_,
                    name,
                )
                .map(|p| json!({ "executed": false, "preview": p }))
            };
            match result {
                Ok(v) => Response::json(200, v),
                Err(e) => {
                    let ctx_candidates = e.context.get("candidates").cloned();
                    Response::json(
                        400,
                        json!({ "error": e.to_string(), "candidates": ctx_candidates }),
                    )
                }
            }
        }
        ("POST", "/api/library/import-entry") => {
            // AIL-065：发现入口（skills.sh/…）导入；预览默认，execute 才复制
            let Some(entry) = req.body.get("entry").and_then(|v| v.as_str()) else {
                return Response::json(
                    400,
                    json!({ "error": "需要 entry（发现入口，如 skills.sh/<owner>/<repo>/<skill>）" }),
                );
            };
            let name = req.body.get("name").and_then(|v| v.as_str());
            let execute = req
                .body
                .get("execute")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match crate::personal_library::import_via_discovery(
                &state.data_root,
                entry,
                name,
                execute,
            ) {
                Ok(v) => Response::json(200, v),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/library/check-update") => {
            // AIL-066：显式检查更新（只比较，不应用，不联网到非来源地址）
            let Some(skill) = req.body.get("skill").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 skill" }));
            };
            match crate::personal_library::check_update(&state.data_root, skill) {
                Ok(st) => Response::json(200, json!({ "status": st })),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/library/update") => {
            // AIL-066：应用更新（仅库内；部署需重新预览+应用）
            let Some(skill) = req.body.get("skill").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 skill" }));
            };
            let execute = req
                .body
                .get("execute")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !execute {
                return match crate::personal_library::check_update(&state.data_root, skill) {
                    Ok(st) => Response::json(200, json!({ "executed": false, "status": st })),
                    Err(e) => Response::json(400, json!({ "error": e.to_string() })),
                };
            }
            match crate::personal_library::update_execute(&state.data_root, skill) {
                Ok(r) => Response::json(200, json!({ "executed": true, "result": r })),
                Err(e) => Response::json(409, json!({ "error": e.to_string() })),
            }
        }
        ("GET", "/api/deploy-status") => {
            // AIL-067/068：库版本 vs 各工作树已部署版本对照（目标须在授权根内）
            let Some(root) = req
                .query
                .iter()
                .find(|(k, _)| k == "root")
                .map(|(_, v)| PathBuf::from(v))
            else {
                return Response::json(400, json!({ "error": "需要 root" }));
            };
            if let Err(e) = ensure_within_roots(state, &root) {
                return Response::json(403, json!({ "error": e }));
            }
            match crate::commands::personal::deploy_status(Some(&root), None, &state.data_root) {
                Ok(v) => Response::json(200, v),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("GET", "/api/library/sources") => {
            // AIL-063：来源身份/版本清单
            let lib = crate::personal_library::library_root(&state.data_root);
            let mut items = Vec::new();
            let skills_dir = lib.join("resources/skills");
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                for e in entries.flatten() {
                    let d = e.path();
                    if !d.is_dir() || e.file_name().to_string_lossy().starts_with('.') {
                        continue;
                    }
                    let meta = crate::skill_source::read_meta(&d);
                    items.push(json!({
                        "skill": e.file_name().to_string_lossy(),
                        "source": meta,
                        "legacy": meta.is_none(),
                    }));
                }
            }
            Response::json(200, json!({ "items": items }))
        }
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
            let version = req
                .query
                .iter()
                .find(|(k, _)| k == "version")
                .and_then(|(_, v)| v.parse::<u32>().ok());
            let result = match version {
                Some(v) => crate::workflow::read_artifact_version(&state.data_root, &id, &art, v),
                None => crate::workflow::read_artifact(&state.data_root, &id, &art),
            };
            match result {
                Ok(content) => {
                    Response::json(200, json!({ "content": content, "version": version }))
                }
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
            // L04：更新已有产物时必须携带 base_version（乐观并发，不静默覆盖他人版本）
            let base_version = req
                .body
                .get("base_version")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            match crate::workflow::put_artifact(
                &state.data_root,
                id,
                stage,
                title,
                content,
                base_version,
            ) {
                Ok(a) => Response::json(200, json!(a)),
                Err(e) if e.code == crate::error::code::USER_CONTENT_CONFLICT => {
                    Response::json(409, json!({ "error": e.to_string(), "context": e.context }))
                }
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("POST", "/api/workflows/rename") => {
            // L04：重命名走独立端点（关联按 id，不因改名产生重复产物）
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            let Some(artifact_id) = req.body.get("artifact_id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 artifact_id" }));
            };
            let Some(title) = req.body.get("title").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 title" }));
            };
            match crate::workflow::rename_artifact(&state.data_root, id, artifact_id, title) {
                Ok(r) => Response::json(200, json!(r)),
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
        ("POST", "/api/workflows/record-input") => {
            // L06：输入版本记录的真实入口（摘要取自个人库实际内容）
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            let Some(rid) = req.body.get("resource_id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 resource_id" }));
            };
            match crate::workflow::record_resource_input(&state.data_root, id, rid) {
                Ok(r) => Response::json(200, json!(r)),
                Err(e) => Response::json(400, json!({ "error": e.to_string() })),
            }
        }
        ("GET", "/api/fs/read") => {
            // L06：现有文件导入产物——只在已批准根内读文本文件（≤256KB）
            let Some(path) = req
                .query
                .iter()
                .find(|(k, _)| k == "path")
                .map(|(_, v)| v.clone())
            else {
                return Response::json(400, json!({ "error": "需要 path" }));
            };
            if let Err(e) = ensure_within_roots(state, Path::new(&path)) {
                return Response::json(403, json!({ "error": e }));
            }
            let p = Path::new(&path);
            if !p.is_file() {
                return Response::json(400, json!({ "error": "路径不是文件" }));
            }
            match std::fs::read(p) {
                Ok(bytes) if bytes.len() <= 256 * 1024 => match String::from_utf8(bytes) {
                    Ok(text) => Response::json(200, json!({ "path": path, "content": text })),
                    Err(_) => Response::json(400, json!({ "error": "仅支持 UTF-8 文本文件" })),
                },
                Ok(_) => Response::json(400, json!({ "error": "文件超过 256KB" })),
                Err(e) => Response::json(400, json!({ "error": format!("读取失败: {e}") })),
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
            // S03：导出目标必须在已批准根内（canonicalize 后校验边界，拒绝符号链接逃逸）
            let target_path = Path::new(target);
            let boundary_probe = if target_path.is_file() {
                target_path.to_path_buf()
            } else {
                target_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| target_path.to_path_buf())
            };
            if let Err(e) = ensure_within_roots(state, &boundary_probe) {
                return Response::json(403, json!({ "error": e }));
            }
            if execute {
                // S03/L03：execute 必须携带预览确认的 target_fingerprint，
                // 目标当前状态与预览不一致 → 拒绝覆盖（外部编辑不丢失）
                let Some(expected_fp) = req.body.get("target_fingerprint").and_then(|v| v.as_str())
                else {
                    return Response::json(
                        400,
                        json!({ "error": "缺少 target_fingerprint（必须先预览导出并确认）" }),
                    );
                };
                match crate::workflow::export_execute(
                    &state.data_root,
                    id,
                    art,
                    &boundary_probe,
                    target_path,
                    Some(expected_fp),
                ) {
                    Ok(r) => Response::json(200, json!({ "exported": true, "target": r.path })),
                    Err(e) => Response::json(409, json!({ "error": e.to_string() })),
                }
            } else {
                match crate::workflow::export_preview(&state.data_root, id, art, target_path) {
                    Ok(v) => Response::json(200, v),
                    Err(e) => Response::json(400, json!({ "error": e.to_string() })),
                }
            }
        }
        ("POST", "/api/repo/relink") => {
            // F04/F05：失联工作树重关联。新路径必须真实属于本仓库（common-dir 一致）；
            // 整仓搬迁在 Git 身份证据吻合时迁移登记身份（repo_id/WorktreeId/配置保持）
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
            match reg.relink_worktree(
                &state.data_root,
                wt_id,
                Path::new(new_path),
                &crate::ids::now_iso(),
            ) {
                Ok(outcome) => {
                    let save = reg.save(&state.data_root);
                    match save {
                        Ok(()) => Response::json(
                            200,
                            json!({
                                "relinked": true,
                                "wt_id": wt_id,
                                "path": new_path,
                                "outcome": outcome,
                                "repo_id": reg.repo_id,
                                "note": "重关联保持登记身份与个人配置；仓库级搬迁经别名解析继续生效",
                            }),
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
        ("GET", p) if p.starts_with("/ui/") => match ui::lookup(p) {
            Some((content, mime)) => Response {
                status: 200,
                content_type: mime,
                body: content.to_string(),
                extra_headers: Vec::new(),
            },
            None => Response::json(404, json!({ "error": "未知资产" })),
        },
        (m, p) => Response::json(404, json!({ "error": format!("未知路径 {m} {p}") })),
    }
}

fn collection_response(result: crate::error::Result<Value>) -> Response {
    match result {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::json(
            if e.code == crate::error::code::USER_CONTENT_CONFLICT {
                409
            } else {
                400
            },
            json!({ "error": e.to_string() }),
        ),
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
            "native_picker": cfg!(target_os = "macos"),
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
                if let Ok(mut v) = serde_json::from_str::<Value>(&text) {
                    let metadata = e.path().join("project.json");
                    if let Ok(text) = std::fs::read_to_string(metadata) {
                        if let Ok(meta) = serde_json::from_str::<Value>(&text) {
                            v["project"] = meta;
                        }
                    }
                    out.push(v);
                }
            }
        }
    }
    Value::Array(out)
}

fn api_project_metadata(state: &Arc<ServerState>, req: &Request) -> Response {
    let id = req.body["repo_id"].as_str().unwrap_or("");
    // Resolve against registered IDs, never accept a client path as a data filename.
    if !read_repos_summary(&state.data_root)
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["repo_id"] == id)
    {
        return Response::json(404, json!({"error": "项目未登记"}));
    }
    let name = req.body["name"].as_str().unwrap_or("").trim();
    let category = req.body["category"].as_str().unwrap_or("").trim();
    if name.is_empty() || name.chars().count() > 120 || category.chars().count() > 80 {
        return Response::json(
            400,
            json!({"error": "项目名称需为 1–120 字，分类最多 80 字"}),
        );
    }
    let meta = json!({"name":name,"category":category});
    let path = state.data_root.join("repos").join(id).join("project.json");
    match crate::sync_common::atomic_write(&path, meta.to_string().as_bytes()) {
        Ok(()) => Response::json(200, meta),
        Err(e) => Response::json(500, json!({"error":e.to_string()})),
    }
}

fn api_pick_directory() -> Response {
    #[cfg(target_os = "macos")]
    {
        // Fixed AppleScript: no user data is interpolated into executable code.
        match std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "POSIX path of (choose folder with prompt \"选择 AILoom 项目文件夹\")",
            ])
            .output()
        {
            Ok(out) if out.status.success() => Response::json(
                200,
                json!({"path":String::from_utf8_lossy(&out.stdout).trim()}),
            ),
            Ok(out) => {
                let message = String::from_utf8_lossy(&out.stderr);
                if message.contains("(-128)") {
                    Response::json(200, json!({"cancelled":true}))
                } else {
                    Response::json(500, json!({"error":message.trim()}))
                }
            }
            Err(e) => Response::json(500, json!({"error":e.to_string()})),
        }
    }
    #[cfg(not(target_os = "macos"))]
    Response::json(
        400,
        json!({"error":"当前平台尚未接入原生文件夹选择器，请输入绝对路径"}),
    )
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
            if let Err(e) = reg.save(&state.data_root) {
                return Response::json(500, json!({"error":e.to_string()}));
            }
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
            // F06：非 Git 路径模式是合法一等作用域（nongit-<hash>），登记后
            // select/plan/sync 全链路可用（能力受限于无 Git exclude/工作树）。
            let ident = crate::commands::personal::nongit_identity(&root);
            if let Err(e) =
                crate::repo_registry::RepoRegistry::load_or_create(&state.data_root, &ident)
                    .and_then(|r| r.save(&state.data_root))
            {
                return Response::json(500, json!({"error":e.to_string()}));
            }
            Response::json(
                200,
                json!({
                    "kind": "nongit",
                    "root": root,
                    "repo_id": ident.identity.repo_id,
                    "note": "非 Git 路径模式：以路径为身份；无 Git exclude 登记与工作树概念",
                }),
            )
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
        // F01：选择目标必须显式定位到用户在页面选择的仓库/工作树，不用服务 cwd 猜
        repo_root: req
            .body
            .get("root")
            .and_then(|v| v.as_str())
            .map(PathBuf::from),
        // F08：并发保护（可选）
        base_revision: req.body.get("base_revision").and_then(|v| v.as_u64()),
    };
    if args.state.is_empty() {
        return Response::json(400, json!({ "error": "需要 state" }));
    }
    if args.repo_root.is_none() {
        return Response::json(
            400,
            json!({ "error": "需要 root（选择目标仓库/工作树的绝对路径）" }),
        );
    }
    if let Err(e) = ensure_within_roots(state, args.repo_root.as_deref().unwrap()) {
        return Response::json(403, json!({ "error": e }));
    }
    match crate::commands::personal::select(&args, &state.data_root) {
        Ok(v) => Response::json(200, v),
        Err(e) if e.code == crate::error::code::USER_CONTENT_CONFLICT => Response::json(
            409,
            json!({
                "error": e.to_string(),
                "current_revision": e.context.get("current_revision").cloned(),
            }),
        ),
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
    let Some(root) = req.body["root"].as_str() else {
        return Response::json(
            400,
            json!({"error":"需要 root，不能用服务启动目录代替项目"}),
        );
    };
    let root = match ensure_within_roots(state, Path::new(root)) {
        Ok(root) => root,
        Err(e) => return Response::json(403, json!({"error":e})),
    };
    let repo = match crate::repo_registry::classify_path(&root) {
        Ok(crate::repo_registry::PathClass::Git(r)) => r,
        Ok(crate::repo_registry::PathClass::NonGit { root }) => {
            crate::commands::personal::nongit_identity(&root)
        }
        Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
    };
    let repo_id = repo.identity.repo_id.clone();
    if req.body["read"].as_bool() == Some(true) {
        return Response::json(
            200,
            json!({"repo_id":repo_id,"content":crate::personal_instructions::load_entry(&state.data_root,&repo_id,None).unwrap_or_default()}),
        );
    }
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
    // AIL-062：宽容列表——坏条目作为 issues 返回（带文件级定位），不锁死整个列表
    let lib = crate::personal_library::library_root(&state.data_root);
    let (entries, issues) = crate::personal_library::list_tolerant(&state.data_root);
    Response::json(
        200,
        json!({
            "path": lib,
            "entries": entries,
            "issues": issues,
            "note": if issues.is_empty() { "".to_string() } else {
                "部分条目无法解析（见 issues）；其余资源仍可编辑/删除，修复后自动恢复".to_string()
            },
        }),
    )
}

fn api_library_list(state: &Arc<ServerState>) -> Response {
    library_entries(state)
}

/// AIL-062：按宽容列表定位资源；坏条目返回 422 + 具体文件错误（可修复后重试）。
struct LibraryTarget {
    file: PathBuf,
    kind: crate::resource::ResourceKind,
    name: String,
}

fn find_library_target(
    state: &ServerState,
    id: &str,
) -> std::result::Result<LibraryTarget, Response> {
    let lib = crate::personal_library::library_root(&state.data_root);
    let (entries, issues) = crate::personal_library::list_tolerant(&state.data_root);
    if let Some(e) = entries.iter().find(|e| e.id == id) {
        let kind = match e.kind.as_str() {
            "skill" => crate::resource::ResourceKind::Skill,
            "rule" => crate::resource::ResourceKind::Rule,
            "doc" => crate::resource::ResourceKind::Doc,
            "agent" => crate::resource::ResourceKind::Agent,
            "mcp" => crate::resource::ResourceKind::Mcp,
            "env" => crate::resource::ResourceKind::Env,
            "hook" => crate::resource::ResourceKind::Hook,
            "package" => crate::resource::ResourceKind::Package,
            _ => crate::resource::ResourceKind::Learning,
        };
        let file = if e.kind == "skill" {
            lib.join(&e.path).join("SKILL.md")
        } else {
            lib.join(&e.path)
        };
        return Ok(LibraryTarget {
            file,
            kind,
            name: e.name.clone(),
        });
    }
    for issue in &issues {
        let name = std::path::Path::new(&issue.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let stem = name.trim_end_matches(".md").trim_end_matches(".toml");
        if id.ends_with(stem) {
            return Err(Response::json(
                422,
                json!({
                    "error": format!("资源存在但无法解析：{}（{}）。修复该文件后即可继续编辑", issue.path, issue.error),
                    "issue_path": issue.path,
                }),
            ));
        }
    }
    Err(Response::json(404, json!({ "error": "资源不存在" })))
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
    let entry = match find_library_target(state, &id) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let file = entry.file;
    match std::fs::read_to_string(&file) {
        Ok(content) => {
            let fingerprint = crate::ids::sha256_hex(content.as_bytes());
            // L05：MCP 原文不回显字面量秘密（占位符替换；保存时回填现值）
            if entry.kind == crate::resource::ResourceKind::Mcp {
                let (redacted_content, fields) = redact_mcp_literals(&content);
                return Response::json(
                    200,
                    json!({
                        "id": id,
                        "content": redacted_content,
                        "fingerprint": fingerprint,
                        "redacted_fields": fields,
                        "note": if fields.is_empty() { "".to_string() } else {
                            "以上键为秘密：编辑器显示占位符，保存时自动回填盘上现值；新增秘密请用 $ENV:NAME 引用".to_string()
                        },
                    }),
                );
            }
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
    let entry = match find_library_target(state, id) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let file = entry.file;
    // L02：保存前按类型完整校验（错误定位到字段；不过不落盘，库不再被坏文件锁死）
    if let Err(msg) = validate_resource_content(&entry.kind, &entry.name, content) {
        return Response::json(400, json!({ "error": format!("保存被拒绝：{msg}") }));
    }
    // L05：MCP 占位符回填 + 字面量秘密边界
    let content_owned: String;
    let content = if entry.kind == crate::resource::ResourceKind::Mcp {
        let existing_now = std::fs::read_to_string(&file).unwrap_or_default();
        match restore_mcp_placeholders(content, &existing_now, &entry.name) {
            Ok(restored) => {
                content_owned = restored;
                content_owned.as_str()
            }
            Err(msg) => return Response::json(400, json!({ "error": msg })),
        }
    } else {
        content
    };
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

// ---------------------------------------------------------------------------
// 资源编辑安全（AIL-049 返工）：保存前完整校验 + 秘密明文边界
// ---------------------------------------------------------------------------

/// 编辑器中秘密的占位值（GET 时原文被替换；PUT 时用盘上现值回填）。
const SECRET_PLACEHOLDER: &str = "__AILOOM_REDACTED__";

/// L02：保存前按资源类型完整校验（错误定位到字段；校验不过不落盘，
/// 杜绝「保存成功但整个库无法再次读取」）。
fn validate_resource_content(
    kind: &crate::resource::ResourceKind,
    entry_name: &str,
    content: &str,
) -> std::result::Result<(), String> {
    match kind {
        crate::resource::ResourceKind::Skill => {
            let (meta, _) = crate::resource::parse_frontmatter(content)
                .map_err(|e| format!("frontmatter 校验失败: {e}"))?;
            match meta {
                Some(m) => {
                    if m.name.as_deref() != Some(entry_name) {
                        return Err(format!(
                            "frontmatter name({:?}) 必须与资源名({entry_name}) 一致（身份稳定）",
                            m.name
                        ));
                    }
                }
                None => return Err("SKILL.md 缺少 frontmatter（name/description）".into()),
            }
            Ok(())
        }
        crate::resource::ResourceKind::Doc | crate::resource::ResourceKind::Learning => {
            // Markdown：frontmatter 存在就必须合法（未闭合/类型错误即时定位）
            crate::resource::parse_frontmatter(content)
                .map(|_| ())
                .map_err(|e| format!("frontmatter 校验失败: {e}"))
        }
        crate::resource::ResourceKind::Mcp => {
            mcp_spec_from_content(entry_name, content).map(|_| ())
        }
        _ => Ok(()),
    }
}

fn mcp_spec_from_content(
    entry_name: &str,
    content: &str,
) -> std::result::Result<crate::adapters::mcp::McpSpec, String> {
    let entry = crate::resource::ResourceEntry {
        id: crate::resource::ResourceId {
            source: "personal".into(),
            kind: crate::resource::ResourceKind::Mcp,
            namespace: "personal".into(),
            name: entry_name.into(),
        },
        meta: crate::resource::ResourceMeta {
            shared: true,
            projects: vec![],
            roles: vec![],
            namespace: "personal".into(),
            tags: vec![],
        },
        path: String::new(),
        description: String::new(),
        raw: Some(content.to_string()),
    };
    crate::adapters::mcp::parse_spec(&entry).map_err(|e| e.to_string())
}

/// L05：GET 时不回显字面量秘密。env/headers 中非 `$ENV:` 引用的值替换为占位符，
/// 返回（脱敏正文, 被脱敏键列表）。
fn redact_mcp_literals(content: &str) -> (String, Vec<String>) {
    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return (content.to_string(), Vec::new()),
    };
    let mut redacted = Vec::new();
    fn scrub(table: &mut toml_edit::Table, prefix: &str, redacted: &mut Vec<String>) {
        for (k, item) in table.iter_mut() {
            if let Some(s) = item.as_str() {
                if !s.starts_with("$ENV:") && s != SECRET_PLACEHOLDER {
                    *item = toml_edit::value(SECRET_PLACEHOLDER);
                    redacted.push(format!("{prefix}{k}"));
                }
            }
        }
    }
    if let Some(mcp) = doc.get_mut("mcp").and_then(|i| i.as_table_mut()) {
        for key in ["env", "headers"] {
            if let Some(t) = mcp.get_mut(key).and_then(|i| i.as_table_mut()) {
                scrub(t, &format!("mcp.{key}."), &mut redacted);
            }
        }
    }
    for key in ["env", "headers"] {
        if let Some(t) = doc.get_mut(key).and_then(|i| i.as_table_mut()) {
            scrub(t, &format!("{key}."), &mut redacted);
        }
    }
    (doc.to_string(), redacted)
}

/// L05：PUT 时把占位符回填为盘上现值（编辑不丢秘密引用）；新增字面量秘密 → 拒绝。
fn restore_mcp_placeholders(
    content: &str,
    existing: &str,
    entry_name: &str,
) -> std::result::Result<String, String> {
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("MCP TOML 解析失败: {e}"))?;
    let existing_spec = mcp_spec_from_content(entry_name, existing).ok();
    let restore = |doc: &mut toml_edit::DocumentMut,
                   section: Option<&str>,
                   key: &str,
                   redacted: &mut Vec<String>| {
        let table_ref = match section {
            Some(s) => doc
                .get_mut(s)
                .and_then(|i| i.as_table_mut())
                .and_then(|t| t.get_mut(key).and_then(|i| i.as_table_mut())),
            None => doc.get_mut(key).and_then(|i| i.as_table_mut()),
        };
        let Some(tbl) = table_ref else { return };
        for (k, item) in tbl.iter_mut() {
            if item.as_str() == Some(SECRET_PLACEHOLDER) {
                let old = existing_spec.as_ref().and_then(|spec| {
                    let pairs = match key {
                        "env" => &spec.env,
                        _ => &spec.headers,
                    };
                    pairs
                        .iter()
                        .find(|(pk, _)| **pk == *k)
                        .map(|(_, pv)| pv.clone())
                });
                match old {
                    Some(v) => {
                        *item = toml_edit::value(v);
                    }
                    None => redacted.push(format!("{key}.{k}")),
                }
            }
        }
    };
    let mut unresolvable = Vec::new();
    for key in ["env", "headers"] {
        restore(&mut doc, Some("mcp"), key, &mut unresolvable);
        restore(&mut doc, None, key, &mut unresolvable);
    }
    if !unresolvable.is_empty() {
        return Err(format!(
            "以下键是占位符但盘上没有现值可回填：{}。请改用 $ENV: 环境变量引用，不要写明文秘密",
            unresolvable.join(", ")
        ));
    }
    // 用户新输入（回填前）含字面量秘密 → 拒绝保存；盘上既有字面量经占位符
    // 回填往返不丢失（历史数据允许存在，编辑不扩大暴露面）
    let input = content.to_string();
    if let Ok(spec) = mcp_spec_from_content(entry_name, &input) {
        let literals: Vec<String> = spec
            .env
            .iter()
            .chain(spec.headers.iter())
            .filter(|(_, v)| !v.starts_with("$ENV:") && v != SECRET_PLACEHOLDER)
            .map(|(k, _)| k.clone())
            .collect();
        if !literals.is_empty() {
            return Err(format!(
                "MCP env/headers 含字面量秘密: {:?}。秘密只允许 $ENV:NAME 引用（值不进入配置/日志/导出）",
                literals
            ));
        }
    }
    Ok(doc.to_string())
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
