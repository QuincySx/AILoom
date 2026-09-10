//! Skills 适配（AIL-009 + ADR-0001）：实体进 SkillStore，Workspace 只挂 symlink。
//! Claude：`.claude/skills/<name>` → store；Codex：`.ailoom/skills/<name>` → store + config.toml。

use super::common::{Artifact, ArtifactBody, CODEX_MANAGED_SKILL_PREFIX};
use super::{Tool, UnsupportedItem};
use crate::error::{code, Error, Result};
use crate::resource::ResourceEntry;
use crate::store;
use std::path::{Path, PathBuf};

pub fn render(
    entry: &ResourceEntry,
    snapshot_root: &Path,
    source_identity: &str,
    skills_root: &str,
    tool: Tool,
    artifacts: &mut Vec<Artifact>,
    _unsupported: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    let src_dir = snapshot_root.join(&entry.path);
    if !src_dir.is_dir() {
        return Err(Error::new(
            code::RENDER_FAILED,
            format!("技能目录不存在: {}", src_dir.display()),
        ));
    }
    let skill_rel = store::skill_rel_under_root(Path::new(&entry.path), Path::new(skills_root))?;
    let store_root = store::resolve_store_root()?;
    let entity = store::skill_entity_dir(&store_root, source_identity, &skill_rel);
    // 只计算期望摘要，不在 plan 阶段写 store（避免覆盖用户经软链的修改）
    let digest = store::dir_digest(&src_dir)?;

    let link_path: PathBuf = match tool {
        Tool::Claude => PathBuf::from(format!(".claude/skills/{}", entry.id.name)),
        Tool::Codex => PathBuf::from(format!("{CODEX_MANAGED_SKILL_PREFIX}{}", entry.id.name)),
        // alva 通过 co-load 直接读取 .claude/skills（paths.rs 核实），无需单独部署
        Tool::Alva => return Ok(()),
    };

    artifacts.push(Artifact {
        resource_id: entry.id.to_string(),
        target_tool: tool.as_str().into(),
        kind: "skill".into(),
        path: link_path,
        body: ArtifactBody::Symlink {
            target: entity,
            content_digest: digest,
            source_dir: src_dir,
            source_identity: source_identity.to_string(),
        },
    });
    Ok(())
}

/// 汇总 Codex skills.config 数组产物（条目 path 指向托管前缀下的链接目录）。
pub fn render_config(
    managed_skill_names: &[String],
    ws_root: &Path,
    artifacts: &mut Vec<Artifact>,
) -> Result<()> {
    if managed_skill_names.is_empty() {
        return Ok(());
    }
    let config_path = ws_root.join(".codex/config.toml");
    let mut user_entries: Vec<toml::Value> = Vec::new();
    if config_path.is_file() {
        let text = std::fs::read_to_string(&config_path)?;
        let value: toml::Value = text.parse().map_err(|e| {
            Error::new(
                code::USER_CONTENT_CONFLICT,
                format!(".codex/config.toml 解析失败（保留原文件）: {e}"),
            )
        })?;
        if let Some(arr) = value
            .get("skills")
            .and_then(|s| s.get("config"))
            .and_then(|c| c.as_array())
        {
            for item in arr {
                let path = item
                    .get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or_default();
                if !path.starts_with(CODEX_MANAGED_SKILL_PREFIX) {
                    user_entries.push(item.clone());
                }
            }
        }
    }
    let mut merged: Vec<toml::Value> = user_entries;
    for name in managed_skill_names {
        let mut item = toml::Value::Table(Default::default());
        let t = item.as_table_mut().unwrap();
        t.insert(
            "path".into(),
            toml::Value::String(format!("{CODEX_MANAGED_SKILL_PREFIX}{name}")),
        );
        t.insert("enabled".into(), toml::Value::Boolean(true));
        merged.push(item);
    }
    let mut root = toml::Value::Table(Default::default());
    let mut skills = toml::map::Map::new();
    skills.insert("config".into(), toml::Value::Array(merged));
    root.as_table_mut()
        .unwrap()
        .insert("skills".into(), toml::Value::Table(skills));
    // 与旧逻辑一致：整文件托管片段不够，仍用 Full 写合并后的 skills 表——
    // 这里保持原 render_config 的 Full 行为见下方；若文件还有其它键需保留则读改写。
    let content = if config_path.is_file() {
        let text = std::fs::read_to_string(&config_path)?;
        let mut existing: toml::Value = text
            .parse()
            .unwrap_or_else(|_| toml::Value::Table(Default::default()));
        let table = existing
            .as_table_mut()
            .ok_or_else(|| Error::new(code::USER_CONTENT_CONFLICT, "config.toml 根不是表"))?;
        table.insert(
            "skills".into(),
            root.get("skills")
                .cloned()
                .unwrap_or(toml::Value::Table(Default::default())),
        );
        toml::to_string_pretty(&existing)?
    } else {
        toml::to_string_pretty(&root)?
    };
    artifacts.push(Artifact {
        resource_id: "ailoom-builtin/codex-skills-config".into(),
        target_tool: "codex".into(),
        kind: "skill-config".into(),
        path: PathBuf::from(".codex/config.toml"),
        body: ArtifactBody::Full { content },
    });
    Ok(())
}
