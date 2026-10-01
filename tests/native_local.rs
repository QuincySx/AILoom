use ailoom::native_files::{self, Request};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    home: PathBuf,
}
fn git(root: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["--literal-pathspecs", "-C"])
        .arg(root)
        .args([
            "-c",
            "user.name=AILoom Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}
impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("AGENTS.md"), "Company instructions\n").unwrap();
        git(&root, &["add", "AGENTS.md"]);
        git(&root, &["commit", "-qm", "initial"]);
        let home = base.join("home");
        std::fs::create_dir(&home).unwrap();
        Self {
            _tmp: tmp,
            root,
            data: base.join("data"),
            home,
        }
    }
    fn call_at(&self, root: &Path, mut body: Value) -> ailoom::error::Result<Value> {
        body["scope"] = json!("project");
        body["root"] = json!(root);
        native_files::run(
            &self.data,
            &self.home,
            &serde_json::from_value::<Request>(body).unwrap(),
        )
    }
    fn call(&self, body: Value) -> ailoom::error::Result<Value> {
        self.call_at(&self.root, body)
    }
    fn read(&self, target: &str, name: &str) -> Value {
        self.call(json!({"action":"read","target":target,"name":name}))
            .unwrap()
    }
    fn save(
        &self,
        target: &str,
        name: &str,
        mode: &str,
        content: &str,
    ) -> ailoom::error::Result<Value> {
        let current = self.read(target, name);
        self.call(json!({"action":"save","target":target,"name":name,"expected":current["expected"],"mode":mode,"content":content}))
    }
    fn restore(&self, target: &str, name: &str) -> ailoom::error::Result<Value> {
        let current = self.read(target, name);
        self.call(
            json!({"action":"restore","target":target,"name":name,"expected":current["expected"]}),
        )
    }
}
#[test]
fn tracked_local_edit_hides_from_normal_add_and_restores_without_losing_personal_copy() {
    let f = Fixture::new();
    let original = git(&f.root, &["rev-parse", "HEAD:AGENTS.md"]);
    assert!(f.read("codex-instructions", "")["local"]["tracked"]
        .as_bool()
        .unwrap());
    f.save("codex-instructions", "", "local", "Personal instructions\n")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(f.root.join("AGENTS.md")).unwrap(),
        "Personal instructions\n"
    );
    assert!(git(&f.root, &["ls-files", "-v", "AGENTS.md"]).starts_with('S'));
    git(&f.root, &["add", "-A"]);
    assert!(git(&f.root, &["diff", "--cached", "--name-only"]).is_empty());
    assert_eq!(git(&f.root, &["rev-parse", ":AGENTS.md"]), original);
    f.save(
        "codex-instructions",
        "",
        "local",
        "Updated personal instructions\n",
    )
    .unwrap();
    f.restore("codex-instructions", "").unwrap();
    assert_eq!(
        std::fs::read_to_string(f.root.join("AGENTS.md")).unwrap(),
        "Company instructions\n"
    );
    assert!(git(&f.root, &["status", "--porcelain"]).is_empty());
    let r = f.read("codex-instructions", "");
    assert_eq!(r["local"]["active"], false);
    assert_eq!(
        r["local"]["personal_content"],
        "Updated personal instructions\n"
    );
    f.save("codex-instructions", "", "local", "Personal again\n")
        .unwrap();
    f.save(
        "codex-instructions",
        "",
        "project",
        "Intentional shared edit\n",
    )
    .unwrap();
    assert!(git(&f.root, &["ls-files", "-v", "AGENTS.md"]).starts_with('H'));
    assert!(git(&f.root, &["status", "--porcelain"]).contains("AGENTS.md"));
}
#[test]
fn local_new_files_use_exact_repo_excludes_and_restore_existing_untracked_content() {
    let f = Fixture::new();
    let exclude = f.root.join(".git/info/exclude");
    std::fs::write(&exclude, "# User rules\n*.log\n").unwrap();
    f.save("claude-rules", "ui [one].md", "local", "Personal rule")
        .unwrap();
    git(&f.root, &["add", "-A"]);
    assert!(git(&f.root, &["diff", "--cached", "--name-only"]).is_empty());
    assert!(!f.root.join(".gitignore").exists());
    std::fs::write(f.root.join(".claude/rules/ui o.md"), "unrelated").unwrap();
    assert!(git(&f.root, &["status", "--porcelain", "--untracked-files=all"]).contains("ui o.md"));
    f.restore("claude-rules", "ui [one].md").unwrap();
    assert!(!f.root.join(".claude/rules/ui [one].md").exists());
    assert_eq!(
        std::fs::read_to_string(&exclude).unwrap(),
        "# User rules\n*.log\n"
    );
    std::fs::write(f.root.join("CLAUDE.md"), "Original untracked").unwrap();
    f.save("claude-instructions", "", "local", "Personal")
        .unwrap();
    f.restore("claude-instructions", "").unwrap();
    assert_eq!(
        std::fs::read_to_string(f.root.join("CLAUDE.md")).unwrap(),
        "Original untracked"
    );
}
#[test]
fn dirty_staged_preexisting_flags_and_stale_edit_are_not_hidden() {
    let f = Fixture::new();
    std::fs::write(f.root.join("AGENTS.md"), "Unsaved work").unwrap();
    assert!(f
        .save("codex-instructions", "", "local", "Personal")
        .is_err());
    git(&f.root, &["add", "AGENTS.md"]);
    assert!(f
        .save("codex-instructions", "", "local", "Personal")
        .is_err());
    assert_eq!(
        std::fs::read_to_string(f.root.join("AGENTS.md")).unwrap(),
        "Unsaved work"
    );
    git(&f.root, &["commit", "-qm", "existing user change"]);
    git(
        &f.root,
        &["update-index", "--assume-unchanged", "AGENTS.md"],
    );
    assert!(f
        .save("codex-instructions", "", "local", "Personal")
        .is_err());
    git(
        &f.root,
        &["update-index", "--no-assume-unchanged", "AGENTS.md"],
    );
    let old = f.read("codex-instructions", "");
    std::fs::write(f.root.join("AGENTS.md"), "External edit").unwrap();
    assert!(f.call(json!({"action":"save","target":"codex-instructions","expected":old["expected"],"mode":"local","content":"Personal"})).is_err());
}
#[test]
fn changed_git_baseline_refuses_restore_and_preserves_both_versions() {
    let f = Fixture::new();
    f.save("codex-instructions", "", "local", "Personal")
        .unwrap();
    git(
        &f.root,
        &["update-index", "--no-skip-worktree", "AGENTS.md"],
    );
    assert!(f.read("codex-instructions", "")["local"]["issue"].is_string());
    std::fs::write(f.root.join("AGENTS.md"), "New company instructions").unwrap();
    git(&f.root, &["add", "AGENTS.md"]);
    git(&f.root, &["commit", "-qm", "upstream update"]);
    assert!(f.restore("codex-instructions", "").is_err());
    let r = f.read("codex-instructions", "");
    assert_eq!(r["content"], "New company instructions");
    assert_eq!(r["local"]["personal_content"], "Personal");
}
#[test]
fn linked_worktrees_have_independent_index_flags_and_personal_copies() {
    let f = Fixture::new();
    let other = f.root.parent().unwrap().join("other");
    git(
        &f.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "other",
            other.to_str().unwrap(),
        ],
    );
    f.save("codex-instructions", "", "local", "Main personal")
        .unwrap();
    assert!(git(&other, &["ls-files", "-v", "AGENTS.md"]).starts_with('H'));
    let r = f
        .call_at(
            &other,
            json!({"action":"read","target":"codex-instructions"}),
        )
        .unwrap();
    f.call_at(&other,json!({"action":"save","target":"codex-instructions","expected":r["expected"],"mode":"local","content":"Other personal"})).unwrap();
    f.restore("codex-instructions", "").unwrap();
    assert_eq!(
        std::fs::read_to_string(other.join("AGENTS.md")).unwrap(),
        "Other personal"
    );
    assert!(git(&other, &["ls-files", "-v", "AGENTS.md"]).starts_with('S'));
}
#[test]
fn negated_gitignore_does_not_pretend_local_exclusion_worked() {
    let f = Fixture::new();
    std::fs::write(f.root.join(".gitignore"), "!CLAUDE.md\n").unwrap();
    assert!(f
        .save("claude-instructions", "", "local", "Private")
        .is_err());
    assert!(!f.root.join("CLAUDE.md").exists());
    let r = f.read("claude-instructions", "");
    assert_eq!(r["local"]["active"], false);
    assert_eq!(r["local"]["personal_content"], "Private");
}

#[cfg(unix)]
#[test]
fn executable_mode_and_external_personal_edits_survive_restore() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let path = f.root.join("AGENTS.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(&f.root, &["add", "AGENTS.md"]);
    git(&f.root, &["commit", "-qm", "executable instructions"]);
    f.save("codex-instructions", "", "local", "Personal")
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o755
    );
    std::fs::write(&path, "Edited outside AILoom").unwrap();
    f.restore("codex-instructions", "").unwrap();
    assert_eq!(
        f.read("codex-instructions", "")["local"]["personal_content"],
        "Edited outside AILoom"
    );
    assert!(git(&f.root, &["status", "--porcelain"]).is_empty());
}
#[test]
fn interrupted_restore_is_recoverable_and_keeps_personal_archive() {
    let f = Fixture::new();
    f.save("codex-instructions", "", "local", "Personal")
        .unwrap();
    let path = std::fs::read_dir(f.data.join("native-files/local"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["phase"] = json!("restoring");
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    std::fs::write(f.root.join("AGENTS.md"), "Company instructions\n").unwrap();
    f.restore("codex-instructions", "").unwrap();
    assert_eq!(
        f.read("codex-instructions", "")["local"]["personal_content"],
        "Personal"
    );
    assert!(git(&f.root, &["ls-files", "-v", "AGENTS.md"]).starts_with('H'));
}
#[test]
fn project_scoped_local_edits_do_not_modify_user_git_configuration() {
    let f = Fixture::new();
    let config = f.root.join(".git/config");
    let before = std::fs::read(&config).unwrap();
    f.save("codex-instructions", "", "local", "Personal")
        .unwrap();
    assert_eq!(std::fs::read(&config).unwrap(), before);
    let global:Request=serde_json::from_value(json!({"scope":"global","action":"save","target":"codex-instructions","mode":"local","content":"Personal","expected":"missing"})).unwrap();
    assert!(native_files::run(&f.data, &f.home, &global).is_err());
}

#[test]
fn index_lock_is_respected_and_only_ailoom_abandoned_locks_are_recovered() {
    let f = Fixture::new();
    let lock = f.root.join(".git/index.lock");
    std::fs::write(&lock, b"Another Git process").unwrap();
    assert!(f
        .save("codex-instructions", "", "local", "Personal")
        .is_err());
    assert_eq!(std::fs::read(&lock).unwrap(), b"Another Git process");
    // Simulate our own abandoned lock, not a lock belonging to another application.
    std::fs::write(&lock, b"AILoom native-files index guard\n").unwrap();
    f.save("codex-instructions", "", "local", "Personal")
        .unwrap();
    assert!(!lock.exists());
    f.restore("codex-instructions", "").unwrap();
    assert!(git(&f.root, &["status", "--porcelain"]).is_empty());
}

#[test]
fn missing_working_file_keeps_recovery_entry_and_last_personal_content() {
    let f = Fixture::new();
    f.save("codex-instructions", "", "local", "Personal")
        .unwrap();
    // Model an external deletion by moving the file to a recoverable test backup.
    std::fs::rename(f.root.join("AGENTS.md"), f.home.join("external-backup.md")).unwrap();
    let list = f.call(json!({"action":"list"})).unwrap();
    assert!(list["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["label"] == "AGENTS.md" && f["local_only"] == true && f["missing"] == true));
    f.restore("codex-instructions", "").unwrap();
    assert_eq!(
        f.read("codex-instructions", "")["local"]["personal_content"],
        "Personal"
    );
    assert!(git(&f.root, &["status", "--porcelain"]).is_empty());
}
