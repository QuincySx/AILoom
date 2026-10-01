//! 同步执行（AIL-008）：重查前置 → 整文件备份 → 原子写 → journal → 推进托管清单。
//! 不承诺多文件事务；失败留下可检查恢复点；恢复保护后来的人为修改。

use crate::adapters::common::{remove_fragment, upsert_fragment, Artifact, ArtifactBody};
use crate::error::{code, Error, Result};
use crate::ids::{new_id, now_iso};
use crate::sync::journal::{JournalEntry, JournalRun};
use crate::sync::lock::SyncLock;
use crate::sync::manifest::{ManagedItem, ManagedManifest};
use crate::sync::plan::{
    current_hash_by_key, current_state, split_key, ActionKind, PlanAction, SyncPlan,
};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ApplyReport {
    pub ok: bool,
    pub applied: Vec<String>,
    pub noop: usize,
    pub skipped_conflicts: Vec<String>,
    pub deployed_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_journal: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn apply(
    plan: &SyncPlan,
    artifacts: &[Artifact],
    managed: &mut ManagedManifest,
    ws_root: &Path,
    lock_dir: &Path,
    journal_root: &Path,
    device_id: &str,
) -> Result<ApplyReport> {
    // 先拿锁再看 journal：否则并发的另一次 sync 正在写的 journal 会被误判为崩溃遗留，
    // 并提示用户 --recover（回滚别人的进行中写入）。持锁时仍存在的 journal 才是真正的遗留。
    let lock = SyncLock::acquire(lock_dir, device_id)?;
    // 崩溃/失败遗留的恢复点必须先处理
    let pending = JournalRun::find_pending(journal_root)?;
    if let Some(first) = pending.first() {
        let _ = lock.release();
        return Err(Error::new(
            code::JOURNAL_RESTORE_FAILED,
            format!(
                "存在未完成的同步 journal，先恢复再同步: {}",
                first.display()
            ),
        )
        .fix("运行 ailoom sync --recover"));
    }

    let mut run = JournalRun::start(
        journal_root,
        &format!("{}-{}", now_iso().replace(':', ""), new_id()),
    )?;
    std::fs::create_dir_all(run.backup_dir())?;

    let mut report = ApplyReport {
        ok: true,
        applied: vec![],
        noop: 0,
        skipped_conflicts: vec![],
        deployed_revision: None,
        failed: None,
        pending_journal: None,
    };

    let failure: Option<Error> = 'outer: {
        for action in &plan.actions {
            match action.action {
                ActionKind::Noop => report.noop += 1,
                ActionKind::Unsupported => report
                    .skipped_conflicts
                    .push(format!("unsupported:{}", action.resource_id)),
                ActionKind::Conflict => report.skipped_conflicts.push(action.path.clone()),
                ActionKind::Create
                | ActionKind::Update
                | ActionKind::Restore
                | ActionKind::Delete => {
                    if let Err(err) = apply_action(action, artifacts, ws_root, &mut run) {
                        break 'outer Some(err);
                    }
                    report.applied.push(action.path.clone());
                }
            }
        }
        None
    };

    if let Some(err) = failure {
        report.ok = false;
        report.failed = Some(err.to_json());
        report.pending_journal = Some(run.dir.display().to_string());
        drop(run);
        let _ = lock.release();
        return Ok(report);
    }

    // 全部成功：推进托管清单（存在冲突时不推进 revision）
    for action in &plan.actions {
        match action.action {
            ActionKind::Create | ActionKind::Update | ActionKind::Restore => {
                let artifact = artifacts.iter().find(|a| a.item_key() == action.item_key);
                let hash = match artifact {
                    Some(a) => a.desired_hash()?,
                    None => continue,
                };
                managed.items.insert(
                    action.item_key.clone(),
                    ManagedItem {
                        resource_id: action.resource_id.clone(),
                        target_tool: action.target_tool.clone(),
                        kind: action.kind.clone(),
                        content_hash: hash,
                        deployed_at: now_iso(),
                    },
                );
            }
            ActionKind::Delete => {
                managed.items.remove(&action.item_key);
            }
            _ => {}
        }
    }
    if report.skipped_conflicts.is_empty() {
        managed.deployed_revision = plan.revision.clone();
    }

    run.complete()?;
    lock.release()?;
    Ok(report)
}

/// 读取整个文件当前字节（供备份与哈希）。
fn whole_file(ws_root: &Path, rel: &str) -> Result<Option<Vec<u8>>> {
    let f = ws_root.join(rel);
    if f.is_file() {
        Ok(Some(std::fs::read(&f)?))
    } else {
        Ok(None)
    }
}

fn apply_action(
    action: &PlanAction,
    artifacts: &[Artifact],
    ws_root: &Path,
    run: &mut JournalRun,
) -> Result<()> {
    let artifact = artifacts.iter().find(|a| a.item_key() == action.item_key);

    // 1. 重查前置哈希（旧计划遇到本地变动拒绝执行）
    let current = match artifact {
        Some(a) => current_state(ws_root, a)?,
        None => current_hash_by_key(ws_root, &action.item_key, &action.resource_id)?,
    };
    let precondition_ok = match (&current, &action.precondition_hash) {
        (None, None) => true,
        (Some(c), Some(p)) => {
            if c == p {
                true
            } else if let Some(a) = artifact {
                // 共享 SkillStore：同 skill 的另一工具链接可能已先物化到期望摘要
                a.desired_hash().ok().as_ref() == Some(c)
            } else {
                false
            }
        }
        // 结构化/片段 create：文件已存在但条目缺失是合法前置
        // Symlink Restore：共享 store 可能已被同 skill 另一工具链接先修好
        (Some(c), None) => match artifact {
            Some(a) if matches!(a.body, ArtifactBody::Symlink { .. }) => {
                a.desired_hash().ok().as_ref() == Some(c)
            }
            Some(a) => !matches!(
                a.body,
                ArtifactBody::Full { .. }
                    | ArtifactBody::Symlink { .. }
                    | ArtifactBody::ExternalSymlink { .. }
            ),
            None => false,
        },
        (None, Some(_)) => false,
    };
    if !precondition_ok {
        return Err(Error::new(
            code::PRECONDITION_FAILED,
            format!("前置校验失败，拒绝执行: {}", action.path),
        )
        .context(serde_json::json!({
            "current": current,
            "precondition": action.precondition_hash,
        }))
        .fix("重新运行 ailoom plan 查看差异"));
    }

    // 2. 每步独立备份：名字带 seq，同一文件的多个片段各自保存恢复点，
    //    后一步绝不覆盖前一步保存的原始内容（R01）
    let seq = run.next_seq();
    let rel_path = action.path.clone();
    let prior_bytes = whole_file(ws_root, &rel_path)?;
    let prior_hash = prior_bytes
        .as_ref()
        .map(|b| format!("sha256:{}", crate::ids::sha256_hex(b)));
    let backup_file = if let Some(bytes) = &prior_bytes {
        let name = format!(
            "{:06}-{}.bak",
            seq,
            crate::ids::sha256_prefix(rel_path.as_bytes(), 16)
        );
        std::fs::write(run.backup_dir().join(&name), bytes)?;
        Some(name)
    } else {
        None
    };

    // 3. 执行写入/删除（按条目键删除，绝不误删整文件中的他人内容）
    match action.action {
        ActionKind::Delete => remove_by_key(ws_root, &action.item_key, &action.resource_id)?,
        _ => {
            let artifact = artifact.ok_or_else(|| {
                Error::new(code::INTERNAL, format!("缺少执行产物: {}", action.path))
            })?;
            write_artifact(ws_root, artifact)?;
        }
    }

    // 4. 记录写入后的整文件哈希（恢复时校验当前内容是否仍是本次写入）
    let after_bytes = whole_file(ws_root, &rel_path)?;
    let written_hash = after_bytes
        .as_ref()
        .map(|b| format!("sha256:{}", crate::ids::sha256_hex(b)))
        .unwrap_or_default();
    run.append(&JournalEntry {
        seq,
        item_key: action.item_key.clone(),
        path: rel_path,
        action: serde_json::to_value(action.action)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "?".into()),
        resource_id: action.resource_id.clone(),
        written_hash,
        backup_file,
        backup_hash: prior_hash,
        done: true,
    })?;
    Ok(())
}

fn write_artifact(ws_root: &Path, artifact: &Artifact) -> Result<()> {
    let file = ws_root.join(&artifact.path);
    match &artifact.body {
        ArtifactBody::ExternalSymlink { target } => {
            if !target.is_absolute()
                || target.canonicalize().ok().as_ref() != Some(target)
                || !target.join("SKILL.md").is_file()
            {
                return Err(Error::new(
                    code::SOURCE_NOT_CACHED,
                    "外部 Skill 路径失效，请修复原目录",
                ));
            }
            if let Ok(meta) = file.symlink_metadata() {
                if !meta.file_type().is_symlink() {
                    return Err(Error::new(
                        code::USER_CONTENT_CONFLICT,
                        "项目挂载位置已有普通文件或目录，拒绝覆盖",
                    ));
                }
                std::fs::remove_file(&file)?;
            }
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, &file)?;
                Ok(())
            }
            #[cfg(not(unix))]
            {
                Err(Error::new(
                    code::WRITE_FAILED,
                    "当前平台暂不支持外部 Skill 链接部署",
                ))
            }
        }
        ArtifactBody::Full { content } => {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::sync_common::atomic_write(&file, content.as_bytes())
        }
        ArtifactBody::JsonPointer { pointer, value } => {
            let mut root: serde_json::Value = if file.is_file() {
                serde_json::from_str(&std::fs::read_to_string(&file)?).map_err(|e| {
                    Error::new(code::USER_CONTENT_CONFLICT, format!("配置解析失败: {e}"))
                })?
            } else {
                serde_json::json!({})
            };
            set_json_pointer(&mut root, pointer, value.clone())?;
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::sync_common::atomic_write(&file, serde_json::to_vec_pretty(&root)?.as_slice())
        }
        ArtifactBody::JsonArrayMerge {
            pointer,
            signature,
            value,
        } => {
            let mut root: serde_json::Value = if file.is_file() {
                serde_json::from_str(&std::fs::read_to_string(&file)?).map_err(|e| {
                    Error::new(code::USER_CONTENT_CONFLICT, format!("配置解析失败: {e}"))
                })?
            } else {
                serde_json::json!({})
            };
            let arr = ensure_json_array_at(&mut root, pointer)?;
            arr.retain(|e| !json_entry_has_signature(e, signature));
            arr.push(value.clone());
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::sync_common::atomic_write(&file, serde_json::to_vec_pretty(&root)?.as_slice())
        }
        ArtifactBody::TomlArrayEntry {
            table,
            key_field,
            entry: value,
        } => {
            let mut root: toml::Value = if file.is_file() {
                std::fs::read_to_string(&file)?.parse().map_err(|e| {
                    Error::new(code::USER_CONTENT_CONFLICT, format!("TOML 解析失败: {e}"))
                })?
            } else {
                toml::Value::Table(Default::default())
            };
            let arr = ensure_toml_array_at(&mut root, table)?;
            let kv = value
                .get(key_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if let Some(slot) = arr
                .iter_mut()
                .find(|item| item.get(key_field).and_then(|v| v.as_str()) == Some(kv.as_str()))
            {
                *slot = value.clone();
            } else {
                arr.push(value.clone());
            }
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::sync_common::atomic_write(&file, toml::to_string_pretty(&root)?.as_bytes())
        }
        ArtifactBody::TomlTable { table, value } => {
            let mut root: toml::Value = if file.is_file() {
                std::fs::read_to_string(&file)?.parse().map_err(|e| {
                    Error::new(code::USER_CONTENT_CONFLICT, format!("TOML 解析失败: {e}"))
                })?
            } else {
                toml::Value::Table(Default::default())
            };
            let mut cur = &mut root;
            for seg in table.split('.') {
                cur = cur
                    .as_table_mut()
                    .ok_or_else(|| Error::new(code::USER_CONTENT_CONFLICT, "TOML 结构冲突"))?
                    .entry(seg.to_string())
                    .or_insert_with(|| toml::Value::Table(Default::default()));
            }
            *cur = value.clone();
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::sync_common::atomic_write(&file, toml::to_string_pretty(&root)?.as_bytes())
        }
        ArtifactBody::Fragment { content } => {
            let text = if file.is_file() {
                std::fs::read_to_string(&file)?
            } else {
                String::new()
            };
            let new_text = upsert_fragment(&text, &artifact.resource_id, content);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::sync_common::atomic_write(&file, new_text.as_bytes())
        }
        ArtifactBody::Symlink {
            target,
            source_dir,
            source_identity,
            content_digest,
        } => {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // 仅在真正 apply 时物化 store，保留冲突路径上的用户修改
            let store_root = crate::store::resolve_store_root()?;
            let rel = target
                .strip_prefix(crate::store::source_bucket(&store_root, source_identity))
                .unwrap_or(target.as_path());
            if artifact.resource_id.starts_with("collection-") && target.exists() {
                // 锁定版本实体可能被其他 Worktree 共同引用；首次向新 Worktree 挂载也不能覆盖其后改。
                if !target.is_dir() || crate::store::dir_digest(target)? != *content_digest {
                    return Err(Error::new(
                        code::USER_CONTENT_CONFLICT,
                        "合集的已缓存 Skill 实体被修改，保留修改并拒绝覆盖",
                    ));
                }
            } else {
                crate::store::materialize_skill_dir(&store_root, source_identity, rel, source_dir)?;
            }
            // 清掉旧实体目录或旧链接
            if let Ok(meta) = std::fs::symlink_metadata(&file) {
                if meta.file_type().is_symlink() || meta.is_file() {
                    std::fs::remove_file(&file)?;
                } else if meta.is_dir() {
                    crate::sync_common::remove_dir_all_guarded(&file)?;
                }
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, &file).map_err(|e| {
                    Error::new(
                        code::WRITE_FAILED,
                        format!(
                            "创建技能软链失败 {} → {}: {e}",
                            file.display(),
                            target.display()
                        ),
                    )
                })?;
            }
            #[cfg(not(unix))]
            {
                return Err(Error::new(
                    code::WRITE_FAILED,
                    "当前平台暂不支持 SkillStore 软链部署",
                ));
            }
            Ok(())
        }
    }
}

/// 删除文件后清理变空的父目录（不超过工作区根）。
fn cleanup_empty_parents(ws_root: &Path, file: &Path) {
    let Some(mut dir) = file.parent() else { return };
    while let Some(parent) = dir.parent() {
        if parent == ws_root || !dir.is_dir() {
            break;
        }
        if std::fs::read_dir(dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(dir);
        } else {
            break;
        }
        dir = parent;
    }
}

/// 确保点分路径末端是数组（`[[agent]]` 场景）；已存在非数组则报冲突。
fn ensure_toml_array_at<'a>(
    root: &'a mut toml::Value,
    table: &str,
) -> Result<&'a mut Vec<toml::Value>> {
    let segs: Vec<&str> = table.split('.').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return Err(Error::new(code::INTERNAL, "TOML 数组表路径为空"));
    }
    if !root.is_table() {
        *root = toml::Value::Table(Default::default());
    }
    let (leaf, parents) = segs.split_last().unwrap();
    let mut cur = root;
    for seg in parents {
        cur = cur
            .as_table_mut()
            .ok_or_else(|| Error::new(code::USER_CONTENT_CONFLICT, "TOML 结构冲突"))?
            .entry(seg.to_string())
            .or_insert_with(|| toml::Value::Table(Default::default()));
    }
    let parent = cur
        .as_table_mut()
        .ok_or_else(|| Error::new(code::USER_CONTENT_CONFLICT, "TOML 结构冲突"))?;
    match parent.get(*leaf) {
        Some(toml::Value::Array(_)) => {}
        Some(_) => {
            return Err(Error::new(
                code::USER_CONTENT_CONFLICT,
                format!("TOML 路径 `{table}` 已存在且不是数组表，拒绝覆盖"),
            ));
        }
        None => {
            parent.insert(leaf.to_string(), toml::Value::Array(Vec::new()));
        }
    }
    Ok(parent.get_mut(*leaf).unwrap().as_array_mut().unwrap())
}

/// 按托管清单条目键删除：只删除自己的条目/片段/文件。
/// 沿点分路径下钻 &mut toml::Value（不存在则 None）。
fn descend_mut<'a>(root: &'a mut toml::Value, segs: &[&str]) -> Option<&'a mut toml::Value> {
    let (first, rest) = segs.split_first()?;
    if rest.is_empty() {
        root.get_mut(first)
    } else {
        descend_mut(root.get_mut(first)?, rest)
    }
}

pub fn remove_by_key(ws_root: &Path, key: &str, resource_id: &str) -> Result<()> {
    let (path, mode) = split_key(key);
    let file = ws_root.join(&path);
    match mode.as_deref() {
        Some("external-link") => match file.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                std::fs::remove_file(&file)?;
                cleanup_empty_parents(ws_root, &file);
                Ok(())
            }
            Ok(_) => Err(Error::new(
                code::USER_CONTENT_CONFLICT,
                "外部链接已被普通文件或目录替换，拒绝删除",
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        },
        None | Some("symlink") => {
            // Full 技能树 / symlink 挂载点
            if let Ok(meta) = file.symlink_metadata() {
                if meta.file_type().is_symlink() || meta.is_file() {
                    std::fs::remove_file(&file)?;
                    cleanup_empty_parents(ws_root, &file);
                } else if meta.is_dir() {
                    crate::sync_common::remove_dir_all_guarded(&file)?;
                }
            }
            Ok(())
        }
        Some(m) if m == "fragment" || m.starts_with("fragment:") => {
            let rid = m.strip_prefix("fragment:").unwrap_or(resource_id);
            if file.is_file() {
                let text = std::fs::read_to_string(&file)?;
                let (new_text, existed) = remove_fragment(&text, rid);
                if existed {
                    crate::sync_common::atomic_write(&file, new_text.as_bytes())?;
                }
            }
            Ok(())
        }
        Some(m) if m.starts_with("json:") => {
            if file.is_file() {
                let mut root: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(&file)?)?;
                remove_json_pointer(&mut root, &m[5..]);
                crate::sync_common::atomic_write(
                    &file,
                    serde_json::to_vec_pretty(&root)?.as_slice(),
                )?;
            }
            Ok(())
        }
        Some(m) if m.starts_with("jsonmerge:") => {
            // 数组托管条目删除：按签名移除嵌套 hooks[].command 匹配项（保留其余内容）
            let rest = &m["jsonmerge:".len()..];
            if let Some((pointer, signature)) = rest.split_once(':') {
                if file.is_file() {
                    let mut root: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(&file)?)?;
                    if let Some(arr) = root.pointer_mut(pointer).and_then(|v| v.as_array_mut()) {
                        arr.retain(|e| !json_entry_has_signature(e, signature));
                        crate::sync_common::atomic_write(
                            &file,
                            serde_json::to_vec_pretty(&root)?.as_slice(),
                        )?;
                    }
                }
            }
            Ok(())
        }
        Some(m) if m.starts_with("tomlarr:") => {
            // 数组条目删除：按 key_field 定位并移除（保留其它条目）
            let rest = &m["tomlarr:".len()..];
            let parts: Vec<&str> = rest.splitn(3, ':').collect();
            if parts.len() == 3 && file.is_file() {
                let (table, key_field, kv) = (parts[0], parts[1], parts[2]);
                let mut root: toml::Value = std::fs::read_to_string(&file)?.parse()?;
                if let Some(arr) = descend_mut(&mut root, &table.split('.').collect::<Vec<_>>())
                    .and_then(|v| v.as_array_mut())
                {
                    arr.retain(|item| item.get(key_field).and_then(|v| v.as_str()) != Some(kv));
                    crate::sync_common::atomic_write(
                        &file,
                        toml::to_string_pretty(&root)?.as_bytes(),
                    )?;
                }
            }
            Ok(())
        }
        Some(m) if m.starts_with("toml:") => {
            if file.is_file() {
                let mut root: toml::Value = std::fs::read_to_string(&file)?.parse()?;
                let table_path = &m[5..];
                let leaf = table_path.rsplit('.').next().unwrap_or(table_path);
                let parent_path = table_path.rsplit_once('.').map(|(p, _)| p.to_string());
                let container = match parent_path {
                    Some(p) => {
                        let mut cur = &mut root;
                        for seg in p.split('.') {
                            cur = cur
                                .get_mut(seg)
                                .ok_or_else(|| Error::new(code::INTERNAL, "TOML 缺表"))?;
                        }
                        cur
                    }
                    None => &mut root,
                };
                if let Some(t) = container.as_table_mut() {
                    t.remove(leaf);
                }
                crate::sync_common::atomic_write(&file, toml::to_string_pretty(&root)?.as_bytes())?;
            }
            Ok(())
        }
        Some(other) => Err(Error::new(
            code::INTERNAL,
            format!("未知托管清单条目模式: {other}"),
        )),
    }
}

/// 定位（必要时创建）JSON pointer 指向的数组；已存在但不是数组 → 冲突。
/// 中间缺失节点一律创建对象，只有叶子创建数组（RW-02/S02：`/hooks/Stop`
/// 首次写入时 hooks 应是对象、Stop 才是数组）。
fn ensure_json_array_at<'a>(
    root: &'a mut serde_json::Value,
    pointer: &str,
) -> Result<&'a mut Vec<serde_json::Value>> {
    let segments: Vec<&str> = pointer.split('/').filter(|s| !s.is_empty()).collect();
    if root.is_null() {
        *root = serde_json::json!({});
    }
    if segments.is_empty() {
        if !root.is_array() {
            return Err(Error::new(
                code::USER_CONTENT_CONFLICT,
                format!("JSON pointer 目标不是数组: {pointer}"),
            ));
        }
        return Ok(root.as_array_mut().unwrap());
    }
    let mut cur = root;
    // 中间段：缺失/显式 null 创建对象，已存在但不是对象 → 冲突（不写盘，原文件不变）
    for seg in &segments[..segments.len() - 1] {
        if !cur.is_object() {
            return Err(Error::new(
                code::USER_CONTENT_CONFLICT,
                format!("JSON pointer 父节点不是对象: /{seg}"),
            ));
        }
        let obj = cur.as_object_mut().unwrap();
        cur = obj
            .entry(seg.to_string())
            .or_insert_with(|| serde_json::json!({}));
        if cur.is_null() {
            *cur = serde_json::json!({});
        }
    }
    // 叶子段：缺失/显式 null 创建数组，已存在但不是数组 → 冲突
    let last = segments.last().unwrap();
    if !cur.is_object() {
        return Err(Error::new(
            code::USER_CONTENT_CONFLICT,
            format!("JSON pointer 父节点不是对象: /{last}"),
        ));
    }
    let obj = cur.as_object_mut().unwrap();
    let entry = obj
        .entry(last.to_string())
        .or_insert_with(|| serde_json::json!([]));
    if entry.is_null() {
        *entry = serde_json::json!([]);
    }
    if !entry.is_array() {
        return Err(Error::new(
            code::USER_CONTENT_CONFLICT,
            format!("JSON pointer 目标不是数组: {pointer}"),
        ));
    }
    Ok(entry.as_array_mut().unwrap())
}

/// 嵌套 hooks[].command 是否以托管签名开头（Claude settings hook 条目形态）。
pub fn json_entry_has_signature(entry: &serde_json::Value, signature: &str) -> bool {
    entry
        .pointer("/hooks")
        .and_then(|h| h.as_array())
        .map(|hs| {
            hs.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.starts_with(signature))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn set_json_pointer(
    root: &mut serde_json::Value,
    pointer: &str,
    value: serde_json::Value,
) -> Result<()> {
    let segments: Vec<&str> = pointer.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        *root = value;
        return Ok(());
    }
    if root.is_null() {
        *root = serde_json::json!({});
    }
    let mut cur = root;
    for (i, seg) in segments.iter().enumerate() {
        let last = i == segments.len() - 1;
        // 数组索引段：父为数组时按下标定位（== len 表示追加）
        if let Ok(idx) = seg.parse::<usize>() {
            if cur.is_array() {
                let arr = cur.as_array_mut().unwrap();
                if last {
                    if idx == arr.len() {
                        arr.push(value);
                    } else if idx < arr.len() {
                        arr[idx] = value;
                    } else {
                        return Err(Error::new(
                            code::USER_CONTENT_CONFLICT,
                            format!("数组索引越界: {idx} > {}", arr.len()),
                        ));
                    }
                    return Ok(());
                }
                if idx == arr.len() {
                    arr.push(serde_json::json!({}));
                }
                cur = arr.get_mut(idx).unwrap();
                continue;
            }
        }
        if last {
            if !cur.is_object() {
                return Err(Error::new(
                    code::USER_CONTENT_CONFLICT,
                    format!("JSON pointer 父节点不是对象: /{seg}"),
                ));
            }
            cur.as_object_mut().unwrap().insert(seg.to_string(), value);
            return Ok(());
        }
        if cur.get(*seg).is_none() {
            if !cur.is_object() {
                return Err(Error::new(
                    code::USER_CONTENT_CONFLICT,
                    format!("JSON pointer 父节点不是对象: /{seg}"),
                ));
            }
            cur.as_object_mut()
                .unwrap()
                .insert(seg.to_string(), serde_json::json!({}));
        }
        cur = cur.get_mut(*seg).unwrap();
    }
    Ok(())
}

fn remove_json_pointer(root: &mut serde_json::Value, pointer: &str) {
    let segments: Vec<&str> = pointer.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return;
    }
    let mut cur = root;
    for seg in &segments[..segments.len() - 1] {
        match cur.get_mut(*seg) {
            Some(v) => cur = v,
            None => return,
        }
    }
    if let Some(obj) = cur.as_object_mut() {
        obj.remove(*segments.last().unwrap());
    }
}

/// 恢复结果报告：ok=false 表示存在损坏/缺失备份，恢复点保留待人工检查。
#[derive(Debug, Default, serde::Serialize)]
pub struct RecoverReport {
    pub ok: bool,
    /// 已逐字节还原的目标（同一文件多个步骤各自独立还原）
    pub recovered: Vec<String>,
    /// 当前内容不属于本次写入（用户已修改/已恢复），明确拒绝覆盖
    pub skipped_user_modified: Vec<String>,
    /// 备份文件缺失或摘要不匹配，无法安全还原
    pub broken_backups: Vec<String>,
    /// 因备份损坏而保留的 journal 运行目录（恢复证据）
    pub pending_runs: Vec<String>,
}

/// 崩溃/失败后的恢复：逆序回滚仍属于本次写入的文件（当前整文件哈希 == written_hash）。
/// 每步使用独立备份并校验备份摘要；目标被用户编辑时拒绝覆盖；备份损坏时报失败并保留证据。
pub fn recover(journal_root: &Path, ws_root: &Path) -> Result<RecoverReport> {
    let mut report = RecoverReport {
        ok: true,
        ..Default::default()
    };
    for run_dir in JournalRun::find_pending(journal_root)? {
        let mut entries = crate::sync::journal::read_entries(&run_dir)?;
        entries.sort_by_key(|e| e.seq);
        let mut run_broken = false;
        for entry in entries.into_iter().rev() {
            let file = ws_root.join(&entry.path);
            // 恢复写回的是 AILoom 写入前的备份字节（回滚自己的部分写入），
            // 即使路径后来被跟踪也只会回到公司原有内容——不属于改写
            let current_hash = match std::fs::read(&file) {
                Ok(bytes) => format!("sha256:{}", crate::ids::sha256_hex(&bytes)),
                Err(_) if entry.written_hash.is_empty() => String::new(),
                Err(_) => "missing".to_string(),
            };
            if current_hash != entry.written_hash {
                // 被人改过或已不在：跳过，保留后来的人为修改
                report.skipped_user_modified.push(entry.path.clone());
                continue;
            }
            match (&entry.backup_file, &entry.backup_hash) {
                (Some(backup), Some(expected_hash)) => {
                    let backup_path = run_dir.join("backup").join(backup);
                    let verified = backup_path
                        .is_file()
                        .then(|| std::fs::read(&backup_path))
                        .transpose()?
                        .map(|bytes| {
                            format!("sha256:{}", crate::ids::sha256_hex(&bytes)) == *expected_hash
                        });
                    match verified {
                        Some(true) => {
                            let bytes = std::fs::read(&backup_path)?;
                            crate::sync_common::atomic_write(&file, bytes.as_slice())?;
                            report.recovered.push(entry.path.clone());
                        }
                        _ => {
                            // 备份缺失或摘要不匹配：不得以恢复成功掩盖损坏
                            report.broken_backups.push(entry.path.clone());
                            run_broken = true;
                        }
                    }
                }
                (None, None) => {
                    // create：回滚=删除本次创建的文件
                    if file.is_file() || file.symlink_metadata().is_ok() {
                        std::fs::remove_file(&file)?;
                        report.recovered.push(entry.path.clone());
                    }
                }
                _ => {
                    report.broken_backups.push(entry.path.clone());
                    run_broken = true;
                }
            }
        }
        if run_broken {
            // 保留运行目录作为恢复证据，等待人工处置
            report.pending_runs.push(run_dir.display().to_string());
        } else {
            crate::sync_common::remove_dir_all_guarded(&run_dir)?;
        }
    }
    report.ok = report.broken_backups.is_empty();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 并发 sync：另一进程持锁并已写出进行中的 journal 时，本次必须报 E4003（锁被持有），
    /// 而不是把它的 journal 当作崩溃遗留、提示用户 --recover。
    #[test]
    fn concurrent_sync_reports_lock_not_pending_journal() {
        let tmp = tempfile::tempdir().unwrap();
        let (locks, journal, ws) = (
            tmp.path().join("locks"),
            tmp.path().join("journal"),
            tmp.path().join("ws"),
        );
        std::fs::create_dir_all(&ws).unwrap();
        let _held = SyncLock::acquire(&locks, "other-device").unwrap();
        let _running = JournalRun::start(&journal, "in-progress").unwrap();
        let plan = SyncPlan {
            schema_version: 1,
            created_at: now_iso(),
            source_identity: "test".into(),
            revision: None,
            actions: vec![],
        };
        let mut managed = ManagedManifest::new("ws");
        let err = apply(&plan, &[], &mut managed, &ws, &locks, &journal, "me").unwrap_err();
        assert_eq!(err.code, code::LOCK_HELD, "{err}");
    }

    /// RW-02/S02：嵌套指针首次写入——中间节点创建对象、叶子创建数组。
    #[test]
    fn nested_pointer_creates_object_intermediate_and_array_leaf() {
        let mut root = serde_json::json!({});
        let arr = ensure_json_array_at(&mut root, "/hooks/Stop").unwrap();
        arr.push(serde_json::json!({"matcher": ""}));
        assert!(root["hooks"].is_object(), "第一层 hooks 应是对象: {root}");
        assert!(
            root["hooks"]["Stop"].is_array(),
            "叶子 Stop 应是数组: {root}"
        );
        assert_eq!(root["hooks"]["Stop"][0]["matcher"], "");
    }

    /// RW-02：settings.json 不存在、`{}`、已有 hooks 对象三种输入均成功。
    #[test]
    fn accepts_missing_empty_and_existing_hooks_object() {
        // 不存在：空根
        let mut root = serde_json::json!({});
        ensure_json_array_at(&mut root, "/hooks/Stop")
            .unwrap()
            .push(serde_json::json!(1));
        assert_eq!(root["hooks"]["Stop"][0], 1);

        // 已有 hooks 对象与其他事件：共存
        let mut root = serde_json::json!({"hooks": {"PreToolUse": []}, "model": "opus"});
        ensure_json_array_at(&mut root, "/hooks/Stop")
            .unwrap()
            .push(serde_json::json!(2));
        assert_eq!(root["hooks"]["PreToolUse"], serde_json::json!([]));
        assert_eq!(root["model"], "opus", "无关字段保留");
        assert_eq!(root["hooks"]["Stop"][0], 2);
    }

    /// RW-02：已有用户值类型冲突（中间/叶子）显式失败。
    #[test]
    fn type_conflicts_fail_explicitly() {
        // 中间节点是数组（旧缺陷产物或用户内容）→ 冲突
        let mut root = serde_json::json!({"hooks": []});
        let err = ensure_json_array_at(&mut root, "/hooks/Stop").unwrap_err();
        assert!(format!("{err}").contains("父节点不是对象"), "{err}");
        // 叶子不是数组 → 冲突
        let mut root = serde_json::json!({"hooks": {"Stop": {"matcher": ""}}});
        let err = ensure_json_array_at(&mut root, "/hooks/Stop").unwrap_err();
        assert!(format!("{err}").contains("目标不是数组"), "{err}");
    }

    /// 显式 null 视为缺省：中间与叶子均可创建。
    #[test]
    fn null_values_treated_as_missing() {
        let mut root = serde_json::json!({"hooks": {"Stop": null}});
        ensure_json_array_at(&mut root, "/hooks/Stop")
            .unwrap()
            .push(serde_json::json!(1));
        assert_eq!(root["hooks"]["Stop"][0], 1);

        let mut root = serde_json::json!({"hooks": null});
        ensure_json_array_at(&mut root, "/hooks/Stop")
            .unwrap()
            .push(serde_json::json!(2));
        assert_eq!(root["hooks"]["Stop"][0], 2);
    }

    /// 一级指针（/Stop）与根指针（""）语义。
    #[test]
    fn shallow_and_root_pointers() {
        let mut root = serde_json::json!({});
        ensure_json_array_at(&mut root, "/Stop")
            .unwrap()
            .push(serde_json::json!(1));
        assert_eq!(root["Stop"][0], 1);

        // 根即数组
        let mut root = serde_json::json!([]);
        ensure_json_array_at(&mut root, "")
            .unwrap()
            .push(serde_json::json!(2));
        assert_eq!(root[0], 2);

        // 根为对象时根指针冲突
        let mut root = serde_json::json!({});
        let err = ensure_json_array_at(&mut root, "").unwrap_err();
        assert!(format!("{err}").contains("目标不是数组"), "{err}");
    }
}
