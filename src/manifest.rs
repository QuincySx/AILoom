//! 团队源清单 `ailoom.toml` v1 解析与校验（契约 §4.1，AIL-001 冻结）。

use crate::error::{code, Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

pub const MANIFEST_FILE: &str = "ailoom.toml";

/// 清单资源目录默认值（契约 §4.1 [paths]）。
pub const DEFAULT_PATHS: [(&str, &str); 9] = [
    ("skills", "resources/skills"),
    ("rules", "resources/rules"),
    ("docs", "resources/docs"),
    ("agents", "resources/agents"),
    ("mcp", "resources/mcp"),
    ("learnings", "resources/learnings"),
    ("env", "resources/env"),
    ("hooks", "resources/hooks"),
    ("packages", "resources/packages"),
];

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct TeamManifest {
    pub schema_version: u32,
    pub team_id: String,
    #[serde(default)]
    pub projects: BTreeMap<String, NamedDef>,
    #[serde(default)]
    pub roles: BTreeMap<String, NamedDef>,
    #[serde(default)]
    pub namespaces: Namespaces,
    #[serde(default)]
    pub paths: ResourcePaths,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct NamedDef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
pub struct Namespaces {
    #[serde(default)]
    pub known: Vec<String>,
    #[serde(default)]
    pub shared: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ResourcePaths {
    #[serde(default = "default_of")]
    pub skills: String,
    #[serde(default = "default_of")]
    pub rules: String,
    #[serde(default = "default_of")]
    pub docs: String,
    #[serde(default = "default_of")]
    pub agents: String,
    #[serde(default = "default_of")]
    pub mcp: String,
    #[serde(default = "default_of")]
    pub learnings: String,
    #[serde(default = "default_of")]
    pub env: String,
    #[serde(default = "default_of")]
    pub hooks: String,
    #[serde(default = "default_of")]
    pub packages: String,
}

fn default_of() -> String {
    String::new() // 由 validation 阶段按 DEFAULT_PATHS 兜底
}

impl Default for ResourcePaths {
    fn default() -> Self {
        ResourcePaths {
            skills: "resources/skills".into(),
            rules: "resources/rules".into(),
            docs: "resources/docs".into(),
            agents: "resources/agents".into(),
            mcp: "resources/mcp".into(),
            learnings: "resources/learnings".into(),
            env: "resources/env".into(),
            hooks: "resources/hooks".into(),
            packages: "resources/packages".into(),
        }
    }
}

/// 标识符：`[a-z0-9-]{1,64}`（契约 §1）。
pub fn valid_id(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// namespace/name：`[a-z0-9][a-z0-9._-]{0,63}`（契约 §1）。
pub fn valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    s.len() <= 64
        && s.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-'
        })
}

/// 相对路径校验：拒绝绝对路径、`..` 穿越与 `~`（E3003/E3007）。
pub fn validate_relative_path(kind: &str, p: &str) -> Result<()> {
    let path = Path::new(p);
    if p.is_empty()
        || path.is_absolute()
        || p.starts_with('~')
        || path.components().any(|c| c.as_os_str() == "..")
    {
        return Err(Error::new(
            code::PATH_TRAVERSAL,
            format!("清单 {kind} 路径非法：必须是非空相对路径，且不含 `..`/`~`"),
        )
        .context(serde_json::json!({ "path": p })));
    }
    Ok(())
}

impl TeamManifest {
    /// 从快照根解析并完整校验。
    pub fn load_from(snapshot_root: &Path) -> Result<TeamManifest> {
        let file = snapshot_root.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&file).map_err(|_| {
            Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("源快照缺少 {MANIFEST_FILE}，不是有效的团队资源源"),
            )
            .context(serde_json::json!({ "root": snapshot_root.display().to_string() }))
        })?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<TeamManifest> {
        let raw: TeamManifest = toml::from_str(text).map_err(|e| {
            Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("{MANIFEST_FILE} 解析失败: {e}"),
            )
        })?;
        raw.validate()?;
        Ok(raw)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::new(
                code::SCHEMA_VERSION,
                format!(
                    "不支持的 ailoom.toml schema_version: {}（本版本仅支持 1）",
                    self.schema_version
                ),
            ));
        }
        if !valid_id(&self.team_id, 64) {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("team_id 非法（需 [a-z0-9-]{{1,64}}）: {}", self.team_id),
            ));
        }
        for id in self.projects.keys() {
            if !valid_id(id, 64) {
                return Err(Error::new(
                    code::MANIFEST_MISSING_FIELD,
                    format!("项目 id 非法: {id}"),
                ));
            }
        }
        for id in self.roles.keys() {
            if !valid_id(id, 64) {
                return Err(Error::new(
                    code::MANIFEST_MISSING_FIELD,
                    format!("角色 id 非法: {id}"),
                ));
            }
        }
        for ns in &self.namespaces.known {
            if !valid_name(ns) {
                return Err(Error::new(
                    code::MANIFEST_MISSING_FIELD,
                    format!("namespace 非法: {ns}"),
                ));
            }
        }
        for ns in &self.namespaces.shared {
            if !self.namespaces.known.contains(ns) {
                return Err(Error::new(
                    code::UNKNOWN_REFERENCE,
                    format!("shared namespace 未在 known 中声明: {ns}"),
                ));
            }
        }
        // paths 校验（空 = 用默认，由 effective_paths 填充）
        let check = |kind: &str, p: &str| -> Result<()> {
            if p.is_empty() {
                return Ok(()); // 空 = 用默认，稍后填充
            }
            validate_relative_path(&format!("paths.{kind}"), p)
        };
        check("skills", &self.paths.skills)?;
        check("rules", &self.paths.rules)?;
        check("docs", &self.paths.docs)?;
        check("agents", &self.paths.agents)?;
        check("mcp", &self.paths.mcp)?;
        check("learnings", &self.paths.learnings)?;
        check("env", &self.paths.env)?;
        check("hooks", &self.paths.hooks)?;
        check("packages", &self.paths.packages)?;
        if self.paths.skills.is_empty() {
            // 空路径由 effective_paths 填充默认值；此处不修改 self
        }
        Ok(())
    }

    /// 返回填充默认值后的路径视图（不修改原清单文件内容）。
    pub fn effective_paths(&self) -> ResourcePaths {
        let d = ResourcePaths::default();
        let fill = |v: &str, dv: &str| -> String {
            if v.is_empty() {
                dv.to_string()
            } else {
                v.to_string()
            }
        };
        ResourcePaths {
            skills: fill(&self.paths.skills, &d.skills),
            rules: fill(&self.paths.rules, &d.rules),
            docs: fill(&self.paths.docs, &d.docs),
            agents: fill(&self.paths.agents, &d.agents),
            mcp: fill(&self.paths.mcp, &d.mcp),
            learnings: fill(&self.paths.learnings, &d.learnings),
            env: fill(&self.paths.env, &d.env),
            hooks: fill(&self.paths.hooks, &d.hooks),
            packages: fill(&self.paths.packages, &d.packages),
        }
    }

    pub fn require_project(&self, id: &str) -> Result<()> {
        if self.projects.contains_key(id) {
            Ok(())
        } else {
            Err(Error::new(
                code::UNKNOWN_REFERENCE,
                format!("项目 `{id}` 不在团队清单中"),
            )
            .context(serde_json::json!({ "known": self.projects.keys().collect::<Vec<_>>() }))
            .fix("核对项目 id，或联系团队管理员更新 ailoom.toml"))
        }
    }

    pub fn require_role(&self, id: &str) -> Result<()> {
        if self.roles.contains_key(id) {
            Ok(())
        } else {
            Err(Error::new(
                code::UNKNOWN_REFERENCE,
                format!("角色 `{id}` 不在团队清单中"),
            )
            .context(serde_json::json!({ "known": self.roles.keys().collect::<Vec<_>>() })))
        }
    }
}
