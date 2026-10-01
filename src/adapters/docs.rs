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
    // 文档对两工具使用同一受管副本；索引片段由 render_index 统一生成。
    // 副本每 entry 只生成一次：render 按工具循环调用，重复推送会因
    // item_key 相同产生两个同路径 Create action，第二个必然前置失败。
    let copy_path = format!(".ailoom/docs/{}.md", entry.id.name);
    if artifacts
        .iter()
        .any(|a| a.kind == "doc" && a.path == std::path::Path::new(&copy_path))
    {
        return;
    }
    let content = entry.raw.clone().unwrap_or_default();
    artifacts.push(Artifact {
        resource_id: entry.id.to_string(),
        target_tool: "*".into(),
        kind: "doc".into(),
        path: copy_path.into(),
        body: ArtifactBody::Full { content },
    });
}

/// 生成入口文件受管索引片段（有文档时每工具一个）。
pub fn render_index(
    desired: &DesiredSet,
    snapshot_root: &Path,
    targets: &ToolTargets,
    artifacts: &mut Vec<Artifact>,
    ws_root: &Path,
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
            Tool::Claude => claude_entry_file(ws_root),
            Tool::Codex => "AGENTS.md",
            Tool::Alva => continue, // alva 文档索引无处安放（不碰其 AGENTS.md），skills-first
        };
        if let Some(existing) = artifacts
            .iter_mut()
            .find(|a| a.resource_id == rid && a.path == Path::new(entry_file))
        {
            existing.target_tool = "*".into();
            continue;
        }
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

/// Default Claude 2.1.277+ fallback. Do not create a CLAUDE.md that shadows AGENTS.md.
pub fn claude_entry_file(root: &Path) -> &'static str {
    if root.join("CLAUDE.md").is_file() {
        return "CLAUDE.md";
    }
    if root.join(".claude/CLAUDE.md").is_file() {
        return ".claude/CLAUDE.md";
    }
    if root.ancestors().any(|dir| {
        ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"]
            .iter()
            .any(|file| dir.join(file).is_file())
    }) {
        "CLAUDE.md"
    } else if root.join(".claude/AGENTS.md").is_file() && !root.join("AGENTS.md").is_file() {
        ".claude/AGENTS.md"
    } else {
        "AGENTS.md"
    }
}
