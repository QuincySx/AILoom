//! 搜索添加/移除的真实 HTTP 与文件写入回归；全局目录只在隔离子进程中使用。
mod common;
use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn request(server: &ConsoleServer, method: &str, path: &str, body: Value) -> (u16, Value) {
    let body = body.to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(stream,"{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{SESSION_HEADER}: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",server.port,server.token,body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (
        status,
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap(),
    )
}

#[test]
fn global_add_remove_applies_only_requested_skill_and_preserves_conflicts() {
    let child_root = std::env::var_os("AILOOM_GLOBAL_HTTP_CHILD");
    if child_root.is_none() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::env::var_os("AILOOM_GLOBAL_UI_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| temp.path().to_path_buf());
        std::fs::create_dir_all(root.join("home")).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "global_add_remove_applies_only_requested_skill_and_preserves_conflicts",
                "--nocapture",
            ])
            .envs(common::isolated_child_env(&root))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("AILOOM_GLOBAL_HTTP_CHILD", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = PathBuf::from(child_root.unwrap());
    let data = common::isolated_data_root(&root);
    for name in ["alpha", "beta", "gamma"] {
        let dir = root.join("fixtures").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"),format!("---\nname: {name}\ndescription: {name} 搜索添加测试技能\n---\nsource content {name}\n")).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_ailoom"))
            .args([
                "--json",
                "--data-root",
                data.to_str().unwrap(),
                "library",
                "--action",
                "import",
                "--dir",
                dir.to_str().unwrap(),
                "--execute",
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    // 两种 Git 传输方式保留各自地址，但全局候选使用同一个仓库身份。
    for (name, url) in [
        ("beta", "https://github.com/example/catalog.git"),
        ("gamma", "git@github.com:example/catalog"),
    ] {
        let dir = ailoom::personal_library::library_root(&data)
            .join("resources/skills")
            .join(name);
        let mut meta = ailoom::skill_source::read_meta(&dir).unwrap();
        meta.repo_url = Some(url.to_string());
        ailoom::skill_source::write_meta(&dir, &meta).unwrap();
    }
    let server = ConsoleServer::start(&ConsoleOptions {
        port: 0,
        data_root: data.clone(),
        open_browser: false,
    })
    .unwrap();
    let id = |name: &str| format!("personal/skill/personal/{name}");
    let select = |name: &str, enabled: bool| {
        let (code, value) = request(
            &server,
            "POST",
            "/api/global/select",
            json!({"skill":id(name),"enabled":enabled}),
        );
        assert_eq!(code, 200, "{value}");
    };
    let apply = |name: &str| {
        let (code, value) = request(
            &server,
            "POST",
            "/api/global/apply",
            json!({"skill":id(name)}),
        );
        assert_eq!(code, 200, "{value}");
        value
    };
    select("alpha", true);
    select("beta", true);
    let copied =
        ailoom::personal_library::library_root(&data).join("resources/skills/alpha/SKILL.md");
    let original = std::fs::read(&copied).unwrap();
    apply("alpha");
    for target in [".claude/skills", ".agents/skills"] {
        assert!(root.join("home").join(target).join("alpha").exists());
        assert!(!root.join("home").join(target).join("beta").exists());
    }
    let status = request(&server, "GET", "/api/global/skills", json!({})).1;
    for name in ["beta", "gamma"] {
        let candidate = status["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap();
        assert_eq!(candidate["repository_key"], "github.com/example/catalog");
        assert!(candidate["repository_url"]
            .as_str()
            .unwrap()
            .contains("example/catalog"));
    }
    assert!(status["pending"].as_u64().unwrap() > 0);
    assert!(status["deployed"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["resource_id"] == id("alpha")));
    select("alpha", false);
    apply("alpha");
    for target in [".claude/skills", ".agents/skills"] {
        assert!(root
            .join("home")
            .join(target)
            .join("alpha")
            .symlink_metadata()
            .is_err());
        assert!(!root.join("home").join(target).join("beta").exists());
    }
    assert_eq!(std::fs::read(&copied).unwrap(), original);
    assert!(
        request(&server, "GET", "/api/global/skills", json!({})).1["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "alpha" && s["global"] == false)
    );
    let foreign = root.join("home/.agents/skills/gamma");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(foreign.join("SKILL.md"), "foreign original").unwrap();
    select("gamma", true);
    let result = apply("gamma");
    assert_eq!(result["skipped_conflicts"], json!([".agents/skills/gamma"]));
    assert_eq!(
        std::fs::read_to_string(foreign.join("SKILL.md")).unwrap(),
        "foreign original"
    );
    assert!(!root.join("home/.agents/skills/beta").exists());
    assert_eq!(
        request(&server, "POST", "/api/global/apply", json!({"skill":12})).0,
        400
    );
    // 全量和单项应用共用外层操作锁，准备之前就拒绝并发写入。
    let operation = ailoom::sync::lock::SyncLock::acquire(
        &data.join("global/locks/operation"),
        "test-operation",
    )
    .unwrap();
    assert_eq!(
        ailoom::global_skills::sync(&data).unwrap_err().code,
        "E4003"
    );
    assert_eq!(
        ailoom::global_skills::sync_skill(&data, &id("alpha"))
            .unwrap_err()
            .code,
        "E4003"
    );
    drop(operation);
    for _ in 0..3 {
        select("alpha", true);
        select("beta", true);
        let barrier = std::sync::Barrier::new(2);
        let responses = std::thread::scope(|scope| {
            let handles: Vec<_> = ["alpha", "beta"]
                .into_iter()
                .map(|name| {
                    let barrier = &barrier;
                    let server = &server;
                    scope.spawn(move || {
                        barrier.wait();
                        (
                            name,
                            request(
                                server,
                                "POST",
                                "/api/global/apply",
                                json!({"skill":format!("personal/skill/personal/{name}")}),
                            ),
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        for (name, (code, value)) in responses {
            if code != 200 {
                assert!(value.to_string().contains("E4003"), "{code}: {value}");
                apply(name);
            }
        }
        let status = request(&server, "GET", "/api/global/skills", json!({})).1;
        for name in ["alpha", "beta"] {
            assert_eq!(
                status["deployed"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|d| d["resource_id"] == id(name))
                    .count(),
                2,
                "托管清单必须保留两项: {status}"
            );
            select(name, false);
            apply(name);
            assert!(root
                .join("home/.agents/skills")
                .join(name)
                .symlink_metadata()
                .is_err());
        }
    }
    // 不同 Skill 可绑定不同目录；修改一个选择不会顺带改动另一个。
    let choose = |name: &str, targets: &[&str]| {
        let (code, value) = request(
            &server,
            "POST",
            "/api/global/select",
            json!({"skill":id(name),"enabled":true,"targets":targets}),
        );
        assert_eq!(code, 200, "{value}");
    };
    choose("alpha", &["claude"]);
    choose("beta", &["agents"]);
    apply("alpha");
    apply("beta");
    assert!(root.join("home/.claude/skills/alpha").exists());
    assert!(!root.join("home/.agents/skills/alpha").exists());
    assert!(root.join("home/.agents/skills/beta").exists());
    assert!(!root.join("home/.claude/skills/beta").exists());
    let status = request(&server, "GET", "/api/global/skills", json!({})).1;
    assert_eq!(status["skill_targets"][id("alpha")], json!(["claude"]));
    assert_eq!(status["skill_targets"][id("beta")], json!(["agents"]));
    // 空、未知、类型错误、停用携带目标及陈旧 revision 全部拒绝，profile 字节不变。
    let profile_path = ailoom::profile::PersonalProfile::profile_path(&data);
    let profile_before = std::fs::read(&profile_path).unwrap();
    for targets in [json!([]), json!(["invalid"]), json!([12]), json!("agents")] {
        assert_eq!(
            request(
                &server,
                "POST",
                "/api/global/select",
                json!({"skill":id("alpha"),"enabled":true,"targets":targets})
            )
            .0,
            400
        );
    }
    assert_eq!(
        request(
            &server,
            "POST",
            "/api/global/select",
            json!({"skill":id("alpha"),"enabled":false,"targets":["claude"]})
        )
        .0,
        400
    );
    assert_eq!(
        request(
            &server,
            "POST",
            "/api/global/select",
            json!({"skill":id("alpha"),"enabled":true,"targets":["agents"],"base_revision":0})
        )
        .0,
        409
    );
    assert_eq!(std::fs::read(&profile_path).unwrap(), profile_before);
    choose("alpha", &["agents"]);
    apply("alpha");
    assert!(root
        .join("home/.claude/skills/alpha")
        .symlink_metadata()
        .is_err());
    assert!(root.join("home/.agents/skills/alpha").exists());
    assert!(root.join("home/.agents/skills/beta").exists());
    // 显式绑定不被旧的默认开关覆盖。
    assert_eq!(
        request(
            &server,
            "POST",
            "/api/global/select",
            json!({"target":"agents","enabled":false})
        )
        .0,
        200
    );
    apply("alpha");
    assert!(root.join("home/.agents/skills/alpha").exists());
    select("alpha", false);
    apply("alpha");
    let profile = ailoom::profile::PersonalProfile::load_or_default(&data).unwrap();
    assert!(!profile.global.skill_targets.contains_key(&id("alpha")));
    assert!(root
        .join("home/.agents/skills/alpha")
        .symlink_metadata()
        .is_err());
    assert!(root.join("home/.agents/skills/beta").exists());
    // 不再选择的冲突目录里的外部文件仍然保留。
    choose("gamma", &["claude"]);
    apply("gamma");
    let gamma_md =
        ailoom::personal_library::library_root(&data).join("resources/skills/gamma/SKILL.md");
    let gamma_raw = std::fs::read_to_string(&gamma_md).unwrap();
    std::fs::write(
        &gamma_md,
        gamma_raw.replace("name: gamma\n", "name: gamma\ntargets: [claude]\n"),
    )
    .unwrap();
    let status = request(&server, "GET", "/api/global/skills", json!({})).1;
    assert_eq!(
        status["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id("gamma"))
            .unwrap()["available_targets"],
        json!(["claude"])
    );
    let profile_before = std::fs::read(&profile_path).unwrap();
    assert_eq!(
        request(
            &server,
            "POST",
            "/api/global/select",
            json!({"skill":id("gamma"),"enabled":true,"targets":["claude","agents"]})
        )
        .0,
        400
    );
    assert_eq!(std::fs::read(&profile_path).unwrap(), profile_before);
    assert_eq!(
        std::fs::read_to_string(foreign.join("SKILL.md")).unwrap(),
        "foreign original"
    );
    assert_eq!(std::fs::read(&copied).unwrap(), original);
    select("beta", false);
    apply("beta");
    assert_eq!(
        request(
            &server,
            "POST",
            "/api/global/select",
            json!({"target":"agents","enabled":true})
        )
        .0,
        200
    );
    select("beta", true); // 留下另一项待应用，用于浏览器验证单项操作不会顺带部署。
                          // 仅手工浏览器验收时保持隔离服务；正常 CI 不等待。
    if std::env::var_os("AILOOM_GLOBAL_UI_ROOT").is_some() {
        std::fs::write(
            root.join("ui-server.json"),
            json!({"port":server.port}).to_string(),
        )
        .unwrap();
        for _ in 0..960 {
            if root.join("stop-ui").exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    server.shutdown();
    server.join();
}
