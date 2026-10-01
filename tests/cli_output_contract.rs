//! AIL-134：CLI 输出契约——结果走 stdout、`--json` 参数错误为 E0001 JSON、
//! 裸跑帮助走 stderr、doctor --strict 失败时退出 10、人类模式无 Debug 格式。

mod common;

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

fn run(tmp: &Path, cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .envs(common::isolated_child_env(tmp))
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn parse_errors_and_bare_invocation() {
    let tmp = tempfile::tempdir().unwrap();
    let (code, out, err) = run(tmp.path(), tmp.path(), &["--json", "bogus"]);
    assert_eq!(code, 2);
    assert!(out.is_empty(), "stdout 保持纯净: {out}");
    let e: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(e["code"], "E0001");
    assert_eq!(e["schema_version"], 1);
    let (code, _, err) = run(tmp.path(), tmp.path(), &["--json", "library"]);
    assert_eq!(code, 2);
    assert!(err.contains("--action"), "{err}");
    let (code, out, err) = run(tmp.path(), tmp.path(), &[]);
    assert_eq!(code, 2);
    assert!(out.is_empty() && err.contains("Usage"), "帮助走 stderr");
    let (code, out, _) = run(tmp.path(), tmp.path(), &["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("Usage"));
}

#[test]
fn results_on_stdout_and_doctor_strict() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = common::make_business_repo(tmp.path(), "biz");
    let dr = tmp.path().join("data");
    let dr = dr.to_str().unwrap();

    // 未绑定工作区：doctor 默认退出 0，--strict 退出 10；输出不含 Debug 引号
    let (code, out, _) = run(tmp.path(), &ws, &["--data-root", dr, "doctor"]);
    assert_eq!(code, 0);
    assert!(!out.contains("OK \"") && !out.contains("!! \""), "{out}");
    let (code, _, _) = run(tmp.path(), &ws, &["--data-root", dr, "doctor", "--strict"]);
    assert_eq!(code, 10);

    let url = common::file_url(&common::make_team_source(&tmp.path().join("src")));
    let (code, out, err) = run(
        tmp.path(),
        &ws,
        &[
            "--data-root",
            dr,
            "init",
            "--url",
            &url,
            "--project",
            "a",
            "--role",
            "dev",
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("已绑定工作区") && !out.contains("[\""),
        "init 结果走 stdout 且无 Debug: {out}"
    );
    let (code, out, err) = run(tmp.path(), &ws, &["--data-root", dr, "sync"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("同步完成"),
        "sync 结果走 stdout: out={out} err={err}"
    );
    let (_, out, _) = run(tmp.path(), &ws, &["--data-root", dr, "status"]);
    assert!(
        !out.contains("Array [") && !out.contains("String("),
        "{out}"
    );

    for args in [
        vec!["library", "--action", "list"],
        vec!["personal", "--action", "effective"],
    ] {
        let mut full = vec!["--data-root", dr];
        full.extend(args.iter().copied());
        let (code, out, err) = run(tmp.path(), &ws, &full);
        assert_eq!(code, 0, "{args:?}: {err}");
        assert!(!out.trim().is_empty(), "{args:?} 人类模式必须有输出");
        if args[0] == "personal" {
            assert!(
                !out.trim_start().starts_with('{'),
                "{args:?} 人类模式不应直接打印 JSON: {out}"
            );
            assert!(out.contains("AI 工具"), "{out}");
        }
    }
}
