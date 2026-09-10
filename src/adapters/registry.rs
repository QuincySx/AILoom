//! 声明式 rules 宿主注册表（AIL-007/009 扩展，T07/T08/T09）。
//! 每个宿主一条声明：发现路径 + 文件格式 + frontmatter 模板；通用渲染器消费。
//! 铁律不变：`verified: false` 的宿主渲染产物在能力矩阵标 file-placed/未核实，不得标 supported。

use super::common::{Artifact, ArtifactBody};
use crate::error::Result;
use crate::resource::ResourceEntry;

#[derive(Debug, Clone, Copy)]
pub struct RulesHostSpec {
    pub tool: &'static str,
    /// 项目内规则发现目录
    pub rules_dir: &'static str,
    pub ext: &'static str,
    /// frontmatter 模板：{name} {description} 占位
    pub frontmatter: &'static str,
    /// 发现路径已按官方文档/源码核实
    pub verified: bool,
}

/// 已注册的 rules-only 宿主。zcode/pi 发现路径未核实——核实前不入表（能力矩阵标 unknown）。
pub static RULES_HOSTS: &[RulesHostSpec] = &[
    // Cursor 官方文档：项目规则位于 .cursor/rules/*.mdc，frontmatter 支持 description + alwaysApply/globs
    RulesHostSpec {
        tool: "cursor",
        rules_dir: ".cursor/rules",
        ext: "mdc",
        frontmatter: "description: {description}\nalwaysApply: true",
        verified: true,
    },
    // Antigravity（Windsurf 血统）：项目规则目录 + 激活模式 frontmatter。发现路径待官方文档核实。
    RulesHostSpec {
        tool: "antigravity",
        rules_dir: ".antigravity/rules",
        ext: "md",
        frontmatter: "trigger: always_on\ndescription: {description}",
        verified: false,
    },
];

pub fn lookup(tool: &str) -> Option<&'static RulesHostSpec> {
    RULES_HOSTS.iter().find(|h| h.tool == tool)
}

/// 把规则资源渲染为指定宿主的规则文件产物（整文件，托管清单管理生命周期）。
pub fn render_rules(
    entry: &ResourceEntry,
    spec: &RulesHostSpec,
    artifacts: &mut Vec<Artifact>,
) -> Result<()> {
    let content =
        crate::adapters::rules::strip_frontmatter_content(entry.raw.as_deref().unwrap_or_default());
    let fm = spec
        .frontmatter
        .replace("{name}", &entry.id.name)
        .replace("{description}", &entry.description);
    let file_content = format!("---\n{fm}\n---\n\n{content}");
    artifacts.push(Artifact {
        resource_id: entry.id.to_string(),
        target_tool: spec.tool.into(),
        kind: "rule".into(),
        path: format!("{}/{}.{}", spec.rules_dir, entry.id.name, spec.ext).into(),
        body: ArtifactBody::Full {
            content: file_content,
        },
    });
    Ok(())
}
