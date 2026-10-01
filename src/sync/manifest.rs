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
}
