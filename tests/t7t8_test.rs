use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    loop {
        path.pop();
        let p = if cfg!(windows) {
            "ailoom.exe"
        } else {
            "ailoom"
        };
        if path.join(p).exists() {
            return path.join(p);
        }
    }
}

#[test]
fn cursor_antigravity_rules_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    // 团队源：带 cursor/antigravity targets 的规则
    let bare = tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let src = tmp.path().join("team-src");
    ailoom::gitx::git_init(&src, false).unwrap();
    std::fs::create_dir_all(src.join("resources/rules")).unwrap();
    std::fs::write(src.join("ailoom.toml"), "schema_version = 1\nteam_id = \"t\"\n[projects.alva]\nname = \"alva\"\n[namespaces]\nknown = [\"common\"]\nshared = [\"common\"]\n").unwrap();
    std::fs::write(
        src.join("resources/rules/commit-style.md"),
        "---\nname: commit-style\ndescription: 提交规范\nshared: true\nnamespace: common\ntargets: [cursor, antigravity]\n---\n\n提交信息用祈使句。\n",
    ).unwrap();
    ailoom::gitx::git_commit_all(&src, "init", &["ailoom.toml", "resources"]).unwrap();

    // 业务仓
    let ws = tmp.path().join("biz");
    ailoom::gitx::git_init(&ws, false).unwrap();
    std::fs::write(ws.join("a.txt"), "x").unwrap();
    ailoom::gitx::git_commit_all(&ws, "init", &["a.txt"]).unwrap();

    let dr = data.to_str().unwrap().to_string();
    let run = |ws: &Path, env_key: Option<(&str, &str)>, args: &[&str]| {
        let mut cmd = Command::new(bin());
        cmd.args(args)
            .current_dir(ws)
            .env("HOME", tmp.path().join("home"))
            .env("AILOOM_LOG", "error");
        if let Some((k, v)) = env_key {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    };
    let url = src.to_str().unwrap().to_string();
    let init_args = vec![
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "alva".to_string(),
        "--target".to_string(),
        "cursor".to_string(),
        "--target".to_string(),
        "antigravity".to_string(),
    ];
    let refs: Vec<&str> = init_args.iter().map(String::as_str).collect();
    let out = run(&ws, None, &refs);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let sync_args = ["--data-root".to_string(), dr.clone(), "sync".to_string()];
    let refs: Vec<&str> = sync_args.iter().map(String::as_str).collect();
    let out = run(&ws, None, &refs);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Cursor .mdc 落盘
    let cursor = ws.join(".cursor/rules/commit-style.mdc");
    assert!(cursor.is_file(), "Cursor 规则未落盘");
    let text = std::fs::read_to_string(&cursor).unwrap();
    assert!(text.contains("alwaysApply: true"), "{text}");
    assert!(text.contains("祈使句"));
    // Antigravity .md 落盘
    let ag = ws.join(".antigravity/rules/commit-style.md");
    assert!(ag.is_file(), "Antigravity 规则未落盘");
    let text = std::fs::read_to_string(&ag).unwrap();
    assert!(text.contains("trigger: always_on"), "{text}");
    let _ = Path::new(".");
}
