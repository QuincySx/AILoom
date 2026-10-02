//! Directory publication journal. Callers hold library-update.lock throughout.
use super::{library_root, LIBRARY_TEAM_ID};
use crate::error::{code, Error, Result};
use crate::manifest::{valid_name, TeamManifest};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

const JOURNAL: &str = ".updates-journal.json";

#[derive(Serialize, Deserialize)]
struct Journal {
    schema_version: u32,
    skill: String,
    stage: String,
    backup: String,
    original_digest: String,
    candidate_digest: String,
    committed: bool,
}

#[derive(Debug, Serialize)]
pub struct Recovery {
    pub skill: String,
    pub outcome: &'static str,
    pub installed: PathBuf,
    pub backup: Option<PathBuf>,
    pub retained_stage: Option<PathBuf>,
}

pub fn recover_updates(data: &Path) -> Result<Option<Recovery>> {
    crate::source::with_file_lock(&data.join("library-update.lock"), || recover_locked(data))
}

pub(super) fn stage(data: &Path) -> Result<tempfile::TempDir> {
    let lib = library_root(data);
    for dir in [&lib, &lib.join("resources"), &lib.join("resources/skills")] {
        if !directory(dir)? {
            return Err(Error::new(code::PRECONDITION_FAILED, "资源库目录不存在"));
        }
    }
    for dir in [lib.join(".updates-staging"), lib.join(".updates-backup")] {
        if !directory(&dir)? {
            std::fs::create_dir(&dir)?;
        }
    }
    Ok(tempfile::tempdir_in(lib.join(".updates-staging"))?)
}

fn directory(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(Error::new(
            code::ILLEGAL_PATH,
            format!("更新恢复路径不是自有目录：{}", path.display()),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

// Include every file (including source metadata) and empty directory. A recovered
// version must not hide edits that the upstream content digest intentionally skips.
fn directory_digest(path: &Path) -> Result<Option<String>> {
    if !directory(path)? {
        return Ok(None);
    }
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(path).min_depth(1) {
        let entry = entry.map_err(|e| Error::new(code::JOURNAL_RESTORE_FAILED, e.to_string()))?;
        let kind = entry.file_type();
        let hash = if kind.is_file() {
            Some(crate::ids::sha256_hex(&std::fs::read(entry.path())?))
        } else if kind.is_dir() {
            None
        } else {
            return Err(Error::new(
                code::ILLEGAL_PATH,
                format!("恢复目录含符号链接或特殊文件：{}", entry.path().display()),
            ));
        };
        let relative = entry.path().strip_prefix(path).unwrap().to_path_buf();
        entries.push((relative, hash));
    }
    entries.sort();
    Ok(Some(crate::ids::sha256_hex(&serde_json::to_vec(&entries)?)))
}

fn component(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

impl Journal {
    fn paths(&self, lib: &Path) -> Result<(PathBuf, PathBuf, PathBuf)> {
        if self.schema_version != 1
            || !valid_name(&self.skill)
            || !component(&self.stage)
            || !component(&self.backup)
            || !self.backup.starts_with(&format!("{}-", self.skill))
        {
            return Err(Error::new(
                code::SOURCE_CACHE_CORRUPT,
                "Skill 更新恢复日志无效",
            ));
        }
        for dir in [
            lib.to_path_buf(),
            lib.join("resources"),
            lib.join("resources/skills"),
            lib.join(".updates-staging"),
            lib.join(".updates-backup"),
        ] {
            if !directory(&dir)? {
                return Err(Error::new(
                    code::JOURNAL_RESTORE_FAILED,
                    format!("恢复父目录不存在：{}", dir.display()),
                ));
            }
        }
        Ok((
            lib.join("resources/skills").join(&self.skill),
            lib.join(".updates-staging").join(&self.stage),
            lib.join(".updates-backup").join(&self.backup),
        ))
    }

    fn save(&self, lib: &Path) -> Result<()> {
        crate::sync_common::atomic_write(&lib.join(JOURNAL), &serde_json::to_vec_pretty(self)?)?;
        sync_directory(lib)
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Idempotent across interruptions, including an interruption during rollback.
pub(super) fn recover_locked(data: &Path) -> Result<Option<Recovery>> {
    let lib = library_root(data);
    if !directory(&lib)? {
        return Ok(None);
    }
    let journal_path = lib.join(JOURNAL);
    match std::fs::symlink_metadata(&journal_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(Error::new(
                code::SOURCE_CACHE_CORRUPT,
                "更新恢复日志不是普通文件",
            ))
        }
        Err(e) => return Err(e.into()),
    }
    let journal: Journal = serde_json::from_slice(&std::fs::read(&journal_path)?).map_err(|e| {
        Error::new(
            code::SOURCE_CACHE_CORRUPT,
            format!("更新恢复日志不可读：{e}"),
        )
    })?;
    let (target, stage, backup) = journal.paths(&lib)?;
    let local = directory_digest(&target)?;
    let conflict = || {
        Error::new(
            code::PRECONDITION_FAILED,
            format!(
                "Skill {} 中断后的目录已变化，保留现场；旧版位置 {}，日志 {}",
                journal.skill,
                backup.display(),
                journal_path.display()
            ),
        )
    };
    let outcome = if journal.committed {
        // Validation finished before the durable marker. Later edits belong to
        // the user; never replace them with the candidate or the backup.
        if local.is_none() {
            return Err(conflict());
        }
        "committed"
    } else {
        let saved = directory_digest(&backup)?;
        if saved.is_none() && local.as_ref() == Some(&journal.original_digest) {
            // Before the first rename, or after rollback but before journal cleanup.
        } else if saved.as_ref() == Some(&journal.original_digest) {
            if local.as_ref() == Some(&journal.candidate_digest) {
                if directory(&stage)? {
                    return Err(conflict());
                }
                std::fs::rename(&target, &stage)?;
            } else if local.is_some() {
                return Err(conflict());
            }
            std::fs::rename(&backup, &target)?;
            sync_directory(target.parent().unwrap())?;
            sync_directory(backup.parent().unwrap())?;
        } else {
            return Err(conflict());
        }
        "restored"
    };
    // Retire any pre-interruption preview before removing the recovery evidence.
    let check = data
        .join("library-updates")
        .join(format!("{}.json", journal.skill));
    directory(check.parent().unwrap())?;
    match std::fs::remove_file(&check) {
        Ok(()) => sync_directory(check.parent().unwrap())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    std::fs::remove_file(&journal_path)?;
    sync_directory(&lib)?;
    Ok(Some(Recovery {
        skill: journal.skill,
        outcome,
        installed: target,
        backup: backup.exists().then_some(backup),
        retained_stage: stage.exists().then_some(stage),
    }))
}

/// Persist intent before moving the installed directory; commit only after validation.
pub(super) fn publish(
    data: &Path,
    name: &str,
    stage: tempfile::TempDir,
    ensure_local: impl FnOnce() -> Result<()>,
) -> Result<PathBuf> {
    let lib = library_root(data);
    let mut journal = Journal {
        schema_version: 1,
        skill: name.into(),
        stage: stage.path().file_name().unwrap().to_string_lossy().into(),
        backup: format!("{name}-{}", crate::ids::new_id()),
        original_digest: String::new(),
        candidate_digest: String::new(),
        committed: false,
    };
    let (target, staged, backup) = journal.paths(&lib)?;
    journal.original_digest = directory_digest(&target)?
        .ok_or_else(|| Error::new(code::PRECONDITION_FAILED, "更新前 Skill 已不存在"))?;
    journal.candidate_digest = directory_digest(&staged)?
        .ok_or_else(|| Error::new(code::SOURCE_CACHE_CORRUPT, "暂存候选已不存在"))?;
    ensure_local()?;
    journal.save(&lib)?;
    // From this point only journal recovery owns the staged directory. A normal
    // return must not let TempDir remove evidence needed after a failed rollback.
    let _ = stage.keep();
    let replacement = (|| -> Result<()> {
        std::fs::rename(&target, &backup)?;
        std::fs::rename(&staged, &target)?;
        let manifest = TeamManifest::load_from(&lib)?;
        // 只要求被更新的这个 Skill 有效；库内其他坏条目不阻止更新（AIL-062）。
        let (_, invalid) = crate::resource::enumerate_isolating(
            &lib,
            &manifest,
            LIBRARY_TEAM_ID,
            &mut Vec::new(),
        )?;
        if let Some(bad) = invalid.into_iter().find(|e| lib.join(&e.path) == target) {
            return Err(bad.error);
        }
        sync_directory(target.parent().unwrap())?;
        sync_directory(backup.parent().unwrap())?;
        sync_directory(staged.parent().unwrap())?;
        journal.committed = true;
        journal.save(&lib)
    })();
    if let Err(error) = replacement {
        let recovery = recover_locked(data).map_err(|restore| {
            Error::new(
                code::JOURNAL_RESTORE_FAILED,
                format!("更新失败，恢复未完成；旧版 {}：{restore}", backup.display()),
            )
        })?;
        // A durable commit may have succeeded even if syncing its directory failed.
        if recovery.is_some_and(|r| r.outcome == "committed") {
            return Ok(backup);
        }
        return Err(Error::new(
            code::WRITE_FAILED,
            format!("更新失败，已恢复旧版：{error}"),
        ));
    }
    // The commit is authoritative even if retiring the cache/cleaning the journal
    // fails. The next access retries that cleanup while preserving this version.
    let _ = recover_locked(data);
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        data: tempfile::TempDir,
        lib: PathBuf,
        target: PathBuf,
        stage: PathBuf,
        backup: PathBuf,
        journal: Journal,
    }

    impl Fixture {
        fn new() -> Self {
            let data = tempfile::tempdir().unwrap();
            let lib = library_root(data.path());
            let target = lib.join("resources/skills/solo");
            let stage = lib.join(".updates-staging/candidate");
            let backup = lib.join(".updates-backup/solo-backup");
            std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
            for (path, body) in [(&target, "v1"), (&stage, "v2")] {
                std::fs::create_dir_all(path).unwrap();
                std::fs::write(path.join("SKILL.md"), format!(
                    "---\nname: solo\ndescription: solo\nnamespace: personal\nshared: true\n---\n{body}\n"
                )).unwrap();
                std::fs::write(path.join(crate::skill_source::IMPORT_META_FILE), body).unwrap();
            }
            std::fs::write(
                lib.join(crate::manifest::MANIFEST_FILE),
                super::super::manifest_text(),
            )
            .unwrap();
            let checks = data.path().join("library-updates");
            std::fs::create_dir_all(&checks).unwrap();
            std::fs::write(checks.join("solo.json"), "old preview").unwrap();
            let journal = Journal {
                schema_version: 1,
                skill: "solo".into(),
                stage: "candidate".into(),
                backup: "solo-backup".into(),
                original_digest: directory_digest(&target).unwrap().unwrap(),
                candidate_digest: directory_digest(&stage).unwrap().unwrap(),
                committed: false,
            };
            journal.save(&lib).unwrap();
            Self {
                data,
                lib,
                target,
                stage,
                backup,
                journal,
            }
        }

        fn interrupt(&mut self, after: u8) {
            if after >= 1 {
                std::fs::rename(&self.target, &self.backup).unwrap();
            }
            if after >= 2 {
                std::fs::rename(&self.stage, &self.target).unwrap();
            }
            if after >= 3 {
                self.journal.committed = true;
                self.journal.save(&self.lib).unwrap();
            }
        }

        fn assert_retired(&self) {
            assert!(!self.lib.join(JOURNAL).exists());
            assert!(!self.data.path().join("library-updates/solo.json").exists());
            assert!(recover_updates(self.data.path()).unwrap().is_none());
        }
    }

    #[test]
    fn recover_every_publication_gap_and_retry() {
        for after in 0..=3 {
            let mut f = Fixture::new();
            f.interrupt(after);
            let recovery = recover_updates(f.data.path()).unwrap().unwrap();
            let committed = after == 3;
            assert_eq!(
                recovery.outcome,
                if committed { "committed" } else { "restored" }
            );
            assert_eq!(
                directory_digest(&f.target).unwrap().as_ref(),
                Some(if committed {
                    &f.journal.candidate_digest
                } else {
                    &f.journal.original_digest
                })
            );
            assert_eq!(f.backup.exists(), committed);
            assert_eq!(recovery.backup.is_some(), committed);
            assert_eq!(recovery.retained_stage.is_some(), !committed);
            f.assert_retired();
        }
    }

    #[test]
    fn recover_an_interrupted_rollback() {
        for restored in [false, true] {
            let mut f = Fixture::new();
            f.interrupt(2);
            std::fs::rename(&f.target, &f.stage).unwrap();
            if restored {
                std::fs::rename(&f.backup, &f.target).unwrap();
            }
            assert_eq!(
                recover_updates(f.data.path()).unwrap().unwrap().outcome,
                "restored"
            );
            assert_eq!(
                directory_digest(&f.target).unwrap().as_ref(),
                Some(&f.journal.original_digest)
            );
            assert_eq!(
                directory_digest(&f.stage).unwrap().as_ref(),
                Some(&f.journal.candidate_digest)
            );
            f.assert_retired();
        }
    }

    #[test]
    fn uncommitted_recovery_preserves_later_edits_and_evidence() {
        for (after, file) in [
            (0, "SKILL.md"),
            (2, "SKILL.md"),
            (2, crate::skill_source::IMPORT_META_FILE),
        ] {
            let mut f = Fixture::new();
            f.interrupt(after);
            std::fs::write(f.target.join(file), "user edit").unwrap();
            let edited = directory_digest(&f.target).unwrap();
            let backup = directory_digest(&f.backup).unwrap();
            let error = recover_updates(f.data.path()).unwrap_err();
            assert_eq!(error.code, code::PRECONDITION_FAILED);
            assert_eq!(directory_digest(&f.target).unwrap(), edited);
            assert_eq!(directory_digest(&f.backup).unwrap(), backup);
            assert!(f.lib.join(JOURNAL).exists());
        }
        let mut f = Fixture::new();
        f.interrupt(2);
        std::fs::create_dir(f.target.join("user-directory")).unwrap();
        assert!(recover_updates(f.data.path()).is_err());
        assert!(f.target.join("user-directory").is_dir());
    }

    #[test]
    fn committed_recovery_preserves_later_edits() {
        let mut f = Fixture::new();
        f.interrupt(3);
        std::fs::write(f.target.join("SKILL.md"), "user edit").unwrap();
        let edited = directory_digest(&f.target).unwrap();
        assert_eq!(
            recover_updates(f.data.path()).unwrap().unwrap().outcome,
            "committed"
        );
        assert_eq!(directory_digest(&f.target).unwrap(), edited);
        assert!(f.backup.exists());
        f.assert_retired();
    }

    #[test]
    fn changed_or_missing_backup_is_not_restored() {
        for missing in [false, true] {
            let mut f = Fixture::new();
            f.interrupt(2);
            if missing {
                std::fs::rename(&f.backup, f.lib.join("saved-elsewhere")).unwrap();
            } else {
                std::fs::write(
                    f.backup.join(crate::skill_source::IMPORT_META_FILE),
                    "backup edited",
                )
                .unwrap();
            }
            assert!(recover_updates(f.data.path()).is_err());
            assert_eq!(
                directory_digest(&f.target).unwrap().as_ref(),
                Some(&f.journal.candidate_digest)
            );
            assert!(f.lib.join(JOURNAL).exists());
        }
    }

    #[test]
    fn invalid_journal_does_not_touch_directories() {
        for invalid in ["json", "path", "version"] {
            let mut f = Fixture::new();
            f.interrupt(1);
            match invalid {
                "json" => std::fs::write(f.lib.join(JOURNAL), "{").unwrap(),
                "path" => {
                    f.journal.stage = "../outside".into();
                    f.journal.save(&f.lib).unwrap();
                }
                _ => {
                    f.journal.schema_version = 2;
                    f.journal.save(&f.lib).unwrap();
                }
            }
            assert_eq!(
                recover_updates(f.data.path()).unwrap_err().code,
                code::SOURCE_CACHE_CORRUPT
            );
            assert!(!f.target.exists());
            assert!(f.backup.is_dir());
            assert!(f.lib.join(JOURNAL).exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn redirected_recovery_paths_are_rejected() {
        for child in [".updates-backup", ".updates-staging", "resources/skills"] {
            let mut f = Fixture::new();
            f.interrupt(1);
            let original = f.lib.join(child);
            let moved = f.data.path().join("redirected");
            std::fs::rename(&original, &moved).unwrap();
            std::os::unix::fs::symlink(&moved, &original).unwrap();
            assert_eq!(
                recover_updates(f.data.path()).unwrap_err().code,
                code::ILLEGAL_PATH
            );
            assert!(f.lib.join(JOURNAL).exists());
        }
    }

    #[test]
    fn list_restores_a_missing_skill_before_scanning() {
        let mut f = Fixture::new();
        f.interrupt(1);
        let (entries, issues) = super::super::list_tolerant(f.data.path());
        assert!(issues.is_empty(), "{:?}", issues);
        assert!(entries.iter().any(|e| e.name == "solo"));
        f.assert_retired();
    }

    #[test]
    fn apply_restores_then_rejects_the_interrupted_preview() {
        let mut f = Fixture::new();
        f.interrupt(1);
        let error =
            super::super::update_execute_checked(f.data.path(), "solo", "old preview").unwrap_err();
        assert_eq!(error.code, code::PRECONDITION_FAILED);
        assert_eq!(
            directory_digest(&f.target).unwrap().as_ref(),
            Some(&f.journal.original_digest)
        );
        f.assert_retired();
    }
}
