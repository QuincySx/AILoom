//! Git 资源源：缓存 clone + 解析 ref + `git archive` 物化不可变快照（AIL-004）。
//! 不执行源仓库代码/hook；fetch 用互斥锁串行化；默认锁定不追新。
//! 只服务 Skill 的缓存按 [`Scope`] 只物化 Skill 目录：大仓库的无关文件既不解包也不参与摘要。

use super::{with_file_lock, Snapshot};
use crate::error::{code, Error, Result};
use crate::gitx;
use crate::ids::now_iso;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 快照标记文件：记录 commit 与摘要，用于完整性校验。
pub const SNAPSHOT_MARKER: &str = ".ailoom-snapshot.json";

/// 单次 `git archive` 携带的路径参数上限（字节），低于各平台命令行长度限制（Windows 为 32K）。
const ARCHIVE_PATHS_BUDGET: usize = if cfg!(windows) { 16 * 1024 } else { 128 * 1024 };

/// 快照物化范围。摘要只覆盖已物化的文件；范围写入快照标记，范围不同的快照不会混用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// 整棵树（团队源、包等需要完整资源目录的场景；旧版快照均为此范围）。
    #[default]
    Full,
    /// 只物化含 `SKILL.md` 的目录及仓内符号链接（个人资源库从仓库安装 Skill；
    /// 链接保留是为了让 `--path` 经链接逃逸时仍按越界拒绝）。
    Skills,
    /// 根目录有 `ailoom.toml` 时物化整树（混合资源合集），否则同 `Skills`。
    Catalog,
}

impl Scope {
    fn marker(self) -> Option<&'static str> {
        match self {
            Scope::Full => None,
            Scope::Skills => Some("skills"),
            Scope::Catalog => Some("catalog"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitSource {
    /// 规范化身份（不含凭据）
    pub identity: String,
    /// 原始 URL（不含凭据）
    pub url: String,
    pub ref_: Option<String>,
    /// 快照物化范围（默认整树）
    pub scope: Scope,
}

impl GitSource {
    /// URL 内嵌 `user:pass` 形式凭据直接拒绝（契约 E2004）。
    pub fn new(url: &str, ref_: Option<&str>) -> Result<Self> {
        let url = url.trim();
        if let Some(idx) = url.find("://") {
            let after = &url[idx + 3..];
            let authority_end = after.find('/').unwrap_or(after.len());
            let authority = &after[..authority_end];
            if let Some((userinfo, _)) = authority.rsplit_once('@') {
                if userinfo.contains(':') {
                    return Err(Error::new(
                        code::SOURCE_URL_CREDENTIAL,
                        "Git URL 不允许内嵌用户名密码，请使用 ssh 或凭据助手",
                    ));
                }
            }
        }
        if url.is_empty() {
            return Err(Error::new(code::SOURCE_INVALID_REF, "Git URL 为空"));
        }
        let identity = gitx::normalize_remote_url(url);
        if identity.contains("hunter2") || identity != gitx::redact_credentials(&identity) {
            return Err(Error::new(
                code::SOURCE_URL_CREDENTIAL,
                "Git URL 含敏感信息，已拒绝",
            ));
        }
        Ok(GitSource {
            identity,
            url: url.to_string(),
            ref_: ref_.map(str::to_string),
            scope: Scope::Full,
        })
    }

    /// 指定快照物化范围；同一缓存目录必须始终使用同一范围。
    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    fn cache_repo(&self, cache_root: &Path) -> PathBuf {
        cache_root.join("repo")
    }

    fn snapshot_dir(&self, cache_root: &Path, commit: &str) -> PathBuf {
        cache_root.join("snapshots").join(commit)
    }

    /// 按锁定状态解析快照（默认入口）：
    /// - 已锁且快照存在且校验通过 → 直接返回（离线可用）；
    /// - 已锁但快照缺失 → 尝试 fetch 后重取；仍失败 → E2005/E2001；
    /// - 未锁 → 必须成功 fetch；离线无缓存 → E2001。
    ///
    /// 永不前移锁定 commit。
    pub fn resolve(&self, cache_root: &Path, lock: Option<&super::SourceLock>) -> Result<Snapshot> {
        if let Some(l) = lock {
            if l.identity != self.identity {
                return Err(
                    Error::new(code::SOURCE_CONFLICT, "锁文件中的源身份与声明不一致")
                        .context(serde_json::json!({
                            "locked": l.identity, "declared": self.identity
                        }))
                        .fix("重新执行 ailoom init 更新绑定"),
                );
            }
            let commit = l.resolved_commit.clone().ok_or_else(|| {
                Error::new(code::SOURCE_CACHE_CORRUPT, "Git 源锁缺少 resolved_commit")
            })?;
            let snap = self.snapshot_dir(cache_root, &commit);
            match self.read_marker(&snap, &commit)? {
                Some(marker) if !self.needs_rematerialize(&marker) => {
                    let digest = verify_digest(&snap, &marker)?;
                    return Ok(Snapshot {
                        identity: self.identity.clone(),
                        resolved_commit: Some(commit),
                        content_digest: locked_digest(&marker, digest, l),
                        root: snap,
                        ref_: self.ref_.clone(),
                        locked_at: marker["created_at"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        mutable: false,
                    });
                }
                // 范围不同（多为旧版整树快照）：commit 已在缓存仓库时离线按范围重新物化。
                Some(marker) if !self.has_commit(cache_root, &commit) => {
                    // 缓存仓库不可用：整树快照仍是合法超集，继续使用。
                    if marker["scope"].is_null() {
                        let digest = verify_digest(&snap, &marker)?;
                        return Ok(Snapshot {
                            identity: self.identity.clone(),
                            resolved_commit: Some(commit),
                            content_digest: digest,
                            root: snap,
                            ref_: self.ref_.clone(),
                            locked_at: l.locked_at.clone(),
                            mutable: false,
                        });
                    }
                    self.fetch(cache_root)?;
                }
                Some(_) => {}
                // 快照缺失：fetch 后重新物化（不改锁定的 commit）
                None => self.fetch(cache_root)?,
            }
            let (root, digest) = self.materialize(cache_root, &commit)?;
            let marker = self.read_marker(&root, &commit)?.unwrap_or_default();
            return Ok(Snapshot {
                identity: self.identity.clone(),
                resolved_commit: Some(commit),
                content_digest: locked_digest(&marker, digest, l),
                root,
                ref_: self.ref_.clone(),
                locked_at: l.locked_at.clone(),
                mutable: false,
            });
        }
        // 未锁定：首次必须联网
        self.refresh(cache_root)
    }

    /// 显式更新入口：fetch 并解析当前 ref；失败返回 Err（调用方保留旧锁）。
    pub fn refresh(&self, cache_root: &Path) -> Result<Snapshot> {
        with_file_lock(&cache_root.join("fetch.lock"), || {
            self.fetch_locked(cache_root)?;
            self.lock_locked_snapshot(cache_root)
        })
    }

    /// 基于缓存 repo 当前解析 ref，物化并校验快照（调用前需已 fetch/clone）。
    fn lock_locked_snapshot(&self, cache_root: &Path) -> Result<Snapshot> {
        let repo = self.cache_repo(cache_root);
        if !repo.exists() {
            return Err(
                Error::new(code::SOURCE_NOT_CACHED, "无缓存且离线，无法解析源")
                    .context(serde_json::json!({ "identity": self.identity })),
            );
        }
        let commit = self.resolve_commit(&repo)?;
        let snapshot = self.materialize(cache_root, &commit)?;
        Ok(Snapshot {
            identity: self.identity.clone(),
            resolved_commit: Some(commit),
            content_digest: snapshot.1,
            root: snapshot.0,
            ref_: self.ref_.clone(),
            locked_at: now_iso(),
            mutable: false,
        })
    }

    /// clone（若无）/fetch（若已有）。全程互斥锁串行化。
    fn fetch(&self, cache_root: &Path) -> Result<()> {
        with_file_lock(&cache_root.join("fetch.lock"), || {
            self.fetch_locked(cache_root)
        })
    }

    fn fetch_locked(&self, cache_root: &Path) -> Result<()> {
        let repo = self.cache_repo(cache_root);
        if repo.exists() {
            // Equivalent cache identities can use different SSH users/URL forms.
            // Always fetch with the caller's chosen machine connection.
            gitx::git(&repo, &["remote", "set-url", "origin", &self.url])?;
            gitx::git(
                &repo,
                &[
                    "fetch",
                    "-q",
                    "--prune",
                    "--prune-tags",
                    "--force",
                    "--tags",
                    "origin",
                ],
            )
            .map_err(|e| self.fetch_error(e, "获取资源源失败；已有锁定快照仍可使用"))?;
            if self.ref_.is_none() {
                gitx::git(&repo, &["remote", "set-head", "origin", "--auto"]).map_err(|e| {
                    self.fetch_error(e, "无法确认远端默认分支；已有锁定快照仍可使用")
                })?;
            }
        } else {
            if let Some(parent) = repo.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = cache_root.join(format!(".clone-tmp-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            gitx::git(
                cache_root,
                &[
                    "clone",
                    "-q",
                    "--no-checkout",
                    &self.url,
                    tmp.to_str().unwrap_or_default(),
                ],
            )
            .map_err(|e| self.fetch_error(e, "首次获取资源源失败（clone）"))?;
            std::fs::rename(&tmp, &repo).map_err(|e| {
                let _ = std::fs::remove_dir_all(&tmp);
                Error::new(code::SOURCE_FETCH_FAILED, format!("缓存目录就位失败: {e}"))
            })?;
        }
        Ok(())
    }

    fn fetch_error(&self, error: Error, message: &str) -> Error {
        let path = Path::new(&self.url);
        let missing_local = path.is_absolute()
            && std::fs::metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
        let message = if missing_local {
            format!("资源源目录不存在：{}", path.display())
        } else {
            message.to_string()
        };
        Error::new(code::SOURCE_FETCH_FAILED, message)
            .context(serde_json::json!({"source": self.identity, "cause": error.to_json()}))
            .fix(if missing_local {
                "恢复原目录；若需更换来源，修改 .ailoom/project.toml 后运行 ailoom init 完成绑定与锁定。失败不会删除已部署文件"
            } else {
                "核对资源源地址、访问权限与网络连接；已有锁定版本与已部署文件保持不变"
            })
    }

    fn resolve_commit(&self, repo: &Path) -> Result<String> {
        let commit = match &self.ref_ {
            Some(r) => {
                let branch = r
                    .strip_prefix("refs/heads/")
                    .or_else(|| r.strip_prefix("refs/remotes/origin/"))
                    .unwrap_or(r);
                let candidate = format!("refs/remotes/origin/{branch}^{{commit}}");
                gitx::git_optional(repo, &["rev-parse", "--verify", &candidate])
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        let tag = r.strip_prefix("refs/tags/").unwrap_or(r);
                        gitx::git_optional(
                            repo,
                            &[
                                "rev-parse",
                                "--verify",
                                &format!("refs/tags/{tag}^{{commit}}"),
                            ],
                        )
                        .filter(|s| !s.is_empty())
                    })
                    .or_else(|| {
                        // A commit must be reachable from currently fetched refs.
                        // Never resolve stale clone-local branches or prior upstream objects.
                        if !(4..=64).contains(&r.len()) || !r.bytes().all(|b| b.is_ascii_hexdigit())
                        {
                            return None;
                        }
                        let commit = gitx::git_optional(
                            repo,
                            &["rev-parse", "--verify", &format!("{r}^{{commit}}")],
                        )?;
                        let refs = gitx::git_optional(
                            repo,
                            &[
                                "for-each-ref",
                                "--format=%(refname)",
                                "--contains",
                                &commit,
                                "refs/remotes/origin",
                                "refs/tags",
                            ],
                        )?;
                        (!refs.is_empty()).then_some(commit)
                    })
            }
            None => gitx::git_optional(
                repo,
                &["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"],
            )
            .and_then(|head| {
                gitx::git_optional(
                    repo,
                    &["rev-parse", "--verify", &format!("refs/remotes/{head}")],
                )
            })
            .filter(|s| !s.is_empty()),
        };
        commit.ok_or_else(|| {
            Error::new(
                code::SOURCE_INVALID_REF,
                format!(
                    "无法解析 ref: {}",
                    self.ref_.as_deref().unwrap_or("<默认分支>")
                ),
            )
            .context(serde_json::json!({ "ref": self.ref_, "identity": self.identity }))
            .fix("核对分支/标签名；无效 ref 不会影响已有锁定版本")
        })
    }

    /// `git archive <commit>` 解包到临时目录 → 校验摘要 → 原子改名到 snapshots/<commit>。
    fn materialize(&self, cache_root: &Path, commit: &str) -> Result<(PathBuf, String)> {
        with_file_lock(&cache_root.join("materialize.lock"), || {
            self.materialize_locked(cache_root, commit)
        })
    }

    fn materialize_locked(&self, cache_root: &Path, commit: &str) -> Result<(PathBuf, String)> {
        let final_dir = self.snapshot_dir(cache_root, commit);
        // 旧版整树快照按范围重建时记录原摘要：锁住原摘要的调用方看到的内容不变。
        let mut replaces = None;
        if let Some(marker) = self.read_marker(&final_dir, commit)? {
            if !self.needs_rematerialize(&marker) {
                let digest = verify_digest(&final_dir, &marker)?;
                return Ok((final_dir, digest));
            }
            if marker["scope"].is_null() {
                replaces = marker["content_digest"].as_str().map(str::to_string);
            }
        }
        let staging = cache_root.join(format!(".snap-tmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let extracted = match self.scoped_paths(cache_root, commit) {
            Ok(None) => self.extract(cache_root, commit, &[], &staging),
            Ok(Some(paths)) => archive_chunks(&paths)
                .iter()
                .try_for_each(|chunk| self.extract(cache_root, commit, chunk, &staging)),
            Err(e) => Err(e),
        };
        if let Err(e) = extracted {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        let digest = super::tree_digest(&staging)?;
        let mut marker = serde_json::json!({
            "identity": self.identity,
            "commit": commit,
            "content_digest": digest,
            "created_at": now_iso(),
        });
        if let Some(scope) = self.scope.marker() {
            marker["scope"] = scope.into();
        }
        if let Some(previous) = replaces.filter(|d| *d != digest) {
            marker["replaces_digest"] = previous.into();
        }
        crate::sync_common::atomic_write(
            &staging.join(SNAPSHOT_MARKER),
            serde_json::to_vec_pretty(&marker)?.as_slice(),
        )?;
        if let Some(parent) = final_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 先把旧快照移开再就位，缩短读者看到目录缺失的窗口。
        let retired = cache_root.join(format!(".snap-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&retired);
        let _ = std::fs::rename(&final_dir, &retired);
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::rename(&staging, &final_dir).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            Error::new(code::SOURCE_FETCH_FAILED, format!("快照就位失败: {e}"))
        })?;
        let _ = std::fs::remove_dir_all(&retired);
        Ok((final_dir, digest))
    }

    /// 需要物化的仓内目录；`None` 表示整树。只读 Git 树对象，不读取文件内容。
    fn scoped_paths(&self, cache_root: &Path, commit: &str) -> Result<Option<Vec<String>>> {
        if self.scope == Scope::Full {
            return Ok(None);
        }
        let listing = gitx::git_bytes(
            &self.cache_repo(cache_root),
            &["ls-tree", "-r", "-z", "--full-tree", commit],
        )?;
        let mut dirs = BTreeSet::new();
        let mut links = Vec::new();
        for entry in listing.split(|b| *b == 0) {
            let Some(tab) = entry.iter().position(|b| *b == b'\t') else {
                continue;
            };
            let (meta, path) = (&entry[..tab], &entry[tab + 1..]);
            // 非 UTF-8 路径无法作为参数传给 git，也无法登记为 Skill 路径。
            let Ok(path) = std::str::from_utf8(path) else {
                continue;
            };
            if self.scope == Scope::Catalog && path == "ailoom.toml" {
                return Ok(None);
            }
            if self.scope == Scope::Skills && meta.starts_with(b"120000 ") {
                links.push(path.to_string());
                continue;
            }
            // 与目录遍历一致：只认普通文件形式的 SKILL.md。
            if !(meta.starts_with(b"100644 blob ") || meta.starts_with(b"100755 blob ")) {
                continue;
            }
            if path == "SKILL.md" {
                return Ok(None);
            }
            if let Some(dir) = path.strip_suffix("/SKILL.md") {
                dirs.insert(dir.to_string());
            }
        }
        // 嵌套 Skill 与 Skill 内的链接已由外层目录覆盖。
        let covered = |p: &str| p.match_indices('/').any(|(i, _)| dirs.contains(&p[..i]));
        Ok(Some(
            dirs.iter()
                .chain(&links)
                .filter(|p| !covered(p))
                .cloned()
                .collect(),
        ))
    }

    /// `git archive` 指定路径（空 = 整树）并解包到 staging。
    fn extract(
        &self,
        cache_root: &Path,
        commit: &str,
        paths: &[String],
        staging: &Path,
    ) -> Result<()> {
        let tar_path = cache_root.join(format!(".snap-tar-{}", std::process::id()));
        let tar = tar_path.to_str().unwrap_or_default();
        let mut args = vec![
            "--literal-pathspecs",
            "archive",
            "--format=tar",
            "-o",
            tar,
            commit,
        ];
        if !paths.is_empty() {
            args.push("--");
            args.extend(paths.iter().map(String::as_str));
        }
        if let Err(e) = gitx::git(&self.cache_repo(cache_root), &args) {
            let _ = std::fs::remove_file(&tar_path);
            return Err(e);
        }
        let extract = std::process::Command::new("tar")
            .arg("-xf")
            .arg(&tar_path)
            .arg("-C")
            .arg(staging)
            .output()
            .map_err(|e| Error::new(code::SOURCE_FETCH_FAILED, format!("无法启动 tar: {e}")));
        let _ = std::fs::remove_file(&tar_path);
        let extract = extract?;
        if !extract.status.success() {
            return Err(
                Error::new(code::SOURCE_FETCH_FAILED, "快照解包失败").context(serde_json::json!({
                    "stderr": String::from_utf8_lossy(&extract.stderr).trim()
                })),
            );
        }
        Ok(())
    }

    fn has_commit(&self, cache_root: &Path, commit: &str) -> bool {
        let repo = self.cache_repo(cache_root);
        repo.is_dir()
            && gitx::git_optional(&repo, &["cat-file", "-e", &format!("{commit}^{{commit}}")])
                .is_some()
    }

    /// 已有快照的范围与请求不同（旧版整树快照或其他范围）时需要重新物化。
    fn needs_rematerialize(&self, marker: &Value) -> bool {
        marker["scope"].as_str() != self.scope.marker()
    }

    /// 读取快照标记并核对 commit；快照不存在返回 None，标记损坏或不符报错。
    fn read_marker(&self, dir: &Path, commit: &str) -> Result<Option<Value>> {
        if !dir.is_dir() {
            return Ok(None);
        }
        let marker: Value = match std::fs::read_to_string(dir.join(SNAPSHOT_MARKER))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
        {
            Some(m) => m,
            None => {
                return Err(Error::new(
                    code::SOURCE_CACHE_CORRUPT,
                    "快照标记损坏，删除缓存目录后重试",
                )
                .context(serde_json::json!({ "snapshot": dir.display().to_string() })));
            }
        };
        if marker["commit"] != *commit {
            return Err(
                Error::new(code::SOURCE_CACHE_CORRUPT, "快照标记与请求 commit 不符")
                    .context(serde_json::json!({ "marker": marker["commit"], "want": commit })),
            );
        }
        Ok(Some(marker))
    }
}

/// 复核已物化内容与标记摘要一致。范围快照只含 Skill 目录，代价随 Skill 大小而非仓库大小增长。
fn verify_digest(dir: &Path, marker: &Value) -> Result<String> {
    let digest = super::tree_digest(dir)?;
    if marker["content_digest"] != *digest {
        return Err(Error::new(
            code::SOURCE_CACHE_CORRUPT,
            "快照内容摘要与标记不符（缓存被改动）",
        )
        .context(serde_json::json!({ "marker": marker["content_digest"], "actual": digest })));
    }
    Ok(digest)
}

/// 旧版整树快照按范围重建后，锁定原摘要的调用方仍得到原摘要（同一 commit，所需内容未变）。
fn locked_digest(marker: &Value, digest: String, lock: &super::SourceLock) -> String {
    if marker["replaces_digest"].as_str() == Some(lock.content_digest.as_str()) {
        lock.content_digest.clone()
    } else {
        digest
    }
}

/// 按参数长度预算分批，避免超长命令行。
fn archive_chunks(paths: &[String]) -> Vec<Vec<String>> {
    let mut chunks: Vec<Vec<String>> = Vec::new();
    let mut size = 0;
    for path in paths {
        if chunks.is_empty() || size + path.len() + 1 > ARCHIVE_PATHS_BUDGET {
            chunks.push(Vec::new());
            size = 0;
        }
        size += path.len() + 1;
        chunks.last_mut().unwrap().push(path.clone());
    }
    chunks
}
