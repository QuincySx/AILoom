//! 资源库与资源编辑接口：列表、导入、单个资源读写、MCP 详情与秘密字面量掩码。

use super::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// AIL-113（F03）：个人副本条目必须携带 name/path —— 本地导入是常用非 Git 来源，
/// 缺 name 会让按名称搜索失效、选择器显示 undefined。
pub(super) fn local_entry_json(e: &crate::personal_library::TolerantEntry) -> Value {
    json!({
        "id": e.id, "kind": e.kind, "name": e.name, "description": e.description,
        "path": e.path, "source_name": "资源库", "readonly": false
    })
}

/// 资源库导入：预览默认；execute=true 才复制。目录必须在已批准根内。
pub(super) fn api_library_import(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(dir) = req.body.get("dir").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 dir" }));
    };
    if let Err(e) = ensure_within_roots(state, Path::new(dir)) {
        return Response::json(403, json!({ "error": e }));
    }
    let execute = req
        .body
        .get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let name = req.body.get("name").and_then(|v| v.as_str());
    let result = if execute {
        crate::personal_library::import_execute(&state.data_root, Path::new(dir), name)
            .map(|r| json!({ "executed": true, "report": r }))
    } else {
        crate::personal_library::import_preview(&state.data_root, Path::new(dir), name)
            .map(|p| json!({ "executed": false, "preview": p }))
    };
    match result {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::error(400, &e),
    }
}

pub(super) fn library_entries(state: &Arc<ServerState>) -> Response {
    // AIL-062：宽容列表——坏条目作为 issues 返回（带文件级定位），不锁死整个列表
    let lib = crate::personal_library::library_root(&state.data_root);
    let (entries, issues) = crate::personal_library::list_tolerant(&state.data_root);
    Response::json(
        200,
        json!({
            "path": lib,
            "entries": entries,
            "issues": issues,
            "note": if issues.is_empty() { "".to_string() } else {
                "部分条目无法解析（见 issues）；其余资源仍可编辑/删除，修复后自动恢复".to_string()
            },
        }),
    )
}

pub(super) fn api_library_list(state: &Arc<ServerState>) -> Response {
    library_entries(state)
}

/// AIL-062：按宽容列表定位资源；坏条目返回 422 + 具体文件错误（可修复后重试）。
pub(super) struct LibraryTarget {
    pub(super) file: PathBuf,
    pub(super) kind: crate::resource::ResourceKind,
    pub(super) name: String,
}

pub(super) fn find_library_target(
    state: &ServerState,
    id: &str,
) -> std::result::Result<LibraryTarget, Response> {
    let lib = crate::personal_library::library_root(&state.data_root);
    let (entries, issues) = crate::personal_library::list_tolerant(&state.data_root);
    if let Some(e) = entries.iter().find(|e| e.id == id) {
        let kind = match e.kind.as_str() {
            "skill" => crate::resource::ResourceKind::Skill,
            "rule" => crate::resource::ResourceKind::Rule,
            "doc" => crate::resource::ResourceKind::Doc,
            "agent" => crate::resource::ResourceKind::Agent,
            "mcp" => crate::resource::ResourceKind::Mcp,
            "env" => crate::resource::ResourceKind::Env,
            "hook" => crate::resource::ResourceKind::Hook,
            "package" => crate::resource::ResourceKind::Package,
            _ => crate::resource::ResourceKind::Learning,
        };
        let file = if e.kind == "skill" {
            lib.join(&e.path).join("SKILL.md")
        } else {
            lib.join(&e.path)
        };
        return Ok(LibraryTarget {
            file,
            kind,
            name: e.name.clone(),
        });
    }
    for issue in &issues {
        let name = std::path::Path::new(&issue.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let stem = name.trim_end_matches(".md").trim_end_matches(".toml");
        if id.ends_with(stem) {
            return Err(Response::json(
                422,
                json!({
                    "error": format!("资源存在但无法解析：{}（{}）。修复该文件后即可继续编辑", issue.path, issue.error),
                    "issue_path": issue.path,
                }),
            ));
        }
    }
    Err(Response::json(404, json!({ "error": "资源不存在" })))
}

/// 读资源正文 + 指纹（并发保护）。
pub(super) fn api_library_resource_get(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(id) = req.query_param("id").map(str::to_string) else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    let entry = match find_library_target(state, &id) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let file = entry.file;
    match std::fs::read_to_string(&file) {
        Ok(content) => {
            let fingerprint = crate::ids::sha256_hex(content.as_bytes());
            // L05：MCP 原文不回显字面量秘密（占位符替换；保存时回填现值）
            if entry.kind == crate::resource::ResourceKind::Mcp {
                let (redacted_content, fields) = redact_mcp_literals(&content);
                return Response::json(
                    200,
                    json!({
                        "id": id,
                        "content": redacted_content,
                        "fingerprint": fingerprint,
                        "redacted_fields": fields,
                        "note": if fields.is_empty() { "".to_string() } else {
                            "以上键为秘密：编辑器显示占位符，保存时自动回填盘上现值；新增秘密请用 $ENV:NAME 引用".to_string()
                        },
                    }),
                );
            }
            Response::json(
                200,
                json!({ "id": id, "content": content, "fingerprint": fingerprint, "definition":crate::personal_library::definition_fields(entry.kind.as_str(), &content).ok() }),
            )
        }
        Err(e) => Response::json(400, json!({ "error": format!("读取失败: {e}") })),
    }
}

/// 保存资源正文：base_fingerprint 不一致 → 409（保护外部编辑，不覆盖）。
pub(super) fn api_library_resource_put(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    let base = req.body.get("base_fingerprint").and_then(|v| v.as_str());
    let entry = match find_library_target(state, id) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let file = entry.file;
    let edited;
    let content = if let Some(fields) = req.body.get("definition") {
        if base.is_none() {
            return Response::json(400, json!({"error":"需要文件版本"}));
        }
        let existing = match std::fs::read_to_string(&file) {
            Ok(v) => v,
            Err(e) => return Response::json(400, json!({ "error": e.to_string() })),
        };
        if base != Some(crate::ids::sha256_hex(existing.as_bytes()).as_str()) {
            return Response::json(409, json!({"error":"文件已被修改，请重新打开"}));
        }
        edited = match crate::personal_library::edit_definition(
            entry.kind.as_str(),
            &existing,
            fields["description"].as_str().unwrap_or(""),
            fields["body"].as_str().unwrap_or(""),
        ) {
            Ok(v) => v,
            Err(e) => return Response::error(400, &e),
        };
        edited.as_str()
    } else if let Some(v) = req.body.get("content").and_then(|v| v.as_str()) {
        v
    } else {
        return Response::json(400, json!({"error":"需要内容"}));
    };
    // L02：保存前按类型完整校验（错误定位到字段；不过不落盘，库不再被坏文件锁死）
    if let Err(msg) = validate_resource_content(&entry.kind, &entry.name, content) {
        return Response::json(400, json!({ "error": format!("保存被拒绝：{msg}") }));
    }
    // L05：MCP 占位符回填 + 字面量秘密边界
    let content_owned: String;
    let content = if entry.kind == crate::resource::ResourceKind::Mcp {
        let existing_now = std::fs::read_to_string(&file).unwrap_or_default();
        match restore_mcp_placeholders(content, &existing_now, &entry.name) {
            Ok(restored) => {
                content_owned = restored;
                content_owned.as_str()
            }
            Err(msg) => return Response::json(400, json!({ "error": msg })),
        }
    } else {
        content
    };
    let current = std::fs::read_to_string(&file).unwrap_or_default();
    let current_fp = crate::ids::sha256_hex(current.as_bytes());
    if let Some(base) = base {
        if base != current_fp {
            return Response::json(
                409,
                json!({
                    "error": "文件已被外部修改，拒绝覆盖（草稿保留在你的编辑器中）",
                    "current_fingerprint": current_fp,
                }),
            );
        }
    }
    let write = crate::sync_common::atomic_write(&file, content.as_bytes());
    if write.is_err() {
        return Response::json(500, json!({ "error": write.err().unwrap().to_string() }));
    }
    Response::json(
        200,
        json!({ "saved": true, "fingerprint": crate::ids::sha256_hex(content.as_bytes()) }),
    )
}

/// 编辑器中秘密的占位值（GET 时原文被替换；PUT 时用盘上现值回填）。
pub(super) const SECRET_PLACEHOLDER: &str = "__AILOOM_REDACTED__";

/// L02：保存前按资源类型完整校验（错误定位到字段；校验不过不落盘，
/// 杜绝「保存成功但整个库无法再次读取」）。
pub(super) fn validate_resource_content(
    kind: &crate::resource::ResourceKind,
    entry_name: &str,
    content: &str,
) -> std::result::Result<(), String> {
    match kind {
        crate::resource::ResourceKind::Skill => {
            let (meta, _) = crate::resource::parse_frontmatter(content)
                .map_err(|e| format!("frontmatter 校验失败: {e}"))?;
            match meta {
                Some(m) => {
                    if m.name.as_deref() != Some(entry_name) {
                        return Err(format!(
                            "frontmatter name({:?}) 必须与资源名({entry_name}) 一致（身份稳定）",
                            m.name
                        ));
                    }
                }
                None => return Err("SKILL.md 缺少 frontmatter（name/description）".into()),
            }
            Ok(())
        }
        crate::resource::ResourceKind::Rule | crate::resource::ResourceKind::Agent => {
            let meta: crate::resource::RawMeta = if *kind == crate::resource::ResourceKind::Rule {
                crate::resource::parse_frontmatter(content)
                    .map_err(|e| e.to_string())?
                    .0
                    .ok_or("缺少规则元数据")?
            } else {
                toml::from_str(content).map_err(|e| format!("Agent 格式错误：{e}"))?
            };
            if meta.name.as_deref() != Some(entry_name)
                || meta.namespace.as_deref() != Some("personal")
            {
                return Err("名称和命名空间不能在编辑内容时修改".into());
            }
            if !meta.shared.unwrap_or(false)
                && meta.projects.as_ref().is_none_or(|v| v.is_empty())
                && meta.roles.as_ref().is_none_or(|v| v.is_empty())
            {
                return Err("资源缺少适用范围".into());
            }
            if *kind == crate::resource::ResourceKind::Agent {
                let entry = crate::resource::ResourceEntry {
                    id: crate::resource::ResourceId {
                        source: "personal".into(),
                        kind: *kind,
                        namespace: "personal".into(),
                        name: entry_name.into(),
                    },
                    meta: crate::resource::ResourceMeta {
                        shared: true,
                        projects: vec![],
                        roles: vec![],
                        namespace: "personal".into(),
                        tags: vec![],
                    },
                    path: String::new(),
                    description: String::new(),
                    raw: Some(content.into()),
                };
                let spec =
                    crate::adapters::agents::parse_spec(&entry).map_err(|e| e.to_string())?;
                if spec.instructions.trim().is_empty() {
                    return Err("Agent 指令不能为空".into());
                }
            }
            Ok(())
        }
        crate::resource::ResourceKind::Doc | crate::resource::ResourceKind::Learning => {
            // Markdown：frontmatter 存在就必须合法（未闭合/类型错误即时定位）
            crate::resource::parse_frontmatter(content)
                .map(|_| ())
                .map_err(|e| format!("frontmatter 校验失败: {e}"))
        }
        crate::resource::ResourceKind::Mcp => {
            mcp_spec_from_content(entry_name, content).map(|_| ())
        }
        _ => Ok(()),
    }
}

pub(super) fn mcp_spec_from_content(
    entry_name: &str,
    content: &str,
) -> std::result::Result<crate::adapters::mcp::McpSpec, String> {
    let entry = crate::resource::ResourceEntry {
        id: crate::resource::ResourceId {
            source: "personal".into(),
            kind: crate::resource::ResourceKind::Mcp,
            namespace: "personal".into(),
            name: entry_name.into(),
        },
        meta: crate::resource::ResourceMeta {
            shared: true,
            projects: vec![],
            roles: vec![],
            namespace: "personal".into(),
            tags: vec![],
        },
        path: String::new(),
        description: String::new(),
        raw: Some(content.to_string()),
    };
    crate::adapters::mcp::parse_spec(&entry).map_err(|e| e.to_string())
}

/// L05：GET 时不回显字面量秘密。env/headers 中非 `$ENV:` 引用的值替换为占位符，
/// 返回（脱敏正文, 被脱敏键列表）。
pub(super) fn redact_mcp_literals(content: &str) -> (String, Vec<String>) {
    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return (content.to_string(), Vec::new()),
    };
    let mut redacted = Vec::new();
    fn scrub(table: &mut toml_edit::Table, prefix: &str, redacted: &mut Vec<String>) {
        for (k, item) in table.iter_mut() {
            if let Some(s) = item.as_str() {
                if !s.starts_with("$ENV:") && s != SECRET_PLACEHOLDER {
                    *item = toml_edit::value(SECRET_PLACEHOLDER);
                    redacted.push(format!("{prefix}{k}"));
                }
            }
        }
    }
    if let Some(mcp) = doc.get_mut("mcp").and_then(|i| i.as_table_mut()) {
        for key in ["env", "headers"] {
            if let Some(t) = mcp.get_mut(key).and_then(|i| i.as_table_mut()) {
                scrub(t, &format!("mcp.{key}."), &mut redacted);
            }
        }
    }
    for key in ["env", "headers"] {
        if let Some(t) = doc.get_mut(key).and_then(|i| i.as_table_mut()) {
            scrub(t, &format!("{key}."), &mut redacted);
        }
    }
    (doc.to_string(), redacted)
}

/// L05：PUT 时把占位符回填为盘上现值（编辑不丢秘密引用）；新增字面量秘密 → 拒绝。
pub(super) fn restore_mcp_placeholders(
    content: &str,
    existing: &str,
    entry_name: &str,
) -> std::result::Result<String, String> {
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("MCP TOML 解析失败: {e}"))?;
    let existing_spec = mcp_spec_from_content(entry_name, existing).ok();
    let restore = |doc: &mut toml_edit::DocumentMut,
                   section: Option<&str>,
                   key: &str,
                   redacted: &mut Vec<String>| {
        let table_ref = match section {
            Some(s) => doc
                .get_mut(s)
                .and_then(|i| i.as_table_mut())
                .and_then(|t| t.get_mut(key).and_then(|i| i.as_table_mut())),
            None => doc.get_mut(key).and_then(|i| i.as_table_mut()),
        };
        let Some(tbl) = table_ref else { return };
        for (k, item) in tbl.iter_mut() {
            if item.as_str() == Some(SECRET_PLACEHOLDER) {
                let old = existing_spec.as_ref().and_then(|spec| {
                    let pairs = match key {
                        "env" => &spec.env,
                        _ => &spec.headers,
                    };
                    pairs
                        .iter()
                        .find(|(pk, _)| **pk == *k)
                        .map(|(_, pv)| pv.clone())
                });
                match old {
                    Some(v) => {
                        *item = toml_edit::value(v);
                    }
                    None => redacted.push(format!("{key}.{k}")),
                }
            }
        }
    };
    let mut unresolvable = Vec::new();
    for key in ["env", "headers"] {
        restore(&mut doc, Some("mcp"), key, &mut unresolvable);
        restore(&mut doc, None, key, &mut unresolvable);
    }
    if !unresolvable.is_empty() {
        return Err(format!(
            "以下键是占位符但盘上没有现值可回填：{}。请改用 $ENV: 环境变量引用，不要写明文秘密",
            unresolvable.join(", ")
        ));
    }
    // 用户新输入（回填前）含字面量秘密 → 拒绝保存；盘上既有字面量经占位符
    // 回填往返不丢失（历史数据允许存在，编辑不扩大暴露面）
    let input = content.to_string();
    if let Ok(spec) = mcp_spec_from_content(entry_name, &input) {
        let literals: Vec<String> = spec
            .env
            .iter()
            .chain(spec.headers.iter())
            .filter(|(_, v)| !v.starts_with("$ENV:") && v != SECRET_PLACEHOLDER)
            .map(|(k, _)| k.clone())
            .collect();
        if !literals.is_empty() {
            return Err(format!(
                "MCP env/headers 含字面量秘密: {:?}。秘密只允许 $ENV:NAME 引用（值不进入配置/日志/导出）",
                literals
            ));
        }
    }
    Ok(doc.to_string())
}

pub(super) fn api_resource_mcp_detail(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(id) = req.query_param("id").map(str::to_string) else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    let registry = match crate::collections::load(&state.data_root) {
        Ok(r) => r,
        Err(e) => return Response::error(400, &e),
    };
    for source in registry.sources.values() {
        let prefix = format!("{}/", source.id);
        if !id.starts_with(&prefix) {
            continue;
        }
        let cat = match crate::collections::catalog(&state.data_root, source) {
            Ok(c) => c,
            Err(e) => return Response::error(400, &e),
        };
        let Some(entry) = cat.entries.iter().find(|e| e.id.to_string() == id) else {
            return Response::json(404, json!({ "error": "来源中不存在该资源" }));
        };
        if entry.id.kind != crate::resource::ResourceKind::Mcp {
            return Response::json(400, json!({ "error": "该资源不是 MCP 定义" }));
        }
        let spec = match crate::adapters::mcp::parse_spec(entry) {
            Ok(s) => s,
            Err(e) => return Response::error(422, &e),
        };
        let render = |v: &str| -> String {
            match v.strip_prefix("$ENV:") {
                Some(name) => format!("$ENV:{name}"),
                None => "•••（字面量已隐藏）".to_string(),
            }
        };
        let env: serde_json::Map<String, Value> = spec
            .env
            .iter()
            .map(|(k, v)| (k.clone(), json!(render(v))))
            .collect();
        let headers: serde_json::Map<String, Value> = spec
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), json!(render(v))))
            .collect();
        return Response::json(
            200,
            json!({
                "id": id,
                "name": spec.name,
                "transport": spec.kind,
                "command": spec.command,
                "args": spec.args,
                "url": spec.url,
                "env": env,
                "headers": headers,
                "source_name": source.name,
                "readonly": true,
                "note": "共享定义只读；项目级覆盖暂未支持（不会提供假编辑）。连接检测需在宿主内新会话验证，工具不代测、不因打开页面启动进程。",
            }),
        );
    }
    Response::json(404, json!({ "error": "未找到该资源的来源" }))
}

/// `GET /api/resources`
pub(super) fn get_resources(state: &Arc<ServerState>, _req: &Request) -> Response {
    let (local, issues) = crate::personal_library::list_tolerant(&state.data_root);
    let result = crate::collections::list(&state.data_root).map(|v| {
        let mut entries: Vec<Value> = local
            .iter()
            .map(local_entry_json)
            .collect();
        let mut source_errors = Vec::new();
        let mut source_warnings = Vec::new();
        for source in v["sources"].as_array().into_iter().flatten() {
            entries.extend(
                source["resources"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            if !source["error"].is_null() {
                source_errors
                    .push(json!({ "source": source["name"], "error": source["error"] }));
            }
            if let Some(w) = source["warnings"].as_array() {
                if !w.is_empty() {
                    source_warnings.push(json!({
                        "source": source["name"],
                        "warnings": w.iter().map(|x| x.as_str().unwrap_or_default()).collect::<Vec<_>>(),
                    }));
                }
            }
        }
        json!({ "entries": entries, "issues": issues, "source_errors": source_errors, "source_warnings": source_warnings })
    });
    collection_response(result)
}

/// `POST /api/library/import-git`
pub(super) fn post_library_import_git(state: &Arc<ServerState>, req: &Request) -> Response {
    // AIL-064：GitHub/远程仓库导入（预览默认；execute 才复制）
    let Some(url) = req.body.get("url").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 url" }));
    };
    let repo_path = req.body.get("path").and_then(|v| v.as_str());
    let ref_ = req.body.get("ref").and_then(|v| v.as_str());
    let name = req.body.get("name").and_then(|v| v.as_str());
    let execute = req
        .body
        .get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let result = if execute {
        crate::personal_library::git_import_execute(&state.data_root, url, repo_path, ref_, name)
            .map(|r| json!({ "executed": true, "report": r }))
    } else {
        crate::personal_library::git_import_preview(&state.data_root, url, repo_path, ref_, name)
            .map(|p| json!({ "executed": false, "preview": p }))
    };
    match result {
        Ok(v) => Response::json(200, v),
        Err(e) => {
            let ctx_candidates = e.context.get("candidates").cloned();
            Response::json(
                400,
                json!({ "error": e.message, "code": e.code, "candidates": ctx_candidates }),
            )
        }
    }
}

/// `POST /api/library/import-entry`
pub(super) fn post_library_import_entry(state: &Arc<ServerState>, req: &Request) -> Response {
    // AIL-065：发现入口（skills.sh/…）导入；预览默认，execute 才复制
    let Some(entry) = req.body.get("entry").and_then(|v| v.as_str()) else {
        return Response::json(
            400,
            json!({ "error": "需要 entry（发现入口，如 skills.sh/<owner>/<repo>/<skill>）" }),
        );
    };
    let name = req.body.get("name").and_then(|v| v.as_str());
    let execute = req
        .body
        .get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match crate::personal_library::import_via_discovery(&state.data_root, entry, name, execute) {
        Ok(v) => Response::json(200, v),
        Err(e) => Response::error(400, &e),
    }
}

/// `POST /api/library/check-update`
pub(super) fn post_library_check_update(state: &Arc<ServerState>, req: &Request) -> Response {
    // AIL-066：显式检查更新（只比较，不应用，不联网到非来源地址）
    let Some(skill) = req.body.get("skill").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 skill" }));
    };
    match crate::personal_library::check_update(&state.data_root, skill) {
        Ok(st) => Response::json(200, json!({ "status": st })),
        Err(e) => Response::error(400, &e),
    }
}

/// `POST /api/library/update`
pub(super) fn post_library_update(state: &Arc<ServerState>, req: &Request) -> Response {
    // AIL-066：应用更新（仅库内；部署需重新预览+应用）
    let Some(skill) = req.body.get("skill").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 skill" }));
    };
    let execute = req
        .body
        .get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !execute {
        return match crate::personal_library::check_update(&state.data_root, skill) {
            Ok(st) => Response::json(200, json!({ "executed": false, "status": st })),
            Err(e) => Response::error(400, &e),
        };
    }
    let Some(preview_id) = req.body["preview_id"].as_str().filter(|id| !id.is_empty()) else {
        return Response::json(409, json!({ "error": "请先检查 Skill 更新，再确认该版本" }));
    };
    match crate::personal_library::update_execute_checked(&state.data_root, skill, preview_id) {
        Ok(r) => Response::json(200, json!({ "executed": true, "result": r })),
        Err(e) => Response::error(409, &e),
    }
}

/// `GET /api/library/sources`
pub(super) fn get_library_sources(state: &Arc<ServerState>, _req: &Request) -> Response {
    // AIL-063：来源身份/版本清单
    if let Err(e) = crate::personal_library::recover_updates(&state.data_root) {
        return Response::error(422, &e);
    }
    let lib = crate::personal_library::library_root(&state.data_root);
    let mut items = Vec::new();
    let skills_dir = lib.join("resources/skills");
    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        for e in entries.flatten() {
            let d = e.path();
            if !d.is_dir() || e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let meta = crate::skill_source::read_meta(&d);
            items.push(json!({
                "skill": e.file_name().to_string_lossy(),
                "source": meta,
                "legacy": meta.is_none(),
            }));
        }
    }
    Response::json(200, json!({ "items": items }))
}

/// `POST /api/library/delete`
pub(super) fn post_library_delete(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(id) = req.body.get("id").and_then(|v| v.as_str()) else {
        return Response::json(400, json!({ "error": "需要 id" }));
    };
    let execute = req
        .body
        .get("execute")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if execute {
        match crate::personal_library::delete_execute(&state.data_root, id) {
            Ok(()) => Response::json(200, json!({ "deleted": true, "id": id })),
            Err(e) => Response::error(400, &e),
        }
    } else {
        match crate::personal_library::delete_preview(&state.data_root, id) {
            Ok(v) => Response::json(200, v),
            Err(e) => Response::error(400, &e),
        }
    }
}

/// `POST /api/library/resource`
pub(super) fn post_library_resource(state: &Arc<ServerState>, req: &Request) -> Response {
    let get = |key| req.body.get(key).and_then(|v| v.as_str()).unwrap_or("");
    match crate::personal_library::create_definition(
        &state.data_root,
        get("kind"),
        get("name"),
        get("description"),
        get("body"),
    ) {
        Ok(id) => Response::json(200, json!({"id":id})),
        Err(e) => Response::error(400, &e),
    }
}
