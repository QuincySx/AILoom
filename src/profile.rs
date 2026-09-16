//! 个人配置层（AIL-040）：仓外 PersonalProfile（<data_root>/profile/profile.toml）
//! 与分层作用域解析（有效配置）。
//!
//! 优先级（低 → 高）：团队声明 → 个人仓库默认 → 仓库子项目模板（浅→深）
//! → 当前工作树覆盖 → 当前工作树子项目覆盖（浅→深）。资源与宿主开关统一用
//! 「继承/启用/禁用」三态：禁用是显式值；未设置（无条目）与显式继承都不同。
//! 每个有效值带来源可追溯；同层重复声明视为冲突，不靠遍历顺序赢。
//!
//! 兼容迁移：v1 工作区（`.ailoom/project.toml` + binding）语义不变；个人层只在
//! 其上叠加期望部署，不重写团队声明、不提升源读取权限。profile.toml 位置在机器
//! 数据区（仓外），项目根不需要任何个人文件。

use crate::error::{code, Error, Result};
use crate::manifest::validate_relative_path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// 三态选择：继承（本层不表态）/ 启用 / 禁用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriState {
    Inherit,
    Enable,
    Disable,
}

/// 一个作用域的选择集：宿主与资源（完整 ResourceId）各一张三态表。
/// 空表 = 该层未设置任何选择（合法且常见）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ScopeSelection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hosts: BTreeMap<String, TriState>,
    /// key 为完整 ResourceId（source/kind/namespace/name）
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<String, TriState>,
}

impl ScopeSelection {
    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty() && self.resources.is_empty()
    }
}

/// 仓库子项目模板 / 工作树子项目覆盖条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubprojectSelection {
    /// 仓库内相对路径（模板按浅→深依次生效；路径不必在当前 worktree 命中）
    pub path: String,
    #[serde(flatten)]
    pub selection: ScopeSelection,
}

/// 单仓库个人配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<ScopeSelection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subprojects: Vec<SubprojectSelection>,
    /// key = 仓库登记的工作树 id（repo_registry RegistryWorktree.id）
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub worktrees: BTreeMap<String, ScopeSelection>,
    /// key = 工作树 id；value = 该工作树内的子项目覆盖
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub wt_subprojects: BTreeMap<String, Vec<SubprojectSelection>>,
}

/// 个人资源库（复用 source manifest 布局；路径是机器绝对路径，仓外）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LibraryRef {
    pub path: PathBuf,
}

/// 仓外个人配置根文件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersonalProfile {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<LibraryRef>,
    /// key = repo_registry RepoIdentity.repo_id
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repos: BTreeMap<String, RepoProfile>,
}

impl PersonalProfile {
    pub fn new() -> PersonalProfile {
        PersonalProfile {
            schema_version: PROFILE_SCHEMA_VERSION,
            library: None,
            repos: Default::default(),
        }
    }

    pub fn profile_path(data_root: &Path) -> PathBuf {
        data_root.join("profile").join("profile.toml")
    }

    /// 加载；文件不存在 → 空个人层（纯 v1 行为，个人模式零门槛）。
    pub fn load_or_default(data_root: &Path) -> Result<PersonalProfile> {
        let path = Self::profile_path(data_root);
        if !path.is_file() {
            return Ok(Self::new());
        }
        Self::load(&path)
    }

    pub fn load(path: &Path) -> Result<PersonalProfile> {
        let text = std::fs::read_to_string(path)?;
        let profile: PersonalProfile = toml::from_str(&text).map_err(|e| {
            Error::new(code::SCHEMA_VERSION, format!("profile.toml 解析失败: {e}"))
                .context(serde_json::json!({ "path": path.display().to_string() }))
        })?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn save(&self, data_root: &Path) -> Result<()> {
        self.validate()?;
        let path = Self::profile_path(data_root);
        std::fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))?;
        crate::sync_common::atomic_write(&path, toml::to_string_pretty(self)?.as_bytes())
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PROFILE_SCHEMA_VERSION {
            return Err(Error::new(
                code::SCHEMA_VERSION,
                format!(
                    "不支持的 profile.toml schema_version: {}",
                    self.schema_version
                ),
            ));
        }
        for (repo_id, repo) in &self.repos {
            if !repo_id.starts_with("repo-") || repo_id.len() != "repo-".len() + 16 {
                return Err(Error::new(
                    code::ILLEGAL_PATH,
                    format!("repos key 非法（应为 repo_registry repo_id）: {repo_id}"),
                ));
            }
            // 子项目路径在两层都必须是相对路径
            for sp in &repo.subprojects {
                validate_relative_path("subprojects.path", &sp.path)?;
            }
            for sps in repo.wt_subprojects.values() {
                for sp in sps {
                    validate_relative_path("wt_subprojects.path", &sp.path)?;
                }
            }
            // 同层重复路径是配置错误：先解决，不能靠遍历顺序赢
            check_dup_paths(&repo.subprojects, &format!("repos.{repo_id}.subprojects"))?;
            for (wt, sps) in &repo.wt_subprojects {
                check_dup_paths(sps, &format!("repos.{repo_id}.wt_subprojects.{wt}"))?;
            }
        }
        Ok(())
    }

    pub fn repo(&self, repo_id: &str) -> RepoProfile {
        self.repos.get(repo_id).cloned().unwrap_or_default()
    }
}

fn check_dup_paths(sps: &[SubprojectSelection], field: &str) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for sp in sps {
        if !seen.insert(sp.path.as_str()) {
            return Err(Error::new(
                code::DUPLICATE_ITEM,
                format!("{field} 含重复子项目路径: {}（同层冲突须先解决）", sp.path),
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 有效配置解析
// ---------------------------------------------------------------------------

/// 配置来源层（低 → 高）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeOrigin {
    TeamDeclaration,
    RepoDefault,
    RepoSubproject { path: String },
    WorktreeOverride,
    WorktreeSubproject { path: String },
}

impl ScopeOrigin {
    pub fn describe(&self) -> String {
        match self {
            ScopeOrigin::TeamDeclaration => "团队声明".into(),
            ScopeOrigin::RepoDefault => "个人仓库默认".into(),
            ScopeOrigin::RepoSubproject { path } => format!("仓库子项目模板 {path}"),
            ScopeOrigin::WorktreeOverride => "当前工作树覆盖".into(),
            ScopeOrigin::WorktreeSubproject { path } => format!("工作树子项目覆盖 {path}"),
        }
    }
    /// 同层内排序（子项目浅→深由调用方保证顺序；此函数用于展示）
    pub fn rank(&self) -> u8 {
        match self {
            ScopeOrigin::TeamDeclaration => 0,
            ScopeOrigin::RepoDefault => 1,
            ScopeOrigin::RepoSubproject { .. } => 2,
            ScopeOrigin::WorktreeOverride => 3,
            ScopeOrigin::WorktreeSubproject { .. } => 4,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceEntry {
    pub origin: ScopeOrigin,
    pub choice: TriState,
    /// 显式 Inherit 条目也进 trace：它表示“该层声明继承下层”，不改变值
    pub effective: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveResource {
    /// 最终是否由 AILoom 部署该资源（个人层叠加在团队层之上后的期望）
    pub deployed: bool,
    /// 当前值的来源；没有任何层表态时为 None（未设置 ≠ 显式禁用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<ScopeOrigin>,
    pub trace: Vec<TraceEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveHost {
    pub enabled: bool,
    pub origin: ScopeOrigin,
    pub trace: Vec<TraceEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub kind: String,
    pub key: String,
    pub origins: Vec<ScopeOrigin>,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EffectiveConfig {
    pub resources: BTreeMap<String, EffectiveResource>,
    pub hosts: BTreeMap<String, EffectiveHost>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<Conflict>,
}

impl EffectiveConfig {
    /// 汇总个人层显式启用的资源（AIL-044 部署期望的输入之一）。
    pub fn enabled_resources(&self) -> Vec<String> {
        self.resources
            .iter()
            .filter(|(_, v)| v.deployed)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

pub struct ResolveScopeRequest<'a> {
    pub profile: &'a PersonalProfile,
    pub repo_id: &'a str,
    /// 仓库登记的工作树 id（repo_registry RegistryWorktree.id）
    pub worktree_id: &'a str,
    /// 当前作用域在仓库内的相对路径（None = worktree 根）
    pub active_rel: Option<&'a str>,
    /// 团队层当前期望部署的完整 ResourceId（v1 解析结果；无团队绑定可为空）
    pub team_enabled: &'a [String],
    /// 团队声明启用的宿主名（claude/codex/alva/…）
    pub team_hosts: &'a [String],
}

fn rel_depth(rel: &str) -> usize {
    Path::new(rel).components().count()
}

/// 子项目条目是否命中当前作用域：active == path 或 active 在 path 之下。
fn subproject_matches(path: &str, active_rel: Option<&str>) -> bool {
    let Some(active) = active_rel else {
        return false;
    };
    if active == path {
        return true;
    }
    let prefix: String = if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    };
    active.starts_with(&prefix)
}

/// 解析某仓库×工作树×子项目作用域的有效配置（每个值可追溯来源）。
/// 同层冲突（重复子项目路径由 validate 拒绝；重复 key 由 TOML 拒绝）之外，
/// 跨层按优先级覆盖；显式 Inherit 记录 trace 但不改变值。
pub fn resolve_effective(req: ResolveScopeRequest<'_>) -> EffectiveConfig {
    let mut out = EffectiveConfig::default();
    let repo = req.profile.repo(req.repo_id);

    // --- 资源 ---
    // 团队层基线（v1 语义不变：团队声明决定的期望集合）
    let mut all_ids: std::collections::BTreeSet<String> =
        req.team_enabled.iter().cloned().collect();
    // 个人层出现的 id 也纳入输出（即使团队层未选中，供禁用/启用解释）
    let mut collect = |sel: &ScopeSelection| {
        for k in sel.resources.keys() {
            all_ids.insert(k.clone());
        }
    };
    if let Some(d) = &repo.default {
        collect(d);
    }
    for sp in &repo.subprojects {
        collect(&sp.selection);
    }
    if let Some(wt) = repo.worktrees.get(req.worktree_id) {
        collect(wt);
    }
    if let Some(sps) = repo.wt_subprojects.get(req.worktree_id) {
        for sp in sps {
            collect(&sp.selection);
        }
    }

    for rid in all_ids {
        let team_hit = req.team_enabled.iter().any(|x| x == &rid);
        let mut deployed = team_hit;
        let mut origin = team_hit.then_some(ScopeOrigin::TeamDeclaration);
        let mut trace = Vec::new();
        if team_hit {
            trace.push(TraceEntry {
                origin: ScopeOrigin::TeamDeclaration,
                choice: TriState::Enable,
                effective: true,
            });
        }
        // 逐层应用（低 → 高）
        let mut layers: Vec<(ScopeOrigin, TriState)> = Vec::new();
        if let Some(d) = &repo.default {
            if let Some(choice) = d.resources.get(&rid) {
                layers.push((ScopeOrigin::RepoDefault, *choice));
            }
        }
        // 仓库子项目模板：浅 → 深
        let mut repo_templates: Vec<&SubprojectSelection> = repo
            .subprojects
            .iter()
            .filter(|sp| subproject_matches(&sp.path, req.active_rel))
            .collect();
        repo_templates.sort_by_key(|sp| rel_depth(&sp.path));
        for sp in repo_templates {
            if let Some(choice) = sp.selection.resources.get(&rid) {
                layers.push((
                    ScopeOrigin::RepoSubproject {
                        path: sp.path.clone(),
                    },
                    *choice,
                ));
            }
        }
        if let Some(wt) = repo.worktrees.get(req.worktree_id) {
            if let Some(choice) = wt.resources.get(&rid) {
                layers.push((ScopeOrigin::WorktreeOverride, *choice));
            }
        }
        if let Some(sps) = repo.wt_subprojects.get(req.worktree_id) {
            let mut wt_templates: Vec<&SubprojectSelection> = sps
                .iter()
                .filter(|sp| subproject_matches(&sp.path, req.active_rel))
                .collect();
            wt_templates.sort_by_key(|sp| rel_depth(&sp.path));
            for sp in wt_templates {
                if let Some(choice) = sp.selection.resources.get(&rid) {
                    layers.push((
                        ScopeOrigin::WorktreeSubproject {
                            path: sp.path.clone(),
                        },
                        *choice,
                    ));
                }
            }
        }
        for (o, choice) in layers {
            let effective = choice != TriState::Inherit;
            if effective {
                deployed = choice == TriState::Enable;
                origin = Some(o.clone());
            }
            trace.push(TraceEntry {
                origin: o,
                choice,
                effective,
            });
        }
        out.resources.insert(
            rid,
            EffectiveResource {
                deployed,
                origin,
                trace,
            },
        );
    }

    // --- 宿主 ---
    let mut host_names: std::collections::BTreeSet<String> =
        req.team_hosts.iter().cloned().collect();
    let mut collect_hosts = |sel: &ScopeSelection| {
        for k in sel.hosts.keys() {
            host_names.insert(k.clone());
        }
    };
    if let Some(d) = &repo.default {
        collect_hosts(d);
    }
    for sp in &repo.subprojects {
        collect_hosts(&sp.selection);
    }
    if let Some(wt) = repo.worktrees.get(req.worktree_id) {
        collect_hosts(wt);
    }
    if let Some(sps) = repo.wt_subprojects.get(req.worktree_id) {
        for sp in sps {
            collect_hosts(&sp.selection);
        }
    }
    for host in host_names {
        let team_on = req.team_hosts.iter().any(|x| x == &host);
        let mut enabled = team_on;
        let mut origin = ScopeOrigin::TeamDeclaration;
        let mut trace = vec![TraceEntry {
            origin: ScopeOrigin::TeamDeclaration,
            choice: if team_on {
                TriState::Enable
            } else {
                TriState::Disable
            },
            effective: true,
        }];
        let mut layers: Vec<(ScopeOrigin, TriState)> = Vec::new();
        if let Some(d) = &repo.default {
            if let Some(choice) = d.hosts.get(&host) {
                layers.push((ScopeOrigin::RepoDefault, *choice));
            }
        }
        let mut repo_templates: Vec<&SubprojectSelection> = repo
            .subprojects
            .iter()
            .filter(|sp| subproject_matches(&sp.path, req.active_rel))
            .collect();
        repo_templates.sort_by_key(|sp| rel_depth(&sp.path));
        for sp in repo_templates {
            if let Some(choice) = sp.selection.hosts.get(&host) {
                layers.push((
                    ScopeOrigin::RepoSubproject {
                        path: sp.path.clone(),
                    },
                    *choice,
                ));
            }
        }
        if let Some(wt) = repo.worktrees.get(req.worktree_id) {
            if let Some(choice) = wt.hosts.get(&host) {
                layers.push((ScopeOrigin::WorktreeOverride, *choice));
            }
        }
        if let Some(sps) = repo.wt_subprojects.get(req.worktree_id) {
            let mut wt_templates: Vec<&SubprojectSelection> = sps
                .iter()
                .filter(|sp| subproject_matches(&sp.path, req.active_rel))
                .collect();
            wt_templates.sort_by_key(|sp| rel_depth(&sp.path));
            for sp in wt_templates {
                if let Some(choice) = sp.selection.hosts.get(&host) {
                    layers.push((
                        ScopeOrigin::WorktreeSubproject {
                            path: sp.path.clone(),
                        },
                        *choice,
                    ));
                }
            }
        }
        for (o, choice) in layers {
            let effective = choice != TriState::Inherit;
            if effective {
                enabled = choice == TriState::Enable;
                origin = o.clone();
            }
            trace.push(TraceEntry {
                origin: o,
                choice,
                effective,
            });
        }
        out.hosts.insert(
            host,
            EffectiveHost {
                enabled,
                origin,
                trace,
            },
        );
    }

    out
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default, clippy::cloned_ref_to_slice_refs)]
mod tests {
    use super::*;

    fn rid(name: &str) -> String {
        format!("team/skill/common/{name}")
    }
    fn pid(name: &str) -> String {
        format!("personal/skill/common/{name}")
    }

    fn scope(resources: &[(&str, TriState)]) -> ScopeSelection {
        ScopeSelection {
            hosts: Default::default(),
            resources: resources.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn layering_priority_team_repo_subproject_worktree() {
        let a = rid("a");
        let mut profile = PersonalProfile::new();
        let mut repo = RepoProfile::default();
        // 仓库默认：禁用 a
        repo.default = Some(scope(&[(&a, TriState::Disable)]));
        // 仓库子项目模板 web：启用 a（比默认深，应覆盖）
        repo.subprojects.push(SubprojectSelection {
            path: "web".into(),
            selection: scope(&[(&a, TriState::Enable)]),
        });
        profile.repos.insert("repo-0123456789abcdef".into(), repo);

        // worktree 根：a 被仓库默认禁用
        let at_root = resolve_effective(ResolveScopeRequest {
            profile: &profile,
            repo_id: "repo-0123456789abcdef",
            worktree_id: "wt1",
            active_rel: None,
            team_enabled: &[a.clone()],
            team_hosts: &["claude".into()],
        });
        let ra = &at_root.resources[&a];
        assert!(!ra.deployed, "仓库默认禁用覆盖团队启用");
        assert_eq!(ra.origin.as_ref().unwrap(), &ScopeOrigin::RepoDefault);

        // web 子项目：模板启用 a
        let at_web = resolve_effective(ResolveScopeRequest {
            profile: &profile,
            repo_id: "repo-0123456789abcdef",
            worktree_id: "wt1",
            active_rel: Some("web"),
            team_enabled: &[a.clone()],
            team_hosts: &["claude".into()],
        });
        let ra = &at_web.resources[&a];
        assert!(ra.deployed, "子项目模板启用覆盖仓库默认禁用");
        assert_eq!(
            ra.origin.as_ref().unwrap(),
            &ScopeOrigin::RepoSubproject { path: "web".into() }
        );
        // 深层子项目同样命中浅模板
        let at_deep = resolve_effective(ResolveScopeRequest {
            profile: &profile,
            repo_id: "repo-0123456789abcdef",
            worktree_id: "wt1",
            active_rel: Some("web/docs/guide"),
            team_enabled: &[],
            team_hosts: &[],
        });
        assert!(at_deep.resources[&a].deployed, "浅模板对深层生效");
    }

    #[test]
    fn worktree_override_and_wt_subproject_top_priority() {
        let a = rid("a");
        let mut profile = PersonalProfile::new();
        let mut repo = RepoProfile::default();
        repo.default = Some(scope(&[(&a, TriState::Enable)]));
        repo.worktrees
            .insert("wt2".into(), scope(&[(&a, TriState::Disable)]));
        repo.wt_subprojects.insert(
            "wt2".into(),
            vec![SubprojectSelection {
                path: "svc".into(),
                selection: scope(&[(&a, TriState::Enable)]),
            }],
        );
        profile.repos.insert("repo-0123456789abcdef".into(), repo);

        let q = |active: Option<&'static str>| {
            resolve_effective(ResolveScopeRequest {
                profile: &profile,
                repo_id: "repo-0123456789abcdef",
                worktree_id: "wt2",
                active_rel: active,
                team_enabled: &[],
                team_hosts: &[],
            })
        };
        assert!(
            !q(None).resources[&a].deployed,
            "worktree 覆盖优先于仓库默认"
        );
        assert!(
            q(Some("svc")).resources[&a].deployed,
            "worktree 子项目优先级最高"
        );
        // 其他工作树（wt1）没有工作树层表态：仓库默认仍生效，但不出现 WorktreeOverride
        let wt1 = resolve_effective(ResolveScopeRequest {
            profile: &profile,
            repo_id: "repo-0123456789abcdef",
            worktree_id: "wt1",
            active_rel: None,
            team_enabled: &[],
            team_hosts: &[],
        });
        let ra1 = &wt1.resources[&a];
        assert!(ra1.deployed, "仓库默认对未覆盖工作树同样生效");
        assert_eq!(ra1.origin.as_ref().unwrap(), &ScopeOrigin::RepoDefault);
        assert!(
            ra1.trace
                .iter()
                .all(|t| t.origin != ScopeOrigin::WorktreeOverride),
            "wt1 的 trace 不含工作树覆盖层"
        );
    }

    #[test]
    fn unset_vs_inherit_vs_disable_distinguished() {
        let a = rid("a");
        let p = rid("b");
        let mut profile = PersonalProfile::new();
        let mut repo = RepoProfile::default();
        repo.default = Some(scope(&[(&a, TriState::Inherit), (&p, TriState::Disable)]));
        profile.repos.insert("repo-0123456789abcdef".into(), repo);
        let out = resolve_effective(ResolveScopeRequest {
            profile: &profile,
            repo_id: "repo-0123456789abcdef",
            worktree_id: "wt1",
            active_rel: None,
            team_enabled: &[a.clone()],
            team_hosts: &[],
        });
        // a：显式继承 → 值仍来自团队层
        let ra = &out.resources[&a];
        assert!(ra.deployed);
        assert_eq!(ra.origin.as_ref().unwrap(), &ScopeOrigin::TeamDeclaration);
        assert_eq!(ra.trace.len(), 2, "inherit 也进 trace");
        assert!(!ra.trace[1].effective);
        // p：显式禁用（团队未选它，个人层也禁用 → 不部署且有来源）
        let rp = &out.resources[&p];
        assert!(!rp.deployed);
        assert_eq!(rp.origin.as_ref().unwrap(), &ScopeOrigin::RepoDefault);
        // c：完全未设置
        let c = rid("c");
        assert!(
            !out.resources.contains_key(&c),
            "未出现的 id 不凭空生成条目"
        );
    }

    #[test]
    fn personal_only_resource_enabled_without_team() {
        let p = pid("my-skill");
        let mut profile = PersonalProfile::new();
        let mut repo = RepoProfile::default();
        repo.default = Some(scope(&[(&p, TriState::Enable)]));
        profile.repos.insert("repo-fedcba9876543210".into(), repo);
        let out = resolve_effective(ResolveScopeRequest {
            profile: &profile,
            repo_id: "repo-fedcba9876543210",
            worktree_id: "wt1",
            active_rel: None,
            team_enabled: &[],
            team_hosts: &["codex".into()],
        });
        let rp = &out.resources[&p];
        assert!(rp.deployed, "个人库资源可独立于团队启用");
        assert_eq!(rp.origin.as_ref().unwrap(), &ScopeOrigin::RepoDefault);
        assert!(out.hosts["codex"].enabled);
        assert_eq!(out.hosts["codex"].origin, ScopeOrigin::TeamDeclaration);
    }

    #[test]
    fn host_three_state_layers() {
        let mut profile = PersonalProfile::new();
        let mut repo = RepoProfile::default();
        let mut host_sel = ScopeSelection::default();
        host_sel.hosts.insert("codex".into(), TriState::Disable);
        repo.default = Some(host_sel);
        repo.subprojects.push(SubprojectSelection {
            path: "web".into(),
            selection: {
                let mut s = ScopeSelection::default();
                s.hosts.insert("codex".into(), TriState::Enable);
                s
            },
        });
        profile.repos.insert("repo-0123456789abcdef".into(), repo);
        let q = |active: Option<&'static str>| {
            resolve_effective(ResolveScopeRequest {
                profile: &profile,
                repo_id: "repo-0123456789abcdef",
                worktree_id: "wt1",
                active_rel: active,
                team_enabled: &[],
                team_hosts: &["claude".into(), "codex".into()],
            })
        };
        assert!(!q(None).hosts["codex"].enabled, "仓库默认禁用宿主");
        assert!(
            q(Some("web")).hosts["codex"].enabled,
            "子项目模板重新启用宿主"
        );
        assert!(q(None).hosts["claude"].enabled, "未表态宿主继承团队");
    }

    #[test]
    fn deeper_template_overrides_shallower() {
        let a = rid("a");
        let mut profile = PersonalProfile::new();
        let mut repo = RepoProfile::default();
        repo.subprojects.push(SubprojectSelection {
            path: "web".into(),
            selection: scope(&[(&a, TriState::Enable)]),
        });
        repo.subprojects.push(SubprojectSelection {
            path: "web/docs".into(),
            selection: scope(&[(&a, TriState::Disable)]),
        });
        profile.repos.insert("repo-0123456789abcdef".into(), repo);
        let out = resolve_effective(ResolveScopeRequest {
            profile: &profile,
            repo_id: "repo-0123456789abcdef",
            worktree_id: "wt1",
            active_rel: Some("web/docs"),
            team_enabled: &[],
            team_hosts: &[],
        });
        assert!(!out.resources[&a].deployed, "深层模板覆盖浅层");
        assert_eq!(
            out.resources[&a].origin.as_ref().unwrap(),
            &ScopeOrigin::RepoSubproject {
                path: "web/docs".into()
            }
        );
    }

    #[test]
    fn duplicate_subproject_path_rejected_same_layer_conflict() {
        let mut profile = PersonalProfile::new();
        let repo = RepoProfile {
            subprojects: vec![
                SubprojectSelection {
                    path: "web".into(),
                    selection: scope(&[("team/skill/common/a", TriState::Enable)]),
                },
                SubprojectSelection {
                    path: "web".into(),
                    selection: scope(&[("team/skill/common/a", TriState::Disable)]),
                },
            ],
            ..Default::default()
        };
        profile.repos.insert("repo-0123456789abcdef".into(), repo);
        let err = profile.validate().unwrap_err();
        assert_eq!(
            err.code,
            code::DUPLICATE_ITEM,
            "同层冲突先解决，不靠遍历顺序赢"
        );
    }

    #[test]
    fn toml_roundtrip_preserves_tri_states_and_empty_layers() {
        let mut profile = PersonalProfile::new();
        profile.library = Some(LibraryRef {
            path: PathBuf::from("/data/library"),
        });
        let mut repo = RepoProfile::default();
        let mut sel = ScopeSelection::default();
        sel.resources
            .insert("personal/skill/common/x".into(), TriState::Disable);
        sel.resources
            .insert("team/skill/common/y".into(), TriState::Inherit);
        repo.default = Some(sel);
        profile.repos.insert("repo-0123456789abcdef".into(), repo);
        let text = toml::to_string_pretty(&profile).unwrap();
        let back: PersonalProfile = toml::from_str(&text).unwrap();
        let sel = &back.repos["repo-0123456789abcdef"]
            .default
            .as_ref()
            .unwrap();
        assert_eq!(sel.resources["personal/skill/common/x"], TriState::Disable);
        assert_eq!(sel.resources["team/skill/common/y"], TriState::Inherit);
        assert_eq!(
            back.library.as_ref().unwrap().path,
            PathBuf::from("/data/library")
        );
    }

    #[test]
    fn empty_profile_is_valid_and_means_pure_v1() {
        let profile = PersonalProfile::new();
        let text = toml::to_string_pretty(&profile).unwrap();
        let back: PersonalProfile = toml::from_str(&text).unwrap();
        assert!(back.repos.is_empty());
        let out = resolve_effective(ResolveScopeRequest {
            profile: &back,
            repo_id: "repo-0123456789abcdef",
            worktree_id: "wt1",
            active_rel: None,
            team_enabled: &["team/skill/common/a".into()],
            team_hosts: &["claude".into()],
        });
        assert!(
            out.resources["team/skill/common/a"].deployed,
            "无个人层 → 团队原样"
        );
        assert!(out.conflicts.is_empty());
    }
}
