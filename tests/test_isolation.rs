//! AIL-002 回归：集成测试子进程路径隔离。
//!
//! 历史缺陷：测试只替换 HOME，继承了运行环境预置的 XDG_DATA_HOME /
//! AILOOM_STORE_ROOT 等覆盖变量后，sync 把 Skill 实体写入真实 Store。
//! 本文件验证 tests/common::isolated_child_env 的两条边界：
//! 1. 运行环境预置外部覆盖变量时，哨兵目录逐字节不变，写入落在受控根；
//! 2. 无任何覆盖变量时，回落到 HOME 布局（~/.ailoom/store），同样不外溢。

mod common;

use std::fs;
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

fn run(cwd: &Path, tmp: &Path, runner_overrides: &[(&str, &Path)], args: &[&str]) -> (i32, String) {
    let mut cmd = Command::new(bin());
    cmd.args(args).current_dir(cwd);
    // 先铺上"运行环境"预置的覆盖变量，再由统一隔离 helper 全部接管
    for (k, v) in runner_overrides {
        cmd.env(k, v);
    }
    cmd.envs(common::isolated_child_env(tmp));
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                out.push((rel, fs::read(&p).unwrap()));
            }
        }
    }
    if dir.exists() {
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    } else {
        Vec::new()
    }
}

fn init_and_sync(cwd: &Path, tmp: &Path, src: &Path, runner_overrides: &[(&str, &Path)]) {
    let dr = tmp.join("data");
    fs::create_dir_all(&dr).unwrap();
    let url = src.to_string_lossy().into_owned();
    let (code, err) = run(
        cwd,
        tmp,
        runner_overrides,
        &["--data-root", dr.to_str().unwrap(), "init", "--url", &url],
    );
    assert_eq!(code, 0, "init: {err}");
    let (code, err) = run(
        cwd,
        tmp,
        runner_overrides,
        &["--data-root", dr.to_str().unwrap(), "sync"],
    );
    assert_eq!(code, 0, "sync: {err}");
}

fn make_workspace(tmp: &Path) -> (PathBuf, PathBuf) {
    let src = common::make_team_source(&tmp.join("src-root"));
    let ws = common::make_business_repo(&tmp.join("biz-root"), "biz");
    (src, ws)
}

#[test]
fn runner_overrides_do_not_leak_into_sentinel_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, ws) = make_workspace(tmp.path());

    // 模拟运行环境已把 5 个根变量全部预置到"真实"哨兵目录
    let sentinel = tmp.path().join("sentinel");
    let dirs = [
        ("HOME", sentinel.join("home")),
        ("XDG_DATA_HOME", sentinel.join("xdg-data")),
        ("XDG_STATE_HOME", sentinel.join("xdg-state")),
        ("AILOOM_DATA_ROOT", sentinel.join("ailoom-data")),
        ("AILOOM_STORE_ROOT", sentinel.join("ailoom-store")),
    ];
    for (_, d) in &dirs {
        fs::create_dir_all(d).unwrap();
        fs::write(d.join(".sentinel-marker"), b"do-not-touch").unwrap();
    }
    let before: Vec<_> = dirs.iter().map(|(_, d)| snapshot(d)).collect();

    let overrides: Vec<(&str, &Path)> = dirs.iter().map(|(k, d)| (*k, d.as_path())).collect();
    init_and_sync(&ws, tmp.path(), &src, &overrides);

    for (i, (_, d)) in dirs.iter().enumerate() {
        let after = snapshot(d);
        assert_eq!(before[i], after, "哨兵目录被测试写入: {}", d.display());
    }
    // 写入必须落在受控 Store 根
    let store = common::isolated_store_root(tmp.path());
    assert!(store.is_dir(), "受控 Store 根应存在: {}", store.display());
    assert!(ws.join(".claude/skills/common-greet/SKILL.md").is_file());
}

#[test]
fn no_override_vars_falls_back_to_home_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, ws) = make_workspace(tmp.path());

    let mut cmd = Command::new(bin());
    cmd.args([
        "--data-root",
        tmp.path().join("data").to_str().unwrap(),
        "init",
        "--url",
        src.to_str().unwrap(),
    ])
    .current_dir(&ws);
    // 显式移除全部覆盖变量：模拟"无覆盖变量"的受控环境
    for k in [
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "AILOOM_DATA_ROOT",
        "AILOOM_STORE_ROOT",
    ] {
        cmd.env_remove(k);
    }
    cmd.env("HOME", tmp.path().join("home"))
        .env("AILOOM_LOG", "error");
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "init 失败");

    let mut cmd = Command::new(bin());
    cmd.args([
        "--data-root",
        tmp.path().join("data").to_str().unwrap(),
        "sync",
    ])
    .current_dir(&ws);
    for k in [
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "AILOOM_DATA_ROOT",
        "AILOOM_STORE_ROOT",
    ] {
        cmd.env_remove(k);
    }
    cmd.env("HOME", tmp.path().join("home"))
        .env("AILOOM_LOG", "error");
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "sync 失败");

    // 契约：无 XDG/AILOOM 覆盖时 Store 根回落 $HOME/.ailoom/store
    let home_store = tmp.path().join("home/.ailoom/store");
    assert!(
        home_store.is_dir(),
        "应回落 HOME 布局: {}",
        home_store.display()
    );
    assert!(ws.join(".claude/skills/common-greet/SKILL.md").is_file());
}
