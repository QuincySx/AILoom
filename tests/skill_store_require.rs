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
    // AIL-002：合法 XDG 覆盖行为保留——未设 AILOOM_STORE_ROOT 时，
    // Store 根按契约落 $XDG_DATA_HOME/ailoom/store（受控 fixture 内）。
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
