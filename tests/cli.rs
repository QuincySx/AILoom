//! CLI 基础行为测试（AIL-002）。

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    // 目标二进制与测试可执行同目录（target/debug）
    while path.pop() {
        if path.join("ailoom").exists() || path.join("ailoom.exe").exists() {
            let p = if cfg!(windows) {
                "ailoom.exe"
            } else {
                "ailoom"
            };
            return path.join(p);
        }
    }
    panic!("未找到 ailoom 二进制");
}

struct Output {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run(args: &[&str], home: &std::path::Path) -> Output {
    let out = Command::new(bin())
        .args(args)
        .env("HOME", home)
        .env("AILOOM_LOG", "error")
        .output()
        .expect("运行 ailoom 失败");
    Output {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code().unwrap_or(-1),
    }
}

#[test]
fn version_returns_zero_and_prints() {
    let home = tempfile::tempdir().unwrap();
    let out = run(&["version"], home.path());
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("ailoom 0.1.0"),
        "stdout: {}",
        out.stdout
    );
}

#[test]
fn help_and_long_version_return_zero() {
    let home = tempfile::tempdir().unwrap();
    let out = run(&["--help"], home.path());
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("Usage:"));

    let out = run(&["--version"], home.path());
    assert_eq!(out.code, 0);
}

#[test]
fn no_subcommand_is_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let out = run(&[], home.path());
    assert_eq!(out.code, 2);
}

#[test]
fn unknown_arg_is_nonzero_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let out = run(&["definitely-not-a-command"], home.path());
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("error"), "stderr: {}", out.stderr);
}

#[test]
fn json_stdout_is_parseable_and_log_free() {
    let home = tempfile::tempdir().unwrap();
    let out = run(&["--json", "version"], home.path());
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: serde_json::Value =
        serde_json::from_str(out.stdout.trim()).expect("JSON stdout 必须可独立解析");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["result"]["name"], "ailoom");
}

#[test]
fn unknown_schema_of_env_does_not_affect_cli() {
    // 环境变量 AILOOM_LOG 任意值不得导致崩溃
    let home = tempfile::tempdir().unwrap();
    let out = Command::new(bin())
        .args(["version"])
        .env("HOME", home.path())
        .env("AILOOM_LOG", "nonsense")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
}
