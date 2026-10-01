//! 资源库命令（AIL-043）：library init | import | list。

use crate::error::{code, Error, Result};
use crate::paths::resolve_data_root;
use crate::personal_library;
use serde_json::json;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn run(
    action: &str,
    dir: Option<&Path>,
    name: Option<&str>,
    execute: bool,
    // 输出由调用方统一处理（JSON envelope 或 output::emit_human）。
    _json: bool,
    data_root: Option<&Path>,
    url: Option<&str>,
    repo_path: Option<&str>,
    git_ref: Option<&str>,
    skill: Option<&str>,
    preview_id: Option<&str>,
) -> Result<serde_json::Value> {
    let data_root = resolve_data_root(data_root)?;
    match action {
        "recover" => Ok(json!({
            "action": "recover",
            "recovery": personal_library::recover_updates(&data_root)?,
        })),
        "init" => {
            let r = personal_library::ensure_library(&data_root)?;
            Ok(json!({
                "action": "init",
                "path": r.path,
                "created": r.created,
                "note": "资源库已在机器数据区生成（仓外、离线、无需团队源或 TOML 手写）",
            }))
        }
        // AIL-120：CLI 对齐 Web 的个人副本删除（预览默认，--execute 才移入归档）
        "delete" => {
            let Some(id) = skill else {
                return Err(crate::error::Error::new(
                    crate::error::code::USAGE,
                    "delete 需要 --skill <资源ID>",
                ));
            };
            if execute {
                personal_library::delete_execute(&data_root, id)?;
                Ok(json!({
                    "action": "delete",
                    "executed": true,
                    "id": id,
                    "note": "已移入本机 library-archive（可手动移回恢复）；不删除上游或项目文件",
                }))
            } else {
                let preview = personal_library::delete_preview(&data_root, id)?;
                Ok(json!({ "action": "delete", "executed": false, "id": id, "preview": preview }))
            }
        }
        "list" => {
            // AIL-062：宽容列表——坏条目定位为 issues（文件级），不使列表失败
            let (entries, issues) = personal_library::list_tolerant(&data_root);
            let skills: Vec<String> = entries
                .iter()
                .filter(|e| e.kind == "skill")
                .map(|e| e.name.clone())
                .collect();
            Ok(json!({
                "action": "list",
                "path": personal_library::library_root(&data_root),
                "skills": skills,
                "entries": entries,
                "issues": issues,
            }))
        }
        "import" => {
            let dir = dir.ok_or_else(|| Error::new(code::USAGE, "import 需要 --dir <技能目录>"))?;
            if execute {
                let r = personal_library::import_execute(&data_root, dir, name)?;
                Ok(json!({
                    "action": "import",
                    "executed": true,
                    "skill_id": r.skill_id,
                    "target_dir": r.target_dir,
                    "files_copied": r.files_copied,
                    "scripts": r.scripts,
                    "scripts_executed": r.scripts_executed,
                    "note": r.note,
                }))
            } else {
                let p = personal_library::import_preview(&data_root, dir, name)?;
                Ok(json!({
                    "action": "import",
                    "executed": false,
                    "preview": p,
                    "note": "预览模式：未复制未执行；加 --execute 执行导入",
                }))
            }
        }
        "import-entry" => {
            // AIL-065：skills.sh 等发现入口 → 实际 GitHub 来源 → 复用导入
            let entry = url.ok_or_else(|| Error::new(code::USAGE, "import-entry 需要 --url <发现入口>"))?;
            let v = personal_library::import_via_discovery(&data_root, entry, name, execute)?;
            Ok(json!({ "action": "import-entry", "result": v }))
        }
        "import-git" => {
            // AIL-064：GitHub 仓库/子目录导入（预览默认；--execute 才复制）
            let url = url.ok_or_else(|| Error::new(code::USAGE, "import-git 需要 --url <仓库URL>"))?;
            if execute {
                let r = personal_library::git_import_execute(&data_root, url, repo_path, git_ref, name)?;
                Ok(json!({
                    "action": "import-git",
                    "executed": true,
                    "skill_id": r.skill_id,
                    "target_dir": r.target_dir,
                    "files_copied": r.files_copied,
                    "scripts": r.scripts,
                    "scripts_executed": r.scripts_executed,
                    "note": r.note,
                }))
            } else {
                let p = personal_library::git_import_preview(&data_root, url, repo_path, git_ref, name)?;
                Ok(json!({
                    "action": "import-git",
                    "executed": false,
                    "preview": p,
                    "note": "预览模式：未复制未执行；加 --execute 执行导入（仅复制，绝不运行仓库脚本）",
                }))
            }
        }
        "sources" => {
            personal_library::recover_updates(&data_root)?;
            // AIL-063：列出库内 skill 的来源身份/版本
            let lib = personal_library::library_root(&data_root);
            let mut items = Vec::new();
            let skills_dir = lib.join("resources/skills");
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                for e in entries.flatten() {
                    let d = e.path();
                    if !d.is_dir() || e.file_name().to_string_lossy().starts_with('.') {
                        continue;
                    }
                    let meta = crate::skill_source::read_meta(&d);
                    items.push(json!({
                        "skill": e.file_name().to_string_lossy(),
                        "source": meta,
                        "legacy": meta.is_none(),
                    }));
                }
            }
            Ok(json!({ "action": "sources", "items": items }))
        }
        "check-update" => {
            // AIL-066：显式检查更新（只比较，不应用）
            let skill = skill.ok_or_else(|| Error::new(code::USAGE, "check-update 需要 --skill <名称>"))?;
            let st = personal_library::check_update(&data_root, skill)?;
            Ok(json!({ "action": "check-update", "status": st }))
        }
        "update" => {
            // AIL-066：应用更新（本地未改才允许；更新前备份旧版）
            let skill = skill.ok_or_else(|| Error::new(code::USAGE, "update 需要 --skill <名称>"))?;
            if !execute {
                let st = personal_library::check_update(&data_root, skill)?;
                return Ok(json!({
                    "action": "update",
                    "executed": false,
                    "status": st,
                    "note": "预览模式：加 --execute 应用更新",
                }));
            }
            let r = match preview_id {
                Some(id) => personal_library::update_execute_checked(&data_root, skill, id)?,
                None => personal_library::update_execute(&data_root, skill)?,
            };
            Ok(json!({ "action": "update", "executed": true, "result": r }))
        }
        other => Err(Error::new(
            code::USAGE,
            format!("未知 library 动作: {other}（支持 init | import | import-git | import-entry | list | sources | check-update | update | delete | recover）"),
        )),
    }
}
