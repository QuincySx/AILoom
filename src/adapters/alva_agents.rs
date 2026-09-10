//! alva 宿主适配（T11）：agent 资源渲染到 `.alva/agents.toml` 的 `[[agent]]` 数组。
//! alva schema（源码核实 2026-09-11，alva-agent/.alva/agents.toml 与 agent_templates.rs）：
//! name (req) / description (req) / system_prompt_base (req) / allowed_tools (opt) /
//! max_iterations (opt)。无 per-agent model 字段——AIL-011 的 model 仅 inherit 时可渲染。
//! 身份叠加机制 = alva 原生按 name overlay，AILoom 条目名全局唯一即无冲突。

use super::common::{Artifact, ArtifactBody};
use super::{Tool, UnsupportedItem};
use crate::error::{code, Error, Result};
use crate::resource::ResourceEntry;

pub const ALVA_AGENTS_FILE: &str = ".alva/agents.toml";
const ALVA_TABLE: &str = "agent";
const ALVA_KEY_FIELD: &str = "name";

pub fn render(
    entry: &ResourceEntry,
    _tool: Tool,
    artifacts: &mut Vec<Artifact>,
    unsupported: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    let _ = _tool;
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
    let tools: Vec<String> = value
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // alva 无 per-agent model 字段：model != inherit 时显式报告降级
    if let Some(m) = &model {
        if m != "inherit" {
            unsupported.push(UnsupportedItem {
                resource_id: format!("{}#model", entry.id),
                tool: "alva".into(),
                kind: "agent".into(),
                reason: format!(
                    "alva 子 agent 暂不支持独立 model（当前值 {m}），子 agent 将继承父模型"
                ),
            });
        }
    }

    let mut agent_entry = toml::Value::Table(Default::default());
    {
        let t = agent_entry.as_table_mut().unwrap();
        t.insert("name".into(), toml::Value::String(name));
        t.insert("description".into(), toml::Value::String(description));
        t.insert(
            "system_prompt_base".into(),
            toml::Value::String(instructions),
        );
        if !tools.is_empty() {
            t.insert(
                "allowed_tools".into(),
                toml::Value::Array(tools.into_iter().map(toml::Value::String).collect()),
            );
        }
    }

    artifacts.push(Artifact {
        resource_id: entry.id.to_string(),
        target_tool: "alva".into(),
        kind: "agent".into(),
        path: ALVA_AGENTS_FILE.into(),
        body: ArtifactBody::TomlArrayEntry {
            table: ALVA_TABLE.into(),
            key_field: ALVA_KEY_FIELD.into(),
            entry: agent_entry,
        },
    });
    Ok(())
}
