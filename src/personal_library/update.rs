//! Personal Skill updates: inspect a fixed snapshot, then replace with rollback.
use super::{recovery, skill_dir_digest, skill_git_cache};
use crate::error::{code, Error, Result};
use crate::ids::now_iso;
use crate::manifest::valid_name;
use crate::skill_source::{SkillSourceMeta, UpdateStatus};
use crate::source::{Snapshot, SourceLock};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct CheckedUpdate {
    source: Option<SkillSourceMeta>,
    snapshot: Option<SourceLock>,
    status: UpdateStatus,
}

impl CheckedUpdate {
    /// 本地来源没有快照锁（返回 None）；内容由执行时的摘要比对固定。
    fn candidate(&self) -> Result<(&SkillSourceMeta, Option<&SourceLock>)> {
        let invalid = || Error::new(code::SOURCE_CACHE_CORRUPT, "更新候选不完整，请重新检查");
        let source = self.source.as_ref().ok_or_else(invalid)?;
        if self.status.local_digest.as_deref() != Some(source.imported_digest.as_str())
            || self.status.upstream_digest.is_none()
            || self.status.preview_id.is_none()
        {
            return Err(invalid());
        }
        if source.local_dir().is_some() {
            return Ok((source, None));
        }
        let lock = self.snapshot.as_ref().ok_or_else(invalid)?;
        if lock.resolved_commit.is_none() || lock.resolved_commit != self.status.upstream_commit {
            return Err(invalid());
        }
        Ok((source, Some(lock)))
    }

    fn fail(&mut self, note: String) {
        self.snapshot = None;
        self.status.state = "error".into();
        self.status.preview_id = None;
        self.status.note = Some(note);
    }

    fn local_matches(&self, target: &Path) -> Result<bool> {
        if self.source != crate::skill_source::read_meta(target) {
            return Ok(false);
        }
        match &self.status.local_digest {
            Some(digest) => Ok(&skill_dir_digest(target)? == digest),
            None => Ok(self.status.state == "not-applicable"),
        }
    }
}

fn skill_path(data: &Path, name: &str) -> Result<PathBuf> {
    if !valid_name(name) {
        return Err(Error::new(code::USAGE, "Skill 名称无效"));
    }
    let path = super::library_root(data)
        .join("resources/skills")
        .join(name);
    if !path.exists() {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("库内不存在 skill: {name}"),
        ));
    }
    if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(Error::new(
            code::ILLEGAL_PATH,
            "外部目录引用不能由资源库更新",
        ));
    }
    Ok(path)
}

fn check_path(data: &Path, name: &str) -> PathBuf {
    data.join("library-updates").join(format!("{name}.json"))
}

fn upstream_path(snapshot: &Snapshot, source: &SkillSourceMeta) -> Result<PathBuf> {
    let relative = Path::new(source.repo_path.as_deref().unwrap_or(""));
    if relative.is_absolute()
        || relative
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(Error::new(code::PATH_TRAVERSAL, "Skill 的上游路径越界"));
    }
    let path = snapshot.root.join(relative);
    // 上游可能把目录换成符号链接指到仓库外：解析后必须仍在快照内。
    if let (Ok(real), Ok(root)) = (path.canonicalize(), snapshot.root.canonicalize()) {
        if !real.starts_with(&root) {
            return Err(Error::new(
                code::PATH_TRAVERSAL,
                "Skill 的上游目录经符号链接指向仓库外，拒绝更新",
            )
            .fix("上游仓库需要把该目录恢复为真实目录后再检查更新"));
        }
    }
    Ok(path)
}

fn save_check(data: &Path, checked: &CheckedUpdate) -> Result<()> {
    let path = check_path(data, &checked.status.skill);
    std::fs::create_dir_all(path.parent().unwrap())?;
    crate::sync_common::atomic_write(&path, &serde_json::to_vec_pretty(checked)?)
}

fn load_check(data: &Path, name: &str) -> Result<Option<CheckedUpdate>> {
    let path = check_path(data, name);
    if !path.exists() {
        return Ok(None);
    }
    let checked: CheckedUpdate = serde_json::from_slice(&std::fs::read(path)?)?;
    if checked.status.skill != name {
        return Err(Error::new(
            code::SOURCE_CACHE_CORRUPT,
            "检查记录与 Skill 名称不一致，请重新检查",
        ));
    }
    Ok(Some(checked))
}

// All supported discovery providers resolve to Git. Keep transport selection here.
fn resolve_upstream(
    data: &Path,
    source: &SkillSourceMeta,
    lock: Option<&SourceLock>,
) -> Result<Snapshot> {
    let git = crate::source::GitSource::new(
        source.repo_url.as_deref().unwrap_or(""),
        source.ref_.as_deref(),
    )?;
    git.resolve(&skill_git_cache(data, &git.identity), lock)
}

/// List saved checks without fetching; local edits invalidate the candidate.
pub fn cached_update(data: &Path, name: &str) -> Result<Option<UpdateStatus>> {
    with_update_lock(data, || cached_update_locked(data, name))
}

fn cached_update_locked(data: &Path, name: &str) -> Result<Option<UpdateStatus>> {
    let target = skill_path(data, name)?;
    let Some(mut checked) = load_check(data, name)? else {
        return Ok(None);
    };
    if checked.status.state == "upstream-new" {
        checked.candidate()?;
    }
    if checked.status.state != "error" && !checked.local_matches(&target)? {
        checked.status.state = "stale".into();
        checked.status.preview_id = None;
        checked.status.note = Some("Skill 在检查后已变化，请重新检查".into());
    }
    Ok(Some(checked.status))
}

/// Check only persists the exact candidate, leaving the installed Skill intact.
pub fn check_update(data: &Path, name: &str) -> Result<UpdateStatus> {
    with_update_lock(data, || {
        let target = skill_path(data, name)?;
        let source = crate::skill_source::read_meta(&target);
        let mut checked = CheckedUpdate {
            status: UpdateStatus::failed(name, source.as_ref(), "检查未完成，请重新检查".into()),
            source,
            snapshot: None,
        };
        // Retire the previous candidate before fetching, including on failed checks.
        save_check(data, &checked)?;
        let outcome = inspect_update(data, name).and_then(|candidate| {
            if !candidate.local_matches(&target)? {
                return Err(Error::new(
                    code::PRECONDITION_FAILED,
                    "Skill 在检查期间已变化，请重新检查",
                ));
            }
            Ok(candidate)
        });
        match outcome {
            Ok(candidate) => {
                save_check(data, &candidate)?;
                Ok(candidate.status)
            }
            Err(error) => {
                checked.fail(error.to_string());
                save_check(data, &checked)?;
                Err(error)
            }
        }
    })
}

/// 上游检查（AIL-066）：比较 导入基线/本地现状/上游最新；只检查，不应用。
fn inspect_update(data_root: &Path, name: &str) -> Result<CheckedUpdate> {
    let skill_dir = skill_path(data_root, name)?;
    if !skill_dir.is_dir() {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("库内不存在 skill: {name}"),
        ));
    }
    let source = crate::skill_source::read_meta(&skill_dir);
    let mut status = UpdateStatus::for_skill(name, source.as_ref());
    let Some(meta) = source.as_ref().filter(|s| s.updatable()) else {
        status.note = Some(
            if source.is_some() {
                "本地来源：无上游可检查"
            } else {
                "旧数据无来源记录：按本地管理，不伪称能上游更新"
            }
            .into(),
        );
        return Ok(CheckedUpdate {
            source,
            snapshot: None,
            status,
        });
    };
    let local_digest = skill_dir_digest(&skill_dir)?;
    status.local_digest = Some(local_digest.clone());
    let (upstream_dir, snap) = match meta.local_dir() {
        Some(dir) => (dir, None),
        None => {
            let snap = resolve_upstream(data_root, meta, None)?;
            (upstream_path(&snap, meta)?, Some(snap))
        }
    };
    status.upstream_commit = snap.as_ref().and_then(|s| s.resolved_commit.clone());
    if !upstream_dir.join("SKILL.md").is_file() {
        status.state = "upstream-missing".into();
        status.note = Some("上游 skill 目录已不存在（可能删除/改名）".into());
        return Ok(CheckedUpdate {
            source,
            snapshot: None,
            status,
        });
    }
    let upstream_digest = super::normalized_digest(data_root, &upstream_dir, name)?;
    status.state = match (
        local_digest == meta.imported_digest,
        upstream_digest == meta.imported_digest,
    ) {
        (true, true) => "up-to-date",
        (false, true) => "local-modified",
        (true, false) => "upstream-new",
        (false, false) => "conflict",
    }
    .into();
    status.upstream_digest = Some(upstream_digest);
    status.preview_id = (status.state == "upstream-new").then(crate::ids::new_id);
    status.note = Some(format!(
        "入口 {}（只检查未应用；检查不等于更新）",
        meta.discovery_entry
    ));
    let lock = snap.map(|snap| SourceLock {
        kind: "git".into(),
        identity: snap.identity,
        ref_: snap.ref_,
        resolved_commit: snap.resolved_commit,
        content_digest: snap.content_digest,
        locked_at: snap.locked_at,
    });
    Ok(CheckedUpdate {
        source,
        snapshot: lock,
        status,
    })
}

/// Compatibility calls may check once when there is no saved candidate.
pub fn update_execute(data: &Path, name: &str) -> Result<serde_json::Value> {
    super::recover_updates(data)?;
    skill_path(data, name)?;
    if load_check(data, name)?.is_none() {
        check_update(data, name)?;
    }
    apply_checked(data, name, None)
}

/// Apply exactly the candidate identified by the caller; never starts a new check.
pub fn update_execute_checked(
    data: &Path,
    name: &str,
    preview_id: &str,
) -> Result<serde_json::Value> {
    apply_checked(data, name, Some(preview_id))
}

fn apply_checked(data: &Path, name: &str, preview_id: Option<&str>) -> Result<serde_json::Value> {
    with_update_lock(data, || {
        skill_path(data, name)?;
        let mut checked = load_check(data, name)?
            .ok_or_else(|| Error::new(code::PRECONDITION_FAILED, "请先检查 Skill 更新"))?;
        if preview_id.is_some_and(|id| checked.status.preview_id.as_deref() != Some(id)) {
            return Err(
                Error::new(code::PRECONDITION_FAILED, "更新预览已变化，请重新检查").fix(format!(
                    "运行 ailoom library --action check-update --skill {name} 获取新的 preview_id"
                )),
            );
        }
        crate::skill_source::ensure_updatable(&checked.status)?;
        let result = match prepare_and_replace(data, name, &checked) {
            Ok((next_source, backup)) => {
                let result = serde_json::json!({
                    "updated": true, "skill": name,
                    "from_commit": checked.source.as_ref().and_then(|s| s.resolved_commit.as_ref()),
                    "to_commit": next_source.resolved_commit, "backup": backup,
                    "note": "资源库已更新；请到需要升级的项目应用改动",
                });
                checked.status.state = "up-to-date".into();
                checked.status.imported_digest = next_source.imported_digest.clone();
                checked.status.local_digest = Some(next_source.imported_digest.clone());
                checked.status.preview_id = None;
                checked.status.note = Some("资源库已更新；请到需要升级的项目应用改动".into());
                checked.source = Some(next_source);
                Ok(result)
            }
            Err(error) => {
                checked.fail(error.to_string());
                Err(error)
            }
        };
        // Preserve the actual replacement outcome if only saving display state fails.
        let _ = save_check(data, &checked);
        result
    })
}

fn prepare_and_replace(
    data: &Path,
    name: &str,
    checked: &CheckedUpdate,
) -> Result<(SkillSourceMeta, PathBuf)> {
    let (source, lock) = checked.candidate()?;
    let target = skill_path(data, name)?;
    let ensure_local = || -> Result<()> {
        if checked.local_matches(&target)? {
            return Ok(());
        }
        Err(Error::new(
            code::PRECONDITION_FAILED,
            "Skill 在检查后已变化，请重新检查；本地内容保留",
        ))
    };
    ensure_local()?;
    let (upstream, resolved_commit) = match (source.local_dir(), lock) {
        (Some(dir), _) => (dir, None),
        (None, Some(lock)) => {
            let snapshot = resolve_upstream(data, source, Some(lock))?;
            (upstream_path(&snapshot, source)?, snapshot.resolved_commit)
        }
        (None, None) => {
            return Err(Error::new(
                code::SOURCE_CACHE_CORRUPT,
                "更新候选不完整，请重新检查",
            ))
        }
    };
    let stage = recovery::stage(data)?;
    super::copy_normalized(data, &upstream, name, stage.path())?;
    let digest = skill_dir_digest(stage.path())?;
    if Some(&digest) != checked.status.upstream_digest.as_ref() {
        return Err(Error::new(
            code::SOURCE_CACHE_CORRUPT,
            "候选 Skill 内容已变化，请重新检查",
        ));
    }
    let mut next_source = source.clone();
    next_source.resolved_commit = resolved_commit;
    next_source.imported_digest = digest;
    next_source.fetched_at = now_iso();
    crate::skill_source::write_meta(stage.path(), &next_source)?;
    let backup = recovery::publish(data, name, stage, ensure_local)?;
    Ok((next_source, backup))
}

fn with_update_lock<T>(data: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    crate::source::with_file_lock(&data.join("library-update.lock"), || {
        recovery::recover_locked(data)?;
        f()
    })
}
