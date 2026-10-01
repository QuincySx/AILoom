//! Directory markers for optional configuration-node discovery.
//! A marker is evidence of an existing configuration directory, not proof that
//! a tool is installed, supported for deployment, or correctly configured.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AgentDirectory {
    pub agent: &'static str,
    pub label: &'static str,
    pub directory: &'static str,
}

/// New adapters register their directory here; the scanner is agent-agnostic.
pub fn directory_markers() -> Vec<AgentDirectory> {
    let mut markers = vec![
        AgentDirectory {
            agent: "claude",
            label: "Claude Code",
            directory: ".claude",
        },
        AgentDirectory {
            agent: "anthropic",
            label: "Anthropic",
            directory: ".anthropic",
        },
        AgentDirectory {
            agent: "codex",
            label: "Codex",
            directory: ".codex",
        },
        AgentDirectory {
            agent: "agents",
            label: "共享 Agent 配置",
            directory: ".agents",
        },
    ];
    markers.extend(
        super::hosts::HOSTS
            .iter()
            .filter(|h| h.id != "cursor")
            .map(|host| AgentDirectory {
                agent: host.id,
                label: host.label,
                directory: host.directory,
            }),
    );
    // Existing rules adapters already declare their discovery directories.
    markers.extend(
        super::registry::RULES_HOSTS
            .iter()
            .map(|host| AgentDirectory {
                agent: host.tool,
                label: host.tool,
                directory: host.rules_dir.split('/').next().unwrap_or(host.rules_dir),
            }),
    );
    markers
}
