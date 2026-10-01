//! Native rule/agent editing, including recoverable Git-local replacements.
mod local;
use crate::error::{code, Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

#[derive(Default, Deserialize)]
pub struct Request {
    pub action: String,
    pub scope: String,
    pub root: Option<PathBuf>,
    pub target: Option<String>,
    pub name: Option<String>,
    pub content: Option<String>,
    pub expected: Option<String>,
    pub mode: Option<String>,
}
struct Target {
    id: String,
    tool: &'static str,
    kind: &'static str,
    path: PathBuf,
    ext: &'static str,
    fixed: bool,
    note: &'static str,
}
fn fail(text: &str) -> Error {
    Error::new(code::USER_CONTENT_CONFLICT, text)
}
fn env_path(key: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(key)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or(fallback)
}
fn targets(home: &Path, r: &Request) -> Result<Vec<Target>> {
    let global = match r.scope.as_str() {
        "global" => true,
        "project" => false,
        _ => return Err(fail("请选择全局或项目范围")),
    };
    let base = if global {
        home.to_path_buf()
    } else {
        r.root
            .as_ref()
            .ok_or_else(|| fail("请选择项目目录"))?
            .canonicalize()?
    };
    let claude = if global {
        env_path("CLAUDE_CONFIG_DIR", home.join(".claude"))
    } else {
        base.join(".claude")
    };
    let codex = if global {
        env_path("CODEX_HOME", home.join(".codex"))
    } else {
        base.clone()
    };
    let pi = if global {
        env_path("PI_CODING_AGENT_DIR", home.join(".pi/agent"))
    } else {
        base.join(".pi")
    };
    let oc = if global {
        env_path("XDG_CONFIG_HOME", home.join(".config")).join("opencode")
    } else {
        base.join(".opencode")
    };
    let mut out = Vec::new();
    let mut add = |id: &str, tool, kind, path: PathBuf, ext, fixed, note| {
        out.push(Target {
            id: id.into(),
            tool,
            kind,
            path,
            ext,
            fixed,
            note,
        })
    };
    add(
        "claude-rules",
        "Claude Code",
        "rule",
        claude.join("rules"),
        "md",
        false,
        "",
    );
    add(
        "claude-agents",
        "Claude Code",
        "agent",
        claude.join("agents"),
        "md",
        false,
        "",
    );
    add(
        "claude-instructions",
        "Claude Code",
        "rule",
        if global {
            claude.join("CLAUDE.md")
        } else {
            base.join("CLAUDE.md")
        },
        "md",
        true,
        "",
    );
    if !global {
        add(
            "claude-dot-instructions",
            "Claude Code",
            "rule",
            claude.join("CLAUDE.md"),
            "md",
            true,
            "",
        );
    }
    add(
        "codex-instructions",
        "Codex / 通用",
        "rule",
        codex.join("AGENTS.md"),
        "md",
        true,
        "",
    );
    add(
        "codex-agents",
        "Codex",
        "agent",
        if global {
            codex.join("agents")
        } else {
            base.join(".codex/agents")
        },
        "toml",
        false,
        "",
    );
    add(
        "cursor-agents",
        "Cursor",
        "agent",
        base.join(".cursor/agents"),
        "md",
        false,
        "",
    );
    if !global {
        add(
            "cursor-rules",
            "Cursor",
            "rule",
            base.join(".cursor/rules"),
            "mdc",
            false,
            "",
        );
    }
    add(
        "pi-agents",
        "Pi",
        "agent",
        pi.join("agents"),
        "md",
        false,
        "需官方 subagent 扩展；项目代理需启用 project/both 范围",
    );
    add(
        "pi-instructions",
        "Pi",
        "rule",
        pi.join("APPEND_SYSTEM.md"),
        "md",
        true,
        "",
    );
    add(
        "opencode-agents",
        "OpenCode",
        "agent",
        oc.join("agents"),
        "md",
        false,
        "",
    );
    if global {
        add(
            "opencode-instructions",
            "OpenCode",
            "rule",
            oc.join("AGENTS.md"),
            "md",
            true,
            "",
        );
    }
    if !global {
        add(
            "grok-rules",
            "Grok",
            "rule",
            base.join(".grok/rules"),
            "md",
            false,
            "",
        );
        add(
            "grok-agents",
            "Grok",
            "agent",
            base.join(".grok/agents"),
            "md",
            false,
            "",
        );
    }
    Ok(out)
}
// Reject links at every component so a rule entry cannot access unrelated local files.
fn check_path(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for c in path.components() {
        if matches!(c, Component::ParentDir) {
            return Err(fail("路径不能包含 .."));
        }
        current.push(c);
        match std::fs::symlink_metadata(&current) {
            Ok(m) if m.file_type().is_symlink() => return Err(fail("符号链接文件请在原位置管理")),
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn file_path(t: &Target, name: &str) -> Result<PathBuf> {
    if t.fixed {
        if !name.is_empty() {
            return Err(fail("此说明文件使用固定名称"));
        }
        return Ok(t.path.clone());
    }
    let rel = Path::new(name);
    if name.is_empty()
        || rel.components().any(|c| !matches!(c, Component::Normal(_)))
        || rel.extension().and_then(|s| s.to_str()) != Some(t.ext)
    {
        return Err(fail("文件名或扩展名无效"));
    }
    Ok(t.path.join(rel))
}
fn read(path: &Path) -> Result<Option<String>> {
    check_path(path)?;
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(m) => {
            if !m.is_file() || m.len() > 1024 * 1024 {
                return Err(fail("仅支持 1 MB 以内的文本文件"));
            }
            Ok(Some(std::fs::read_to_string(path)?))
        }
    }
}
fn version(content: &Option<String>) -> String {
    content
        .as_ref()
        .map(|s| crate::ids::sha256_hex(s.as_bytes()))
        .unwrap_or_else(|| "missing".into())
}
fn managed_paths(data: &Path, root: Option<&Path>) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if let Some(root) = root {
        for ancestor in root.ancestors().take(12) {
            let id = crate::ids::workspace_id_from_root(ancestor);
            let file = data.join("ws").join(id).join("managed-manifest.json");
            if let Some(manifest) = crate::sync::manifest::ManagedManifest::load(&file)? {
                for key in manifest.items.keys().filter(|k| !k.contains('#')) {
                    paths.push(ancestor.join(key));
                }
            }
        }
    }
    Ok(paths)
}
fn creation_record(data: &Path, path: &Path) -> PathBuf {
    data.join("native-files/created")
        .join(crate::ids::sha256_hex(path.to_string_lossy().as_bytes()))
}
pub fn run(data: &Path, home: &Path, r: &Request) -> Result<Value> {
    let targets = targets(home, r)?;
    if r.action == "list" {
        let managed = managed_paths(
            data,
            if r.scope == "project" {
                r.root.as_deref()
            } else {
                None
            },
        )?;
        let pending = local::pending_paths(data)?;
        let mut files = Vec::new();
        let mut warnings = Vec::new();
        for t in &targets {
            if let Err(e) = check_path(&t.path) {
                warnings.push(format!("{}: {e}", t.path.display()));
                continue;
            }
            let mut paths: Vec<PathBuf> = if t.fixed {
                if t.path.exists() {
                    vec![t.path.clone()]
                } else {
                    Vec::new()
                }
            } else if !t.path.exists() {
                Vec::new()
            } else {
                let mut paths = Vec::new();
                for e in walkdir::WalkDir::new(&t.path)
                    .follow_links(false)
                    .max_depth(10)
                {
                    match e {
                        Ok(e) if e.file_type().is_symlink() => {
                            warnings.push(format!("符号链接：{}", e.path().display()))
                        }
                        Ok(e)
                            if e.file_type().is_file()
                                && e.path().extension().and_then(|s| s.to_str()) == Some(t.ext) =>
                        {
                            paths.push(e.into_path())
                        }
                        Ok(_) => (),
                        Err(e) => warnings.push(e.to_string()),
                    }
                    if paths.len() > 2000 {
                        return Err(fail("文件过多，请缩小项目范围"));
                    }
                }
                paths
            };
            for path in &pending {
                let allowed = if t.fixed {
                    path == &t.path
                } else {
                    path.strip_prefix(&t.path)
                        .ok()
                        .is_some_and(|p| file_path(t, &p.to_string_lossy()).is_ok())
                };
                if allowed && !paths.contains(path) {
                    paths.push(path.clone());
                }
            }
            for path in paths {
                let name = if t.fixed {
                    String::new()
                } else {
                    path.strip_prefix(&t.path)?.to_string_lossy().into_owned()
                };
                files.push(json!({"target":t.id,"tool":t.tool,"kind":t.kind,"name":name,
                    "missing":!path.exists(),"local_only":local::active(data,&path)?,"managed":managed.contains(&path),"created_by_ailoom":creation_record(data,&path).is_file(),
                    "path":path,"label":path.file_name().unwrap_or_default().to_string_lossy(),"note":t.note}));
            }
        }
        files.sort_by_key(|f| f["path"].as_str().unwrap_or_default().to_owned());
        return Ok(
            json!({"git_project":r.scope=="project" && r.root.as_deref().and_then(local::repo).is_some(),"files":files,"targets":targets.iter().map(|t|json!({
            "id":t.id,"tool":t.tool,"kind":t.kind,"path":t.path,"ext":t.ext,"fixed":t.fixed,"note":t.note
        })).collect::<Vec<_>>(),"warnings":warnings,
        "limitations":["Cursor 全局 Rules 请在 Cursor 设置中管理","Grok 全局文件暂未接入"]}),
        );
    }
    let t = targets
        .iter()
        .find(|t| Some(&t.id) == r.target.as_ref())
        .ok_or_else(|| fail("未知工具目录"))?;
    let path = file_path(t, r.name.as_deref().unwrap_or(""))?;
    // Serialize compare-and-write across requests, including different browser tabs.
    std::fs::create_dir_all(data.join("native-files"))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data.join("native-files/operation.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let content = read(&path)?;
    if r.action == "read" {
        return Ok(
            json!({"content":content,"expected":version(&content),"path":path,
            "local":local::info(data,if r.scope=="project"{r.root.as_deref()}else{None},&path)?}),
        );
    }
    if !matches!(r.action.as_str(), "save" | "delete" | "restore") {
        return Err(fail("未知操作"));
    }
    if r.expected.as_deref() != Some(&version(&content)) {
        return Err(fail("文件已变化，请重新打开后再保存"));
    }
    let active = local::active(data, &path)?;
    let mode = r
        .mode
        .as_deref()
        .unwrap_or(if active { "local" } else { "project" });
    if !matches!(mode, "local" | "project") {
        return Err(fail("未知保存方式"));
    }
    if r.action == "delete" && active {
        return Err(fail("请先恢复项目版本，再删除文件"));
    }
    if r.action == "save" && mode == "local" {
        let repo = r
            .root
            .as_deref()
            .filter(|_| r.scope == "project")
            .and_then(local::repo)
            .ok_or_else(|| fail("仅本机生效需要 Git 项目"))?;
        if data.canonicalize()?.starts_with(repo) {
            return Err(fail("本机副本需要保存在项目外的 AILoom 数据目录"));
        }
        if managed_paths(data, r.root.as_deref())?.contains(&path) {
            return Err(fail("此文件由资源库应用生成，请先在项目中移除托管关联"));
        }
    }
    if r.action == "delete" && content.is_none() {
        return Err(fail("文件不存在"));
    }
    let next = if r.action == "save" {
        let s = r.content.as_deref().ok_or_else(|| fail("缺少文件内容"))?;
        if s.len() > 1024 * 1024 {
            return Err(fail("内容超过 1 MB"));
        }
        Some(s)
    } else {
        None
    };
    if let Some(old) = &content {
        let backup = data.join("native-files/backups").join(crate::ids::new_id());
        std::fs::create_dir_all(&backup)?;
        crate::sync_common::atomic_write(&backup.join("content"), old.as_bytes())?;
        crate::sync_common::atomic_write(
            &backup.join("metadata.json"),
            &serde_json::to_vec(&json!({"path":path,"action":r.action}))?,
        )?;
    }
    if r.action == "restore" {
        local::restore(data, &path)?;
        return Ok(json!({"restored":true,"path":path}));
    }
    if let Some(next) = next {
        if mode == "local" {
            local::save(
                data,
                r.root.as_deref().unwrap(),
                &path,
                next,
                r.expected.as_deref().unwrap(),
            )?;
            return Ok(json!({"saved":true,"path":path,"local_only":true}));
        }
        if active {
            local::restore(data, &path)?;
        }
    }
    if let Some(next) = next {
        crate::sync_common::atomic_write(&path, next.as_bytes())?;
        if content.is_none() {
            crate::sync_common::atomic_write(
                &creation_record(data, &path),
                path.to_string_lossy().as_bytes(),
            )?;
        }
    } else {
        std::fs::remove_file(&path)?;
        let record = creation_record(data, &path);
        if record.exists() {
            std::fs::remove_file(record)?;
        }
    }
    Ok(json!({"saved":true,"path":path}))
}
