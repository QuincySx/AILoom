//! Recoverable per-worktree native-file replacement. Index flags are not a commit firewall.
use super::{check_path, fail, read};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Serialize, Deserialize)]
struct Record {
    path: PathBuf,
    root: PathBuf,
    git_dir: PathBuf,
    relative: String,
    index: Option<String>,
    baseline: Option<String>,
    permissions: Option<u32>,
    personal: Option<String>,
    phase: String,
}
fn command(root: &Path, args: &[&str]) -> Result<Output> {
    let mut cmd = Command::new("git");
    if args.first() != Some(&"check-ignore") {
        cmd.arg("--literal-pathspecs");
    }
    Ok(cmd
        .arg("-C")
        .arg(root)
        .args(args)
        .env_remove("GIT_LITERAL_PATHSPECS")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?)
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = command(root, args)?;
    if !out.status.success() {
        return Err(fail(&format!(
            "Git 操作失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    String::from_utf8(out.stdout).map_err(|_| fail("Git 路径或文件不是 UTF-8 文本"))
}
pub(super) fn repo(root: &Path) -> Option<PathBuf> {
    let out = command(root, &["rev-parse", "--show-toplevel"]).ok()?;
    if !out.status.success() {
        return None;
    }
    PathBuf::from(String::from_utf8(out.stdout).ok()?.trim())
        .canonicalize()
        .ok()
}
fn record_path(data: &Path, path: &Path) -> PathBuf {
    data.join("native-files/local").join(format!(
        "{}.json",
        crate::ids::sha256_hex(path.to_string_lossy().as_bytes())
    ))
}
fn load(data: &Path, path: &Path) -> Result<Option<Record>> {
    let file = record_path(data, path);
    if !file.exists() {
        return Ok(None);
    }
    check_path(&file)?;
    let record: Record = serde_json::from_slice(&std::fs::read(file)?)?;
    if record.path != path
        || record.root.join(&record.relative) != path
        || Path::new(&record.relative)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(fail("本机副本记录与文件不匹配"));
    }
    Ok(Some(record))
}
fn store(data: &Path, record: &Record) -> Result<()> {
    let path = record_path(data, &record.path);
    check_path(&path)?;
    crate::sync_common::atomic_write(&path, &serde_json::to_vec_pretty(record)?)
}
pub(super) fn active(data: &Path, path: &Path) -> Result<bool> {
    Ok(load(data, path)?.is_some_and(|r| r.phase != "inactive"))
}
pub(super) fn pending_paths(data: &Path) -> Result<Vec<PathBuf>> {
    let directory = data.join("native-files/local");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    check_path(&directory)?;
    let mut paths = Vec::new();
    for item in std::fs::read_dir(directory)? {
        let file = item?.path();
        if file.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        check_path(&file)?;
        let record: Record = serde_json::from_slice(&std::fs::read(&file)?)?;
        if record.phase != "inactive" && file == record_path(data, &record.path) {
            paths.push(record.path);
        }
    }
    Ok(paths)
}
pub(super) fn info(data: &Path, root: Option<&Path>, path: &Path) -> Result<Value> {
    let record = load(data, path)?;
    let repository = root.and_then(repo);
    let active = record.as_ref().is_some_and(|r| r.phase != "inactive");
    let tracked = if let Some(root) = &repository {
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .ok_or_else(|| fail("路径不是 UTF-8"))?;
        index(root, relative)?.is_some()
    } else {
        false
    };
    let issue = if active {
        let r = record.as_ref().unwrap();
        verify(r).and_then(|_| {
            let enabled=if r.index.is_some() {git(&r.root,&["ls-files","-v","-z","--",&r.relative])?.starts_with('S')}
            else {command(&r.root,&["check-ignore","--quiet","--",&r.relative])?.status.success()};
            if enabled {Ok(())}else{Err(fail("本地 Git 标记已失效，个人内容可能显示为 Git 改动；请恢复项目版本或重新保存"))}
        }).err().map(|e|e.to_string())
    } else {
        None
    };
    Ok(
        json!({"available":repository.is_some(),"tracked":tracked,"active":active,"issue":issue,
        "phase":record.as_ref().map(|r|r.phase.as_str()),
        "has_personal_copy":record.is_some(),"personal_content":record.and_then(|r|r.personal)}),
    )
}
fn index(root: &Path, relative: &str) -> Result<Option<String>> {
    let text = git(root, &["ls-files", "--stage", "-z", "--", relative])?;
    if text.is_empty() {
        return Ok(None);
    }
    let entries: Vec<_> = text.split('\0').filter(|s| !s.is_empty()).collect();
    if entries.len() != 1 {
        return Err(fail("文件有 Git 合并冲突，请先解决"));
    }
    let metadata = entries[0].split('\t').next().unwrap_or("");
    let fields: Vec<_> = metadata.split_whitespace().collect();
    if fields.len() != 3 || fields[2] != "0" || !matches!(fields[0], "100644" | "100755") {
        return Err(fail("仅支持普通文件，不能替换冲突项、链接或子模块"));
    }
    Ok(Some(metadata.to_owned()))
}
fn verify(record: &Record) -> Result<()> {
    if repo(&record.root).as_ref() != Some(&record.root) {
        return Err(fail("项目 Git 仓库已变化，保留个人副本，停止写入"));
    }
    let git_dir = PathBuf::from(git(&record.root, &["rev-parse", "--absolute-git-dir"])?.trim())
        .canonicalize()?;
    if git_dir != record.git_dir || index(&record.root, &record.relative)? != record.index {
        return Err(fail(
            "Git 中的文件版本已变化，保留个人副本；请先核对项目版本",
        ));
    }
    if record.index.is_some() {
        let staged = command(
            &record.root,
            &["diff", "--cached", "--quiet", "--", &record.relative],
        )?;
        if !staged.status.success() {
            return Err(fail("文件已有暂存改动，请先处理，个人副本仍保留"));
        }
    }
    Ok(())
}
fn ignore_block(record: &Record) -> String {
    let key = crate::ids::sha256_hex(record.path.to_string_lossy().as_bytes());
    let mut pattern = String::from("/");
    for c in record.relative.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']' | ' ' | '#' | '!') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    format!("# AILoom local {key}\n{pattern}\n# /AILoom local {key}\n")
}
fn exclude(record: &Record, enable: bool) -> Result<()> {
    // Git resolves info/exclude through the common directory for linked worktrees.
    let path = PathBuf::from(
        git(
            &record.root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "info/exclude",
            ],
        )?
        .trim(),
    );
    check_path(&path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock_path = path.with_extension("ailoom-lock");
    check_path(&lock_path)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let old = read(&path)?.unwrap_or_default();
    let block = ignore_block(record);
    let next = if enable {
        if old.contains(&block) {
            old.clone()
        } else {
            format!(
                "{old}{}{block}",
                if old.is_empty() || old.ends_with('\n') {
                    ""
                } else {
                    "\n"
                }
            )
        }
    } else {
        old.replace(&block, "")
    };
    if old != next {
        crate::sync_common::atomic_write(&path, next.as_bytes())?;
    }
    Ok(())
}
fn permissions(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).ok().map(|m| m.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}
fn write_content(path: &Path, content: Option<&str>, mode: Option<u32>) -> Result<()> {
    check_path(path)?;
    if let Some(text) = content {
        crate::sync_common::atomic_write(path, text.as_bytes())?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
// Hold Git's index lock while replacing the working file, so checkout/add cannot race the write.
const INDEX_GUARD_MARKER: &[u8] = b"AILoom native-files index guard\n";
struct IndexGuard {
    path: PathBuf,
    _file: std::fs::File,
}
impl Drop for IndexGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
fn lock_index(record: &Record) -> Result<IndexGuard> {
    use std::io::{Read, Write};
    let path = record.git_dir.join("index.lock");
    check_path(&path)?;
    for _ in 0..2 {
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => {
                let mut guard = IndexGuard {
                    path: path.clone(),
                    _file: file,
                };
                fs2::FileExt::lock_exclusive(&guard._file)?;
                guard._file.write_all(INDEX_GUARD_MARKER)?;
                guard._file.sync_all()?;
                return Ok(guard);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let mut file = std::fs::File::open(&path)?;
                let mut marker = Vec::new();
                Read::by_ref(&mut file).take(128).read_to_end(&mut marker)?;
                // Recover only our own abandoned lock, never another Git client's lock.
                if marker != INDEX_GUARD_MARKER || fs2::FileExt::try_lock_exclusive(&file).is_err()
                {
                    break;
                }
                std::fs::remove_file(&path)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(fail("Git 正在操作这个 Worktree，请完成后重试"))
}
fn restore_record(data: &Path, record: &mut Record) -> Result<()> {
    let guard = lock_index(record)?;
    verify(record)?;
    if record.phase == "active"
        && read(&record.path)?.is_some()
        && read(&record.path)? != record.personal
    {
        return Err(fail("文件再次发生变化，请重新打开后恢复"));
    }
    record.phase = "restoring".into();
    store(data, record)?;
    write_content(&record.path, record.baseline.as_deref(), record.permissions)?;
    drop(guard);
    if record.index.is_some() {
        git(
            &record.root,
            &["update-index", "--no-skip-worktree", "--", &record.relative],
        )?;
    } else {
        exclude(record, false)?;
    }
    record.phase = "inactive".into();
    store(data, record)
}
pub(super) fn restore(data: &Path, path: &Path) -> Result<()> {
    let mut record = load(data, path)?.ok_or_else(|| fail("没有本机替换记录"))?;
    if record.phase == "inactive" {
        return Err(fail("已经恢复项目版本"));
    }
    verify(&record)?;
    // An interrupted restore must not replace the archived personal copy with baseline text.
    if record.phase != "restoring" {
        let current = read(path)?;
        if current.is_some() && (record.phase == "active" || current != record.baseline) {
            record.personal = current;
        }
    }
    restore_record(data, &mut record)
}
pub(super) fn save(
    data: &Path,
    root: &Path,
    path: &Path,
    content: &str,
    expected: &str,
) -> Result<()> {
    let current = read(path)?;
    if super::version(&current) != expected {
        return Err(fail("文件已变化，请重新打开"));
    }
    let existing = load(data, path)?;
    let mut record = if let Some(record) = existing.filter(|r| r.phase != "inactive") {
        if record.phase == "restoring" {
            return Err(fail("上次恢复尚未完成，请先恢复项目版本"));
        }
        verify(&record)?;
        record
    } else {
        let root = repo(root).ok_or_else(|| fail("仅本机生效需要 Git 项目"))?;
        let relative = path
            .strip_prefix(&root)?
            .to_str()
            .ok_or_else(|| fail("路径不是 UTF-8"))?
            .to_owned();
        if relative.contains(['\n', '\r']) {
            return Err(fail("文件路径不能含换行"));
        }
        let sparse = command(&root, &["config", "--bool", "core.sparseCheckout"])?;
        if String::from_utf8_lossy(&sparse.stdout).trim() == "true" {
            return Err(fail("稀疏检出项目暂不支持本机替换"));
        }
        let index = index(&root, &relative)?;
        if let Some(metadata) = &index {
            let flags = git(&root, &["ls-files", "-v", "-z", "--", &relative])?;
            if !flags.starts_with('H') {
                return Err(fail(
                    "此文件已有 Git 忽略标记，请先处理，AILoom 不会接管已有标记",
                ));
            }
            let oid = metadata.split_whitespace().nth(1).unwrap();
            let original = git(&root, &["cat-file", "blob", oid])?;
            if current.as_deref() != Some(original.as_str()) {
                return Err(fail("原文件已有本地修改，请先保存或处理后再启用仅本机生效"));
            }
            // Check executable-bit changes too; content equality alone is not a clean file.
            if !command(&root, &["diff", "--quiet", "--", &relative])?
                .status
                .success()
            {
                return Err(fail("原文件已有本地修改，请先处理"));
            }
        }
        let record = Record {
            git_dir: PathBuf::from(git(&root, &["rev-parse", "--absolute-git-dir"])?.trim())
                .canonicalize()?,
            root,
            path: path.to_path_buf(),
            relative,
            index,
            baseline: current.clone(),
            permissions: permissions(path),
            personal: None,
            phase: "preparing".into(),
        };
        verify(&record)?;
        record
    };
    let initial_guard = lock_index(&record)?;
    verify(&record)?;
    record.personal = Some(content.into());
    record.phase = "preparing".into();
    store(data, &record)?; // Recovery journal precedes any changes to Git or the working file.
    drop(initial_guard);
    let result = (|| -> Result<()> {
        if record.index.is_some() {
            git(
                &record.root,
                &["update-index", "--skip-worktree", "--", &record.relative],
            )?;
        } else {
            exclude(&record, true)?;
            if !command(
                &record.root,
                &["check-ignore", "--quiet", "--", &record.relative],
            )?
            .status
            .success()
            {
                return Err(fail("项目的忽略规则覆盖了本地排除，无法启用仅本机生效"));
            }
        }
        let _guard = lock_index(&record)?;
        verify(&record)?;
        if read(path)? != current {
            return Err(fail("文件已被其他程序修改，请重新打开"));
        }
        write_content(path, Some(content), record.permissions)?;
        record.phase = "active".into();
        store(data, &record)
    })();
    if let Err(error) = result {
        // Leave the persisted recovery journal when Git changed or rollback cannot finish.
        if read(path)? != current && read(path)?.as_deref() != Some(content) {
            return Err(fail(&format!(
                "{error}；原文件出现外部修改，未覆盖；个人副本已保留，可核对后恢复项目版本"
            )));
        }
        if let Err(rollback) = restore_record(data, &mut record) {
            return Err(fail(&format!(
                "{error}；恢复未完成：{rollback}。个人副本已保留"
            )));
        }
        return Err(error);
    }
    Ok(())
}
