//! Hook 生命周期与注册（AIL-018）。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Claude Code 支持的事件（官方 hooks 键名，2026-09-09 核实 settings.json hooks 配置）。
pub const CLAUDE_HOOK_EVENTS: [&str; 4] =
    ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop"];

/// 宿主事件 → 标准事件（session-start/prompt/tool/stop）的显式映射。
/// 同时兼容历史错误注册（全小写连接形式）；标准名原样通过；未知返回 None。
pub fn host_event_to_kind(host_event: &str) -> Option<&'static str> {
    let norm = host_event
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], "");
    match norm.as_str() {
        "sessionstart" | "session-start" => Some("session-start"),
        "userpromptsubmit" | "prompt" => Some("prompt"),
        "posttooluse" | "tool" => Some("tool"),
        "stop" => Some("stop"),
        _ => None,
    }
}

pub struct HookArgs {
    pub tool: String,
    pub event: String,
    pub root: Option<PathBuf>,
}

/// hook 子命令：读 stdin payload → 标准事件 → 追加工作区事件文件。
/// 任何失败只写 stderr 诊断并退出 0（绝不阻塞宿主）；限时完成（本命令无网络操作）。
pub fn run_hook(
    args: &HookArgs,
    data_root: Option<&std::path::Path>,
    explicit_root: Option<&Path>,
) -> Result<Value> {
    use std::io::Read;
    let mut payload = String::new();
    // 限时读取：stdin 就绪内容（宿主会关闭写端）
    std::io::stdin().read_to_string(&mut payload).map_err(|e| {
        crate::logging::error(format!("hook stdin 读取失败: {e}（宿主不受影响）"));
        Error::new(code::EVENT_PAYLOAD_INVALID, "stdin 不可读")
    })?;

    // 工作区根优先级（AIL-018 冻结）：显式 --root > 宿主 payload.cwd > 进程 cwd。
    let payload_cwd: Option<PathBuf> = serde_json::from_str::<serde_json::Value>(&payload)
        .ok()
        .and_then(|v| {
            v.get("cwd")
                .and_then(|c| c.as_str())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        });
    let start_dir: PathBuf = match (explicit_root, &payload_cwd) {
        (Some(r), _) => r.to_path_buf(),
        (None, Some(c)) => c.clone(),
        (None, None) => std::env::current_dir().unwrap_or_default(),
    };
    if payload_cwd.is_none() && explicit_root.is_none() {
        crate::logging::error(
            "hook：payload 无 cwd 且未显式指定 --root，使用进程 cwd（有串工作区风险）",
        );
    }
    let ctx = match AppContext::discover(data_root, &start_dir, explicit_root) {
        Ok(c) => c,
        Err(e) => {
            // 无绑定/不可发现根：诊断后跳过（不阻塞宿主）
            crate::logging::error(format!(
                "hook 跳过：{}（root={}，宿主不受影响）",
                e.message,
                start_dir.display()
            ));
            return Ok(json!({ "captured": false, "reason": e.code }));
        }
    };
    // 宿主事件名 → 标准事件；未知事件诊断后跳过
    let kind = match host_event_to_kind(&args.event) {
        Some(k) => k.to_string(),
        None => {
            crate::logging::error(format!(
                "hook：未知宿主事件 {}（E7001，宿主不受影响）",
                args.event
            ));
            return Ok(json!({ "captured": false, "reason": "unknown-event" }));
        }
    };
    // AIL-019：纠正关键词在 Hook 受控入口即时匹配（文本不落盘），配置可经
    // heuristic.toml 覆盖
    let heuristic = crate::events::aggregate::HeuristicConfig::load(&ctx.layout.ws_dir);
    let event = match args.tool.as_str() {
        "claude" => crate::events::schema::parse_claude_payload(
            &payload,
            &ctx.workspace.workspace_id,
            &ctx.device,
            &kind,
            &heuristic.correction_keywords,
        )?,
        other => {
            crate::logging::warn(format!("hook：未知工具 {other}，跳过"));
            return Ok(json!({ "captured": false }));
        }
    };
    let captured = crate::events::store::append_event(&ctx.layout.events_file, &event)
        .inspect_err(|e| {
            crate::logging::error(format!("hook 事件写入失败：{}（宿主不受影响）", e.message));
        })?;
    // RW-15/R14：把采集阶段解析好的工作区根带回给调用者（显式 root > payload.cwd
    // > 进程 cwd），后续处理（自动同步/摩擦提示）必须沿用同一解析结果，
    // 不得回退进程 cwd 重新发现。
    Ok(json!({
        "captured": captured,
        "event_id": event.event_id,
        "session_id": event.session_id,
        "kind": event.kind,
        "workspace_root": ctx.workspace.workspace_root.display().to_string(),
    }))
}

/// hooks 注册（专用托管操作）：读取现有 settings.json，按签名替换 AILoom 条目
/// （command 以 "ailoom hook" 开头），保留用户条目，原子写回。
/// 返回 (注册的事件键, 不支持说明)。 drift 检测：清单记录数组哈希。
pub fn install_registration(
    declaration: &ProjectDeclaration,
    ctx: &AppContext,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut registered = Vec::new();
    let mut unsupported = Vec::new();
    if !declaration.targets.claude {
        return Ok((registered, unsupported));
    }
    let settings_path = ctx.workspace.workspace_root.join(".claude/settings.json");
    let mut root: serde_json::Value = if settings_path.is_file() {
        let text = std::fs::read_to_string(&settings_path).map_err(|e| {
            Error::new(
                code::USER_CONTENT_CONFLICT,
                format!("settings.json 不可读: {e}"),
            )
        })?;
        serde_json::from_str(&text).map_err(|e| {
            Error::new(
                code::USER_CONTENT_CONFLICT,
                format!("settings.json 解析失败（保留原文件，不自动改写）: {e}"),
            )
        })?
    } else {
        json!({})
    };
    if root.get("hooks").is_none() {
        root["hooks"] = json!({});
    }
    for event in CLAUDE_HOOK_EVENTS {
        let key = format!("/hooks/{event}");
        let mut merged: Vec<Value> = Vec::new();
        if let Some(arr) = root.pointer(&key).and_then(|v| v.as_array()) {
            for entry in arr {
                let is_ours = entry
                    .pointer("/hooks")
                    .and_then(|h| h.as_array())
                    .map(|hs| {
                        hs.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .map(|c| c.starts_with("ailoom hook"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);
                if !is_ours {
                    merged.push(entry.clone());
                }
            }
        }
        merged.push(json!({
            "matcher": "",
            "hooks": [{
                "type": "command",
                "command": format!("ailoom hook --tool claude --event {event}"),
                "timeout": 5
            }]
        }));
        root["hooks"][event] = Value::Array(merged);
        registered.push(event.to_string());
    }
    if root
        .pointer("/hooks")
        .and_then(|h| h.as_object())
        .map(|o| o.is_empty())
        .unwrap_or(false)
    {
        if let Some(obj) = root.as_object_mut() {
            obj.remove("hooks");
        }
    }
    if declaration.targets.codex {
        unsupported
            .push("codex: 项目级 hooks 配置未在官方文档核实（能力矩阵 unknown），不注册".into());
    }
    if registered.is_empty() {
        return Ok((registered, unsupported));
    }
    crate::sync_common::atomic_write(
        settings_path.as_path(),
        serde_json::to_vec_pretty(&root)?.as_slice(),
    )?;
    Ok((registered, unsupported))
}

/// hooks 注销：按签名移除 AILoom 条目（保留用户条目）；幂等。
pub fn remove_registration(ctx: &AppContext) -> Result<usize> {
    let settings_path = ctx.workspace.workspace_root.join(".claude/settings.json");
    if !settings_path.is_file() {
        return Ok(0);
    }
    let text = std::fs::read_to_string(&settings_path)?;
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Err(Error::new(
            code::USER_CONTENT_CONFLICT,
            "settings.json 解析失败（保留原文件，不猜删）",
        ));
    };
    let mut removed = 0usize;
    if let Some(events) = root.pointer_mut("/hooks").and_then(|h| h.as_object_mut()) {
        for (_event, arr) in events.iter_mut() {
            if let Some(list) = arr.as_array_mut() {
                let before = list.len();
                list.retain(|entry| {
                    let is_ours = entry
                        .pointer("/hooks")
                        .and_then(|h| h.as_array())
                        .map(|hs| {
                            hs.iter().any(|h| {
                                h.get("command")
                                    .and_then(|c| c.as_str())
                                    .map(|c| c.starts_with("ailoom hook"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false);
                    !is_ours
                });
                removed += before - list.len();
            }
        }
        // 空数组清理
        if let Some(obj) = root.pointer_mut("/hooks").and_then(|h| h.as_object_mut()) {
            obj.retain(|_, v| !v.as_array().map(|a| a.is_empty()).unwrap_or(false));
            if obj.is_empty() {
                if let Some(top) = root.as_object_mut() {
                    top.remove("hooks");
                }
            }
        }
    }
    if removed > 0 {
        crate::sync_common::atomic_write(
            settings_path.as_path(),
            serde_json::to_vec_pretty(&root)?.as_slice(),
        )?;
    }
    Ok(removed)
}
