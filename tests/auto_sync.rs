//! 无感知 auto_sync：TTL 门控与 sync --refresh。

mod common;

use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Ctx {
    tmp: tempfile::TempDir,
}

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

impl Ctx {
    fn new() -> Ctx {
        Ctx {
            tmp: tempfile::tempdir().unwrap(),
        }
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
    fn run_stdin(
        &self,
        cwd: &Path,
        args: &[&str],
        stdin_payload: &str,
        extra_env: &[(&str, &str)],
    ) -> (i32, String, String) {
        let mut cmd = Command::new(bin());
        cmd.args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin_payload.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn dr(&self) -> String {
        self.tmp.path().join("data").to_string_lossy().to_string()
    }
}

fn setup(c: &Ctx) -> PathBuf {
    let bare = c.tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let team_src = common::make_team_source(c.tmp.path());
    ailoom::gitx::git(&team_src, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(
        &team_src,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    )
    .unwrap();
    ailoom::gitx::git(&team_src, &["push", "-q", "-u", "origin", "main"]).unwrap();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = team_src.to_str().unwrap().to_string();
    let dr = c.dr();
    let args = [
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
        "--target".to_string(),
        "claude".to_string(),
        "--root".to_string(),
        ws.to_str().unwrap().to_string(),
    ];
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, _, err) = c.run(&ws, &args_ref);
    assert_eq!(code, 0, "init: {err}");
    let (code, _, err) = c.run(
        &ws,
        &["--data-root", &dr, "sync", "--root", ws.to_str().unwrap()],
    );
    assert_eq!(code, 0, "sync: {err}");
    ws
}

#[test]
fn auto_sync_ttl_skips_second_hook() {
    let c = Ctx::new();
    let ws = setup(&c);
    let dr = c.dr();
    let payload = r#"{"session_id":"s-auto","cwd":"."}"#;
    let (code, out, _) = c.run_stdin(
        &ws,
        &[
            "--json",
            "--data-root",
            &dr,
            "hook",
            "--tool",
            "claude",
            "--event",
            "sessionstart",
            "--root",
            ws.to_str().unwrap(),
        ],
        payload,
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("scheduled") || out.contains("\"auto_sync\""),
        "first hook should schedule or report auto_sync: {out}"
    );

    let (code2, out2, _) = c.run_stdin(
        &ws,
        &[
            "--json",
            "--data-root",
            &dr,
            "hook",
            "--tool",
            "claude",
            "--event",
            "userpromptsubmit",
            "--root",
            ws.to_str().unwrap(),
        ],
        payload,
        &[],
    );
    assert_eq!(code2, 0, "{out2}");
    assert!(
        out2.contains("not_due"),
        "second hook within 1d TTL must be not_due: {out2}"
    );
}

#[test]
fn auto_sync_disabled_by_env() {
    let c = Ctx::new();
    let ws = setup(&c);
    let dr = c.dr();
    let payload = r#"{"session_id":"s-off","cwd":"."}"#;
    let (code, out, _) = c.run_stdin(
        &ws,
        &[
            "--json",
            "--data-root",
            &dr,
            "hook",
            "--tool",
            "claude",
            "--event",
            "sessionstart",
            "--root",
            ws.to_str().unwrap(),
        ],
        payload,
        &[("AILOOM_AUTO_SYNC", "0")],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("not_due"), "disabled → not_due: {out}");
}

#[test]
fn sync_refresh_flag_ok() {
    let c = Ctx::new();
    let ws = setup(&c);
    let dr = c.dr();
    let (code, out, err) = c.run(
        &ws,
        &[
            "--json",
            "--data-root",
            &dr,
            "sync",
            "--refresh",
            "--root",
            ws.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "stderr={err} stdout={out}");
    assert!(out.contains("lock_refreshed"), "{out}");
}
