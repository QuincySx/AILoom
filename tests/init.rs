//! 项目绑定与 init/status（AIL-005）集成测试。
//! 通过真实二进制驱动，覆盖退出码与 JSON/文本输出约定。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    while path.pop() {
        let p = if cfg!(windows) {
            "ailoom.exe"
        } else {
            "ailoom"
        };
        if path.join(p).exists() {
            return path.join(p);
        }
    }
    panic!("未找到 ailoom 二进制");
}

struct Ctx {
    tmp: tempfile::TempDir,
}

impl Ctx {
    fn new() -> Ctx {
        Ctx {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    fn data_root(&self) -> PathBuf {
        self.tmp.path().join("data-root")
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn init_args(data_root: &Path, url: &str, extra: &[&str]) -> Vec<String> {
    let mut v = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        data_root.to_str().unwrap().to_string(),
        "init".to_string(),
        "--url".to_string(),
        url.to_string(),
    ];
    v.extend(extra.iter().map(|s| s.to_string()));
    v
}

#[test]
fn init_creates_declaration_binding_and_lock() {
    let c = Ctx::new();
    let src = common::make_team_source(&c.tmp.path().join("src"));
    let ws = common::make_business_repo(&c.tmp.path().join("ws"), "biz");
    let url = common::file_url(&src);

    let args = init_args(&c.data_root(), &url, &["--project", "a", "--role", "dev"]);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&ws, &arg_refs);
    assert_eq!(code, 0, "stderr: {stderr}");

    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["projects"], serde_json::json!(["a"]));
    assert_eq!(v["result"]["targets"]["claude"], true);
    assert!(common::declaration_path(&ws).exists());
    assert!(ws.join(".ailoom/machine/sources.lock.json").exists());
    // machine 目录必须自 gitignore
    assert_eq!(
        std::fs::read_to_string(ws.join(".ailoom/machine/.gitignore"))
            .unwrap()
            .trim(),
        "*"
    );

    // status 一致
    let dr = c.data_root().to_str().unwrap().to_string();
    let status_args = vec!["--json", "--data-root", dr.as_str(), "status"];
    let (code, stdout, stderr) = c.run(&ws, &status_args);
    assert_eq!(code, 0, "stderr: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v["result"]["ok"], true,
        "status issues: {}",
        v["result"]["issues"]
    );
    assert_eq!(v["result"]["snapshot"]["available"], true);
}

#[test]
fn repeated_init_is_idempotent() {
    let c = Ctx::new();
    let src = common::make_team_source(&c.tmp.path().join("src"));
    let ws = common::make_business_repo(&c.tmp.path().join("ws"), "biz");
    let url = common::file_url(&src);

    let args = init_args(&c.data_root(), &url, &["--project", "a", "--role", "dev"]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let decl_before = std::fs::read(common::declaration_path(&ws)).unwrap();

    // 重复 init（无参数变化）
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["declaration_changed"], false);
    assert_eq!(v["result"]["lock_changed"], false);
    assert_eq!(
        decl_before,
        std::fs::read(common::declaration_path(&ws)).unwrap(),
        "声明文件无冗余变更"
    );

    // 显式改参数才变化：添加角色 pm
    let args2 = init_args(&c.data_root(), &url, &["--role", "pm"]);
    let refs2: Vec<&str> = args2.iter().map(String::as_str).collect();
    let (code, stdout, _) = c.run(&ws, &refs2);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["declaration_changed"], true);
    // 只改显式参数：projects 保持 a（未显式给 --project）
    assert_eq!(v["result"]["projects"], serde_json::json!(["a"]));
    assert_eq!(v["result"]["roles"], serde_json::json!(["pm"]));
}

#[test]
fn manifest_only_a_user_selects_nothing_stays_empty() {
    let c = Ctx::new();
    let src = common::make_team_source(&c.tmp.path().join("src"));
    let ws = common::make_business_repo(&c.tmp.path().join("ws"), "biz");
    let url = common::file_url(&src);
    // 不选任何项目/角色 → 允许（零项目仍可见 shared 资源）
    let args = init_args(&c.data_root(), &url, &[]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["projects"], serde_json::json!([]));
    assert_eq!(v["result"]["roles"], serde_json::json!([]));
}

#[test]
fn unknown_project_returns_readable_error_and_exit12() {
    let c = Ctx::new();
    let src = common::make_team_source(&c.tmp.path().join("src"));
    let ws = common::make_business_repo(&c.tmp.path().join("ws"), "biz");
    let url = common::file_url(&src);
    let args = init_args(&c.data_root(), &url, &["--project", "nope"]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 12, "未知项目应退出 12");
    let e: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(e["code"], "E3004");
    assert!(!e["context"]["known"].as_array().unwrap().is_empty(), "{e}");
    assert!(stdout.trim().is_empty(), "错误时 stdout 必须为空: {stdout}");
}

#[test]
fn two_checkouts_share_cache_but_have_distinct_bindings() {
    let c = Ctx::new();
    let src = common::make_team_source(&c.tmp.path().join("src"));
    let biz_origin = common::make_business_repo(c.tmp.path(), "biz-origin");
    let ws_a = c.tmp.path().join("ws-a");
    let ws_b = c.tmp.path().join("ws b"); // 含空格
    common::clone(&biz_origin, &ws_a).unwrap();
    common::clone(&biz_origin, &ws_b).unwrap();
    let url = common::file_url(&src);

    let args_a = init_args(&c.data_root(), &url, &["--project", "a", "--role", "dev"]);
    let refs_a: Vec<&str> = args_a.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws_a, &refs_a);
    assert_eq!(code, 0, "{stderr}");

    let args_b = init_args(&c.data_root(), &url, &["--project", "b", "--role", "pm"]);
    let refs_b: Vec<&str> = args_b.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&ws_b, &refs_b);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["projects"], serde_json::json!(["b"]));

    // binding 不同、缓存共享（同一源身份 → 同一缓存目录）
    let da = c.data_root().join("cache");
    let caches: Vec<_> = std::fs::read_dir(&da).unwrap().collect();
    assert_eq!(caches.len(), 1, "同一源只应有一个缓存目录");
    let ws_root = c.data_root().join("ws");
    assert!(ws_root.exists());
    // 两个 binding 文件存在且 projects 不同
    let mut found_a = false;
    let mut found_b = false;
    for entry in std::fs::read_dir(&ws_root).unwrap().flatten() {
        let bp = entry.path().join("binding.json");
        if let Ok(text) = std::fs::read_to_string(&bp) {
            if text.contains("\"a\"") {
                found_a = true;
            }
            if text.contains("\"b\"") {
                found_b = true;
            }
        }
    }
    assert!(found_a && found_b, "两个工作区绑定互不影响");
}

#[test]
fn repo_move_rebind_keeps_old_workspace() {
    let c = Ctx::new();
    let src = common::make_team_source(&c.tmp.path().join("src"));
    let ws = common::make_business_repo(&c.tmp.path().join("ws"), "biz");
    let url = common::file_url(&src);
    let args = init_args(&c.data_root(), &url, &["--project", "a"]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    // 移动仓库（模拟路径变化）后重新绑定
    let moved = c.tmp.path().join("ws-moved");
    std::fs::rename(&ws, &moved).unwrap();
    let (code, _, stderr) = c.run(&moved, &refs);
    assert_eq!(code, 0, "{stderr}");

    // 旧工作区的 binding 仍在（不被覆盖删除）
    let ws_root = c.data_root().join("ws");
    let count = std::fs::read_dir(&ws_root).unwrap().flatten().count();
    assert_eq!(count, 2, "移动前后是两个独立工作区绑定");
}

#[test]
fn status_reports_missing_lock_and_binding_drift() {
    let c = Ctx::new();
    let src = common::make_team_source(&c.tmp.path().join("src"));
    let ws = common::make_business_repo(&c.tmp.path().join("ws"), "biz");
    let url = common::file_url(&src);
    let args = init_args(&c.data_root(), &url, &["--project", "a"]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);

    // 手改 binding 制造漂移
    let ws_root_json = c.data_root().join("ws");
    for entry in std::fs::read_dir(&ws_root_json).unwrap().flatten() {
        let bp = entry.path().join("binding.json");
        if bp.exists() {
            let text = std::fs::read_to_string(&bp).unwrap();
            std::fs::write(&bp, text.replace("\"a\"", "\"b\"")).unwrap();
        }
    }
    let dr = c.data_root().to_str().unwrap().to_string();
    let status_args = vec!["--json", "--data-root", dr.as_str(), "status"];
    let (code, stdout, _) = c.run(&ws, &status_args);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["ok"], false);
    assert_eq!(v["result"]["binding"]["matches_declaration"], false);
}

#[test]
fn fixture_declarations_validate_with_expected_codes() {
    // AIL-001 冻结的无效 fixture 必须以对应错误码失败
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/fixtures/contract");
    for (file, expect_code) in [
        ("invalid/dup-project.toml", "E3008"),
        ("invalid/project-abs-path.toml", "E3007"),
        ("invalid/project-credential-url.toml", "E2004"),
    ] {
        let text = std::fs::read_to_string(fixtures.join(file)).unwrap();
        let err = ailoom::config::ProjectDeclaration::parse(&text).unwrap_err();
        assert_eq!(err.code, expect_code, "{file}: {err}");
    }
    let valid = std::fs::read_to_string(fixtures.join("valid/project.toml")).unwrap();
    ailoom::config::ProjectDeclaration::parse(&valid).unwrap();

    // 清单 fixtures
    let bad_version =
        std::fs::read_to_string(fixtures.join("invalid/bad-version/ailoom.toml")).unwrap();
    assert_eq!(
        ailoom::manifest::TeamManifest::parse(&bad_version)
            .unwrap_err()
            .code,
        "E3001"
    );
    let traversal =
        std::fs::read_to_string(fixtures.join("invalid/path-traversal/ailoom.toml")).unwrap();
    assert_eq!(
        ailoom::manifest::TeamManifest::parse(&traversal)
            .unwrap_err()
            .code,
        "E3003"
    );
    let valid_manifest = std::fs::read_to_string(fixtures.join("valid/ailoom.toml")).unwrap();
    let m = ailoom::manifest::TeamManifest::parse(&valid_manifest).unwrap();
    assert_eq!(m.team_id, "example-team");
}

#[test]
fn credential_url_rejected_at_cli() {
    let c = Ctx::new();
    let ws = common::make_business_repo(&c.tmp.path().join("ws"), "biz");
    let args = init_args(&c.data_root(), "https://user:pass@example.com/t.git", &[]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 11);
    let e: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(e["code"], "E2004");
    assert!(!stderr.contains("user:pass"));
}
