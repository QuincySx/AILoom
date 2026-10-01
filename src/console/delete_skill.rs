//! AIL-112：未托管 Skill 两段式删除。
//!
//! 第一段（execute=false）：服务端以项目根重新扫描，只接受扫描结果里的
//! `unmanaged` / `external_link` 条目；生成文件清单与内容指纹，签发绑定项目根的短期单次令牌。
//! 第二段（execute=true）：校验令牌/项目根/过期/目录名，重新扫描并复核指纹后归档到
//! 数据区 `project-archive/`（可恢复）。符号链接只摘除链接本身，并记录原链接目标以便恢复。

use super::{ensure_within_roots, LockExt, Request, Response, ServerState};
use crate::ids::new_id;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TOKEN_TTL: Duration = Duration::from_secs(600);
const MAX_FILES: u64 = 2000;
const MAX_DEPTH: usize = 6;

/// 一次删除确认：令牌只对签发时的项目根、目录与内容有效。
pub struct DeleteGrant {
    root: PathBuf,
    path: PathBuf,
    fingerprint: String,
    issued: Instant,
}

/// 目录快照：符号链接以链接目标为指纹；目录以相对路径 + 文件内容为指纹。
struct Snapshot {
    link_target: Option<PathBuf>,
    files: u64,
    bytes: u64,
    fingerprint: String,
}

fn snapshot(path: &Path) -> Result<Snapshot, String> {
    use sha2::Digest;
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("目录不可访问: {e}"))?;
    let mut hasher = sha2::Sha256::new();
    if !path.join("SKILL.md").is_file() {
        return Err("只接受包含 SKILL.md 的 Skill 目录".into());
    }
    if meta.is_symlink() {
        let target = std::fs::read_link(path).map_err(|e| format!("链接不可读: {e}"))?;
        hasher.update(b"symlink\0");
        hasher.update(target.to_string_lossy().as_bytes());
        return Ok(Snapshot {
            link_target: Some(target),
            files: 0,
            bytes: 0,
            fingerprint: format!("{:x}", hasher.finalize()),
        });
    }
    if !meta.is_dir() {
        return Err("只接受包含 SKILL.md 的 Skill 目录".into());
    }
    let (mut files, mut bytes) = (0u64, 0u64);
    hasher.update(b"dir\0");
    let mut entries: Vec<_> = walkdir::WalkDir::new(path)
        .follow_links(false)
        .max_depth(MAX_DEPTH)
        .into_iter()
        .flatten()
        .filter(|e| e.depth() > 0)
        .collect();
    entries.sort_by(|a, b| a.path().cmp(b.path()));
    for entry in entries {
        let rel = entry.path().strip_prefix(path).unwrap_or(entry.path());
        let ft = entry.file_type();
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        if ft.is_symlink() {
            // 目录内链接只记录目标，不跟随读取。
            let target = std::fs::read_link(entry.path()).unwrap_or_default();
            hasher.update(b"l");
            hasher.update(target.to_string_lossy().as_bytes());
        } else if ft.is_file() {
            files += 1;
            if files > MAX_FILES {
                return Err(format!(
                    "目录过大（>{MAX_FILES} 文件），请人工确认后手动处理"
                ));
            }
            let content = std::fs::read(entry.path())
                .map_err(|e| format!("文件不可读 {}: {e}", rel.display()))?;
            bytes += content.len() as u64;
            hasher.update(b"f");
            hasher.update(sha2::Sha256::digest(&content));
        } else {
            hasher.update(b"d");
        }
        hasher.update(b"\0");
    }
    Ok(Snapshot {
        link_target: None,
        files,
        bytes,
        fingerprint: format!("{:x}", hasher.finalize()),
    })
}

/// 以服务端扫描结果为准：路径必须是 `root` 内扫描出的未托管或外部链接条目。
/// 托管实体（指向 Store）与任意其他路径一律拒绝 —— 托管资源走「从本项目移除 + 应用」。
fn locate_in_scan(
    state: &ServerState,
    root: &Path,
    sub: Option<&str>,
    path: &str,
) -> Result<PathBuf, (u16, String)> {
    let store_root = crate::paths::resolve_store_root().unwrap_or_else(|_| state.data_root.clone());
    let store_root = store_root.canonicalize().unwrap_or(store_root);
    let scan = crate::commands::scan_skills::scan_project_skills(root, sub, &store_root);
    if let Some(e) = scan.get("error").and_then(Value::as_str) {
        return Err((400, e.to_string()));
    }
    let item = scan["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|it| it["path"].as_str() == Some(path));
    match item.and_then(|it| it["management"].as_str()) {
        Some("unmanaged") | Some("external_link") => Ok(PathBuf::from(path)),
        Some("managed") => Err((
            403,
            "这是 AILoom 托管部署：请用「从本项目移除」并应用，而不是删除原文件".into(),
        )),
        Some(_) => Err((403, "该条目状态异常，不能删除".into())),
        None => Err((
            403,
            "只能删除本项目扫描出的未托管 Skill（请重新扫描后再试）".into(),
        )),
    }
}

fn project_root(state: &ServerState, req: &Request) -> Result<PathBuf, Response> {
    let Some(root) = req.body["root"].as_str() else {
        return Err(Response::json(
            400,
            json!({ "error": "需要 root（项目目录）" }),
        ));
    };
    ensure_within_roots(state, Path::new(root))
        .map_err(|e| Response::json(403, json!({ "error": e })))
}

fn scan_sub(req: &Request) -> Option<String> {
    req.body["sub"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn handle(state: &Arc<ServerState>, req: &Request) -> Response {
    if req.body["execute"].as_bool().unwrap_or(false) {
        execute(state, req)
    } else {
        preview(state, req)
    }
}

fn preview(state: &Arc<ServerState>, req: &Request) -> Response {
    let root = match project_root(state, req) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let Some(raw) = req.body["path"].as_str() else {
        return Response::json(400, json!({ "error": "需要 path" }));
    };
    let path = match locate_in_scan(state, &root, scan_sub(req).as_deref(), raw) {
        Ok(p) => p,
        Err((status, e)) => return Response::json(status, json!({ "error": e })),
    };
    if path == root
        || state
            .approved_roots
            .lock()
            .unwrap()
            .iter()
            .any(|r| r == &path)
    {
        return Response::json(
            400,
            json!({ "error": "拒绝删除已批准根本身（项目根/资源库根）" }),
        );
    }
    let snap = match snapshot(&path) {
        Ok(s) => s,
        Err(e) => return Response::json(400, json!({ "error": e })),
    };
    let token = new_id();
    {
        let mut tokens = state.delete_tokens.lock_ok();
        tokens.retain(|_, g| g.issued.elapsed() <= TOKEN_TTL);
        tokens.insert(
            token.clone(),
            DeleteGrant {
                root: root.clone(),
                path: path.clone(),
                fingerprint: snap.fingerprint.clone(),
                issued: Instant::now(),
            },
        );
    }
    Response::json(
        200,
        json!({
            "token": token,
            "root": root.display().to_string(),
            "path": path.display().to_string(),
            "is_symlink": snap.link_target.is_some(),
            "link_target": snap.link_target.as_ref().map(|t| t.display().to_string()),
            "files": snap.files,
            "bytes": snap.bytes,
            "fingerprint": snap.fingerprint,
            "note": "令牌 10 分钟内有效、单次使用且只对本项目有效；执行前会重新校验目录未变化。",
        }),
    )
}

fn execute(state: &Arc<ServerState>, req: &Request) -> Response {
    let Some(token) = req.body["token"].as_str() else {
        return Response::json(400, json!({ "error": "需要 token（先预览生成确认）" }));
    };
    let name = req.body["name"].as_str().unwrap_or("").trim().to_string();
    // 单次有效：先取走令牌，防双击/重放；之后任何校验失败都需要重新预览。
    let Some(grant) = state.delete_tokens.lock_ok().remove(token) else {
        return Response::json(
            404,
            json!({ "error": "确认不存在或已使用：请重新预览生成新确认" }),
        );
    };
    if grant.issued.elapsed() > TOKEN_TTL {
        return Response::json(410, json!({ "error": "确认已过期（10 分钟）：请重新预览" }));
    }
    let root = match project_root(state, req) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    if root != grant.root {
        return Response::json(
            403,
            json!({ "error": "确认属于其他项目，本次确认已作废：请在当前项目重新预览" }),
        );
    }
    let path = grant.path;
    let dir_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if name != dir_name {
        return Response::json(
            400,
            json!({ "error": format!("输入名称与目录名不一致，本次确认已作废：请关闭对话框重新点「删除…」预览（需输入 {dir_name}）") }),
        );
    }
    // 执行前以当前文件系统重新判定：仍是本项目扫描出的未托管条目，且内容/链接目标未变。
    let path_str = path.display().to_string();
    if let Err((status, e)) = locate_in_scan(state, &root, scan_sub(req).as_deref(), &path_str) {
        return Response::json(status, json!({ "error": format!("目录状态已变化：{e}") }));
    }
    let snap = match snapshot(&path) {
        Ok(s) => s,
        Err(e) => {
            return Response::json(
                409,
                json!({ "error": format!("目录内容已变化：{e}；请重新预览确认") }),
            )
        }
    };
    if snap.fingerprint != grant.fingerprint {
        return Response::json(
            409,
            json!({ "error": "目录内容已变化（与预览不一致）：请重新预览确认" }),
        );
    }
    archive(
        state,
        &path,
        &dir_name,
        snap.link_target.as_deref(),
        &grant.fingerprint,
    )
}

fn archive(
    state: &ServerState,
    path: &Path,
    dir_name: &str,
    link_target: Option<&Path>,
    fingerprint: &str,
) -> Response {
    let archive_dir = state.data_root.join("project-archive");
    if let Err(e) = std::fs::create_dir_all(&archive_dir) {
        return Response::json(
            500,
            json!({ "error": format!("无法创建归档目录：{e}；目录未删除") }),
        );
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = archive_dir.join(format!("{stamp}-{dir_name}"));
    if let Some(target) = link_target {
        // 先写恢复记录再摘除链接：记录写不成就不删。
        let record = dest.with_extension("json");
        let body = json!({
            "schema_version": 1,
            "removed_symlink": path.display().to_string(),
            "link_target": target.display().to_string(),
            "sealed_at": stamp,
            "restore": format!("ln -s '{}' '{}'", target.display(), path.display()),
        });
        if let Err(e) = crate::sync_common::atomic_write(&record, body.to_string().as_bytes()) {
            return Response::json(
                500,
                json!({ "error": format!("无法写入恢复记录：{e}；链接未摘除") }),
            );
        }
        if let Err(e) = std::fs::remove_file(path) {
            let _ = std::fs::remove_file(&record);
            return Response::json(500, json!({ "error": format!("摘除链接失败: {e}") }));
        }
        return Response::json(
            200,
            json!({
                "archived_to": record.display().to_string(),
                "is_symlink": true,
                "link_target": target.display().to_string(),
                "note": "已摘除链接本身，链接目标未改动；恢复记录中保存了原链接目标。",
            }),
        );
    }
    if std::fs::rename(path, &dest).is_ok() {
        // 复核与移动之间仍可能被替换：在归档位置再算一次指纹，不一致就移回原处（AIL-132）。
        if snapshot(&dest).map(|s| s.fingerprint).as_deref() != Ok(fingerprint) {
            let restored = std::fs::rename(&dest, path).is_ok();
            return Response::json(
                409,
                json!({ "error": if restored {
                    "目录在执行期间发生变化，已放回原处：请重新预览确认".to_string()
                } else {
                    format!("目录在执行期间发生变化且无法放回原处，请从归档手动恢复：{}", dest.display())
                } }),
            );
        }
    } else {
        // 跨设备：复制完整并核对指纹后再删原目录；失败则清理半成品，原目录保持不动。
        let copied = copy_rec(path, &dest)
            .map_err(|e| e.to_string())
            .and_then(|_| match snapshot(&dest) {
                Ok(s) if s.fingerprint == fingerprint => Ok(()),
                Ok(_) => Err("目录在执行期间发生变化".to_string()),
                Err(e) => Err(e),
            });
        if let Err(e) = copied {
            let _ = std::fs::remove_dir_all(&dest);
            return Response::json(
                500,
                json!({ "error": format!("归档失败：{e}；目录未删除") }),
            );
        }
        if let Err(e) = std::fs::remove_dir_all(path) {
            return Response::json(
                500,
                json!({ "error": format!("已复制到归档但删除原目录失败：{e}（归档：{}）", dest.display()) }),
            );
        }
    }
    Response::json(
        200,
        json!({
            "archived_to": dest.display().to_string(),
            "is_symlink": false,
            "note": "已移入本机归档（可手动移回原路径恢复）；未删除任何外部链接目标。",
        }),
    )
}

fn copy_rec(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)?.flatten() {
        let ft = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ft.is_symlink() {
            let target = std::fs::read_link(entry.path())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, to)?;
            #[cfg(not(unix))]
            let _ = (target, to);
        } else if ft.is_dir() {
            copy_rec(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}
