//! 全局 Skill（AIL-152）：把选中的资源库 / 合集 Skill 部署到用户级目录，所有项目可见。
//!
//! 目标目录：Claude Code 的 `~/.claude/skills`（遵循 `CLAUDE_CONFIG_DIR`），以及遵循
//! `.agents` 规范的 agent 共用的 `~/.agents/skills`。只部署 Skill 链接，不写任何宿主的
//! 全局配置文件（项目部署时 Codex 会顺带写 `.codex/config.toml`，到全局就是用户真实配置）。
//!
//! 所有权与项目部署相同：托管清单只记录 AILoom 部署的条目；目录里已有的同名条目
//! （例如 CC Switch 安装的链接）判为冲突保留，用户确认「接管」后才移入归档（可恢复）。

use crate::adapters::common::{Artifact, ArtifactBody, CODEX_NATIVE_SKILLS_DIR};
use crate::adapters::ToolTargets;
use crate::error::{code, Error, Result};
use crate::profile::{GlobalKey, PersonalProfile};
use crate::sync::manifest::ManagedManifest;
use crate::sync::plan::{build_plan, ActionKind, SyncPlan};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const CLAUDE_SKILLS_DIR: &str = ".claude/skills";

/// 一个全局目标目录。
#[derive(Debug, Clone)]
pub struct GlobalTarget {
    pub key: &'static str,
    pub label: &'static str,
    /// 绝对路径
    pub dir: PathBuf,
    /// 相对用户目录的路径；目录不在用户目录下时为 None（暂不支持，避免写到任意位置）
    pub rel: Option<PathBuf>,
    pub enabled: bool,
    /// 由哪个渲染产物前缀映射过来（项目级路径）
    project_prefix: &'static str,
}

impl GlobalTarget {
    pub fn usable(&self) -> bool {
        self.enabled && self.rel.is_some()
    }
}

/// 数据根下的全局部署状态目录。
struct Layout {
    dir: PathBuf,
}

impl Layout {
    fn new(data_root: &Path) -> Layout {
        Layout {
            dir: data_root.join("global"),
        }
    }
    fn managed(&self) -> PathBuf {
        self.dir.join("managed-manifest.json")
    }
    fn journal(&self) -> PathBuf {
        self.dir.join("journal")
    }
    fn locks(&self) -> PathBuf {
        self.dir.join("locks")
    }
    fn archive(&self) -> PathBuf {
        self.dir.join("archive")
    }
}

/// 用户目录（全局部署的根）。
pub fn home() -> Result<PathBuf> {
    let home = crate::paths::user_home()
        .ok_or_else(|| Error::new(code::WORKSPACE_INVALID, "无法确定用户目录（HOME 未设置）"))?;
    Ok(home.canonicalize().unwrap_or(home))
}

/// 规范化路径；末端尚不存在时规范化最深的已存在祖先再拼回剩余部分。
/// 否则首次使用（`~/.claude` 未建）或经符号链接的 `CLAUDE_CONFIG_DIR` 会因前缀
/// 与规范化后的 HOME 不一致（如 macOS `/var` → `/private/var`）被误判为不在用户目录下。
fn canonical_lenient(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while existing.canonicalize().is_err() {
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
    let mut out = existing.canonicalize().unwrap_or(existing);
    for name in rest.into_iter().rev() {
        out.push(name);
    }
    out
}

/// 全局目标目录与开关。
pub fn targets(home: &Path, profile: &PersonalProfile) -> Vec<GlobalTarget> {
    let claude_config = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"));
    let claude_dir = canonical_lenient(&claude_config).join("skills");
    let agents_dir = home.join(CODEX_NATIVE_SKILLS_DIR);
    let rel = |dir: &Path| dir.strip_prefix(home).ok().map(Path::to_path_buf);
    vec![
        GlobalTarget {
            key: "claude",
            label: "Claude Code",
            rel: rel(&claude_dir),
            dir: claude_dir,
            enabled: profile.global.target_enabled("claude"),
            project_prefix: CLAUDE_SKILLS_DIR,
        },
        GlobalTarget {
            key: "agents",
            label: "Codex 等其他 Agent",
            rel: rel(&agents_dir),
            dir: agents_dir,
            enabled: profile.global.target_enabled("agents"),
            project_prefix: CODEX_NATIVE_SKILLS_DIR,
        },
    ]
}

/// 计划结果（plan / sync / 状态页共用）。
pub struct GlobalPrepare {
    pub home: PathBuf,
    pub targets: Vec<GlobalTarget>,
    pub plan: SyncPlan,
    pub artifacts: Vec<Artifact>,
    pub managed: ManagedManifest,
    pub notes: Vec<String>,
    pub unsupported: Vec<crate::adapters::UnsupportedItem>,
    pub skipped: Vec<crate::personal_instructions::SkippedTarget>,
    layout: Layout,
}

pub fn prepare(data_root: &Path) -> Result<GlobalPrepare> {
    let profile = PersonalProfile::load_or_default(data_root)?;
    let home = home()?;
    let targets = targets(&home, &profile);
    let layout = Layout::new(data_root);
    let mut notes = Vec::new();
    for t in targets.iter().filter(|t| t.enabled && t.rel.is_none()) {
        notes.push(format!(
            "{} 的全局目录 {} 不在用户目录下，暂不支持部署",
            t.label,
            t.dir.display()
        ));
    }

    let ids: Vec<String> = profile.global.skills.iter().cloned().collect();
    let rendered = crate::commands::personal::render_selected(
        data_root,
        &ids,
        &ToolTargets {
            claude: true,
            codex: true,
            extra: Vec::new(),
        },
        &home,
        &home,
    )?;
    notes.extend(rendered.notes);
    let mut unsupported = rendered.unsupported;
    for id in &rendered.unresolved {
        unsupported.push(crate::adapters::UnsupportedItem {
            resource_id: id.clone(),
            tool: "-".into(),
            kind: "resource".into(),
            reason: "合集来源读取失败或资源已不存在；可在全局页停用".into(),
        });
    }
    for entry in &rendered.entries {
        if entry.id.kind != crate::resource::ResourceKind::Skill {
            unsupported.push(crate::adapters::UnsupportedItem {
                resource_id: entry.id.to_string(),
                tool: "-".into(),
                kind: entry.id.kind.as_str().into(),
                reason: "全局只部署 Skill；其他类型请在项目中启用".into(),
            });
        }
    }

    // 只保留 Skill 链接，并改写到全局目标目录；其余产物（如 .codex/config.toml）一律丢弃
    let mut artifacts = Vec::new();
    for mut a in rendered.artifacts {
        if a.kind != "skill"
            || !matches!(
                a.body,
                ArtifactBody::Symlink { .. } | ArtifactBody::ExternalSymlink { .. }
            )
        {
            continue;
        }
        let Some(target) = targets
            .iter()
            .find(|t| a.path.starts_with(t.project_prefix))
        else {
            continue;
        };
        let (Some(rel), true) = (&target.rel, target.enabled) else {
            continue;
        };
        let name = a.path.file_name().map(PathBuf::from).unwrap_or_default();
        a.path = rel.join(name);
        // 同一链接可能由多个宿主渲染（例如 .agents 目录被多个 agent 共用）：按路径去重
        if !artifacts.iter().any(|x: &Artifact| x.path == a.path) {
            artifacts.push(a);
        }
    }
    // 用户目录本身可能是 dotfiles 仓库：被跟踪的路径不写
    let (artifacts, skipped) = crate::personal_instructions::guard_company_files(&home, artifacts)?;

    let managed =
        ManagedManifest::load(&layout.managed())?.unwrap_or_else(|| ManagedManifest::new("global"));
    let mut plan = build_plan(&artifacts, &managed, &home, "global", None)?;
    // 定义无效的资源只是这次无法渲染：已部署的全局链接保留到修复为止（与项目部署一致）
    let invalid: std::collections::BTreeSet<&str> = unsupported
        .iter()
        .filter(|u| u.tool == "-" && u.kind != "resource")
        .map(|u| u.resource_id.as_str())
        .collect();
    plan.actions.retain(|a| {
        !(crate::commands::sync_core::is_cleanup(a) && invalid.contains(a.resource_id.as_str()))
    });
    Ok(GlobalPrepare {
        home,
        targets,
        plan,
        artifacts,
        managed,
        notes,
        unsupported,
        skipped,
        layout,
    })
}

/// 已全局部署的 (资源 ID, 宿主)。项目部署据此跳过重复项：以实际部署为准，
/// 全局页勾选后、同步前，项目里的 Skill 不会先消失。
pub fn deployed_set(data_root: &Path) -> std::collections::BTreeSet<(String, String)> {
    ManagedManifest::load(&Layout::new(data_root).managed())
        .ok()
        .flatten()
        .map(|m| {
            m.items
                .values()
                .map(|i| (i.resource_id.clone(), i.target_tool.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// 从项目产物中去掉已全局部署的 Skill（Claude / Codex 链接），并同步删掉 Codex
/// skills.config 里对应的条目。返回被跳过的资源 ID。
pub fn skip_globally_deployed(
    data_root: &Path,
    artifacts: &mut Vec<Artifact>,
) -> std::collections::BTreeSet<String> {
    let global = deployed_set(data_root);
    let mut skipped = std::collections::BTreeSet::new();
    if global.is_empty() {
        return skipped;
    }
    let mut codex_names = Vec::new();
    artifacts.retain(|a| {
        let hit =
            a.kind == "skill" && global.contains(&(a.resource_id.clone(), a.target_tool.clone()));
        if hit {
            skipped.insert(a.resource_id.clone());
            if a.target_tool == "codex" {
                if let Some(name) = a.path.file_name() {
                    codex_names.push(format!(
                        "{CODEX_NATIVE_SKILLS_DIR}/{}/SKILL.md",
                        name.to_string_lossy()
                    ));
                }
            }
        }
        !hit
    });
    if codex_names.is_empty() {
        return skipped;
    }
    artifacts.retain(|a| {
        if a.resource_id != "ailoom-builtin/codex-skills-config" {
            return true;
        }
        match &a.body {
            ArtifactBody::TomlArrayEntry {
                table,
                key_field,
                entry,
            } if table == "skills.config" && key_field == "path" => !entry
                .get("path")
                .and_then(|p| p.as_str())
                .is_some_and(|path| codex_names.iter().any(|n| n == path)),
            _ => true,
        }
    });
    for a in artifacts.iter_mut() {
        if a.resource_id != "ailoom-builtin/codex-skills-config" {
            continue;
        }
        let ArtifactBody::Full { content } = &mut a.body else {
            continue;
        };
        let Ok(mut doc) = content.parse::<toml::Value>() else {
            continue;
        };
        if let Some(arr) = doc
            .get_mut("skills")
            .and_then(|s| s.get_mut("config"))
            .and_then(|c| c.as_array_mut())
        {
            arr.retain(|item| {
                let path = item
                    .get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or_default();
                !codex_names.iter().any(|n| n == path)
            });
        }
        if let Ok(text) = toml::to_string_pretty(&doc) {
            *content = text;
        }
    }
    skipped
}

fn action_json(plan: &SyncPlan) -> Vec<Value> {
    plan.actions
        .iter()
        .map(|a| {
            json!({
                "action": a.action,
                "path": a.path,
                "resource_id": a.resource_id,
                "reason": a.reason,
            })
        })
        .collect()
}

/// 预览全局部署（不写入）。
pub fn plan(data_root: &Path) -> Result<Value> {
    let p = prepare(data_root)?;
    Ok(json!({
        "home": p.home,
        "summary": p.plan.summary(),
        "actions": action_json(&p.plan),
        "has_conflicts": p.plan.has_conflicts(),
        "unsupported": p.unsupported,
        "skipped": p.skipped,
        "notes": p.notes,
    }))
}

/// 应用全局部署：与项目同步相同的锁 / journal / 托管清单管道。
pub fn sync(data_root: &Path) -> Result<Value> {
    let mut p = prepare(data_root)?;
    let device = crate::config::device_id(data_root)?;
    let report = crate::sync::apply::apply(
        &p.plan,
        &p.artifacts,
        &mut p.managed,
        &p.home,
        &p.layout.locks(),
        &p.layout.journal(),
        &device,
    )?;
    if report.ok {
        p.managed.save(&p.layout.managed())?;
    }
    let value = json!({
        "ok": report.ok,
        "applied": report.applied,
        "noop": report.noop,
        "skipped_conflicts": report.skipped_conflicts,
        "skipped_unsupported": report.skipped_unsupported,
        "skipped_company_files": p.skipped,
        "failed": report.failed,
        "pending_journal": report.pending_journal,
        "notes": p.notes,
        "next": "在 AI 工具中新开会话后生效（文件落盘不等于宿主已加载）",
    });
    if !report.ok {
        return Err(Error::new(
            code::WRITE_FAILED,
            "全局同步未全部完成：已写入的部分保留在恢复点中",
        )
        .context(value)
        .fix("修正问题后运行 ailoom global --action recover 撤回已写入的部分，再重新同步"));
    }
    Ok(value)
}

/// 撤回中途失败的全局同步。
pub fn recover(data_root: &Path) -> Result<Value> {
    let layout = Layout::new(data_root);
    let device = crate::config::device_id(data_root)?;
    let _lock = crate::sync::lock::SyncLock::acquire(&layout.locks(), &device)?;
    let report = crate::sync::apply::recover(&layout.journal(), &home()?)?;
    Ok(json!({ "recovered": report }))
}

/// 可全局启用的 Skill（资源库 + 合集），带当前状态。
fn candidates(data_root: &Path, profile: &PersonalProfile) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let (entries, _) = crate::personal_library::list_tolerant(data_root);
    for e in entries.iter().filter(|e| e.kind == "skill") {
        out.push(json!({
            "id": e.id, "name": e.name, "description": e.description,
            "source": "资源库", "global": profile.global.skills.contains(&e.id),
        }));
    }
    let collections = crate::collections::list(data_root)?;
    for s in collections["sources"].as_array().into_iter().flatten() {
        for r in s["resources"].as_array().into_iter().flatten() {
            if r["kind"] != "skill" {
                continue;
            }
            let id = r["id"].as_str().unwrap_or_default();
            out.push(json!({
                "id": id, "name": r["name"], "description": r["description"],
                "source": s["name"], "global": profile.global.skills.contains(id),
            }));
        }
    }
    Ok(out)
}

/// 条目的真实位置说明：是不是链接、沿链接解析到底的真实目录、中间经过的链接。
/// 只写一个「→」时，用户分不清链接本身在哪、内容在哪，链接套链接时也看不出终点。
fn describe_location(path: &Path) -> Value {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return json!({ "kind": "missing" });
    };
    if !meta.file_type().is_symlink() {
        return json!({ "kind": if meta.is_dir() { "dir" } else { "file" }, "real_path": path });
    }
    let first = std::fs::read_link(path).ok().map(|t| {
        if t.is_absolute() {
            t
        } else {
            path.parent().unwrap_or(Path::new("/")).join(t)
        }
    });
    // 中间每一跳（链接套链接时列出）
    let mut hops = Vec::new();
    let mut cur = first.clone();
    while let Some(c) = cur.take() {
        if hops.len() >= 8
            || !c
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        {
            break;
        }
        // 只规范化父目录：规范化链接本身会直接跳到终点
        let shown = match (c.parent(), c.file_name()) {
            (Some(parent), Some(name)) => canonical_lenient(parent).join(name),
            _ => c.clone(),
        };
        hops.push(shown);
        cur = std::fs::read_link(&c).ok().map(|t| {
            if t.is_absolute() {
                t
            } else {
                c.parent().unwrap_or(Path::new("/")).join(t)
            }
        });
    }
    match path.canonicalize() {
        Ok(real) => json!({ "kind": "link", "real_path": real, "via": hops }),
        Err(_) => json!({ "kind": "broken_link", "points_to": first, "via": hops }),
    }
}

/// 托管清单是否已管理某个路径。清单 key 带部署方式后缀（如 `…/name#symlink`），按路径部分比较。
fn is_managed(managed: &ManagedManifest, rel_path: &str) -> bool {
    managed
        .items
        .keys()
        .any(|k| k.split('#').next() == Some(rel_path))
}

/// 全局目录里不由 AILoom 管理的条目（只读展示；同名时可接管）。
fn foreign_entries(p: &GlobalPrepare) -> Vec<Value> {
    let mut out = Vec::new();
    for t in &p.targets {
        let Some(rel) = &t.rel else { continue };
        for entry in std::fs::read_dir(&t.dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let key = rel.join(&name).to_string_lossy().to_string();
            if is_managed(&p.managed, &key) {
                continue;
            }
            let path = entry.path();
            let link = std::fs::read_link(&path).ok();
            let location = describe_location(&path);
            let conflict = p
                .plan
                .actions
                .iter()
                .find(|a| a.action == ActionKind::Conflict && a.path == key)
                .map(|a| a.resource_id.clone());
            out.push(json!({
                "target": t.key,
                "name": name,
                "path": path,
                "link_target": link,
                "location": location,
                "conflicts_with": conflict,
            }));
        }
    }
    out
}

/// 全局页状态：目标目录、候选 Skill、待应用改动、目录里的非托管条目、可恢复的归档。
pub fn status(data_root: &Path) -> Result<Value> {
    let profile = PersonalProfile::load_or_default(data_root)?;
    let p = prepare(data_root)?;
    let pending = p
        .plan
        .actions
        .iter()
        .filter(|a| {
            matches!(
                a.action,
                ActionKind::Create | ActionKind::Update | ActionKind::Delete | ActionKind::Restore
            )
        })
        .count();
    let targets: Vec<Value> = p
        .targets
        .iter()
        .map(|t| {
            json!({
                "key": t.key, "label": t.label, "dir": t.dir, "enabled": t.enabled,
                "supported": t.rel.is_some(),
            })
        })
        .collect();
    Ok(json!({
        "revision": profile.revision,
        "home": p.home,
        "targets": targets,
        "skills": candidates(data_root, &profile)?,
        "deployed": p.managed.items.iter().map(|(k, i)| json!({"path": k, "resource_id": i.resource_id, "tool": i.target_tool})).collect::<Vec<_>>(),
        "pending": pending,
        "actions": action_json(&p.plan),
        "foreign": foreign_entries(&p),
        "archive": archive_records(data_root),
        "unsupported": p.unsupported,
        "notes": p.notes,
    }))
}

/// 启用 / 停用一个全局 Skill，或开关一个目标目录。Skill 必须是资源库或合集里真实存在的 Skill。
pub fn select(
    data_root: &Path,
    key: &GlobalKey,
    enabled: bool,
    expect_revision: Option<u64>,
) -> Result<Value> {
    if let (GlobalKey::Skill(id), true) = (key, enabled) {
        let profile = PersonalProfile::load_or_default(data_root)?;
        let known = candidates(data_root, &profile)?
            .iter()
            .any(|c| c["id"] == json!(id));
        if !known {
            return Err(Error::new(
                code::UNKNOWN_REFERENCE,
                format!("资源库或合集中没有这个 Skill: {id}"),
            )
            .fix("运行 ailoom global --action status 查看可全局启用的 Skill（需完整资源 ID）"));
        }
    }
    let revision = crate::profile::set_global_in_place(data_root, key, enabled, expect_revision)?;
    Ok(json!({
        "revision": revision,
        "enabled": enabled,
        "skill": match key { GlobalKey::Skill(id) => Some(id), _ => None },
        "target": match key { GlobalKey::Target(t) => Some(t), _ => None },
        "next": "运行 ailoom global --action plan 预览、--action sync 应用",
    }))
}

fn archive_records(data_root: &Path) -> Vec<Value> {
    let mut out: Vec<Value> = std::fs::read_dir(Layout::new(data_root).archive())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("record.json")).ok())
        .filter_map(|t| serde_json::from_str(&t).ok())
        .filter(|v: &Value| v["restored"] != true)
        .collect();
    out.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    out
}

fn target_by_key<'a>(p: &'a GlobalPrepare, key: &str) -> Result<&'a GlobalTarget> {
    p.targets.iter().find(|t| t.key == key).ok_or_else(|| {
        Error::new(
            code::UNKNOWN_REFERENCE,
            format!("未知全局目标: {key}（支持 claude / agents）"),
        )
    })
}

/// 接管：把全局目录里一个非 AILoom 管理的条目移入归档（链接只移动链接本身，不碰其指向的内容），
/// 让 AILoom 能在该位置部署。归档可用 restore 放回。
pub fn takeover(data_root: &Path, target: &str, name: &str) -> Result<Value> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') {
        return Err(Error::new(
            code::PATH_TRAVERSAL,
            format!("条目名非法: {name}"),
        ));
    }
    let layout = Layout::new(data_root);
    let device = crate::config::device_id(data_root)?;
    let _lock = crate::sync::lock::SyncLock::acquire(&layout.locks(), &device)?;
    let p = prepare(data_root)?;
    let t = target_by_key(&p, target)?;
    let rel = t.rel.as_ref().ok_or_else(|| {
        Error::new(
            code::WORKSPACE_INVALID,
            "该全局目录不在用户目录下，暂不支持",
        )
    })?;
    let key = rel.join(name).to_string_lossy().to_string();
    if is_managed(&p.managed, &key) {
        return Err(Error::new(
            code::PRECONDITION_FAILED,
            "该条目已由 AILoom 管理，无需接管",
        ));
    }
    let path = t.dir.join(name);
    if path.symlink_metadata().is_err() {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("条目不存在: {}", path.display()),
        ));
    }
    if crate::personal_instructions::path_is_git_tracked(&p.home, &key) {
        return Err(Error::new(
            code::TARGET_CONFLICT,
            format!("{key} 被用户目录的 Git 仓库跟踪，不接管"),
        ));
    }
    let location = describe_location(&path);
    let id = crate::ids::new_id();
    let dir = Layout::new(data_root).archive().join(&id);
    std::fs::create_dir_all(&dir)?;
    let archived = dir.join(name);
    let record = json!({
        "id": id, "target": target, "name": name, "original": path,
        "archived": archived, "link_target": std::fs::read_link(&path).ok(),
        "location": location,
        "at": crate::ids::now_iso(), "restored": false,
    });
    let bytes = serde_json::to_vec_pretty(&record)?;
    let moved = move_entry_with_record(&path, &archived, || {
        crate::sync_common::atomic_write(&dir.join("record.json"), &bytes)
    });
    let notes = match moved {
        Ok(notes) => notes,
        Err(e) => {
            // 只删除空归档；回滚异常时保留其中的原条目供恢复。
            let _ = std::fs::remove_dir(&dir);
            return Err(e);
        }
    };
    Ok(json!({
        "archived": record,
        "notes": notes,
        "next": "运行 ailoom global --action sync 由 AILoom 部署该位置；需要还原时 ailoom global --action restore --id <id>",
    }))
}

/// 把接管时归档的条目放回原位置；原位置已被占用时拒绝（不覆盖）。
pub fn restore(data_root: &Path, id: &str) -> Result<Value> {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(Error::new(code::USAGE, format!("归档 ID 非法: {id}")));
    }
    let layout = Layout::new(data_root);
    let device = crate::config::device_id(data_root)?;
    let _lock = crate::sync::lock::SyncLock::acquire(&layout.locks(), &device)?;
    let dir = layout.archive().join(id);
    let text = std::fs::read_to_string(dir.join("record.json"))
        .map_err(|_| Error::new(code::UNKNOWN_REFERENCE, format!("归档不存在: {id}")))?;
    let mut record: Value = serde_json::from_str(&text)?;
    if record["restored"] == true {
        return Err(Error::new(code::PRECONDITION_FAILED, "该归档已经还原"));
    }
    let original = PathBuf::from(record["original"].as_str().unwrap_or_default());
    let archived = PathBuf::from(record["archived"].as_str().unwrap_or_default());
    if original.symlink_metadata().is_ok() {
        return Err(Error::new(
            code::TARGET_CONFLICT,
            format!("原位置已被占用: {}", original.display()),
        )
        .fix("先在全局页停用占用该位置的 Skill 并同步，再还原"));
    }
    if let Some(parent) = original.parent() {
        std::fs::create_dir_all(parent)?;
    }
    record["restored"] = json!(true);
    let bytes = serde_json::to_vec_pretty(&record)?;
    let notes = move_entry_with_record(&archived, &original, || {
        crate::sync_common::atomic_write(&dir.join("record.json"), &bytes)
    })?;
    Ok(json!({ "restored": record, "notes": notes }))
}

/// 同盘直接移动；跨盘先复制到目标盘暂存，再把原条目改名留在原盘。
/// 在恢复记录落盘前不删除原条目，记录失败时可以原样改名回滚。
fn move_entry_with_record(
    from: &Path,
    to: &Path,
    save_record: impl FnOnce() -> Result<()>,
) -> Result<Vec<String>> {
    match to.symlink_metadata() {
        Ok(_) => return Err(Error::new(code::TARGET_CONFLICT, "移动目标已存在，不覆盖")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    match std::fs::rename(from, to) {
        Ok(()) => {
            if let Err(e) = save_record() {
                rollback_move(to, from)?;
                return Err(e);
            }
            Ok(Vec::new())
        }
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            copy_entry_with_record(from, to, save_record)
        }
        Err(e) => Err(Error::new(
            code::WRITE_FAILED,
            format!("移动失败，原条目保留: {e}"),
        )),
    }
}

fn rollback_move(from: &Path, to: &Path) -> Result<()> {
    if to.symlink_metadata().is_ok() {
        return Err(Error::new(
            code::JOURNAL_RESTORE_FAILED,
            "回滚位置被占用，原条目保留在暂存位置",
        )
        .context(json!({ "preserved": from, "original": to })));
    }
    std::fs::rename(from, to).map_err(|e| {
        Error::new(
            code::JOURNAL_RESTORE_FAILED,
            format!("回滚失败，原条目保留: {e}"),
        )
        .context(json!({ "preserved": from, "original": to }))
    })
}

fn copy_entry_with_record(
    from: &Path,
    to: &Path,
    save_record: impl FnOnce() -> Result<()>,
) -> Result<Vec<String>> {
    let suffix = crate::ids::new_id();
    let stage = to.with_file_name(format!(".ailoom-copy-{suffix}"));
    let held = from.with_file_name(format!(".ailoom-move-{suffix}"));
    if let Err(e) = copy_entry(from, &stage) {
        let _ = remove_copied_entry(&stage);
        return Err(Error::new(
            code::WRITE_FAILED,
            format!("跨盘复制失败，原条目未改动: {e}"),
        ));
    }
    if let Err(e) = std::fs::rename(from, &held) {
        let _ = remove_copied_entry(&stage);
        return Err(Error::new(
            code::WRITE_FAILED,
            format!("暂存原条目失败，原条目未改动: {e}"),
        ));
    }
    // 复制期间原内容可能改变；复核留在原盘的本体，不跟随任何符号链接。
    let copied =
        entry_digest(&held).and_then(|original| entry_digest(&stage).map(|copy| original == copy));
    let publish = match copied {
        Ok(true) if to.symlink_metadata().is_err() => std::fs::rename(&stage, to),
        Ok(_) => Err(std::io::Error::other("条目在复制期间变化或目标被占用")),
        Err(e) => Err(e),
    };
    if let Err(e) = publish {
        rollback_move(&held, from)?;
        let _ = remove_copied_entry(&stage);
        return Err(Error::new(
            code::WRITE_FAILED,
            format!("跨盘移动失败，原条目已放回: {e}"),
        ));
    }
    if let Err(e) = save_record() {
        // 原本体仍在 held；即使副本清理失败，也先把原本体放回。
        let cleanup = remove_copied_entry(to);
        rollback_move(&held, from)?;
        if let Err(cleanup) = cleanup {
            return Err(Error::new(
                code::WRITE_FAILED,
                format!("{e}；原条目已放回，副本清理失败: {cleanup}"),
            )
            .context(json!({ "copy": to })));
        }
        return Err(e);
    }
    // 记录已经提交；清理失败不能再把成功的还原/归档误报为未执行。
    match remove_copied_entry(&held) {
        Ok(()) => Ok(Vec::new()),
        Err(e) => Ok(vec![format!(
            "移动已完成，原盘暂存副本尚未清理: {}（{e}）",
            held.display()
        )]),
    }
}

fn copy_entry(from: &Path, to: &Path) -> std::io::Result<()> {
    let metadata = from.symlink_metadata()?;
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(from)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, to)?;
        #[cfg(not(unix))]
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "当前平台不支持跨盘复制符号链接",
        ));
    } else if metadata.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
        std::fs::set_permissions(to, metadata.permissions())?;
    } else if metadata.is_file() {
        let mut source = std::fs::File::open(from)?;
        let mut destination = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(to)?;
        std::io::copy(&mut source, &mut destination)?;
        destination.sync_all()?;
        std::fs::set_permissions(to, metadata.permissions())?;
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "不能跨盘移动特殊文件",
        ));
    }
    Ok(())
}

fn entry_digest(path: &Path) -> std::io::Result<Vec<u8>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let metadata = path.symlink_metadata()?;
    let mut hash = Sha256::new();
    if metadata.file_type().is_symlink() {
        hash.update(b"link");
        hash.update(std::fs::read_link(path)?.as_os_str().as_encoded_bytes());
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            hash.update(metadata.permissions().mode().to_le_bytes());
        }
        if metadata.is_dir() {
            hash.update(b"dir");
            let mut entries = std::fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let name = entry.file_name();
                let bytes = name.as_encoded_bytes();
                hash.update(bytes.len().to_le_bytes());
                hash.update(bytes);
                hash.update(entry_digest(&entry.path())?);
            }
        } else if metadata.is_file() {
            hash.update(b"file");
            let mut file = std::fs::File::open(path)?;
            let mut buf = [0; 65536];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hash.update(&buf[..n]);
            }
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "不能跨盘移动特殊文件",
            ));
        }
    }
    Ok(hash.finalize().to_vec())
}

/// 只用于本次创建的副本/已提交后的暂存本体；不遍历链接目标。
fn remove_copied_entry(path: &Path) -> std::io::Result<()> {
    let metadata = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if metadata.is_dir() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                path,
                std::fs::Permissions::from_mode(metadata.permissions().mode() | 0o700),
            )?;
        }
        for entry in std::fs::read_dir(path)? {
            remove_copied_entry(&entry?.path())?;
        }
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(id: &str, tool: &str, kind: &str, path: &str, body: ArtifactBody) -> Artifact {
        Artifact {
            resource_id: id.into(),
            target_tool: tool.into(),
            kind: kind.into(),
            path: path.into(),
            body,
        }
    }

    #[test]
    fn lenient_canonicalization_handles_missing_tails() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().canonicalize().unwrap();
        let missing = tmp.path().join("not/yet/created");
        assert_eq!(canonical_lenient(&missing), real.join("not/yet/created"));
        #[cfg(unix)]
        {
            std::fs::create_dir_all(real.join("target")).unwrap();
            std::os::unix::fs::symlink(real.join("target"), real.join("link")).unwrap();
            assert_eq!(
                canonical_lenient(&real.join("link/skills")),
                real.join("target/skills")
            );
        }
    }

    #[test]
    fn skipping_global_skills_also_prunes_codex_config_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let mut managed = ManagedManifest::new("global");
        managed.items.insert(
            ".agents/skills/g".into(),
            crate::sync::manifest::ManagedItem {
                resource_id: "personal/skill/personal/g".into(),
                target_tool: "codex".into(),
                kind: "skill".into(),
                content_hash: "x".into(),
                deployed_at: "t".into(),
            },
        );
        let layout = Layout::new(tmp.path());
        std::fs::create_dir_all(&layout.dir).unwrap();
        managed.save(&layout.managed()).unwrap();
        let link = || ArtifactBody::Full {
            content: String::new(),
        };
        let config = "[[skills.config]]\nenabled = true\npath = \".agents/skills/g/SKILL.md\"\n\n[[skills.config]]\nenabled = true\npath = \".agents/skills/p/SKILL.md\"\n";
        let mut artifacts = vec![
            artifact(
                "personal/skill/personal/g",
                "codex",
                "skill",
                ".agents/skills/g",
                link(),
            ),
            artifact(
                "personal/skill/personal/g",
                "claude",
                "skill",
                ".claude/skills/g",
                link(),
            ),
            artifact(
                "personal/skill/personal/p",
                "codex",
                "skill",
                ".agents/skills/p",
                link(),
            ),
            artifact(
                "ailoom-builtin/codex-skills-config",
                "codex",
                "skill-config",
                ".codex/config.toml",
                ArtifactBody::Full {
                    content: config.into(),
                },
            ),
        ];
        let skipped = skip_globally_deployed(tmp.path(), &mut artifacts);
        assert_eq!(skipped.len(), 1);
        // 只有 Codex 那份被全局覆盖；Claude 那份全局未部署，项目里保留
        assert!(artifacts
            .iter()
            .any(|a| a.path == Path::new(".claude/skills/g")));
        assert!(!artifacts
            .iter()
            .any(|a| a.path == Path::new(".agents/skills/g")));
        let ArtifactBody::Full { content } = &artifacts.last().unwrap().body else {
            panic!("config artifact");
        };
        assert!(
            !content.contains("skills/g/") && content.contains("skills/p/"),
            "{content}"
        );
        // 新配置按 path 拥有各条目；只过滤被全局覆盖的那一个。
        let entry = |name: &str| ArtifactBody::TomlArrayEntry {
            table: "skills.config".into(),
            key_field: "path".into(),
            entry: toml::Value::try_from(
                json!({"path": format!(".agents/skills/{name}/SKILL.md"), "enabled": true}),
            )
            .unwrap(),
        };
        let mut artifacts = vec![
            artifact(
                "personal/skill/personal/g",
                "codex",
                "skill",
                ".agents/skills/g",
                link(),
            ),
            artifact(
                "personal/skill/personal/p",
                "codex",
                "skill",
                ".agents/skills/p",
                link(),
            ),
            artifact(
                "ailoom-builtin/codex-skills-config",
                "codex",
                "skill-config",
                ".codex/config.toml",
                entry("g"),
            ),
            artifact(
                "ailoom-builtin/codex-skills-config",
                "codex",
                "skill-config",
                ".codex/config.toml",
                entry("p"),
            ),
        ];
        skip_globally_deployed(tmp.path(), &mut artifacts);
        let entries: Vec<_> = artifacts
            .iter()
            .filter_map(|a| match &a.body {
                ArtifactBody::TomlArrayEntry { entry, .. } => {
                    entry.get("path").and_then(|v| v.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(entries, vec![".agents/skills/p/SKILL.md"]);
    }

    #[cfg(unix)]
    fn fixture_tree(path: &Path) {
        use std::os::unix::fs::{symlink, PermissionsExt};
        std::fs::create_dir_all(path.join("nested")).unwrap();
        std::fs::write(path.join("nested/file"), b"original\0bytes").unwrap();
        std::fs::set_permissions(
            path.join("nested/file"),
            std::fs::Permissions::from_mode(0o751),
        )
        .unwrap();
        symlink("nested/file", path.join("relative-link")).unwrap();
        symlink("missing", path.join("broken-link")).unwrap();
        std::fs::set_permissions(path.join("nested"), std::fs::Permissions::from_mode(0o500))
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_copy_preserves_entry_types_bytes_and_permissions_both_ways() {
        let tmp = tempfile::tempdir().unwrap();
        let original = tmp.path().join("original");
        let archived = tmp.path().join("archived");
        fixture_tree(&original);
        let digest = entry_digest(&original).unwrap();
        copy_entry_with_record(&original, &archived, || Ok(())).unwrap();
        assert_eq!(entry_digest(&archived).unwrap(), digest);
        assert!(original.symlink_metadata().is_err());
        copy_entry_with_record(&archived, &original, || Ok(())).unwrap();
        assert_eq!(entry_digest(&original).unwrap(), digest);
        assert!(archived.symlink_metadata().is_err());
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().count(),
            1,
            "无暂存残留"
        );
        remove_copied_entry(&original).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_record_failure_rolls_back_takeover_and_restore_without_deleting_original() {
        use std::os::unix::fs::MetadataExt;
        let tmp = tempfile::tempdir().unwrap();
        for restoring in [false, true] {
            let from = tmp
                .path()
                .join(if restoring { "archived" } else { "original" });
            let to = tmp
                .path()
                .join(if restoring { "original" } else { "archived" });
            fixture_tree(&from);
            let inode = from.symlink_metadata().unwrap().ino();
            let digest = entry_digest(&from).unwrap();
            // 模拟真实 record.json 原子替换被目录阻塞，而非无关字符串断言。
            let record = tmp.path().join("record.json");
            std::fs::create_dir(&record).unwrap();
            let e = copy_entry_with_record(&from, &to, || {
                crate::sync_common::atomic_write(&record, b"new record")
            })
            .unwrap_err();
            assert_eq!(e.code, code::WRITE_FAILED);
            assert_eq!(
                from.symlink_metadata().unwrap().ino(),
                inode,
                "本体改名放回"
            );
            assert_eq!(entry_digest(&from).unwrap(), digest);
            assert!(to.symlink_metadata().is_err());
            assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
            remove_copied_entry(&from).unwrap();
            std::fs::remove_dir(record).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_copy_failure_preserves_source_and_removes_partial_copy() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("original");
        let to = tmp.path().join("archived");
        std::fs::create_dir(&from).unwrap();
        std::fs::write(from.join("file"), "untouched").unwrap();
        let fifo = std::ffi::CString::new(from.join("fifo").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let mut saved = false;
        let e = copy_entry_with_record(&from, &to, || {
            saved = true;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(e.code, code::WRITE_FAILED);
        assert!(!saved);
        assert_eq!(
            std::fs::read_to_string(from.join("file")).unwrap(),
            "untouched"
        );
        assert!(from.join("fifo").symlink_metadata().is_ok());
        assert!(!to.exists());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn same_device_record_failure_rolls_back_and_occupied_target_is_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("original");
        let to = tmp.path().join("archived");
        std::fs::write(&from, "original").unwrap();
        let e = move_entry_with_record(&from, &to, || {
            Err(Error::new(code::WRITE_FAILED, "record failed"))
        })
        .unwrap_err();
        assert_eq!(e.code, code::WRITE_FAILED);
        assert_eq!(std::fs::read_to_string(&from).unwrap(), "original");
        assert!(!to.exists());
        std::fs::write(&to, "occupied").unwrap();
        assert_eq!(
            move_entry_with_record(&from, &to, || Ok(()))
                .unwrap_err()
                .code,
            code::TARGET_CONFLICT
        );
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "occupied");
        assert_eq!(std::fs::read_to_string(&from).unwrap(), "original");
    }
}
