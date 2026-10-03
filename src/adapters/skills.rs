//! Skills 适配（AIL-009 + ADR-0001；AIL-041 复核）：实体进 SkillStore，Workspace 只挂 symlink。
//! Claude：`.claude/skills/<name>` → store；
//! Codex：`.agents/skills/<name>` → store（官方原生项目技能目录，支持 symlink、自动发现），
//! 并在 `.codex/config.toml` skills.config 保留显式条目（path 指向 SKILL.md，可禁用）。
//! 旧版 `.ailoom/skills/<name>` 部署由 sync 的过期清理迁移（旧条目 Delete → 新条目 Create）。

use super::common::{Artifact, ArtifactBody, CODEX_NATIVE_SKILLS_DIR};
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
    let link_path: PathBuf = match tool {
        Tool::Claude => PathBuf::from(format!(".claude/skills/{}", entry.id.name)),
        // AIL-041：官方原生项目技能目录（symlink 可被宿主扫描发现）
        Tool::Codex => PathBuf::from(format!("{CODEX_NATIVE_SKILLS_DIR}/{}", entry.id.name)),
        // alva 通过 co-load 直接读取 .claude/skills（paths.rs 核实），无需单独部署
        Tool::Alva => return Ok(()),
    };

    render_at(
        entry,
        snapshot_root,
        source_identity,
        skills_root,
        tool.as_str(),
        link_path,
        artifacts,
    )
}

pub fn render_at(
    entry: &ResourceEntry,
    snapshot_root: &Path,
    source_identity: &str,
    skills_root: &str,
    tool: &str,
    link_path: PathBuf,
    artifacts: &mut Vec<Artifact>,
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
    // 只计算期望摘要，不在 plan 阶段写 store（避免覆盖用户经软链的修改）
    let digest = store::dir_digest(&src_dir)?;
    let source_identity = store::skill_revision_identity(source_identity, &digest);
    let entity = store::skill_entity_dir(&store_root, &source_identity, &skill_rel);

    artifacts.push(Artifact {
        resource_id: entry.id.to_string(),
        target_tool: tool.into(),
        kind: "skill".into(),
        path: link_path,
        body: ArtifactBody::Symlink {
            target: entity,
            content_digest: digest,
            source_dir: src_dir,
            source_identity,
        },
    });
    Ok(())
}

/// 汇总 Codex skills.config 数组产物（条目 path 指向原生发现目录下的 SKILL.md，
/// 形态与官方 skills.config 示例一致；AILoom 托管旧条目 `.ailoom/skills/…` 一并清除迁移）。
pub fn render_config(
    managed_skill_names: &[String],
    _ws_root: &Path,
    artifacts: &mut Vec<Artifact>,
) -> Result<()> {
    for name in managed_skill_names {
        let mut item = toml::Value::Table(Default::default());
        let t = item.as_table_mut().unwrap();
        t.insert(
            "path".into(),
            toml::Value::String(format!("{CODEX_NATIVE_SKILLS_DIR}/{name}/SKILL.md")),
        );
        t.insert("enabled".into(), toml::Value::Boolean(true));
        artifacts.push(Artifact {
            resource_id: "ailoom-builtin/codex-skills-config".into(),
            target_tool: "codex".into(),
            kind: "skill-config".into(),
            path: PathBuf::from(".codex/config.toml"),
            body: ArtifactBody::TomlArrayEntry {
                table: "skills.config".into(),
                key_field: "path".into(),
                entry: item,
            },
        });
    }
    Ok(())
}
