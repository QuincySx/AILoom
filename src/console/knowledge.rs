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
            if let Some(p) = &r.path {
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
                r.path = Some(dest);
            }
            collection_response(crate::knowledge::location::run(&state.data_root, &r))
        }
    }
}
