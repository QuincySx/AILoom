//! 内置资源（AIL-017）：召回 Agent 与经验总结 Skill。
//! 通过统一 sync 管道部署（plan/apply/卸载全生命周期管理）；声明可关闭。

use super::common::{upsert_fragment, Artifact, ArtifactBody};
use super::ToolTargets;

pub const RECALL_AGENT_ID: &str = "ailoom-builtin/recall-agent";
pub const SHARE_LEARNING_ID: &str = "ailoom-builtin/share-learning";
pub const RECALL_HINT_ID: &str = "ailoom-builtin/recall-hint";

const RECALL_AGENT_MD: &str = include_str!("res/recall-agent.md");
const SHARE_LEARNING_SKILL_MD: &str = include_str!("res/share-learning-skill.md");
pub const RECALL_INSTRUCTIONS: &str = include_str!("res/recall-instructions.md");

/// 渲染内置资源产物（目标工具启用且未被声明关闭时）。
/// 文件内容与 AILoom 内置 Skill 完全一致：说明它由同步部署且未被改动（扫描时按托管展示）。
pub fn is_builtin_skill(content: &str) -> bool {
    content == SHARE_LEARNING_SKILL_MD
}

pub fn render(targets: &ToolTargets, builtins_enabled: bool) -> Vec<Artifact> {
    if !builtins_enabled {
        return Vec::new();
    }
    let mut artifacts = Vec::new();
    if targets.claude {
        artifacts.push(Artifact {
            resource_id: RECALL_HINT_ID.into(),
            target_tool: "claude".into(),
            kind: "rule".into(),
            path: ".claude/rules/ailoom-knowledge.md".into(),
            body: ArtifactBody::Full {
                content: RECALL_INSTRUCTIONS.into(),
            },
        });
        artifacts.push(Artifact {
            resource_id: RECALL_AGENT_ID.into(),
            target_tool: "claude".into(),
            kind: "agent".into(),
            path: ".claude/agents/ailoom-recall.md".into(),
            body: ArtifactBody::Full {
                content: RECALL_AGENT_MD.into(),
            },
        });
        artifacts.push(Artifact {
            resource_id: SHARE_LEARNING_ID.into(),
            target_tool: "claude".into(),
            kind: "skill".into(),
            path: ".claude/skills/ailoom-share-learning/SKILL.md".into(),
            body: ArtifactBody::Full {
                content: SHARE_LEARNING_SKILL_MD.into(),
            },
        });
    }
    if targets.codex {
        // 主会话直接获得检索流程，不依赖模型先决定是否调用召回 Agent。
        artifacts.push(Artifact {
            resource_id: RECALL_HINT_ID.into(),
            target_tool: "codex".into(),
            kind: "doc".into(),
            path: "AGENTS.md".into(),
            body: ArtifactBody::Fragment {
                content: format!("{RECALL_INSTRUCTIONS}\n可关闭：ailoom init --no-builtin"),
            },
        });
    }
    artifacts
}

/// 幂等插入片段（AGENTS.md 等）。
pub fn upsert_managed_fragment(file_text: &str, resource_id: &str, content: &str) -> String {
    upsert_fragment(file_text, resource_id, content)
}
