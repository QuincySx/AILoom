//! CC Switch 来源迁移。用户点击扫描时，只查询 skills 表的来源字段。
//! 扫描离线；预览从原 Git 仓库拉取；确认后复用合集的原子登记与更新机制。
use crate::collections;
use crate::error::{code, Error, Result};
use crate::source::GitSource;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 仅计算默认路径，不在页面加载或启动时访问数据库。
pub fn default_directory() -> Result<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".cc-switch"))
        .ok_or_else(|| invalid("无法确定 CC Switch 默认目录，请选择数据文件夹"))
}

fn sqlite_program() -> &'static str {
    if cfg!(target_os = "macos") {
        "/usr/bin/sqlite3"
    } else {
        "sqlite3"
    }
}

/// 产品内的用户发起只读操作。调用者必须验证目录权限和显式读取确认。
/// 不导出整库、不查询 providers/settings，不加载 .sqliterc 或扩展。
pub fn scan_directory(data: &Path, directory: &Path) -> Result<Value> {
    let directory = directory
        .canonicalize()
        .map_err(|_| invalid("CC Switch 数据目录不存在，请选择正确的数据文件夹"))?;
    let database = directory.join("cc-switch.db");
    let metadata = std::fs::symlink_metadata(&database)
        .map_err(|_| invalid("此目录没有 CC Switch 数据库，请选择数据目录而不是 skills 文件夹"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("CC Switch 数据库必须是普通文件，不能是符号链接"));
    }
    // 查询文本固定，用户路径仅作为独立进程参数传入。safe 禁用 ATTACH/readfile/load_extension。
    // 显式空 init 防止加载用户 SQL 启动脚本；readonly 禁止改写数据库。
    let query = |sql: &str| -> Result<Value> {
        let output = std::process::Command::new(sqlite_program())
            .args([
                "-init",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
                "-safe",
                "-readonly",
                "-nofollow",
                "-json",
                "-cmd",
                ".timeout 3000",
                "-cmd",
                "PRAGMA trusted_schema=OFF;",
            ])
            .arg(&database)
            .arg(sql)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|_| {
                invalid("无法启动 SQLite 只读组件；macOS 使用系统组件，其他平台需提供 sqlite3")
            })?;
        if !output.status.success() {
            // 不回显 SQLite stderr，避免损坏文件或 schema 内容被作为诊断泄露。
            return Err(invalid("无法读取 Skill 来源：数据库忙、版本不兼容或文件损坏。请关闭 CC Switch 后重试，或选择正确的数据目录"));
        }
        if output.stdout.len() > 2 * 1024 * 1024 {
            return Err(invalid("来源记录过大，请分批迁移"));
        }
        if output.stdout.is_empty() {
            return Ok(json!([]));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|_| invalid("SQLite 组件未返回有效的来源数据"))
    };
    let schema = query("SELECT type, sql FROM sqlite_schema WHERE name = 'skills';")?;
    if schema.as_array().map_or(true, |rows| rows.len() != 1)
        || schema[0]["type"] != "table"
        || schema[0]["sql"]
            .as_str()
            .unwrap_or("")
            .to_ascii_uppercase()
            .contains("VIRTUAL TABLE")
    {
        return Err(invalid(
            "此数据库没有受支持的 skills 表；不会读取其他表或视图",
        ));
    }
    let rows = query("SELECT id, name, directory, repo_owner, repo_name, repo_branch, readme_url FROM skills ORDER BY name LIMIT 1001;")?;
    if rows.as_array().is_some_and(Vec::is_empty) {
        return Err(invalid("CC Switch 尚未登记任何 Skill 来源"));
    }
    let mut result = scan(data, &rows)?;
    result["directory"] = json!(directory);
    Ok(result)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillOrigin {
    pub original_id: String,
    pub name: String,
    pub directory: String,
    pub discovery_entry: String,
    pub repo_url: String,
    pub branch: Option<String>,
    pub repo_path: Option<String>,
    pub resource_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationOrigin {
    pub provider: String,
    pub imported_at: String,
    pub skills: Vec<SkillOrigin>,
}

#[derive(Serialize, Deserialize)]
struct ScanItem {
    id: String,
    name: String,
    source: Option<SkillOrigin>,
    error: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Scan {
    items: Vec<ScanItem>,
}

#[derive(Serialize, Deserialize)]
struct Prepared {
    preview_ids: Vec<String>,
    groups: Vec<Value>,
}

fn invalid(message: &str) -> Error {
    Error::new(code::USAGE, message)
}

fn field<'a>(row: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| row[*k].as_str().map(str::trim).filter(|s| !s.is_empty()))
}

fn coordinate(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn relative(s: &str) -> Result<String> {
    let s = s.trim_end_matches('/');
    if s == "." || s.is_empty() {
        return Ok(".".into());
    }
    if s.starts_with('/')
        || s.contains(['\\', ':', '%'])
        || s.chars().any(char::is_control)
        || s.split('/').any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(invalid("仓内路径必须是安全的相对路径"));
    }
    Ok(s.into())
}

fn remote(url: &str) -> Result<String> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) || url.contains(['?', '#', '%']) {
        return Err(invalid("仓库地址不能包含空白、查询参数或编码路径"));
    }
    let (host, path) = if let Some(rest) = url.strip_prefix("https://") {
        let (host, path) = rest
            .split_once('/')
            .ok_or_else(|| invalid("仓库地址缺少路径"))?;
        if host.contains('@') {
            return Err(invalid("HTTPS 地址不能包含凭据"));
        }
        (host, path)
    } else if let Some(rest) = url.strip_prefix("ssh://git@") {
        rest.split_once('/')
            .ok_or_else(|| invalid("SSH 仓库地址缺少路径"))?
    } else if let Some(rest) = url.strip_prefix("git@") {
        rest.split_once(':')
            .ok_or_else(|| invalid("SSH 仓库地址缺少路径"))?
    } else {
        return Err(invalid(
            "来源须为 HTTPS 或 Git SSH 仓库地址，不接受本地路径或 Git helper",
        ));
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
    {
        return Err(invalid("仓库域名无效"));
    }
    relative(path)?;
    GitSource::new(url, None)?;
    Ok(url.trim_end_matches('/').into())
}

fn normalize(row: &Value) -> Result<SkillOrigin> {
    if !row.is_object() {
        return Err(invalid("每个 Skill 必须是一条来源记录"));
    }
    let name = field(row, &["name", "directory"]).ok_or_else(|| invalid("Skill 缺少名称"))?;
    let directory = field(row, &["directory"]).unwrap_or(name);
    let mut branch = field(row, &["repo_branch", "repoBranch", "ref"]).map(str::to_string);
    let mut path = field(row, &["repo_path", "repoPath"])
        .map(relative)
        .transpose()?;
    let owner = field(row, &["repo_owner", "repoOwner"]);
    let repo = field(row, &["repo_name", "repoName"]);
    let explicit = field(row, &["repo_url", "repoUrl"]);
    let entry = field(row, &["readme_url", "readmeUrl", "source_url", "sourceUrl"]);
    let mut url = match (owner, repo) {
        (Some(o), Some(r)) if coordinate(o) && coordinate(r) => Some(format!(
            "https://github.com/{o}/{}",
            r.trim_end_matches(".git")
        )),
        (None, None) => None,
        _ => return Err(invalid("GitHub 仓库 owner/name 不完整或无效")),
    };
    if let Some(explicit) = explicit {
        let resolved = remote(explicit)?;
        // 显式 SSH 地址保留协议；owner/name 仍必须指向同一 GitHub 仓库。
        if let Some(expected) = &url {
            let suffix = expected.trim_start_matches("https://github.com/");
            let actual = resolved
                .strip_prefix("https://github.com/")
                .or_else(|| resolved.strip_prefix("git@github.com:"))
                .or_else(|| resolved.strip_prefix("ssh://git@github.com/"));
            if actual.map(|s| s.trim_end_matches(".git")) != Some(suffix) {
                return Err(invalid("来源记录中的仓库地址互相冲突"));
            }
        }
        url = Some(resolved);
    }
    if let Some(entry) = entry {
        if let Some(rest) = entry
            .strip_prefix("https://skills.sh/")
            .or_else(|| entry.strip_prefix("https://skill.sh/"))
        {
            let parts: Vec<_> = rest.trim_end_matches('/').split('/').collect();
            if !(2..=3).contains(&parts.len()) || !parts.iter().all(|s| coordinate(s)) {
                return Err(invalid("skills.sh 来源格式无效"));
            }
            let derived = format!("https://github.com/{}/{}", parts[0], parts[1]);
            if url.is_none() {
                url = Some(derived);
            } else if repo_key(url.as_ref().unwrap()) != repo_key(&derived) {
                return Err(invalid("skills.sh 入口与记录仓库不一致"));
            }
        } else if let Some(rest) = entry.strip_prefix("https://github.com/") {
            let parts: Vec<_> = rest.trim_end_matches('/').split('/').collect();
            if parts.len() < 2 || !coordinate(parts[0]) || !coordinate(parts[1]) {
                return Err(invalid("GitHub 来源链接无效"));
            }
            let derived = format!(
                "https://github.com/{}/{}",
                parts[0],
                parts[1].trim_end_matches(".git")
            );
            if let Some(url) = &url {
                if repo_key(url) != repo_key(&derived) {
                    return Err(invalid("文档链接与记录仓库不一致"));
                }
            } else {
                url = Some(derived);
            }
            if parts.len() > 2 {
                if parts.len() < 4 || !["blob", "tree"].contains(&parts[2]) {
                    return Err(invalid("来源链接须指向仓库或 blob/tree 路径"));
                }
                let tail = parts[3..].join("/");
                let repo_path = if let Some(b) = &branch {
                    tail.strip_prefix(&format!("{b}/"))
                        .ok_or_else(|| {
                            invalid("文档链接分支与记录分支不一致；请修正来源记录或链接")
                        })?
                        .to_string()
                } else {
                    let (b, p) = tail
                        .split_once('/')
                        .ok_or_else(|| invalid("文档链接缺少 Skill 路径"))?;
                    branch = Some(b.into());
                    p.into()
                };
                let parsed = relative(repo_path.strip_suffix("/SKILL.md").unwrap_or(
                    if repo_path == "SKILL.md" {
                        "."
                    } else {
                        &repo_path
                    },
                ))?;
                if path.as_ref().is_some_and(|p| p != &parsed) {
                    return Err(invalid("仓内路径与文档链接冲突"));
                }
                path = Some(parsed);
            }
        } else {
            let derived = remote(entry)?;
            if url
                .as_ref()
                .is_some_and(|u| repo_key(u) != repo_key(&derived))
            {
                return Err(invalid("来源链接与仓库地址不一致"));
            }
            url = Some(derived);
        }
    }
    let url = remote(&url.ok_or_else(|| invalid("缺少上游来源；不会按本地目录猜测仓库"))?)?;
    if branch.as_ref().is_some_and(|b| {
        b.starts_with('-')
            || b.contains(['~', '^', ':', '?', '*', '[', '\\'])
            || b.contains("..")
            || b.contains("@{")
            || b.chars().any(|c| c.is_whitespace() || c.is_control())
    }) {
        return Err(invalid("分支或版本格式无效"));
    }
    Ok(SkillOrigin {
        original_id: field(row, &["id"]).unwrap_or(directory).into(),
        name: name.into(),
        directory: directory.into(),
        discovery_entry: entry
            .or(explicit)
            .map(str::to_string)
            .unwrap_or_else(|| url.clone()),
        repo_url: url,
        branch,
        repo_path: path,
        resource_id: None,
    })
}

// GitHub 的 HTTPS/SSH 与 .git 后缀合并为同一仓库，但保留实际拉取协议。
fn repo_key(url: &str) -> String {
    if let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
    {
        return format!(
            "github.com/{}",
            rest.trim_end_matches('/')
                .trim_end_matches(".git")
                .to_ascii_lowercase()
        );
    }
    crate::gitx::normalize_remote_url(url)
        .trim_end_matches(".git")
        .into()
}

fn file(data: &Path, stage: &str, id: &str) -> Result<PathBuf> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(invalid("迁移预览 ID 无效"));
    }
    Ok(data
        .join("migrations/cc-switch")
        .join(stage)
        .join(format!("{id}.json")))
}

fn save(data: &Path, stage: &str, id: &str, value: &impl Serialize) -> Result<()> {
    let path = file(data, stage, id)?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    crate::sync_common::atomic_write(&path, &serde_json::to_vec_pretty(value)?)
}

/// 清单只允许 Skill 数组或 {skills:[...]}，绝不接受整库备份。
pub fn scan(data: &Path, input: &Value) -> Result<Value> {
    let rows = input
        .as_array()
        .or_else(|| {
            let object = input.as_object()?;
            if object.keys().any(|k| k != "skills") {
                return None;
            }
            object.get("skills")?.as_array()
        })
        .ok_or_else(|| {
            invalid("请提供仅含 Skill 来源的 JSON 数组，不要上传 CC Switch 数据库或完整配置")
        })?;
    if rows.is_empty() || rows.len() > 1000 {
        return Err(invalid("一次扫描需要 1–1000 个 Skill"));
    }
    let items = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let result = normalize(row);
            ScanItem {
                id: i.to_string(),
                name: field(row, &["name", "directory"])
                    .unwrap_or("未命名 Skill")
                    .into(),
                error: result.as_ref().err().map(ToString::to_string),
                source: result.ok(),
            }
        })
        .collect();
    let scan = Scan { items };
    let id = crate::ids::new_id();
    save(data, "scans", &id, &scan)?;
    Ok(json!({"scan_id":id, "items":scan.items}))
}

fn match_skill(skill: &mut SkillOrigin, resources: &[Value]) -> Result<()> {
    let candidates: Vec<_> = resources
        .iter()
        .filter(|r| {
            if r["kind"] != "skill" {
                return false;
            }
            let path = r["path"].as_str().unwrap_or("");
            if let Some(wanted) = &skill.repo_path {
                // 根 Skill 的合集路径是内部快照绝对路径。
                return path == wanted || (wanted == "." && Path::new(path).is_absolute());
            }
            r["name"].as_str() == Some(&skill.name)
                || Path::new(path).file_name().and_then(|s| s.to_str())
                    == Some(skill.directory.as_str())
        })
        .collect();
    if candidates.len() != 1 {
        return Err(invalid(
            "上游 Skill 无匹配或匹配不唯一，请补充准确 repo_path 后重试",
        ));
    }
    let entry = candidates[0];
    skill.repo_path = Some(
        if Path::new(entry["path"].as_str().unwrap()).is_absolute() {
            ".".into()
        } else {
            entry["path"].as_str().unwrap().into()
        },
    );
    skill.resource_id = entry["id"].as_str().map(str::to_string);
    Ok(())
}

pub fn prepare(data: &Path, scan_id: &str, selected: &[String]) -> Result<Value> {
    let scan: Scan = serde_json::from_slice(&std::fs::read(file(data, "scans", scan_id)?)?)?;
    let ids: BTreeSet<_> = selected.iter().cloned().collect();
    if ids.is_empty()
        || ids.len() != selected.len()
        || ids
            .iter()
            .any(|id| !scan.items.iter().any(|s| &s.id == id && s.source.is_some()))
    {
        return Err(invalid("请选择有效且不重复的 Skill 来源"));
    }
    let mut groups: BTreeMap<String, Vec<SkillOrigin>> = BTreeMap::new();
    for item in scan.items.into_iter().filter(|i| ids.contains(&i.id)) {
        let source = item.source.unwrap();
        groups
            .entry(repo_key(&source.repo_url))
            .or_default()
            .push(source);
    }
    if groups.len() > 100 {
        return Err(invalid("一次最多迁移 100 个仓库"));
    }
    let registry = collections::load(data)?;
    let mut prepared = Prepared {
        preview_ids: vec![],
        groups: vec![],
    };
    for (key, mut skills) in groups {
        let url = skills[0].repo_url.clone();
        let result = (|| -> Result<Value> {
            let branch = skills[0].branch.clone();
            if skills.iter().any(|s| s.branch != branch) {
                return Err(invalid(
                    "同一仓库的 Skill 使用不同分支，当前合集只能锁定一个分支；请分开选择",
                ));
            }
            if let Some(existing) = registry
                .sources
                .values()
                .find(|s| s.external_path.is_none() && repo_key(&s.url) == key)
            {
                if existing.lock.ref_ != branch {
                    return Err(invalid("资源中心已有该仓库，但分支不同；不会覆盖已有来源"));
                }
                let cat = collections::catalog(data, existing)?;
                let resources: Vec<Value> = cat.entries.iter().map(|e| json!({"kind":e.id.kind.as_str(),"name":e.id.name,"path":e.path,"id":e.id.to_string()})).collect();
                for skill in &mut skills {
                    match_skill(skill, &resources)?;
                }
                return Ok(
                    json!({"state":"existing", "source_id":existing.id, "commit":existing.lock.resolved_commit, "skills":skills}),
                );
            }
            let preview = collections::preview(
                data,
                &format!("CC Switch · {}", url.rsplit('/').next().unwrap_or("skills")),
                &url,
                branch.as_deref(),
                None,
            )?;
            for skill in &mut skills {
                match_skill(skill, preview["resources"].as_array().unwrap())?;
            }
            let token = preview["preview_id"].as_str().unwrap();
            collections::annotate_migration(
                data,
                token,
                MigrationOrigin {
                    provider: "cc-switch".into(),
                    imported_at: crate::ids::now_iso(),
                    skills: skills.clone(),
                },
            )?;
            prepared.preview_ids.push(token.into());
            Ok(
                json!({"state":"ready", "source_id":preview["source"]["id"], "commit":preview["source"]["lock"]["resolved_commit"], "skills":skills, "resource_count":preview["resources"].as_array().unwrap().len()}),
            )
        })();
        let mut group = result
            .unwrap_or_else(|e| json!({"state":"error", "error":e.to_string(), "skills":skills}));
        group["url"] = json!(url);
        prepared.groups.push(group);
    }
    let id = crate::ids::new_id();
    save(data, "previews", &id, &prepared)?;
    Ok(json!({"preview_id":id,"groups":prepared.groups,"ready":prepared.preview_ids.len()}))
}

pub fn apply(data: &Path, id: &str) -> Result<Value> {
    let prepared: Prepared = serde_json::from_slice(&std::fs::read(file(data, "previews", id)?)?)?;
    if prepared.preview_ids.is_empty() {
        return Err(invalid("没有可以登记的来源"));
    }
    // 复用合集事务：版本过期则全部拒绝，不会半途覆盖其他导入。
    let result = collections::apply_previews(data, &prepared.preview_ids)?;
    Ok(
        json!({"updated":result["updated"],"groups":prepared.groups,"note":"来源已登记；CC Switch 文件与所有项目均未修改"}),
    )
}

pub fn prepare_external(
    data: &Path,
    scan_id: &str,
    selected: &[String],
    skills_root: &Path,
) -> Result<Value> {
    let scan: Scan = serde_json::from_slice(&std::fs::read(file(data, "scans", scan_id)?)?)?;
    let root = skills_root.canonicalize()?;
    let ids: BTreeSet<_> = selected.iter().cloned().collect();
    if ids.is_empty()
        || ids.len() != selected.len()
        || ids.len() > 100
        || ids
            .iter()
            .any(|id| !scan.items.iter().any(|s| &s.id == id && s.source.is_some()))
    {
        return Err(invalid("请选择 1–100 个有效 Skill 来源"));
    }
    let mut prepared = Prepared {
        preview_ids: vec![],
        groups: vec![],
    };
    let mut seen = BTreeSet::new();
    for item in scan.items.into_iter().filter(|i| ids.contains(&i.id)) {
        let source = item.source.unwrap();
        let result = (|| -> Result<Value> {
            let rel = relative(&source.directory)?;
            if rel == "." {
                return Err(invalid("外部 Skill 必须位于所选 skills 目录下"));
            }
            let path = root.join(rel);
            if path.canonicalize().ok().as_ref() != Some(&path) {
                return Err(invalid(
                    "Skill 目录不存在或包含符号链接，请选择真实的 Skill 存放目录",
                ));
            }
            if !seen.insert(path.clone()) {
                return Err(invalid("重复的外部 Skill 目录"));
            }
            let mut group = collections::preview_external(data, &path, source.clone())?;
            if let Some(token) = group["preview_id"].as_str() {
                prepared.preview_ids.push(token.into());
            }
            group["url"] = json!(source.repo_url);
            Ok(group)
        })();
        prepared.groups.push(result.unwrap_or_else(|e| json!({"state":"error","error":e.to_string(),"url":source.repo_url,"skills":[source],"management":"external"})));
    }
    let id = crate::ids::new_id();
    save(data, "previews", &id, &prepared)?;
    Ok(
        json!({"preview_id":id,"groups":prepared.groups,"ready":prepared.preview_ids.len(),"management":"external"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn user_triggered_database_scan_reads_only_skill_sources_and_keeps_database_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("synthetic-cc-switch");
        std::fs::create_dir_all(&dir).unwrap();
        let database = dir.join("cc-switch.db");
        let out = Command::new(sqlite_program()).args(["-init", if cfg!(windows) { "NUL" } else { "/dev/null" }]).arg(&database).arg(
            "CREATE TABLE skills(id TEXT,name TEXT,directory TEXT,repo_owner TEXT,repo_name TEXT,repo_branch TEXT,readme_url TEXT); INSERT INTO skills VALUES('fixture-1','one','one','fixture','skills','main','https://github.com/fixture/skills/blob/main/nested/one/SKILL.md'); CREATE TABLE providers(synthetic_marker TEXT); INSERT INTO providers VALUES('SYNTHETIC_DO_NOT_EXPORT');"
        ).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let before = std::fs::read(&database).unwrap();
        let data = tmp.path().join("data");
        let result = scan_directory(&data, &dir).unwrap();
        assert_eq!(result["items"][0]["source"]["repo_path"], "nested/one");
        assert!(!result.to_string().contains("SYNTHETIC_DO_NOT_EXPORT"));
        let stored = std::fs::read_to_string(
            file(&data, "scans", result["scan_id"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(!stored.contains("SYNTHETIC_DO_NOT_EXPORT"));
        assert_eq!(std::fs::read(&database).unwrap(), before);
        assert!(!data.join("collections").exists());
        assert!(scan_directory(&data, &tmp.path().join("missing")).is_err());
        #[cfg(unix)]
        {
            let linked = tmp.path().join("linked");
            std::fs::create_dir(&linked).unwrap();
            std::os::unix::fs::symlink(&database, linked.join("cc-switch.db")).unwrap();
            assert!(scan_directory(&data, &linked).is_err());
        }
    }

    #[test]
    fn parses_official_records_and_preserves_nested_path_and_branch() {
        let source = normalize(&json!({"id":"original", "name":"Nice skill", "directory":"local-alias", "repo_owner":"owner", "repo_name":"skills", "repo_branch":"feature/ui", "readme_url":"https://github.com/owner/skills/blob/feature/ui/plugins/dev/skills/tool/SKILL.md"})).unwrap();
        assert_eq!(source.repo_path.as_deref(), Some("plugins/dev/skills/tool"));
        assert_eq!(source.branch.as_deref(), Some("feature/ui"));
        assert_eq!(source.directory, "local-alias");
        assert_eq!(source.original_id, "original");
        let camel = normalize(&json!({"name":"tool", "repoOwner":"o", "repoName":"r", "repoBranch":"main", "readmeUrl":"https://github.com/o/r/blob/main/SKILL.md"})).unwrap();
        assert_eq!(camel.repo_path.as_deref(), Some("."));
    }

    #[test]
    fn accepts_ssh_and_skills_directory_without_guessing_local_path() {
        let ssh = normalize(
            &json!({"name":"x", "repo_url":"git@github.com:o/r.git", "repo_path":"plugins/x"}),
        )
        .unwrap();
        assert_eq!(ssh.repo_url, "git@github.com:o/r.git");
        let entry =
            normalize(&json!({"name":"x", "source_url":"https://skills.sh/o/r/x"})).unwrap();
        assert_eq!(entry.repo_url, "https://github.com/o/r");
        assert!(entry.repo_path.is_none());
        assert_eq!(repo_key(&ssh.repo_url), repo_key(&entry.repo_url));
        let explicit = normalize(&json!({"name":"x", "repo_owner":"o", "repo_name":"r", "repo_url":"ssh://git@github.com/o/r.git"})).unwrap();
        assert!(explicit.repo_url.starts_with("ssh://"));
    }

    #[test]
    fn rejects_unsafe_and_conflicting_sources() {
        for row in [
            json!({"name":"x"}),
            json!({"name":"x","repo_owner":"../x","repo_name":"r"}),
            json!({"name":"x","repo_url":"file:///tmp/repo"}),
            json!({"name":"x","repo_url":"ext::command"}),
            json!({"name":"x","repo_url":"https://user:password@github.com/o/r"}),
            json!({"name":"x","repo_url":"https://github.com/o/r?token=example"}),
            json!({"name":"x","repo_url":"https://github.com/o/r","repo_path":"../escape"}),
            json!({"name":"x","repo_url":"https://github.com/o/r","repo_path":"%2e%2e/escape"}),
            json!({"name":"x","repo_owner":"o","repo_name":"r","readme_url":"https://github.com/o/other/blob/main/x/SKILL.md"}),
            json!({"name":"x","repo_owner":"o","repo_name":"r","repo_branch":"other","readme_url":"https://github.com/o/r/blob/main/x/SKILL.md"}),
            json!({"name":"x","readme_url":"https://github.com/o/r/blob/--all/x/SKILL.md"}),
        ] {
            assert!(normalize(&row).is_err(), "{row}");
        }
    }

    #[test]
    fn scan_is_offline_partial_and_never_registers_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let v = scan(
            tmp.path(),
            &json!([{ "name":"valid", "repo_owner":"o", "repo_name":"r" }, {"name":"no-source"}]),
        )
        .unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        assert!(!v["items"][0]["source"].is_null());
        assert!(v["items"][1]["error"].is_string());
        assert!(!collections::registry_path(tmp.path()).exists());
        assert!(!tmp.path().join("collections/cache").exists());
        assert!(scan(tmp.path(), &json!({"skills":[],"providers":[]})).is_err());
        assert!(prepare(tmp.path(), "../bad", &["0".into()]).is_err());
        assert!(prepare(tmp.path(), v["scan_id"].as_str().unwrap(), &["1".into()]).is_err());
    }

    fn git(path: &Path, args: &[&str]) {
        let result = Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fn fixture(tmp: &Path) -> (PathBuf, String) {
        let repo = tmp.join("repo");
        for name in ["one", "two"] {
            let dir = repo.join("plugins/skills").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: fixture\n---\n# Test\n"),
            )
            .unwrap();
        }
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "fixture"]);
        let data = tmp.join("data");
        let items = ["one", "two"]
            .iter()
            .enumerate()
            .map(|(i, name)| ScanItem {
                id: i.to_string(),
                name: (*name).into(),
                error: None,
                source: Some(SkillOrigin {
                    original_id: format!("original-{name}"),
                    name: (*name).into(),
                    directory: (*name).into(),
                    discovery_entry: format!("https://skills.sh/fixture/repo/{name}"),
                    // Only fixture-created scan files bypass the public remote-only validator.
                    repo_url: repo.to_string_lossy().into_owned(),
                    branch: Some("main".into()),
                    repo_path: None,
                    resource_id: None,
                }),
            })
            .collect();
        let id = crate::ids::new_id();
        save(&data, "scans", &id, &Scan { items }).unwrap();
        (data, id)
    }

    #[test]
    fn real_git_preview_apply_reuse_and_update_preserve_provenance() {
        let tmp = tempfile::tempdir().unwrap();
        let (data, scan_id) = fixture(tmp.path());
        let p = prepare(&data, &scan_id, &["0".into(), "1".into()]).unwrap();
        assert_eq!(p["ready"], 1, "同一仓库只拉取登记一次");
        assert_eq!(
            p["groups"][0]["skills"][0]["repo_path"],
            "plugins/skills/one"
        );
        assert!(collections::load(&data).unwrap().sources.is_empty());
        let result = apply(&data, p["preview_id"].as_str().unwrap()).unwrap();
        assert_eq!(result["updated"], 1);
        let registry = collections::load(&data).unwrap();
        let source = registry.sources.values().next().unwrap();
        let origin = source.migration.as_ref().unwrap();
        assert_eq!(origin.skills.len(), 2);
        assert!(origin.skills[0].resource_id.is_some());
        assert!(!data.join("profile").exists(), "不启用到项目");
        assert!(!data.join("library").exists(), "不是本地副本导入");
        let again = prepare(&data, &scan_id, &["0".into()]).unwrap();
        assert_eq!(again["groups"][0]["state"], "existing");
        assert_eq!(again["ready"], 0);
        assert!(
            apply(&data, p["preview_id"].as_str().unwrap()).is_err(),
            "旧预览不能重复应用"
        );
        let update = collections::preview(
            &data,
            &source.name,
            &source.url,
            Some("main"),
            Some(&source.id),
        )
        .unwrap();
        collections::apply_preview(&data, update["preview_id"].as_str().unwrap()).unwrap();
        assert_eq!(
            collections::load(&data).unwrap().sources[&source.id]
                .migration
                .as_ref()
                .unwrap()
                .skills
                .len(),
            2
        );
    }

    #[test]
    fn ambiguous_matching_and_missing_paths_are_not_guessed() {
        let mut skill =
            normalize(&json!({"name":"same", "repo_owner":"o", "repo_name":"r"})).unwrap();
        let resources = vec![
            json!({"id":"a","kind":"skill","name":"same","path":"a/same"}),
            json!({"id":"b","kind":"skill","name":"same","path":"b/same"}),
        ];
        assert!(match_skill(&mut skill, &resources).is_err());
        skill.repo_path = Some("b/same".into());
        match_skill(&mut skill, &resources).unwrap();
        assert_eq!(skill.resource_id.as_deref(), Some("b"));
        skill.repo_path = Some("missing".into());
        assert!(match_skill(&mut skill, &resources).is_err());
    }

    #[test]
    fn conflicting_branches_and_stale_registry_do_not_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let (data, scan_id) = fixture(tmp.path());
        let path = file(&data, "scans", &scan_id).unwrap();
        let mut scan: Scan = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        scan.items[1].source.as_mut().unwrap().branch = Some("other".into());
        save(&data, "scans", &scan_id, &scan).unwrap();
        let p = prepare(&data, &scan_id, &["0".into(), "1".into()]).unwrap();
        assert_eq!(p["ready"], 0);
        assert_eq!(p["groups"][0]["state"], "error");
        let p = prepare(&data, &scan_id, &["0".into()]).unwrap();
        let mut registry = collections::load(&data).unwrap();
        registry.revision += 1;
        crate::sync_common::atomic_write(
            &collections::registry_path(&data),
            &serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();
        assert!(apply(&data, p["preview_id"].as_str().unwrap()).is_err());
        assert!(collections::load(&data).unwrap().sources.is_empty());
    }
}
