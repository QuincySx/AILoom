//! 同步计划引擎（AIL-007）：读取目标当前状态 → 按所有权矩阵分类 → 产出带前置哈希的计划。
//! plan 不写任何文件；Apply（AIL-008）逐项重查前置哈希。

use crate::adapters::common::{current_json_entry, read_fragment, Artifact, ArtifactBody};
use crate::error::Result;
use crate::ids::now_iso;
use crate::sync::manifest::ManagedManifest;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Create,
    Update,
    Delete,
    /// 目标消失但清单在列：计划恢复
    Restore,
    /// 用户修改或未托管：保留并冲突，不自动接管
    Conflict,
    /// 已是期望内容
    Noop,
    /// 宿主/适配器不支持该资源：显式列出，绝不静默降级
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanAction {
    pub action: ActionKind,
    pub path: String,
    /// 托管清单条目键（path[#mode:loc]）
    pub item_key: String,
    pub resource_id: String,
    pub target_tool: String,
    pub kind: String,
    pub reason: String,
    /// Apply 前置：目标当前内容哈希（存在时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precondition_hash: Option<String>,
    pub desired_hash: String,
    /// 删除类操作：清单中的旧哈希
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncPlan {
    pub schema_version: u32,
    pub created_at: String,
    pub source_identity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub actions: Vec<PlanAction>,
}

impl SyncPlan {
    pub fn summary(&self) -> String {
        let mut c = String::new();
        let mut n = 0usize;
        for a in &self.actions {
            let tag = match a.action {
                ActionKind::Create => "create",
                ActionKind::Update => "update",
                ActionKind::Delete => "delete",
                ActionKind::Restore => "restore",
                ActionKind::Conflict => "conflict",
                ActionKind::Noop => "noop",
                ActionKind::Unsupported => "unsupported",
            };
            if tag == "noop" {
                n += 1;
                continue;
            }
            c.push_str(&format!("{tag:8} {}\n", a.path));
        }
        if n > 0 {
            c.push_str(&format!("noop     {n} 项已是期望内容\n"));
        }
        c
    }

    pub fn has_conflicts(&self) -> bool {
        self.actions
            .iter()
            .any(|a| a.action == ActionKind::Conflict)
    }
}

/// 目标当前内容哈希（不存在 → None）。
pub fn current_state(ws_root: &Path, artifact: &Artifact) -> Result<Option<String>> {
    let file = ws_root.join(&artifact.path);
    match &artifact.body {
        ArtifactBody::Full { .. } => {
            if !file.is_file() {
                return Ok(None);
            }
            let bytes = std::fs::read(&file)?;
            Ok(Some(format!("sha256:{}", crate::ids::sha256_hex(&bytes))))
        }
        ArtifactBody::JsonPointer { pointer, .. } => current_json_entry(&file, pointer),
        ArtifactBody::TomlTable { table, .. } => {
            if !file.is_file() {
                return Ok(None);
            }
            let text = std::fs::read_to_string(&file)?;
            let value: toml::Value = text.parse().map_err(|e| {
                crate::error::Error::new(
                    crate::error::code::USER_CONTENT_CONFLICT,
                    format!("TOML 配置解析失败（保留原文件）: {e}"),
                )
                .context(serde_json::json!({ "file": file.display().to_string() }))
            })?;
            let mut cur = &value;
            for seg in table.split('.') {
                match cur.get(seg) {
                    Some(v) => cur = v,
                    None => return Ok(None),
                }
            }
            let rendered = serde_json::to_vec(cur).map_err(|e| {
                crate::error::Error::new(
                    crate::error::code::RENDER_FAILED,
                    format!("TOML 值规范化失败: {e}"),
                )
            })?;
            Ok(Some(format!(
                "sha256:{}",
                crate::ids::sha256_hex(&rendered)
            )))
        }
        ArtifactBody::Fragment { .. } => {
            if !file.is_file() {
                return Ok(None);
            }
            let text = std::fs::read_to_string(&file)?;
            Ok(read_fragment(&text, &artifact.resource_id)
                .map(|c| format!("sha256:{}", crate::ids::sha256_hex(c.as_bytes()))))
        }
        ArtifactBody::Symlink { .. } => {
            let meta = match std::fs::symlink_metadata(&file) {
                Ok(m) => m,
                Err(_) => return Ok(None),
            };
            if !meta.file_type().is_symlink() {
                // 旧版曾是实体目录：视为需要更新为链接
                return Ok(Some(format!(
                    "sha256:{}",
                    crate::ids::sha256_hex(b"not-a-symlink")
                )));
            }
            let target = std::fs::read_link(&file)?;
            let abs = if target.is_absolute() {
                target
            } else {
                file.parent().unwrap_or(ws_root).join(target)
            };
            if !abs.is_dir() {
                // 坏链 / store 目标缺失 → 视同目标消失，走 Restore
                return Ok(None);
            }
            let digest = crate::store::dir_digest(&abs).unwrap_or_else(|_| "missing".into());
            let payload = format!("{}|{digest}", abs.display());
            Ok(Some(format!(
                "sha256:{}",
                crate::ids::sha256_hex(payload.as_bytes())
            )))
        }
    }
}

/// 构建计划：期望产物 + 现有托管清单 + 工作区当前状态。
/// 输入相同则输出稳定（动作按 (path, resource_id) 排序）。
pub fn build_plan(
    artifacts: &[Artifact],
    managed: &ManagedManifest,
    ws_root: &Path,
    source_identity: &str,
    revision: Option<String>,
) -> Result<SyncPlan> {
    let mut actions: Vec<PlanAction> = Vec::new();

    for artifact in artifacts {
        artifact.validate()?;
        let key = artifact.item_key();
        let desired_hash = artifact.desired_hash()?;
        let current = current_state(ws_root, artifact)?;
        let item = managed.items.get(&key);

        let action = match (current.clone(), item) {
            // 目标不存在
            (None, None) => ActionKind::Create,
            // 目标消失，清单在列 → 恢复
            (None, Some(item)) => {
                actions.push(plan_action(
                    artifact,
                    ActionKind::Restore,
                    &desired_hash,
                    None,
                    Some(item.content_hash.clone()),
                    "目标消失，按托管清单恢复",
                ));
                continue;
            }
            (Some(cur), Some(item)) => {
                if cur == desired_hash {
                    ActionKind::Noop
                } else if cur == item.content_hash {
                    // 未被用户修改 → 正常更新
                    ActionKind::Update
                } else {
                    // 用户改过 → 冲突（保留）
                    actions.push(plan_action(
                        artifact,
                        ActionKind::Conflict,
                        &desired_hash,
                        Some(cur),
                        Some(item.content_hash.clone()),
                        "用户修改与部署版本不一致，保留并冲突",
                    ));
                    continue;
                }
            }
            (Some(cur), None) => {
                // 未托管同名（整文件或既有条目）：即使内容相同也不自动接管
                actions.push(plan_action(
                    artifact,
                    ActionKind::Conflict,
                    &desired_hash,
                    Some(cur),
                    None,
                    "目标未被 AILoom 托管，不自动接管",
                ));
                continue;
            }
        };
        let reason = match action {
            ActionKind::Create => "目标不存在，创建",
            ActionKind::Update => "托管内容与期望不一致，更新",
            ActionKind::Noop => "已是期望内容",
            _ => "",
        };
        actions.push(plan_action(
            artifact,
            action,
            &desired_hash,
            current.clone(),
            item.map(|i| i.content_hash.clone()),
            reason,
        ));
    }

    // 清理：清单在列但本次期望产物不再包含的条目
    let mut stale_keys: Vec<&String> = managed
        .items
        .keys()
        .filter(|key| !artifacts.iter().any(|a| a.item_key() == **key))
        .collect();
    stale_keys.sort();
    for key in stale_keys {
        let item = &managed.items[key];
        let (path, _) = split_key(key);
        let current = current_hash_by_key(ws_root, key, &item.resource_id)?;
        let (action, reason) = match &current {
            None => (ActionKind::Noop, "目标已不存在，仅清理托管清单"),
            Some(cur) if *cur == item.content_hash => {
                (ActionKind::Delete, "源不再需要该目标，计划清理")
            }
            Some(_) => (
                ActionKind::Conflict,
                "源已删除该资源但目标被用户修改，保留并冲突",
            ),
        };
        actions.push(PlanAction {
            action,
            item_key: key.clone(),
            path,
            resource_id: item.resource_id.clone(),
            target_tool: item.target_tool.clone(),
            kind: item.kind.clone(),
            reason: reason.into(),
            precondition_hash: current,
            desired_hash: String::new(),
            manifest_hash: Some(item.content_hash.clone()),
        });
    }

    actions.sort_by(|a, b| (&a.path, &a.resource_id).cmp(&(&b.path, &b.resource_id)));
    Ok(SyncPlan {
        schema_version: 1,
        created_at: now_iso(),
        source_identity: source_identity.to_string(),
        revision,
        actions,
    })
}

/// 按托管清单条目键读取当前内容哈希（删除分类用）。
pub fn current_hash_by_key(ws_root: &Path, key: &str, resource_id: &str) -> Result<Option<String>> {
    let (path, mode) = split_key(key);
    let file = ws_root.join(&path);
    match mode {
        None => {
            // 可能是旧 Full 文件，也可能是目录/软链挂载点
            if let Ok(meta) = file.symlink_metadata() {
                if meta.file_type().is_symlink() {
                    return current_symlink_hash(&file);
                }
                if meta.is_dir() {
                    // 旧版实体目录：用占位哈希，交给删除逻辑处理
                    return Ok(Some(format!(
                        "sha256:{}",
                        crate::ids::sha256_hex(b"directory")
                    )));
                }
            }
            if !file.is_file() {
                return Ok(None);
            }
            let bytes = std::fs::read(&file)?;
            Ok(Some(format!("sha256:{}", crate::ids::sha256_hex(&bytes))))
        }
        Some(mode) if mode == "symlink" => {
            if !file
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                return Ok(None);
            }
            current_symlink_hash(&file)
        }
        Some(mode) if mode.starts_with("json:") => current_json_entry(&file, &mode[5..]),
        Some(mode) if mode.starts_with("toml:") => {
            if !file.is_file() {
                return Ok(None);
            }
            let text = std::fs::read_to_string(&file)?;
            let value: toml::Value = text.parse().map_err(|e| {
                crate::error::Error::new(
                    crate::error::code::USER_CONTENT_CONFLICT,
                    format!("TOML 配置解析失败: {e}"),
                )
            })?;
            let table = &mode[5..];
            let mut cur = &value;
            for seg in table.split('.') {
                match cur.get(seg) {
                    Some(v) => cur = v,
                    None => return Ok(None),
                }
            }
            let rendered = serde_json::to_vec(cur).map_err(|e| {
                crate::error::Error::new(
                    crate::error::code::RENDER_FAILED,
                    format!("TOML 值规范化失败: {e}"),
                )
            })?;
            Ok(Some(format!(
                "sha256:{}",
                crate::ids::sha256_hex(&rendered)
            )))
        }
        Some(mode) if mode == "fragment" || mode.starts_with("fragment:") => {
            let rid = mode.strip_prefix("fragment:").unwrap_or(resource_id);
            if !file.is_file() {
                return Ok(None);
            }
            let text = std::fs::read_to_string(&file)?;
            Ok(read_fragment(&text, rid)
                .map(|c| format!("sha256:{}", crate::ids::sha256_hex(c.as_bytes()))))
        }
        Some(other) => Err(crate::error::Error::new(
            crate::error::code::INTERNAL,
            format!("未知托管清单条目模式: {other}"),
        )),
    }
}

fn plan_action(
    artifact: &Artifact,
    action: ActionKind,
    desired_hash: &str,
    precondition: Option<String>,
    manifest_hash: Option<String>,
    reason: &str,
) -> PlanAction {
    PlanAction {
        action,
        item_key: artifact.item_key(),
        path: artifact.path.display().to_string(),
        resource_id: artifact.resource_id.clone(),
        target_tool: artifact.target_tool.clone(),
        kind: artifact.kind.clone(),
        reason: reason.to_string(),
        precondition_hash: precondition,
        desired_hash: desired_hash.to_string(),
        manifest_hash,
    }
}

/// 拆分 item key：`path[#mode:loc]` → (path, mode)
pub fn split_key(key: &str) -> (String, Option<String>) {
    match key.find("#") {
        Some(idx) => (key[..idx].to_string(), Some(key[idx + 1..].to_string())),
        None => (key.to_string(), None),
    }
}

fn current_symlink_hash(file: &Path) -> Result<Option<String>> {
    let target = std::fs::read_link(file)?;
    let abs = if target.is_absolute() {
        target
    } else {
        file.parent().map(|p| p.join(&target)).unwrap_or(target)
    };
    if !abs.is_dir() {
        return Ok(None);
    }
    let digest = crate::store::dir_digest(&abs).unwrap_or_else(|_| "missing".into());
    let payload = format!("{}|{digest}", abs.display());
    Ok(Some(format!(
        "sha256:{}",
        crate::ids::sha256_hex(payload.as_bytes())
    )))
}
