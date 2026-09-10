//! Workspace Require：选用 Skill 门禁（相对选择模型的交集）。

use crate::config::RequireDeclaration;
use crate::error::{code, Error, Result};
use crate::resolver::{DesiredSet, Excluded};
use crate::resource::ResourceKind;
use crate::store;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
struct AgentPersonaFile {
    #[serde(default)]
    skills: Vec<String>,
}

/// 解析有效 Require 键列表；无 Require/Agent 时返回 None（不做门禁）。
pub fn effective_require_keys(
    require: &RequireDeclaration,
    workspace_root: &Path,
) -> Result<Option<Vec<String>>> {
    if require.is_empty() {
        return Ok(None);
    }
    let mut keys: BTreeSet<String> = require.skills.iter().cloned().collect();
    if let Some(agent) = require.agent.as_deref() {
        let agent_skills = load_agent_skills(workspace_root, agent)?;
        if keys.is_empty() {
            keys = agent_skills.into_iter().collect();
        } else {
            for s in &agent_skills {
                if !keys.contains(s) {
                    return Err(Error::new(
                        code::UNKNOWN_REFERENCE,
                        format!("Agent `{agent}` 引用了未在 Workspace Require 中的 Skill: {s}"),
                    )
                    .fix("把该 Skill 加入 [require].skills，或从人设中删除"));
                }
            }
            keys = agent_skills.into_iter().collect();
        }
    }
    if keys.is_empty() {
        return Ok(None);
    }
    Ok(Some(keys.into_iter().collect()))
}

fn load_agent_skills(workspace_root: &Path, name: &str) -> Result<Vec<String>> {
    if !crate::manifest::valid_id(name, 64) {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("require.agent 非法: {name}"),
        ));
    }
    let path = workspace_root
        .join(crate::workspace::AILOOM_DIR)
        .join("agents")
        .join(format!("{name}.toml"));
    if !path.is_file() {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("Agent 人设不存在: {}", path.display()),
        )
        .fix("创建 .ailoom/agents/<name>.toml，或修正 require.agent"));
    }
    let text = std::fs::read_to_string(&path)?;
    let file: AgentPersonaFile = toml::from_str(&text).map_err(|e| {
        Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("Agent 人设解析失败: {e}"),
        )
    })?;
    if file.skills.is_empty() {
        return Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("Agent `{name}` 的 skills 为空"),
        ));
    }
    crate::config::check_unique_public(&file.skills, "agent.skills")?;
    Ok(file.skills)
}

fn skill_match_keys(entry_path: &str, skills_root: &str, name: &str) -> Vec<String> {
    let mut keys = vec![name.to_string()];
    if let Ok(rel) = store::skill_rel_under_root(Path::new(entry_path), Path::new(skills_root)) {
        let s = rel.to_string_lossy().replace('\\', "/");
        if !s.is_empty() && s != name {
            keys.push(s);
        }
    }
    keys
}

fn matches_any(entry_path: &str, skills_root: &str, name: &str, reqs: &[String]) -> bool {
    let keys = skill_match_keys(entry_path, skills_root, name);
    reqs.iter().any(|r| keys.iter().any(|k| k == r))
}

/// 校验 Require 键均可在当前选择结果中解析（未知或未入选 → E3004）。
pub fn assert_require_resolvable(sets: &[&DesiredSet], keys: &[String]) -> Result<()> {
    let mut in_scope: BTreeSet<String> = BTreeSet::new();
    let mut known: BTreeSet<String> = BTreeSet::new();
    for d in sets {
        for s in &d.selected {
            if s.entry.id.kind != ResourceKind::Skill {
                continue;
            }
            for k in skill_match_keys(&s.entry.path, &d.skills_root, &s.entry.id.name) {
                known.insert(k.clone());
                if keys.iter().any(|r| r == &k) {
                    in_scope.insert(k);
                }
            }
        }
        for e in &d.excluded {
            if let Some(name) = e.id.rsplit('/').next() {
                known.insert(name.to_string());
            }
        }
    }
    for req in keys {
        let known_hit = known.iter().any(|k| k == req);
        let scope_hit = in_scope.iter().any(|k| k == req);
        if !known_hit {
            return Err(Error::new(
                code::UNKNOWN_REFERENCE,
                format!("Require 引用了未知 Skill: {req}"),
            )
            .fix("修正 .ailoom/project.toml 的 [require].skills"));
        }
        if !scope_hit {
            return Err(Error::new(
                code::UNKNOWN_REFERENCE,
                format!("Require 的 Skill `{req}` 存在但不在当前项目/角色选择范围内"),
            )
            .fix("调整 projects/roles，或从 Require 中移除"));
        }
    }
    Ok(())
}

/// 将 DesiredSet 中的 Skill 与 Require 取交集；非 Skill 不动。
pub fn filter_skills_by_require(mut desired: DesiredSet, keys: &[String]) -> DesiredSet {
    let skills_root = desired.skills_root.clone();
    let mut kept = Vec::new();
    for s in desired.selected.drain(..) {
        if s.entry.id.kind != ResourceKind::Skill {
            kept.push(s);
            continue;
        }
        if matches_any(&s.entry.path, &skills_root, &s.entry.id.name, keys) {
            kept.push(s);
        } else {
            desired.excluded.push(Excluded {
                id: s.id.clone(),
                reason: format!("Require 未选用（source={}）", desired.source),
            });
        }
    }
    desired.selected = kept;
    desired
}

/// 检查工作区托管 skill symlink 是否指向仍存在的目录（供 status/doctor）。
pub fn broken_skill_symlinks(
    workspace_root: &Path,
    managed: &crate::sync::manifest::ManagedManifest,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (key, item) in &managed.items {
        if item.kind != "skill" {
            continue;
        }
        let (rel, _) = crate::sync::plan::split_key(key);
        let path = workspace_root.join(&rel);
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                out.push((key.clone(), "missing".into()));
                continue;
            }
        };
        if !meta.file_type().is_symlink() {
            continue;
        }
        let target = match std::fs::read_link(&path) {
            Ok(t) => t,
            Err(_) => {
                out.push((key.clone(), "unreadable-link".into()));
                continue;
            }
        };
        let abs = if target.is_absolute() {
            target
        } else {
            path.parent().unwrap_or(workspace_root).join(target)
        };
        if !abs.is_dir() {
            out.push((
                key.clone(),
                format!("store-target-missing:{}", abs.display()),
            ));
        }
    }
    out
}
