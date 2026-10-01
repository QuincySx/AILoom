//! 流程工作台接口：运行、产物读写、绑定、导出。

use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

/// `GET /api/workflows/artifact`
pub(super) fn get_workflows_artifact(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(id) = req.query_param("id").map(str::to_string) else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    let Some(art) = req.query_param("artifact_id").map(str::to_string) else {
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
        Ok(content) => Response::json(200, json!({ "content": content, "version": version })),
        Err(e) => Response::error(404, &e),
    }
}

/// `POST /api/workflows/bind`
pub(super) fn post_workflows_bind(state: &Arc<ServerState>, req: &Request) -> Response {
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
        Err(e) => Response::error(400, &e),
    }
}

/// `POST /api/workflows/artifact`
pub(super) fn post_workflows_artifact(state: &Arc<ServerState>, req: &Request) -> Response {
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
    match crate::workflow::put_artifact(&state.data_root, id, stage, title, content, base_version) {
        Ok(a) => Response::json(200, json!(a)),
        Err(e) if crate::error::is_conflict(&e.code) => Response::error(409, &e),
        Err(e) => Response::error(400, &e),
    }
}

/// `POST /api/workflows/rename`
pub(super) fn post_workflows_rename(state: &Arc<ServerState>, req: &Request) -> Response {
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
        Err(e) => Response::error(400, &e),
    }
}

/// `POST /api/workflows/record-input`
pub(super) fn post_workflows_record_input(state: &Arc<ServerState>, req: &Request) -> Response {
    // L06：输入版本记录的真实入口（摘要取自资源库实际内容）
    let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    let Some(rid) = req.body.get("resource_id").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 resource_id" }));
    };
    match crate::workflow::record_resource_input(&state.data_root, id, rid) {
        Ok(r) => Response::json(200, json!(r)),
        Err(e) => Response::error(400, &e),
    }
}

/// `POST /api/workflows/export`
pub(super) fn post_workflows_export(state: &Arc<ServerState>, req: &Request) -> Response {
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
        let Some(expected_fp) = req.body.get("target_fingerprint").and_then(|v| v.as_str()) else {
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
            Err(e) => Response::error(409, &e),
        }
    } else {
        match crate::workflow::export_preview(&state.data_root, id, art, target_path) {
            Ok(v) => Response::json(200, v),
            Err(e) => Response::error(400, &e),
        }
    }
}
