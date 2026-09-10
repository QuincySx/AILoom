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

    if args.event == "stop" {
        if let Ok(ctx) = AppContext::discover(
            data_root,
            &std::env::current_dir().unwrap_or_default(),
            args.root.as_deref(),
        ) {
            if let Ok(decision) =
                crate::commands::session::friction_check_for_session(&ctx, "prompt-summary")
            {
                let _ = decision;
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
            // 团队 hook 执行：从工作区 hook-specs 读取结构化参数（无 shell），超时回收
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
            let mut child = std::process::Command::new(&command[0])
                .args(&command[1..])
                .current_dir(&ctx.workspace.workspace_root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| {
                    crate::error::Error::new(
                        crate::error::code::EVENT_PAYLOAD_INVALID,
                        format!("hook 启动失败: {e}"),
                    )
                })?;
            // 超时回收：轮询等待
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
            let status = loop {
                match child.try_wait() {
                    Ok(Some(st)) => break Some(st),
                    Ok(None) => {
                        if std::time::Instant::now() > deadline {
                            let _ = child.kill();
                            child.wait().ok();
                            break None;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(_) => break None,
                }
            };
            match status {
                Some(st) if st.success() => Ok(json!({ "executed": true, "id": id })),
                Some(st) => Err(crate::error::Error::new(
                    crate::error::code::EVENT_PAYLOAD_INVALID,
                    format!("hook 退出码 {}", st.code().unwrap_or(-1)),
                )),
                None => Err(crate::error::Error::new(
                    crate::error::code::EVENT_PAYLOAD_INVALID,
                    format!("hook 超时（{timeout_ms}ms），已回收子进程"),
                )),
            }
        }
        other => Err(crate::error::Error::new(
            crate::error::code::USAGE,
            format!("未知 hooks 动作: {other}（install/remove/exec）"),
        )),
    }
}
