//! 文档适配（AIL-010）：受管副本落 `.ailoom/docs/`（可提交、无机器路径），
//! 两个宿主的入口文件（CLAUDE.md / AGENTS.md）只放受管索引片段（不内联全文）。

use super::common::{Artifact, ArtifactBody};
use super::{Tool, ToolTargets};
use crate::resolver::DesiredSet;
use crate::resource::ResourceKind;
use std::path::Path;

pub fn render(
    entry: &crate::resource::ResourceEntry,
    _snapshot_root: &Path,
    _tool: Tool,
    artifacts: &mut Vec<Artifact>,
) {
    // 文档对两工具使用同一受管副本；索引片段由 render_index 统一生成
    let content = entry.raw.clone().unwrap_or_default();
    artifacts.push(Artifact {
        resource_id: entry.id.to_string(),
        target_tool: "*".into(),
        kind: "doc".into(),
        path: format!(".ailoom/docs/{}.md", entry.id.name).into(),
        body: ArtifactBody::Full { content },
    });
}

/// 生成入口文件受管索引片段（有文档时每工具一个）。
pub fn render_index(
    desired: &DesiredSet,
    snapshot_root: &Path,
    targets: &ToolTargets,
    artifacts: &mut Vec<Artifact>,
) -> crate::error::Result<()> {
    let _ = snapshot_root;
    let docs: Vec<&crate::resolver::Selected> = desired
        .deployable()
        .iter()
        .filter(|s| s.entry.id.kind == ResourceKind::Doc)
        .copied()
        .collect();
    if docs.is_empty() {
        return Ok(());
    }
    let mut lines = String::from("### 团队文档索引（AILoom 管理，按需阅读）\n\n");
    for d in &docs {
        lines.push_str(&format!(
            "- [{}](.ailoom/docs/{}.md)\n",
            d.entry.description, d.entry.id.name
        ));
    }
    let rid = "ailoom-internal/doc-index";
    for tool in targets.iter() {
        let entry_file = match tool {
            Tool::Claude => "CLAUDE.md",
            Tool::Codex => "AGENTS.md",
            Tool::Alva => continue, // alva 文档索引无处安放（不碰其 AGENTS.md），skills-first
        };
        artifacts.push(Artifact {
            resource_id: rid.into(),
            target_tool: tool.as_str().into(),
            kind: "doc".into(),
            path: entry_file.into(),
            body: ArtifactBody::Fragment {
                content: lines.clone(),
            },
        });
    }
    Ok(())
}
