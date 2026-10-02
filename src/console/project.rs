//! 项目与目录接口：项目默认预览、有效配置、部署状态、目录发现、仓库重新关联。

use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

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
