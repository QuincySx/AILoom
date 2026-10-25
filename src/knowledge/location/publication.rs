//! Publish editable pages together; an interrupted write is rolled back on the next read.
use super::*;
const JOURNAL: &str = ".ailoom/wiki-transaction.json";
type Before = BTreeMap<PathBuf, Option<Vec<u8>>>;

#[derive(Serialize, Deserialize)]
struct Journal {
    before: Before,
    after: BTreeMap<PathBuf, Option<String>>,
}
fn restore(path: &Path, journal: &Journal) -> Result<()> {
    let tree = scan(path)?;
    for (p, old) in &journal.before {
        if p.components().any(|c| !matches!(c, Component::Normal(_)))
            || !(p == Path::new("README.md")
                || p == Path::new(".ailoom/wiki.json")
                || (p.starts_with("wiki") && p.extension().is_some_and(|e| e == "md")))
        {
            return Err(fail("Wiki 恢复记录包含无效路径"));
        }
        let current = tree.get(p).map(|b| crate::ids::sha256_hex(b));
        let before = old.as_ref().map(|b| crate::ids::sha256_hex(b));
        let after = journal
            .after
            .get(p)
            .ok_or_else(|| fail("Wiki 恢复记录不完整"))?;
        if current != before && &current != after {
            return Err(fail(
                "发现中断后的人工作修改，已停止自动恢复；当前文件及恢复记录均保留，请先核对",
            ));
        }
        if path.join(p).is_dir() {
            return Err(fail("Wiki 文件位置被目录占用，已停止恢复"));
        }
    }
    for (p, old) in &journal.before {
        match old {
            Some(bytes) => crate::sync_common::atomic_write(&path.join(p), bytes)?,
            None => match std::fs::remove_file(path.join(p)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            },
        }
    }
    std::fs::remove_file(path.join(JOURNAL))?;
    Ok(())
}
pub(super) fn recover(path: &Path) -> Result<bool> {
    let p = path.join(JOURNAL);
    if !p.exists() {
        return Ok(false);
    }
    if p.metadata()?.len() > 32 * 1024 * 1024 {
        return Err(fail("Wiki 恢复记录过大"));
    }
    let journal: Journal = serde_json::from_slice(&std::fs::read(p)?)?;
    restore(path, &journal)?;
    Ok(true)
}
pub(super) fn commit(
    path: &Path,
    tree: &Tree,
    mut files: Tree,
    previous: &[wiki::Page],
) -> Result<()> {
    let managed: BTreeMap<PathBuf, String> = tree
        .get(Path::new(".ailoom/wiki.json"))
        .map(|b| serde_json::from_slice(b))
        .transpose()?
        .unwrap_or_default();
    let mut remove = Vec::new();
    let mut manual = Vec::new();
    for page in previous.iter().filter(|p| !files.contains_key(&p.path)) {
        if page.path.starts_with("wiki")
            && page
                .path
                .components()
                .nth(1)
                .and_then(|c| c.as_os_str().to_str())
                .is_none_or(|c| uuid::Uuid::parse_str(c).is_err())
        {
            if managed.get(&page.path).is_some_and(|digest| {
                tree.get(&page.path)
                    .is_some_and(|b| crate::ids::sha256_hex(b) == *digest)
            }) {
                remove.push(page.path.clone());
            } else {
                manual.push(page);
            }
        }
    }
    if !manual.is_empty() {
        let home = files.get_mut(Path::new("README.md")).unwrap();
        home.extend_from_slice(b"\n\n## ");
        home.extend_from_slice("人工补充文档\n\n".as_bytes());
        for page in manual {
            home.extend_from_slice(
                format!(
                    "- [{}]({})\n",
                    page.title.replace(['[', ']', '\n'], " "),
                    wiki::markdown_path(&page.path)
                )
                .as_bytes(),
            );
        }
    }
    let hashes: BTreeMap<_, _> = files
        .iter()
        .filter(|(p, _)| **p != Path::new(".ailoom/wiki.json"))
        .map(|(p, b)| (p.clone(), crate::ids::sha256_hex(b)))
        .collect();
    files.insert(
        ".ailoom/wiki.json".into(),
        serde_json::to_vec_pretty(&hashes)?,
    );
    let before: Before = files
        .keys()
        .chain(&remove)
        .map(|p| (p.clone(), tree.get(p).cloned()))
        .collect();
    let after = before
        .keys()
        .map(|p| (p.clone(), files.get(p).map(|b| crate::ids::sha256_hex(b))))
        .collect();
    let journal = Journal { before, after };
    let encoded = serde_json::to_vec(&journal)?;
    if encoded.len() > 32 * 1024 * 1024 {
        return Err(fail(
            "正文变更的恢复副本超过 32 MB，原文未覆盖；请减小单次整理范围",
        ));
    }
    let observed = scan(path)?;
    if journal
        .before
        .iter()
        .any(|(p, b)| observed.get(p) != b.as_ref() || path.join(p).is_dir())
    {
        return Err(fail("写入前发现文档或路径已变化，原有内容保留"));
    }
    crate::sync_common::atomic_write(&path.join(JOURNAL), &encoded)?;
    let result = (|| {
        for (p, b) in files.iter().filter(|(p, _)| **p != Path::new("README.md")) {
            crate::sync_common::atomic_write(&path.join(p), b)?;
        }
        for p in &remove {
            std::fs::remove_file(path.join(p))?;
        }
        crate::sync_common::atomic_write(&path.join("README.md"), &files[Path::new("README.md")])?;
        std::fs::remove_file(path.join(JOURNAL))?;
        Ok(())
    })();
    if result.is_err() {
        restore(path, &journal)?;
    }
    result
}
