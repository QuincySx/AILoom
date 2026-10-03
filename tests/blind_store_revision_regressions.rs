//! R01: each project pins immutable Skill content; undo restores that pinned revision.

use ailoom::{ids::sha256_hex, store};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RESOURCE: &str = "personal/skill/personal/versioned-skill";
const LINK: &str = ".claude/skills/versioned-skill";

struct Fixture {
    tmp: tempfile::TempDir,
    source: PathBuf,
    a: PathBuf,
    b: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source/versioned-skill");
        let a = tmp.path().join("project A 中文");
        let b = tmp.path().join("project B");
        for path in [&source, &a, &b] {
            std::fs::create_dir_all(path).unwrap();
        }
        let fixture = Self { tmp, source, a, b };
        fixture.write_source("ONE");
        fixture.ok(
            &fixture.a,
            &[
                "library",
                "--action",
                "import",
                "--dir",
                fixture.source.to_str().unwrap(),
                "--execute",
            ],
        );
        for project in [&fixture.a, &fixture.b] {
            fixture.ok(
                project,
                &[
                    "personal",
                    "--action",
                    "select",
                    "--repo",
                    project.to_str().unwrap(),
                    "--host",
                    "claude",
                    "--state",
                    "enable",
                ],
            );
            fixture.ok(
                project,
                &[
                    "personal",
                    "--action",
                    "select",
                    "--repo",
                    project.to_str().unwrap(),
                    "--resource",
                    RESOURCE,
                    "--state",
                    "enable",
                ],
            );
        }
        fixture
    }

    fn data(&self) -> PathBuf {
        self.tmp.path().join("data")
    }
    fn store(&self) -> PathBuf {
        self.tmp.path().join("xdg-data/ailoom/store")
    }

    fn run(&self, project: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ailoom"))
            .args(["--json", "--data-root"])
            .arg(self.data())
            .args(args)
            .current_dir(project)
            .env("XDG_DATA_HOME", self.tmp.path().join("xdg-data"))
            .env("XDG_STATE_HOME", self.tmp.path().join("xdg-state"))
            .env("XDG_CONFIG_HOME", self.tmp.path().join("xdg-config"))
            .env("PI_CODING_AGENT_DIR", self.tmp.path().join("pi"))
            .env("AILOOM_REPORTING", "off")
            .env("AILOOM_AUTO_SYNC", "off")
            .output()
            .unwrap()
    }

    fn ok(&self, project: &Path, args: &[&str]) -> Value {
        let out = self.run(project, args);
        assert!(
            out.status.success(),
            "{args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["result"].clone()
    }

    fn personal(&self, project: &Path, action: &str) -> Value {
        self.ok(
            project,
            &[
                "personal",
                "--action",
                action,
                "--root",
                project.to_str().unwrap(),
            ],
        )
    }

    fn write_source(&self, version: &str) {
        std::fs::write(
            self.source.join("SKILL.md"),
            format!("# versioned-skill\n\nVERSION {version}\n"),
        )
        .unwrap();
        std::fs::create_dir_all(self.source.join("references")).unwrap();
        std::fs::write(self.source.join("references/version.txt"), version).unwrap();
    }

    fn update_library(&self, version: &str) {
        self.write_source(version);
        let checked = self.ok(
            &self.a,
            &[
                "library",
                "--action",
                "check-update",
                "--skill",
                "versioned-skill",
            ],
        );
        assert_eq!(checked["status"]["state"], "upstream-new");
        let updated = self.ok(
            &self.a,
            &[
                "library",
                "--action",
                "update",
                "--skill",
                "versioned-skill",
                "--preview-id",
                checked["status"]["preview_id"].as_str().unwrap(),
                "--execute",
            ],
        );
        assert_eq!(updated["result"]["updated"], true);
    }

    fn target(&self, project: &Path) -> PathBuf {
        std::fs::read_link(project.join(LINK)).unwrap()
    }
    fn undo(&self, project: &Path, sync: &Value) -> Value {
        self.ok(
            project,
            &[
                "personal",
                "--action",
                "undo",
                "--id",
                sync["job_id"].as_str().unwrap(),
            ],
        )
    }
}

fn assert_version(path: &Path, version: &str) {
    assert!(std::fs::read_to_string(path.join("SKILL.md"))
        .unwrap()
        .contains(&format!("VERSION {version}")));
    assert_eq!(
        std::fs::read_to_string(path.join("references/version.txt")).unwrap(),
        version
    );
}

fn tree_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            (
                e.path().strip_prefix(root).unwrap().to_path_buf(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

fn skill_action(plan: &Value) -> &str {
    plan["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["path"] == LINK)
        .unwrap()["action"]
        .as_str()
        .unwrap()
}

#[test]
fn library_and_one_project_updates_leave_other_projects_pinned() {
    let f = Fixture::new();
    f.personal(&f.a, "sync");
    f.personal(&f.b, "sync");
    let old = f.target(&f.a);
    assert_eq!(old, f.target(&f.b));
    assert!(old.to_string_lossy().contains("/revisions/sha256-"));
    assert_version(&old, "ONE");
    let before = tree_files(&f.store());
    f.update_library("TWO");
    assert_eq!(
        tree_files(&f.store()),
        before,
        "library update must not change deployed Store content"
    );
    assert_version(&f.a.join(LINK), "ONE");
    assert_version(&f.b.join(LINK), "ONE");
    assert_eq!(skill_action(&f.personal(&f.a, "plan")), "update");
    assert_eq!(
        tree_files(&f.store()),
        before,
        "plan must not materialize its desired revision"
    );
    let update = f.personal(&f.a, "sync");
    assert_eq!(update["ok"], true);
    assert_ne!(f.target(&f.a), old);
    assert_version(&f.a.join(LINK), "TWO");
    assert_eq!(f.target(&f.b), old);
    assert_version(&f.b.join(LINK), "ONE");
    assert_version(&old, "ONE");
    assert_eq!(skill_action(&f.personal(&f.a, "plan")), "noop");
    assert!(f.personal(&f.a, "sync")["applied"]
        .as_array()
        .unwrap()
        .is_empty());
    // A different library Skill does not move the already pinned selected Skill.
    let other = f.data().join("library/resources/skills/unselected");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("SKILL.md"),
        "# unselected\n\nDifferent content\n",
    )
    .unwrap();
    assert_eq!(skill_action(&f.personal(&f.a, "plan")), "noop");
}

#[test]
fn undo_restores_prior_skill_content_and_can_be_applied_again() {
    let f = Fixture::new();
    f.personal(&f.a, "sync");
    let old = f.target(&f.a);
    f.update_library("TWO");
    let update = f.personal(&f.a, "sync");
    let new = f.target(&f.a);
    let undo = f.undo(&f.a, &update);
    assert!(undo["conflicts"].as_array().unwrap().is_empty());
    assert_eq!(f.target(&f.a), old);
    assert_version(&f.a.join(LINK), "ONE");
    assert_version(&new, "TWO");
    let plan = f.personal(&f.a, "plan");
    assert_eq!(
        skill_action(&plan),
        "update",
        "normal undo must not appear as user drift: {plan}"
    );
    f.personal(&f.a, "sync");
    assert_eq!(f.target(&f.a), new);
    assert_version(&f.a.join(LINK), "TWO");
}

#[test]
fn undo_preserves_later_user_changes_and_their_manifest() {
    let f = Fixture::new();
    f.personal(&f.a, "sync");
    f.update_library("TWO");
    let update = f.personal(&f.a, "sync");
    let new = f.target(&f.a);
    let manifests = tree_files(&f.data().join("ws"));
    std::fs::write(new.join("references/version.txt"), "USER MODIFIED").unwrap();
    let undo = f.undo(&f.a, &update);
    assert!(!undo["conflicts"].as_array().unwrap().is_empty());
    assert_eq!(f.target(&f.a), new);
    assert_eq!(
        std::fs::read_to_string(new.join("references/version.txt")).unwrap(),
        "USER MODIFIED"
    );
    for (path, bytes) in manifests {
        if path.file_name().unwrap() == "managed-manifest.json" {
            assert_eq!(
                std::fs::read(f.data().join("ws").join(path)).unwrap(),
                bytes
            );
        }
    }
    assert_eq!(skill_action(&f.personal(&f.a, "plan")), "conflict");
}

#[test]
fn undo_refuses_modified_prior_entity_shared_by_another_project() {
    let f = Fixture::new();
    f.personal(&f.a, "sync");
    f.personal(&f.b, "sync");
    let old = f.target(&f.b);
    f.update_library("TWO");
    let update = f.personal(&f.a, "sync");
    let new = f.target(&f.a);
    std::fs::write(old.join("references/version.txt"), "B USER MODIFIED").unwrap();
    let undo = f.undo(&f.a, &update);
    assert!(!undo["conflicts"].as_array().unwrap().is_empty());
    assert_eq!(f.target(&f.a), new);
    assert_version(&f.a.join(LINK), "TWO");
    assert_eq!(
        std::fs::read_to_string(f.b.join(LINK).join("references/version.txt")).unwrap(),
        "B USER MODIFIED"
    );
    assert_eq!(skill_action(&f.personal(&f.a, "plan")), "noop");
}

#[test]
fn legacy_working_links_migrate_per_project_and_undo_keeps_old_entity() {
    let f = Fixture::new();
    f.personal(&f.a, "sync");
    f.personal(&f.b, "sync");
    let current = f.target(&f.a);
    let legacy = current
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("working/versioned-skill");
    std::fs::create_dir_all(&legacy).unwrap();
    for (path, bytes) in tree_files(&current) {
        std::fs::create_dir_all(legacy.join(&path).parent().unwrap()).unwrap();
        std::fs::write(legacy.join(path), bytes).unwrap();
    }
    let legacy_hash = format!(
        "sha256:{}",
        sha256_hex(
            format!(
                "{}|{}",
                legacy.display(),
                store::dir_digest(&legacy).unwrap()
            )
            .as_bytes()
        )
    );
    for project in [&f.a, &f.b] {
        std::fs::remove_file(project.join(LINK)).unwrap();
        std::os::unix::fs::symlink(&legacy, project.join(LINK)).unwrap();
    }
    // Fixture simulates the old release's managed link hash; no product internals are bypassed during migration.
    for file in walkdir::WalkDir::new(f.data().join("ws"))
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if file.file_name() == "managed-manifest.json" {
            let mut manifest: Value =
                serde_json::from_slice(&std::fs::read(file.path()).unwrap()).unwrap();
            manifest["items"][format!("{LINK}#symlink")]["content_hash"] =
                Value::String(legacy_hash.clone());
            std::fs::write(file.path(), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        }
    }
    let before = tree_files(&f.store());
    assert_eq!(skill_action(&f.personal(&f.a, "plan")), "update");
    assert_eq!(tree_files(&f.store()), before);
    let sync = f.personal(&f.a, "sync");
    assert_eq!(f.target(&f.a), current);
    assert_eq!(f.target(&f.b), legacy);
    assert_version(&legacy, "ONE");
    assert!(f.undo(&f.a, &sync)["conflicts"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(f.target(&f.a), legacy);
    assert_version(&f.a.join(LINK), "ONE");
    assert_eq!(skill_action(&f.personal(&f.a, "plan")), "update");
}

#[test]
fn immutable_store_reuses_matching_content_and_preserves_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), "VERSION ONE").unwrap();
    let digest = store::dir_digest(&source).unwrap();
    let identity = store::skill_revision_identity("local:/test-library", &digest);
    let root = tmp.path().join("store");
    let rel = Path::new("nested/skill");
    let (dest, first_digest) =
        store::materialize_skill_dir(&root, &identity, rel, &source).unwrap();
    assert_eq!(first_digest, digest);
    let before = tree_files(&dest);
    assert_eq!(
        store::materialize_skill_dir(&root, &identity, rel, &source).unwrap(),
        (dest.clone(), digest.clone())
    );
    assert_eq!(tree_files(&dest), before);
    std::fs::write(dest.join("SKILL.md"), "USER MODIFIED").unwrap();
    let modified = tree_files(&dest);
    let error = store::materialize_skill_dir(&root, &identity, rel, &source).unwrap_err();
    assert_eq!(error.code, ailoom::error::code::USER_CONTENT_CONFLICT);
    assert_eq!(tree_files(&dest), modified);
    std::fs::write(source.join("SKILL.md"), "SOURCE MOVED AFTER PLAN").unwrap();
    let error = store::materialize_skill_dir(&root, &identity, rel, &source).unwrap_err();
    assert_eq!(error.code, ailoom::error::code::PRECONDITION_FAILED);
    assert_eq!(tree_files(&dest), modified);
    let git = "git+https://example.test/org/skills#abc123";
    assert_eq!(store::skill_revision_identity(git, &digest), git);
}
