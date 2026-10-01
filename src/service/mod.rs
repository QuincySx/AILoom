//! Optional web process. CLI commands continue to call domain services directly.
//! A lifetime file lock is authoritative; a PID alone is never used to stop a process.
mod autostart;

use crate::console::{ConsoleOptions, ConsoleServer};
use crate::error::{code, Error, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
struct Runtime {
    /// 契约 §0：机器可读文件带 schema_version；旧文件缺省按 1 读取。
    #[serde(default = "schema_v1")]
    schema_version: u32,
    pid: u32,
    port: u16,
    token: String,
    data_root: PathBuf,
}

fn schema_v1() -> u32 {
    1
}

fn failure(message: impl Into<String>) -> Error {
    Error::new(code::SERVICE_FAILED, message)
}

fn root_path(root: &Path, create: bool) -> Result<PathBuf> {
    if create {
        fs::create_dir_all(root)?;
    }
    if root.exists() {
        return Ok(root.canonicalize()?);
    }
    Ok(if root.is_absolute() {
        root.to_owned()
    } else {
        std::env::current_dir()?.join(root)
    })
}

fn prepare(root: &Path) -> Result<PathBuf> {
    let root = root_path(root, true)?;
    let dir = root.join("service");
    if fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(failure("服务运行目录不能是符号链接"));
    }
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(root)
}

fn file(path: &Path) -> Result<File> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(failure("服务文件不能是符号链接"));
    }
    let mut opts = OpenOptions::new();
    opts.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    Ok(opts.open(path)?)
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", crate::ids::new_id()));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn management_lock(root: &Path) -> Result<File> {
    operation_lock(root, "control.lock")
}

fn operation_lock(root: &Path, name: &str) -> Result<File> {
    let f = file(&root.join("service").join(name))?;
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        match f.try_lock_exclusive() {
            Ok(()) => return Ok(f),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(e) => {
                return Err(Error::new(
                    code::LOCK_HELD,
                    format!("另一个服务管理操作尚未完成：{e}"),
                ))
            }
        }
    }
}

fn held(root: &Path) -> Result<bool> {
    let path = root.join("service/instance.lock");
    if !path.exists() {
        return Ok(false);
    }
    let f = file(&path)?;
    match f.try_lock_exclusive() {
        Ok(()) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
        Err(e) => Err(e.into()),
    }
}

fn read_runtime(root: &Path) -> Result<Runtime> {
    let r: Runtime = serde_json::from_slice(&fs::read(root.join("service/runtime.json"))?)?;
    if r.schema_version != 1
        || r.data_root != root
        || r.port == 0
        || uuid::Uuid::parse_str(&r.token).is_err()
    {
        return Err(failure("服务状态文件无效"));
    }
    Ok(r)
}

fn request(r: &Runtime, path: &str) -> Result<Value> {
    let addr = SocketAddr::from(([127, 0, 0, 1], r.port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    write!(stream, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-AILoom-Session: {}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}", r.port, r.token)?;
    let mut response = String::new();
    stream.take(64 * 1024).read_to_string(&mut response)?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| failure("网页服务返回无效响应"))?;
    if head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        != Some("200")
    {
        return Err(failure("网页服务未通过身份校验，未执行操作"));
    }
    Ok(serde_json::from_str(body)?)
}

fn healthy(root: &Path) -> Result<(Runtime, Value)> {
    let r = read_runtime(root)?;
    let response = request(&r, "/api/service/probe")?;
    if response["pid"].as_u64() != Some(r.pid as u64) || response["data_root"] != json!(root) {
        return Err(failure("网页服务身份不匹配，未执行操作"));
    }
    Ok((r, response))
}

/// No token in routine status or service logs. Only `web` returns a login URL.
pub fn status(root: &Path) -> Result<Value> {
    let root = root_path(root, false)?;
    let startup = autostart::status(&root)?;
    if !held(&root)? {
        return Ok(
            json!({"state":"stopped", "running":false, "data_root":root, "autostart":startup}),
        );
    }
    match healthy(&root) {
        Ok((r, probe)) => Ok(
            json!({"state":if probe["stopping"]==true {"stopping"} else {"running"},
            "running":true,"pid":r.pid,"port":r.port,"data_root":root,"autostart":startup}),
        ),
        Err(_) => Ok(
            json!({"state":"unreachable","running":true,"data_root":root,"autostart":startup,
            "message":"服务正在启动、停止或暂时无响应；未启动重复实例"}),
        ),
    }
}

pub fn start(root: &Path, port: u16) -> Result<Value> {
    let root = prepare(root)?;
    let _control = management_lock(&root)?;
    if held(&root)? {
        let (_, probe) = healthy(&root)
            .map_err(|_| failure("已有服务正在启动、停止或无响应，请稍后查看 service status"))?;
        if probe["stopping"] == true {
            return Err(failure("服务正在停止，请稍后重试"));
        }
        let mut v = status(&root)?;
        v["reused"] = json!(true);
        return Ok(v);
    }
    let log_path = root.join("service/service.log");
    // Bound logs across restarts, preserving one previous file for diagnostics.
    if fs::metadata(&log_path).is_ok_and(|m| m.len() > 4 * 1024 * 1024) {
        fs::rename(&log_path, root.join("service/service.previous.log"))?;
    }
    let log = file(&log_path)?;
    let mut opts = OpenOptions::new();
    opts.append(true);
    let output = opts.open(&log_path)?;
    drop(log);
    let store = root_path(&crate::paths::resolve_store_root()?, false)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .env("AILOOM_STORE_ROOT", store)
        .env("XDG_DATA_HOME", "");
    command
        .args(["--data-root"])
        .arg(&root)
        .args(["service", "run", "--port"])
        .arg(port.to_string())
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // setsid detaches from the invoking terminal; no shell or double-fork needed.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x00000200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if held(&root)? && healthy(&root).is_ok() {
            let mut v = status(&root)?;
            v["reused"] = json!(false);
            return Ok(v);
        }
        if let Some(code) = child.try_wait()? {
            return Err(failure(format!(
                "网页服务启动失败（{code}），查看 {}",
                log_path.display()
            )));
        }
        if Instant::now() >= deadline {
            return Err(failure(format!(
                "服务尚未就绪；未启动第二个实例。请查看 service status 或 {}",
                log_path.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

pub fn stop(root: &Path) -> Result<Value> {
    let root = root_path(root, false)?;
    if !root.join("service").exists() {
        return status(&root);
    }
    let _control = management_lock(&root)?;
    if held(&root)? {
        let (r, _) =
            healthy(&root).map_err(|_| failure("服务暂时无响应，未按 PID 强制终止；请稍后重试"))?;
        request(&r, "/api/shutdown")?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while held(&root)? {
            if Instant::now() >= deadline {
                return Err(failure(
                    "服务正在等待任务完成，未强制中断；请稍后查看 service status",
                ));
            }
            std::thread::sleep(Duration::from_millis(80));
        }
    }
    status(&root)
}

pub fn web(root: &Path, port: u16, open: bool) -> Result<Value> {
    let mut result = start(root, port)?;
    let root = root_path(root, false)?;
    let (r, _) = healthy(&root)?;
    let url = format!("http://127.0.0.1:{}/", r.port);
    if open {
        crate::console::open_url(&url)?;
    }
    result["url"] = json!(url);
    Ok(result)
}

pub fn set_autostart(root: &Path, enabled: bool, port: u16) -> Result<Value> {
    let root = prepare(root)?;
    let _control = operation_lock(&root, "autostart.lock")?;
    autostart::set(&root, enabled, port)?;
    autostart::status(&root)
}

pub fn autostart_status(root: &Path) -> Result<Value> {
    autostart::status(root)
}

struct Instance {
    _lock: File,
    path: PathBuf,
}
impl Drop for Instance {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Foreground implementation shared by legacy console and the managed background child.
pub fn run(root: &Path, port: u16, open: bool, show_url: bool) -> Result<()> {
    let root = prepare(root)?;
    let lock = file(&root.join("service/instance.lock"))?;
    lock.try_lock_exclusive().map_err(|_| {
        Error::new(
            code::LOCK_HELD,
            "网页服务已运行；使用 ailoom web 打开现有服务",
        )
    })?;
    let _instance = Instance {
        _lock: lock,
        path: root.join("service/runtime.json"),
    };
    #[cfg(unix)]
    unsafe {
        INTERRUPTED.store(false, std::sync::atomic::Ordering::Relaxed);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
    let server = ConsoleServer::start(&ConsoleOptions {
        port,
        data_root: root.clone(),
        open_browser: false,
    })?;
    let runtime = Runtime {
        schema_version: 1,
        pid: std::process::id(),
        port: server.port,
        token: server.token.clone(),
        data_root: root,
    };
    atomic_write(&_instance.path, &serde_json::to_vec(&runtime)?)?;
    if show_url {
        crate::logging::info(format!(
            "控制台：http://127.0.0.1:{}/（Ctrl+C 停止）",
            runtime.port
        ));
    } else {
        crate::logging::info(format!("网页服务已启动，端口 {}", runtime.port));
    }
    if open {
        server.open_browser();
    }
    loop {
        #[cfg(unix)]
        if INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed) {
            server.shutdown();
        }
        if *server.state.shutdown_flag.lock().unwrap() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    server.join(); // drains requests and jobs before releasing the lifetime lock
    crate::logging::info("网页服务已停止");
    Ok(())
}
