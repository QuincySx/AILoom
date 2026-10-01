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
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 控制台共享状态的锁：某个请求线程持锁 panic 后锁会中毒，
/// 继续使用内部数据而不是让之后的每个请求都 panic（服务整体不可用）。
pub(crate) trait LockExt<T> {
    fn lock_ok(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_ok(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub const SESSION_HEADER: &str = "x-ailoom-session";

/// 控制台 / 网页服务的默认端口；占用时自动向后寻找可用端口。
pub const DEFAULT_PORT: u16 = 47831;

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
    active_requests: AtomicUsize,
    /// 任务事件序列（AIL-050 任务进度在此追加，SSE 订阅读取）
    pub events: Mutex<Vec<Value>>,
    /// 任务账本（AIL-050）
    pub jobs: Mutex<std::collections::BTreeMap<String, jobs::Job>>,
    /// AIL-112：未托管删除的两段式确认令牌（内存态；重启即失效，重新预览即可）
    pub delete_tokens: Mutex<std::collections::BTreeMap<String, delete_skill::DeleteGrant>>,
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
        for p in (0..50).filter_map(|offset| opts.port.checked_add(offset)) {
            match TcpListener::bind(("127.0.0.1", p)) {
                Ok(listener) => {
                    let p = listener.local_addr()?.port();
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
                        active_requests: AtomicUsize::new(0),
                        events: Mutex::new(Vec::new()),
                        jobs: Mutex::new(jobs_map),
                        delete_tokens: Mutex::new(std::collections::BTreeMap::new()),
                    });
                    for id in &interrupted {
                        state.events.lock_ok().push(json!({
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
        let url = format!("http://127.0.0.1:{}/", self.port);
        if let Err(e) = open_url(&url) {
            crate::logging::warn(e.message);
        }
    }

    pub fn shutdown(&self) {
        *self.state.shutdown_flag.lock_ok() = true;
        // 打一次空闲连接唤醒 accept 循环
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }

    /// 追加任务事件（AIL-050 任务进度推送入口）。
    pub fn push_event(&self, event: Value) {
        self.state.events.lock_ok().push(event);
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
        if *state.shutdown_flag.lock_ok()
            && state.active_requests.load(Ordering::SeqCst) == 0
            && !state
                .jobs
                .lock()
                .unwrap()
                .values()
                .any(|j| matches!(j.status, jobs::JobStatus::Queued | jobs::JobStatus::Running))
        {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let st = Arc::clone(&state);
                st.active_requests.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    // macOS：accept 出的连接继承 listener 的非阻塞模式，
                    // 必须显式恢复阻塞，否则读请求 EAGAIN 会丢响应
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
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
                    st.active_requests.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

pub fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).output();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).output();
    #[cfg(windows)]
    let result = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .output();
    #[cfg(not(any(unix, windows)))]
    let result: std::io::Result<std::process::Output> =
        Err(std::io::Error::other("当前系统不支持自动打开浏览器"));
    match result {
        Ok(s) if s.status.success() => Ok(()),
        _ => Err(crate::error::Error::new(
            crate::error::code::INTERNAL,
            "无法打开浏览器；服务仍在运行，使用 ailoom web --no-open 获取地址",
        )),
    }
}

// ---------------------------------------------------------------------------
// 路由与安全检查
// ---------------------------------------------------------------------------

mod delete_skill;
mod fs;
mod http;
mod knowledge;
mod library;
mod profile;
mod project;
mod sources;
mod workflows;
use fs::*;
use http::*;
pub use http::{Request, Response};
use library::*;
use profile::*;
pub mod jobs;
pub mod ui;
pub mod web;

/// 路由表中的全部静态路径。已知路径收到不支持的方法时返回 405，而不是 404。
/// `known_paths_match_route_table` 测试保证它与 `route` 中的分支同步。
const KNOWN_PATHS: &[&str] = &[
    "/",
    "/api/capabilities",
    "/api/collections",
    "/api/collections/apply",
    "/api/collections/check",
    "/api/collections/preview",
    "/api/collections/remove",
    "/api/collections/update",
    "/api/deploy-status",
    "/api/draft",
    "/api/effective",
    "/api/events",
    "/api/fs/approve",
    "/api/fs/list",
    "/api/fs/pick-directory",
    "/api/fs/read",
    "/api/hosts/detect",
    "/api/jobs",
    "/api/jobs/apply",
    "/api/jobs/plan",
    "/api/jobs/undo",
    "/api/knowledge",
    "/api/library/check-update",
    "/api/library/delete",
    "/api/library/import",
    "/api/library/import-entry",
    "/api/library/import-git",
    "/api/library/list",
    "/api/library/resource",
    "/api/library/sources",
    "/api/library/update",
    "/api/migrations/cc-switch/apply",
    "/api/migrations/cc-switch/location",
    "/api/migrations/cc-switch/preview",
    "/api/migrations/cc-switch/read",
    "/api/migrations/cc-switch/scan",
    "/api/native-files",
    "/api/preview/repo-default",
    "/api/profile/instructions",
    "/api/profile/scope",
    "/api/profile/select",
    "/api/project/delete-skill",
    "/api/project/dirs",
    "/api/project/discover-directories",
    "/api/project/scan-skills",
    "/api/projects/metadata",
    "/api/repo/discover",
    "/api/repo/relink",
    "/api/resources",
    "/api/resources/mcp-detail",
    "/api/server-info",
    "/api/service/autostart",
    "/api/service/probe",
    "/api/service/status",
    "/api/shutdown",
    "/api/state",
    "/api/workflows",
    "/api/workflows/artifact",
    "/api/workflows/bind",
    "/api/workflows/export",
    "/api/workflows/new",
    "/api/workflows/record-input",
    "/api/workflows/rename",
    "/api/workflows/reviewed",
    "/api/workflows/show",
    "/index.html",
];

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
    if is_write
        && *state.shutdown_flag.lock_ok()
        && !matches!(req.path.as_str(), "/api/shutdown" | "/api/service/probe")
    {
        return Response::json(503, json!({"error":"服务正在停止，等待现有任务完成"}));
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/api/service/probe") => Response::json(
            200,
            json!({
                "pid":std::process::id(), "data_root":state.data_root,
                "stopping":*state.shutdown_flag.lock_ok()
            }),
        ),
        ("POST", "/api/service/status") => match crate::service::autostart_status(&state.data_root)
        {
            Ok(startup) => Response::json(
                200,
                json!({"running":true,"port":state.port,"autostart":startup}),
            ),
            Err(e) => Response::json(500, json!({"error":e.message})),
        },
        ("POST", "/api/service/autostart") => {
            let Some(enabled) = req.body.get("enabled").and_then(Value::as_bool) else {
                return Response::json(400, json!({"error":"缺少 enabled"}));
            };
            match crate::service::set_autostart(&state.data_root, enabled, state.port) {
                Ok(v) => Response::json(200, v),
                Err(e) => Response::json(400, json!({"error":e.message})),
            }
        }
        ("GET", "/") | ("GET", "/index.html") => index_page(state),
        ("GET", "/api/state") => api_state(state),
        ("GET", "/api/events") => api_events_snapshot(state, req),
        ("POST", "/api/fs/approve") => api_fs_approve(state, req),
        ("POST", "/api/fs/pick-directory") => api_pick_directory(),
        ("POST", "/api/projects/metadata") => api_project_metadata(state, req),
        ("POST", "/api/native-files") => knowledge::post_native_files(state, req),
        ("POST", "/api/knowledge") => knowledge::post_knowledge(state, req),
        ("GET", "/api/fs/list") => api_fs_list(state, req),
        ("POST", "/api/repo/discover") => api_repo_discover(state, req),
        ("POST", "/api/profile/select") => api_profile_select(state, req),
        ("POST", "/api/profile/scope") => api_profile_scope(state, req),
        ("POST", "/api/profile/instructions") => api_profile_instructions(state, req),
        ("GET", "/api/draft") => api_draft_get(state),
        ("PUT", "/api/draft") => api_draft_put(state, req),
        ("GET", "/api/capabilities") => Response::json(
            200,
            json!({ "capabilities": crate::adapters::capability::matrix() }),
        ),
        ("POST", "/api/preview/repo-default") => project::post_preview_repo_default(state, req),
        ("GET", "/api/effective") => project::get_effective(state, req),
        ("GET", "/api/project/discover-directories") => {
            project::get_project_discover_directories(state, req)
        }
        ("GET", "/api/project/dirs") => project::get_project_dirs(state, req),
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
            sources::post_migrations_cc_switch_read(state, req)
        }
        ("POST", "/api/migrations/cc-switch/scan") => collection_response(crate::cc_switch::scan(
            &state.data_root,
            &req.body["manifest"],
        )),
        ("POST", "/api/migrations/cc-switch/preview") => {
            sources::post_migrations_cc_switch_preview(state, req)
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
        ("POST", "/api/collections/preview") => sources::post_collections_preview(state, req),
        ("POST", "/api/collections/apply") => {
            collection_response(crate::collections::apply_preview(
                &state.data_root,
                req.body["preview_id"].as_str().unwrap_or(""),
            ))
        }
        ("POST", "/api/collections/check") => collection_response(
            crate::collections::check_updates(&state.data_root, req.body["source_id"].as_str()),
        ),
        ("POST", "/api/collections/update") => sources::post_collections_update(state, req),
        ("POST", "/api/collections/remove") => collection_response(crate::collections::remove(
            &state.data_root,
            req.body["source_id"].as_str().unwrap_or(""),
            req.body["execute"].as_bool().unwrap_or(false),
        )),
        ("GET", "/api/project/scan-skills") => api_project_scan_skills(state, req),
        ("POST", "/api/project/delete-skill") => delete_skill::handle(state, req),
        ("GET", "/api/resources") => library::get_resources(state, req),
        ("GET", "/api/resources/mcp-detail") => api_resource_mcp_detail(state, req),
        ("POST", "/api/library/import-git") => library::post_library_import_git(state, req),
        ("POST", "/api/library/import-entry") => library::post_library_import_entry(state, req),
        ("POST", "/api/library/check-update") => library::post_library_check_update(state, req),
        ("POST", "/api/library/update") => library::post_library_update(state, req),
        ("GET", "/api/deploy-status") => project::get_deploy_status(state, req),
        ("GET", "/api/library/sources") => library::get_library_sources(state, req),
        ("POST", "/api/library/delete") => library::post_library_delete(state, req),
        ("POST", "/api/library/resource") => library::post_library_resource(state, req),
        ("GET", "/api/library/resource") => api_library_resource_get(state, req),
        ("PUT", "/api/library/resource") => api_library_resource_put(state, req),
        ("GET", "/api/workflows") => Response::json(
            200,
            json!({ "runs": crate::workflow::list(&state.data_root) }),
        ),
        ("GET", "/api/workflows/show") => {
            let Some(id) = req.query_param("id").map(str::to_string) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            match crate::workflow::show(&state.data_root, &id) {
                Ok(r) => Response::json(200, json!(r)),
                Err(e) => Response::error(404, &e),
            }
        }
        ("GET", "/api/workflows/artifact") => workflows::get_workflows_artifact(state, req),
        ("POST", "/api/workflows/new") => {
            let Some(name) = req.body.get("name").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 name" }));
            };
            match crate::workflow::create(&state.data_root, name) {
                Ok(r) => Response::json(200, json!(r)),
                Err(e) => Response::error(400, &e),
            }
        }
        ("POST", "/api/workflows/bind") => workflows::post_workflows_bind(state, req),
        ("POST", "/api/workflows/artifact") => workflows::post_workflows_artifact(state, req),
        ("POST", "/api/workflows/rename") => workflows::post_workflows_rename(state, req),
        ("POST", "/api/workflows/reviewed") => {
            let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
                return Response::json(400, json!({ "error": "需要 id" }));
            };
            match crate::workflow::mark_reviewed(&state.data_root, id) {
                Ok(r) => Response::json(200, json!(r)),
                Err(e) => Response::error(400, &e),
            }
        }
        ("POST", "/api/workflows/record-input") => {
            workflows::post_workflows_record_input(state, req)
        }
        ("GET", "/api/fs/read") => fs::get_fs_read(state, req),
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
        ("POST", "/api/workflows/export") => workflows::post_workflows_export(state, req),
        ("POST", "/api/repo/relink") => project::post_repo_relink(state, req),
        ("POST", "/api/shutdown") => {
            *state.shutdown_flag.lock_ok() = true;
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
        (m, p) if KNOWN_PATHS.contains(&p) || p.starts_with("/api/jobs/") => {
            Response::json(405, json!({ "error": format!("{p} 不支持 {m} 方法") }))
        }
        (m, p) => Response::json(404, json!({ "error": format!("未知路径 {m} {p}") })),
    }
}

fn collection_response(result: crate::error::Result<Value>) -> Response {
    match result {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::error(
            if crate::error::is_conflict(&e.code) {
                409
            } else {
                400
            },
            &e,
        ),
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
        Err(e) => Response::error(400, &e),
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

// ---------------------------------------------------------------------------
// 资源编辑安全（AIL-049 返工）：保存前完整校验 + 秘密明文边界
// ---------------------------------------------------------------------------

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
    crate::service::run(&opts.data_root, opts.port, opts.open_browser, true)
}

#[cfg(test)]
mod console_tests {
    #[test]
    fn poisoned_lock_keeps_serving() {
        use super::LockExt;
        let m = std::sync::Arc::new(std::sync::Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("持锁 panic");
        })
        .join();
        assert!(m.lock().is_err(), "锁已中毒");
        *m.lock_ok() += 1;
        assert_eq!(*m.lock_ok(), 2);
    }

    #[test]
    fn known_paths_match_route_table() {
        let src = include_str!("mod.rs");
        let mut found: Vec<&str> = Vec::new();
        for verb in [
            "(\"GET\", \"",
            "(\"POST\", \"",
            "(\"PUT\", \"",
            "(\"DELETE\", \"",
        ] {
            for (idx, _) in src.match_indices(verb) {
                let rest = &src[idx + verb.len()..];
                let path = &rest[..rest.find('"').unwrap()];
                if path.starts_with('/') && rest[path.len()..].starts_with("\")") {
                    found.push(path);
                }
            }
        }
        found.sort();
        found.dedup();
        let mut known = super::KNOWN_PATHS.to_vec();
        known.sort();
        assert_eq!(found, known, "KNOWN_PATHS 必须与 route 分支一致");
    }

    use super::*;

    #[test]
    fn skill_update_api_requires_preview_before_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(ServerState {
            port: 12345,
            data_root: tmp.path().join("data"),
            token: "test-session".into(),
            approved_roots: Mutex::new(Vec::new()),
            draft: Mutex::new(None),
            shutdown_flag: Mutex::new(false),
            active_requests: AtomicUsize::new(0),
            events: Mutex::new(Vec::new()),
            jobs: Mutex::new(std::collections::BTreeMap::new()),
            delete_tokens: Mutex::new(std::collections::BTreeMap::new()),
        });
        for preview in [None, Some("")] {
            let mut body = json!({"skill":"demo","execute":true});
            if let Some(id) = preview {
                body["preview_id"] = json!(id);
            }
            let response = route(
                &Request {
                    method: "POST".into(),
                    path: "/api/library/update".into(),
                    query: Vec::new(),
                    headers: vec![
                        ("host".into(), "127.0.0.1:12345".into()),
                        (SESSION_HEADER.into(), "test-session".into()),
                    ],
                    body,
                },
                &state,
            );
            assert_eq!(response.status, 409);
            assert!(response.body.contains("先检查"));
            assert!(!state.data_root.exists(), "no implicit check or mutation");
        }
    }

    // AIL-113（F03）契约：个人副本条目必须带 name/path，前端按名称检索才可用。
    #[test]
    fn local_entry_json_includes_name_and_path() {
        let e = crate::personal_library::TolerantEntry {
            id: "personal/skill/personal/demo".into(),
            kind: "skill".into(),
            name: "demo".into(),
            namespace: "personal".into(),
            description: "示例".into(),
            path: "resources/skills/demo".into(),
            can_check_update: false,
            source: None,
            update: None,
        };
        let v = local_entry_json(&e);
        assert_eq!(v["name"], "demo");
        assert_eq!(v["path"], "resources/skills/demo");
        assert_eq!(v["kind"], "skill");
        assert_eq!(v["source_name"], "资源库");
    }
}
