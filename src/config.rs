//! 项目声明 `.ailoom/project.toml` 与机器绑定（契约 §4.3/§4.4）。

use crate::error::{code, Error, Result};
use crate::manifest::valid_id;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SOURCE_NAME_MAX: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceDeclaration {
    pub name: String,
    /// git | local
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// 额外订阅源（AIL-025）：tags 过滤、exclude 排除、独立 projects/roles 范围。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtraSource {
    pub name: String,
    #[serde(rename = "type", default = "default_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    /// 标签订阅：仅选择 tags 相交的资源；不改变 learning 的项目/共享语义
    #[serde(default)]
    pub tags: Vec<String>,
    /// 显式排除的资源名
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn default_kind() -> String {
    "git".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetsDeclaration {
    #[serde(default = "yes")]
    pub claude: bool,
    #[serde(default = "yes")]
    pub codex: bool,
    /// 额外宿主：`alva`，以及 registry 已注册的 rules 宿主（cursor/antigravity…）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<String>,
}

fn yes() -> bool {
    true
}

impl Default for TargetsDeclaration {
    fn default() -> Self {
        TargetsDeclaration {
            claude: true,
            codex: true,
            extra: vec![],
        }
    }
}

/// Workspace 级 Skill 选用（有则与选择模型取交集；无则保持全量可部署）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RequireDeclaration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// 可选：加载 `.ailoom/agents/<name>.toml` 进一步收窄 skills
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

impl RequireDeclaration {
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty() && self.agent.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectDeclaration {
    pub schema_version: u32,
    pub source: SourceDeclaration,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub targets: TargetsDeclaration,
    /// 内置资源（召回 Agent/总结 Skill）默认开启，--no-builtin 关闭
    #[serde(default = "yes")]
    pub builtins: bool,
    /// 团队统计上报（默认关闭；显式开启才上报）
    #[serde(default)]
    pub reporting_enabled: bool,
    /// 额外订阅源（AIL-025）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_sources: Vec<ExtraSource>,
    /// Skill 选用门禁（ADR-0001 / Require）
    #[serde(default, skip_serializing_if = "RequireDeclaration::is_empty")]
    pub require: RequireDeclaration,
}

impl ProjectDeclaration {
    pub fn parse(text: &str) -> Result<ProjectDeclaration> {
        let decl: ProjectDeclaration = toml::from_str(text).map_err(|e| {
            Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("project.toml 解析失败: {e}"),
            )
        })?;
        decl.validate()?;
        Ok(decl)
    }

    pub fn load(path: &Path) -> Result<Option<ProjectDeclaration>> {
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text).map(Some)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::new(
                code::SCHEMA_VERSION,
                format!(
                    "不支持的 project.toml schema_version: {}",
                    self.schema_version
                ),
            ));
        }
        if !valid_id(&self.source.name, SOURCE_NAME_MAX) {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!(
                    "source.name 非法（需 [a-z0-9-]{{1,{SOURCE_NAME_MAX}}}）: {}",
                    self.source.name
                ),
            ));
        }
        match self.source.kind.as_str() {
            "git" => {
                let url = self.source.url.as_deref().unwrap_or_default();
                if url.trim().is_empty() {
                    return Err(Error::new(code::MANIFEST_MISSING_FIELD, "git 源缺少 url"));
                }
                if let Some(idx) = url.find("://") {
                    let after = &url[idx + 3..];
                    let authority = &after[..after.find('/').unwrap_or(after.len())];
                    if let Some((userinfo, _)) = authority.rsplit_once('@') {
                        if userinfo.contains(':') {
                            return Err(Error::new(
                                code::SOURCE_URL_CREDENTIAL,
                                "source.url 不允许内嵌用户名密码",
                            ));
                        }
                    }
                }
            }
            "local" | "self" => {
                let p = self.source.path.as_deref().unwrap_or_default();
                let path = Path::new(p);
                let empty_ok = self.source.kind == "self" && p.is_empty();
                if (!p.is_empty() && (path.is_absolute() || p.starts_with('~')))
                    || (!empty_ok && (p.is_empty() || path.is_absolute() || p.starts_with('~')))
                {
                    return Err(Error::new(
                        code::ILLEGAL_PATH,
                        "local/self 源 path 必须是相对路径（可提交声明不得含绝对机器路径）",
                    )
                    .context(serde_json::json!({ "path": p })));
                }
            }
            other => {
                return Err(Error::new(
                    code::MANIFEST_MISSING_FIELD,
                    format!("未知 source.type: {other}（支持 git/local/self）"),
                ));
            }
        }
        check_unique(&self.projects, "projects")?;
        check_unique(&self.roles, "roles")?;
        for p in &self.projects {
            if !valid_id(p, 64) {
                return Err(Error::new(
                    code::MANIFEST_MISSING_FIELD,
                    format!("项目 id 非法: {p}"),
                ));
            }
        }
        for r in &self.roles {
            if !valid_id(r, 64) {
                return Err(Error::new(
                    code::MANIFEST_MISSING_FIELD,
                    format!("角色 id 非法: {r}"),
                ));
            }
        }
        validate_require(&self.require)?;
        Ok(())
    }
}

fn validate_require(req: &RequireDeclaration) -> Result<()> {
    check_unique(&req.skills, "require.skills")?;
    for s in &req.skills {
        let t = s.trim();
        if t.is_empty() || t.contains("..") || t.starts_with('/') || t.starts_with('\\') {
            return Err(Error::new(
                code::ILLEGAL_PATH,
                format!("require.skills 非法条目: {s}"),
            ));
        }
    }
    if let Some(agent) = &req.agent {
        if !valid_id(agent, 64) {
            return Err(Error::new(
                code::MANIFEST_MISSING_FIELD,
                format!("require.agent 非法: {agent}"),
            ));
        }
    }
    Ok(())
}

pub fn check_unique_public(items: &[String], field: &str) -> Result<()> {
    check_unique(items, field)
}

fn check_unique(items: &[String], field: &str) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        if !seen.insert(item.as_str()) {
            return Err(Error::new(
                code::DUPLICATE_ITEM,
                format!("{field} 含重复项: {item}"),
            ));
        }
    }
    Ok(())
}

/// 机器绑定（不提交；位于 <data_root>/ws/<workspace_id>/binding.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub schema_version: u32,
    pub workspace_root: String,
    pub repository_anchor: String,
    pub workspace_id: String,
    pub device_id: String,
    pub declaration: ProjectDeclaration,
    pub created_at: String,
    pub updated_at: String,
}

impl Binding {
    pub fn save(&self, path: &Path) -> Result<()> {
        crate::sync_common::atomic_write(path, serde_json::to_vec_pretty(self)?.as_slice())
    }

    pub fn load(path: &Path) -> Result<Option<Binding>> {
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)?;
        let b: Binding = serde_json::from_str(&text).map_err(|e| {
            Error::new(code::INTERNAL, format!("binding.json 损坏: {e}"))
                .fix("删除该文件后重新 ailoom init")
        })?;
        Ok(Some(b))
    }
}

/// device_id：按数据根持久化。
pub fn device_id(data_root: &Path) -> Result<String> {
    std::fs::create_dir_all(data_root)?;
    let file = data_root.join("device-id");
    if let Ok(existing) = std::fs::read_to_string(&file) {
        let t = existing.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let id = crate::ids::new_id();
    crate::sync_common::atomic_write(&file, id.as_bytes())?;
    Ok(id)
}
