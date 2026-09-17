//! 多合集真实 CLI 回归：不联网、不启动 MCP，所有项目/源/store 均隔离。
mod common;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Fixture {
    tmp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }
    fn data(&self) -> PathBuf {
        common::isolated_data_root(self.tmp.path())
    }
    fn run(&self, cwd: &Path, args: &[&str]) -> (bool, Value, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_ailoom"))
            .args(["--json"])
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .output()
            .unwrap();
        let parsed: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        (
            out.status.success(),
            parsed["result"].clone(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn ok(&self, cwd: &Path, args: &[&str]) -> Value {
        let (ok, v, err) = self.run(cwd, args);
        assert!(ok, "{args:?}: {err}");
        v
    }
    fn git(&self, cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn commit(&self, cwd: &Path) {
        self.git(cwd, &["add", "."]);
        self.git(
            cwd,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-qm",
                "fixture",
            ],
        );
    }
    fn source(&self, name: &str, body: &str, mixed: bool) -> PathBuf {
        let repo = self.tmp.path().join(name);
        let skills = repo.join(if mixed { "resources/skills" } else { "skills" });
        for skill in ["chosen", "unused"] {
            let dir = skills.join(skill);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), format!("---\nname: {skill}\ndescription: fixture\nnamespace: common\nshared: true\n---\n{body}\n")).unwrap();
        }
        if mixed {
            std::fs::write(
                repo.join("ailoom.toml"),
                "schema_version = 1\nteam_id = 'fixture'\n[namespaces]\nknown = ['common']\n",
            )
            .unwrap();
            std::fs::create_dir_all(repo.join("resources/mcp")).unwrap();
            std::fs::write(repo.join("resources/mcp/search.toml"), "name='search'\nnamespace='common'\nshared=true\ntype='stdio'\ncommand='never-start-this-fixture'\nargs=[]\n").unwrap();
        }
        self.git(&repo, &["init", "-q"]);
        self.commit(&repo);
        repo
    }
    fn add(&self, path: &Path, name: &str) -> Value {
        let p = self.ok(
            self.tmp.path(),
            &[
                "collection",
                "--action",
                "preview",
                "--name",
                name,
                "--url",
                path.to_str().unwrap(),
            ],
        );
        assert!(!self.data().join("profile/profile.toml").exists());
        self.ok(
            self.tmp.path(),
            &[
                "collection",
                "--action",
                "apply",
                "--preview-id",
                p["preview_id"].as_str().unwrap(),
            ],
        );
        p
    }
    fn select(&self, root: &Path, resource: &str, state: &str) {
        self.ok(
            root,
            &[
                "personal",
                "--action",
                "select",
                "--repo",
                root.to_str().unwrap(),
                "--resource",
                resource,
                "--state",
                state,
            ],
        );
    }
    fn host(&self, root: &Path) {
        self.ok(
            root,
            &[
                "personal",
                "--action",
                "select",
                "--repo",
                root.to_str().unwrap(),
                "--host",
                "claude",
                "--state",
                "enable",
            ],
        );
    }
    fn sync(&self, root: &Path) {
        self.ok(
            root,
            &[
                "personal",
                "--action",
                "sync",
                "--root",
                root.to_str().unwrap(),
            ],
        );
    }
}

#[test]
fn multiple_collections_only_deploy_explicit_skill_and_mcp_references() {
    let f = Fixture::new();
    let a = f.source("my-collection", "A-v1", true);
    let b = f.source("third-party", "B-v1", false);
    let pa = f.add(&a, "我的合集");
    let pb = f.add(&b, "第三方合集");
    let id_a = pa["source"]["id"].as_str().unwrap();
    let id_b = pb["source"]["id"].as_str().unwrap();
    assert_ne!(id_a, id_b);
    let list = f.ok(f.tmp.path(), &["collection", "--action", "list"]);
    assert_eq!(list["sources"].as_array().unwrap().len(), 2);
    let ws = f.tmp.path().join("project");
    std::fs::create_dir_all(&ws).unwrap();
    f.host(&ws);
    f.sync(&ws);
    assert!(
        !ws.join(".claude/skills/chosen").exists(),
        "添加合集不能安装全仓"
    );
    let skill = format!("{id_b}/skill/common/chosen");
    let mcp = format!("{id_a}/mcp/common/search");
    f.select(&ws, &skill, "enable");
    f.select(&ws, &mcp, "enable");
    f.sync(&ws);
    assert!(
        std::fs::read_to_string(ws.join(".claude/skills/chosen/SKILL.md"))
            .unwrap()
            .contains("B-v1")
    );
    assert!(!ws.join(".claude/skills/unused").exists());
    let config: Value =
        serde_json::from_slice(&std::fs::read(ws.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(
        config["mcpServers"]["search"]["command"],
        "never-start-this-fixture"
    );
    // 同名不同来源允许共存于目录，但同时部署到同一宿主位置必须冲突。
    f.select(&ws, &format!("{id_a}/skill/common/chosen"), "enable");
    let (ok, _, err) = f.run(
        &ws,
        &[
            "personal",
            "--action",
            "plan",
            "--root",
            ws.to_str().unwrap(),
        ],
    );
    assert!(!ok);
    assert!(err.contains("同一目标"), "{err}");
    assert!(
        std::fs::read_to_string(ws.join(".claude/skills/chosen/SKILL.md"))
            .unwrap()
            .contains("B-v1")
    );
}

#[test]
fn pinned_collection_update_does_not_change_unselected_worktree_or_local_edits() {
    let f = Fixture::new();
    let source = f.source("upstream", "v1", false);
    let p = f.add(&source, "upstream");
    let id = p["source"]["id"].as_str().unwrap();
    let ws = f.tmp.path().join("worktree-a");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("README.md"), "company baseline").unwrap();
    f.git(&ws, &["init", "-q"]);
    f.commit(&ws);
    let wt = f.tmp.path().join("worktree-b");
    f.git(
        &ws,
        &["worktree", "add", "-q", "-b", "other", wt.to_str().unwrap()],
    );
    f.host(&ws);
    f.select(&ws, &format!("{id}/skill/common/chosen"), "enable");
    f.sync(&ws);
    f.sync(&wt);
    let link = wt.join(".claude/skills/chosen");
    let old_target = std::fs::read_link(&link).unwrap();
    let source_file = source.join("skills/chosen/SKILL.md");
    let text = std::fs::read_to_string(&source_file).unwrap();
    std::fs::write(&source_file, text.replace("v1", "v2")).unwrap();
    f.commit(&source);
    // 只推进上游不改变已锁定版本。
    f.sync(&ws);
    assert!(std::fs::read_to_string(link.join("SKILL.md"))
        .unwrap()
        .contains("v1"));
    let update = f.ok(
        &ws,
        &[
            "collection",
            "--action",
            "preview",
            "--name",
            "upstream",
            "--url",
            source.to_str().unwrap(),
            "--source",
            id,
        ],
    );
    f.ok(
        &ws,
        &[
            "collection",
            "--action",
            "apply",
            "--preview-id",
            update["preview_id"].as_str().unwrap(),
        ],
    );
    let status = f.ok(
        &ws,
        &[
            "personal",
            "--action",
            "deploy-status",
            "--root",
            ws.to_str().unwrap(),
        ],
    );
    assert_eq!(status["items"][0]["state"], "stale");
    f.sync(&ws);
    assert!(
        std::fs::read_to_string(ws.join(".claude/skills/chosen/SKILL.md"))
            .unwrap()
            .contains("v2")
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), old_target);
    assert!(
        std::fs::read_to_string(link.join("SKILL.md"))
            .unwrap()
            .contains("v1"),
        "只更新A不能经store改掉B"
    );
    // 用户通过旧实体做的后改必须保留；不能因来源新版而覆盖。
    std::fs::write(link.join("SKILL.md"), "USER EDIT").unwrap();
    let (_, report, _) = f.run(
        &wt,
        &[
            "personal",
            "--action",
            "sync",
            "--root",
            wt.to_str().unwrap(),
        ],
    );
    assert!(
        !report["skipped_conflicts"].as_array().unwrap().is_empty(),
        "用户后改必须列为冲突而非执行"
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), old_target);
    assert_eq!(
        std::fs::read_to_string(link.join("SKILL.md")).unwrap(),
        "USER EDIT"
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("README.md")).unwrap(),
        "company baseline"
    );
    // 新工作树首次引用同版本，也不能重新物化并覆盖别人改过的共享实体。
    let current_skill = ws.join(".claude/skills/chosen/SKILL.md");
    std::fs::write(&current_skill, "SHARED USER EDIT").unwrap();
    let third = f.tmp.path().join("worktree-c");
    f.git(
        &ws,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "third",
            third.to_str().unwrap(),
        ],
    );
    let (_, report, error) = f.run(
        &third,
        &[
            "personal",
            "--action",
            "sync",
            "--root",
            third.to_str().unwrap(),
        ],
    );
    assert_eq!(
        report["ok"], false,
        "共享实体后改必须拒绝覆盖: {report}; {error}"
    );
    assert!(!report["failed"].is_null());
    assert_eq!(
        std::fs::read_to_string(current_skill).unwrap(),
        "SHARED USER EDIT"
    );
    assert!(!third.join(".claude/skills/chosen").exists());
}

#[test]
fn collection_previews_are_version_bound_and_do_not_accept_forged_paths() {
    let f = Fixture::new();
    let a = f.source("a", "A", false);
    let b = f.source("b", "B", false);
    let p1 = ailoom::collections::preview(&f.data(), "a", a.to_str().unwrap(), None, None).unwrap();
    let p2 = ailoom::collections::preview(&f.data(), "b", b.to_str().unwrap(), None, None).unwrap();
    ailoom::collections::apply_preview(&f.data(), p1["preview_id"].as_str().unwrap()).unwrap();
    let err = ailoom::collections::apply_preview(&f.data(), p2["preview_id"].as_str().unwrap())
        .unwrap_err();
    assert_eq!(err.code, ailoom::error::code::USER_CONTENT_CONFLICT);
    assert!(ailoom::collections::apply_preview(&f.data(), "../../profile").is_err());
    assert_eq!(
        ailoom::collections::list(&f.data()).unwrap()["sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(ailoom::collections::preview(
        &f.data(),
        "unsafe",
        "ext::unexpected-helper",
        None,
        None
    )
    .is_err());
}

#[test]
fn invalid_mcp_collection_does_not_replace_registered_snapshot() {
    let f = Fixture::new();
    let source = f.source("mixed", "v1", true);
    let p = f.add(&source, "mixed");
    let id = p["source"]["id"].as_str().unwrap();
    let mcp = source.join("resources/mcp/search.toml");
    let text = std::fs::read_to_string(&mcp).unwrap();
    std::fs::write(
        &mcp,
        format!("{text}\n[env]\nTOKEN='dummy-not-a-real-secret'\n"),
    )
    .unwrap();
    f.commit(&source);
    let (ok, _, _) = f.run(
        f.tmp.path(),
        &[
            "collection",
            "--action",
            "preview",
            "--name",
            "mixed",
            "--url",
            source.to_str().unwrap(),
            "--source",
            id,
        ],
    );
    assert!(!ok);
    let list = f.ok(f.tmp.path(), &["collection", "--action", "list"]);
    assert_eq!(
        list["sources"][0]["lock"]["resolved_commit"],
        p["source"]["lock"]["resolved_commit"]
    );
    assert_eq!(list["sources"][0]["resources"].as_array().unwrap().len(), 3);
}
