//! AIL-044 集成验收：按资源与宿主三态选择 + 有效配置解释 + 个人模式 sync。
//! 默认开启→子项目禁用/增加→worktree 覆盖；输出每一项来源；
//! 个人层不改变团队模式行为；无团队源、无网络可完成。

mod common;

use std::path::Path;
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

use std::path::PathBuf;

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
    fn run_json(&self, cwd: &Path, args: &[&str]) -> serde_json::Value {
        let (code, out, stderr) = self.run(cwd, args);
        assert_eq!(code, 0, "命令失败: {stderr}");
        let v: serde_json::Value =
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("JSON 解析失败 ({e}): {out}"));
        v["result"].clone()
    }
    fn dr(&self) -> String {
        self.tmp.path().join("data").to_string_lossy().to_string()
    }
}

fn make_repo(c: &Ctx, name: &str) -> PathBuf {
    let repo = c.tmp.path().join(name);
    std::fs::create_dir_all(&repo).unwrap();
    let mut cmd = Command::new("git");
    cmd.args(["init", "-q"]).current_dir(&repo);
    let st = cmd.status().unwrap();
    assert!(st.success());
    std::fs::write(repo.join("README.md"), "demo").unwrap();
    repo
}

fn import_skill(c: &Ctx, repo: &Path, name: &str) {
    let src = c.tmp.path().join("skills").join(name);
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        format!("# {name}\n\n{name} 的说明正文。\n"),
    )
    .unwrap();
    let dr = c.dr();
    let dir_arg = src.to_string_lossy().to_string();
    let (code, _, stderr) = c.run(
        repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import",
            "--dir",
            &dir_arg,
            "--execute",
        ],
    );
    assert_eq!(code, 0, "import {name}: {stderr}");
}

/// 无团队源、无网络：选宿主 → 库 skill 启用 → sync 部署 → 子项目禁用/增补 → worktree 覆盖。
#[test]
fn ail044_personal_selection_layers_end_to_end() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo");
    std::fs::create_dir_all(repo.join("web")).unwrap();
    import_skill(&c, &repo, "skill-a");
    import_skill(&c, &repo, "skill-b");
    let dr = c.dr();

    // 发现 + 选宿主（仓库默认）
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    let v = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--host",
            "claude",
            "--state",
            "enable",
        ],
    );
    assert_eq!(v["state"], serde_json::json!("enable"));

    // 仓库默认启用 skill-a；skill-b 未设置
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/skill-a",
            "--state",
            "enable",
        ],
    );
    let eff = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    let ra = &eff["resources"]["personal/skill/personal/skill-a"];
    assert_eq!(ra["deployed"], serde_json::json!(true));
    assert_eq!(
        ra["origin"],
        serde_json::json!("repo_default"),
        "来源可解释"
    );
    assert_eq!(eff["hosts"]["claude"]["enabled"], serde_json::json!(true));

    // plan 无写入；sync 部署到当前 worktree
    let (code, _, stderr) = c.run(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(
        repo.join(".claude/skills/skill-a").exists(),
        "skill-a 已部署"
    );
    assert!(
        !repo.join(".claude/skills/skill-b").exists(),
        "未设置不部署"
    );
    assert!(
        !repo.join(".agents/skills/skill-a").exists(),
        "仅启用的 claude 宿主部署"
    );

    // 子项目模板：web 下禁用 skill-a、启用 skill-b
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/skill-a",
            "--state",
            "disable",
            "--subproject",
            "web",
        ],
    );
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/skill-b",
            "--state",
            "enable",
            "--subproject",
            "web",
        ],
    );
    let eff_web = c.run_json(
        &repo.join("web"),
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    assert_eq!(
        eff_web["active_rel"],
        serde_json::json!("web"),
        "cwd 决定作用域"
    );
    assert_eq!(
        eff_web["resources"]["personal/skill/personal/skill-a"]["deployed"],
        serde_json::json!(false)
    );
    assert_eq!(
        eff_web["resources"]["personal/skill/personal/skill-a"]["origin"],
        serde_json::json!({"repo_subproject": {"path": "web"}})
    );
    assert_eq!(
        eff_web["resources"]["personal/skill/personal/skill-b"]["deployed"],
        serde_json::json!(true)
    );

    // 在 web 上下文 sync：目标仍是 worktree 根（ADR：worktree 是落点，子项目只影响选择）
    let _ = c.run_json(
        &repo.join("web"),
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert!(
        repo.join(".claude/skills/skill-b").exists(),
        "web 上下文部署 skill-b"
    );
    assert!(
        !repo.join(".claude/skills/skill-a").exists(),
        "web 上下文移除 skill-a"
    );

    // 其他 worktree 覆盖：不改变主 worktree 的文件
    let wt2 = c.tmp.path().join("wt2");
    let mut git = Command::new("git");
    git.args(["worktree", "add", "-q", wt2.to_str().unwrap(), "-b", "wt2"])
        .current_dir(&repo);
    assert!(git.status().unwrap().success());
    let eff_wt2 = c.run_json(
        &wt2,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    assert_ne!(
        eff_wt2["worktree_id"], eff["worktree_id"],
        "两个 worktree 是不同作用域"
    );
    assert_eq!(
        eff_wt2["resources"]["personal/skill/personal/skill-a"]["deployed"],
        serde_json::json!(true),
        "wt2 无子项目上下文 → 仓库默认启用"
    );
    assert!(
        repo.join(".claude/skills/skill-b").exists(),
        "主 worktree 不被 wt2 影响"
    );

    // wt2 显式禁用 + sync → 只影响 wt2
    let _ = c.run_json(
        &wt2,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/skill-a",
            "--state",
            "disable",
            "--worktree",
        ],
    );
    let _ = c.run_json(
        &wt2,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert!(
        !wt2.join(".claude/skills/skill-a").exists(),
        "wt2 覆盖禁用生效"
    );
    assert!(
        repo.join(".claude/skills/skill-b").exists(),
        "主 worktree 文件不受影响"
    );
}

/// 个人层叠加在团队声明之上：禁用团队 skill → personal sync 移除；
/// 团队模式 sync 不受个人层污染（角色∪项目选择与 learning 隔离仍成立）。
#[test]
fn ail044_personal_overlay_on_team_declaration_and_isolation() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let src = c.tmp.path().join("src");
    let url = common::file_url(&common::make_team_source_full(&src));
    let dr = c.dr();
    let (code, _, stderr) = c.run(
        &ws,
        &[
            "--data-root",
            &dr,
            "init",
            "--url",
            &url,
            "--project",
            "a",
            "--role",
            "dev",
        ],
    );
    assert_eq!(code, 0, "init: {stderr}");
    let (code, _, stderr) = c.run(&ws, &["--data-root", &dr, "sync"]);
    assert_eq!(code, 0, "team sync: {stderr}");
    assert!(ws.join(".claude/skills/a-deploy").exists());

    // 个人层：禁用团队 skill（full ResourceId）
    let _ = c.run_json(
        &ws,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    let _ = c.run_json(
        &ws,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "team/skill/team-lib/a-deploy",
            "--state",
            "disable",
        ],
    );
    let v = c.run_json(
        &ws,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true), "{v}");
    assert!(
        !ws.join(".claude/skills/a-deploy").exists(),
        "个人禁用移除团队部署（过期清理）"
    );
    let eff = c.run_json(
        &ws,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    assert_eq!(
        eff["resources"]["team/skill/team-lib/a-deploy"]["origin"],
        serde_json::json!("repo_default"),
        "个人禁用来源可解释"
    );

    // 团队模式 sync 不受个人层污染：仍按团队语义部署（个人层只在 personal sync 生效）
    let (code, _, stderr) = c.run(&ws, &["--data-root", &dr, "sync"]);
    assert_eq!(code, 0, "team sync again: {stderr}");
    assert!(
        ws.join(".claude/skills/a-deploy").exists(),
        "团队模式恢复部署"
    );

    // personal sync 再次禁用（幂等、可重复）
    let v = c.run_json(
        &ws,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(!ws.join(".claude/skills/a-deploy").exists());

    // 未设置的团队资源不受影响
    assert!(
        ws.join(".claude/skills/common-greet").exists(),
        "未禁用资源保持团队部署"
    );
    // learning 隔离不受个人选择影响（learning 本就不部署，可从 effective 缺席验证）
    assert!(
        !eff["resources"]
            .as_object()
            .unwrap()
            .keys()
            .any(|k| k.contains("learning")),
        "learning 不进入部署期望"
    );
}

/// 个人指令 CLI：保存→sync→公司文件不动→exclude 登记生效。
#[test]
fn ail044_instructions_and_exclude_via_cli() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo2");
    std::fs::write(repo.join("AGENTS.md"), "# 公司规范\n\n- 用英文\n").unwrap();
    std::fs::write(repo.join("s.txt"), "s").unwrap();
    let dr = c.dr();
    // 提交基线
    let mut git = Command::new("git");
    git.args([
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
        "add",
        ".",
    ])
    .current_dir(&repo);
    assert!(git.status().unwrap().success());
    let mut git = Command::new("git");
    git.args([
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "base",
    ])
    .current_dir(&repo);
    assert!(git.status().unwrap().success());
    let status_before = {
        let out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // 个人指令（工作树级）
    let instr = c.tmp.path().join("instr.md");
    std::fs::write(&instr, "- 个人：回答用中文\n").unwrap();
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    let v = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "instructions",
            "--file",
            instr.to_str().unwrap(),
            "--worktree",
        ],
    );
    assert_eq!(v["saved"], serde_json::json!(true));
    // 宿主未选 → 指令不部署（需要至少一个宿主启用）
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--host",
            "claude",
            "--state",
            "enable",
        ],
    );
    let v = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true), "{v}");
    assert!(
        repo.join(".claude/rules/ailoom-personal.md").is_file(),
        "Claude 个人条目落盘"
    );
    let agents = std::fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("用英文") && !agents.contains("个人"),
        "公司 AGENTS.md 不变"
    );

    // git 状态只有 AILoom 新增文件（被 exclude 收录，status 不显示新增）
    let status_after = {
        let out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert_eq!(
        status_before, status_after,
        "个人产物经 info/exclude 不污染 git status；公司文件零改动"
    );
}

/// AIL-051：子项目局部关闭后恢复继承（state=inherit 回到仓库默认/团队层值）。
#[test]
fn ail051_restore_inheritance_via_inherit_state() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo3");
    std::fs::create_dir_all(repo.join("web")).unwrap();
    import_skill(&c, &repo, "restore-flow");
    let dr = c.dr();
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--host",
            "claude",
            "--state",
            "enable",
        ],
    );
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/restore-flow",
            "--state",
            "enable",
        ],
    );
    // 子项目局部禁用
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/restore-flow",
            "--state",
            "disable",
            "--subproject",
            "web",
        ],
    );
    let eff = c.run_json(
        &repo.join("web"),
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    assert_eq!(
        eff["resources"]["personal/skill/personal/restore-flow"]["deployed"],
        serde_json::json!(false)
    );
    // 恢复继承：inherit 清除本层表态效果，回到仓库默认启用
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/restore-flow",
            "--state",
            "inherit",
            "--subproject",
            "web",
        ],
    );
    let eff2 = c.run_json(
        &repo.join("web"),
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    let r = &eff2["resources"]["personal/skill/personal/restore-flow"];
    assert_eq!(
        r["deployed"],
        serde_json::json!(true),
        "恢复继承后回到仓库默认值: {r}"
    );
    assert_eq!(r["origin"], serde_json::json!("repo_default"));
}
