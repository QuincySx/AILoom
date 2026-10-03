//! 托管清单（AIL-007）：每个生成目标的来源、内容哈希与所属工作区。

use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedItem {
    pub resource_id: String,
    pub target_tool: String,
    pub kind: String,
    pub content_hash: String,
    pub deployed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedManifest {
    pub schema_version: u32,
    pub workspace_id: String,
    /// 成功部署的源 revision（与锁文件是不同状态：部署可能落后于锁）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployed_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployed_digest: Option<String>,
    #[serde(default)]
    pub items: BTreeMap<String, ManagedItem>,
}

impl ManagedManifest {
    pub fn new(workspace_id: &str) -> Self {
        ManagedManifest {
            schema_version: 1,
            workspace_id: workspace_id.to_string(),
            deployed_revision: None,
            deployed_digest: None,
            items: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Option<ManagedManifest>> {
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)?;
        let m: ManagedManifest = serde_json::from_str(&text).map_err(|e| {
            Error::new(
                code::MANAGED_MANIFEST_CORRUPT,
                format!("托管清单损坏，无法判断哪些文件由 AILoom 部署（不做猜测）: {e}"),
            )
            .context(serde_json::json!({ "path": path.display().to_string() }))
            .fix(format!(
                "从备份恢复 {}；没有备份时把它移走后重新同步——已有部署会被当作用户文件按冲突保留，确认后手动删除再同步",
                path.display()
            ))
        })?;
        if m.schema_version != 1 {
            return Err(Error::new(
                code::SCHEMA_VERSION,
                format!("不支持的托管清单 schema_version: {}", m.schema_version),
            ));
        }
        Ok(Some(m))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::sync_common::atomic_write(path, serde_json::to_vec_pretty(self)?.as_slice())
    }

    /// 旧版 Codex Skill 配置曾以整文件记账。仅把有托管 Skill 链接佐证的
    /// 标准条目转换为独立所有权；其余配置保持用户所有，读取本身不写盘。
    pub fn load_for_workspace(path: &Path, ws_root: &Path) -> Result<Option<Self>> {
        let Some(mut managed) = Self::load(path)? else {
            return Ok(None);
        };
        let key = ".codex/config.toml";
        let Some(legacy) = managed.items.get(key).cloned().filter(|item| {
            item.resource_id == "ailoom-builtin/codex-skills-config" && item.kind == "skill-config"
        }) else {
            return Ok(Some(managed));
        };
        let file = ws_root.join(key);
        if file.is_file() {
            let config: toml::Value = std::fs::read_to_string(&file)?.parse()?;
            if let Some(entries) = config
                .get("skills")
                .and_then(|v| v.get("config"))
                .and_then(|v| v.as_array())
            {
                for entry in entries {
                    let Some(skill_path) = entry.get("path").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let link_path = skill_path.strip_suffix("/SKILL.md").unwrap_or(skill_path);
                    let owned = managed.items.iter().any(|(key, item)| {
                        crate::sync::plan::split_key(key).0 == link_path
                            && item.target_tool == "codex"
                            && item.kind == "skill"
                    });
                    if !owned || !crate::adapters::common::is_codex_managed_config_path(skill_path)
                    {
                        continue;
                    }
                    // 使用旧版生成值，而非当前值：手动禁用/新增字段仍表现为漂移。
                    let expected = toml::Value::Table(toml::map::Map::from_iter([
                        ("path".into(), toml::Value::String(skill_path.into())),
                        ("enabled".into(), toml::Value::Boolean(true)),
                    ]));
                    let mut item = legacy.clone();
                    item.content_hash = format!(
                        "sha256:{}",
                        crate::ids::sha256_hex(&serde_json::to_vec(&expected)?)
                    );
                    managed
                        .items
                        .entry(format!("{key}#tomlarr:skills.config:path:{skill_path}"))
                        .or_insert(item);
                }
            }
        }
        managed.items.remove(key);
        Ok(Some(managed))
    }
}
