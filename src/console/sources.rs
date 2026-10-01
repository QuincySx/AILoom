//! 资源来源接口：合集预览 / 更新与 CC Switch 迁移。

use super::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `POST /api/migrations/cc-switch/read`
pub(super) fn post_migrations_cc_switch_read(state: &Arc<ServerState>, req: &Request) -> Response {
    if req.body["confirm_source_read"].as_bool() != Some(true) {
        return Response::json(
            400,
            json!({"error":"请在迁移界面点击扫描，确认读取 Skill 来源"}),
        );
    }
    let default = crate::cc_switch::default_directory();
    let directory = req.body["directory"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| default.as_ref().ok().cloned());
    let Some(directory) = directory else {
        return Response::json(400, json!({"error":"请选择 CC Switch 数据目录"}));
    };
    // 默认目录仅开放此固定来源查询，不加入通用文件读取授权根。
    if default.as_ref().ok() != Some(&directory) {
        if let Err(e) = ensure_within_roots(state, &directory) {
            return Response::json(403, json!({"error":e}));
        }
    }
    collection_response(crate::cc_switch::scan_directory(
        &state.data_root,
        &directory,
    ))
}

/// `POST /api/migrations/cc-switch/preview`
pub(super) fn post_migrations_cc_switch_preview(
    state: &Arc<ServerState>,
    req: &Request,
) -> Response {
    let selected: std::result::Result<Vec<String>, _> =
        serde_json::from_value(req.body["selected"].clone());
    if req.body["management"].as_str() == Some("external") {
        let root = Path::new(req.body["skills_directory"].as_str().unwrap_or(""));
        let root = match ensure_within_roots(state, root) {
            Ok(p) => p,
            Err(e) => return Response::json(403, json!({"error":e})),
        };
        return match selected {
            Ok(selected) => collection_response(crate::cc_switch::prepare_external(
                &state.data_root,
                req.body["scan_id"].as_str().unwrap_or(""),
                &selected,
                &root,
            )),
            Err(_) => Response::json(400, json!({"error":"请选择来源记录"})),
        };
    }
    if req.body["management"]
        .as_str()
        .is_some_and(|m| m != "managed")
    {
        return Response::json(400, json!({"error":"未知管理方式"}));
    }
    match selected {
        Ok(selected) => collection_response(crate::cc_switch::prepare(
            &state.data_root,
            req.body["scan_id"].as_str().unwrap_or(""),
            &selected,
        )),
        Err(_) => Response::json(400, json!({"error":"请选择来源记录"})),
    }
}

/// `POST /api/collections/preview`
pub(super) fn post_collections_preview(state: &Arc<ServerState>, req: &Request) -> Response {
    let url = req.body["url"].as_str().unwrap_or("");
    // 本机 Git 仓库也必须先批准目录；远程 URL 交 GitSource 校验。
    if let Some(path) = url
        .strip_prefix("file://")
        .or_else(|| (Path::new(url).is_absolute() || !url.contains(':')).then_some(url))
    {
        if let Err(e) = ensure_within_roots(state, Path::new(path)) {
            return Response::json(403, json!({ "error": e }));
        }
    }
    collection_response(crate::collections::preview(
        &state.data_root,
        req.body["name"].as_str().unwrap_or(""),
        url,
        req.body["ref"].as_str(),
        req.body["source_id"].as_str(),
    ))
}

/// `POST /api/collections/update`
pub(super) fn post_collections_update(state: &Arc<ServerState>, req: &Request) -> Response {
    let tokens: Vec<String> = req.body["preview_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    collection_response(crate::collections::apply_previews(
        &state.data_root,
        &tokens,
    ))
}
