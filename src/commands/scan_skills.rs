//! AIL-111/120：项目内已有 Skill 的只读扫描（CLI 与控制台共用）。
//! 只读 SKILL.md 的 frontmatter，不跟随符号链接、不执行内容、不写任何文件。
//! 管理方式分类：managed（AILoom 部署的实体链接，指向 Store 根）/ external_link
//! （指向 Store 之外或不可判定的链接，记录目标但拒绝读取）/ unmanaged（项目自有）。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SCAN_SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".cache",
    "dist",
    "target",
    ".venv",
    "__pycache__",
];

fn scan_dir_entry(
    skill_md: &Path,
    store_root: &Path,
    project_root: &Path,
    seen: &mut Vec<PathBuf>,
    items: &mut Vec<Value>,
) {
    let dir = match skill_md.parent() {
        Some(d) => d,
        None => return,
    };
    let canon_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if seen.contains(&canon_dir) {
        return;
    }
    seen.push(canon_dir.clone());
    let dir_name = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    // 展示用原始路径（链接本身），去重用规范路径。
    let display_path = dir.display().to_string();
    let dir_meta = match std::fs::symlink_metadata(dir) {
        Ok(m) => m,
        Err(e) => {
            items.push(json!({ "path": display_path, "dir_name": dir_name,
                "management": "error", "error": format!("目录不可读: {e}") }));
            return;
        }
    };
    if dir_meta.is_symlink() {
        let target = std::fs::read_link(dir)
            .map(|p| {
                let t = if p.is_absolute() {
                    p
                } else {
                    dir.parent().unwrap_or(dir).join(p)
                };
                t.canonicalize().unwrap_or(t)
            })
            .unwrap_or_else(|_| PathBuf::new());
        if target.starts_with(store_root) || target.starts_with(project_root) {
            // AILoom 部署的实体链接：正常解析 frontmatter 供展示。
            let mut entry = json!({ "path": display_path, "dir_name": dir_name, "management": if target.starts_with(store_root) { "managed" } else { "unmanaged" } });
            fill_frontmatter(skill_md, &mut entry);
            items.push(entry);
        } else {
            items.push(json!({ "path": display_path, "dir_name": dir_name,
                "management": "external_link", "link_target": target.display().to_string(),
                "error": "符号链接指向项目外：为避免越界读取，未解析其内容" }));
        }
        return;
    }
    // 内置 Skill 以单个文件部署（目录本身不是链接）：内容与内置版本一致即为 AILoom 托管（C-18）。
    let builtin = std::fs::read_to_string(skill_md)
        .is_ok_and(|raw| crate::adapters::builtin::is_builtin_skill(&raw));
    let management = if builtin { "managed" } else { "unmanaged" };
    let mut entry = json!({ "path": display_path, "dir_name": dir_name, "management": management });
    if builtin {
        entry["note"] = json!("AILoom 内置 Skill（由同步部署）");
    }
    fill_frontmatter(skill_md, &mut entry);
    items.push(entry);
}

fn fill_frontmatter(skill_md: &Path, entry: &mut Value) {
    if std::fs::symlink_metadata(skill_md).is_ok_and(|m| m.is_symlink()) {
        entry["error"] = json!("SKILL.md 是符号链接，未读取内容");
        return;
    }
    if std::fs::metadata(skill_md).is_ok_and(|m| m.len() > 1024 * 1024) {
        entry["error"] = json!("SKILL.md 超过 1 MB，未读取内容");
        return;
    }
    let parsed = std::fs::read_to_string(skill_md)
        .map_err(|e| format!("SKILL.md 不可读: {e}"))
        .and_then(|raw| {
            crate::resource::parse_frontmatter(&raw)
                .map(|(m, _)| m)
                .map_err(|e| format!("frontmatter 解析失败: {e}"))
        });
    match parsed {
        Err(e) => {
            entry["management"] = Value::String("error".into());
            entry["error"] = serde_json::json!(e);
        }
        Ok(None) => {
            entry["management"] = Value::String("error".into());
            entry["error"] =
                Value::String("SKILL.md 缺少 frontmatter（名称/说明不可知；文件未改动）".into());
        }
        Ok(Some(meta)) => {
            let fm_name = meta.name.clone().unwrap_or_default();
            let dir_name = entry["dir_name"].as_str().unwrap_or("").to_string();
            if !fm_name.is_empty() && fm_name != dir_name {
                entry["warning"] = serde_json::json!(format!(
                    "frontmatter name `{fm_name}` 与目录名 `{dir_name}` 不一致（按目录名展示）"
                ));
                entry["name"] = serde_json::json!(dir_name);
            } else {
                entry["name"] = serde_json::json!(if fm_name.is_empty() {
                    dir_name
                } else {
                    fm_name
                });
            }
            entry["description"] = serde_json::json!(meta.description.clone().unwrap_or_default());
        }
    }
}

/// 只读扫描：宿主目录 depth≤2，用户指定 Skill 根 depth≤6；不跟随链接内容。
/// `root` 必须已通过调用方的边界校验（CLI：显式路径；控制台：已批准根 canonicalize）。
/// AIL-121 契约补充：扫描范围必须与配置作用域一致 —— 用户指定子目录后，
/// 该子目录内的宿主目录（.claude/.agents）与嵌套 Skill 都要能被发现；
/// 深度 6 覆盖 `sub/.claude/skills/<skill>/SKILL.md`（4 层）与一层分类目录。
pub fn scan_project_skills(root: &Path, sub: Option<&str>, store_root: &Path) -> Value {
    let mut scan_roots: Vec<(PathBuf, usize)> = vec![
        (root.join(".claude/skills"), 3),
        (root.join(".agents/skills"), 3),
        (root.join(".codex/skills"), 3),
        (root.join(".cursor/skills"), 3),
        (root.join(".grok/skills"), 3),
        (root.join(".pi/skills"), 3),
        (root.join(".opencode/skills"), 3),
    ];
    if let Some(sub) = sub.map(str::trim).filter(|s| !s.is_empty()) {
        let joined = root.join(sub.trim_start_matches('/'));
        let canon = match joined.canonicalize() {
            Ok(c) if c.starts_with(root) && c.is_dir() => c,
            _ => {
                return json!({ "error": format!("Skill 根不存在或不是目录：{sub}") });
            }
        };
        scan_roots.push((canon, 6));
    }
    let mut items: Vec<Value> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for (base, max_depth) in scan_roots {
        if !base.is_dir() {
            continue;
        }
        let scan_base = match base.canonicalize() {
            Ok(p) if p.starts_with(root) => p,
            _ => {
                items.push(json!({"path":base,"dir_name":base.file_name().unwrap_or_default().to_string_lossy(),
                    "management":"external_link","error":"Skill 目录链接到项目外，未展开内容"}));
                continue;
            }
        };
        let walker = walkdir::WalkDir::new(&scan_base)
            .follow_links(false)
            .max_depth(max_depth)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if e.depth() == 0 {
                    return true;
                }
                // 宿主目录是 Skill 的合法存放位置；其余点前缀目录（.git 等）跳过。
                if [
                    ".claude",
                    ".agents",
                    ".codex",
                    ".cursor",
                    ".grok",
                    ".pi",
                    ".opencode",
                ]
                .contains(&name.as_str())
                {
                    return true;
                }
                !(name.starts_with('.') || SCAN_SKIP_DIRS.contains(&name.as_str()))
            });
        for entry in walker.flatten() {
            let ft = entry.file_type();
            if ft.is_symlink() {
                // 符号链接目录可能正是一个 Skill（含宿主托管的实体链接）：
                // 只做分类与存在性判定，不读取链接目标的内容。
                let probe = entry.path().join("SKILL.md");
                if !probe.exists() {
                    continue;
                }
                scan_dir_entry(&probe, store_root, root, &mut seen, &mut items);
                continue;
            }
            if ft.is_dir() || entry.file_name() != "SKILL.md" {
                continue;
            }
            scan_dir_entry(entry.path(), store_root, root, &mut seen, &mut items);
        }
    }
    items.sort_by(|a, b| {
        a["path"]
            .as_str()
            .unwrap_or("")
            .cmp(b["path"].as_str().unwrap_or(""))
    });
    json!({ "items": items })
}

#[cfg(test)]
mod tests {
    #[test]
    fn builtin_skill_file_is_reported_as_managed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let dir = root.join(".claude/skills/ailoom-share-learning");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            include_str!("../adapters/res/share-learning-skill.md"),
        )
        .unwrap();
        let v = scan_project_skills(&root, None, &root.join("store"));
        assert_eq!(v["items"][0]["management"], "managed", "{v}");
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: ailoom-share-learning\ndescription: 改过\n---\n",
        )
        .unwrap();
        let v = scan_project_skills(&root, None, &root.join("store"));
        assert_eq!(
            v["items"][0]["management"], "unmanaged",
            "被用户改过就不再算托管: {v}"
        );
    }

    use super::*;
    use std::fs;

    /// AIL-121 契约回归：子目录扫描必须发现该目录内的宿主 Skill
    /// （web/.claude/skills/<name>/SKILL.md），且不越界、不影响兄弟目录。
    #[test]
    fn sub_scan_discovers_host_skills_and_respects_boundary() {
        let tmp = std::env::temp_dir().join(format!("ailoom-scan-test-{}", std::process::id()));
        let root = tmp.join("repo");
        let host_skills = root.join("web/.claude/skills/old-notes");
        fs::create_dir_all(&host_skills).unwrap();
        fs::write(
            host_skills.join("SKILL.md"),
            "---\nname: old-notes\ndescription: 旧笔记\n---\n正文",
        )
        .unwrap();
        let sibling = root.join("docs");
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("SKILL.md"), "---\nname: nope\n---\n").unwrap();
        let plain = root.join("web/docs/deep");
        fs::create_dir_all(&plain).unwrap();
        fs::write(root.join("web/docs/deep/note.md"), "普通文档，不是 Skill").unwrap();

        let store = tmp.join("store");
        fs::create_dir_all(&store).unwrap();
        // 契约要求 root 已规范（/tmp 是符号链接，必须 canonicalize 后传入）。
        let root = root.canonicalize().unwrap();

        // 子目录扫描：发现 web 内宿主 Skill
        let v = scan_project_skills(&root, Some("web"), &store);
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "应恰好发现 old-notes：{items:?}");
        assert_eq!(items[0]["dir_name"], "old-notes");
        assert_eq!(items[0]["management"], "unmanaged");

        // 根扫描（无 sub）：不进入 web/.claude（点目录剪枝，行为与既有语义一致）
        let v0 = scan_project_skills(&root, None, &store);
        assert_eq!(v0["items"].as_array().unwrap().len(), 0);

        fs::remove_dir_all(&tmp).ok();
    }
}
