//! 项目知识库与原生文件接口。

use super::*;
use serde_json::json;
use std::sync::Arc;

/// `POST /api/native-files`
pub(super) fn post_native_files(state: &Arc<ServerState>, req: &Request) -> Response {
    let parsed = serde_json::from_value::<crate::native_files::Request>(req.body.clone());
    match parsed {
        Err(e) => Response::json(400, json!({ "error": e.to_string() })),
        Ok(mut r) => {
            if r.scope == "project" {
                let Some(root) = r.root.as_ref() else {
                    return Response::json(400, json!({"error":"请选择项目目录"}));
                };
                r.root = Some(match ensure_within_roots(state, root) {
                    Ok(p) => p,
                    Err(e) => return Response::json(403, json!({"error":e})),
                });
            }
            match crate::paths::user_home() {
                Some(home) => {
                    collection_response(crate::native_files::run(&state.data_root, &home, &r))
                }
                None => Response::json(400, json!({"error":"无法确定用户目录"})),
            }
        }
    }
}

/// `POST /api/knowledge`
pub(super) fn post_knowledge(state: &Arc<ServerState>, req: &Request) -> Response {
    let parsed = serde_json::from_value::<crate::knowledge::location::Request>(req.body.clone());
    match parsed {
        Err(e) => Response::json(400, json!({ "error": e.to_string() })),
        Ok(mut r) => {
            r.root = match ensure_within_roots(state, &r.root) {
                Ok(p) => p,
                Err(e) => return Response::json(403, json!({"error":e})),
            };
            if r.action == "organization-status" {
                return super::knowledge_organize::status(state, &r.root);
            }
            if r.action == "organization-page" {
                let Some(page) = req.body["page"].as_str() else {
                    return Response::json(400, json!({"error":"请选择知识页面"}));
                };
                return collection_response(crate::knowledge::location::organization_page(
                    &state.data_root,
                    &r.root,
                    Path::new(page),
                ));
            }
            if r.action == "ingestion-status" {
                return collection_response(crate::knowledge::location::ingest::status(
                    &state.data_root,
                    &r.root,
                ));
            }
            if matches!(r.action.as_str(), "ingestion-preview" | "ingestion-apply") {
                let Some(source_id) = req.body["source_id"].as_str() else {
                    return Response::json(400, json!({"error":"请选择资料来源"}));
                };
                let status =
                    match crate::knowledge::location::ingest::status(&state.data_root, &r.root) {
                        Ok(v) => v,
                        Err(e) => return Response::error(400, &e),
                    };
                let source = status["sources"]
                    .as_array()
                    .and_then(|s| s.iter().find(|s| s["id"] == source_id));
                let Some(path) = source.and_then(|s| s["path"].as_str()) else {
                    return Response::json(400, json!({"error":"资料来源不属于本项目"}));
                };
                if let Err(e) = ensure_within_roots(state, Path::new(path)) {
                    return Response::json(403, json!({"error":e}));
                }
                if r.action == "ingestion-preview" {
                    return collection_response(crate::knowledge::location::ingest::preview(
                        &state.data_root,
                        &r.root,
                        source_id,
                    ));
                }
                let expected = req.body["expected"].as_str().unwrap_or("");
                let documents = match serde_json::from_value::<Vec<std::path::PathBuf>>(
                    req.body["documents"].clone(),
                ) {
                    Ok(v) => v,
                    Err(_) => return Response::json(400, json!({"error":"请提供选中文档清单"})),
                };
                return collection_response(crate::knowledge::location::ingest::apply(
                    &state.data_root,
                    &r.root,
                    source_id,
                    expected,
                    &documents,
                ));
            }
            if r.action == "organize" {
                return super::knowledge_organize::start(state, &r.root, req);
            }
            for field in [&mut r.path, &mut r.repository_path] {
                if let Some(p) = field.as_ref() {
                    let dest = match crate::knowledge::location::destination(p) {
                        Ok(p) => p,
                        Err(e) => return collection_response(Err(e)),
                    };
                    let mut ancestor = dest.as_path();
                    while !ancestor.exists() {
                        ancestor = ancestor.parent().unwrap();
                    }
                    if let Err(e) = ensure_within_roots(state, ancestor) {
                        return Response::json(403, json!({"error":e}));
                    }
                    *field = Some(dest);
                }
            }
            collection_response(crate::knowledge::location::run(&state.data_root, &r))
        }
    }
}
