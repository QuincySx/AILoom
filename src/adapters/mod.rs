//! 宿主适配器（AIL-009—012）：按能力声明渲染产物，不直接写盘。
//! 能力矩阵见 docs/capabilities/（官方来源与核实日期记录在其中）。

pub mod agents;
pub mod builtin;
pub mod common;
pub mod docs;
pub mod env;
pub mod hooks_team;
pub mod mcp;
pub mod rules;
pub mod skills;

use crate::error::Result;
use crate::resolver::DesiredSet;
use crate::resource::ResourceKind;
use common::Artifact;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Tool {
    Claude,
    Codex,
}

impl Tool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
        }
    }
}

/// 渲染输入：绑定启用的工具集合。
#[derive(Debug, Clone, Default)]
pub struct ToolTargets {
    pub claude: bool,
    pub codex: bool,
}

impl ToolTargets {
    pub fn enabled(&self, tool: Tool) -> bool {
        match tool {
            Tool::Claude => self.claude,
            Tool::Codex => self.codex,
        }
    }
    pub fn iter(&self) -> Vec<Tool> {
        let mut v = Vec::new();
        if self.claude {
            v.push(Tool::Claude);
        }
        if self.codex {
            v.push(Tool::Codex);
        }
        v
    }
}

/// 显式不支持记录（进入计划 Unsupported 动作，绝不静默）。
#[derive(Debug, Serialize)]
pub struct UnsupportedItem {
    pub resource_id: String,
    pub tool: String,
    pub kind: String,
    pub reason: String,
}

/// 把期望资源渲染为各工具产物。
pub fn render(
    desired: &DesiredSet,
    snapshot_root: &Path,
    targets: &ToolTargets,
    ws_root: &Path,
) -> Result<(Vec<Artifact>, Vec<UnsupportedItem>)> {
    let mut artifacts = Vec::new();
    let mut unsupported = Vec::new();

    for selected in desired.deployable() {
        let entry = &selected.entry;
        // 资源级 targets ∩ 绑定 targets
        let resource_targets = common::resource_targets(entry.raw.as_deref());
        for tool in targets.iter() {
            if let Some(list) = &resource_targets {
                if !list.iter().any(|t| t == tool.as_str()) {
                    continue; // 该资源未面向此工具发布
                }
            }
            match entry.id.kind {
                ResourceKind::Skill => {
                    skills::render(
                        entry,
                        snapshot_root,
                        &desired.identity,
                        &desired.skills_root,
                        tool,
                        &mut artifacts,
                        &mut unsupported,
                    )?;
                }
                ResourceKind::Rule => {
                    rules::render(entry, snapshot_root, tool, &mut artifacts, &mut unsupported)
                }
                ResourceKind::Doc => docs::render(entry, snapshot_root, tool, &mut artifacts),
                ResourceKind::Agent => {
                    agents::render(entry, tool, &mut artifacts, &mut unsupported)?
                }
                ResourceKind::Mcp => mcp::render(entry, tool, &mut artifacts, &mut unsupported)?,
                ResourceKind::Env => {
                    env::render(entry, snapshot_root, tool, &mut artifacts, &mut unsupported)?;
                }
                ResourceKind::Hook => {
                    hooks_team::render(entry, ws_root, tool, &mut artifacts, &mut unsupported)?;
                }
                ResourceKind::Package => {
                    unsupported.push(UnsupportedItem {
                        resource_id: entry.id.to_string(),
                        tool: tool.as_str().into(),
                        kind: "package".into(),
                        reason: "包依赖经 ailoom packages check/install 显式管理，不随 sync 部署（AIL-033）".into(),
                    });
                }
                ResourceKind::Learning => {}
            }
        }
    }

    // 文档索引片段（每工具最多一个）
    docs::render_index(desired, snapshot_root, targets, &mut artifacts)?;

    Ok((artifacts, unsupported))
}
