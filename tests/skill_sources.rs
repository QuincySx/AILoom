//! AIL-063～067 集成回归：Skill 多来源导入、来源身份、上游检查/更新与部署闭环。
//! 上游用本地 Git 夹具（file:// ）模拟 GitHub；不执行上游脚本、不访问网络。

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

/// 构造上游 git 仓库：skills/<name>/SKILL.md；返回仓库路径（可作 file:// URL）。
fn make_upstream(c: &Ctx, skills: &[(&str, &str)]) -> PathBuf {
    let repo = c.tmp.path().join("upstream");
    for (name, body) in skills {
        let d = repo.join("skills").join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} 技能\nnamespace: personal\nshared: true\n---\n\n{body}\n"),
        )
        .unwrap();
    }
    assert!(c.git(&repo, &["init", "-q"]));
    assert!(c.git(&repo, &["add", "."]));
    assert!(c.git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "v1"
        ]
    ));
    repo
}

fn upstream_commit(c: &Ctx, repo: &Path, msg: &str) {
    assert!(c.git(repo, &["add", "."]));
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
// AIL-063：来源身份记录、同名不同来源不混同、旧数据=来源未知
// ---------------------------------------------------------------------------
#[test]
fn ail063_source_identity_recorded_and_legacy_is_unknown() {
    let c = Ctx::new();
    let dir = c.tmp.path().join("plain");
    std::fs::create_dir_all(&dir).unwrap();
    let dr = c.dr();

    // 本地导入 → 来源=local + 入口路径
    let src = c.tmp.path().join("local-skill");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("SKILL.md"), "# local-skill\n").unwrap();
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
    assert_eq!(code, 0, "{stderr}");

    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "sources",
        ],
    );
    let items = v["items"].as_array().unwrap();
    let local = items
        .iter()
        .find(|i| i["skill"] == "local-skill")
        .expect("列出已导入技能");
    assert_eq!(local["legacy"], serde_json::json!(false));
    assert_eq!(local["source"]["source_kind"], serde_json::json!("local"));
    assert!(
        local["source"]["discovery_entry"]
            .as_str()
            .unwrap()
            .starts_with("local-dir:"),
        "保留本地入口: {local}"
    );

    // 旧数据（手工放入、无来源记录）→ legacy=true，check-update = not-applicable
    let legacy = c
        .tmp
        .path()
        .join("data/library/resources/skills/legacy-skill");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("SKILL.md"), "# legacy\n").unwrap();
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "sources",
        ],
    );
    let legacy_item = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["skill"] == "legacy-skill")
        .unwrap();
    assert_eq!(
        legacy_item["legacy"],
        serde_json::json!(true),
        "旧数据标记来源未知"
    );
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "check-update",
            "--skill",
            "legacy-skill",
        ],
    );
    assert_eq!(
        v["status"]["state"],
        serde_json::json!("not-applicable"),
        "不伪称能上游更新"
    );

    // 同名不同来源不混同：再次从不同本地目录导入同名 → 冲突且指出既有来源
    let src2 = c.tmp.path().join("other-place");
    std::fs::create_dir_all(&src2).unwrap();
    std::fs::write(src2.join("SKILL.md"), "# local-skill v2\n").unwrap();
    let src2_s = src2.to_string_lossy().to_string();
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
            &src2_s,
            "--name",
            "local-skill",
            "--execute",
        ],
    );
    assert_ne!(code, 0, "同名冲突拒绝");
    assert!(
        stderr.contains("local-dir:") || stderr.contains("来自"),
        "冲突信息指出既有来源: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// AIL-064：GitHub（本地夹具）仓库/子目录导入——多 skill 枚举、ref 校验、commit 解析
// ---------------------------------------------------------------------------
#[test]
fn ail064_git_import_preview_execute_and_multi_skill() {
    let c = Ctx::new();
    let dir = c.tmp.path().join("plain");
    std::fs::create_dir_all(&dir).unwrap();
    let dr = c.dr();
    let upstream = make_upstream(&c, &[("alpha", "alpha v1"), ("beta", "beta v1")]);
    let url = format!("file://{}", upstream.display());

    // 多 skill 仓库：不给 --path → 列出候选，不默默导入
    let (code, _, stderr) = c.run(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import-git",
            "--url",
            &url,
        ],
    );
    assert_ne!(code, 0);
    assert!(
        stderr.contains("skills/alpha") && stderr.contains("skills/beta"),
        "候选列表: {stderr}"
    );

    // 无效 ref → 显式失败，不污染库
    let bad_ref = "no-such-ref";
    let (code, _, stderr) = c.run(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import-git",
            "--url",
            &url,
            "--path",
            "skills/alpha",
            "--git-ref",
            bad_ref,
        ],
    );
    assert_ne!(code, 0, "无效 ref 拒绝: {stderr}");
    let (code, out, _) = c.run(
        &dir,
        &["--json", "--data-root", &dr, "library", "--action", "list"],
    );
    assert_eq!(code, 0);
    assert!(!out.contains("alpha"), "失败导入不进库: {out}");

    // 预览：解析 commit、列文件/脚本，不复制
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import-git",
            "--url",
            &url,
            "--path",
            "skills/alpha",
        ],
    );
    let preview = &v["preview"];
    assert_eq!(preview["skill_name"], serde_json::json!("alpha"));
    assert!(
        preview["resolved_commit"].as_str().is_some(),
        "解析 commit: {preview}"
    );
    assert_eq!(preview["repo_path"], serde_json::json!("skills/alpha"));
    assert!(preview["scripts"].as_array().unwrap().is_empty() || preview["conflicts"].is_array());

    // 执行导入（仅复制，不执行）→ 来源元数据含 upstream/commit
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import-git",
            "--url",
            &url,
            "--path",
            "skills/alpha",
            "--execute",
        ],
    );
    assert_eq!(
        v["skill_id"],
        serde_json::json!("personal/skill/personal/alpha")
    );
    assert_eq!(v["scripts_executed"], serde_json::json!(false));
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "sources",
        ],
    );
    let alpha = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["skill"] == "alpha")
        .unwrap();
    assert_eq!(alpha["source"]["source_kind"], serde_json::json!("git"));
    assert_eq!(
        alpha["source"]["repo_path"],
        serde_json::json!("skills/alpha")
    );
    assert!(alpha["source"]["resolved_commit"].as_str().is_some());
    assert!(
        alpha["source"]["discovery_entry"]
            .as_str()
            .unwrap()
            .starts_with("github:"),
        "保留发现入口: {alpha}"
    );
}

// ---------------------------------------------------------------------------
// AIL-066：上游检查状态机 + 更新执行（备份/冲突保护/锁定推进）
// ---------------------------------------------------------------------------
#[test]
fn ail066_upstream_check_update_and_conflict_protection() {
    let c = Ctx::new();
    let dir = c.tmp.path().join("plain");
    std::fs::create_dir_all(&dir).unwrap();
    let dr = c.dr();
    let upstream = make_upstream(&c, &[("solo", "solo v1")]);
    let url = format!("file://{}", upstream.display());
    let _ = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import-git",
            "--url",
            &url,
            "--path",
            "skills/solo",
            "--execute",
        ],
    );

    // 未变化：up-to-date，update 拒绝（无意义）
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "check-update",
            "--skill",
            "solo",
        ],
    );
    assert_eq!(v["status"]["state"], serde_json::json!("up-to-date"));

    // 上游 v2 → upstream-new；检查不等于应用（库内容不变）
    std::fs::write(
        upstream.join("skills/solo/SKILL.md"),
        "---\nname: solo\ndescription: solo\nnamespace: personal\nshared: true\n---\n\nsolo v2\n",
    )
    .unwrap();
    upstream_commit(&c, &upstream, "v2");
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "check-update",
            "--skill",
            "solo",
        ],
    );
    assert_eq!(v["status"]["state"], serde_json::json!("upstream-new"));
    assert!(v["status"]["upstream_commit"].as_str().is_some());
    let lib_skill = c
        .tmp
        .path()
        .join("data/library/resources/skills/solo/SKILL.md");
    assert!(
        std::fs::read_to_string(&lib_skill).unwrap().contains("v1"),
        "检查不应用"
    );

    // 更新执行：内容推进、来源 commit 前进、旧版备份
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "update",
            "--skill",
            "solo",
            "--execute",
        ],
    );
    assert_eq!(v["result"]["updated"], serde_json::json!(true));
    assert!(std::fs::read_to_string(&lib_skill).unwrap().contains("v2"));
    assert!(
        c.tmp
            .path()
            .join("data/library/.updates-backup")
            .read_dir()
            .map(|d| d.count())
            .unwrap_or(0)
            >= 1,
        "旧版已备份"
    );
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "check-update",
            "--skill",
            "solo",
        ],
    );
    assert_eq!(
        v["status"]["state"],
        serde_json::json!("up-to-date"),
        "锁定版本已推进"
    );

    // 本地修改 + 上游再变化 → conflict；update 拒绝且本地内容保留
    std::fs::write(
        &lib_skill,
        "---\nname: solo\ndescription: solo\nnamespace: personal\nshared: true\n---\n\n本地定制\n",
    )
    .unwrap();
    std::fs::write(
        upstream.join("skills/solo/SKILL.md"),
        "---\nname: solo\ndescription: solo\nnamespace: personal\nshared: true\n---\n\nsolo v3\n",
    )
    .unwrap();
    upstream_commit(&c, &upstream, "v3");
    let v = c.run_json(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "check-update",
            "--skill",
            "solo",
        ],
    );
    assert_eq!(v["status"]["state"], serde_json::json!("conflict"));
    let (code, _, stderr) = c.run(
        &dir,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "update",
            "--skill",
            "solo",
            "--execute",
        ],
    );
    assert_ne!(code, 0, "冲突必须显式处理");
    assert!(
        stderr.contains("冲突") || stderr.contains("本地"),
        "{stderr}"
    );
    assert!(
        std::fs::read_to_string(&lib_skill)
            .unwrap()
            .contains("本地定制"),
        "本地修改保留"
    );
}

// ---------------------------------------------------------------------------
// AIL-067：库更新 → 引用作用域显示待同步 → 选择目标分别应用
// ---------------------------------------------------------------------------
#[test]
fn ail067_library_update_to_selected_worktrees_flow() {
    let c = Ctx::new();
    let repo = c.tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    assert!(c.git(&repo, &["init", "-q"]));
    let wt2 = c.tmp.path().join("wt2");
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
    assert!(c.git(
        &repo,
        &["worktree", "add", "-q", wt2.to_str().unwrap(), "-b", "wt2b"]
    ));

    // 上游 v1 → 库 → wt1 部署
    let upstream = make_upstream(&c, &[("flow", "flow v1")]);
    let url = format!("file://{}", upstream.display());
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "import-git",
            "--url",
            &url,
            "--path",
            "skills/flow",
            "--execute",
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
            "personal/skill/personal/flow",
            "--state",
            "enable",
        ],
    );
    let v = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(repo.join(".claude/skills/flow").exists());
    let v = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "deploy-status",
        ],
    );
    assert_eq!(v["items"][0]["state"], serde_json::json!("current"), "{v}");

    // 上游 v2 → 库更新 → wt1 变 stale（待同步），wt2 从未部署
    std::fs::write(
        upstream.join("skills/flow/SKILL.md"),
        "---\nname: flow\ndescription: flow\nnamespace: personal\nshared: true\n---\n\nflow v2\n",
    )
    .unwrap();
    upstream_commit(&c, &upstream, "v2");
    let _ = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "library",
            "--action",
            "update",
            "--skill",
            "flow",
            "--execute",
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
            "deploy-status",
        ],
    );
    assert!(
        v["items"][0]["state"]
            .as_str()
            .unwrap()
            .starts_with("stale"),
        "库更新后当前工作树显示待同步: {v}"
    );
    let v = c.run_json(
        &wt2,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "deploy-status",
        ],
    );
    assert_eq!(
        v["items"][0]["state"],
        serde_json::json!("not-deployed"),
        "未选择的工作树保持原样"
    );

    // 仅选择 wt1 重新应用 → current；wt2 仍 not-deployed
    let v = c.run_json(
        &repo,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(
        std::fs::read_to_string(repo.join(".claude/skills/flow/SKILL.md"))
            .map(|t| t.contains("v2"))
            .unwrap_or(false),
        "wt1 更新到 v2"
    );
    let v = c.run_json(
        &repo,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "deploy-status",
        ],
    );
    assert_eq!(v["items"][0]["state"], serde_json::json!("current"));
    assert!(
        !wt2.join(".claude/skills/flow").exists(),
        "未选择的工作树不被写入"
    );

    // B 工作树随后选择部署 → 追上 v2
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
            "enable",
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
            "--resource",
            "personal/skill/personal/flow",
            "--state",
            "enable",
        ],
    );
    let v = c.run_json(
        &wt2,
        &["--json", "--data-root", &dr, "personal", "--action", "sync"],
    );
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(wt2.join(".claude/skills/flow").exists(), "B 部署");
    let v = c.run_json(
        &wt2,
        &[
            "--json",
            "--data-root",
            &dr,
            "personal",
            "--action",
            "deploy-status",
        ],
    );
    assert_eq!(v["items"][0]["state"], serde_json::json!("current"));
}
