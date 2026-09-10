//! 环境配置适配（AIL-031）：字面量与秘密引用分离；按宿主原生能力渲染。
//! Claude Code：settings.json `env`（字面量注入为原生能力）；
//! 秘密引用（$ENV:NAME）：settings env 值不做变量展开（未核实）→ 拒绝写明文 → 显式 Unsupported。
//! Codex：项目级环境注入未核实 → 显式 Unsupported。

use super::common::{Artifact, ArtifactBody};
use super::{Tool, UnsupportedItem};
use crate::error::{code, Error, Result};
use crate::resource::ResourceEntry;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct EnvSpec {
    pub name: String,
    /// 字面量变量
    pub vars: Vec<(String, String)>,
    /// 秘密引用：NAME -> $ENV:REF
    pub secret_refs: Vec<(String, String)>,
}

pub fn parse_spec(entry: &ResourceEntry) -> Result<EnvSpec> {
    let text = entry.raw.as_deref().ok_or_else(|| {
        Error::new(
            code::RENDER_FAILED,
            format!("env 资源缺少原文: {}", entry.id.name),
        )
    })?;
    let value: toml::Value = text
        .parse()
        .map_err(|e| Error::new(code::RENDER_FAILED, format!("env TOML 解析失败: {e}")))?;
    let pairs = |key: &str| -> Vec<(String, String)> {
        value
            .get(key)
            .and_then(|v| v.as_table())
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(EnvSpec {
        name: entry.id.name.clone(),
        vars: pairs("vars"),
        secret_refs: pairs("secret_refs"),
    })
}

pub fn render(
    entry: &ResourceEntry,
    snapshot_root: &Path,
    tool: Tool,
    artifacts: &mut Vec<Artifact>,
    unsupported: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    let _ = snapshot_root;
    let spec = parse_spec(entry)?;
    match tool {
        Tool::Claude => {
            // 字面量变量：settings.json /env/<NAME>（条目粒度，保留他人条目）
            for (k, v) in &spec.vars {
                artifacts.push(Artifact {
                    resource_id: format!("{}#{}", entry.id, k),
                    target_tool: tool.as_str().into(),
                    kind: "env".into(),
                    path: ".claude/settings.json".into(),
                    body: ArtifactBody::JsonPointer {
                        pointer: format!("/env/{}", json_escape_pointer_key(k)),
                        value: serde_json::json!(v),
                    },
                });
            }
            // 秘密引用：settings env 不做插值（未核实）→ 拒绝写明文
            for (k, _) in &spec.secret_refs {
                unsupported.push(UnsupportedItem {
                    resource_id: format!("{}#{}", entry.id, k),
                    tool: tool.as_str().into(),
                    kind: "env".into(),
                    reason: format!(
                        "变量 {k} 为秘密引用：Claude settings env 未核实插值能力，拒绝写明文"
                    ),
                });
            }
            Ok(())
        }
        Tool::Codex => {
            // 项目级环境注入未核实 → 全部显式 unsupported
            for (k, _) in spec.vars.iter().chain(spec.secret_refs.iter()) {
                unsupported.push(UnsupportedItem {
                    resource_id: format!("{}#{}", entry.id, k),
                    tool: tool.as_str().into(),
                    kind: "env".into(),
                    reason: format!(
                        "Codex 项目级环境注入未核实（能力矩阵 unknown），变量 {k} 不写入"
                    ),
                });
            }
            Ok(())
        }
        Tool::Alva => {
            for (k, _) in spec.vars.iter().chain(spec.secret_refs.iter()) {
                unsupported.push(UnsupportedItem {
                    resource_id: format!("{}#{}", entry.id, k),
                    tool: tool.as_str().into(),
                    kind: "env".into(),
                    reason: format!(
                        "alva 项目级环境注入未核实（能力矩阵 unknown），变量 {k} 不写入"
                    ),
                });
            }
            Ok(())
        }
    }
}

/// JSON pointer 转义（~ → ~0，/ → ~1）。
pub fn json_escape_pointer_key(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
