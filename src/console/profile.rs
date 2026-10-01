//! 个人配置与项目元数据接口：状态汇总、项目元数据、作用域 / 选择 / 指令、配置草稿。

use super::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) fn api_state(state: &Arc<ServerState>) -> Response {
    let repos = read_repos_summary(&state.data_root);
    let profile_path = crate::profile::PersonalProfile::profile_path(&state.data_root);
    Response::json(
        200,
        json!({
            "service": "ailoom-console",
            "schema_version": 1,
            "repos": repos,
            "profile_path": profile_path,
            "has_profile": profile_path.is_file(),
            "approved_roots": &*state.approved_roots.lock_ok(),
            "native_picker": cfg!(target_os = "macos"),
        }),
    )
}

pub(super) fn read_repos_summary(data_root: &Path) -> Value {
    let dir = data_root.join("repos");
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let reg_file = e.path().join("registry.json");
            if let Ok(text) = std::fs::read_to_string(&reg_file) {
                if let Ok(mut v) = serde_json::from_str::<Value>(&text) {
                    let metadata = e.path().join("project.json");
                    if let Ok(text) = std::fs::read_to_string(metadata) {
                        if let Ok(meta) = serde_json::from_str::<Value>(&text) {
                            v["project"] = meta;
                        }
                    }
                    // AIL-110：文件夹项目没有 Git Worktree 概念；按 common_dir 是否可访问
                    // 合成 local 工作目录状态，让「失联」在列表/详情可见且可恢复。
                    if v["repo_id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with("nongit-"))
                        && v["worktrees"].as_object().is_some_and(|w| w.is_empty())
                    {
                        if let Some(common) = v["common_dir"].as_str() {
                            let status = if std::path::Path::new(common).is_dir() {
                                "active"
                            } else {
                                "missing"
                            };
                            v["worktrees"] =
                                json!({ "local": { "path": common, "status": status } });
                        }
                    }
                    out.push(v);
                }
            }
        }
    }
    Value::Array(out)
}

pub(super) fn api_project_metadata(state: &Arc<ServerState>, req: &Request) -> Response {
    let id = req.body["repo_id"].as_str().unwrap_or("");
    // Resolve against registered IDs, never accept a client path as a data filename.
    if !read_repos_summary(&state.data_root)
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["repo_id"] == id)
    {
        return Response::json(404, json!({"error": "项目未登记"}));
    }
    let name = req.body["name"].as_str().unwrap_or("").trim();
    let category = req.body["category"].as_str().unwrap_or("").trim();
    if name.is_empty() || name.chars().count() > 120 || category.chars().count() > 80 {
        return Response::json(
            400,
            json!({"error": "项目名称需为 1–120 字，分类最多 80 字"}),
        );
    }
    let meta = json!({"name":name,"category":category});
    let path = state.data_root.join("repos").join(id).join("project.json");
    match crate::sync_common::atomic_write(&path, meta.to_string().as_bytes()) {
        Ok(()) => Response::json(200, meta),
        Err(e) => Response::error(500, &e),
    }
}

pub(super) fn api_profile_scope(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(root) = req
        .body
        .get("root")
        .and_then(Value::as_str)
        .map(PathBuf::from)
    else {
        return Response::json(400, json!({"error":"需要 root"}));
    };
    if let Err(e) = ensure_within_roots(state, &root) {
        return Response::json(403, json!({"error":e}));
    }
    let Some(inherit) = req.body.get("inherit_resources").and_then(Value::as_bool) else {
        return Response::json(400, json!({"error":"需要 inherit_resources"}));
    };
    let result = crate::commands::personal::configure_scope(
        &root,
        req.body.get("subproject").and_then(Value::as_str),
        req.body
            .get("worktree")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        inherit,
        req.body.get("base_revision").and_then(Value::as_u64),
        &state.data_root,
    );
    match result {
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

pub(super) fn api_profile_select(state: &Arc<ServerState>, req: &Request) -> Response {
    let args = crate::commands::personal::SelectArgs {
        resource: req
            .body
            .get("resource")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        host: req
            .body
            .get("host")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        state: req
            .body
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        subproject: req
            .body
            .get("subproject")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        worktree: req
            .body
            .get("worktree")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        // F01：选择目标必须显式定位到用户在页面选择的仓库/Worktree，不用服务 cwd 猜
        repo_root: req
            .body
            .get("root")
            .and_then(|v| v.as_str())
            .map(PathBuf::from),
        // F08：并发保护（可选）
        base_revision: req.body.get("base_revision").and_then(|v| v.as_u64()),
    };
    if args.state.is_empty() {
        return Response::json(400, json!({ "error": "需要 state" }));
    }
    if args.repo_root.is_none() {
        return Response::json(
            400,
            json!({ "error": "需要 root（选择目标仓库/Worktree 的绝对路径）" }),
        );
    }
    if let Err(e) = ensure_within_roots(state, args.repo_root.as_deref().unwrap()) {
        return Response::json(403, json!({ "error": e }));
    }
    match crate::commands::personal::select(&args, &state.data_root) {
        Ok(v) => Response::json(200, v),
        Err(e) if crate::error::is_conflict(&e.code) => Response::json(
            409,
            json!({
                "error": e.message,
                "code": e.code,
                "current_revision": e.context.get("current_revision").cloned(),
            }),
        ),
        Err(e) => Response::error(400, &e),
    }
}

pub(super) fn api_profile_instructions(state: &Arc<ServerState>, req: &Request) -> Response {
    // 内容按不可信文本处理：仅作为 Markdown 保存/渲染，绝不注入命令或模板
    let clear = req
        .body
        .get("clear")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let Some(root) = req.body["root"].as_str() else {
        return Response::json(
            400,
            json!({"error":"需要 root，不能用服务启动目录代替项目"}),
        );
    };
    let root = match ensure_within_roots(state, Path::new(root)) {
        Ok(root) => root,
        Err(e) => return Response::json(403, json!({"error":e})),
    };
    let repo = match crate::repo_registry::classify_path(&root) {
        Ok(crate::repo_registry::PathClass::Git(r)) => r,
        Ok(crate::repo_registry::PathClass::NonGit { root }) => {
            crate::commands::personal::nongit_identity(&root)
        }
        Err(e) => return Response::error(400, &e),
    };
    let repo_id = repo.identity.repo_id.clone();
    // 乐观并发：版本号即当前内容的哈希。写入时带 base_revision，若与当前不同则拒绝，
    // 避免两个页面同时编辑时后保存的一方静默覆盖前者（AIL-106）。
    let current = crate::personal_instructions::load_entry(&state.data_root, &repo_id, None)
        .unwrap_or_default();
    let revision = |text: &str| crate::ids::sha256_hex(text.as_bytes())[..16].to_string();
    if req.body["read"].as_bool() == Some(true) {
        return Response::json(
            200,
            json!({"repo_id":repo_id,"content":current,"revision":revision(&current)}),
        );
    }
    if let Some(base) = req.body["base_revision"].as_str() {
        if base != revision(&current) {
            return Response::json(
                409,
                json!({
                    "error": "项目说明已在其他页面或会话中被修改，本次未保存",
                    "code": crate::error::code::PRECONDITION_FAILED,
                    "current_content": current,
                    "current_revision": revision(&current),
                }),
            );
        }
    }
    let result = if clear {
        crate::personal_instructions::clear_entry(&state.data_root, &repo_id, None)
            .map(|_| json!({ "cleared": true, "revision": revision("") }))
    } else {
        match req.body.get("content").and_then(|v| v.as_str()) {
            Some(content) => {
                crate::personal_instructions::save_entry(&state.data_root, &repo_id, None, content)
                    .map(|_| json!({ "saved": true, "revision": revision(content) }))
            }
            None => Err(crate::error::Error::new(
                crate::error::code::USAGE,
                "需要 content 或 clear",
            )),
        }
    };
    match result {
        Ok(mut v) => {
            v["repo_id"] = json!(repo_id);
            Response::json(200, v)
        }
        Err(e) => Response::error(400, &e),
    }
}

pub(super) fn draft_disk_path(data_root: &Path) -> PathBuf {
    data_root.join("console").join("draft.json")
}

pub(super) fn api_draft_get(state: &Arc<ServerState>) -> Response {
    // 内存优先；服务重启后从磁盘恢复（只恢复输入草稿，不重放动作）
    {
        let mut cur = state.draft.lock_ok();
        if cur.is_none() {
            let p = draft_disk_path(&state.data_root);
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    let rev = v.get("revision").and_then(|r| r.as_u64()).unwrap_or(0);
                    let draft = v.get("draft").cloned();
                    if let Some(d) = draft {
                        *cur = Some((rev, d));
                    }
                }
            }
        }
    }
    let draft = state.draft.lock_ok().clone();
    match draft {
        Some((rev, v)) => Response::json(200, json!({ "revision": rev, "draft": v })),
        None => Response::json(200, json!({ "revision": 0, "draft": null })),
    }
}

pub(super) fn api_draft_put(state: &Arc<ServerState>, req: &Request) -> Response {
    let base = req.body.get("base_revision").and_then(|v| v.as_u64());
    let draft = req.body.get("draft").cloned();
    let Some(draft) = draft else {
        return Response::json(400, json!({ "error": "需要 draft" }));
    };
    let mut cur = state.draft.lock_ok();
    let current_rev = cur.as_ref().map(|(r, _)| *r).unwrap_or(0);
    // 并发保存保护：过期 base → 冲突并返回当前草稿（用户草稿不丢失）
    if let Some(base) = base {
        if base != current_rev {
            return Response::json(
                409,
                json!({
                    "error": "草稿已被其他会话修改",
                    "current_revision": current_rev,
                    "draft": cur.as_ref().map(|(_, d)| d).cloned(),
                }),
            );
        }
    }
    let new_rev = current_rev + 1;
    *cur = Some((new_rev, draft.clone()));
    drop(cur);
    // 落盘：服务重启后可恢复（无执行动作，不会自动重放）
    {
        let p = draft_disk_path(&state.data_root);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = crate::sync_common::atomic_write(
            &p,
            serde_json::to_vec_pretty(&json!({ "revision": new_rev, "draft": draft }))
                .unwrap_or_default()
                .as_slice(),
        );
    }
    Response::json(200, json!({ "revision": new_rev }))
}
