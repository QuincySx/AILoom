//! AIL-133：此前没有任何测试引用的 25 条控制台路由的冒烟测试。
//! 每条路由至少验证：不返回 5xx、响应是 JSON 对象、失败时带 `error` 文本；能给出合法参数的路由验证 200。
//! `POST /api/fs/pick-directory` 会弹出系统文件夹选择框，只验证未带会话令牌时被拒绝。

mod common;

use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn call(
    port: u16,
    token: Option<&str>,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let body_text = body.map(|b| b.to_string()).unwrap_or_default();
    let auth = token
        .map(|t| format!("{SESSION_HEADER}: {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n{auth}\r\n{body_text}",
        body_text.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    let status = buf
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let idx = buf.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    (
        status,
        serde_json::from_str(&buf[idx..]).unwrap_or(Value::Null),
    )
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[test]
fn knowledge_organization_api_authorizes_root_and_only_accepts_known_runners() {
    common::isolate_in_process_roots();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    let data = tmp.path().join("data");
    std::fs::create_dir(&root).unwrap();
    ailoom::knowledge::location::run(
        &data,
        &ailoom::knowledge::location::Request {
            action: "init".into(),
            root: root.clone(),
            path: Some(tmp.path().join("knowledge")),
            ..Default::default()
        },
    )
    .unwrap();
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 0,
        data_root: data.clone(),
        open_browser: false,
    })
    .unwrap();
    let (port, token) = (server.port, server.token.as_str());
    let status_body = json!({"root":root,"action":"organization-status"});
    assert_eq!(
        call(
            port,
            None,
            "POST",
            "/api/knowledge",
            Some(status_body.clone())
        )
        .0,
        401
    );
    assert_eq!(
        call(
            port,
            Some(token),
            "POST",
            "/api/knowledge",
            Some(status_body.clone())
        )
        .0,
        403
    );
    assert_eq!(
        call(
            port,
            Some(token),
            "POST",
            "/api/fs/approve",
            Some(json!({"path":root}))
        )
        .0,
        200
    );
    let (status, result) = call(
        port,
        Some(token),
        "POST",
        "/api/knowledge",
        Some(status_body.clone()),
    );
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["source_count"], 0);
    assert!(result["report"].is_null());
    let runners = result["runners"].as_array().unwrap();
    assert!(runners.iter().any(|r| r["id"] == "codex"));
    for runner in runners {
        assert_eq!(
            runner["available"].as_bool().unwrap(),
            runner["executable"].is_string()
        );
    }
    // 无知识时不启动真实 CLI，未接入的命令或超长任务说明也不能启动进程。
    for body in [
        json!({"root":root,"action":"organize","runner":"codex"}),
        json!({"root":root,"action":"organize","runner":"/bin/sh"}),
        json!({"root":root,"action":"organize","runner":"codex; echo injected"}),
        json!({"root":root,"action":"organize","focus":"长".repeat(2001)}),
    ] {
        let (status, result) = call(port, Some(token), "POST", "/api/knowledge", Some(body));
        assert_eq!(status, 400, "{result}");
        assert!(result["error"].is_string());
    }
    assert_eq!(
        call(port, Some(token), "GET", "/api/jobs", None).1["jobs"],
        json!([])
    );
    let note = tmp.path().join("note.md");
    std::fs::write(&note, "# 排障经验\n检查迁移行数").unwrap();
    ailoom::knowledge::location::save(&data, &root, &note, "migration").unwrap();
    let (status, result) = call(
        port,
        Some(token),
        "POST",
        "/api/knowledge",
        Some(status_body.clone()),
    );
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["source_count"], 1, "恢复配置不计入经验数量");
    let report = std::path::Path::new(result["report_path"].as_str().unwrap());
    std::fs::create_dir_all(report.parent().unwrap()).unwrap();
    std::fs::write(report, "# 已有维护建议\n保留这份结果").unwrap();
    let (status, result) = call(
        port,
        Some(token),
        "POST",
        "/api/knowledge",
        Some(status_body),
    );
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["source_count"], 1, "之前的整理报告不计入原始知识");
    assert!(result["report"].as_str().unwrap().contains("保留这份结果"));
    server.shutdown();
    server.join();
}

#[test]
fn knowledge_ingestion_api_requires_source_approval_and_binds_the_preview() {
    common::isolate_in_process_roots();
    let tmp = tempfile::tempdir().unwrap();
    let root = common::make_business_repo(tmp.path(), "ingestion-main");
    let wt = tmp.path().join("ingestion-feature");
    ailoom::gitx::git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "ingestion-feature",
            wt.to_str().unwrap(),
        ],
    )
    .unwrap();
    std::fs::write(wt.join("lesson.md"), "# Collected\nbranch-specific lesson").unwrap();
    let data = tmp.path().join("data");
    ailoom::knowledge::location::run(
        &data,
        &ailoom::knowledge::location::Request {
            action: "init".into(),
            root: root.clone(),
            path: Some(tmp.path().join("knowledge")),
            ..Default::default()
        },
    )
    .unwrap();
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 0,
        data_root: data,
        open_browser: false,
    })
    .unwrap();
    let (port, token) = (server.port, server.token.as_str());
    let post = |body| call(port, Some(token), "POST", "/api/knowledge", Some(body));
    let state_body = json!({"root":root,"action":"ingestion-status"});
    assert_eq!(
        call(
            port,
            None,
            "POST",
            "/api/knowledge",
            Some(state_body.clone())
        )
        .0,
        401
    );
    assert_eq!(post(state_body.clone()).0, 403);
    assert_eq!(
        call(
            port,
            Some(token),
            "POST",
            "/api/fs/approve",
            Some(json!({"path":root}))
        )
        .0,
        200
    );
    let (status, state) = post(state_body);
    assert_eq!(status, 200, "{state}");
    let canonical_wt = wt.canonicalize().unwrap();
    let id = state["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["path"].as_str() == canonical_wt.to_str())
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let preview_body = json!({"root":root,"action":"ingestion-preview","source_id":id});
    assert_eq!(
        post(preview_body.clone()).0,
        403,
        "another Worktree needs explicit directory approval"
    );
    assert_eq!(
        call(
            port,
            Some(token),
            "POST",
            "/api/fs/approve",
            Some(json!({"path":wt}))
        )
        .0,
        200
    );
    let (status, preview) = post(preview_body.clone());
    assert_eq!(status, 200, "{preview}");
    assert!(preview["documents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["path"] == "lesson.md"));
    let apply = |expected: &Value, documents: Value| {
        post(
            json!({"root":root,"action":"ingestion-apply","source_id":id,"expected":expected,"documents":documents}),
        )
    };
    assert_eq!(apply(&preview["expected"], json!(["../outside.md"])).0, 409);
    assert_eq!(
        post(json!({"root":root,"action":"ingestion-preview","source_id":"foreign-project"})).0,
        400
    );
    std::fs::write(wt.join("lesson.md"), "# Collected\nupdated before save").unwrap();
    assert_eq!(apply(&preview["expected"], json!(["lesson.md"])).0, 409);
    let (_, preview) = post(preview_body.clone());
    let (status, saved) = apply(&preview["expected"], json!(["lesson.md"]));
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["new_documents"], 1);
    let (_, preview) = post(preview_body);
    let (status, saved) = apply(&preview["expected"], json!(["lesson.md"]));
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["new_documents"], 0);
    assert_eq!(saved["revision_count"], 1);
    let (status, organized) = post(json!({"root":root,"action":"organization-status"}));
    assert_eq!(status, 200, "{organized}");
    assert_eq!(organized["source_count"], 1);
    server.shutdown();
    server.join();
}

#[test]
fn prune_missing_worktrees_preserves_configuration_and_survives_rediscovery() {
    common::isolate_in_process_roots();
    let tmp = tempfile::tempdir().unwrap();
    let main = common::make_business_repo(tmp.path(), "main");
    let git = |args: &[&str]| ailoom::gitx::git(&main, args).unwrap();
    let removed = tmp.path().join("removed");
    let stale = tmp.path().join("stale");
    let restored = tmp.path().join("restored");
    for (path, branch) in [
        (&removed, "removed"),
        (&stale, "stale"),
        (&restored, "restored"),
    ] {
        git(&[
            "worktree",
            "add",
            "-q",
            path.to_str().unwrap(),
            "-b",
            branch,
        ]);
    }
    let data = tmp.path().join("data");
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 0,
        data_root: data.clone(),
        open_browser: false,
    })
    .unwrap();
    let (port, tok) = (server.port, server.token.as_str());
    assert_eq!(
        call(
            port,
            Some(tok),
            "POST",
            "/api/fs/approve",
            Some(json!({"path":main}))
        )
        .0,
        200
    );
    let discover = || {
        let (status, result) = call(
            port,
            Some(tok),
            "POST",
            "/api/repo/discover",
            Some(json!({"path":main})),
        );
        assert_eq!(status, 200, "{result}");
        result
    };
    let discovered = discover();
    let repo_id = discovered["repo_id"].as_str().unwrap();
    let registry = || ailoom::repo_registry::load_or_create_by_id(&data, repo_id).unwrap();
    let original = registry();
    let removed_ids: Vec<String> = original
        .worktrees
        .iter()
        .filter(|(_, wt)| {
            wt.path == removed.canonicalize().unwrap() || wt.path == stale.canonicalize().unwrap()
        })
        .map(|(id, _)| id.clone())
        .collect();
    assert_eq!(removed_ids.len(), 2);
    let profile = ailoom::profile::PersonalProfile::profile_path(&data);
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    let profile_text = format!(
        "schema_version = 1\n[repos.\"{repo_id}\".worktrees.\"{}\"]\ninherit_resources = false\n",
        removed_ids[0]
    );
    std::fs::write(&profile, &profile_text).unwrap();
    let instructions =
        ailoom::personal_instructions::entry_path(&data, repo_id, Some(&removed_ids[0]));
    std::fs::create_dir_all(instructions.parent().unwrap()).unwrap();
    std::fs::write(&instructions, "保留失联目录的个人指令").unwrap();

    git(&["worktree", "remove", removed.to_str().unwrap()]);
    std::fs::remove_dir_all(&stale).unwrap();
    let moved = tmp.path().join("temporarily-moved");
    std::fs::rename(&restored, &moved).unwrap();
    discover();
    assert_eq!(
        registry()
            .worktrees
            .values()
            .filter(|wt| wt.status == ailoom::repo_registry::WorktreeStatus::Missing)
            .count(),
        3
    );
    // 刷新后路径恢复，清理必须重新检查而不是仅信任 missing 标记。
    std::fs::rename(&moved, &restored).unwrap();
    let git_before = git(&["worktree", "list", "--porcelain"]);
    let body = Some(json!({"repo_id":repo_id}));
    assert_eq!(
        call(port, None, "POST", "/api/repo/prune", body.clone()).0,
        401
    );
    assert_eq!(call(port, Some(tok), "GET", "/api/repo/prune", None).0, 405);
    assert_eq!(
        call(port, Some(tok), "POST", "/api/repo/prune", Some(json!({}))).0,
        400
    );
    for id in ["unknown", "../main/.git"] {
        assert_eq!(
            call(
                port,
                Some(tok),
                "POST",
                "/api/repo/prune",
                Some(json!({"repo_id":id}))
            )
            .0,
            404
        );
    }
    let (status, result) = call(port, Some(tok), "POST", "/api/repo/prune", body.clone());
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["removed"], json!(removed_ids));
    assert_eq!(result["worktrees"].as_object().unwrap().len(), 2);
    assert_eq!(registry().worktrees.len(), 2, "清理结果必须保存到磁盘");
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), profile_text);
    assert_eq!(
        std::fs::read_to_string(&instructions).unwrap(),
        "保留失联目录的个人指令"
    );
    assert_eq!(
        git(&["worktree", "list", "--porcelain"]),
        git_before,
        "不修改 Git Worktree 元数据"
    );
    assert!(main.is_dir() && restored.is_dir());
    discover();
    assert_eq!(
        registry().worktrees.len(),
        2,
        "Git 中残留的失联路径不能重新登记"
    );
    let (status, result) = call(port, Some(tok), "POST", "/api/repo/prune", body.clone());
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["removed"], json!([]), "重复清理应为空操作");

    // 所有工作目录都失联时，也能不依赖可用工作目录清理登记并保留项目。
    let discovery = ailoom::repo_registry::discover_repo(&main).unwrap();
    std::fs::rename(&main, tmp.path().join("main-gone")).unwrap();
    std::fs::rename(&restored, &moved).unwrap();
    let mut reg = registry();
    reg.refresh_worktrees(&discovery, "missing");
    reg.save(&data).unwrap();
    let (status, result) = call(port, Some(tok), "POST", "/api/repo/prune", body);
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["removed"].as_array().unwrap().len(), 2);
    assert!(registry().worktrees.is_empty());
    let (status, state) = call(port, Some(tok), "GET", "/api/state", None);
    assert_eq!(status, 200, "{state}");
    assert!(state["repos"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["repo_id"] == repo_id));
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), profile_text);
    assert!(instructions.is_file());
    server.shutdown();
    server.join();
}

#[test]
fn previously_untested_routes_respond_with_json_and_no_server_errors() {
    common::isolate_in_process_roots();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir_all(repo.join(".claude/skills/local")).unwrap();
    std::fs::write(
        repo.join(".claude/skills/local/SKILL.md"),
        "---\nname: local\ndescription: d\n---\nbody\n",
    )
    .unwrap();
    std::fs::write(repo.join("notes.md"), "hello").unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());

    let server = ConsoleServer::start(&ConsoleOptions {
        port: 17990,
        data_root: tmp.path().join("data"),
        open_browser: false,
    })
    .unwrap();
    let (port, tok) = (server.port, server.token.clone());
    let t = Some(tok.as_str());
    assert_eq!(
        call(
            port,
            t,
            "POST",
            "/api/fs/approve",
            Some(json!({ "path": repo }))
        )
        .0,
        200
    );
    let root = enc(repo.to_str().unwrap());

    // 有合法参数时必须成功的只读路由
    let ok_gets = [
        "/api/library/list".to_string(),
        "/api/library/sources".to_string(),
        "/api/migrations/cc-switch/location".to_string(),
        format!("/api/project/scan-skills?root={root}"),
        format!("/api/project/dirs?root={root}"),
        format!("/api/project/discover-directories?root={root}"),
        format!("/api/effective?root={root}"),
        "/api/diagnose".to_string(),
        format!("/api/deploy-status?root={root}"),
        format!(
            "/api/fs/read?path={}",
            enc(repo.join("notes.md").to_str().unwrap())
        ),
    ];
    for path in &ok_gets {
        let (code, v) = call(port, t, "GET", path, None);
        assert!(v.is_object(), "{path}: 非 JSON 对象 {v}");
        assert!(
            code == 200 || (code < 500 && v["error"].is_string()),
            "{path}: {code} {v}"
        );
        if path.starts_with("/api/library/")
            || path.contains("scan-skills")
            || path.contains("fs/read")
        {
            assert_eq!(code, 200, "{path}: {v}");
        }
    }
    let (_, scan) = call(
        port,
        t,
        "GET",
        &format!("/api/project/scan-skills?root={root}"),
        None,
    );
    assert_eq!(scan["items"][0]["management"], "unmanaged", "{scan}");

    // 缺参数 / 越界参数：结构化 4xx，不 panic
    let bad_gets = [
        "/api/resources/mcp-detail",
        "/api/resources/mcp-detail?id=nope/mcp/x/y",
        "/api/fs/read?path=/etc/hosts",
        "/api/project/scan-skills",
        "/api/effective",
    ];
    for path in bad_gets {
        let (code, v) = call(port, t, "GET", path, None);
        assert!(
            (400..500).contains(&code),
            "{path}: 期望 4xx，实际 {code} {v}"
        );
        assert!(v["error"].is_string(), "{path}: {v}");
    }

    // 写路由：空请求体不导致 5xx；「检查全部」类动作可返回 200，其余被校验拒绝并带 error
    let posts = [
        "/api/collections/check",
        "/api/collections/remove",
        "/api/collections/update",
        "/api/library/check-update",
        "/api/library/delete",
        "/api/library/import-entry",
        "/api/library/import-git",
        "/api/library/update",
        "/api/profile/scope",
        "/api/repo/prune",
        "/api/repo/relink",
        "/api/workflows/bind",
        "/api/workflows/record-input",
        "/api/workflows/rename",
        "/api/workflows/reviewed",
    ];
    for path in posts {
        let (code, v) = call(port, t, "POST", path, Some(json!({})));
        assert!(v.is_object(), "{path}: 非 JSON 对象 {v}");
        assert!(
            code < 300 || ((400..500).contains(&code) && v["error"].is_string()),
            "{path}: {code} {v}"
        );
        // 写路由都要求会话令牌
        assert_eq!(
            call(port, None, "POST", path, Some(json!({}))).0,
            401,
            "{path}"
        );
    }
    assert_eq!(
        call(
            port,
            None,
            "POST",
            "/api/fs/pick-directory",
            Some(json!({}))
        )
        .0,
        401
    );
    // 撤回中途失败的同步：没有恢复点时返回空结果；缺 root 与未带令牌被拒绝
    let (code, v) = call(
        port,
        t,
        "POST",
        "/api/project/recover",
        Some(json!({ "root": repo })),
    );
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["recovered"], json!([]));
    assert_eq!(
        call(port, t, "POST", "/api/project/recover", Some(json!({}))).0,
        400
    );
    assert_eq!(
        call(
            port,
            None,
            "POST",
            "/api/project/recover",
            Some(json!({ "root": repo }))
        )
        .0,
        401
    );

    server.shutdown();
    server.join();
}

#[test]
fn unavailable_team_source_can_be_rebound_without_changing_deployed_files_or_choices() {
    common::isolate_in_process_roots();
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let repo = base.join("project");
    let old = base.join("old-source");
    let replacement = base.join("new-source");
    let invalid = base.join("invalid-source");
    let data = base.join("data");
    for path in [&repo, &old, &replacement, &invalid] {
        std::fs::create_dir_all(path).unwrap();
    }
    ailoom::gitx::git_init(&repo, false).unwrap();
    std::fs::write(old.join("ailoom.toml"), common::MANIFEST_TOML).unwrap();
    std::fs::write(
        replacement.join("ailoom.toml"),
        common::MANIFEST_TOML.replace("example-team", "replacement-team"),
    )
    .unwrap();
    let initial = ailoom::commands::init::run(
        &ailoom::commands::init::InitArgs {
            no_builtin: true,
            url: None,
            ref_: None,
            name: None,
            local_path: Some("../old-source".into()),
            projects: vec!["a".into()],
            roles: vec!["dev".into()],
            targets: vec!["claude".into()],
            refresh: false,
            root: Some(repo.clone()),
        },
        true,
        Some(&data),
    )
    .unwrap();
    // Reproduce the missing-directory state while retaining the last binding and lock.
    std::fs::rename(&old, base.join("moved-source")).unwrap();
    let deployed = repo.join(".claude/skills/keep/SKILL.md");
    std::fs::create_dir_all(deployed.parent().unwrap()).unwrap();
    std::fs::write(&deployed, "keep deployed skill\n").unwrap();
    let profile = data.join("profile/profile.toml");
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(&profile, "schema_version = 1\n").unwrap();
    let declaration = repo.join(".ailoom/project.toml");
    let binding = std::path::Path::new(initial["binding_path"].as_str().unwrap());
    let lock = std::path::Path::new(initial["lock_path"].as_str().unwrap());
    let preserved =
        [&declaration, binding, lock, &deployed, &profile].map(|p| (p, std::fs::read(p).unwrap()));

    let server = ConsoleServer::start(&ConsoleOptions {
        port: 0,
        data_root: data.clone(),
        open_browser: false,
    })
    .unwrap();
    let token = Some(server.token.as_str());
    for path in [&repo, &invalid] {
        assert_eq!(
            call(
                server.port,
                token,
                "POST",
                "/api/fs/approve",
                Some(json!({"path":path}))
            )
            .0,
            200
        );
    }
    let read = || {
        call(
            server.port,
            token,
            "POST",
            "/api/project/source",
            Some(json!({"root":repo,"action":"read"})),
        )
    };
    let (status, current) = read();
    assert_eq!(status, 200, "{current}");
    assert_eq!(
        current["cache_root"],
        data.join("cache").display().to_string()
    );
    assert_eq!(
        std::path::Path::new(current["source"]["path"].as_str().unwrap()),
        repo.join("../old-source")
    );
    let save = |kind: &str, value: &std::path::Path, base: &Value| json!({"root":repo,"action":"save","type":kind,"value":value,"base_fingerprint":base});
    let request = save("local", &replacement, &current["base_fingerprint"]);
    assert_eq!(
        call(
            server.port,
            None,
            "POST",
            "/api/project/source",
            Some(request.clone())
        )
        .0,
        401
    );
    assert_eq!(
        call(
            server.port,
            token,
            "POST",
            "/api/project/source",
            Some(request.clone())
        )
        .0,
        403
    );
    let (status, failure) = call(
        server.port,
        token,
        "POST",
        "/api/project/source",
        Some(save("local", &invalid, &current["base_fingerprint"])),
    );
    assert_eq!(status, 400, "{failure}");
    for (path, bytes) in &preserved {
        assert_eq!(
            std::fs::read(path).unwrap(),
            *bytes,
            "failed validation changed {}",
            path.display()
        );
    }

    assert_eq!(
        call(
            server.port,
            token,
            "POST",
            "/api/fs/approve",
            Some(json!({"path":replacement}))
        )
        .0,
        200
    );
    let (status, saved) = call(
        server.port,
        token,
        "POST",
        "/api/project/source",
        Some(request.clone()),
    );
    assert_eq!(status, 200, "{saved}");
    let decl = ailoom::config::ProjectDeclaration::load(&declaration)
        .unwrap()
        .unwrap();
    assert_eq!(decl.source.path.as_deref(), Some("../new-source"));
    assert_eq!(decl.projects, ["a"]);
    assert_eq!(decl.roles, ["dev"]);
    assert!(decl.targets.claude && !decl.targets.codex && !decl.builtins);
    assert_eq!(
        ailoom::config::Binding::load(binding)
            .unwrap()
            .unwrap()
            .declaration,
        decl
    );
    let sources = ailoom::source::SourcesLock::load(lock).unwrap().unwrap();
    assert_eq!(
        sources.sources["team"].identity,
        ailoom::source::LocalSource::new(&replacement)
            .unwrap()
            .identity
    );
    assert_eq!(
        call(
            server.port,
            token,
            "POST",
            "/api/project/source",
            Some(request)
        )
        .0,
        409,
        "stale form must not overwrite the new source"
    );

    // Switching to Git with a blank branch must use the new repository's default.
    ailoom::gitx::git_init(&replacement, false).unwrap();
    ailoom::gitx::git_commit_all(&replacement, "team fixture", &["."]).unwrap();
    ailoom::gitx::git(&replacement, &["branch", "-m", "develop"]).unwrap();
    let (_, current) = read();
    let mut default_download = save("git", &replacement, &current["base_fingerprint"]);
    default_download["cache_root"] = json!(data.join("cache"));
    assert!(
        !data.join("cache").exists(),
        "first Git download should create the default directory automatically"
    );
    let (status, saved) = call(
        server.port,
        token,
        "POST",
        "/api/project/source",
        Some(default_download),
    );
    assert_eq!(status, 200, "{saved}");
    let decl = ailoom::config::ProjectDeclaration::load(&declaration)
        .unwrap()
        .unwrap();
    assert_eq!(decl.source.kind, "git");
    assert_eq!(decl.source.ref_.as_deref(), Some("develop"));
    assert!(decl.source.path.is_none());

    // A custom download root persists independently of the shareable declaration.
    let downloads = base.join("downloads");
    assert_eq!(
        call(
            server.port,
            None,
            "POST",
            "/api/fs/approve",
            Some(json!({"path":downloads,"create":true}))
        )
        .0,
        401
    );
    assert!(
        !downloads.exists(),
        "unauthenticated requests must not create directories"
    );
    let (_, current) = read();
    let mut custom = save("git", &replacement, &current["base_fingerprint"]);
    custom["cache_root"] = json!(downloads);
    assert_eq!(
        call(
            server.port,
            token,
            "POST",
            "/api/project/source",
            Some(custom.clone())
        )
        .0,
        403
    );
    assert_eq!(
        call(
            server.port,
            token,
            "POST",
            "/api/fs/approve",
            Some(json!({"path":downloads,"create":true}))
        )
        .0,
        200
    );
    assert!(
        downloads.is_dir(),
        "custom download directories should be created automatically"
    );
    let declaration_before = std::fs::read(&declaration).unwrap();
    let (status, saved) = call(
        server.port,
        token,
        "POST",
        "/api/project/source",
        Some(custom.clone()),
    );
    assert_eq!(status, 200, "{saved}");
    assert!(
        std::path::Path::new(saved["source"]["snapshot_root"].as_str().unwrap())
            .starts_with(&downloads)
    );
    assert_eq!(std::fs::read(&declaration).unwrap(), declaration_before);
    let (_, current) = read();
    assert_eq!(current["cache_root"], json!(downloads));
    assert_eq!(current["default_cache_root"], json!(data.join("cache")));
    assert_eq!(
        call(
            server.port,
            token,
            "POST",
            "/api/project/source",
            Some(custom)
        )
        .0,
        409,
        "changing only the download root must invalidate old forms"
    );

    let ctx = ailoom::appctx::AppContext::discover(Some(&data), &repo, Some(&repo)).unwrap();
    let settings_before = std::fs::read(ctx.source_cache_settings_path()).unwrap();
    let mut bad = save("git", &invalid, &current["base_fingerprint"]);
    bad["cache_root"] = json!(downloads);
    assert_eq!(
        call(server.port, token, "POST", "/api/project/source", Some(bad)).0,
        400
    );
    assert_eq!(
        std::fs::read(ctx.source_cache_settings_path()).unwrap(),
        settings_before,
        "failed fetch must keep the selected download location"
    );

    // CLI refresh follows the saved directory, and offline init reuses its snapshot.
    std::fs::write(
        replacement.join("ailoom.toml"),
        format!("{}\n# newer commit\n", common::MANIFEST_TOML),
    )
    .unwrap();
    ailoom::gitx::git_commit_all(&replacement, "new source revision", &["ailoom.toml"]).unwrap();
    assert!(ailoom::commands::source_refresh::refresh_source_locks(&ctx, &decl).unwrap());
    std::fs::rename(&replacement, base.join("offline-source")).unwrap();
    let offline = ailoom::commands::init::run(
        &ailoom::commands::init::InitArgs {
            no_builtin: false,
            url: None,
            ref_: None,
            name: None,
            local_path: None,
            projects: vec![],
            roles: vec![],
            targets: vec![],
            refresh: false,
            root: Some(repo.clone()),
        },
        true,
        Some(&data),
    )
    .unwrap();
    assert!(
        std::path::Path::new(offline["source"]["snapshot_root"].as_str().unwrap())
            .starts_with(&downloads)
    );
    std::fs::rename(base.join("offline-source"), &replacement).unwrap();

    let (_, current) = read();
    let mut reset = save("git", &replacement, &current["base_fingerprint"]);
    reset["cache_root"] = json!(data.join("cache"));
    let (status, saved) = call(
        server.port,
        token,
        "POST",
        "/api/project/source",
        Some(reset),
    );
    assert_eq!(status, 200, "{saved}");
    assert!(
        std::path::Path::new(saved["source"]["snapshot_root"].as_str().unwrap())
            .starts_with(data.join("cache"))
    );
    let (_, current) = read();
    assert_eq!(current["cache_root"], current["default_cache_root"]);
    assert!(
        downloads.is_dir(),
        "restoring the default must not delete the previous download directory"
    );
    for (path, bytes) in &preserved[3..] {
        assert_eq!(
            std::fs::read(path).unwrap(),
            *bytes,
            "rebinding changed {}",
            path.display()
        );
    }
    server.shutdown();
    server.join();
}

/// 守卫：路由表中的每条 `/api/*` 路径都必须至少被一个测试文件引用。
/// 新增路由时请同时补测试（原生文件选择框等需要 GUI 的路由也至少要有鉴权断言）。
#[test]
fn every_api_route_is_referenced_by_a_test() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(root.join("src/console/mod.rs")).unwrap();
    let start = src
        .find("const KNOWN_PATHS: &[&str] = &[")
        .expect("KNOWN_PATHS");
    let end = start + src[start..].find("];").unwrap();
    let paths: Vec<&str> = src[start..end]
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix('"')
                .and_then(|l| l.strip_suffix("\","))
        })
        .filter(|p| p.starts_with("/api/"))
        .collect();
    assert!(paths.len() > 60, "KNOWN_PATHS 解析失败");
    let mut tests = String::new();
    for entry in std::fs::read_dir(root.join("tests")).unwrap().flatten() {
        let p = entry.path();
        if matches!(p.extension().and_then(|e| e.to_str()), Some("rs" | "mjs")) {
            tests.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
        }
    }
    let missing: Vec<&&str> = paths.iter().filter(|p| !tests.contains(**p)).collect();
    assert!(missing.is_empty(), "没有测试引用的路由: {missing:?}");
}
