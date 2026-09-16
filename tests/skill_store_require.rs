//! SkillStore 软链 + Require + 可观测（specs/0001 + tickets 01–04）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

struct Ctx {
    tmp: tempfile::TempDir,
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
    fn store_root(&self) -> PathBuf {
        common::isolated_store_root(self.tmp.path())
    }
}

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

fn init_sync(c: &Ctx, ws: &Path) {
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    let url = common::file_url(&src);
    let dr = c.dr();
    let (code, _, stderr) = c.run(
        ws,
        &[
            "--data-root",
            dr.as_str(),
            "init",
            "--url",
            &url,
            "--project",
            "a",
            "--role",
            "dev",
            "--no-builtin",
        ],
    );
    assert_eq!(code, 0, "init: {stderr}");
    let (code, _, stderr) = c.run(ws, &["--data-root", dr.as_str(), "sync"]);
    assert_eq!(code, 0, "sync: {stderr}");
}

fn patch_require(ws: &Path, body: &str) {
    let path = ws.join(".ailoom/project.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let text = if text.contains("[require]") {
        // replace existing require section roughly: append wins via rewrite
        let before = text.split("[require]").next().unwrap().trim_end();
        format!("{before}\n\n{body}\n")
    } else {
        format!("{}\n\n{body}\n", text.trim_end())
    };
    std::fs::write(&path, text).unwrap();
}

#[test]
fn ticket01_store_symlink_layout_and_idempotent_sync() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_sync(&c, &ws);

    let link = ws.join(".claude/skills/a-deploy");
    assert!(link.is_dir() || link.exists(), "skill 可见");
    #[cfg(unix)]
    {
        assert!(
            link.symlink_metadata().unwrap().file_type().is_symlink(),
            "应为 symlink"
        );
        let target = std::fs::read_link(&link).unwrap();
        let t = target.to_string_lossy();
        assert!(
            target.starts_with(c.store_root()),
            "{t} 应位于 {:?} 下",
            c.store_root()
        );
        assert!(!t.contains("/sources/"), "{t}");
        assert!(!t.contains("resources/skills"), "{t}");
        assert!(target.join("SKILL.md").is_file(), "实体可读");
        assert!(
            c.store_root().read_dir().unwrap().any(|e| e
                .unwrap()
                .path()
                .join(".meta")
                .join("SOURCE.json")
                .is_file()),
            "应有 .meta/SOURCE.json"
        );
    }

    let dr = c.dr();
    let (code, _, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "sync"]);
    assert_eq!(code, 0, "second sync: {stderr}");
    #[cfg(unix)]
    {
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(std::fs::read_link(&link)
            .unwrap()
            .join("SKILL.md")
            .is_file());
    }
}

#[test]
fn ticket02_require_filters_and_unknown_fails() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_sync(&c, &ws);
    assert!(ws.join(".claude/skills/a-deploy").exists());
    assert!(ws.join(".claude/skills/common-greet").exists());

    patch_require(
        &ws,
        r#"[require]
skills = ["common-greet"]
"#,
    );
    let dr = c.dr();
    let (code, _, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "sync"]);
    assert_eq!(code, 0, "require sync: {stderr}");
    assert!(ws.join(".claude/skills/common-greet").exists());
    assert!(
        !ws.join(".claude/skills/a-deploy").exists(),
        "未 Require 的 skill 应被拆除"
    );

    patch_require(
        &ws,
        r#"[require]
skills = ["does-not-exist-skill"]
"#,
    );
    let (code, out, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "sync"]);
    assert_ne!(code, 0, "unknown require must fail");
    let blob = format!("{out}{stderr}");
    assert!(
        blob.contains("E3004") || blob.contains("未知"),
        "expect E3004: {blob}"
    );
}

#[test]
fn ticket03_agent_persona_narrows_require() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_sync(&c, &ws);

    std::fs::create_dir_all(ws.join(".ailoom/agents")).unwrap();
    std::fs::write(
        ws.join(".ailoom/agents/inker.toml"),
        "skills = [\"common-greet\"]\n",
    )
    .unwrap();
    patch_require(
        &ws,
        r#"[require]
skills = ["common-greet", "a-deploy"]
agent = "inker"
"#,
    );
    let dr = c.dr();
    let (code, _, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "sync"]);
    assert_eq!(code, 0, "agent require: {stderr}");
    assert!(ws.join(".claude/skills/common-greet").exists());
    assert!(
        !ws.join(".claude/skills/a-deploy").exists(),
        "人设应收窄掉 a-deploy"
    );

    // 人设引用未在 Workspace Require 中的 skill
    std::fs::write(
        ws.join(".ailoom/agents/inker.toml"),
        "skills = [\"common-greet\", \"dev-tooling\"]\n",
    )
    .unwrap();
    // dev-tooling may exist in fixture - if it's in require list we're fine; use invented
    std::fs::write(
        ws.join(".ailoom/agents/inker.toml"),
        "skills = [\"not-in-workspace-require\"]\n",
    )
    .unwrap();
    let (code, out, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "sync"]);
    assert_ne!(code, 0);
    let blob = format!("{out}{stderr}");
    assert!(
        blob.contains("E3004") || blob.contains("未在 Workspace Require"),
        "{blob}"
    );
}

#[test]
fn ticket04_doctor_broken_link_and_uninstall_keeps_store() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_sync(&c, &ws);

    let link = ws.join(".claude/skills/a-deploy");
    #[cfg(unix)]
    let store_target = std::fs::read_link(&link).unwrap();
    #[cfg(unix)]
    {
        assert!(store_target.join("SKILL.md").is_file());
        // 破坏：删除 store 实体目录
        std::fs::remove_dir_all(&store_target).unwrap();
    }

    let dr = c.dr();
    let (code, out, _) = c.run(&ws, &["--data-root", dr.as_str(), "--json", "doctor"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("skill-store-links") && out.contains("false")
            || out.contains("store-target-missing"),
        "doctor should flag broken store link: {out}"
    );

    let (code, out, _) = c.run(&ws, &["--data-root", dr.as_str(), "--json", "status"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("Skill 链接异常") || out.contains("skill-store-link"),
        "status issues: {out}"
    );

    // 恢复实体后再卸载：store 应保留
    #[cfg(unix)]
    {
        let (code, _, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "sync"]);
        assert_eq!(code, 0, "repair sync: {stderr}");
        let target = std::fs::read_link(&link).unwrap();
        assert!(target.join("SKILL.md").is_file());
        let (code, _, stderr) = c.run(&ws, &["--data-root", dr.as_str(), "uninstall", "--execute"]);
        assert_eq!(code, 0, "uninstall: {stderr}");
        assert!(!link.exists(), "工作区链接应消失");
        assert!(target.join("SKILL.md").is_file(), "SkillStore 实体应保留");
    }
}

#[test]
fn xdg_data_home_store_layout_is_respected() {
    // AIL-002：合法 XDG 覆盖行为保留——契约 v1.1 下 XDG 优先于 AILOOM_*，
    // Store 根落 $XDG_DATA_HOME/ailoom/store（受控 fixture 内）。
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    let url = common::file_url(&src);
    let dr = c.dr();
    let xdg_data = c.tmp.path().join("xdg-data");

    let spawn = |args: &[&str]| {
        let mut cmd = Command::new(bin());
        cmd.args(args).current_dir(&ws);
        for (k, v) in common::isolated_child_env(c.tmp.path()) {
            if k == "AILOOM_STORE_ROOT" {
                continue; // 取消最高优先级覆盖，让 XDG_DATA_HOME 生效
            }
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    };

    let out = spawn(&[
        "--data-root",
        dr.as_str(),
        "init",
        "--url",
        &url,
        "--project",
        "a",
        "--role",
        "dev",
        "--no-builtin",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let out = spawn(&["--data-root", dr.as_str(), "sync"]);
    assert_eq!(out.status.code(), Some(0));

    let xdg_store = xdg_data.join("ailoom/store");
    assert!(
        xdg_store.is_dir(),
        "Store 应回落 XDG 布局: {}",
        xdg_store.display()
    );
    #[cfg(unix)]
    {
        let link = ws.join(".claude/skills/common-greet");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        let target = std::fs::read_link(&link).unwrap();
        assert!(
            target.starts_with(&xdg_store),
            "{target:?} 应位于 {xdg_store:?} 下"
        );
        assert!(target.join("SKILL.md").is_file());
    }
}

/// RW-01 迁移用例的子进程环境：不设 XDG/AILOOM_*（空值视为未设），
/// 使默认根解析走 `~/.ailoom` → XDG 规范默认的一次性迁移路径。
fn legacy_env(home: &Path) -> Vec<(String, String)> {
    vec![
        ("HOME".into(), home.to_string_lossy().into_owned()),
        ("USERPROFILE".into(), home.to_string_lossy().into_owned()),
        ("XDG_DATA_HOME".into(), String::new()),
        ("XDG_STATE_HOME".into(), String::new()),
        ("AILOOM_LOG".into(), "error".into()),
    ]
}

fn run_in(cwd: &Path, env: &[(String, String)], args: &[&str]) -> (i32, String, String) {
    let out = Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .envs(env.to_vec())
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// 旧世界部署：AILOOM_* 指向 `~/.ailoom`（等价升级前版本的落盘布局），
/// 返回（legacy 根, 部署时 device-id, SKILL.md 原始字节）。
fn deploy_legacy_world(c: &Ctx, ws: &Path) -> (PathBuf, String, Vec<u8>) {
    let home = c.tmp.path().join("home");
    let legacy = home.join(".ailoom");
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    let url = common::file_url(&src);
    let mut env = legacy_env(&home);
    env.push((
        "AILOOM_DATA_ROOT".into(),
        legacy.to_string_lossy().into_owned(),
    ));
    env.push((
        "AILOOM_STORE_ROOT".into(),
        legacy.join("store").to_string_lossy().into_owned(),
    ));
    let (code, _, stderr) = run_in(
        ws,
        &env,
        &["init", "--url", &url, "--target", "claude", "--no-builtin"],
    );
    assert_eq!(code, 0, "旧世界 init: {stderr}");
    let (code, _, stderr) = run_in(ws, &env, &["sync"]);
    assert_eq!(code, 0, "旧世界 sync: {stderr}");

    let device = std::fs::read_to_string(legacy.join("device-id")).unwrap();
    let skill_bytes = std::fs::read(ws.join(".claude/skills/common-greet/SKILL.md")).unwrap();
    // 模拟安装器 bin 与既有绝对链接
    std::fs::create_dir_all(legacy.join("bin")).unwrap();
    std::fs::write(legacy.join("bin/ailoom"), b"#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        let link = ws.join(".claude/skills/common-greet");
        let target = std::fs::read_link(&link).unwrap();
        assert!(
            target.starts_with(&legacy),
            "阶段1 链接应指向旧默认：{target:?}"
        );
    }
    (legacy, device, skill_bytes)
}

/// RW-01/S01 回归：默认根迁移后，未重新 sync 的工作区绝对链接仍可达、
/// 内容不变；device-id 连续；bin 等非契约内容原地保留；显式 XDG 优先级不回归。
#[test]
fn legacy_default_migration_keeps_links_and_device_identity() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let home = c.tmp.path().join("home");
    let (legacy, old_device, skill_bytes) = deploy_legacy_world(&c, &ws);

    // 升级：仅 HOME，运行只读 status 触发迁移
    let up = legacy_env(&home);
    let (code, out, stderr) = run_in(&ws, &up, &["status"]);
    assert_eq!(code, 0, "status: {stderr}");
    assert!(
        !out.contains("store-target-missing") && !stderr.contains("store-target-missing"),
        "迁移后链接不应断裂: {out}{stderr}"
    );

    // 既有绝对链接逐字节可读，且链接目标未被改写（未重新 sync）
    assert_eq!(
        std::fs::read(ws.join(".claude/skills/common-greet/SKILL.md")).unwrap(),
        skill_bytes,
        "SKILL.md 内容不变"
    );
    #[cfg(unix)]
    {
        let link = ws.join(".claude/skills/common-greet");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(
            std::fs::read_link(&link).unwrap().starts_with(&legacy),
            "旧链接目标不因迁移改写（未重新 sync）"
        );
        assert!(
            c.tmp
                .path()
                .join("home/.ailoom/store")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "旧 store 位置留兼容链接"
        );
    }
    // 设备身份连续
    let new_device = std::fs::read_to_string(home.join(".local/state/ailoom/device-id")).unwrap();
    assert_eq!(new_device, old_device, "迁移前后 device-id 不变");
    // bin 原地保留；幂等：重复运行稳定
    assert!(legacy.join("bin/ailoom").is_file(), "bin 不被移动");
    let (code, _, stderr) = run_in(&ws, &up, &["status"]);
    assert_eq!(code, 0, "幂等 status: {stderr}");
    assert_eq!(
        std::fs::read(ws.join(".claude/skills/common-greet/SKILL.md")).unwrap(),
        skill_bytes
    );

    // 已设 XDG 优先级不回归：XDG_DATA_HOME 存储根优先，旧目录不再被触碰
    let xdg_data = c.tmp.path().join("xdg-data");
    let mut xdg_env = legacy_env(&home);
    xdg_env.push((
        "XDG_DATA_HOME".into(),
        xdg_data.to_string_lossy().into_owned(),
    ));
    let (code, _, stderr) = run_in(&ws, &xdg_env, &["status"]);
    assert_eq!(code, 0, "XDG 优先 status: {stderr}");
    assert!(
        !xdg_data.join("ailoom/store").exists(),
        "只读 status 不应创建新 store"
    );
}

/// RW-01：迁移中段失败 → 回退旧根且无半迁移残留；下一进程重试完整迁移；
/// 不误选空新根。
#[test]
fn legacy_migration_midway_failure_rolls_back_and_retry_completes() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let home = c.tmp.path().join("home");
    let (legacy, old_device, skill_bytes) = deploy_legacy_world(&c, &ws);

    // 注入：~/.local/state 为文件 → store 移动后 ws 步骤必然失败
    std::fs::create_dir_all(home.join(".local")).unwrap();
    std::fs::write(home.join(".local/state"), b"blocker").unwrap();
    let up = legacy_env(&home);
    let (code, _, _) = run_in(&ws, &up, &["status"]);
    assert_eq!(code, 0, "失败回退后 status 仍可用");
    assert_eq!(
        std::fs::read(ws.join(".claude/skills/common-greet/SKILL.md")).unwrap(),
        skill_bytes,
        "回退后旧链接可达"
    );
    let store_meta = std::fs::symlink_metadata(legacy.join("store")).unwrap();
    assert!(
        store_meta.is_dir(),
        "store 完整回滚在旧位置（无兼容链接、无半迁移）"
    );
    assert!(
        !home.join(".local/share/ailoom").exists(),
        "本次新建的空新根已清理"
    );
    assert_eq!(
        std::fs::read_to_string(legacy.join("device-id")).unwrap(),
        old_device
    );

    // 阻断解除，第二个进程重试：完整迁移成功且身份连续
    std::fs::remove_file(home.join(".local/state")).unwrap();
    let (code, _, stderr) = run_in(&ws, &up, &["status"]);
    assert_eq!(code, 0, "重试 status: {stderr}");
    assert_eq!(
        std::fs::read(ws.join(".claude/skills/common-greet/SKILL.md")).unwrap(),
        skill_bytes,
        "重试迁移后旧链接仍可达"
    );
    let new_device = std::fs::read_to_string(home.join(".local/state/ailoom/device-id")).unwrap();
    assert_eq!(new_device, old_device, "重试迁移 device-id 连续");
}
