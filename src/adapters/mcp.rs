//! MCP 适配（AIL-012）：stdio/http 服务声明 → 各宿主项目级配置。
//! Claude Code：.mcp.json `mcpServers.<name>`（stdio：command/args/env；http：type/url/headers，支持 ${VAR} 插值）。
//! Codex：项目级 .codex/config.toml `mcp_servers.<name>`（stdio：command/args/env；http：url）。
//! 秘密只存引用：Claude 用原生 ${VAR}；Codex 插值未在官方文档确认 → 拒绝写明文并显式报错。

use super::common::{Artifact, ArtifactBody};
use super::{Tool, UnsupportedItem};
use crate::error::{code, Error, Result};
use crate::resource::ResourceEntry;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum McpType {
    Stdio,
    Http,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: McpType,
    pub command: Option<String>,
    pub args: Vec<String>,
    /// 值为字面量或 "$ENV:NAME" 引用
    pub env: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub url: Option<String>,
}

pub fn parse_spec(entry: &ResourceEntry) -> Result<McpSpec> {
    let text = entry.raw.as_deref().ok_or_else(|| {
        Error::new(
            code::RENDER_FAILED,
            format!("mcp 资源缺少原文: {}", entry.id.name),
        )
    })?;
    let value: toml::Value = text
        .parse()
        .map_err(|e| Error::new(code::RENDER_FAILED, format!("mcp TOML 解析失败: {e}")))?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&entry.id.name)
        .to_string();
    let kind = match value.get("type").and_then(|v| v.as_str()) {
        Some("stdio") => McpType::Stdio,
        Some("http") => McpType::Http,
        other => {
            return Err(Error::new(
                code::RENDER_FAILED,
                format!("mcp `{name}` 的 type 非法: {}", other.unwrap_or("<缺失>")),
            ))
        }
    };
    let command = value
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let args = value
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    // env/headers 位于 [mcp] 子表（契约 §4.2），兼容顶层
    let pairs = |key: &str| -> Vec<(String, String)> {
        let nested = value.get("mcp").and_then(|m| m.get(key));
        let raw = nested.or_else(|| value.get(key));
        raw.and_then(|v| v.as_table())
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    };
    let env = pairs("env");
    let headers = pairs("headers");
    let url = value
        .get("url")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    // 连接类型校验：stdio 需要 command，http 需要 url
    match kind {
        McpType::Stdio if command.is_none() => {
            return Err(Error::new(
                code::RENDER_FAILED,
                format!("stdio 服务 `{name}` 缺少 command"),
            ))
        }
        McpType::Http if url.is_none() => {
            return Err(Error::new(
                code::RENDER_FAILED,
                format!("http 服务 `{name}` 缺少 url"),
            ))
        }
        _ => {}
    }
    Ok(McpSpec {
        name,
        kind,
        command,
        args,
        env,
        headers,
        url,
    })
}

fn is_secret_ref(v: &str) -> bool {
    v.starts_with("$ENV:")
}

/// 缺失的引用环境变量清单（用于诊断，不输出值）。
pub fn missing_env_refs(spec: &McpSpec) -> Vec<String> {
    let mut missing = Vec::new();
    for (k, v) in spec.env.iter().chain(spec.headers.iter()) {
        if let Some(name) = v.strip_prefix("$ENV:") {
            if std::env::var(name).is_err() {
                missing.push(format!("{k} -> {name}"));
            }
        }
    }
    missing
}

pub fn render(
    entry: &ResourceEntry,
    tool: Tool,
    artifacts: &mut Vec<Artifact>,
    unsupported: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    let spec = parse_spec(entry)?;
    match tool {
        // alva co-load .mcp.json：claude 适配器已部署同一文件，无需重复产物
        Tool::Alva => Ok(()),
        Tool::Claude => {
            let mut server = serde_json::Map::new();
            match spec.kind {
                McpType::Stdio => {
                    server.insert(
                        "command".into(),
                        serde_json::json!(spec.command.clone().unwrap_or_default()),
                    );
                    if !spec.args.is_empty() {
                        server.insert("args".into(), serde_json::json!(spec.args));
                    }
                    if !spec.env.is_empty() {
                        // $ENV:NAME → 原生 ${NAME} 插值（官方支持）
                        let env: serde_json::Map<String, serde_json::Value> = spec
                            .env
                            .iter()
                            .map(|(k, v)| {
                                let rendered = match v.strip_prefix("$ENV:") {
                                    Some(name) => format!("${{{name}}}"),
                                    None => v.clone(),
                                };
                                (k.clone(), serde_json::json!(rendered))
                            })
                            .collect();
                        server.insert("env".into(), serde_json::Value::Object(env));
                    }
                }
                McpType::Http => {
                    server.insert("type".into(), serde_json::json!("http"));
                    server.insert(
                        "url".into(),
                        serde_json::json!(spec.url.clone().unwrap_or_default()),
                    );
                    if !spec.headers.is_empty() {
                        let headers: serde_json::Map<String, serde_json::Value> = spec
                            .headers
                            .iter()
                            .map(|(k, v)| {
                                let rendered = match v.strip_prefix("$ENV:") {
                                    Some(name) => format!("${{{name}}}"),
                                    None => v.clone(),
                                };
                                (k.clone(), serde_json::json!(rendered))
                            })
                            .collect();
                        server.insert("headers".into(), serde_json::Value::Object(headers));
                    }
                }
            }
            let pointer = format!("/mcpServers/{}", spec.name);
            artifacts.push(Artifact {
                resource_id: entry.id.to_string(),
                target_tool: tool.as_str().into(),
                kind: "mcp".into(),
                path: ".mcp.json".into(),
                body: ArtifactBody::JsonPointer {
                    pointer,
                    value: serde_json::Value::Object(server),
                },
            });
            Ok(())
        }
        Tool::Codex => {
            // 秘密引用：Codex 配置的插值语义未在官方文档确认 → 拒绝写明文
            let secret_refs: Vec<&(String, String)> = spec
                .env
                .iter()
                .chain(spec.headers.iter())
                .filter(|(_, v)| is_secret_ref(v))
                .collect();
            if !secret_refs.is_empty() {
                let keys: Vec<String> = secret_refs.iter().map(|(k, _)| k.clone()).collect();
                unsupported.push(UnsupportedItem {
                    resource_id: entry.id.to_string(),
                    tool: tool.as_str().into(),
                    kind: "mcp".into(),
                    reason: format!(
                        "Codex 配置环境变量插值未在官方文档确认，拒绝写入秘密引用条目: {keys:?}（不落明文）"
                    ),
                });
                return Ok(());
            }
            let mut table = toml::Value::Table(Default::default());
            {
                let t = table.as_table_mut().unwrap();
                match spec.kind {
                    McpType::Stdio => {
                        t.insert(
                            "command".into(),
                            toml::Value::String(spec.command.clone().unwrap_or_default()),
                        );
                        if !spec.args.is_empty() {
                            t.insert(
                                "args".into(),
                                toml::Value::Array(
                                    spec.args
                                        .iter()
                                        .map(|a| toml::Value::String(a.clone()))
                                        .collect(),
                                ),
                            );
                        }
                        if !spec.env.is_empty() {
                            let mut env = toml::Value::Table(Default::default());
                            for (k, v) in &spec.env {
                                env.as_table_mut()
                                    .unwrap()
                                    .insert(k.clone(), toml::Value::String(v.clone()));
                            }
                            t.insert("env".into(), env);
                        }
                    }
                    McpType::Http => {
                        t.insert(
                            "url".into(),
                            toml::Value::String(spec.url.clone().unwrap_or_default()),
                        );
                        if !spec.headers.is_empty() {
                            let mut headers = toml::Value::Table(Default::default());
                            for (k, v) in &spec.headers {
                                headers
                                    .as_table_mut()
                                    .unwrap()
                                    .insert(k.clone(), toml::Value::String(v.clone()));
                            }
                            t.insert("headers".into(), headers);
                        }
                    }
                }
            }
            let table_path = format!("mcp_servers.{}", spec.name);
            artifacts.push(Artifact {
                resource_id: entry.id.to_string(),
                target_tool: tool.as_str().into(),
                kind: "mcp".into(),
                path: ".codex/config.toml".into(),
                body: ArtifactBody::TomlTable {
                    table: table_path,
                    value: table,
                },
            });
            Ok(())
        }
    }
}
