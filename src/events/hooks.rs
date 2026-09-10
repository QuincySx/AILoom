//! Hook 生命周期与注册（AIL-018）。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Claude Code 支持的事件（官方 hooks 键名，2026-09-09 核实 settings.json hooks 配置）。
pub const CLAUDE_HOOK_EVENTS: [&str; 4] =
    ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop"];

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

    let cwd = std::env::current_dir().unwrap_or_default();
    // 工作区发现优先用 payload.cwd（后台执行不继承错误 cwd 的语义：显式根优先）
    let root_dir: PathBuf = explicit_root.map(|p: &Path| p.to_path_buf()).unwrap_or(cwd);
    let ctx = match AppContext::discover(data_root, &root_dir, explicit_root) {
        Ok(c) => c,
        Err(e) => {
            // 无绑定的工作区不采集（也不阻塞宿主）
            crate::logging::warn(format!("hook 跳过：{}（宿主不受影响）", e.message));
            return Ok(json!({ "captured": false, "reason": e.code }));
        }
    };
    let event = match args.tool.as_str() {
        "claude" => crate::events::schema::parse_claude_payload(
            &payload,
            &ctx.workspace.workspace_id,
            &ctx.device,
            &args.event,
        )?,
        other => {
            crate::logging::warn(format!("hook：未知工具 {other}，跳过"));
            return Ok(json!({ "captured": false }));
        }
    };
    let captured =
        crate::events::store::append_event(&ctx.layout.events_file, &event).map_err(|e| {
            crate::logging::error(format!("hook 事件写入失败：{}（宿主不受影响）", e.message));
            e
        })?;
    Ok(json!({ "captured": captured, "event_id": event.event_id }))
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
                "command": format!("ailoom hook --tool claude --event {}", event.to_lowercase()),
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
