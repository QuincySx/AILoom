//! 全局 Skill 接口（AIL-152）：状态、选择、预览、应用、接管与还原。

use super::{Request, Response, ServerState};
use crate::profile::GlobalKey;
use serde_json::json;
use std::sync::Arc;

fn respond(result: crate::error::Result<serde_json::Value>) -> Response {
    match result {
        Ok(v) => Response::json(200, v),
        Err(e) if crate::error::is_conflict(&e.code) => Response::error(409, &e),
        Err(e) => Response::error(400, &e),
    }
}

/// `GET /api/global/skills`
pub(super) fn get_global_skills(state: &Arc<ServerState>) -> Response {
    respond(crate::global_skills::status(&state.data_root))
}

/// `GET /api/global/plan`
pub(super) fn get_global_plan(state: &Arc<ServerState>) -> Response {
    respond(crate::global_skills::plan(&state.data_root))
}

/// `POST /api/global/select`：`{skill | target, enabled, targets?, base_revision?}`
pub(super) fn post_global_select(state: &Arc<ServerState>, req: &Request) -> Response {
    let body = &req.body;
    let key = match (body["skill"].as_str(), body["target"].as_str()) {
        (Some(s), None) => match body.get("targets") {
            None => GlobalKey::Skill(s.to_string()),
            Some(serde_json::Value::Array(values)) if values.iter().all(|v| v.is_string()) => {
                GlobalKey::SkillTargets {
                    id: s.to_string(),
                    targets: values
                        .iter()
                        .map(|v| v.as_str().unwrap().to_string())
                        .collect(),
                }
            }
            Some(_) => {
                return Response::json(400, json!({ "error": "targets 必须是全局目标名称的数组" }))
            }
        },
        (None, Some(t)) if body.get("targets").is_none() => GlobalKey::Target(t.to_string()),
        _ => return Response::json(400, json!({ "error": "需要 skill 或 target 其中之一" })),
    };
    let Some(enabled) = body["enabled"].as_bool() else {
        return Response::json(400, json!({ "error": "需要 enabled" }));
    };
    respond(crate::global_skills::select(
        &state.data_root,
        &key,
        enabled,
        body["base_revision"].as_u64(),
    ))
}

/// `POST /api/global/apply`
pub(super) fn post_global_apply(state: &Arc<ServerState>, req: &Request) -> Response {
    match req.body.get("skill") {
        Some(serde_json::Value::String(id)) => {
            respond(crate::global_skills::sync_skill(&state.data_root, id))
        }
        Some(_) => Response::json(400, json!({ "error": "skill 必须是资源 ID" })),
        None => respond(crate::global_skills::sync(&state.data_root)),
    }
}

/// `POST /api/global/takeover`：`{target, name}`
pub(super) fn post_global_takeover(state: &Arc<ServerState>, req: &Request) -> Response {
    let (Some(target), Some(name)) = (req.body["target"].as_str(), req.body["name"].as_str())
    else {
        return Response::json(400, json!({ "error": "需要 target 与 name" }));
    };
    respond(crate::global_skills::takeover(
        &state.data_root,
        target,
        name,
    ))
}

/// `POST /api/global/restore`：`{id}`
pub(super) fn post_global_restore(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(id) = req.body["id"].as_str() else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    respond(crate::global_skills::restore(&state.data_root, id))
}
