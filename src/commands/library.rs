//! 个人资源库命令（AIL-043）：library init | import | list。

use crate::error::{code, Error, Result};
use crate::paths::resolve_data_root;
use crate::personal_library;
use serde_json::json;
use std::path::Path;

pub fn run(
    action: &str,
    dir: Option<&Path>,
    name: Option<&str>,
    execute: bool,
    json: bool,
    data_root: Option<&Path>,
) -> Result<serde_json::Value> {
    let _ = json;
    let data_root = resolve_data_root(data_root)?;
    match action {
        "init" => {
            let r = personal_library::ensure_library(&data_root)?;
            Ok(json!({
                "action": "init",
                "path": r.path,
                "created": r.created,
                "note": "个人库已在机器数据区生成（仓外、离线、无需团队源或 TOML 手写）",
            }))
        }
        "list" => {
            let l = personal_library::list(&data_root)?;
            Ok(json!({
                "action": "list",
                "path": l.path,
                "skills": l.skills,
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
        other => Err(Error::new(
            code::USAGE,
            format!("未知 library 动作: {other}（支持 init | import | list）"),
        )),
    }
}
