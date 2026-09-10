//! 规则适配（AIL-010）：Claude Code 走原生 `.claude/rules/<name>.md`（支持 paths 条件）；
//! Codex 走 AGENTS.md 受管片段（不支持条件语义 → 显式 Unsupported）。

use super::common::{upsert_fragment, Artifact, ArtifactBody};
use super::{Tool, UnsupportedItem};
use crate::resource::ResourceEntry;
use std::path::Path;

/// 规则是否声明了条件（frontmatter `paths`）。
fn conditional(raw: Option<&str>) -> bool {
    let Some(text) = raw else { return false };
    let Some(rest) = text.strip_prefix("---\n") else {
        return false;
    };
    let Some(yaml) = rest.split("\n---").next() else {
        return false;
    };
    serde_yaml::from_str::<serde_yaml::Value>(yaml)
        .ok()
        .and_then(|v| v.get("paths").map(|_| true))
        .unwrap_or(false)
}

/// 渲染后的规则正文（去掉 AILoom 元数据仍保留 paths 供 Claude 原生解析）。
fn body_for_claude(entry: &ResourceEntry) -> String {
    entry.raw.clone().unwrap_or_default()
}

pub fn render(
    entry: &ResourceEntry,
    snapshot_root: &Path,
    tool: Tool,
    artifacts: &mut Vec<Artifact>,
    unsupported: &mut Vec<UnsupportedItem>,
) {
    match tool {
        Tool::Claude => {
            // 原生规则文件：整文件产物，frontmatter 原样保留（paths 条件由宿主解析）
            artifacts.push(Artifact {
                resource_id: entry.id.to_string(),
                target_tool: tool.as_str().into(),
                kind: "rule".into(),
                path: format!(".claude/rules/{}.md", entry.id.name).into(),
                body: ArtifactBody::Full {
                    content: body_for_claude(entry),
                },
            });
        }
        Tool::Alva => {
            unsupported.push(UnsupportedItem {
                resource_id: entry.id.to_string(),
                tool: tool.as_str().into(),
                kind: "rule".into(),
                reason: "alva 的 AGENTS.md 由项目手工维护（single source of truth），AILoom 不注入（skills-first 决策）".into(),
            });
        }
        Tool::Codex => {
            if conditional(entry.raw.as_deref()) {
                unsupported.push(UnsupportedItem {
                    resource_id: entry.id.to_string(),
                    tool: tool.as_str().into(),
                    kind: "rule".into(),
                    reason: "Codex 的 AGENTS.md 片段无 path 条件语义，不支持条件规则".into(),
                });
                return;
            }
            // AGENTS.md 受管片段：只主张自己的片段
            let content = strip_frontmatter(entry.raw.as_deref().unwrap_or_default());
            artifacts.push(Artifact {
                resource_id: entry.id.to_string(),
                target_tool: tool.as_str().into(),
                kind: "rule".into(),
                path: "AGENTS.md".into(),
                body: ArtifactBody::Fragment {
                    content: format!("### 规则：{}\n\n{}", entry.id.name, content.trim()),
                },
            });
        }
    }
    let _ = snapshot_root;
}

pub fn strip_frontmatter_content(text: &str) -> String {
    strip_frontmatter(text)
}

fn strip_frontmatter(text: &str) -> String {
    match text.strip_prefix("---\n") {
        Some(rest) => match rest.find("\n---") {
            Some(end) => rest[end + 4..].trim_start_matches('\n').to_string(),
            None => text.to_string(),
        },
        None => text.to_string(),
    }
}

/// 供 sync 命令渲染 AGENTS.md 片段（同机制复用）。
pub fn fragment_for(resource_id: &str, content: &str) -> String {
    upsert_fragment("", resource_id, content)
}
