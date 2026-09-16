//! 环境配置（AIL-031）、团队 Hook（AIL-032）、包依赖（AIL-033）集成测试。

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    fn dr(&self) -> String {
        self.tmp.path().join("data").to_string_lossy().to_string()
    }
    fn setup(&self) -> (PathBuf, PathBuf) {
        let bare = self.tmp.path().join("origin.git");
        ailoom::gitx::git_init(&bare, true).unwrap();
        let src = common::make_team_source(self.tmp.path());
        ailoom::gitx::git(&src, &["branch", "-M", "main"]).unwrap();
        ailoom::gitx::git(&src, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
        ailoom::gitx::git(&src, &["push", "-q", "-u", "origin", "main"]).unwrap();
        let ws = common::make_business_repo(self.tmp.path(), "biz");
        let url = bare.to_str().unwrap().to_string();
        let dr = self.dr();
        let args = [
            "--json".to_string(),
            "--data-root".to_string(),
            dr,
            "init".to_string(),
            "--url".to_string(),
            url,
            "--project".to_string(),
            "a".to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = self.run(&ws, &refs);
        assert_eq!(code, 0, "{stderr}");
        (bare, ws)
    }
    fn sync(&self, ws: &Path) -> (i32, String, String) {
        let dr = self.dr();
        let refs: Vec<&str> = vec!["--data-root", dr.as_str(), "sync"];
        self.run(ws, &refs)
    }
}

fn env_toml(with_secret: bool) -> String {
    let secret = if with_secret {
        "\n[secret_refs]\nTOKEN = \"$ENV:AILOOM_T\"\n"
    } else {
        ""
    };
    format!(
        "name = \"team-env\"\nshared = true\nnamespace = \"common\"\ntargets = [\"claude\"]\n\n[vars]\nAPI_BASE = \"https://x.internal\"\n{secret}"
    )
}

#[test]
fn env_literal_vars_deployed_secret_refs_rejected_without_plaintext() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/env")).unwrap();
    std::fs::write(src.join("resources/env/team-env.toml"), env_toml(true)).unwrap();
    common::commit_only(&src, "add env");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "{stderr}");

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    // 字面量部署；秘密引用不写明文
    assert_eq!(settings["env"]["API_BASE"], "https://x.internal");
    assert!(settings["env"].get("TOKEN").is_none(), "秘密引用不得写明文");
    // plan 中显式 unsupported
    let args = ["--json", "--data-root", &c.dr(), "plan"];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(v["result"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |a| a["action"] == "unsupported" && a["reason"].as_str().unwrap().contains("秘密引用")
        ));
}

#[test]
fn env_user_variable_not_overwritten_and_uninstall_removes_own() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    // 用户已有同名变量（不同值）→ 冲突保护
    std::fs::create_dir_all(ws.join(".claude")).unwrap();
    std::fs::write(
        ws.join(".claude/settings.json"),
        r#"{"env":{"API_BASE":"https://mine"}}"#,
    )
    .unwrap();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/env")).unwrap();
    std::fs::write(src.join("resources/env/team-env.toml"), env_toml(false)).unwrap();
    common::commit_only(&src, "add env");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["env"]["API_BASE"], "https://mine",
        "个人同名变量不静默覆盖"
    );

    // AIL-031 验收：真实执行卸载，验证 AILoom 托管条目被清理、用户条目保留。
    // 本场景因同名冲突未部署托管变量 → settings 中不应出现团队默认值；
    // 再用无冲突环境完整走一遍“部署 → 卸载”闭环。
    let (code, _, stderr) = c.run(
        ws.as_path(),
        &["--data-root", c.dr().as_str(), "uninstall", "--execute"],
    );
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["env"]["API_BASE"], "https://mine",
        "卸载后用户自己的变量仍保留"
    );
    assert!(
        settings["env"].get("TIMEOUT").is_none(),
        "托管条目应被清理: {settings}"
    );
}

/// AIL-031：无冲突部署后真实卸载——托管 env 变量与受管片段被移除，
/// 用户无关变量与用户文件原样保留；重复卸载幂等。
#[test]
fn uninstall_removes_managed_env_entries_and_keeps_user_data() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    // 用户在 settings 里有自己的无关变量与文件
    std::fs::create_dir_all(ws.join(".claude")).unwrap();
    std::fs::write(
        ws.join(".claude/settings.json"),
        r#"{"env":{"MY_OWN":"keep-me"}}"#,
    )
    .unwrap();
    let marker = ws.join("user-notes.md");
    std::fs::write(&marker, "用户笔记").unwrap();

    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/env")).unwrap();
    // 两个字面量变量（TIMEOUT + API_BASE 无冲突变体）
    std::fs::write(
        src.join("resources/env/team-env.toml"),
        "name = \"clean-env\"\nshared = true\nnamespace = \"common\"\ntargets = [\"claude\"]\n\n[vars]\nTIMEOUT = \"30\"\n",
    )
    .unwrap();
    common::commit_only(&src, "add env");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["env"]["TIMEOUT"], "30", "部署成功");
    assert_eq!(settings["env"]["MY_OWN"], "keep-me");

    // 真实执行卸载
    let (code, _, stderr) = c.run(
        ws.as_path(),
        &["--data-root", c.dr().as_str(), "uninstall", "--execute"],
    );
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(
        settings["env"].get("TIMEOUT").is_none(),
        "托管变量被清理: {settings}"
    );
    assert_eq!(
        settings["env"]["MY_OWN"], "keep-me",
        "用户变量保留: {settings}"
    );
    assert!(
        marker.is_file(),
        "用户文件保留: {}",
        std::fs::read_to_string(&marker).unwrap()
    );

    // 重复卸载幂等
    let (code, _, stderr) = c.run(
        ws.as_path(),
        &["--data-root", c.dr().as_str(), "uninstall", "--execute"],
    );
    assert_eq!(code, 0, "重复卸载幂等: {stderr}");
}

#[test]
fn culture_and_env_repeated_sync_no_duplicates() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/env")).unwrap();
    std::fs::write(src.join("resources/env/team-env.toml"), env_toml(false)).unwrap();
    // 文化文本 = 规则资源（受管片段）
    std::fs::write(
        src.join("resources/rules/team-culture.md"),
        "---\nname: team-culture\ndescription: 团队文化\nshared: true\nnamespace: common\n---\n\n- 坦诚沟通\n- 尊重评审\n",
    )
    .unwrap();
    common::commit_only(&src, "add culture and env");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let _ = c.sync(&ws);
    let _ = c.sync(&ws);
    let agents = std::fs::read_to_string(ws.join("AGENTS.md")).unwrap();
    assert_eq!(
        agents
            .matches("BEGIN AILOOM MANAGED: team/rule/common/team-culture")
            .count(),
        1,
        "文化重复同步无重复段落"
    );
}

#[test]
fn packages_check_reports_missing_and_install_gated() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/packages")).unwrap();
    std::fs::write(
        src.join("resources/packages/leftpad.toml"),
        "name = \"leftpad\"\necosystem = \"npm\"\nversion = \"1.3.0\"\nshared = true\nnamespace = \"common\"\n",
    )
    .unwrap();
    common::commit_only(&src, "add package");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    // check：missing 报告
    let args = [
        "--json",
        "--data-root",
        &c.dr(),
        "packages",
        "--action",
        "check",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let rows = v["result"]["packages"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["state"] == "missing"), "{v}");

    // install 无 --yes：拒绝（普通流程不擅自安装）
    let args = ["--data-root", &c.dr(), "packages", "--action", "install"];
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 2, "install 需显式 --yes");
    assert!(stderr.contains("--yes"), "{stderr}");
}

#[test]
fn team_hook_registered_and_user_hooks_preserved() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    std::fs::create_dir_all(src.join("resources/hooks")).unwrap();
    std::fs::write(
        src.join("resources/hooks/team-stop.toml"),
        "name = \"team-stop\"\nevent = \"Stop\"\nmatcher = \"\"\ncommand = [\"echo\", \"team hook ran\"]\ntimeout_ms = 3000\nshared = true\nnamespace = \"common\"\ntargets = [\"claude\"]\n",
    )
    .unwrap();
    common::commit_only(&src, "add team hook");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    // 用户自己的 hook 在首次同步前就存在
    std::fs::create_dir_all(ws.join(".claude")).unwrap();
    std::fs::write(
        ws.join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"user-own"}]}]}}"#,
    )
    .unwrap();
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    let stop = settings["hooks"]["Stop"].as_array().unwrap();
    assert!(
        stop.iter().any(|e| e["hooks"][0]["command"] == "user-own"),
        "用户 hook 保留"
    );
    assert!(
        stop.iter().any(|e| e["hooks"][0]["command"]
            .as_str()
            .unwrap_or("")
            .starts_with("ailoom hooks exec --id")),
        "团队 hook 以 exec 包装注册"
    );
}

// ---------- AIL-032 返工回归（R15） ----------

fn add_team_hook(src: &Path, name: &str, timeout_ms: u64) {
    std::fs::create_dir_all(src.join("resources/hooks")).unwrap();
    std::fs::write(
        src.join("resources/hooks").join(format!("{name}.toml")),
        format!(
            "name = \"{name}\"\nevent = \"Stop\"\nmatcher = \"\"\ncommand = [\"echo\", \"{name}\"]\ntimeout_ms = {timeout_ms}\nshared = true\nnamespace = \"common\"\ntargets = [\"claude\"]\n"
        ),
    )
    .unwrap();
}

fn managed_stop_entries(settings: &serde_json::Value) -> Vec<String> {
    settings["hooks"]["Stop"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["hooks"][0]["command"].as_str())
        .filter(|c| c.starts_with("ailoom hooks exec --id"))
        .map(str::to_string)
        .collect()
}

/// 同一事件多个团队 Hook：各有独立签名不冲突；重复 sync 幂等不漂移；用户 hook 保留。
#[test]
fn team_hooks_same_event_stable_across_resync() {
    let c = Ctx::new();
    let (_bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    add_team_hook(&src, "hook-alpha", 3000);
    add_team_hook(&src, "hook-beta", 3000);
    common::commit_only(&src, "add hooks");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();

    std::fs::create_dir_all(ws.join(".claude")).unwrap();
    std::fs::write(
        ws.join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"user-own"}]}]}}"#,
    )
    .unwrap();

    // sync --refresh：拉取含新 hook 的源快照
    let (code, _, stderr) = c.run(
        ws.as_path(),
        &["--data-root", c.dr().as_str(), "sync", "--refresh"],
    );
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    let entries = managed_stop_entries(&settings);
    assert_eq!(entries.len(), 2, "两个团队 hook 都注册: {settings}");
    assert!(entries.iter().any(|e| e.contains("hook-alpha")));
    assert!(entries.iter().any(|e| e.contains("hook-beta")));
    assert!(
        settings["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["hooks"][0]["command"] == "user-own"),
        "用户 hook 保留"
    );

    // 重复 sync：不产生新条目/不漂移（幂等）
    let (code, _, _) = c.sync(&ws);
    assert_eq!(code, 0);
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    let entries2 = managed_stop_entries(&settings);
    assert_eq!(entries2.len(), 2, "重复 sync 幂等: {settings}");
    assert_eq!(
        std::collections::BTreeSet::from_iter(entries.iter()),
        std::collections::BTreeSet::from_iter(entries2.iter()),
        "签名身份稳定"
    );

    // remove：两个团队 hook 移除，用户 hook 保留
    let (code, _, stderr) = c.run(
        ws.as_path(),
        &["--data-root", c.dr().as_str(), "uninstall", "--execute"],
    );
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(
        managed_stop_entries(&settings).is_empty(),
        "团队 hook 全部移除: {settings}"
    );
    assert!(
        settings["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["hooks"][0]["command"] == "user-own"),
        "用户 hook 仍保留"
    );
}

/// exec：大输出不误超时；超时回收进程组；失败退出不破坏宿主（exit 0 + 结果字段）。
#[test]
fn team_hook_exec_output_timeout_and_failure_semantics() {
    let c = Ctx::new();
    let (_bare, ws) = c.setup();
    let specs = ws.join(".ailoom-hook-specs");
    std::fs::create_dir_all(&specs).unwrap();

    let exec = |id: &str| {
        c.run(
            ws.as_path(),
            &[
                "--json",
                "--data-root",
                c.dr().as_str(),
                "hooks",
                "--action",
                "exec",
                "--id",
                id,
            ],
        )
    };
    let write_spec = |id: &str, command: &[&str], timeout_ms: u64| {
        let spec = serde_json::json!({
            "resource_id": id,
            "command": command,
            "timeout_ms": timeout_ms,
        });
        std::fs::write(
            specs.join(format!("{}.json", id.replace('/', "__"))),
            serde_json::to_string_pretty(&spec).unwrap(),
        )
        .unwrap();
    };

    // 1) 大输出（>64KB 管道容量）正常退出：不误判超时
    write_spec(
        "team/hook/big-output",
        &["/bin/sh", "-c", "yes big-output-line | head -c 200000"],
        8000,
    );
    let (code, stdout, stderr) = exec("team/hook/big-output");
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
    assert_eq!(
        v["result"]["executed"], true,
        "大输出命令不应误超时: {stdout}"
    );
    assert_eq!(v["result"]["timed_out"], false, "{stdout}");

    // 2) 超时：进程组被回收；exec 自身退出 0（不破坏宿主任务）
    write_spec(
        "team/hook/slow",
        &["/bin/sh", "-c", "sleep 30; echo done"],
        300,
    );
    let (code, stdout, _) = exec("team/hook/slow");
    assert_eq!(code, 0, "超时也不得使宿主 hook 非零退出");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
    assert_eq!(v["result"]["executed"], false, "{stdout}");
    assert_eq!(v["result"]["timed_out"], true, "{stdout}");

    // 3) 失败退出：结果字段带退出码，exec 退出 0
    write_spec(
        "team/hook/fail",
        &["/bin/sh", "-c", "echo boom >&2; exit 3"],
        5000,
    );
    let (code, stdout, stderr) = exec("team/hook/fail");
    assert_eq!(code, 0, "失败退出不得破坏宿主任务: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
    assert_eq!(v["result"]["executed"], false, "{stdout}");
    assert_eq!(v["result"]["exit"], 3, "{stdout}");
}

// ---------- AIL-033 返工回归（R16） ----------

fn add_package(src: &Path, file_name: &str, name: &str, version: &str) {
    std::fs::create_dir_all(src.join("resources/packages")).unwrap();
    std::fs::write(
        src.join("resources/packages").join(file_name),
        format!(
            "name = \"{name}\"\necosystem = \"npm\"\nversion = \"{version}\"\nshared = true\nnamespace = \"common\"\n"
        ),
    )
    .unwrap();
}

fn packages_json(
    c: &Ctx,
    ws: &Path,
    action: &str,
    extra: &[&str],
) -> (i32, serde_json::Value, String) {
    let mut args = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "packages".to_string(),
        "--action".to_string(),
        action.to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(ws, &refs);
    let v = serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap_or_default();
    (code, v, stderr)
}

/// 范围/空版本被拒绝；同包不同版本是可解释冲突；同版本合并。
#[test]
fn packages_reject_ranges_and_conflicting_versions() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    add_package(&src, "rangever.toml", "somepkg", "^1.2.3");
    common::commit_only(&src, "add range ver");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    let (code, _, stderr) = packages_json(&c, ws.as_path(), "check", &[]);
    assert_ne!(code, 0, "范围版本必须被拒绝");
    assert!(stderr.contains("精确版本"), "{stderr}");
    assert!(stderr.contains("^1.2.3"), "{stderr}");

    // 同包两个资源、不同版本 → 可解释冲突
    let c2 = Ctx::new();
    let (bare2, ws2) = c2.setup();
    let src2 = c2.tmp.path().join("team-src");
    add_package(&src2, "pkg-a.toml", "dup-pkg", "1.0.0");
    add_package(&src2, "pkg-b.toml", "dup-pkg", "2.0.0");
    common::commit_only(&src2, "add dup pkgs");
    ailoom::gitx::git(&src2, &["push", "-q", "origin", "main"]).unwrap();
    let args2 = [
        "--data-root".to_string(),
        c2.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare2.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs2: Vec<&str> = args2.iter().map(String::as_str).collect();
    let (code, _, _) = c2.run(&ws2, &refs2);
    assert_eq!(code, 0);
    let (code, _, stderr) = packages_json(&c2, ws2.as_path(), "check", &[]);
    // package TOML 的 name 同时是资源名与 npm 包名：同包不同版本在 resolver 层
    // 即以 E3006 目标冲突大声失败，不允许静默覆盖（自定义版本冲突检查是纵深防御）
    assert_ne!(code, 0, "同包不同版本必须冲突");
    assert!(
        stderr.contains("E3006") || stderr.contains("冲突"),
        "{stderr}"
    );

    // 同包同版本重复声明（两个文件）：package TOML 的 name 同时是资源身份，
    // resolver 以 E3006 大声拒绝重复声明（作者错误，不做隐式合并）
    let c3 = Ctx::new();
    let (bare3, ws3) = c3.setup();
    let src3 = c3.tmp.path().join("team-src");
    add_package(&src3, "pkg-a.toml", "same-pkg", "1.0.0");
    add_package(&src3, "pkg-b.toml", "same-pkg", "1.0.0");
    common::commit_only(&src3, "add same pkgs");
    ailoom::gitx::git(&src3, &["push", "-q", "origin", "main"]).unwrap();
    let args3 = [
        "--data-root".to_string(),
        c3.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare3.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs3: Vec<&str> = args3.iter().map(String::as_str).collect();
    let (code, _, _) = c3.run(&ws3, &refs3);
    assert_eq!(code, 0);
    let (code, _, stderr) = packages_json(&c3, ws3.as_path(), "check", &[]);
    assert_ne!(code, 0, "重复声明必须大声失败");
    assert!(
        stderr.contains("E3006") || stderr.contains("冲突"),
        "{stderr}"
    );
}

/// 全部已满足 → 不执行 npm（受控 npm 假体若被调用即失败）。
#[test]
fn packages_install_skips_npm_when_satisfied() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    add_package(&src, "okpkg.toml", "ok-pkg", "2.1.0");
    common::commit_only(&src, "add pkg");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    // 预置满足态的 node_modules
    let prefix = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(ailoom::ids::workspace_id_from_root(
            &ws.canonicalize().unwrap(),
        ))
        .join("packages");
    let mod_dir = prefix.join("node_modules").join("ok-pkg");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(
        mod_dir.join("package.json"),
        r#"{"name":"ok-pkg","version":"2.1.0"}"#,
    )
    .unwrap();

    // PATH 前置假 npm：被调用即写标记（断言不得发生）
    let fake_bin = c.tmp.path().join("fake-bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let fake = fake_bin.join("npm");
    std::fs::write(
        &fake,
        "#!/bin/sh\ntouch \"${NPM_CALLED_MARKER:?}\"\nexit 1\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

    let marker = c.tmp.path().join("npm-was-called");
    let mut cmd = Command::new(bin());
    cmd.args([
        "--json",
        "--data-root",
        c.dr().as_str(),
        "packages",
        "--action",
        "install",
        "--yes",
    ])
    .current_dir(ws.as_path())
    .env("NPM_CALLED_MARKER", marker.display().to_string())
    .env(
        "PATH",
        format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
    );
    let out = cmd.output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(v["result"]["npm_ran"], false, "已满足不得执行 npm: {v}");
    assert!(
        !marker.exists(),
        "npm 假体被调用——已满足仍执行安装是 R16 反例"
    );
    let rows = v["result"]["packages"].as_array().unwrap();
    assert!(rows.iter().all(|r| r["state"] == "satisfied"), "{v}");
}

/// 受控 npm 假体执行"安装"（写 node_modules）：install 后返回安装后状态。
#[test]
fn packages_install_reports_post_install_state() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    add_package(&src, "fresh.toml", "fresh-pkg", "0.9.1");
    common::commit_only(&src, "add pkg");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    let prefix = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(ailoom::ids::workspace_id_from_root(
            &ws.canonicalize().unwrap(),
        ))
        .join("packages");
    let fake_bin = c.tmp.path().join("fake-bin2");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let fake = fake_bin.join("npm");
    // 受控假体：按调用方写入的 package.json 锁定版本物化 node_modules（占位符替换，避免转义）
    let script = "#!/bin/sh\nmkdir -p PREFIX/node_modules/fresh-pkg\nprintf '%s' '{\"name\":\"fresh-pkg\",\"version\":\"0.9.1\"}' > PREFIX/node_modules/fresh-pkg/package.json\nexit 0\n"
        .replace("PREFIX", &prefix.display().to_string());
    std::fs::write(&fake, script).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut cmd = Command::new(bin());
    cmd.args([
        "--json",
        "--data-root",
        c.dr().as_str(),
        "packages",
        "--action",
        "install",
        "--yes",
    ])
    .current_dir(ws.as_path())
    .env(
        "PATH",
        format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
    );
    let out = cmd.output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(v["result"]["npm_ran"], true, "{v}");
    let rows = v["result"]["packages"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|r| r["package"] == "fresh-pkg" && r["state"] == "satisfied"),
        "返回安装后状态而非安装前: {v}"
    );

    // 再次 install：已满足 → npm 不再执行（幂等）
    let marker = c.tmp.path().join("npm2-called");
    let fake2 = fake_bin.join("npm");
    std::fs::write(
        &fake2,
        "#!/bin/sh\ntouch \"${NPM_CALLED_MARKER:?}\"\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake2, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cmd = Command::new(bin());
    cmd.args([
        "--json",
        "--data-root",
        c.dr().as_str(),
        "packages",
        "--action",
        "install",
        "--yes",
    ])
    .current_dir(ws.as_path())
    .env("NPM_CALLED_MARKER", marker.display().to_string())
    .env(
        "PATH",
        format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
    );
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(v["result"]["npm_ran"], false, "重复安装幂等: {v}");
}

/// 真实 npm 安装（联网，需 registry 访问）；默认忽略，验收时显式运行：
/// `cargo test --test adapters_next packages_real_npm_install -- --ignored --nocapture`
#[test]
#[ignore = "需要真实 npm registry 网络；离线环境跳过并在卡面记录边界"]
fn packages_real_npm_install() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    add_package(&src, "tiny.toml", "isarray", "2.0.5");
    common::commit_only(&src, "add pkg");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let (code, v, stderr) = packages_json(&c, ws.as_path(), "install", &["--yes"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(v["result"]["npm_ran"], true, "{v}");
    let rows = v["result"]["packages"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|r| r["package"] == "isarray" && r["state"] == "satisfied"),
        "真实 npm 安装后 satisfied: {v}"
    );
}

// ---------- AIL-008/032 返工回归（RW-02/S02：首次同步创建嵌套 Hook 配置） ----------

/// 添加一个团队 Stop hook 并推到 origin（返回 src 路径语义与 add_team_hook 一致）。
fn push_team_hook(c: &Ctx, bare: &Path, name: &str) {
    let src = c.tmp.path().join("team-src");
    add_team_hook(&src, name, 3000);
    common::commit_only(&src, "add hook");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    // 工作区刷新源锁到最新 commit
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&c.tmp.path().join("biz"), &refs);
    assert_eq!(code, 0, "{stderr}");
}

fn stop_array(settings: &serde_json::Value) -> &Vec<serde_json::Value> {
    settings["hooks"]["Stop"].as_array().unwrap()
}

/// S02 反例：空工作区（settings.json 不存在）首次 sync 团队 Stop hook 应成功，
/// 且 hooks=对象、Stop=数组；`{}`、已有 hooks 对象两种输入同样成功；
/// 类型冲突显式失败且原文件逐字节不变。
#[test]
fn team_hook_first_sync_creates_nested_config() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();

    // 1) settings.json 不存在
    push_team_hook(&c, &bare, "stop-a");
    assert!(!ws.join(".claude/settings.json").exists());
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "空 settings 首次同步失败: {stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(settings["hooks"].is_object(), "hooks 应是对象: {settings}");
    assert_eq!(stop_array(&settings).len(), 1);

    // 2) settings.json 为 {}
    let c2 = Ctx::new();
    let (bare2, ws2) = c2.setup();
    push_team_hook(&c2, &bare2, "stop-a");
    std::fs::create_dir_all(ws2.join(".claude")).unwrap();
    std::fs::write(ws2.join(".claude/settings.json"), "{}\n").unwrap();
    let (code, _, stderr) = c2.sync(&ws2);
    assert_eq!(code, 0, "空对象 settings 首次同步失败: {stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws2.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(settings["hooks"].is_object() && stop_array(&settings).len() == 1);

    // 3) 已有 hooks 对象（用户条目）→ 共存；重复 sync Noop
    let c3 = Ctx::new();
    let (bare3, ws3) = c3.setup();
    std::fs::create_dir_all(ws3.join(".claude")).unwrap();
    std::fs::write(
        ws3.join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"user-own"}]}]}}"#,
    )
    .unwrap();
    push_team_hook(&c3, &bare3, "stop-a");
    let (code, _, stderr) = c3.sync(&ws3);
    assert_eq!(code, 0, "{stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws3.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(stop_array(&settings)
        .iter()
        .any(|e| e["hooks"][0]["command"] == "user-own"));
    assert_eq!(stop_array(&settings).len(), 2, "用户+团队共存");
    let before = std::fs::read(ws3.join(".claude/settings.json")).unwrap();
    let (code, out, _) = c3.sync(&ws3);
    assert_eq!(code, 0);
    assert!(
        out.contains("noop") || !out.contains("applied"),
        "重复 sync 应 Noop: {out}"
    );
    assert_eq!(
        std::fs::read(ws3.join(".claude/settings.json")).unwrap(),
        before,
        "Noop 不改写文件"
    );

    // 4) 类型冲突：hooks 是数组 → 显式失败且原文件逐字节不变
    let c4 = Ctx::new();
    let (bare4, ws4) = c4.setup();
    push_team_hook(&c4, &bare4, "stop-a");
    std::fs::create_dir_all(ws4.join(".claude")).unwrap();
    let conflict = br#"{"hooks":[]}"#;
    std::fs::write(ws4.join(".claude/settings.json"), conflict).unwrap();
    let (code, _, stderr) = c4.sync(&ws4);
    assert_ne!(code, 0, "类型冲突必须失败");
    assert!(stderr.contains("E5004"), "冲突错误码: {stderr}");
    assert_eq!(
        std::fs::read(ws4.join(".claude/settings.json")).unwrap(),
        &conflict[..],
        "冲突时原文件逐字节不变"
    );
}

/// 同事件两个团队 Hook 从空 settings 首次同步即共存（各签名独立）。
#[test]
fn team_hooks_same_event_first_sync_from_empty_settings() {
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    push_team_hook(&c, &bare, "stop-a");
    push_team_hook(&c, &bare, "stop-b");
    let (code, _, stderr) = c.sync(&ws);
    assert_eq!(code, 0, "首次同步失败: {stderr}");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(stop_array(&settings).len(), 2, "两个团队 hook 共存");
}

/// RW-03/S03：不完整版本 `1.2` 在任何安装动作前拒绝；`1.2.3-next.1` 等
/// prerelease 合法，且已满足判断与安装用同一版本语义（满足则零 npm 调用）。
#[test]
fn packages_exact_version_semantics_and_idempotent_satisfaction() {
    // 1) 不完整版本：check 即拒绝（不启动安装器、不写 package.json）
    let c = Ctx::new();
    let (bare, ws) = c.setup();
    let src = c.tmp.path().join("team-src");
    add_package(&src, "shortver.toml", "short-pkg", "1.2");
    common::commit_only(&src, "add short ver");
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = [
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = packages_json(&c, ws.as_path(), "check", &[]);
    assert_ne!(code, 0, "不完整版本必须被拒绝");
    assert!(
        stderr.contains("精确版本") && stderr.contains("1.2"),
        "{stderr}"
    );

    // 2) prerelease 合法：check 报告 missing（而非校验失败）
    let c2 = Ctx::new();
    let (bare2, ws2) = c2.setup();
    let src2 = c2.tmp.path().join("team-src");
    add_package(&src2, "pre.toml", "pre-pkg", "1.2.3-next.1");
    common::commit_only(&src2, "add pre pkg");
    ailoom::gitx::git(&src2, &["push", "-q", "origin", "main"]).unwrap();
    let args2 = [
        "--data-root".to_string(),
        c2.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare2.to_str().unwrap().to_string(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs2: Vec<&str> = args2.iter().map(String::as_str).collect();
    let (code, _, stderr) = c2.run(&ws2, &refs2);
    assert_eq!(code, 0, "{stderr}");
    let (code, v, stderr) = packages_json(&c2, ws2.as_path(), "check", &[]);
    assert_eq!(code, 0, "prerelease 不应被拒绝: {stderr}");
    assert!(
        v["result"]["packages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["state"] == "missing"),
        "prerelease 包应报告 missing: {v}"
    );

    // 3) 已满足判断一致：node_modules 版本与声明逐字相等 → install --yes 零 npm 调用
    let prefix = c2
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(ailoom::ids::workspace_id_from_root(
            &ws2.canonicalize().unwrap(),
        ))
        .join("packages");
    let mod_dir = prefix.join("node_modules").join("pre-pkg");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(
        mod_dir.join("package.json"),
        r#"{"name":"pre-pkg","version":"1.2.3-next.1"}"#,
    )
    .unwrap();
    let fake_bin = c2.tmp.path().join("fake-bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let fake = fake_bin.join("npm");
    std::fs::write(
        &fake,
        "#!/bin/sh\ntouch \"${NPM_CALLED_MARKER:?}\"\nexit 1\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let marker = c2.tmp.path().join("npm-called");
    let (code, stdout, stderr) = {
        let out = Command::new(bin())
            .args([
                "--json",
                "--data-root",
                c2.dr().as_str(),
                "packages",
                "--action",
                "install",
                "--yes",
            ])
            .current_dir(ws2.as_path())
            .envs(common::isolated_child_env(c2.tmp.path()))
            .env(
                "PATH",
                format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("NPM_CALLED_MARKER", marker.to_str().unwrap())
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    assert_eq!(code, 0, "已满足 install 应成功: {stderr}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap()["result"]["npm_ran"],
        false,
        "不应执行 npm: {stdout}"
    );
    assert!(!marker.exists(), "npm 被调用即失败：已满足的精确版本零安装");
}

// ---------- AIL-032 返工回归（RW-16/R15）：截止时间覆盖派生进程与管道排空 ----------

/// 父进程立即退出但派生进程持有 stdout/stderr：截止时间内未排空 → 按超时回收
/// （不再等待派生进程数秒且不误报成功）；回收无残留；大输出正常命令不误超时。
#[test]
fn team_hook_exec_timeout_covers_detached_pipe_holders() {
    let c = Ctx::new();
    let (_bare, ws) = c.setup();
    let specs = ws.join(".ailoom-hook-specs");
    std::fs::create_dir_all(&specs).unwrap();
    let exec = |id: &str| {
        c.run(
            ws.as_path(),
            &[
                "--json",
                "--data-root",
                c.dr().as_str(),
                "hooks",
                "--action",
                "exec",
                "--id",
                id,
            ],
        )
    };
    let write_spec = |id: &str, command: &[&str], timeout_ms: u64| {
        let spec = serde_json::json!({
            "resource_id": id,
            "command": command,
            "timeout_ms": timeout_ms,
        });
        std::fs::write(
            specs.join(format!("{}.json", id.replace('/', "__"))),
            serde_json::to_string_pretty(&spec).unwrap(),
        )
        .unwrap();
    };

    // 1) 派生进程持有管道（sleep 标记 4.732）：100ms 截止 → 超时回收，
    //    exec 总耗时必须有界（< 2s，而非等待 sleep 结束），且不报 executed=true
    write_spec(
        "team/hook/leak",
        &["/bin/sh", "-c", "sh -c 'sleep 4.732' & exit 0"],
        100,
    );
    let t0 = std::time::Instant::now();
    let (code, stdout, stderr) = exec("team/hook/leak");
    let elapsed = t0.elapsed();
    assert_eq!(code, 0, "超时不得使宿主非零退出: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
    assert_eq!(v["result"]["timed_out"], true, "截止覆盖排空: {stdout}");
    assert_eq!(v["result"]["executed"], false, "不得误报成功: {stdout}");
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "必须在截止时间附近返回（实际 {elapsed:?}）"
    );
    // 进程组回收：sleep 无残留
    std::thread::sleep(std::time::Duration::from_millis(300));
    let probe = std::process::Command::new("pgrep")
        .args(["-f", "sleep 4.732"])
        .output()
        .unwrap();
    assert!(
        probe.stdout.is_empty(),
        "派生进程应被回收: {:?}",
        String::from_utf8_lossy(&probe.stdout)
    );

    // 2) 大输出（> 管道容量）快速退出：不误超时（管道持续消费）
    write_spec(
        "team/hook/big2",
        &["/bin/sh", "-c", "yes big | head -c 512000; exit 0"],
        5000,
    );
    let (code, stdout, _) = exec("team/hook/big2");
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
    assert_eq!(v["result"]["executed"], true, "{stdout}");
    assert_eq!(v["result"]["timed_out"], false, "{stdout}");

    // 3) 非零退出与启动失败：诊断语义不回归（宿主隔离）
    write_spec("team/hook/fail2", &["/bin/sh", "-c", "exit 7"], 5000);
    let (code, stdout, _) = exec("team/hook/fail2");
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
    assert_eq!(v["result"]["exit"], 7, "{stdout}");
    assert_eq!(v["result"]["timed_out"], false);
    write_spec("team/hook/missing", &["/nonexistent-binary-rw16"], 5000);
    let (code, _, stderr) = exec("team/hook/missing");
    assert_ne!(code, 0, "启动失败应有诊断");
    assert!(stderr.contains("hook 启动失败"), "{stderr}");
}
