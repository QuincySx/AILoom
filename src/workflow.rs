//! Grill→Spec→Tickets→Implement 流程包（AIL-045）。
//!
//! 流程 = 四个方法 skill 按**实际资源身份**绑定（缺项显式显示缺失，不凭名字假定
//! 已安装，不从未知位置静默复制）+ 一次工作的产物账本：稳定流程 ID、阶段
//! （align/spec/tickets/implementation/acceptance）、输入版本、产物关联。
//! 重命名不丢关联（产物以 ID 关联）；规格变更提升下游版本并标记「需复核」，
//! 不自动删除/覆盖或宣称票据已实现。全部数据存机器数据区（仓外）。

use crate::error::{code, Error, Result};
use crate::ids::{new_id, now_iso};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STAGES: [&str; 5] = ["align", "spec", "tickets", "implementation", "acceptance"];

/// 流程包默认的四个方法 skill 身份模板（绑定时要解析为实际存在的资源，
/// 解析不到就保留占位并显式缺失）。
pub const PACK_TEMPLATE: [(&str, &str); 4] = [
    ("align", "grill"),
    ("spec", "to-spec"),
    ("tickets", "to-tickets"),
    ("implementation", "implement"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackBinding {
    /// 阶段名 → 绑定的完整资源 ID（未绑定为 None）
    pub stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    /// 绑定时的源内容指纹（输入版本的一部分）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_digest: Option<String>,
    /// 解析状态：bound | missing
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub stage: String,
    /// 数据区内相对 workflows/<id>/ 的路径
    pub file: String,
    /// 展示名（可重命名；关联用 id）
    pub title: String,
    pub version: u32,
    pub created_at: String,
    pub updated_at: String,
    /// needs_review 标记（上游 spec 变更后置位）
    #[serde(default)]
    pub needs_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInput {
    pub identity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub pack: Vec<PackBinding>,
    /// 输入版本（源身份/修订/摘要）
    pub inputs: Vec<SourceInput>,
    pub artifacts: Vec<ArtifactRef>,
    /// spec 当前版本；变更自增并使下游 needs_review
    pub spec_version: u32,
    pub downstream_needs_review: bool,
}

fn workflows_root(data_root: &Path) -> PathBuf {
    data_root.join("profile").join("workflows")
}

fn run_dir(data_root: &Path, id: &str) -> PathBuf {
    workflows_root(data_root).join(id)
}

fn run_file(data_root: &Path, id: &str) -> PathBuf {
    run_dir(data_root, id).join("run.json")
}

fn load_run(data_root: &Path, id: &str) -> Result<WorkflowRun> {
    let p = run_file(data_root, id);
    let text = std::fs::read_to_string(&p)
        .map_err(|_| Error::new(code::UNKNOWN_REFERENCE, format!("流程不存在: {id}")))?;
    serde_json::from_str(&text)
        .map_err(|e| Error::new(code::INTERNAL, format!("run.json 损坏: {e}")))
}

fn save_run(data_root: &Path, run: &WorkflowRun) -> Result<()> {
    let p = run_file(data_root, &run.id);
    std::fs::create_dir_all(p.parent().unwrap_or_else(|| Path::new(".")))?;
    crate::sync_common::atomic_write(&p, serde_json::to_vec_pretty(run)?.as_slice())
}

/// 建立新流程（含流程包占位绑定；绑定阶段再解析为实际资源）。
pub fn create(data_root: &Path, name: &str) -> Result<WorkflowRun> {
    let id = format!("wf-{}", &new_id()[..8]);
    let now = now_iso();
    let run = WorkflowRun {
        schema_version: 1,
        id,
        name: name.to_string(),
        created_at: now.clone(),
        updated_at: now,
        pack: PACK_TEMPLATE
            .iter()
            .map(|(stage, hint)| PackBinding {
                stage: stage.to_string(),
                resource_id: None,
                bound_digest: None,
                // 未解析前显式 missing，不假装已安装
                status: format!("missing（待绑定；提示名 {hint}）"),
            })
            .collect(),
        inputs: Vec::new(),
        artifacts: Vec::new(),
        spec_version: 0,
        downstream_needs_review: false,
    };
    save_run(data_root, &run)?;
    Ok(run)
}

fn library_resource_digest(data_root: &Path, resource_id: &str) -> Option<String> {
    // 资源库内按完整 ResourceId 校验真实存在（不凭名字假定）
    let lib = crate::personal_library::library_root(data_root);
    if !lib.is_dir() {
        return None;
    }
    let manifest = crate::manifest::TeamManifest::load_from(&lib).ok()?;
    let entries = crate::resource::enumerate(
        &lib,
        &manifest,
        crate::personal_library::LIBRARY_TEAM_ID,
        &mut Vec::new(),
    )
    .ok()?;
    entries
        .iter()
        .find(|e| e.id.to_string() == resource_id)
        .and_then(|e| {
            let full = lib.join(&e.path);
            if e.id.kind == crate::resource::ResourceKind::Skill {
                crate::store::dir_digest(&full).ok()
            } else {
                std::fs::read(&full).ok().map(|b| sha256_of(&b))
            }
        })
}

fn sha256_of(b: &[u8]) -> String {
    crate::ids::sha256_hex(b)
}

/// 绑定流程包：逐阶段解析资源身份；资源库中找不到 → 显式 missing（不静默装）。
pub fn bind_pack(
    data_root: &Path,
    id: &str,
    stage_resource: &[(String, String)],
) -> Result<WorkflowRun> {
    let mut run = load_run(data_root, id)?;
    for (stage, rid) in stage_resource {
        let binding = run
            .pack
            .iter_mut()
            .find(|b| b.stage == *stage)
            .ok_or_else(|| {
                Error::new(
                    code::USAGE,
                    format!("未知流程阶段: {stage}（可选 {STAGES:?}）"),
                )
            })?;
        match library_resource_digest(data_root, rid) {
            Some(digest) => {
                binding.resource_id = Some(rid.clone());
                binding.bound_digest = Some(digest);
                binding.status = "bound".into();
            }
            None => {
                // 显式缺失：保持 missing，不从未知位置复制
                binding.resource_id = Some(rid.clone());
                binding.bound_digest = None;
                binding.status = "missing（资源库中未找到该资源）".into();
            }
        }
    }
    run.updated_at = now_iso();
    save_run(data_root, &run)?;
    Ok(run)
}

/// 记录一次输入版本（源身份/摘要）。
pub fn record_input(
    data_root: &Path,
    id: &str,
    identity: &str,
    digest: &str,
) -> Result<WorkflowRun> {
    let mut run = load_run(data_root, id)?;
    run.inputs.push(SourceInput {
        identity: identity.to_string(),
        revision: None,
        digest: digest.to_string(),
    });
    run.updated_at = now_iso();
    save_run(data_root, &run)?;
    Ok(run)
}

/// 按资源身份记录输入版本（L06：给页面一个真实入口；摘要取自资源库实际内容，
/// 资源不存在 → 显式报错，不凭名字记录）。
pub fn record_resource_input(data_root: &Path, id: &str, resource_id: &str) -> Result<WorkflowRun> {
    let digest = library_resource_digest(data_root, resource_id).ok_or_else(|| {
        Error::new(
            code::UNKNOWN_REFERENCE,
            format!("资源不存在或不可读: {resource_id}（绑定校验基于真实资源身份）"),
        )
    })?;
    record_input(data_root, id, resource_id, &digest)
}

/// 新增/更新阶段产物（返回产物引用；更新时版本 +1）。
/// L04：`base_version` 提供时必须与当前版本一致（乐观并发），否则拒绝覆盖。
pub fn put_artifact(
    data_root: &Path,
    id: &str,
    stage: &str,
    title: &str,
    content: &str,
    base_version: Option<u32>,
) -> Result<ArtifactRef> {
    if !STAGES.contains(&stage) {
        return Err(Error::new(
            code::USAGE,
            format!("未知流程阶段: {stage}（可选 {STAGES:?}）"),
        ));
    }
    let mut run = load_run(data_root, id)?;
    let artifact_id = format!(
        "art-{}",
        crate::ids::sha256_prefix(format!("{id}/{stage}/{title}").as_bytes(), 8)
    );
    if let (Some(base), Some(existing)) = (
        base_version,
        run.artifacts.iter().find(|a| a.id == artifact_id),
    ) {
        if existing.version != base {
            return Err(Error::new(
                code::PRECONDITION_FAILED,
                format!(
                    "产物版本已变化（期望 v{base}，当前 v{}）——拒绝覆盖，请基于最新版本重试",
                    existing.version
                ),
            )
            .context(serde_json::json!({ "artifact_id": artifact_id, "current_version": existing.version })));
        }
    }
    let file = format!("artifacts/{artifact_id}.md");
    let full = run_dir(data_root, id).join(&file);
    std::fs::create_dir_all(full.parent().unwrap_or_else(|| Path::new(".")))?;
    // AIL-070：更新前把当前正文存为历史版本（可恢复，不随覆盖丢失）
    if full.is_file() {
        let prev_version = run
            .artifacts
            .iter()
            .find(|a| a.id == artifact_id)
            .map(|a| a.version)
            .unwrap_or(1);
        let history = run_dir(data_root, id)
            .join("artifacts")
            .join(format!("{artifact_id}.v{prev_version}.md"));
        std::fs::copy(&full, &history)?;
    }
    crate::sync_common::atomic_write(&full, content.as_bytes())?;
    let now = now_iso();
    let existing = run.artifacts.iter_mut().find(|a| a.id == artifact_id);
    match existing {
        Some(a) => {
            a.version += 1;
            a.updated_at = now;
            a.title = title.to_string();
        }
        None => {
            run.artifacts.push(ArtifactRef {
                id: artifact_id,
                stage: stage.to_string(),
                file,
                title: title.to_string(),
                version: 1,
                created_at: now.clone(),
                updated_at: now,
                needs_review: false,
            });
        }
    }
    // spec 阶段变更：版本提升 + 下游需复核
    if stage == "spec" {
        run.spec_version += 1;
        run.downstream_needs_review = true;
        for a in run.artifacts.iter_mut() {
            if a.stage == "tickets" || a.stage == "implementation" || a.stage == "acceptance" {
                a.needs_review = true;
            }
        }
    }
    run.updated_at = now_iso();
    save_run(data_root, &run)?;
    run.artifacts
        .iter()
        .find(|a| a.title == title && a.stage == stage)
        .cloned()
        .ok_or_else(|| Error::new(code::INTERNAL, "产物引用缺失"))
}

/// 读取产物历史版本正文（AIL-070：可恢复旧版）。
pub fn read_artifact_version(
    data_root: &Path,
    id: &str,
    artifact_id: &str,
    version: u32,
) -> Result<String> {
    let run = load_run(data_root, id)?;
    let a = run
        .artifacts
        .iter()
        .find(|a| a.id == artifact_id)
        .ok_or_else(|| {
            Error::new(
                code::UNKNOWN_REFERENCE,
                format!("产物不存在: {artifact_id}"),
            )
        })?;
    let p = run_dir(data_root, id)
        .join("artifacts")
        .join(format!("{}.v{version}.md", a.id));
    if !p.is_file() {
        return Err(Error::new(
            code::UNKNOWN_REFERENCE,
            format!("历史版本不存在: v{version}"),
        ));
    }
    Ok(std::fs::read_to_string(&p)?)
}

/// 读取产物正文。
pub fn read_artifact(data_root: &Path, id: &str, artifact_id: &str) -> Result<String> {
    let run = load_run(data_root, id)?;
    let a = run
        .artifacts
        .iter()
        .find(|a| a.id == artifact_id)
        .ok_or_else(|| {
            Error::new(
                code::UNKNOWN_REFERENCE,
                format!("产物不存在: {artifact_id}"),
            )
        })?;
    let full = run_dir(data_root, id).join(&a.file);
    Ok(std::fs::read_to_string(&full)?)
}

/// 重命名产物标题（关联按 id，不受重命名影响）。
pub fn rename_artifact(
    data_root: &Path,
    id: &str,
    artifact_id: &str,
    title: &str,
) -> Result<WorkflowRun> {
    let mut run = load_run(data_root, id)?;
    let a = run
        .artifacts
        .iter_mut()
        .find(|a| a.id == artifact_id)
        .ok_or_else(|| {
            Error::new(
                code::UNKNOWN_REFERENCE,
                format!("产物不存在: {artifact_id}"),
            )
        })?;
    a.title = title.to_string();
    run.updated_at = now_iso();
    save_run(data_root, &run)?;
    Ok(run)
}

/// 确认复核（清除 needs_review）。
pub fn mark_reviewed(data_root: &Path, id: &str) -> Result<WorkflowRun> {
    let mut run = load_run(data_root, id)?;
    run.downstream_needs_review = false;
    for a in run.artifacts.iter_mut() {
        a.needs_review = false;
    }
    run.updated_at = now_iso();
    save_run(data_root, &run)?;
    Ok(run)
}

/// 导出预览：目标路径 + 与现有内容的差异 + 目标当前指纹（S03：execute 必须携带
/// 该指纹，确认页面看到的状态就是要覆盖的状态；不绑定预览的写入一律拒绝）。
pub fn export_preview(
    data_root: &Path,
    id: &str,
    artifact_id: &str,
    target: &Path,
) -> Result<serde_json::Value> {
    let content = read_artifact(data_root, id, artifact_id)?;
    let existing = if target.is_file() {
        Some(std::fs::read_to_string(target)?)
    } else {
        None
    };
    let fingerprint = match &existing {
        Some(text) => format!("sha256:{}", sha256_of(text.as_bytes())),
        None => "absent".to_string(),
    };
    // AIL-054：覆盖已有文件必须先展示具体差异
    let diff = existing
        .as_deref()
        .map(|old| simple_line_diff(old, &content))
        .unwrap_or_default();
    Ok(serde_json::json!({
        "target": target,
        "exists": existing.is_some(),
        "same_content": existing.as_deref() == Some(content.as_str()),
        "content_chars": content.chars().count(),
        "target_fingerprint": fingerprint,
        "diff": diff,
        "note": "确认后才写入；execute 必须回传 target_fingerprint；覆盖时旧版本自动备份；已跟踪路径拒绝（公司文件保护）",
    }))
}

/// 简单行级差异（ unified 风格前缀 +/−；AIL-054 预览用，截断到 400 行）。
fn simple_line_diff(old: &str, new: &str) -> String {
    let mut out = String::new();
    for line in old.lines() {
        if !new.lines().any(|l| l == line) {
            out.push_str(&format!(
                "- {line}
"
            ));
        }
    }
    for line in new.lines() {
        if !old.lines().any(|l| l == line) {
            out.push_str(&format!(
                "+ {line}
"
            ));
        }
    }
    if out.lines().count() > 400 {
        let head: Vec<&str> = out.lines().take(400).collect();
        out = format!("{}\n…（差异截断）\n", head.join("\n"));
    }
    out
}

/// 导出执行：写入目标文件。
/// S03/L03：调用方必须先校验目标在授权根内（控制台路由负责）；已跟踪路径拒绝；
/// `expected_fingerprint` 与目标当前状态不一致 → 拒绝覆盖（外部编辑不丢失）。
pub fn export_execute(
    data_root: &Path,
    id: &str,
    artifact_id: &str,
    ws_root: &Path,
    target: &Path,
    expected_fingerprint: Option<&str>,
) -> Result<crate::adapters::common::SkippedLike> {
    let content = read_artifact(data_root, id, artifact_id)?;
    let _ = ws_root;
    // 公司文件守卫：目标在 git 已跟踪列表 → 拒绝
    if crate::personal_instructions::path_is_git_tracked(
        target.parent().unwrap_or(target),
        &target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
    ) {
        return Err(Error::new(
            code::TARGET_CONFLICT,
            "目标已被 Git 跟踪（公司文件），导出拒绝覆盖",
        ));
    }
    // 指纹前置：目标当前状态必须与预览确认时一致
    let current = if target.is_file() {
        format!("sha256:{}", sha256_of(std::fs::read(target)?.as_slice()))
    } else {
        "absent".to_string()
    };
    match expected_fingerprint {
        None => {
            return Err(Error::new(
                code::USAGE,
                "缺少导出确认指纹（必须先预览并回传 target_fingerprint）",
            ));
        }
        Some(expected) if expected != current => {
            return Err(Error::new(
                code::PRECONDITION_FAILED,
                format!("导出目标已变化（预览时 {expected}，当前 {current}），拒绝覆盖"),
            ));
        }
        _ => {}
    }
    // AIL-054：覆盖已有用户文档前，把旧版本备份到数据区（可恢复）
    let mut backup_note = String::new();
    if target.is_file() {
        let backup_dir = data_root
            .join("profile")
            .join("workflows")
            .join(id)
            .join("export-backups");
        std::fs::create_dir_all(&backup_dir)?;
        let name = format!(
            "{}-{}-{}.bak",
            crate::ids::sha256_prefix(artifact_id.as_bytes(), 8),
            crate::ids::sha256_prefix(target.display().to_string().as_bytes(), 8),
            now_iso().replace(':', "")
        );
        let backup_path = backup_dir.join(name);
        std::fs::copy(target, &backup_path)?;
        backup_note = format!("；旧版本已备份到 {}", backup_path.display());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::sync_common::atomic_write(target, content.as_bytes())?;
    Ok(crate::adapters::common::SkippedLike {
        path: format!("{}{}", target.display(), backup_note),
        reason: "exported".into(),
    })
}

pub fn list(data_root: &Path) -> Vec<WorkflowRun> {
    let dir = workflows_root(data_root);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let f = e.path().join("run.json");
            if let Ok(text) = std::fs::read_to_string(&f) {
                if let Ok(r) = serde_json::from_str::<WorkflowRun>(&text) {
                    out.push(r);
                }
            }
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    out
}

pub fn show(data_root: &Path, id: &str) -> Result<WorkflowRun> {
    load_run(data_root, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_bind_put_and_spec_review_flow() {
        let data = tempfile::tempdir().unwrap();
        // 资源库 + skill（用于真实绑定校验）
        crate::personal_library::ensure_library(data.path()).unwrap();
        let src = data.path().join("sk");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "# to-spec\n").unwrap();
        crate::personal_library::import_execute(data.path(), &src, Some("to-spec")).unwrap();

        let run = create(data.path(), "登录重构").unwrap();
        assert!(run.id.starts_with("wf-"));
        assert_eq!(run.pack.len(), 4);
        assert!(
            run.pack.iter().all(|b| b.status.starts_with("missing")),
            "未绑定显式缺失"
        );

        // 绑定：spec 阶段真实存在；align 阶段缺失显式记录
        let run = bind_pack(
            data.path(),
            &run.id,
            &[
                ("spec".into(), "personal/skill/personal/to-spec".into()),
                (
                    "align".into(),
                    "personal/skill/personal/grill-not-installed".into(),
                ),
            ],
        )
        .unwrap();
        let spec = run.pack.iter().find(|b| b.stage == "spec").unwrap();
        assert_eq!(spec.status, "bound");
        assert!(spec.bound_digest.is_some());
        let align = run.pack.iter().find(|b| b.stage == "align").unwrap();
        assert!(
            align.status.contains("missing"),
            "缺项显示缺失: {}",
            align.status
        );

        // 产物：对齐 → 规格 → 票据
        put_artifact(data.path(), &run.id, "align", "对齐记录", "v1 对齐", None).unwrap();
        let _spec_art =
            put_artifact(data.path(), &run.id, "spec", "规格", "spec v1", None).unwrap();
        let tickets =
            put_artifact(data.path(), &run.id, "tickets", "票据", "ticket A", None).unwrap();
        assert!(!tickets.needs_review);

        // 规格变更 → 下游需复核；版本提升
        let spec2 = put_artifact(data.path(), &run.id, "spec", "规格", "spec v2", None).unwrap();
        // L04：过期 base_version 拒绝覆盖
        let stale = put_artifact(data.path(), &run.id, "spec", "规格", "spec v3??", Some(1));
        assert!(stale.is_err(), "过期版本前置应拒绝");
        assert_eq!(spec2.version, 2);
        let run2 = show(data.path(), &run.id).unwrap();
        assert!(run2.downstream_needs_review);
        assert!(run2.spec_version >= 1);
        let tickets2 = run2.artifacts.iter().find(|a| a.id == tickets.id).unwrap();
        assert!(tickets2.needs_review, "规格变更后票据需复核");
        assert!(!spec2.needs_review);

        // 重命名不丢关联
        let run3 = rename_artifact(data.path(), &run.id, &tickets.id, "票据（改名）").unwrap();
        assert!(run3
            .artifacts
            .iter()
            .any(|a| a.id == tickets.id && a.title == "票据（改名）"));
        let content = read_artifact(data.path(), &run.id, &tickets.id).unwrap();
        assert_eq!(content, "ticket A");

        // 复核清除
        let run4 = mark_reviewed(data.path(), &run.id).unwrap();
        assert!(!run4.downstream_needs_review);
        assert!(run4.artifacts.iter().all(|a| !a.needs_review));
    }

    #[test]
    fn library_skill_digest_detects_content_change() {
        let data = tempfile::tempdir().unwrap();
        crate::personal_library::ensure_library(data.path()).unwrap();
        let src = data.path().join("sk");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "# implement\n").unwrap();
        crate::personal_library::import_execute(data.path(), &src, Some("implement")).unwrap();
        let d1 = library_resource_digest(data.path(), "personal/skill/personal/implement");
        assert!(d1.is_some());
        // 内容变化 → 指纹变化（输入版本可检测）
        let lib = crate::personal_library::library_root(data.path());
        let skill_md = lib.join("resources/skills/implement/SKILL.md");
        std::fs::write(&skill_md, "# implement v2\n").unwrap();
        let d2 = library_resource_digest(data.path(), "personal/skill/personal/implement");
        assert_ne!(d1, d2);
    }
}
