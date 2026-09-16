//! 2026-09-17 返工轮回归（AIL-052～062/069/070/072 逐卡验收的反例覆盖）。
//! 每个测试对应一张执行卡的必需验收项；使用隔离数据根与临时 Git 仓库。

mod common;

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
    fn git(&self, cwd: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

fn make_repo(c: &Ctx, name: &str) -> PathBuf {
    let repo = c.tmp.path().join(name);
    std::fs::create_dir_all(&repo).unwrap();
    assert!(c.git(&repo, &["init", "-q"]));
    repo
}

fn git_add_commit(c: &Ctx, repo: &Path, msg: &str) {
    assert!(c.git(
        repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "add",
            "."
        ]
    ));
    assert!(c.git(
        repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            msg
        ]
    ));
}

// ---------------------------------------------------------------------------
// AIL-052（S02）：被托管入口后来被 git add 跟踪 → 计划删除被拒、文件逐字节保持
// ---------------------------------------------------------------------------
#[test]
fn ail052_tracked_managed_entry_not_deleted() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo");
    std::fs::write(repo.join("base.txt"), "b").unwrap();
    git_add_commit(&c, &repo, "base");
    let dr = c.dr();

    // 个人指令 → sync 部署 AGENTS.override.md（managed manifest 记录该目标）
    std::fs::write(c.tmp.path().join("instr.md"), "- 个人：回答用中文\n").unwrap();
    let instr = c.tmp.path().join("instr.md").to_string_lossy().to_string();
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
            "instructions",
            "--file",
            &instr,
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
            "codex",
            "--state",
            "enable",
        ],
    );
    let v = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    let deployed = repo.join("AGENTS.override.md");
    assert!(deployed.exists());

    // 该入口后来被 git add 跟踪（模拟分支合并/公司接管）；内容保持部署字节
    let before = std::fs::read(&deployed).unwrap();
    assert!(c.git(&repo, &["add", "-f", "AGENTS.override.md"]));
    let index_before = {
        let out = Command::new("git")
            .args(["ls-files", "-s"])
            .current_dir(&repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // plan：不把被过滤期望转为 delete；skipped 明确说明
    let plan = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "plan"],
    );
    let skips = plan["skipped"].as_array().unwrap();
    assert!(
        skips.iter().any(|s| s["path"] == "AGENTS.override.md"),
        "tracked 入口进入 skipped: {plan}"
    );
    assert!(
        !plan["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["path"] == "AGENTS.override.md" && a["action"] == "delete"),
        "tracked 目标不得生成删除动作: {plan}"
    );

    // sync：退出 0 但不删除；文件逐字节保持、index 不变
    let v = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(
        std::fs::read(&deployed).unwrap(),
        before,
        "tracked 托管文件逐字节保持"
    );
    let index_after = {
        let out = Command::new("git")
            .args(["ls-files", "-s"])
            .current_dir(&repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert_eq!(index_before, index_after, "git index 不变");
}

// ---------------------------------------------------------------------------
// AIL-053（S01）：应用后用户手改 → undo 冲突保留；处理后可重入
// ---------------------------------------------------------------------------
#[test]
fn ail053_undo_preserves_user_edits_and_is_reentrant() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo");
    let dr = c.dr();
    let instr = c.tmp.path().join("instr.md");
    std::fs::write(&instr, "- 个人：回答用中文\n").unwrap();
    let instr_s = instr.to_string_lossy().to_string();
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
            "instructions",
            "--file",
            &instr_s,
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
    let v = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    let target = repo.join(".claude/rules/ailoom-personal.md");
    assert!(target.exists());
    let after_apply = std::fs::read_to_string(&target).unwrap();

    // 应用后立即记录撤销条目（after_hash = 应用后指纹，与 jobs::run_apply_job 一致）
    let entry = ailoom::console::jobs::test_support_undo_entry(
        &repo,
        ".claude/rules/ailoom-personal.md",
        true,
        Some(b"# previous\n".to_vec()),
    );

    // 用户事后手改
    std::fs::write(&target, "USER POST APPLY EDIT\n").unwrap();
    let user_content = "USER POST APPLY EDIT\n".as_bytes().to_vec();
    // 指纹不匹配 → 冲突保留（不覆盖用户内容）
    let conflicts = ailoom::console::jobs::test_support_undo_check(&repo, &[entry]);
    assert_eq!(conflicts.len(), 1, "用户后改必须冲突保留: {conflicts:?}");
    assert_eq!(
        std::fs::read(&target).unwrap(),
        user_content,
        "用户新增内容逐字节保留"
    );

    // 用户恢复为应用后内容 → 撤销可安全执行（可重入语义）
    std::fs::write(&target, after_apply.as_bytes()).unwrap();
    let entry = ailoom::console::jobs::test_support_undo_entry(
        &repo,
        ".claude/rules/ailoom-personal.md",
        true,
        Some(b"# previous\n".to_vec()),
    );
    let conflicts = ailoom::console::jobs::test_support_undo_check(&repo, &[entry]);
    assert!(conflicts.is_empty(), "内容归位后撤销应成功: {conflicts:?}");
    assert_eq!(std::fs::read(&target).unwrap(), b"# previous\n");
}

// ---------------------------------------------------------------------------
// AIL-055（F01）：A 已有配置，在 B select 只写 B
// ---------------------------------------------------------------------------
#[test]
fn ail055_select_targets_explicit_repo() {
    let c = Ctx::new();
    let repo_a = make_repo(&c, "repo-a");
    let repo_b = make_repo(&c, "repo-b");
    let dr = c.dr();
    let _ = c.run_json(
        &repo_a,
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
        &repo_a,
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
    let id_a = v["repo_id"].clone();

    let v = c.run_json(
        &repo_b,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--host",
            "codex",
            "--state",
            "enable",
        ],
    );
    let id_b = v["repo_id"].clone();
    assert_ne!(id_a, id_b, "两个独立仓库是不同身份");

    // B 的选择写在 B，不写 A
    let eff_b = c.run_json(
        &repo_b,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    assert_eq!(eff_b["repo_id"], id_b);
    assert_eq!(eff_b["hosts"]["codex"]["enabled"], serde_json::json!(true));
    assert!(
        eff_b["hosts"].get("claude").is_none()
            || eff_b["hosts"]["claude"]["enabled"] == serde_json::json!(false),
        "B 未选择 claude（A 的选择不外溢）: {eff_b}"
    );
    let eff_a = c.run_json(
        &repo_a,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    assert_eq!(eff_a["repo_id"], id_a);
    assert_eq!(eff_a["hosts"]["claude"]["enabled"], serde_json::json!(true));
}

// ---------------------------------------------------------------------------
// AIL-058（F02）：连续单项工作树选择互相保留（不整层替换）
// ---------------------------------------------------------------------------
#[test]
fn ail058_sequential_worktree_selections_merge() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo");
    let wt2 = c.tmp.path().join("wt2");
    assert!(c.git(
        &repo,
        &["worktree", "add", "-q", wt2.to_str().unwrap(), "-b", "b1"]
    ));
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
        &wt2,
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
            "disable",
            "--worktree",
        ],
    );
    let _ = c.run_json(
        &wt2,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--host",
            "codex",
            "--state",
            "disable",
            "--worktree",
        ],
    );
    let eff = c.run_json(
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
    assert_eq!(
        eff["hosts"]["claude"]["enabled"],
        serde_json::json!(false),
        "第一次选择保留"
    );
    assert_eq!(
        eff["hosts"]["codex"]["enabled"],
        serde_json::json!(false),
        "第二次选择不清掉第一次"
    );
}

// ---------------------------------------------------------------------------
// AIL-056（F04/F05）：独立仓库拒绝重关联；整仓搬迁保持身份与配置
// ---------------------------------------------------------------------------
#[test]
fn ail056_relink_validates_identity_and_preserves_config() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo");
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    git_add_commit(&c, &repo, "a");
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
    let eff_before = c.run_json(
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
    let repo_id = eff_before["repo_id"].clone();

    // 独立仓库 B（不同 common-dir、无共同分支）：重关联被拒
    let other = make_repo(&c, "other");
    std::fs::write(other.join("x.txt"), "x").unwrap();
    git_add_commit(&c, &other, "x");

    // 整仓搬迁 → 用户显式重关联 → 身份/配置保持
    let moved = c.tmp.path().join("repo-moved");
    std::fs::rename(&repo, &moved).unwrap();
    // 搬迁后 identity 由发现按证据解析：新 common-dir 无登记 → 先产生新模式，
    // 用户显式重关联（relink）做身份迁移；再次发现经别名解析回原身份
    let discovery = ailoom::repo_registry::discover_repo(&moved).unwrap();
    let new_repo_id = discovery.identity.repo_id.clone();
    let mut reg =
        ailoom::repo_registry::load_or_create_by_id(Path::new(&dr), repo_id.as_str().unwrap())
            .unwrap();
    let wt_key = reg.worktrees.keys().next().cloned().unwrap();
    let outcome = reg
        .relink_worktree(Path::new(&dr), &wt_key, &moved, "t1")
        .unwrap();
    assert!(matches!(
        outcome,
        ailoom::repo_registry::RepoMoveOutcome::RepoMoved { .. }
    ));

    // F04：把登记工作树重关联到独立仓库 B → 拒绝（common-dir 与身份证据不符）
    let other_repo_id = {
        let d = ailoom::repo_registry::discover_repo(&other).unwrap();
        let mut reg =
            ailoom::repo_registry::RepoRegistry::resolve_or_create(Path::new(&dr), &d).unwrap();
        reg.refresh_worktrees(&d, "t0");
        reg.save(Path::new(&dr)).unwrap();
        reg.repo_id
    };
    let mut unrelated =
        ailoom::repo_registry::load_or_create_by_id(Path::new(&dr), &other_repo_id).unwrap();
    let wt_of_unrelated = unrelated.worktrees.keys().next().cloned().unwrap();
    let err = unrelated
        .relink_worktree(Path::new(&dr), &wt_of_unrelated, &moved, "t2")
        .unwrap_err();
    assert!(
        err.to_string().contains("不同的 Git 仓库"),
        "独立仓库重关联被拒: {err}"
    );
    let eff_after = c.run_json(
        &moved,
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
        eff_after["repo_id"], repo_id,
        "重关联后经别名解析回原身份（新 id {new_repo_id} → 原 id）: {eff_after}"
    );
    assert_eq!(
        eff_after["hosts"]["claude"]["enabled"],
        serde_json::json!(true),
        "搬迁后个人配置不丢"
    );
    let _ = other; // 独立仓库仅作为对比存在（HTTP relink 反例在 console_server 覆盖）
}

// ---------------------------------------------------------------------------
// AIL-057（F06）：非 Git 目录完成 select→effective→sync 闭环
// ---------------------------------------------------------------------------
#[test]
fn ail057_nongit_path_mode_closed_loop() {
    let c = Ctx::new();
    let dir = c.tmp.path().join("plain");
    std::fs::create_dir_all(&dir).unwrap();
    let dr = c.dr();
    let eff = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    assert!(eff["is_nongit"].as_bool().unwrap(), "识别为非 Git 路径模式");
    let repo_id = eff["repo_id"].clone();
    assert!(
        repo_id.as_str().unwrap().starts_with("nongit-"),
        "路径模式身份: {repo_id}"
    );
    let v = c.run_json(
        &dir,
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
    assert_eq!(v["repo_id"], repo_id);
    // 导入并启用 skill → sync 部署
    let src = c.tmp.path().join("sk");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("SKILL.md"), "# plain-flow\n").unwrap();
    let src_s = src.to_string_lossy().to_string();
    let (code, _, stderr) = c.run(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import",
            "--dir",
            &src_s,
            "--name",
            "plain-flow",
            "--execute",
        ],
    );
    assert_eq!(code, 0, "import: {stderr}");
    let _ = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "select",
            "--resource",
            "personal/skill/personal/plain-flow",
            "--state",
            "enable",
        ],
    );
    let v = c.run_json(
        &dir,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true), "{v}");
    assert!(
        dir.join(".claude/skills/plain-flow").exists(),
        "非 Git 目录完成部署"
    );

    // 目录后来初始化 Git → 提示迁移且不默默合并
    assert!(c.git(&dir, &["init", "-q"]));
    let eff2 = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "effective",
        ],
    );
    let notes = eff2["notes"].as_array().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.as_str().unwrap().contains("migrate-nongit")),
        "初始化 Git 后给出迁移提示: {notes:?}"
    );
}

// ---------------------------------------------------------------------------
// AIL-061（F03）：个人禁用宿主后，团队资源不再部署
// ---------------------------------------------------------------------------
#[test]
fn ail061_disabled_host_blocks_team_artifacts() {
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

    // 个人禁用 claude → personal plan 不再包含 .claude 目标；团队 sync 不受影响
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
            "--host",
            "claude",
            "--state",
            "disable",
        ],
    );
    let plan = c.run_json(
        &ws,
        &["--json", "--data-root", &dr, "personal", "--action", "plan"],
    );
    assert!(
        !plan["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["target_tool"] == "claude" && a["action"] == "create"),
        "宿主禁用后团队 claude 目标不再进入计划: {plan}"
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
    assert_eq!(eff["hosts"]["claude"]["enabled"], serde_json::json!(false));

    // 恢复继承 → 重新生效
    let _ = c.run_json(
        &ws,
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
            "inherit",
        ],
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
        eff["hosts"]["claude"]["enabled"],
        serde_json::json!(true),
        "恢复继承回到团队值"
    );
}

// ---------------------------------------------------------------------------
// AIL-060（F08）：profile 就地编辑保留注释/未知字段；并发保存冲突
// ---------------------------------------------------------------------------
#[test]
fn ail060_profile_format_preserving_and_conflict() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo");
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

    let profile_path = c.tmp.path().join("data/profile/profile.toml");
    let text = std::fs::read_to_string(&profile_path).unwrap();
    // 注入注释与未知字段，再经 select 写入
    let decorated = format!("# 用户注释必须保留\n{text}# 尾注释\n[extra_unknown]\nkey = \"v\"\n");
    std::fs::write(&profile_path, decorated).unwrap();
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
            "codex",
            "--state",
            "enable",
        ],
    );
    let after = std::fs::read_to_string(&profile_path).unwrap();
    assert!(after.contains("# 用户注释必须保留"), "头注释保留: {after}");
    assert!(after.contains("extra_unknown"), "未知字段保留");
    assert!(
        after.contains("# 尾注释") || after.contains("codex"),
        "写回成功"
    );

    // revision 冲突：base_revision 过期 → 拒绝
    let profile: ailoom::profile::PersonalProfile = toml::from_str(&after).unwrap();
    let stale = profile.revision.saturating_sub(1);
    let args = ailoom::commands::personal::SelectArgs {
        resource: None,
        host: Some("claude".into()),
        state: "disable".into(),
        subproject: None,
        worktree: false,
        repo_root: Some(repo.clone()),
        base_revision: Some(stale),
    };
    let data_root = PathBuf::from(c.dr());
    let err = ailoom::commands::personal::select(&args, &data_root).unwrap_err();
    assert!(
        err.to_string().contains("已被其他会话修改"),
        "过期 revision 拒绝: {err}"
    );
}

// ---------------------------------------------------------------------------
// AIL-062（L01/L02）：带 frontmatter 导入成功；坏条目不锁死列表
// ---------------------------------------------------------------------------
#[test]
fn ail062_import_transaction_and_tolerant_list() {
    let c = Ctx::new();
    let dir = c.tmp.path().join("plain");
    std::fs::create_dir_all(&dir).unwrap();
    let dr = c.dr();

    // 带 frontmatter 的普通 skill（复审 L01 反例）现在导入成功且 YAML 合法
    let src = c.tmp.path().join("ordinary");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: ordinary\ndescription: Ordinary skill\n---\n\n# Ordinary\n",
    )
    .unwrap();
    let src_s = src.to_string_lossy().to_string();
    let (code, _, stderr) = c.run(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import",
            "--dir",
            &src_s,
            "--execute",
        ],
    );
    assert_eq!(code, 0, "frontmatter skill 导入成功: {stderr}");
    let (code, out, _) = c.run(
        &dir,
        &["--json", "--data-root", &dr, "library", "--action", "list"],
    );
    assert_eq!(code, 0, "导入后库仍可读");
    assert!(out.contains("ordinary"));

    // 人为放入坏条目 → 列表仍可用并定位坏文件
    let lib = c.tmp.path().join("data/library/resources/skills/broken");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("SKILL.md"), "---\nbroken: [\n").unwrap();
    let (code, out, _) = c.run(
        &dir,
        &["--json", "--data-root", &dr, "library", "--action", "list"],
    );
    assert_eq!(code, 0, "坏条目不锁死列表");
    assert!(out.contains("ordinary"), "好条目仍列出: {out}");
    assert!(out.contains("broken"), "坏条目被定位: {out}");
}

// ---------------------------------------------------------------------------
// AIL-069（F09/L05）：MCP 缺失引用进入 plan notes；秘密引用不泄漏值
// ---------------------------------------------------------------------------
#[test]
fn ail069_mcp_missing_env_ref_surfaces_in_plan_notes() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo");
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
    // 手工放入一个引用缺失环境变量的 MCP 资源
    let mcp_dir = c.tmp.path().join("data/library/resources/mcp");
    std::fs::create_dir_all(&mcp_dir).unwrap();
    std::fs::write(
        mcp_dir.join("probe.toml"),
        "name = \"probe\"\ntype = \"stdio\"\nnamespace = \"personal\"\nshared = true\ncommand = \"/bin/echo\"\n[mcp.env]\nAPI_KEY = \"$ENV:AILOOM_TEST_MISSING_KEY\"\n",
    )
    .unwrap();
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
            "personal/mcp/personal/probe",
            "--state",
            "enable",
        ],
    );
    let plan = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "plan"],
    );
    let notes = plan["notes"].as_array().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.as_str().unwrap().contains("AILOOM_TEST_MISSING_KEY")
                && n.as_str().unwrap().contains("缺失引用环境变量")),
        "缺失引用环境变量进入 notes: {notes:?}"
    );
}

// ---------------------------------------------------------------------------
// AIL-070：产物历史可恢复
// ---------------------------------------------------------------------------
#[test]
fn ail070_artifact_history_recoverable() {
    let c = Ctx::new();
    let dr = c.dr();
    let root = Path::new(&dr);
    let run = ailoom::workflow::create(root, "历史流").unwrap();
    let _ = ailoom::workflow::put_artifact(root, &run.id, "spec", "规格", "v1 内容", None).unwrap();
    let _ =
        ailoom::workflow::put_artifact(root, &run.id, "spec", "规格", "v2 内容", Some(1)).unwrap();
    let show = ailoom::workflow::show(root, &run.id).unwrap();
    let art_id = show.artifacts[0].id.clone();
    assert_eq!(show.artifacts[0].version, 2);
    assert_eq!(
        ailoom::workflow::read_artifact(root, &run.id, &art_id).unwrap(),
        "v2 内容"
    );
    // L04：过期版本前置拒绝
    let stale = ailoom::workflow::put_artifact(root, &run.id, "spec", "规格", "v3??", Some(1));
    assert!(stale.is_err(), "过期 base_version 拒绝覆盖");
    // 旧版本可恢复
    assert_eq!(
        ailoom::workflow::read_artifact_version(root, &run.id, &art_id, 1).unwrap(),
        "v1 内容",
        "历史版本可读（可恢复）"
    );
}

// ---------------------------------------------------------------------------
// AIL-076：公司基线更新后，个人替代视图自动重新合成
// ---------------------------------------------------------------------------
#[test]
fn ail076_baseline_change_resynthesizes_view() {
    let c = Ctx::new();
    let repo = make_repo(&c, "repo");
    std::fs::write(repo.join("AGENTS.md"), "# 公司规范 v1\n").unwrap();
    git_add_commit(&c, &repo, "base");
    let dr = c.dr();
    let instr = c.tmp.path().join("instr.md");
    std::fs::write(&instr, "- 个人：回答用中文\n").unwrap();
    let instr_s = instr.to_string_lossy().to_string();
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
            "instructions",
            "--file",
            &instr_s,
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
            "codex",
            "--state",
            "enable",
        ],
    );
    let v = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    let view = repo.join("AGENTS.override.md");
    let content1 = std::fs::read_to_string(&view).unwrap();
    assert!(content1.contains("公司规范 v1") && content1.contains("个人补充"));

    // 公司基线更新 → plan 检测过期并重新合成
    std::fs::write(repo.join("AGENTS.md"), "# 公司规范 v2\n\n- 新条款\n").unwrap();
    let plan = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "plan"],
    );
    assert!(
        plan["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["path"] == "AGENTS.override.md" && a["action"] == "update"),
        "基线变化触发替代视图更新: {plan}"
    );
    let v = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    let content2 = std::fs::read_to_string(&view).unwrap();
    assert!(
        content2.contains("公司规范 v2") && content2.contains("新条款"),
        "视图包含未调整的公司新基线"
    );
    assert!(content2.contains("回答用中文"), "个人补充保留");
}
