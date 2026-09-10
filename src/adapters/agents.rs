//! 专用 Agent 适配（AIL-011）：统一 AgentSpec（TOML）→ 各宿主原生格式。
//! Claude Code：.claude/agents/<name>.md（frontmatter name/description/tools/model + 正文指令）。
//! Codex：项目级自定义 Agent 未在官方文档确认 → 显式 Unsupported（能力矩阵标 unknown）。

use super::common::{raw_string_field, resource_targets};
use super::{Tool, UnsupportedItem};
use crate::adapters::common::{Artifact, ArtifactBody};
use crate::error::{code, Error, Result};
use crate::resource::ResourceEntry;

/// 解析 AgentSpec 公共字段；tool_extras 仅透传给匹配工具。
pub struct AgentSpec {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub model: Option<String>,
    pub tools: Vec<String>,
}

pub fn parse_spec(entry: &ResourceEntry) -> Result<AgentSpec> {
    let text = entry.raw.as_deref().ok_or_else(|| {
        Error::new(
            code::RENDER_FAILED,
            format!("agent 资源缺少原文: {}", entry.id.name),
        )
    })?;
    let value: toml::Value = text
        .parse()
        .map_err(|e| Error::new(code::RENDER_FAILED, format!("agent TOML 解析失败: {e}")))?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&entry.id.name)
        .to_string();
    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let instructions = value
        .get("instructions")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            Error::new(
                code::RENDER_FAILED,
                format!("agent 缺少 instructions: {name}"),
            )
        })?
        .to_string();
    let model = value
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let tools = value
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(AgentSpec {
        name,
        description,
        instructions,
        model,
        tools,
    })
}

pub fn render(
    entry: &ResourceEntry,
    tool: Tool,
    artifacts: &mut Vec<Artifact>,
    unsupported: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    match tool {
        Tool::Claude => {
            let spec = parse_spec(entry)?;
            let mut fm = String::new();
            fm.push_str("---\n");
            fm.push_str(&format!("name: {}\n", spec.name));
            fm.push_str(&format!("description: {}\n", spec.description));
            if !spec.tools.is_empty() {
                fm.push_str(&format!("tools: {}\n", spec.tools.join(", ")));
            }
            if let Some(model) = &spec.model {
                fm.push_str(&format!("model: {model}\n"));
            }
            // tool_extras.claude 透传（团队须使用官方字段名）
            if let Some(text) = entry.raw.as_deref() {
                if let Ok(value) = toml::from_str::<toml::Value>(text) {
                    if let Some(extras) = value
                        .get("tool_extras")
                        .and_then(|t| t.get("claude"))
                        .and_then(|c| c.as_table())
                    {
                        for (k, v) in extras {
                            let vs = match v {
                                toml::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            fm.push_str(&format!("{k}: {vs}\n"));
                        }
                    }
                }
            }
            fm.push_str("---\n\n");
            fm.push_str(&spec.instructions);
            fm.push('\n');
            artifacts.push(Artifact {
                resource_id: entry.id.to_string(),
                target_tool: tool.as_str().into(),
                kind: "agent".into(),
                path: format!(".claude/agents/{}.md", entry.id.name).into(),
                body: ArtifactBody::Full { content: fm },
            });
            Ok(())
        }
        Tool::Codex => {
            // 未在官方文档确认项目级自定义 Agent → 显式不支持，不写用户级配置充数
            unsupported.push(UnsupportedItem {
                resource_id: entry.id.to_string(),
                tool: tool.as_str().into(),
                kind: "agent".into(),
                reason: "Codex 项目级自定义 Agent 未在官方文档确认（能力矩阵标 unknown）".into(),
            });
            Ok(())
        }
        Tool::Alva => {
            // alva 宿主由 alva_agents::render 在 mod 层直接分发（[[agent]] 数组条目）
            Ok(())
        }
    }
}

/// 诊断用：agent 是否声明了发布到该工具。
pub fn declared_targets(entry: &ResourceEntry) -> Option<Vec<String>> {
    resource_targets(entry.raw.as_deref())
}

#[allow(unused)]
fn model_field(entry: &ResourceEntry) -> Option<String> {
    raw_string_field(entry.raw.as_deref(), "model")
}
