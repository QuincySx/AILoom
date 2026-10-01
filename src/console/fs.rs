//! 本机目录相关接口：目录授权与边界、文件夹选择、列目录、仓库发现、Skill 扫描、宿主探测与事件快照。

use super::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) fn api_pick_directory() -> Response {
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
            Err(e) => Response::json(500, json!({ "error": e.to_string() })),
        }
    }
    #[cfg(not(target_os = "macos"))]
    Response::json(
        400,
        json!({"error":"当前平台尚未接入原生文件夹选择器，请输入绝对路径"}),
    )
}

pub(super) fn api_events_snapshot(state: &Arc<ServerState>, req: &Request) -> Response {
    // 会话令牌：优先请求头；query 仅为 EventSource（无法设置自定义头）保留
    let token_ok = req
        .headers
        .iter()
        .any(|(k, v)| k == SESSION_HEADER && v == &state.token)
        || req
            .query
            .iter()
            .any(|(k, v)| k == "token" && v == &state.token);
    if !token_ok {
        return Response::json(401, json!({ "error": "缺少会话令牌" }));
    }
    let events = state.events.lock_ok().clone();
    Response::json(200, json!({ "events": events }))
}

pub(super) fn api_fs_approve(state: &Arc<ServerState>, req: &Request) -> Response {
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
    let mut roots = state.approved_roots.lock_ok();
    if !roots.contains(&canon) {
        roots.push(canon.clone());
    }
    Response::json(200, json!({ "approved": canon }))
}

/// 读取必填的 `root` 查询参数并确认它位于已批准根内；返回规范化路径，或可直接返回的 400 / 403 响应。
pub(super) fn required_root(
    state: &ServerState,
    req: &Request,
) -> std::result::Result<PathBuf, Response> {
    let Some(root) = req.query_param("root") else {
        return Err(Response::json(400, json!({ "error": "需要 root" })));
    };
    ensure_within_roots(state, Path::new(root))
        .map_err(|e| Response::json(403, json!({ "error": e })))
}

pub(super) fn ensure_within_roots(
    state: &ServerState,
    target: &Path,
) -> std::result::Result<PathBuf, String> {
    let canon = target
        .canonicalize()
        .map_err(|e| format!("路径不可用: {e}"))?;
    let roots = state.approved_roots.lock_ok();
    for r in roots.iter() {
        if canon.starts_with(r) {
            return Ok(canon);
        }
    }
    Err("路径不在已批准根内（拒绝越界与符号链接逃逸）".to_string())
}

pub(super) fn api_fs_list(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(path) = req.query_param("path").map(str::to_string) else {
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

pub(super) fn api_repo_discover(state: &Arc<ServerState>, req: &Request) -> Response {
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
                    Err(e) => return Response::error(500, &e),
                };
            reg.refresh_worktrees(&d, &crate::ids::now_iso());
            if let Err(e) = reg.save(&state.data_root) {
                return Response::error(500, &e);
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
            // select/plan/sync 全链路可用（能力受限于无 Git exclude/Worktree）。
            let ident = crate::commands::personal::nongit_identity(&root);
            if let Err(e) =
                crate::repo_registry::RepoRegistry::load_or_create(&state.data_root, &ident)
                    .and_then(|r| r.save(&state.data_root))
            {
                return Response::error(500, &e);
            }
            Response::json(
                200,
                json!({
                    "kind": "nongit",
                    "root": root,
                    "repo_id": ident.identity.repo_id,
                    "note": "非 Git 路径模式：以路径为身份；无 Git exclude 登记与 Worktree 概念",
                }),
            )
        }
        Err(e) => Response::error(400, &e),
    }
}

pub(super) fn api_project_scan_skills(state: &Arc<ServerState>, req: &Request) -> Response {
    let root = match required_root(state, req) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let extra = req.query_param("sub").map(|v| v.trim().to_string());
    let extra = extra.filter(|s| !s.is_empty());
    if let Some(sub) = &extra {
        // 自定 Skill 根必须是 root 内真实存在的目录（防穿越）
        let joined = root.join(sub.trim_start_matches('/'));
        match joined.canonicalize() {
            Ok(c) if c.starts_with(&root) && c.is_dir() => {}
            Ok(c) if !c.starts_with(&root) => {
                return Response::json(
                    400,
                    json!({ "error": format!("Skill 根必须在项目目录内：{sub}") }),
                )
            }
            _ => {
                return Response::json(
                    400,
                    json!({ "error": format!("Skill 根不存在或不是目录：{sub}") }),
                )
            }
        }
    }
    // 托管实体的链接目标在 Store 根（XDG 数据目录），一并规范化避免 /tmp 与 /private/tmp 前缀不一致。
    let store_root = crate::paths::resolve_store_root().unwrap_or_else(|_| state.data_root.clone());
    let store_root = store_root.canonicalize().unwrap_or(store_root);
    let mut v =
        crate::commands::scan_skills::scan_project_skills(&root, extra.as_deref(), &store_root);
    if v.get("error").is_some() {
        return Response::json(400, v);
    }
    v["root"] = json!(root.display().to_string());
    v["scanned_sub"] = json!(extra);
    v["note"] = json!("只读扫描：未修改任何文件，未自动入库。");
    Response::json(200, v)
}

/// 宿主版本探测：仅在用户显式点击时调用；只运行 `--version`（只读、无副作用）。
pub(super) fn api_hosts_detect(_state: &Arc<ServerState>) -> Response {
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

/// `GET /api/fs/read`
pub(super) fn get_fs_read(state: &Arc<ServerState>, req: &Request) -> Response {
    // L06：现有文件导入产物——只在已批准根内读文本文件（≤256KB）
    let Some(path) = req.query_param("path").map(str::to_string) else {
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
