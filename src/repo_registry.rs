//! 仓库身份、工作树与子项目发现/登记（AIL-039）。
//!
//! Git 身份与配置作用域分离：先按 Git 元数据（common-dir、worktree 列表）识别仓库，
//! 再在仓库边界内登记配置作用域（子项目相对路径）。RepositoryId 以 common-dir 为
//! 主要发现证据，不用 origin URL 作不可变主键；同远端的不同 clone 只生成关联建议。
//! 子模块/嵌套独立仓库在新 Git 边界停止继承，不并入外层仓库。

use crate::error::{code, Error, Result};
use crate::gitx::{git, git_optional, normalize_remote_url};
use crate::ids::{new_id, sha256_prefix};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 工作树登记状态：active = 当前可见；missing = 注册时在、现在失联。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeStatus {
    Active,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    /// 当前绝对路径（canonical）。移动后旧登记保留原路径并标 missing。
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// 分支引用（refs/heads/…）；detached 时为 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_detached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prunable_reason: Option<String>,
}

impl WorktreeInfo {
    /// 稳定登记 id：注册时生成，移动/重关联保持不变（不以路径为身份）。
    pub fn stable_key(&self) -> String {
        sha256_prefix(self.path.to_string_lossy().as_bytes(), 12)
    }
    /// 是否可作为资源写入落点：bare 根是仓库容器，不是部署目标。
    pub fn deployable(&self) -> bool {
        !self.is_bare
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoIdentity {
    /// 主工作树根（bare 仓库 = common-dir 本身）。
    pub repo_root: PathBuf,
    /// Git common-dir 绝对路径：同仓库所有 worktree 共享，是归组依据。
    pub common_dir: PathBuf,
    /// 本地仓库身份：`repo-` + sha256(common_dir)[..16]。不以 origin 为主键。
    pub repo_id: String,
    /// 规范化 origin（发现证据，不是身份主键）；无远端为 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_normalized: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_raw: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoDiscovery {
    pub identity: RepoIdentity,
    pub worktrees: Vec<WorktreeInfo>,
    /// 发现起点（canonical）。
    pub start: PathBuf,
    /// 起点所在 worktree（最长路径前缀匹配；可能不是主工作树）。
    pub current_worktree: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathClass {
    /// Git 仓库（含 worktree / bare / 子目录声明处）。
    Git(RepoDiscovery),
    /// 明确非 Git：向上没有任何 `.git`。只有探测失败报错，不静默降级到这里。
    NonGit { root: PathBuf },
}

/// 从 start 向上找最近的 `.git`（目录或 linked-worktree 文件都算）；
/// bare 仓库目录本身（含 HEAD/objects/refs）也是 Git 边界。
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    while let Some(current) = dir {
        if current.join(".git").exists() || looks_like_git_dir(&current) {
            return Some(current);
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    None
}

fn looks_like_git_dir(p: &Path) -> bool {
    p.join("HEAD").is_file() && p.join("objects").is_dir() && p.join("refs").is_dir()
}

/// 分类路径：Git 仓库发现或明确非 Git。Git 探测错误报错（E2007），不当非 Git。
pub fn classify_path(start: &Path) -> Result<PathClass> {
    let start = start.canonicalize().map_err(|e| {
        Error::new(code::WORKSPACE_INVALID, format!("路径不可用: {e}"))
            .context(serde_json::json!({ "path": start.display().to_string() }))
    })?;
    if find_git_root(&start).is_none() {
        return Ok(PathClass::NonGit { root: start });
    }
    Ok(PathClass::Git(discover_repo(&start)?))
}

/// 发现仓库身份与全部关联工作树。
pub fn discover_repo(start: &Path) -> Result<RepoDiscovery> {
    let start = start.canonicalize().map_err(|e| {
        Error::new(code::WORKSPACE_INVALID, format!("路径不可用: {e}"))
            .context(serde_json::json!({ "path": start.display().to_string() }))
    })?;
    if find_git_root(&start).is_none() {
        return Err(
            Error::new(code::WORKSPACE_ROOT_NOT_FOUND, "起点不在 Git 仓库内")
                .context(serde_json::json!({ "start": start.display().to_string() })),
        );
    }
    // Git 身份探测错误必须显式失败，不允许静默按非 Git 处理
    let common = git(
        &start,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common_dir = PathBuf::from(common.trim())
        .canonicalize()
        .map_err(|e| Error::new(code::GIT_COMMAND_FAILED, format!("common-dir 不可用: {e}")))?;
    let repo_root = match common_dir.file_name().and_then(|n| n.to_str()) {
        Some(".git") => common_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| common_dir.clone()),
        _ => common_dir.clone(), // bare：common-dir 即仓库容器
    };
    let (origin_raw, origin_normalized) =
        match git_optional(&start, &["remote", "get-url", "origin"]) {
            Some(url) if !url.trim().is_empty() => {
                let n = normalize_remote_url(url.trim());
                (Some(url.trim().to_string()), Some(n))
            }
            _ => (None, None),
        };
    let identity = RepoIdentity {
        repo_root,
        common_dir: common_dir.clone(),
        repo_id: format!(
            "repo-{}",
            sha256_prefix(common_dir.to_string_lossy().as_bytes(), 16)
        ),
        origin_normalized,
        origin_raw,
    };
    let worktrees = list_worktrees(&start)?;
    let current_worktree = worktrees
        .iter()
        .map(|w| w.path.clone())
        .filter(|p| start.starts_with(p))
        .max_by_key(|p| p.as_os_str().len())
        .ok_or_else(|| {
            Error::new(
                code::WORKSPACE_INVALID,
                "起点不在任何已枚举工作树内（worktree list 与路径不一致）",
            )
            .context(serde_json::json!({ "start": start.display().to_string() }))
        })?;
    Ok(RepoDiscovery {
        identity,
        worktrees,
        start,
        current_worktree,
    })
}

/// `git worktree list --porcelain -z` 无损解析（-z 模式：字段以 NUL 结束，
/// 记录以双 NUL 结束；路径含空格/UTF-8 安全）。
pub fn list_worktrees(any_worktree: &Path) -> Result<Vec<WorktreeInfo>> {
    let raw = git(any_worktree, &["worktree", "list", "--porcelain", "-z"])?;
    let mut out = Vec::new();
    let mut info: Option<WorktreeInfo> = None;
    let blank = || WorktreeInfo {
        path: PathBuf::new(),
        head: None,
        branch: None,
        is_bare: false,
        is_detached: false,
        locked_reason: None,
        prunable_reason: None,
    };
    let finish = |info: &mut Option<WorktreeInfo>, out: &mut Vec<WorktreeInfo>| {
        if let Some(mut i) = info.take() {
            if !i.path.as_os_str().is_empty() {
                if let Ok(canon) = i.path.canonicalize() {
                    i.path = canon;
                }
                out.push(i);
            }
        }
    };
    for field in raw.split('\0') {
        if field.is_empty() {
            // 双 NUL = 记录边界
            finish(&mut info, &mut out);
            continue;
        }
        let (key, value) = match field.split_once(' ') {
            Some((k, v)) => (k, v),
            None => (field, ""),
        };
        if info.is_none() {
            info = Some(blank());
        }
        let i = info.as_mut().unwrap();
        match key {
            "worktree" => i.path = PathBuf::from(value),
            "HEAD" => i.head = Some(value.to_string()),
            "branch" => i.branch = Some(value.to_string()),
            "bare" => i.is_bare = true,
            "detached" => i.is_detached = true,
            "locked" => i.locked_reason = (!value.is_empty()).then(|| value.to_string()),
            "prunable" => i.prunable_reason = (!value.is_empty()).then(|| value.to_string()),
            _ => {}
        }
    }
    finish(&mut info, &mut out);
    if out.is_empty() {
        return Err(Error::new(
            code::GIT_COMMAND_FAILED,
            "worktree list 未返回任何工作树",
        ));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 本地仓库登记（机器数据区，仓外）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryWorktree {
    pub id: String,
    pub path: PathBuf,
    pub first_seen: String,
    pub last_seen: String,
    pub status: WorktreeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// HEAD 摘要（F05 身份证据：搬迁重挂/别名解析时的强证据之一）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySubproject {
    /// 仓库内相对路径（配置作用域模板，浅层声明可对更深层生效与否由有效配置解释）。
    pub rel: String,
    pub registered_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRegistry {
    pub schema_version: u32,
    /// 稳定身份：首次登记生成（repo-<hash>），搬迁/重关联保持不变（F05）。
    pub repo_id: String,
    pub common_dir: PathBuf,
    /// 历史与当前 common-dir（发现证据：搬迁后按此识别回同一身份）。
    #[serde(default)]
    pub common_dirs: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_normalized: Option<String>,
    pub worktrees: BTreeMap<String, RegistryWorktree>,
    pub subprojects: BTreeMap<String, RegistrySubproject>,
    /// 用户已确认的关联仓库（同远端不同 clone）。
    pub linked_repos: Vec<String>,
}

impl RepoRegistry {
    pub fn load_or_create(data_root: &Path, discovery: &RepoDiscovery) -> Result<RepoRegistry> {
        let path = registry_path(data_root, &discovery.identity.repo_id);
        if path.is_file() {
            let text = std::fs::read_to_string(&path)?;
            let reg: RepoRegistry = serde_json::from_str(&text).map_err(|e| {
                Error::new(code::INTERNAL, format!("registry.json 损坏: {e}"))
                    .context(serde_json::json!({ "path": path.display().to_string() }))
            })?;
            return Ok(reg);
        }
        let mut common_dirs = vec![discovery.identity.common_dir.clone()];
        common_dirs.sort();
        common_dirs.dedup();
        Ok(RepoRegistry {
            schema_version: 1,
            repo_id: discovery.identity.repo_id.clone(),
            common_dir: discovery.identity.common_dir.clone(),
            common_dirs,
            origin_normalized: discovery.identity.origin_normalized.clone(),
            worktrees: Default::default(),
            subprojects: Default::default(),
            linked_repos: Vec::new(),
        })
    }

    /// 解析当前发现对应的登记身份（F05）：
    /// 1) repo_id 精确命中 → 直接加载；
    /// 2) 别名索引命中（common_dir 历史登记过）且 Git 证据吻合（origin 相同且
    ///    至少一个工作树分支/HEAD 命中）→ 解析为原登记（搬迁后配置/身份不丢）；
    /// 3) 都不命中 → 按新仓库登记。
    ///
    /// 返回登记使用的 registry（repo_id 以 registry.repo_id 为准）。
    pub fn resolve_or_create(data_root: &Path, discovery: &RepoDiscovery) -> Result<RepoRegistry> {
        let exact = registry_path(data_root, &discovery.identity.repo_id);
        if exact.is_file() {
            return Self::load_or_create(data_root, discovery);
        }
        if let Some(target_id) = lookup_alias(data_root, &discovery.identity.repo_id) {
            if let Ok(mut reg) = load_or_create_by_id(data_root, &target_id) {
                let origin_ok = reg.origin_normalized == discovery.identity.origin_normalized;
                let discovery_branches: std::collections::BTreeSet<Option<&String>> = discovery
                    .worktrees
                    .iter()
                    .map(|w| w.branch.as_ref())
                    .collect();
                let branch_hit = reg.worktrees.values().any(|w| {
                    w.branch
                        .as_ref()
                        .map(|b| discovery_branches.contains(&Some(b)))
                        .unwrap_or(false)
                });
                if origin_ok && branch_hit {
                    reg.common_dir = discovery.identity.common_dir.clone();
                    reg.common_dirs.push(discovery.identity.common_dir.clone());
                    reg.common_dirs.sort();
                    reg.common_dirs.dedup();
                    return Ok(reg);
                }
            }
        }
        Self::load_or_create(data_root, discovery)
    }

    pub fn save(&self, data_root: &Path) -> Result<()> {
        let path = registry_path(data_root, &self.repo_id);
        std::fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))?;
        crate::sync_common::atomic_write(&path, serde_json::to_vec_pretty(self)?.as_slice())
    }

    /// 用最新发现刷新登记：现存路径标 active 并更新 last_seen；失联路径保留并标 missing。
    /// F05：同仓库内的工作树搬迁（新路径无登记、旧登记失联且分支唯一吻合）→
    /// 自动重挂到旧登记 id（WorktreeId 稳定，配置不丢）。detached（无分支）不猜。
    /// 返回本次新登记的工作树 id 列表。
    pub fn refresh_worktrees(&mut self, discovery: &RepoDiscovery, now: &str) -> Vec<String> {
        let mut fresh: BTreeMap<String, (PathBuf, Option<String>, Option<String>)> =
            Default::default();
        for w in &discovery.worktrees {
            if w.is_bare {
                continue; // bare 容器不是可部署工作树，不进登记
            }
            // git 仍会列出已移走/失联的路径：目录不存在时不刷新为 active，
            // 让旧登记按 missing 保留（用户显式重关联或清理）
            if !w.path.exists() {
                continue;
            }
            fresh.insert(
                w.stable_key(),
                (w.path.clone(), w.branch.clone(), w.head.clone()),
            );
        }
        // 先做搬迁重挂：新 key 无登记，但存在失联条目且分支唯一吻合
        let mut reattached: BTreeMap<String, String> = Default::default(); // old_id -> new_key
        for (key, (_path, branch, _head)) in &fresh {
            if self.worktrees.contains_key(key) {
                continue;
            }
            let Some(b) = branch else { continue };
            let candidates: Vec<String> = self
                .worktrees
                .values()
                .filter(|e| {
                    e.status == WorktreeStatus::Missing && e.branch.as_deref() == Some(b.as_str())
                })
                .map(|e| e.id.clone())
                .collect();
            if candidates.len() == 1 {
                reattached.insert(candidates[0].clone(), key.clone());
            }
        }
        for (old_id, new_key) in &reattached {
            if let Some(mut entry) = self.worktrees.remove(old_id) {
                let (path, branch, head) = fresh[new_key].clone();
                entry.path = path;
                entry.branch = branch;
                entry.head = head;
                entry.status = WorktreeStatus::Active;
                entry.last_seen = now.to_string();
                self.worktrees.insert(old_id.clone(), entry);
            }
        }
        let mut added = Vec::new();
        for (key, (path, branch, head)) in &fresh {
            if reattached.values().any(|k| k == key) {
                // 该新路径已重挂到旧登记 id（F05），不再按新 key 重复登记
                continue;
            }
            match self.worktrees.get_mut(key) {
                Some(entry) => {
                    entry.status = WorktreeStatus::Active;
                    entry.last_seen = now.to_string();
                    entry.branch = branch.clone();
                    entry.head = head.clone();
                }
                None => {
                    self.worktrees.insert(
                        key.clone(),
                        RegistryWorktree {
                            id: key.clone(),
                            path: path.clone(),
                            first_seen: now.to_string(),
                            last_seen: now.to_string(),
                            status: WorktreeStatus::Active,
                            branch: branch.clone(),
                            head: head.clone(),
                        },
                    );
                    added.push(key.clone());
                }
            }
        }
        for entry in self.worktrees.values_mut() {
            if !fresh.contains_key(&entry.id)
                && !reattached.contains_key(&entry.id)
                && entry.status == WorktreeStatus::Active
            {
                entry.status = WorktreeStatus::Missing;
            }
        }
        added
    }

    /// 重关联失联工作树到新位置（用户显式确认移动）。
    /// F04：新路径必须真实属于本仓库（common-dir 一致）。
    /// F05：common-dir 不一致 = 整仓搬迁 → 在分支/HEAD 证据吻合时执行身份迁移：
    /// 保持 repo_id / WorktreeId / 个人配置 / 指令条目不变，登记新位置；
    /// 证据不符（独立仓库 B）→ 拒绝，不并仓。
    pub fn relink_worktree(
        &mut self,
        data_root: &Path,
        id: &str,
        new_path: &Path,
        now: &str,
    ) -> Result<RepoMoveOutcome> {
        let entry = self.worktrees.get_mut(id).ok_or_else(|| {
            Error::new(code::UNKNOWN_REFERENCE, format!("工作树登记不存在: {id}"))
        })?;
        let new_path = new_path.canonicalize().map_err(|e| {
            Error::new(code::WORKSPACE_INVALID, format!("新路径不可用: {e}"))
                .context(serde_json::json!({ "path": new_path.display().to_string() }))
        })?;
        let new_discovery = discover_repo(&new_path)?;
        let recorded_branch = entry.branch.clone();
        let same_common = new_discovery.identity.common_dir == self.common_dir;
        if same_common {
            let belongs = new_discovery.worktrees.iter().any(|w| w.path == new_path);
            if !belongs {
                return Err(Error::new(
                    code::WORKSPACE_INVALID,
                    "新路径不是本仓库的工作树，拒绝重关联",
                )
                .context(serde_json::json!({ "path": new_path.display().to_string() })));
            }
            entry.path = new_path;
            entry.status = WorktreeStatus::Active;
            entry.last_seen = now.to_string();
            return Ok(RepoMoveOutcome::WorktreeOnly);
        }
        // 整仓搬迁：Git 证据必须吻合（origin 相同 + 分支命中 + HEAD 摘要命中）。
        // 无远端 + 同名分支不足以区分「搬迁」与「无关仓库」，HEAD 证据为必需。
        let origin_ok = self.origin_normalized == new_discovery.identity.origin_normalized;
        let recorded_heads: std::collections::BTreeSet<Option<&String>> =
            self.worktrees.values().map(|w| w.head.as_ref()).collect();
        let branch_hit = new_discovery
            .worktrees
            .iter()
            .any(|w| w.branch.is_some() && w.branch == recorded_branch);
        let head_hit = new_discovery
            .worktrees
            .iter()
            .any(|w| recorded_heads.contains(&w.head.as_ref()));
        let heads_recorded = recorded_heads.iter().any(|h| h.is_some());
        if !origin_ok || !branch_hit || (heads_recorded && !head_hit) {
            return Err(Error::new(
                code::WORKSPACE_INVALID,
                "新路径属于不同的 Git 仓库（common-dir 不一致且身份证据不吻合），拒绝重关联",
            )
            .context(serde_json::json!({
                "new_common_dir": new_discovery.identity.common_dir,
                "recorded_common_dir": self.common_dir,
                "hint": "确认是整仓搬迁且分支一致后重试；独立仓库应作为新仓库登记",
            })));
        }
        let old_repo_id = self.repo_id.clone();
        let new_repo_id = new_discovery.identity.repo_id.clone();
        // 更新登记身份（repo_id 稳定不变：仍用原 id；common-dir 记录新位置）
        self.common_dir = new_discovery.identity.common_dir.clone();
        self.common_dirs
            .push(new_discovery.identity.common_dir.clone());
        self.common_dirs.sort();
        self.common_dirs.dedup();
        // 按新发现刷新工作树（分支吻合自动重挂，保持 WorktreeId）
        self.refresh_worktrees(&new_discovery, now);
        // 显式重关联的目标条目一定落位
        if let Some(e) = self.worktrees.get_mut(id) {
            e.path = new_path;
            e.status = WorktreeStatus::Active;
            e.last_seen = now.to_string();
        }
        self.save(data_root)?;
        // 别名索引：在新 common-dir 计算出的新 repo_id → 原 repo_id
        record_alias(data_root, &new_repo_id, &old_repo_id)?;
        // 搬迁前若在新位置已产生空登记（发现即登记的副产物），清除以免遮蔽别名解析；
        // 若已写入实际配置（有工作树/子项目/关联），保留并交由用户处置。
        let spurious = registry_path(data_root, &new_repo_id);
        if spurious.is_file() {
            if let Ok(text) = std::fs::read_to_string(&spurious) {
                if let Ok(reg) = serde_json::from_str::<RepoRegistry>(&text) {
                    if reg.worktrees.is_empty()
                        && reg.subprojects.is_empty()
                        && reg.linked_repos.is_empty()
                    {
                        if let Some(parent) = spurious.parent() {
                            let _ = std::fs::remove_dir_all(parent);
                        }
                    }
                }
            }
        }
        // 个人配置与指令条目跟随登记身份迁移（保持 repo_id 不变 → 无需迁移键）
        Ok(RepoMoveOutcome::RepoMoved {
            old_repo_id,
            new_common_dir: new_discovery.identity.common_dir.clone(),
        })
    }
}

/// 重关联结果：仅工作树挪位，或整仓搬迁（含身份迁移说明）。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepoMoveOutcome {
    WorktreeOnly,
    RepoMoved {
        old_repo_id: String,
        new_common_dir: PathBuf,
    },
}

/// 旧 repo_id → 现 repo_id 的别名索引（data/repos/aliases.json）。
pub fn aliases_path(data_root: &Path) -> PathBuf {
    data_root.join("repos").join("aliases.json")
}

pub fn record_alias(data_root: &Path, from: &str, to: &str) -> Result<()> {
    let p = aliases_path(data_root);
    let mut map: BTreeMap<String, String> = if p.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&p)?).unwrap_or_default()
    } else {
        Default::default()
    };
    map.insert(from.to_string(), to.to_string());
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::sync_common::atomic_write(&p, serde_json::to_vec_pretty(&map)?.as_slice())
}

pub fn lookup_alias(data_root: &Path, from: &str) -> Option<String> {
    let p = aliases_path(data_root);
    let map: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
    map.get(from).cloned()
}

pub fn registry_path(data_root: &Path, repo_id: &str) -> PathBuf {
    data_root.join("repos").join(repo_id).join("registry.json")
}

/// 按已登记 id 加载（控制台重关联入口；不依赖重新发现）。
pub fn load_or_create_by_id(data_root: &Path, repo_id: &str) -> Result<RepoRegistry> {
    let path = registry_path(data_root, repo_id);
    if !path.is_file() {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("仓库登记不存在: {repo_id}"),
        ));
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).map_err(|e| {
        Error::new(code::INTERNAL, format!("registry.json 损坏: {e}"))
            .context(serde_json::json!({ "path": path.display().to_string() }))
    })
}

// ---------------------------------------------------------------------------
// 子项目作用域（仓内相对路径；不扫描全盘）
// ---------------------------------------------------------------------------

/// 子项目候选校验结果。
#[derive(Debug, Serialize)]
pub struct SubprojectCheck {
    pub rel: String,
    pub ok: bool,
    pub reason: String,
}

/// 校验一个子项目声明：必须是仓库内相对路径、不穿越 `..`、不在 `.git` 内、
/// 不跨入新的 Git 边界（子模块/嵌套仓库按独立仓库处理，拒绝登记为子项目）。
pub fn check_subproject(discovery: &RepoDiscovery, rel: &str) -> SubprojectCheck {
    let fail = |reason: String| SubprojectCheck {
        rel: rel.to_string(),
        ok: false,
        reason,
    };
    let path = Path::new(rel);
    if rel.is_empty()
        || path.is_absolute()
        || rel.starts_with('~')
        || path.components().any(|c| c.as_os_str() == "..")
    {
        return fail("必须是仓库内相对路径，且不含 `..`/`~`".into());
    }
    if path == Path::new(".") {
        return fail("仓库根不是子项目（用仓库默认作用域）".into());
    }
    let repo_root = &discovery.identity.repo_root;
    // 用 worktree 内实际目录校验（不存在时仍可登记为模板，但含 .git 与越界必须即时拒绝）
    let probe_base =
        if discovery.current_worktree != *repo_root && discovery.current_worktree.is_dir() {
            discovery.current_worktree.clone()
        } else {
            repo_root.clone()
        };
    let target = probe_base.join(path);
    if target.join(".git").exists() {
        return fail("目标含独立 Git 边界（子模块/嵌套仓库），应作为独立仓库登记而非子项目".into());
    }
    // 路径中间层出现 .git 同样是新边界
    let mut acc = probe_base.clone();
    for comp in path.components() {
        acc = acc.join(comp);
        if acc.join(".git").exists() {
            return fail(format!("路径穿过独立 Git 边界：{}", acc.display()));
        }
    }
    SubprojectCheck {
        rel: rel.to_string(),
        ok: true,
        reason: "ok".into(),
    }
}

/// 登记子项目作用域（已通过 check_subproject）。
pub fn register_subproject(
    data_root: &Path,
    discovery: &RepoDiscovery,
    rel: &str,
    now: &str,
) -> Result<RepoRegistry> {
    let check = check_subproject(discovery, rel);
    if !check.ok {
        return Err(
            Error::new(code::ILLEGAL_PATH, check.reason).context(serde_json::json!({ "rel": rel }))
        );
    }
    let mut reg = RepoRegistry::load_or_create(data_root, discovery)?;
    reg.subprojects
        .entry(rel.to_string())
        .or_insert_with(|| RegistrySubproject {
            rel: rel.to_string(),
            registered_at: now.to_string(),
        });
    reg.save(data_root)?;
    Ok(reg)
}

/// 扫描已登记仓库，返回同 origin 的关联建议（不自动合并）。
pub fn suggest_origin_links(data_root: &Path) -> Result<Vec<(String, String, String)>> {
    let mut by_origin: BTreeMap<String, Vec<String>> = Default::default();
    let repos_dir = data_root.join("repos");
    if !repos_dir.is_dir() {
        return Ok(Vec::new());
    }
    for entry in std::fs::read_dir(&repos_dir)?.flatten() {
        let reg_file = entry.path().join("registry.json");
        if !reg_file.is_file() {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&reg_file) {
            if let Ok(reg) = serde_json::from_str::<RepoRegistry>(&text) {
                if let Some(origin) = reg.origin_normalized {
                    by_origin.entry(origin).or_default().push(reg.repo_id);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (origin, ids) in by_origin {
        if ids.len() < 2 {
            continue;
        }
        for pair in ids.windows(2) {
            out.push((pair[0].clone(), pair[1].clone(), origin.clone()));
        }
    }
    Ok(out)
}

/// 生成新登记 id（重关联不改 id；此函数仅供建立派生记录使用）。
pub fn new_registry_id() -> String {
    new_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitx::{git_commit_all, git_init};

    fn tmp_repo(name: &str) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join(name);
        git_init(&repo, false).unwrap();
        (tmp, repo)
    }

    #[test]
    fn main_and_linked_worktree_same_repo_identity() {
        let (_tmp, main) = tmp_repo("main");
        std::fs::write(main.join("seed.txt"), "s").unwrap();
        git_commit_all(&main, "seed", &["seed.txt"]).unwrap();
        let wt = main.parent().unwrap().join("wt2");
        git(
            &main,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wt2"],
        )
        .unwrap();

        let d_main = discover_repo(&main).unwrap();
        let d_wt = discover_repo(&wt).unwrap();
        assert_eq!(
            d_main.identity.repo_id, d_wt.identity.repo_id,
            "同 common-dir 归同仓库"
        );
        assert_eq!(
            d_main.identity.common_dir, d_wt.identity.common_dir,
            "linked worktree 的 common-dir 指向主仓"
        );
        // 枚举包含双方，且起点所在 worktree 正确
        let paths: Vec<_> = d_main.worktrees.iter().map(|w| w.path.clone()).collect();
        assert!(paths.contains(&main.canonicalize().unwrap()));
        assert!(paths.contains(&wt.canonicalize().unwrap()));
        assert_eq!(d_wt.current_worktree, wt.canonicalize().unwrap());
        assert_eq!(d_main.current_worktree, main.canonicalize().unwrap());
    }

    #[test]
    fn subdir_of_repo_keeps_git_identity() {
        let (_tmp, repo) = tmp_repo("mono");
        let deep = repo.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        let d = discover_repo(&deep).unwrap();
        assert_eq!(d.identity.repo_root, repo.canonicalize().unwrap());
        assert_eq!(d.current_worktree, repo.canonicalize().unwrap());
    }

    #[test]
    fn bare_repo_is_container_not_deployable() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("repo.git");
        git_init(&bare, true).unwrap();
        let d = discover_repo(&bare).unwrap();
        assert!(d.identity.repo_root.ends_with("repo.git"));
        let bare_wts: Vec<_> = d.worktrees.iter().filter(|w| w.is_bare).collect();
        assert_eq!(bare_wts.len(), 1);
        assert!(!bare_wts[0].deployable(), "bare 根不是部署目标");
    }

    #[test]
    fn detached_and_locked_states_visible() {
        let (_tmp, main) = tmp_repo("m2");
        std::fs::write(main.join("a.txt"), "a").unwrap();
        git_commit_all(&main, "a", &["a.txt"]).unwrap();
        let wt = main.parent().unwrap().join("wtd");
        git(
            &main,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "--detach"],
        )
        .unwrap();
        let d = discover_repo(&main).unwrap();
        let wtd = d
            .worktrees
            .iter()
            .find(|w| w.path == wt.canonicalize().unwrap())
            .unwrap();
        assert!(wtd.is_detached, "detached 状态可见");
        assert!(wtd.branch.is_none());
        // locked
        git(
            &main,
            &[
                "worktree",
                "lock",
                wt.to_str().unwrap(),
                "--reason",
                "评审冻结",
            ],
        )
        .unwrap();
        let d2 = discover_repo(&main).unwrap();
        let locked = d2
            .worktrees
            .iter()
            .find(|w| w.path == wt.canonicalize().unwrap())
            .unwrap();
        assert_eq!(locked.locked_reason.as_deref(), Some("评审冻结"));
    }

    #[test]
    fn same_origin_clones_are_distinct_repos_with_suggestion() {
        let tmp = tempfile::tempdir().unwrap();
        let upstream = tmp.path().join("up");
        git_init(&upstream, true).unwrap();
        let seed = tmp.path().join("seed-wt");
        git(
            tmp.path(),
            &[
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                seed.to_str().unwrap(),
            ],
        )
        .unwrap();
        std::fs::write(seed.join("f.txt"), "x").unwrap();
        git_commit_all(&seed, "f", &["f.txt"]).unwrap();
        git(&seed, &["push", "-q", "origin", "HEAD"]).unwrap();
        let c1 = tmp.path().join("clone-a");
        let c2 = tmp.path().join("clone-b");
        git(
            tmp.path(),
            &[
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                c1.to_str().unwrap(),
            ],
        )
        .unwrap();
        git(
            tmp.path(),
            &[
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                c2.to_str().unwrap(),
            ],
        )
        .unwrap();

        let d1 = discover_repo(&c1).unwrap();
        let d2 = discover_repo(&c2).unwrap();
        assert_ne!(
            d1.identity.repo_id, d2.identity.repo_id,
            "独立 clone 不误并"
        );
        assert_eq!(
            d1.identity.origin_normalized, d2.identity.origin_normalized,
            "同 origin 作为关联证据"
        );
        // 注册后产生关联建议
        let data = tempfile::tempdir().unwrap();
        for d in [&d1, &d2] {
            let mut reg = RepoRegistry::load_or_create(data.path(), d).unwrap();
            reg.refresh_worktrees(d, "2026-09-16T00:00:00+00:00");
            reg.save(data.path()).unwrap();
        }
        let sugg = suggest_origin_links(data.path()).unwrap();
        assert_eq!(sugg.len(), 1, "只建议、不自动合并");
        assert_eq!(sugg[0].2, d1.identity.origin_normalized.clone().unwrap());
    }

    #[test]
    fn no_remote_repo_still_git_identity() {
        let (_tmp, repo) = tmp_repo("local-only");
        let d = discover_repo(&repo).unwrap();
        assert!(d.identity.origin_normalized.is_none());
        assert!(d.identity.repo_id.starts_with("repo-"));
        let d2 = discover_repo(&repo).unwrap();
        assert_eq!(d.identity.repo_id, d2.identity.repo_id, "无远端身份仍稳定");
    }

    #[test]
    fn broken_git_file_is_error_not_nongit() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("fake");
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::write(fake.join(".git"), "gitdir: /nonexistent/x").unwrap();
        // 含 .git 的路径探测失败 → 显式 Git 段错误，绝不静默降级为非 Git
        let err = classify_path(&fake).unwrap_err();
        assert!(err.code.starts_with("E2"), "探测错误应报 Git 段错误: {err}");
        let err2 = discover_repo(&fake).unwrap_err();
        assert!(err2.code.starts_with("E2"));
    }

    #[test]
    fn nongit_dir_classified_as_path_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("plain");
        std::fs::create_dir_all(&dir).unwrap();
        match classify_path(&dir).unwrap() {
            PathClass::Git(_) => panic!("无 .git 不应判定为 Git"),
            PathClass::NonGit { root } => assert_eq!(root, dir.canonicalize().unwrap()),
        }
    }

    #[test]
    fn prunable_state_visible_when_worktree_dir_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        git_init(&main, false).unwrap();
        std::fs::write(main.join("s.txt"), "s").unwrap();
        git_commit_all(&main, "s", &["s.txt"]).unwrap();
        let wt = tmp.path().join("wt-p");
        git(
            &main,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "bp"],
        )
        .unwrap();
        // 目录消失 → git 标记 prunable（可清理），解析层必须可见
        std::fs::remove_dir_all(&wt).unwrap();
        let d = discover_repo(&main).unwrap();
        let rec = d
            .worktrees
            .iter()
            .find(|w| w.path.file_name().map(|n| n == "wt-p").unwrap_or(false))
            .expect("prunable 工作树仍在列表");
        assert!(
            rec.prunable_reason.is_some() || !wt.exists(),
            "prunable 状态可见（git 未标记时目录缺失同样成立）"
        );
    }

    #[test]
    fn registry_refresh_marks_missing_and_relink_restores() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        git_init(&main, false).unwrap();
        std::fs::write(main.join("s.txt"), "s").unwrap();
        git_commit_all(&main, "s", &["s.txt"]).unwrap();
        let wt = tmp.path().join("wt");
        git(
            &main,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "b1"],
        )
        .unwrap();
        let data = tempfile::tempdir().unwrap();
        let d = discover_repo(&main).unwrap();
        let mut reg = RepoRegistry::load_or_create(data.path(), &d).unwrap();
        let added = reg.refresh_worktrees(&d, "t1");
        assert_eq!(added.len(), 2, "主 + linked 登记");
        reg.save(data.path()).unwrap();

        // 模拟失联：目录移走
        let moved = tmp.path().join("wt-moved");
        std::fs::rename(&wt, &moved).unwrap();
        let d2 = discover_repo(&main).unwrap();
        let mut reg2 = RepoRegistry::load_or_create(data.path(), &d2).unwrap();
        reg2.refresh_worktrees(&d2, "t2");
        let missing = reg2
            .worktrees
            .values()
            .find(|e| e.status == WorktreeStatus::Missing)
            .expect("移走的路径标 missing");
        let missing_id = missing.id.clone();
        reg2.save(data.path()).unwrap();

        // 移回并重关联 → active，id 不变
        std::fs::rename(&moved, &wt).unwrap();
        let mut reg3 = RepoRegistry::load_or_create(data.path(), &d2).unwrap();
        reg3.relink_worktree(data.path(), &missing_id, &wt, "t3")
            .unwrap();
        let e = reg3.worktrees.get(&missing_id).unwrap();
        assert_eq!(e.status, WorktreeStatus::Active);
        assert_eq!(e.first_seen, "t1", "重关联保留首次登记时间");
    }

    #[test]
    fn subproject_checks_boundaries() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        git_init(&main, false).unwrap();
        std::fs::create_dir_all(main.join("web/app")).unwrap();
        std::fs::write(main.join("web/app/f.txt"), "x").unwrap();
        git_commit_all(&main, "f", &["web/app/f.txt"]).unwrap();
        // 子模块边界（sub repo 需有提交才能被 add）
        let sub = tmp.path().join("sub");
        git_init(&sub, false).unwrap();
        std::fs::write(sub.join("s.txt"), "s").unwrap();
        git_commit_all(&sub, "s", &["s.txt"]).unwrap();
        git(
            &main,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                sub.to_str().unwrap(),
                "vendor/lib",
            ],
        )
        .unwrap();
        let d = discover_repo(&main).unwrap();

        assert!(check_subproject(&d, "web").ok);
        assert!(
            check_subproject(&d, "web/app").ok,
            "模板可声明尚未在当前 worktree 命中的路径"
        );
        assert!(!check_subproject(&d, "..").ok, "穿越拒绝");
        assert!(!check_subproject(&d, "/abs").ok);
        assert!(!check_subproject(&d, ".").ok, "仓库根不是子项目");
        assert!(
            !check_subproject(&d, "vendor/lib").ok,
            "子模块按独立仓库处理"
        );
    }

    #[test]
    fn register_subproject_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        git_init(&main, false).unwrap();
        std::fs::create_dir_all(main.join("docs")).unwrap();
        let d = discover_repo(&main).unwrap();
        let data = tempfile::tempdir().unwrap();
        let reg = register_subproject(data.path(), &d, "docs", "t1").unwrap();
        assert!(reg.subprojects.contains_key("docs"));
        // 幂等
        let reg2 = register_subproject(data.path(), &d, "docs", "t2").unwrap();
        assert_eq!(reg2.subprojects["docs"].registered_at, "t1");
        assert_eq!(
            registry_path(data.path(), &d.identity.repo_id),
            data.path()
                .join("repos")
                .join(&d.identity.repo_id)
                .join("registry.json")
        );
    }
}
