//! AIL-112 回归：未托管 Skill 删除只接受本项目扫描结果中的条目，
//! 令牌绑定项目根，指纹覆盖文件内容与链接目标，链接摘除保留恢复记录。

use ailoom::console::{ConsoleOptions, ConsoleServer, SESSION_HEADER};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

fn post(port: u16, token: &str, path: &str, body: Value) -> (u16, Value) {
    let body_text = body.to_string();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n{SESSION_HEADER}: {token}\r\n\r\n{body_text}",
        body_text.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
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

fn skill(dir: &Path, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: d\n---\n{body}\n"),
    )
    .unwrap();
}

#[test]
fn ail112_delete_is_bound_to_project_scan_and_content() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp = tmp.path().canonicalize().unwrap();
    // 隔离 Store 根：托管判定只看这里，不触碰真实用户目录。
    std::env::set_var("XDG_DATA_HOME", tmp.join("xdg"));
    let store = ailoom::paths::resolve_store_root().unwrap();
    std::fs::create_dir_all(&store).unwrap();
    let store = store.canonicalize().unwrap();

    let a = tmp.join("proj-a");
    let b = tmp.join("proj-b");
    let local = a.join(".claude/skills/local-one");
    skill(&local, "aaaa");
    skill(&b.join(".claude/skills/other"), "x");
    // 不在宿主 Skill 目录中的 SKILL.md：不是扫描对象，不能删。
    let stray = a.join("docs/stray");
    skill(&stray, "x");
    // 托管实体链接（指向 Store）。
    skill(&store.join("skills/managed-one"), "m");
    std::os::unix::fs::symlink(
        store.join("skills/managed-one"),
        a.join(".claude/skills/managed-one"),
    )
    .unwrap();
    // 外部链接。
    let ext_a = tmp.join("outside/ext-a");
    let ext_b = tmp.join("outside/ext-b");
    skill(&ext_a, "e");
    skill(&ext_b, "e");
    let link = a.join(".claude/skills/linked");
    std::os::unix::fs::symlink(&ext_a, &link).unwrap();

    let server = ConsoleServer::start(&ConsoleOptions {
        port: 17960,
        data_root: tmp.join("data"),
        open_browser: false,
    })
    .unwrap();
    let (port, tok) = (server.port, server.token.clone());
    for root in [&a, &b] {
        assert_eq!(
            post(port, &tok, "/api/fs/approve", json!({ "path": root })).0,
            200
        );
    }
    let preview = |root: &Path, path: &Path| {
        post(
            port,
            &tok,
            "/api/project/delete-skill",
            json!({ "root": root, "path": path, "execute": false }),
        )
    };
    let execute = |root: &Path, token: &str, name: &str| {
        post(
            port,
            &tok,
            "/api/project/delete-skill",
            json!({ "root": root, "token": token, "name": name, "execute": true }),
        )
    };

    // 缺 root、非扫描对象、托管实体均拒绝。
    assert_eq!(
        post(
            port,
            &tok,
            "/api/project/delete-skill",
            json!({ "path": local, "execute": false })
        )
        .0,
        400
    );
    assert_eq!(preview(&a, &stray).0, 403, "扫描范围外的目录不能删");
    assert_eq!(
        preview(&a, &store.join("skills/managed-one")).0,
        403,
        "Store 实体不能删"
    );
    assert_eq!(
        preview(&a, &a.join(".claude/skills/managed-one")).0,
        403,
        "托管部署走移除流程"
    );
    assert_eq!(preview(&b, &local).0, 403, "其他项目的目录不能借道删除");

    // 跨项目复用令牌：作废。
    let (code, p) = preview(&a, &local);
    assert_eq!(code, 200, "{p}");
    assert_eq!(
        execute(&b, p["token"].as_str().unwrap(), "local-one").0,
        403
    );
    assert!(local.join("SKILL.md").is_file());

    // 等长改写内容：指纹不一致。
    let (_, p) = preview(&a, &local);
    let md = local.join("SKILL.md");
    let changed = std::fs::read_to_string(&md)
        .unwrap()
        .replace("aaaa", "bbbb");
    std::fs::write(&md, changed).unwrap();
    assert_eq!(
        execute(&a, p["token"].as_str().unwrap(), "local-one").0,
        409
    );
    assert!(md.is_file());

    // 正常删除：归档可恢复。
    let (_, p) = preview(&a, &local);
    let (code, r) = execute(&a, p["token"].as_str().unwrap(), "local-one");
    assert_eq!(code, 200, "{r}");
    assert!(!local.exists());
    assert!(Path::new(r["archived_to"].as_str().unwrap())
        .join("SKILL.md")
        .is_file());

    // 链接中途改指：拒绝；之后正常摘除并记录原目标。
    let (code, p) = preview(&a, &link);
    assert_eq!(code, 200, "{p}");
    assert_eq!(p["is_symlink"], true);
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&ext_b, &link).unwrap();
    assert_eq!(execute(&a, p["token"].as_str().unwrap(), "linked").0, 409);
    let (_, p) = preview(&a, &link);
    let (code, r) = execute(&a, p["token"].as_str().unwrap(), "linked");
    assert_eq!(code, 200, "{r}");
    assert!(std::fs::symlink_metadata(&link).is_err(), "链接已摘除");
    assert!(ext_b.join("SKILL.md").is_file(), "链接目标不动");
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(r["archived_to"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(record["link_target"], ext_b.display().to_string());
    assert_eq!(record["schema_version"], 1);

    server.shutdown();
    server.join();
}
