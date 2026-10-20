//! 项目与目录接口：项目默认预览、有效配置、部署状态、目录发现、仓库重新关联。

use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

fn source_edit_fingerprint(
    ctx: &crate::appctx::AppContext,
    path: &Path,
) -> crate::error::Result<String> {
    let mut bytes = std::fs::read(path)?;
    bytes.push(0);
    match std::fs::read(ctx.source_cache_settings_path()) {
        Ok(settings) => bytes.extend(settings),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(crate::ids::sha256_hex(&bytes))
}

/// Read or replace a project's team source, using the same validation and binding as `init`.
pub(super) fn post_project_source(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(root) = req.body["root"].as_str() else {
        return Response::json(400, json!({"error":"需要 root"}));
    };
    let root = match ensure_within_roots(state, Path::new(root)) {
        Ok(p) => p,
        Err(e) => return Response::json(403, json!({"error":e})),
    };
    let ctx = match crate::appctx::AppContext::discover(Some(&state.data_root), &root, Some(&root))
    {
        Ok(ctx) => ctx,
        Err(e) => return Response::error(400, &e),
    };
    if let Err(e) = ensure_within_roots(state, &ctx.workspace.workspace_root) {
        return Response::json(403, json!({"error":e}));
    }
    let Some(path) = ctx.workspace.declaration_path.as_ref() else {
        return Response::json(400, json!({"error":"项目尚未绑定团队来源"}));
    };
    if req.body["action"] == "read" {
        return collection_response((|| {
            let mut declaration =
                crate::config::ProjectDeclaration::load(path)?.ok_or_else(|| {
                    crate::error::Error::new(
                        crate::error::code::WORKSPACE_INVALID,
                        "项目声明不存在",
                    )
                })?;
            let cache_root = if let Some(url) = declaration.source.url.as_deref() {
                let source = crate::source::GitSource::new(url, None)?;
                ctx.source_cache_root(&source.identity)
            } else {
                ctx.default_source_cache_root()
            };
            declaration.source.url = declaration
                .source
                .url
                .map(|url| crate::gitx::redact_credentials(&url));
            declaration.source.path = declaration.source.path.map(|path| {
                let path = std::path::PathBuf::from(path);
                if path.is_absolute() {
                    path
                } else {
                    ctx.workspace.workspace_root.join(path)
                }
                .display()
                .to_string()
            });
            Ok(
                json!({"source":declaration.source,"root":ctx.workspace.workspace_root,"cache_root":cache_root,"default_cache_root":ctx.default_source_cache_root(),"base_fingerprint":source_edit_fingerprint(&ctx,path)?}),
            )
        })());
    }
    if req.body["action"] != "save" {
        return Response::json(400, json!({"error":"未知来源操作"}));
    }
    let Some(value) = req.body["value"]
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return Response::json(400, json!({"error":"请选择资源目录或填写 Git 地址"}));
    };
    let kind = req.body["type"].as_str().unwrap_or("");
    if !matches!(kind, "local" | "git") {
        return Response::json(400, json!({"error":"来源类型必须是 local 或 git"}));
    }
    if kind == "local" && !Path::new(value).is_absolute() {
        return Response::json(400, json!({"error":"本地资源目录需要绝对路径"}));
    }
    if kind == "git" && !Path::new(value).is_absolute() && !value.contains(':') {
        return Response::json(
            400,
            json!({"error":"请填写 Git 地址，本机仓库请使用绝对路径"}),
        );
    }
    if let Some(local) = value
        .strip_prefix("file://")
        .or_else(|| Path::new(value).is_absolute().then_some(value))
    {
        if let Err(e) = ensure_within_roots(state, Path::new(local)) {
            return Response::json(403, json!({"error":e}));
        }
    }
    // The picker uses absolute paths; project.toml keeps a portable relative path.
    let local_path = if kind == "local" {
        let Some((levels, suffix)) = ctx
            .workspace
            .workspace_root
            .ancestors()
            .enumerate()
            .find_map(|(levels, base)| {
                Path::new(value)
                    .strip_prefix(base)
                    .ok()
                    .map(|suffix| (levels, suffix))
            })
        else {
            return Response::json(
                400,
                json!({"error":"资源目录与项目不在同一磁盘，请改用 Git 来源"}),
            );
        };
        let mut relative = std::path::PathBuf::new();
        for _ in 0..levels {
            relative.push("..");
        }
        relative.push(suffix);
        if relative.as_os_str().is_empty() {
            relative.push(".");
        }
        Some(relative.display().to_string())
    } else {
        None
    };
    let Some(base) = req.body["base_fingerprint"].as_str() else {
        return Response::json(400, json!({"error":"请重新读取当前来源后保存"}));
    };
    let cache_root = if kind == "git" {
        if let Some(value) = req.body.get("cache_root").filter(|v| !v.is_null()) {
            let Some(folder) = value.as_str().map(str::trim).filter(|v| !v.is_empty()) else {
                return Response::json(400, json!({"error":"请填写下载目录，或恢复默认目录"}));
            };
            let folder = Path::new(folder);
            if !folder.is_absolute() {
                return Response::json(400, json!({"error":"下载目录需要绝对路径"}));
            }
            if folder == ctx.default_source_cache_root() {
                Some(folder.to_path_buf())
            } else {
                match ensure_within_roots(state, folder) {
                    Ok(folder) if folder.is_dir() => Some(folder),
                    Ok(_) => return Response::json(400, json!({"error":"下载位置必须是文件夹"})),
                    Err(e) => return Response::json(403, json!({"error":e})),
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    collection_response(crate::source::with_file_lock(
        &state
            .data_root
            .join("console/source-edits")
            .join(format!("{}.lock", ctx.workspace.workspace_id)),
        || {
            if source_edit_fingerprint(&ctx, path)? != base {
                return Err(crate::error::Error::new(
                    crate::error::code::PRECONDITION_FAILED,
                    "来源已被其他页面修改，请重新打开后保存",
                ));
            }
            let ref_ = if kind == "git" {
                crate::source::GitSource::new(value, None)?;
                Some(
                    req.body["ref"]
                        .as_str()
                        .map(str::trim)
                        .filter(|r| !r.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            crate::gitx::remote_default_branch(value)
                                .unwrap_or_else(|| "main".into())
                        }),
                )
            } else {
                None
            };
            crate::commands::init::run_with_cache_root(
                &crate::commands::init::InitArgs {
                    no_builtin: false,
                    url: (kind == "git").then(|| value.to_string()),
                    ref_,
                    name: None,
                    local_path,
                    projects: vec![],
                    roles: vec![],
                    targets: vec![],
                    refresh: true,
                    root: Some(ctx.workspace.workspace_root.clone()),
                },
                true,
                Some(&state.data_root),
                cache_root.as_deref(),
            )
        },
    ))
}

/// `POST /api/preview/repo-default`
pub(super) fn post_preview_repo_default(state: &Arc<ServerState>, _req: &Request) -> Response {
    // 仓库默认变更影响预览：对登记的每个 active Worktree 分别出计划（无写入）
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
        json!({ "worktrees": per_wt, "note": "预览无写入；默认只应用到当前 Worktree" }),
    )
}

/// `GET /api/effective`
pub(super) fn get_effective(state: &Arc<ServerState>, req: &Request) -> Response {
    // F01/U04：作用域解析跟随页面选择的仓库/Worktree 根。必须显式给出并经目录授权：
    // 退回服务 cwd 既绕过授权，又取决于后台服务碰巧从哪个目录启动。
    let root = match required_root(state, req) {
        Ok(root) => root,
        Err(resp) => return resp,
    };
    if req.query.iter().any(|(k, v)| k == "view" && v == "project") {
        return collection_response(crate::commands::personal::project_effective(
            &root,
            &state.data_root,
        ));
    }
    match crate::commands::personal::effective(
        Some(&root),
        req.query_param("scope").map(str::to_string),
        // 与 data_root_resolved 同源（AIL-107：避免 XDG 默认根分叉）
        Some(&state.data_root),
        &state.data_root,
    ) {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::error(400, &e),
    }
}

/// `GET /api/project/discover-directories`
pub(super) fn get_project_discover_directories(
    state: &Arc<ServerState>,
    req: &Request,
) -> Response {
    let root = match required_root(state, req) {
        Ok(root) => root,
        Err(resp) => return resp,
    };
    let depth = req
        .query_param("depth")
        .map(|v| v.parse::<usize>().unwrap_or(0))
        .unwrap_or(3);
    let sub = req.query_param("sub");
    collection_response(crate::commands::discover_directories::discover(
        &root, sub, depth,
    ))
}

/// `GET /api/project/dirs`
pub(super) fn get_project_dirs(state: &Arc<ServerState>, req: &Request) -> Response {
    // AIL-122：目录选择器的「有单独配置」标注（真实 profile 记录）
    let root = match required_root(state, req) {
        Ok(root) => root,
        Err(resp) => return resp,
    };
    match crate::commands::personal::project_dirs(
        Some(&root),
        Some(&state.data_root),
        &state.data_root,
    ) {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::error(400, &e),
    }
}

/// `GET /api/deploy-status`
pub(super) fn get_deploy_status(state: &Arc<ServerState>, req: &Request) -> Response {
    // AIL-067/068：库版本 vs 各 Worktree 已部署版本对照（目标须在授权根内）
    let root = match required_root(state, req) {
        Ok(root) => root,
        Err(resp) => return resp,
    };
    match crate::commands::personal::deploy_status(
        Some(&root),
        // AIL-121：部署清单按查看目录作用域计算（scope = 相对目录）
        req.query_param("scope").map(str::to_string),
        // AIL-107 修复：必须与 data_root_resolved 同源，否则 layout 解析到
        // XDG 默认位置，managed 索引读空，部署状态永远显示「未部署」。
        Some(&state.data_root),
        &state.data_root,
    ) {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::error(400, &e),
    }
}

/// `POST /api/repo/relink`
pub(super) fn post_repo_relink(state: &Arc<ServerState>, req: &Request) -> Response {
    // F04/F05：失联 Worktree 重关联。新路径必须真实属于本仓库（common-dir 一致）；
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
    let mut reg = match crate::repo_registry::load_or_create_by_id(&state.data_root, repo_id) {
        Ok(r) => r,
        Err(e) => return Response::error(404, &e),
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
                Err(e) => Response::error(500, &e),
            }
        }
        Err(e) => Response::error(400, &e),
    }
}

/// `POST /api/project/recover`：撤回该目录中途失败的同步（与 `ailoom personal --action recover` 同一实现）。
pub(super) fn post_project_recover(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(root) = req.body["root"].as_str() else {
        return Response::json(400, json!({ "error": "需要 root" }));
    };
    let root = match ensure_within_roots(state, Path::new(root)) {
        Ok(p) => p,
        Err(e) => return Response::json(403, json!({ "error": e })),
    };
    match crate::commands::personal::recover(&root, Some(&state.data_root)) {
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
