//! 项目/角色解析器（AIL-006）：角色 ∪ 项目取并集；shared 显式；learning 只看 shared/项目。
//! 输出 DesiredResourceSet，逐资源带 selected/excluded 原因。

use crate::error::{code, Error, Result};
use crate::manifest::TeamManifest;
use crate::resource::{enumerate, ResourceEntry, ResourceKind};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct Selected {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub reason: String,
    #[serde(skip)]
    pub entry: ResourceEntry,
}

#[derive(Debug, Clone, Serialize)]
pub struct Excluded {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesiredSet {
    pub source: String,
    pub identity: String,
    /// 源仓 skills 根相对路径（用于 SkillStore 实体路径）。
    pub skills_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub content_digest: String,
    pub active_projects: Vec<String>,
    pub active_roles: Vec<String>,
    pub selected: Vec<Selected>,
    pub excluded: Vec<Excluded>,
}

impl DesiredSet {
    pub fn learnings(&self) -> Vec<&Selected> {
        self.selected
            .iter()
            .filter(|s| s.entry.id.kind == ResourceKind::Learning)
            .collect()
    }

    /// 可部署（非 learning）资源。
    pub fn deployable(&self) -> Vec<&Selected> {
        self.selected
            .iter()
            .filter(|s| s.entry.id.kind != ResourceKind::Learning)
            .collect()
    }
}

/// 选择原因字符串是稳定契约的一部分（可解释性验收）。
fn reason_for(
    entry: &ResourceEntry,
    active_projects: &[String],
    active_roles: &[String],
) -> Option<String> {
    let is_learning = entry.id.kind == ResourceKind::Learning;
    let shared = entry.meta.shared;
    let proj_hits: Vec<&str> = entry
        .meta
        .projects
        .iter()
        .filter(|p| active_projects.contains(p))
        .map(String::as_str)
        .collect();
    let role_hits: Vec<&str> = entry
        .meta
        .roles
        .iter()
        .filter(|r| active_roles.contains(r))
        .map(String::as_str)
        .collect();

    if is_learning {
        if shared {
            return Some("shared".into());
        }
        if !proj_hits.is_empty() {
            return Some(format!("project:{}", proj_hits.join(",")));
        }
        return None; // 角色不参与经验选择
    }
    if shared {
        return Some("shared".into());
    }
    match (!proj_hits.is_empty(), !role_hits.is_empty()) {
        (true, true) => Some(format!(
            "project:{}+role:{}",
            proj_hits.join(","),
            role_hits.join(",")
        )),
        (true, false) => Some(format!("project:{}", proj_hits.join(","))),
        (false, true) => Some(format!("role:{}", role_hits.join(","))),
        (false, false) => None,
    }
}

/// 解析请求参数（收敛函数签名）。
pub struct ResolveRequest<'a> {
    pub snapshot_root: &'a Path,
    pub manifest: &'a TeamManifest,
    pub source: &'a str,
    pub identity: &'a str,
    pub revision: Option<String>,
    pub content_digest: String,
    pub active_projects: &'a [String],
    pub active_roles: &'a [String],
}

/// 解析并选择：输入快照 + manifest + 活跃项目/角色；输出带原因的期望资源集合。
/// 同目标键不同 id 的选中资源显式冲突（E3006），绝不最后写入者胜。
pub fn resolve(req: ResolveRequest<'_>) -> Result<DesiredSet> {
    // 活跃项目/角色本身必须合法（binding 校验已做，这里再断言一次）
    for p in req.active_projects {
        req.manifest.require_project(p)?;
    }
    for r in req.active_roles {
        req.manifest.require_role(r)?;
    }
    let entries = enumerate(req.snapshot_root, req.manifest, req.source)?;

    let mut selected = Vec::new();
    let mut excluded = Vec::new();
    for entry in entries {
        let id = entry.id.to_string();
        match reason_for(&entry, req.active_projects, req.active_roles) {
            Some(reason) => selected.push(Selected {
                id,
                kind: entry.id.kind.as_str().to_string(),
                name: entry.id.name.clone(),
                namespace: entry.meta.namespace.clone(),
                reason,
                entry,
            }),
            None => excluded.push(Excluded {
                id,
                reason: format!(
                    "scope 未命中（资源 projects={:?} roles={:?}）",
                    entry.meta.projects, entry.meta.roles
                ),
            }),
        }
    }

    // 目标键冲突检测（独立于 ResourceId 冲突）
    check_target_conflicts(&selected)?;

    Ok(DesiredSet {
        source: req.source.to_string(),
        identity: req.identity.to_string(),
        skills_root: req.manifest.effective_paths().skills,
        revision: req.revision.clone(),
        content_digest: req.content_digest.clone(),
        active_projects: req.active_projects.to_vec(),
        active_roles: req.active_roles.to_vec(),
        selected,
        excluded,
    })
}

/// 目标键冲突：同 key 不同 id → E3006，绝不最后写入者胜。
/// 单源内结构性难以出现；多源（AIL-025）与未来适配器共享落点时是主要防线。
pub fn check_target_conflicts(selected: &[Selected]) -> Result<()> {
    let mut seen: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for s in selected {
        if let Some(key) = s.entry.target_key() {
            seen.entry(key).or_default().push(s.id.clone());
        }
    }
    if let Some((key, ids)) = seen.iter().find(|(_, v)| v.len() > 1) {
        return Err(Error::new(
            code::RESOURCE_ID_CONFLICT,
            "多个资源渲染到同一目标路径，须显式重命名或排除其一",
        )
        .context(serde_json::json!({ "target": key, "resources": ids })));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{ResourceId, ResourceKind, ResourceMeta};

    fn entry(kind: ResourceKind, namespace: &str, name: &str) -> ResourceEntry {
        ResourceEntry {
            id: ResourceId {
                source: "team".into(),
                kind,
                namespace: namespace.into(),
                name: name.into(),
            },
            meta: ResourceMeta {
                shared: true,
                projects: vec![],
                roles: vec![],
                namespace: namespace.into(),
                tags: vec![],
            },
            path: "x".into(),
            description: String::new(),
            raw: None,
        }
    }

    fn sel(kind: ResourceKind, namespace: &str, name: &str, id_name: &str) -> Selected {
        let entry = entry(kind, namespace, name);
        Selected {
            id: id_name.into(),
            kind: kind.as_str().into(),
            name: name.into(),
            namespace: namespace.into(),
            reason: "shared".into(),
            entry,
        }
    }

    #[test]
    fn same_target_key_different_ids_conflict() {
        // 模拟多源：两个不同 id 落到 skill:deploy
        let a = sel(
            ResourceKind::Skill,
            "common",
            "deploy",
            "src-a/skill/common/deploy",
        );
        let b = sel(
            ResourceKind::Skill,
            "team-lib",
            "deploy",
            "src-b/skill/team-lib/deploy",
        );
        let err = check_target_conflicts(&[a, b]).unwrap_err();
        assert_eq!(err.code, "E3006");
        assert_eq!(
            err.to_json()["context"]["resources"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn different_target_keys_ok() {
        let a = sel(
            ResourceKind::Skill,
            "common",
            "deploy",
            "team/skill/common/deploy",
        );
        let b = sel(
            ResourceKind::Rule,
            "common",
            "deploy",
            "team/rule/common/deploy",
        );
        check_target_conflicts(&[a, b]).unwrap();
    }
}
