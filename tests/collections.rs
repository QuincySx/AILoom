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
fn external_skills_link_original_content_and_unlink_without_owning_it() {
    let f = Fixture::new();
    let repo = f.source("cc-switch-external", "original", false);
    let data = f.data();
    let scan = ailoom::cc_switch::scan(&data, &serde_json::json!([{"id":"cc-one","name":"chosen","directory":"chosen","repo_owner":"fixture","repo_name":"skills","repo_branch":"main"}])).unwrap();
    let preview = ailoom::cc_switch::prepare_external(
        &data,
        scan["scan_id"].as_str().unwrap(),
        &["0".into()],
        &repo.join("skills"),
    )
    .unwrap();
    assert_eq!(preview["ready"], 1);
    assert!(
        !data.join("collections/cache").exists(),
        "外部登记不 clone 或复制源"
    );
    ailoom::cc_switch::apply(&data, preview["preview_id"].as_str().unwrap()).unwrap();
    let registry = ailoom::collections::load(&data).unwrap();
    let source = registry.sources.values().next().unwrap();
    let skill_id = source.migration.as_ref().unwrap().skills[0]
        .resource_id
        .as_ref()
        .unwrap();
    assert!(source.external_path.is_some());
    assert!(
        ailoom::collections::preview(&data, &source.name, &source.url, None, Some(&source.id))
            .is_err(),
        "外部源不能通过 Git 更新接口接管"
    );
    assert_eq!(
        ailoom::collections::check_updates(&data, Some(&source.id)).unwrap()["items"][0]["state"],
        "external"
    );
    let ws = f.tmp.path().join("project");
    std::fs::create_dir(&ws).unwrap();
    f.host(&ws);
    f.select(&ws, skill_id, "enable");
    f.sync(&ws);
    let target = repo.join("skills/chosen").canonicalize().unwrap();
    let link = ws.join(".claude/skills/chosen");
    assert_eq!(std::fs::read_link(&link).unwrap(), target);
    assert!(
        !common::isolated_store_root(f.tmp.path()).exists(),
        "外部部署不生成 SkillStore 副本"
    );
    let file = target.join("SKILL.md");
    let edited = std::fs::read_to_string(&file)
        .unwrap()
        .replace("original", "edited-by-external-owner");
    std::fs::write(&file, &edited).unwrap();
    assert_eq!(
        std::fs::read_to_string(link.join("SKILL.md")).unwrap(),
        edited,
        "外部修改即时可见"
    );
    f.sync(&ws); // 修改外部内容不应变成 AILoom 所有权冲突。
    assert!(
        ailoom::collections::remove(&data, &source.id, true).is_err(),
        "仍有项目引用不能移除"
    );
    let moved = repo.join("skills/moved");
    std::fs::rename(&target, &moved).unwrap();
    assert!(
        ailoom::collections::catalog(&data, source).is_err(),
        "失效路径明确报错"
    );
    // 坏链也可停用：只读链接身份，不读取目标内容，不删除外部目录。
    f.select(&ws, skill_id, "disable");
    f.sync(&ws);
    assert!(link.symlink_metadata().is_err());
    ailoom::collections::remove(&data, &source.id, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(moved.join("SKILL.md")).unwrap(),
        edited
    );
}

#[test]
fn external_preview_is_content_bound_and_rejects_symlink_escape() {
    let f = Fixture::new();
    let repo = f.source("external-guards", "one", false);
    let scan = ailoom::cc_switch::scan(&f.data(), &serde_json::json!([{"name":"chosen","directory":"chosen","repo_owner":"fixture","repo_name":"skills"}])).unwrap();
    let preview = ailoom::cc_switch::prepare_external(
        &f.data(),
        scan["scan_id"].as_str().unwrap(),
        &["0".into()],
        &repo.join("skills"),
    )
    .unwrap();
    std::fs::write(
        repo.join("skills/chosen/notes.txt"),
        "changed after preview",
    )
    .unwrap();
    assert!(ailoom::cc_switch::apply(&f.data(), preview["preview_id"].as_str().unwrap()).is_err());
    assert!(ailoom::collections::load(&f.data())
        .unwrap()
        .sources
        .is_empty());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            repo.join("skills/unused"),
            repo.join("skills/chosen/escape"),
        )
        .unwrap();
        let preview = ailoom::cc_switch::prepare_external(
            &f.data(),
            scan["scan_id"].as_str().unwrap(),
            &["0".into()],
            &repo.join("skills"),
        )
        .unwrap();
        assert_eq!(preview["ready"], 0);
        assert_eq!(preview["groups"][0]["state"], "error");
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
    // 新 Worktree 首次引用同版本，也不能重新物化并覆盖别人改过的共享实体。
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
    let (ok, _, error) = f.run(
        &third,
        &[
            "personal",
            "--action",
            "sync",
            "--root",
            third.to_str().unwrap(),
        ],
    );
    // 中途失败以非 0 退出，结果放在错误 JSON 的 context 中
    assert!(!ok, "共享实体后改必须拒绝覆盖: {error}");
    let err: Value = serde_json::from_str(error.trim().lines().last().unwrap()).unwrap();
    assert_eq!(err["code"], "E4004", "{err}");
    let report = err["context"].clone();
    assert_eq!(report["ok"], false, "{report}");
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
    assert_eq!(err.code, ailoom::error::code::PRECONDITION_FAILED);
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

#[test]
fn checking_all_persists_status_and_batch_update_is_version_bound() {
    let f = Fixture::new();
    let a = f.source("batch-a", "v1", false);
    let b = f.source("batch-b", "v1", false);
    f.add(&a, "a");
    f.add(&b, "b");
    let before = ailoom::collections::load(&f.data()).unwrap();
    for source in [&a, &b] {
        let file = source.join("skills/chosen/SKILL.md");
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(file, text.replace("v1", "v2")).unwrap();
        f.commit(source);
    }
    let checked = f.ok(f.tmp.path(), &["collection", "--action", "check"]);
    let items = checked["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i["state"] == "available"));
    assert_eq!(
        ailoom::collections::load(&f.data()).unwrap().revision,
        before.revision
    );
    let listed = f.ok(f.tmp.path(), &["collection", "--action", "list"]);
    assert!(listed["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["update"]["state"] == "available"));
    let tokens: Vec<String> = items
        .iter()
        .map(|i| i["preview"]["preview_id"].as_str().unwrap().to_string())
        .collect();
    let applied = ailoom::collections::apply_previews(&f.data(), &tokens).unwrap();
    assert_eq!(applied["updated"], 2);
    assert_eq!(
        ailoom::collections::load(&f.data()).unwrap().revision,
        before.revision + 1
    );
    assert!(
        ailoom::collections::apply_previews(&f.data(), &tokens).is_err(),
        "旧预览不能重复应用"
    );
    let rechecked = f.ok(f.tmp.path(), &["collection", "--action", "check"]);
    assert!(rechecked["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["state"] == "current"));
}

#[test]
fn removing_collection_blocks_references_and_preserves_store_after_unlink() {
    let f = Fixture::new();
    let source = f.source("removable", "v1", false);
    let p = f.add(&source, "removable");
    let id = p["source"]["id"].as_str().unwrap();
    let ws = common::make_business_repo(f.tmp.path(), "consumer");
    f.host(&ws);
    let resource = format!("{id}/skill/common/chosen");
    f.select(&ws, &resource, "enable");
    f.sync(&ws);
    let link = ws.join(".claude/skills/chosen");
    let entity = std::fs::read_link(&link).unwrap();
    let preview = f.ok(&ws, &["collection", "--action", "remove", "--source", id]);
    assert_eq!(preview["references"].as_array().unwrap().len(), 1);
    assert!(
        !f.run(
            &ws,
            &[
                "collection",
                "--action",
                "remove",
                "--source",
                id,
                "--execute"
            ]
        )
        .0
    );
    assert!(entity.join("SKILL.md").is_file());
    f.select(&ws, &resource, "disable");
    f.sync(&ws);
    f.ok(
        &ws,
        &[
            "collection",
            "--action",
            "remove",
            "--source",
            id,
            "--execute",
        ],
    );
    assert!(
        entity.join("SKILL.md").is_file(),
        "移除来源不能清空历史实体"
    );
    assert!(!link.exists());
    assert!(ailoom::collections::load(&f.data())
        .unwrap()
        .sources
        .is_empty());
    assert_eq!(
        std::fs::read_dir(f.data().join("collections/archive"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
#[cfg(unix)]
fn legacy_base64_links_switch_individually_without_removing_old_entities() {
    let f = Fixture::new();
    let source = f.source("legacy", "v1", false);
    let p = f.add(&source, "legacy");
    let id = p["source"]["id"].as_str().unwrap();
    let ws = common::make_business_repo(f.tmp.path(), "legacy-consumer");
    f.host(&ws);
    f.select(&ws, &format!("{id}/skill/common/chosen"), "enable");
    f.sync(&ws);
    let link = ws.join(".claude/skills/chosen");
    let readable = std::fs::read_link(&link).unwrap();
    let bucket = readable
        .ancestors()
        .find(|p| p.join(".meta/SOURCE.json").is_file())
        .unwrap()
        .to_path_buf();
    let meta: Value =
        serde_json::from_slice(&std::fs::read(bucket.join(".meta/SOURCE.json")).unwrap()).unwrap();
    let legacy = common::isolated_store_root(f.tmp.path()).join(meta["key"].as_str().unwrap());
    std::fs::rename(&bucket, &legacy).unwrap();
    let legacy_skill = legacy.join(readable.strip_prefix(&bucket).unwrap());
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&legacy_skill, &link).unwrap();
    let digest = ailoom::store::dir_digest(&legacy_skill).unwrap();
    let hash = format!(
        "sha256:{}",
        ailoom::ids::sha256_hex(format!("{}|{digest}", legacy_skill.display()).as_bytes())
    );
    for file in walkdir::WalkDir::new(f.data())
        .into_iter()
        .filter_map(Result::ok)
        .filter(|f| f.file_name() == "managed-manifest.json")
    {
        let mut m: Value = serde_json::from_slice(&std::fs::read(file.path()).unwrap()).unwrap();
        for item in m["items"].as_object_mut().unwrap().values_mut() {
            if item["resource_id"] == format!("{id}/skill/common/chosen") {
                item["content_hash"] = serde_json::json!(hash);
            }
        }
        std::fs::write(file.path(), serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    }
    f.sync(&ws);
    assert_eq!(std::fs::read_link(&link).unwrap(), readable);
    assert!(readable.join("SKILL.md").is_file());
    assert!(
        legacy_skill.join("SKILL.md").is_file(),
        "旧实体保留，其他 Worktree 的旧链接不受影响"
    );
}

#[test]
fn personal_copy_delete_is_guarded_and_recoverably_archived() {
    let f = Fixture::new();
    let source = f.source("personal-copy", "preserve-me", false);
    let file = source.join("skills/chosen/SKILL.md");
    let text = std::fs::read_to_string(&file)
        .unwrap()
        .replace("namespace: common", "namespace: personal");
    std::fs::write(&file, text).unwrap();
    f.ok(
        f.tmp.path(),
        &[
            "library",
            "--action",
            "import",
            "--dir",
            source.join("skills/chosen").to_str().unwrap(),
            "--execute",
        ],
    );
    let (entries, _) = ailoom::personal_library::list_tolerant(&f.data());
    let id = &entries[0].id;
    let ws = common::make_business_repo(f.tmp.path(), "copy-consumer");
    f.select(&ws, id, "enable");
    assert!(ailoom::personal_library::delete_execute(&f.data(), id).is_err());
    f.select(&ws, id, "disable");
    ailoom::personal_library::delete_execute(&f.data(), id).unwrap();
    assert!(ailoom::personal_library::list_tolerant(&f.data())
        .0
        .is_empty());
    let archived = walkdir::WalkDir::new(f.data().join("library-archive"))
        .into_iter()
        .filter_map(Result::ok)
        .find(|f| f.file_name() == "SKILL.md")
        .unwrap();
    assert!(std::fs::read_to_string(archived.path())
        .unwrap()
        .contains("preserve-me"));
    assert!(source.join("skills/chosen/SKILL.md").is_file());
}

/// AIL-133 组合场景：团队源 × 个人层宿主 × 合集引用 × 子目录作用域。
/// 团队 sync、个人 sync（根与子目录）任意交替后，两条计划都没有增删，且各层部署都在。
#[test]
fn team_personal_collection_and_subdirectory_layers_converge() {
    let f = Fixture::new();
    let ws = common::make_business_repo(f.tmp.path(), "biz");
    std::fs::create_dir_all(ws.join("web")).unwrap();
    let team = common::make_team_source(&f.tmp.path().join("team"));
    f.ok(
        &ws,
        &[
            "init",
            "--url",
            &common::file_url(&team),
            "--project",
            "a",
            "--role",
            "dev",
        ],
    );
    let col = f.source("combo-collection", "C-v1", false);
    let id = f.add(&col, "组合合集")["source"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.host(&ws);
    f.select(&ws, &format!("{id}/skill/common/chosen"), "enable");
    f.ok(
        &ws,
        &[
            "personal",
            "--action",
            "select",
            "--repo",
            ws.to_str().unwrap(),
            "--subproject",
            "web",
            "--resource",
            &format!("{id}/skill/common/unused"),
            "--state",
            "enable",
        ],
    );

    let team_sync = || f.ok(&ws, &["sync"]);
    let personal_sync = || f.sync(&ws);
    let sub_sync = || {
        f.ok(
            &ws,
            &[
                "personal",
                "--action",
                "sync",
                "--root",
                ws.to_str().unwrap(),
                "--scope",
                "web",
            ],
        );
    };
    for step in [0, 1, 2, 0, 2, 1, 0] {
        match step {
            0 => {
                team_sync();
            }
            1 => personal_sync(),
            _ => sub_sync(),
        }
    }
    assert!(
        ws.join(".claude/skills/common-greet").exists(),
        "团队层部署保留"
    );
    assert!(
        ws.join(".claude/agents/ailoom-recall.md").exists(),
        "内置资源保留"
    );
    assert!(ws.join(".claude/skills/chosen").exists(), "合集引用保留");
    assert!(
        ws.join("web/.claude/skills/unused").exists(),
        "子目录引用保留"
    );

    let changes = |v: &Value| -> Vec<String> {
        v["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| matches!(a["action"].as_str(), Some("create" | "update" | "delete")))
            .map(|a| format!("{} {}", a["action"], a["path"]))
            .collect()
    };
    let team_plan = f.ok(&ws, &["plan"]);
    assert!(
        changes(&team_plan).is_empty(),
        "团队计划: {:?}",
        changes(&team_plan)
    );
    let root_plan = f.ok(
        &ws,
        &[
            "personal",
            "--action",
            "plan",
            "--root",
            ws.to_str().unwrap(),
        ],
    );
    assert!(
        changes(&root_plan).is_empty(),
        "个人计划: {:?}",
        changes(&root_plan)
    );
    let sub_plan = f.ok(
        &ws,
        &[
            "personal",
            "--action",
            "plan",
            "--root",
            ws.to_str().unwrap(),
            "--scope",
            "web",
        ],
    );
    assert!(
        changes(&sub_plan).is_empty(),
        "子目录计划: {:?}",
        changes(&sub_plan)
    );
}

/// AIL-119 缺口：符号链接成环 / 外逃的合集——预览、应用、部署都不卡住，
/// 部署到 Store 的实体不含任何符号链接；项目扫描遇到环也能正常返回。
#[test]
fn looping_symlinks_in_collection_and_project_are_contained() {
    let f = Fixture::new();
    let col = f.source("loop-collection", "L", false);
    let skill = col.join("skills/chosen");
    std::os::unix::fs::symlink("../..", skill.join("loop")).unwrap();
    std::os::unix::fs::symlink(".", skill.join("self")).unwrap();
    f.commit(&col);
    let id = f.add(&col, "环链接合集")["source"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let ws = f.tmp.path().join("project");
    std::fs::create_dir_all(ws.join(".claude/skills/local")).unwrap();
    std::fs::write(
        ws.join(".claude/skills/local/SKILL.md"),
        "---\nname: local\ndescription: d\n---\n",
    )
    .unwrap();
    std::os::unix::fs::symlink("..", ws.join(".claude/skills/local/up")).unwrap();
    std::os::unix::fs::symlink("../.claude", ws.join(".claude/skills/selfref")).unwrap();
    f.host(&ws);
    f.select(&ws, &format!("{id}/skill/common/chosen"), "enable");
    f.sync(&ws);
    let deployed = ws.join(".claude/skills/chosen").canonicalize().unwrap();
    for entry in walkdir::WalkDir::new(&deployed).follow_links(false) {
        assert!(
            !entry.unwrap().file_type().is_symlink(),
            "部署实体不应含符号链接"
        );
    }
    let scan = f.ok(
        &ws,
        &[
            "personal",
            "--action",
            "scan-skills",
            "--root",
            ws.to_str().unwrap(),
        ],
    );
    assert!(
        scan["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["dir_name"] == "local"),
        "{scan}"
    );
}

/// AIL-119 缺口：同一来源同时供项目 A、B 使用；A 取消引用并应用后，B 的部署不受影响。
#[test]
fn removing_reference_in_one_project_keeps_other_project_deployed() {
    let f = Fixture::new();
    let col = f.source("shared-collection", "S", false);
    let id = f.add(&col, "共享合集")["source"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let rid = format!("{id}/skill/common/chosen");
    let (a, b) = (
        f.tmp.path().join("project-a"),
        f.tmp.path().join("project-b"),
    );
    for ws in [&a, &b] {
        std::fs::create_dir_all(ws).unwrap();
        f.host(ws);
        f.select(ws, &rid, "enable");
        f.sync(ws);
        assert!(ws.join(".claude/skills/chosen/SKILL.md").exists());
    }
    f.select(&a, &rid, "disable");
    f.sync(&a);
    assert!(!a.join(".claude/skills/chosen").exists(), "A 已移除");
    assert!(
        b.join(".claude/skills/chosen/SKILL.md").exists(),
        "B 不受影响"
    );
    assert!(
        std::fs::read_to_string(b.join(".claude/skills/chosen/SKILL.md"))
            .unwrap()
            .contains('S')
    );
    let list = f.ok(f.tmp.path(), &["collection", "--action", "list"]);
    assert_eq!(list["sources"].as_array().unwrap().len(), 1, "来源仍登记");
}

/// 盲测回归：不加 --json 也能走完快速上手的合集流程——preview 给出 apply 命令，
/// list 给出可直接用于 select 的完整资源 ID；更新已登记的合集只需 --source。
#[test]
fn collection_flow_works_without_json_and_update_needs_only_source() {
    let f = Fixture::new();
    let repo = f.source("human", "v1", false);
    let human = |args: &[&str]| -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_ailoom"))
            .args(args)
            .current_dir(f.tmp.path())
            .envs(common::isolated_child_env(f.tmp.path()))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let url = repo.to_str().unwrap();
    let out = human(&[
        "collection",
        "--action",
        "preview",
        "--name",
        "human",
        "--url",
        url,
    ]);
    let apply_line = out
        .lines()
        .find(|l| l.contains("--action apply --preview-id"))
        .unwrap_or_else(|| panic!("preview 普通输出要给出 apply 命令: {out}"));
    let preview_id = apply_line.rsplit(' ').next().unwrap();
    human(&[
        "collection",
        "--action",
        "apply",
        "--preview-id",
        preview_id,
    ]);
    let out = human(&["collection", "--action", "list"]);
    let id = out
        .split_whitespace()
        .find(|w| w.ends_with("/skill/common/chosen"))
        .unwrap_or_else(|| panic!("list 普通输出要列出资源 ID: {out}"))
        .to_string();
    let source_id = id.split('/').next().unwrap().to_string();

    std::fs::write(
        repo.join("skills/chosen/SKILL.md"),
        "---\nname: chosen\ndescription: fixture\nnamespace: common\nshared: true\n---\nv2\n",
    )
    .unwrap();
    f.commit(&repo);
    let p = f.ok(
        f.tmp.path(),
        &["collection", "--action", "preview", "--source", &source_id],
    );
    assert_eq!(p["source"]["id"], source_id.as_str(), "{p}");
    assert_eq!(p["source"]["name"], "human", "沿用登记的名称: {p}");
}
