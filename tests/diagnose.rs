//! `ailoom diagnose`：报告可直接转发，不含用户目录、用户名与令牌。
mod common;

use std::process::Command;

#[test]
fn diagnose_writes_a_redacted_report() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(data.join("service")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        data.join("service/service.log"),
        format!(
            "[info] 控制台：http://127.0.0.1:1/\n[debug] {}/.claude/skills/x token=abc123secret\n",
            home.display()
        ),
    )
    .unwrap();
    let out_dir = tmp.path().join("out");
    let out = Command::new(env!("CARGO_BIN_EXE_ailoom"))
        .args(["--json", "--data-root"])
        .arg(&data)
        .args(["diagnose", "--out"])
        .arg(&out_dir)
        .current_dir(tmp.path())
        .envs(common::isolated_child_env(tmp.path()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let path = v["result"]["path"].as_str().unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["service"]["state"], "stopped", "{report}");
    assert!(
        text.contains("~/.claude/skills/x"),
        "用户目录替换为 ~: {text}"
    );
    assert!(!text.contains(&home.display().to_string()), "{text}");
    assert!(!text.contains("abc123secret"), "令牌被抹掉: {text}");
}
