//! AIL-135 守卫：每个命令 `--action` 的可选值（help 中的 possible values）都必须被实现识别，
//! 不能出现「help 列出、执行时却报未知动作」。所有命令在隔离目录中运行，不触网。

mod common;

use std::process::Command;

fn run(tmp: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ailoom"))
        .args(args)
        .current_dir(tmp)
        .envs(common::isolated_child_env(tmp))
        .env("AILOOM_REPORTING", "off")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn every_listed_action_is_recognized() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let data = tmp.path().join("data");
    let data = data.to_str().unwrap();
    let mut checked = 0;
    for cmd in [
        "hooks",
        "session",
        "pr",
        "data",
        "members",
        "packages",
        "code",
        "report",
        "knowledge",
        "library",
        "personal",
        "collection",
    ] {
        let (_, help, _) = run(&work, &[cmd, "--help"]);
        let line = help
            .lines()
            .skip_while(|l| !l.contains("--action"))
            .find(|l| l.contains("[possible values:") || l.contains("[default:"))
            .unwrap_or("");
        let start = help
            .find("--action")
            .unwrap_or_else(|| panic!("{cmd} 没有 --action"));
        let rest = &help[start..];
        let values = rest
            .split("[possible values: ")
            .nth(1)
            .and_then(|v| v.split(']').next())
            .unwrap_or_else(|| panic!("{cmd} --action 未列出可选值: {line}"));
        for action in values.split(", ").map(str::trim) {
            // 只验证分派：不带其余参数，期望得到「缺参数 / 未绑定」等业务错误或成功，而不是未知动作。
            if (cmd, action) == ("library", "update") || (cmd, action) == ("collection", "check") {
                continue; // 这两个动作无参数时会检查全部来源，可能访问网络
            }
            let (_, _, err) = run(
                &work,
                &["--data-root", data, "--json", cmd, "--action", action],
            );
            assert!(
                !err.contains("未知") || !err.contains("动作"),
                "{cmd} --action {action} 未被识别: {err}"
            );
            checked += 1;
        }
    }
    assert!(checked > 50, "检查数量过少: {checked}");
}
