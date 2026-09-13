//! hook 与 hooks 命令入口（AIL-018/020）。

use crate::appctx::AppContext;
use crate::error::Result;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct HookArgs {
    pub tool: String,
    pub event: String,
    pub root: Option<PathBuf>,
}

/// 单次 hook 事件：stdin → 标准事件；session-start/prompt 可调度无感知 auto_sync；
/// stop 事件附加摩擦提示决策（每会话最多一次）。
///
/// `emit_host_stdout`: 为 true 时，若有 pending notice 则向 stdout 写 Claude hook JSON
/// （真实宿主调用）；`--json` 调试时传 false，避免污染 envelope。
pub fn run_hook_cmd(
    args: &HookArgs,
    data_root: Option<&std::path::Path>,
    emit_host_stdout: bool,
) -> Result<Value> {
    let mut value = crate::events::hooks::run_hook(
        &crate::events::hooks::HookArgs {
            tool: args.tool.clone(),
            event: args.event.clone(),
            root: args.root.clone(),
        },
        data_root,
        args.root.as_deref(),
    )?;

    // 无感知自动同步：TTL 内跳过；到期则后台 spawn sync --refresh --from-auto
    if let Ok(ctx) = AppContext::discover(
        data_root,
        &std::env::current_dir().unwrap_or_default(),
        args.root.as_deref(),
    ) {
        let extra = crate::events::auto_sync::maybe_schedule(&ctx, &args.event);
        if let Some(obj) = value.as_object_mut() {
            if let Some(m) = extra.as_object() {
                for (k, v) in m {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        if emit_host_stdout && args.tool == "claude" {
            if let Some(ctx_text) = extra.get("additional_context").and_then(|v| v.as_str()) {
                let hook_out = json!({
                    "continue": true,
                    "additionalContext": ctx_text,
                });
                let _ = writeln_stdout(&hook_out.to_string());
            }
        }
    }

    // stop：摩擦提示接线（AIL-020）——用事件解析出的真实 session_id 做决策；
    // 认领唯一提示权后经宿主支持通道（Claude hook JSON systemMessage）投递，
    // 投递失败释放认领；不发起任何 LLM/远端请求。
    // 事件名按标准 kind 归一化：注册命令传宿主事件名（如 Stop），必须同样命中。
    if crate::events::hooks::host_event_to_kind(&args.event) == Some("stop") {
        let session_id = value
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(sid) = session_id {
            if let Ok(ctx) = AppContext::discover(
                data_root,
                &std::env::current_dir().unwrap_or_default(),
                args.root.as_deref(),
            ) {
                match crate::commands::session::friction_check_for_session(&ctx, &sid) {
                    Ok(decision) => {
                        let should = decision.get("prompt").and_then(Value::as_bool) == Some(true);
                        if should {
                            let msg = format!(
                                "会话 {} 摩擦较高（打断 {} / 工具错误 {} / 纠正 {}）。可运行 `ailoom session summary --session {}` 沉淀经验（本提示每会话最多一次）",
                                sid,
                                decision.get("interventions").and_then(Value::as_u64).unwrap_or(0),
                                decision.get("tool_errors").and_then(Value::as_u64).unwrap_or(0),
                                decision.get("corrections").and_then(Value::as_u64).unwrap_or(0),
                                sid
                            );
                            if crate::events::friction::claim_prompted(
                                &ctx.layout.summary_dir,
                                &sid,
                            )
                            .unwrap_or(false)
                            {
                                let delivered = if emit_host_stdout {
                                    writeln_stdout(
                                        &json!({ "continue": true, "systemMessage": msg })
                                            .to_string(),
                                    )
                                    .is_ok()
                                } else {
                                    // --json 调试路径：不投递宿主通道，仅在结果中呈现
                                    true
                                };
                                if delivered {
                                    if let Some(obj) = value.as_object_mut() {
                                        obj.insert("friction_notice".into(), json!(msg));
                                    }
                                } else {
                                    // 不可见提示不允许占用“已提示”状态
                                    crate::events::friction::release_prompted(
                                        &ctx.layout.summary_dir,
                                        &sid,
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        crate::logging::warn(format!("摩擦提示决策失败（宿主不受影响）: {e}"));
                    }
                }
            }
        }
    }
    Ok(value)
}

fn writeln_stdout(s: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{s}")?;
    out.flush()
}

/// 回收整个进程组（子进程以 process_group(0) 启动，pgid = pid）；
/// 组杀失败时兜底杀直接子进程。
fn kill_process_group(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &format!("-{pid}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = pid;
}

pub struct HooksArgs {
    pub action: String,
    pub id: Option<String>,
    pub event: Option<String>,
    pub root: Option<PathBuf>,
}

/// hooks install/remove：注册走托管清单（plan/apply），未知事件与不支持宿主显式报告。
pub fn run_hooks_cmd(
    args: &HooksArgs,
    json: bool,
    data_root: Option<&std::path::Path>,
) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let decl_path = ctx.declaration_path().ok_or_else(|| {
        crate::error::Error::new(crate::error::code::WORKSPACE_INVALID, "工作区未绑定")
            .fix("先运行 ailoom init")
    })?;
    let declaration = crate::config::ProjectDeclaration::load(&decl_path)?.ok_or_else(|| {
        crate::error::Error::new(crate::error::code::WORKSPACE_INVALID, "工作区未绑定")
    })?;

    let managed_path = ctx.layout.managed_manifest_path.clone();
    let mut managed =
        crate::sync::manifest::ManagedManifest::load(&managed_path)?.unwrap_or_else(|| {
            crate::sync::manifest::ManagedManifest::new(&ctx.workspace.workspace_id)
        });

    match args.action.as_str() {
        "install" => {
            let (registered, unsupported) =
                crate::events::hooks::install_registration(&declaration, &ctx)?;
            let value = json!({ "registered": registered, "unsupported": unsupported });
            if !json {
                crate::logging::info(format!("hooks 注册：{} 个事件", registered.len()));
            }
            Ok(value)
        }
        "remove" => {
            let removed = crate::events::hooks::remove_registration(&ctx)?;
            // 清单中的 hook 记录一并清理
            let hook_keys: Vec<String> = managed
                .items
                .iter()
                .filter(|(_, item)| item.resource_id.starts_with("ailoom-builtin/hook-"))
                .map(|(k, _)| k.clone())
                .collect();
            for key in hook_keys {
                managed.items.remove(&key);
            }
            managed.save(&managed_path)?;
            let value = json!({ "removed": removed });
            if !json {
                crate::logging::info("已移除 ailoom hook 注册");
            }
            Ok(value)
        }
        "exec" => {
            // 团队 hook 执行：从工作区 hook-specs 读取结构化参数（无 shell），超时回收。
            // 契约（AIL-032）：本命令由宿主 Hook 直接调用——团队 hook 一次失败/超时
            // 不得破坏宿主任务，因此 exec 自身始终退出 0，失败细节走 stderr 诊断
            // 与 JSON 结果字段；并发输出用独立线程持续消费，管道满不再误判超时。
            let id = args.id.as_deref().ok_or_else(|| {
                crate::error::Error::new(crate::error::code::USAGE, "exec 需要 --id <resource-id>")
            })?;
            let spec_path = ctx
                .workspace
                .workspace_root
                .join(".ailoom-hook-specs")
                .join(format!("{}.json", id.replace('/', "__")));
            let spec: Value =
                serde_json::from_str(&std::fs::read_to_string(&spec_path)?).map_err(|e| {
                    crate::error::Error::new(
                        crate::error::code::EVENT_PAYLOAD_INVALID,
                        format!("hook spec 损坏: {e}"),
                    )
                })?;
            let command: Vec<String> = spec["command"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if command.is_empty() {
                return Err(crate::error::Error::new(
                    crate::error::code::USAGE,
                    "hook spec 无 command",
                ));
            }
            let timeout_ms = spec["timeout_ms"].as_u64().unwrap_or(5000);
            let mut cmd = std::process::Command::new(&command[0]);
            cmd.args(&command[1..])
                .current_dir(&ctx.workspace.workspace_root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            // 独立进程组：超时回收派生进程树，而不是只 kill 直接子进程
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }
            let mut child = cmd.spawn().map_err(|e| {
                crate::error::Error::new(
                    crate::error::code::EVENT_PAYLOAD_INVALID,
                    format!("hook 启动失败: {e}"),
                )
            })?;
            // 输出持续消费（避免管道容量阻塞子进程导致误超时）
            let out_pipe = child.stdout.take();
            let err_pipe = child.stderr.take();
            let out_thread = std::thread::spawn(move || {
                let mut buf = String::new();
                if let Some(mut p) = out_pipe {
                    use std::io::Read;
                    let _ = p.read_to_string(&mut buf);
                }
                buf
            });
            let err_thread = std::thread::spawn(move || {
                let mut buf = String::new();
                if let Some(mut p) = err_pipe {
                    use std::io::Read;
                    let _ = p.read_to_string(&mut buf);
                }
                buf
            });
            // 超时回收：轮询等待；到期 kill 整个进程组
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
            let status = loop {
                match child.try_wait() {
                    Ok(Some(st)) => break Some(st),
                    Ok(None) => {
                        if std::time::Instant::now() > deadline {
                            kill_process_group(child.id());
                            let _ = child.wait();
                            break None;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(_) => break None,
                }
            };
            let _stdout = out_thread.join().unwrap_or_default();
            let stderr = err_thread.join().unwrap_or_default();
            if !stderr.trim().is_empty() {
                crate::logging::info(format!("hook {id} stderr: {}", stderr.trim_end()));
            }
            match status {
                Some(st) if st.success() => {
                    Ok(json!({ "executed": true, "id": id, "timed_out": false }))
                }
                Some(st) => {
                    let code = st.code().unwrap_or(-1);
                    crate::logging::error(format!(
                        "团队 hook {id} 退出码 {code}（宿主任务不受影响）"
                    ));
                    Ok(json!({ "executed": false, "id": id, "exit": code, "timed_out": false }))
                }
                None => {
                    crate::logging::error(format!(
                        "团队 hook {id} 超时（{timeout_ms}ms），已回收进程组（宿主任务不受影响）"
                    ));
                    Ok(json!({ "executed": false, "id": id, "timed_out": true }))
                }
            }
        }
        other => Err(crate::error::Error::new(
            crate::error::code::USAGE,
            format!("未知 hooks 动作: {other}（install/remove/exec）"),
        )),
    }
}
