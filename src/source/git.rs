//! Git 资源源：缓存 clone + 解析 ref + `git archive` 物化不可变快照（AIL-004）。
//! 不执行源仓库代码/hook；fetch 用互斥锁串行化；默认锁定不追新。

use super::{with_file_lock, Snapshot};
use crate::error::{code, Error, Result};
use crate::gitx;
use crate::ids::now_iso;
use std::path::{Path, PathBuf};

/// 快照标记文件：记录 commit 与摘要，用于完整性校验。
pub const SNAPSHOT_MARKER: &str = ".ailoom-snapshot.json";

#[derive(Debug, Clone)]
pub struct GitSource {
    /// 规范化身份（不含凭据）
    pub identity: String,
    /// 原始 URL（不含凭据）
    pub url: String,
    pub ref_: Option<String>,
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
        })
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
            if let Some(s) = self.verify_snapshot(&snap, &commit)? {
                return Ok(s);
            }
            // 快照缺失：fetch 后重新物化（不改锁定的 commit）
            self.fetch(cache_root).map_err(|e| {
                let code = e.code.clone();
                e.fix(format!(
                    "无法离线恢复已锁快照（{code}），恢复网络后执行 ailoom init --refresh"
                ))
            })?;
            let (root, digest) = self.materialize(cache_root, &commit)?;
            return Ok(Snapshot {
                identity: self.identity.clone(),
                resolved_commit: Some(commit),
                content_digest: digest,
                root,
                ref_: self.ref_.clone(),
                locked_at: l.locked_at.clone(),
                mutable: false,
            });
        }
        // 未锁定：首次必须联网
        self.fetch(cache_root)?;
        self.lock_locked_snapshot(cache_root)
    }

    /// 显式更新入口：fetch 并解析当前 ref；失败返回 Err（调用方保留旧锁）。
    pub fn refresh(&self, cache_root: &Path) -> Result<Snapshot> {
        self.fetch(cache_root)?;
        self.lock_locked_snapshot(cache_root)
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
            let repo = self.cache_repo(cache_root);
            if repo.exists() {
                gitx::git(&repo, &["fetch", "-q", "--prune", "--tags", "origin"]).map_err(|e| {
                    Error::new(code::SOURCE_FETCH_FAILED, "fetch 失败：已锁定快照不受影响")
                        .context(e.to_json())
                        .fix("检查网络与远端权限；恢复后执行 ailoom init --refresh")
                })?;
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
                .map_err(|e| {
                    Error::new(code::SOURCE_FETCH_FAILED, "首次获取源失败（clone）")
                        .context(e.to_json())
                        .fix("首次使用该源必须能访问远端")
                })?;
                std::fs::rename(&tmp, &repo).map_err(|e| {
                    let _ = std::fs::remove_dir_all(&tmp);
                    Error::new(code::SOURCE_FETCH_FAILED, format!("缓存目录就位失败: {e}"))
                })?;
            }
            Ok(())
        })
    }

    fn resolve_commit(&self, repo: &Path) -> Result<String> {
        let commit = match &self.ref_ {
            Some(r) => {
                let candidate = format!("refs/remotes/origin/{r}");
                gitx::git_optional(repo, &["rev-parse", "--verify", &candidate])
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        gitx::git_optional(
                            repo,
                            &["rev-parse", "--verify", &format!("{r}^{{commit}}")],
                        )
                        .filter(|s| !s.is_empty())
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
        if self.verify_snapshot(&final_dir, commit)?.is_some() {
            let digest = super::tree_digest(&final_dir)?;
            return Ok((final_dir, digest));
        }
        let staging = cache_root.join(format!(".snap-tmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let repo = self.cache_repo(cache_root);
        let tar_path = cache_root.join(format!(".snap-tar-{}", std::process::id()));
        gitx::git(
            &repo,
            &[
                "archive",
                "--format=tar",
                "-o",
                tar_path.to_str().unwrap_or_default(),
                commit,
            ],
        )?;
        let extract = std::process::Command::new("tar")
            .arg("-xf")
            .arg(&tar_path)
            .arg("-C")
            .arg(&staging)
            .output()
            .map_err(|e| Error::new(code::SOURCE_FETCH_FAILED, format!("无法启动 tar: {e}")))?;
        let _ = std::fs::remove_file(&tar_path);
        if !extract.status.success() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(
                Error::new(code::SOURCE_FETCH_FAILED, "快照解包失败").context(serde_json::json!({
                    "stderr": String::from_utf8_lossy(&extract.stderr).trim()
                })),
            );
        }
        let digest = super::tree_digest(&staging)?;
        let marker = serde_json::json!({
            "identity": self.identity,
            "commit": commit,
            "content_digest": digest,
            "created_at": now_iso(),
        });
        crate::sync_common::atomic_write(
            &staging.join(SNAPSHOT_MARKER),
            serde_json::to_vec_pretty(&marker)?.as_slice(),
        )?;
        if let Some(parent) = final_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::rename(&staging, &final_dir).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            Error::new(code::SOURCE_FETCH_FAILED, format!("快照就位失败: {e}"))
        })?;
        Ok((final_dir, digest))
    }

    /// 校验快照存在且标记完整（含摘要一致性）。
    fn verify_snapshot(&self, dir: &Path, commit: &str) -> Result<Option<Snapshot>> {
        if !dir.is_dir() {
            return Ok(None);
        }
        let marker_path = dir.join(SNAPSHOT_MARKER);
        let marker: serde_json::Value = match std::fs::read_to_string(&marker_path)
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
        let digest = super::tree_digest(dir)?;
        if marker["content_digest"] != *digest {
            return Err(Error::new(
                code::SOURCE_CACHE_CORRUPT,
                "快照内容摘要与标记不符（缓存被改动）",
            )
            .context(serde_json::json!({ "marker": marker["content_digest"], "actual": digest })));
        }
        Ok(Some(Snapshot {
            identity: self.identity.clone(),
            resolved_commit: Some(commit.to_string()),
            content_digest: digest,
            root: dir.to_path_buf(),
            ref_: self.ref_.clone(),
            locked_at: marker["created_at"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            mutable: false,
        }))
    }
}
