//! 团队自定义 Hook 适配（AIL-032）：源仓库 hooks/*.toml → Claude settings 托管注册。
//! 与 AIL-018 内置观测 Hook 分开归属；执行经 `ailoom hooks exec` 包装（结构化参数、无 shell、超时回收）。

use super::common::{Artifact, ArtifactBody};
use super::{Tool, UnsupportedItem};
use crate::error::{code, Error, Result};
use crate::resource::ResourceEntry;
use std::path::{Path, PathBuf};

pub const TEAM_HOOK_EXEC_PREFIX: &str = "ailoom hooks exec --id ";

#[derive(Debug, Clone)]
pub struct TeamHookSpec {
    pub name: String,
    pub event: String,
    pub matcher: Option<String>,
    pub command: Vec<String>,
    pub timeout_ms: u64,
}

pub fn parse_spec(entry: &ResourceEntry) -> Result<TeamHookSpec> {
    let text = entry.raw.as_deref().ok_or_else(|| {
        Error::new(
            code::RENDER_FAILED,
            format!("hook 资源缺少原文: {}", entry.id.name),
        )
    })?;
    let value: toml::Value = text
        .parse()
        .map_err(|e| Error::new(code::RENDER_FAILED, format!("hook TOML 解析失败: {e}")))?;
    let event = value.get("event").and_then(|v| v.as_str()).ok_or_else(|| {
        Error::new(
            code::RENDER_FAILED,
            format!("hook 缺 event: {}", entry.id.name),
        )
    })?;
    // 仅适配宿主支持事件
    if !crate::events::hooks::CLAUDE_HOOK_EVENTS.contains(&event) {
        return Err(Error::new(
            code::HOST_UNSUPPORTED,
            format!(
                "hook 事件 `{event}` 不受支持（Claude 支持 {:?}）",
                crate::events::hooks::CLAUDE_HOOK_EVENTS
            ),
        ));
    }
    let command: Vec<String> = value
        .get("command")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .ok_or_else(|| {
            Error::new(
                code::RENDER_FAILED,
                format!("hook 缺 command 数组: {}", entry.id.name),
            )
        })?;
    if command.is_empty() {
        return Err(Error::new(
            code::RENDER_FAILED,
            format!("hook command 为空: {}", entry.id.name),
        ));
    }
    Ok(TeamHookSpec {
        name: entry.id.name.clone(),
        event: event.to_string(),
        matcher: value
            .get("matcher")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        timeout_ms: value
            .get("timeout_ms")
            .and_then(|v| v.as_integer())
            .unwrap_or(5000) as u64,
        command,
    })
}

/// 渲染注册条目：command = `ailoom hooks exec --id <resource_id>`（结构化参数存储在 hook-specs，
/// 执行时无 shell 解释）；注册条目以 exec 前缀作为托管签名，与用户 Hook 分离。
pub fn render(
    entry: &ResourceEntry,
    _ws_root: &Path,
    tool: Tool,
    artifacts: &mut Vec<Artifact>,
    unsupported: &mut Vec<UnsupportedItem>,
) -> Result<()> {
    let spec = parse_spec(entry)?;
    match tool {
        Tool::Claude => {
            let exec_command = format!("{TEAM_HOOK_EXEC_PREFIX}{}", entry.id);
            // 参数与超时保存为 spec（hook 注册时写入工作区 hook-specs）
            let spec_json = serde_json::json!({
                "resource_id": entry.id,
                "command": spec.command,
                "timeout_ms": spec.timeout_ms,
                "matcher": spec.matcher,
            });
            artifacts.push(Artifact {
                resource_id: format!("{}#spec", entry.id),
                target_tool: tool.as_str().into(),
                kind: "hook".into(),
                path: PathBuf::from(format!(
                    ".ailoom-hook-specs/{}.json",
                    entry.id.to_string().replace('/', "__")
                )),
                body: ArtifactBody::Full {
                    content: serde_json::to_string_pretty(&spec_json)?,
                },
            });
            // 托管签名 = exec 前缀 + 本资源完整 ID：同一事件多个团队 Hook 各有签名，
            // 重复同步按签名替换（不按下标），用户/内置 hook 与其他团队 hook 互不影响。
            let signature = format!("{TEAM_HOOK_EXEC_PREFIX}{}", entry.id);
            artifacts.push(Artifact {
                resource_id: entry.id.to_string(),
                target_tool: tool.as_str().into(),
                kind: "hook".into(),
                path: ".claude/settings.json".into(),
                body: ArtifactBody::JsonArrayMerge {
                    pointer: format!("/hooks/{}", spec.event),
                    signature,
                    value: serde_json::json!({
                        "matcher": spec.matcher.unwrap_or_default(),
                        "hooks": [{
                            "type": "command",
                            "command": format!("{exec_command} --event {}", spec.event),
                            "timeout": (spec.timeout_ms / 1000).max(1),
                        }]
                    }),
                },
            });
            Ok(())
        }
        Tool::Alva => {
            unsupported.push(UnsupportedItem {
                resource_id: entry.id.to_string(),
                tool: tool.as_str().into(),
                kind: "hook".into(),
                reason: "alva hook 体系未核实（能力矩阵 unknown），不注册团队 Hook".into(),
            });
            Ok(())
        }
        Tool::Codex => {
            unsupported.push(UnsupportedItem {
                resource_id: entry.id.to_string(),
                tool: tool.as_str().into(),
                kind: "hook".into(),
                reason: "Codex 项目级 hooks 未核实（能力矩阵 unknown），不注册团队 Hook".into(),
            });
            Ok(())
        }
    }
}
