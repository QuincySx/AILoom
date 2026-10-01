//! Additional native hosts. Sources and limits: docs/capabilities/extra-hosts.md.
use super::common::{Artifact, ArtifactBody};
use super::{agents, mcp, skills, UnsupportedItem};
use crate::error::Result;
use crate::resolver::DesiredSet;
use crate::resource::{ResourceEntry, ResourceKind};
use std::path::Path;

pub struct Host {
    pub id: &'static str,
    pub label: &'static str,
    pub directory: &'static str,
    pub skills: &'static str,
    pub agents: Option<&'static str>,
}

pub const HOSTS: &[Host] = &[
    Host {
        id: "grok",
        label: "Grok",
        directory: ".grok",
        skills: ".grok/skills",
        agents: Some(".grok/agents"),
    },
    Host {
        id: "pi",
        label: "Pi",
        directory: ".pi",
        skills: ".pi/skills",
        agents: Some(".pi/agents"),
    },
    Host {
        id: "opencode",
        label: "OpenCode",
        directory: ".opencode",
        skills: ".opencode/skills",
        agents: Some(".opencode/agents"),
    },
    Host {
        id: "cursor",
        label: "Cursor",
        directory: ".cursor",
        skills: ".cursor/skills",
        agents: Some(".cursor/agents"),
    },
];

pub fn lookup(id: &str) -> Option<&'static Host> {
    HOSTS.iter().find(|h| h.id == id)
}

fn unsupported(
    host: &Host,
    entry: &ResourceEntry,
    reason: impl Into<String>,
    out: &mut Vec<UnsupportedItem>,
) {
    out.push(UnsupportedItem {
        resource_id: entry.id.to_string(),
        tool: host.id.into(),
        kind: entry.id.kind.as_str().into(),
        reason: reason.into(),
    });
}

pub fn render(
    host: &Host,
    entry: &ResourceEntry,
    desired: &DesiredSet,
    snapshot: &Path,
    root: &Path,
    out: &mut Vec<Artifact>,
    missing: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    match entry.id.kind {
        ResourceKind::Skill => skills::render_at(
            entry,
            snapshot,
            &desired.identity,
            &desired.skills_root,
            host.id,
            format!("{}/{}", host.skills, entry.id.name).into(),
            out,
        )?,
        ResourceKind::Agent => render_agent(host, entry, out, missing)?,
        ResourceKind::Mcp => render_mcp(host, entry, root, out, missing)?,
        ResourceKind::Rule => {
            if host.id == "cursor" {
                super::registry::render_rules(
                    entry,
                    super::registry::lookup("cursor").unwrap(),
                    out,
                )?;
            } else if super::rules::conditional(entry.raw.as_deref()) {
                unsupported(
                    host,
                    entry,
                    "此工具暂不支持转换带 paths 条件的规则",
                    missing,
                );
            } else {
                out.extend(instructions(
                    host.id,
                    &entry.id.to_string(),
                    &super::rules::strip_frontmatter_content(
                        entry.raw.as_deref().unwrap_or_default(),
                    ),
                    root,
                )?);
            }
        }
        ResourceKind::Learning => {}
        _ => unsupported(
            host,
            entry,
            format!("{} 暂未适配这类资源", host.label),
            missing,
        ),
    }
    Ok(())
}

fn render_agent(
    host: &Host,
    entry: &ResourceEntry,
    out: &mut Vec<Artifact>,
    missing: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    let Some(dir) = host.agents else {
        unsupported(
            host,
            entry,
            "Pi 子代理需要 subagent 扩展；原生 Pi 不会加载 Agent 配置",
            missing,
        );
        return Ok(());
    };
    let spec = agents::parse_spec(entry)?;
    let raw: toml::Value = entry.raw.as_deref().unwrap_or_default().parse()?;
    let extras = raw.get("tool_extras").and_then(|v| v.get(host.id));
    let mut fm = serde_json::Map::new();
    fm.insert("name".into(), spec.name.into());
    fm.insert("description".into(), spec.description.into());
    // Model IDs and tool names belong to the host. Never translate a Claude model alias by guessing.
    let single_target =
        super::common::resource_targets(entry.raw.as_deref()).is_some_and(|v| v == [host.id]);
    if spec.model.is_some() && extras.and_then(|v| v.get("model")).is_none() && !single_target {
        unsupported(
            host,
            entry,
            format!(
                "请在 tool_extras.{}.model 指定此工具的模型，或将该配置限定为 targets = [\"{}\"]",
                host.id, host.id
            ),
            missing,
        );
        return Ok(());
    }
    if let Some(model) = spec.model {
        fm.insert("model".into(), model.into());
    }
    if !spec.tools.is_empty() {
        match host.id {
            "grok" | "pi" if single_target || extras.and_then(|v| v.get("tools")).is_some() => {
                fm.insert("tools".into(), serde_json::json!(spec.tools));
            }
            "opencode" if extras.and_then(|v| v.get("permission")).is_some() => {}
            _ => {
                unsupported(
                    host,
                    entry,
                    format!(
                        "{} 无法直接使用这份工具列表；请提供该工具专用的代理配置",
                        host.label
                    ),
                    missing,
                );
                return Ok(());
            }
        }
    }
    if host.id == "opencode" {
        fm.insert("mode".into(), "subagent".into());
    }
    if let Some(values) = extras.and_then(|v| v.as_table()) {
        for (key, value) in values {
            fm.insert(key.clone(), serde_json::to_value(value)?);
        }
    }
    let yaml = serde_yaml::to_string(&fm)?;
    out.push(Artifact {
        resource_id: entry.id.to_string(),
        target_tool: host.id.into(),
        kind: "agent".into(),
        path: format!("{dir}/{}.md", entry.id.name).into(),
        body: ArtifactBody::Full {
            content: format!(
                "---\n{}---\n\n{}\n",
                yaml.trim_start_matches("---\n"),
                spec.instructions
            ),
        },
    });
    Ok(())
}

fn interpolation(tool: &str, value: &str) -> String {
    match value.strip_prefix("$ENV:") {
        Some(name) => match tool {
            "cursor" => format!("${{env:{name}}}"),
            "opencode" => format!("{{env:{name}}}"),
            _ => format!("${{{name}}}"),
        },
        None => value.into(),
    }
}
fn pointer_key(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

fn render_mcp(
    host: &Host,
    entry: &ResourceEntry,
    root: &Path,
    out: &mut Vec<Artifact>,
    missing: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    if host.id == "pi" {
        unsupported(
            host,
            entry,
            "Pi 原生没有 MCP 配置入口，需要通过 MCP 扩展接入",
            missing,
        );
        return Ok(());
    }
    let spec = mcp::parse_spec(entry)?;
    let pairs = |items: &[(String, String)]| -> serde_json::Value {
        items
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    serde_json::Value::String(interpolation(host.id, v)),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    };
    let mut server = serde_json::Map::new();
    let is_local = spec.kind == mcp::McpType::Stdio;
    if host.id == "opencode" {
        server.insert(
            "type".into(),
            if is_local { "local" } else { "remote" }.into(),
        );
        server.insert("enabled".into(), true.into());
    } else if host.id == "cursor" && is_local {
        server.insert("type".into(), "stdio".into());
    }
    if is_local {
        let cmd = interpolation(host.id, spec.command.as_deref().unwrap());
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|v| interpolation(host.id, v))
            .collect();
        if host.id == "opencode" {
            server.insert(
                "command".into(),
                serde_json::json!(std::iter::once(cmd).chain(args).collect::<Vec<_>>()),
            );
        } else {
            server.insert("command".into(), cmd.into());
            server.insert("args".into(), serde_json::json!(args));
        }
        if !spec.env.is_empty() {
            server.insert(
                if host.id == "opencode" {
                    "environment"
                } else {
                    "env"
                }
                .into(),
                pairs(&spec.env),
            );
        }
    } else {
        server.insert(
            "url".into(),
            interpolation(host.id, spec.url.as_deref().unwrap()).into(),
        );
        if !spec.headers.is_empty() {
            server.insert("headers".into(), pairs(&spec.headers));
        }
    }
    let (path, body) = if host.id == "grok" {
        // The TOML path utility splits on dots: reject names that cannot be represented safely.
        if spec.name.contains('.') {
            unsupported(
                host,
                entry,
                "Grok MCP 名称暂不支持句点，请重命名服务",
                missing,
            );
            return Ok(());
        }
        let value: toml::Value = serde_json::from_value(serde_json::Value::Object(server))?;
        (
            ".grok/config.toml",
            ArtifactBody::TomlTable {
                table: format!("mcp_servers.{}", spec.name),
                value,
            },
        )
    } else {
        if host.id == "opencode" && root.join(".opencode/opencode.jsonc").exists() {
            unsupported(
                host,
                entry,
                "项目已有 .opencode/opencode.jsonc；暂不自动改写此带注释配置",
                missing,
            );
            return Ok(());
        }
        let (path, key) = if host.id == "cursor" {
            (".cursor/mcp.json", "mcpServers")
        } else {
            (".opencode/opencode.json", "mcp")
        };
        (
            path,
            ArtifactBody::JsonPointer {
                pointer: format!("/{key}/{}", pointer_key(&spec.name)),
                value: server.into(),
            },
        )
    };
    out.push(Artifact {
        resource_id: entry.id.to_string(),
        target_tool: host.id.into(),
        kind: "mcp".into(),
        path: path.into(),
        body,
    });
    Ok(())
}

/// Native additive instruction entry points; never manufacture a CLAUDE.md.
pub fn instructions(
    tool: &str,
    resource: &str,
    content: &str,
    root: &Path,
) -> Result<Vec<Artifact>> {
    let mut out = Vec::new();
    let path = match tool {
        "cursor" => ".cursor/rules/ailoom-personal.mdc",
        "grok" => ".grok/rules/ailoom-personal.md",
        "pi" => ".pi/APPEND_SYSTEM.md",
        "opencode" => "AGENTS.md",
        _ => return Ok(out),
    };
    // OpenCode's AGENTS.md would shadow a pre-existing CLAUDE.md. Keep that baseline in view.
    let content = if tool == "opencode"
        && !root.join("AGENTS.md").exists()
        && root.join("CLAUDE.md").is_file()
    {
        format!(
            "{}\n\n{}",
            std::fs::read_to_string(root.join("CLAUDE.md"))?,
            content
        )
    } else {
        content.into()
    };
    let body = if tool == "cursor" {
        ArtifactBody::Full { content: format!("---\ndescription: AILoom project instructions\nalwaysApply: true\n---\n\n{content}\n") }
    } else {
        ArtifactBody::Fragment { content }
    };
    out.push(Artifact {
        resource_id: resource.into(),
        target_tool: tool.into(),
        kind: "rule".into(),
        path: path.into(),
        body,
    });
    Ok(out)
}
