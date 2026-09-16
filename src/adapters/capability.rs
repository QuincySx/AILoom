//! 宿主能力查询接口（AIL-041）：tool × resource_kind × scope 的能力矩阵，
//! 供 onboarding/控制台解释「实际生效状态/是否需要新会话/不支持原因」。
//! 矩阵与 docs/capabilities/*.md 同步维护：代码是查询视图，md 是证据与来源。

use serde::Serialize;

/// 支持级别：native=宿主原生发现；generated=AILoom 生成入口/受管片段；
/// unsupported=明确不支持；unknown=未核实（显式标 unknown，不静默降级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Native,
    Generated,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capability {
    pub tool: &'static str,
    pub kind: &'static str,
    /// project | user | any
    pub scope: &'static str,
    pub support: Support,
    /// 加载方式说明（入口文件/目录、是否启动注入）
    pub load_mode: &'static str,
    /// 生效是否需要重启/新会话
    pub requires_new_session: bool,
    pub notes: &'static str,
    /// 官方来源
    pub official_doc: &'static str,
    /// 实测记录（含版本与日期）；None=未实测
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<&'static str>,
}

/// 全量能力矩阵（与 docs/capabilities/ 保持同步；变更需同步更新 md）。
pub fn matrix() -> Vec<Capability> {
    vec![
        // ---- Claude Code ----
        Capability {
            tool: "claude",
            kind: "skill",
            scope: "project",
            support: Support::Native,
            load_mode: ".claude/skills/<name>/SKILL.md（symlink 实体进 SkillStore）",
            requires_new_session: false,
            notes: "按需加载；子目录 skills 亦被发现",
            official_doc: "code.claude.com/docs/en/skills",
            verified: Some("claude 2.1.272，2026-09-16 隔离项目真实调用通过（docs/evidence/console/2026-09-16-host-probes.md）"),
        },
        Capability {
            tool: "claude",
            kind: "rule",
            scope: "project",
            support: Support::Native,
            load_mode: ".claude/rules/*.md（递归，frontmatter paths 条件由宿主解析）",
            requires_new_session: true,
            notes: "会话加载；与 CLAUDE.md 基线共同生效",
            official_doc: "code.claude.com/docs/en/memory",
            verified: None,
        },
        Capability {
            tool: "claude",
            kind: "doc",
            scope: "project",
            support: Support::Generated,
            load_mode: "受管副本 .ailoom/docs/ + CLAUDE.md 索引片段",
            requires_new_session: true,
            notes: "个人模式不写 CLAUDE.md（AIL-042），改用个人指令视图",
            official_doc: "code.claude.com/docs/en/memory",
            verified: None,
        },
        Capability {
            tool: "claude",
            kind: "agent",
            scope: "project",
            support: Support::Native,
            load_mode: ".claude/agents/*.md",
            requires_new_session: true,
            notes: "会话加载",
            official_doc: "code.claude.com/docs/en/sub-agents",
            verified: None,
        },
        Capability {
            tool: "claude",
            kind: "mcp",
            scope: "project",
            support: Support::Native,
            load_mode: ".mcp.json mcpServers.<name>（stdio/http）",
            requires_new_session: true,
            notes: "首次连接需用户批准；设置成功 ≠ 连接成功",
            official_doc: "code.claude.com/docs/en/mcp",
            verified: Some("claude 2.1.272，2026-09-16 mcp list 连接探测（docs/evidence/console/2026-09-16-host-probes.md）"),
        },
        // ---- Codex CLI ----
        Capability {
            tool: "codex",
            kind: "skill",
            scope: "project",
            support: Support::Native,
            load_mode: ".agents/skills/<name>（CWD 向上扫描至仓库根，支持 symlink）+ .codex/config.toml skills.config 显式条目",
            requires_new_session: true,
            notes: "2026-09-16 官方复核：原生发现路径为 .agents/skills；旧 .ailoom/skills 路径由 sync 过期清理迁移",
            official_doc: "learn.chatgpt.com/docs/build-skills",
            verified: Some("codex-cli 0.154.0，2026-09-16 隔离项目真实发现（docs/evidence/console/2026-09-16-host-probes.md）"),
        },
        Capability {
            tool: "codex",
            kind: "rule",
            scope: "project",
            support: Support::Generated,
            load_mode: "仓库根 AGENTS.md 受管片段（就近优先）",
            requires_new_session: true,
            notes: "片段无条件语义；条件 paths 显式 Unsupported",
            official_doc: "learn.chatgpt.com/docs/agent-configuration/agents-md",
            verified: None,
        },
        Capability {
            tool: "codex",
            kind: "agent",
            scope: "project",
            support: Support::Unknown,
            load_mode: "—",
            requires_new_session: false,
            notes: "官方文档未确认项目级自定义 Agent → 显式 Unsupported，不写用户级配置充数",
            official_doc: "未能核实",
            verified: None,
        },
        Capability {
            tool: "codex",
            kind: "mcp",
            scope: "project",
            support: Support::Native,
            load_mode: ".codex/config.toml [mcp_servers.<name>]（stdio/http）",
            requires_new_session: true,
            notes: "官方文档：项目级配置仅受信项目加载（trust gate）；0.153.4 实测不加载 → 0.154.0 待本轮实测记录",
            official_doc: "learn.chatgpt.com/docs/extend/mcp",
            verified: Some("codex-cli 0.154.0，2026-09-16 mcp list 实测（docs/evidence/console/2026-09-16-host-probes.md）"),
        },
        // ---- alva ----
        Capability {
            tool: "alva",
            kind: "skill",
            scope: "project",
            support: Support::Native,
            load_mode: "co-load .claude/skills 与 .mcp.json（paths.rs 核实）",
            requires_new_session: true,
            notes: "无需单独部署产物",
            official_doc: "alva 本地核实",
            verified: Some("2026-09-11 alva-agent 仓库实测（docs/capabilities/alva.md）"),
        },
    ]
}

/// 查询单条能力；未登记返回 None（调用方按 unknown 处理并显式说明）。
pub fn query(tool: &str, kind: &str, scope: &str) -> Option<Capability> {
    matrix()
        .into_iter()
        .find(|c| c.tool == tool && c.kind == kind && (c.scope == scope || c.scope == "any"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_skill_uses_native_dir_and_requires_session() {
        let c = query("codex", "skill", "project").unwrap();
        assert_eq!(c.support, Support::Native);
        assert!(c.load_mode.contains(".agents/skills"));
        assert!(c.verified.is_some(), "AIL-041 必须有实测记录引用");
    }

    #[test]
    fn unknown_capabilities_stay_unknown() {
        let c = query("codex", "agent", "project").unwrap();
        assert_eq!(c.support, Support::Unknown);
        assert!(query("claude", "package", "project").is_none());
    }

    #[test]
    fn mcp_capabilities_record_trust_notes() {
        let c = query("codex", "mcp", "project").unwrap();
        assert!(c.notes.contains("受信"), "必须说明受信项目前提");
        let cl = query("claude", "mcp", "project").unwrap();
        assert!(cl.requires_new_session);
    }
}
