//! 控制台任务（AIL-050）：把「计划 → 应用 → 宿主验证 → 撤销」连接为可重入任务。
//!
//! - 计划绑定配置/源/目标指纹：修改任一项后旧计划拒绝应用（不能写错 Worktree）。
//! - 任务幂等：相同幂等键返回同一任务；浏览器断连重连不重复执行（服务端推进）。
//! - 取消只在安全边界（步骤间）停止；部分应用可恢复；撤销仅回滚本任务写入项，
//!   冲突项显式报告，不谎称全部回滚。

use crate::console::{LockExt, ServerState};
use crate::error::Result;
use crate::ids::{new_id, now_iso, sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Plan,
    Apply,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
    /// 服务重启时中断（S04）：保留现场，不自动重放
    Interrupted,
    Undone,
    UndoPartial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub path: String,
    /// 应用前存在
    pub existed: bool,
    /// 应用前内容（存在时），撤销时逐字节恢复
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<Vec<u8>>,
    /// 应用后该路径的指纹（S01）：撤销前必须校验当前内容仍等于本次写入结果，
    /// 用户事后修改过则冲突保留，绝不覆盖。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_hash: Option<String>,
    /// 应用前是符号链接（previous 为链接目标）：删除后撤销需要重建链接而不是写文件
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub was_link: bool,
    /// Skill 链接撤销时恢复同一条托管记账；旧任务无此字段，保持原撤销兼容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_skill: Option<UndoManagedSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoManagedSkill {
    pub manifest_path: PathBuf,
    pub item_key: String,
    pub previous: Option<crate::sync::manifest::ManagedItem>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
}

/// 应用后路径不存在（删除动作）时记录的指纹：撤销只在路径仍不存在时重建。
const ABSENT: &str = "absent";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub created_at: String,
    pub updated_at: String,
    pub root: PathBuf,
    pub scope: Option<String>,
    /// 计划指纹：配置/源/目标任一变化即失配
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// apply 任务引用的 plan 任务
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_job_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub progress: Vec<String>,
    pub result: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<Vec<UndoEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub cancel_requested: bool,
}

fn jobs_dir(data_root: &Path) -> PathBuf {
    data_root.join("console").join("jobs")
}

/// 当前路径的内容指纹（S01）：普通文件按字节；符号链接按链接目标字节；目录按摘要。
/// 撤销前用它判断「当前内容是否仍属于本次写入」。
fn current_fingerprint(target: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(target).ok()?;
    if meta.file_type().is_symlink() {
        let link = std::fs::read_link(target).ok()?;
        return Some(format!(
            "sha256:{}",
            sha256_hex(link.as_os_str().as_encoded_bytes())
        ));
    }
    if meta.is_dir() {
        let digest = crate::store::dir_digest(target).ok()?;
        return Some(format!("dirsha256:{digest}"));
    }
    let bytes = std::fs::read(target).ok()?;
    Some(format!("sha256:{}", sha256_hex(&bytes)))
}

fn persist_job(data_root: &Path, job: &Job) {
    let dir = jobs_dir(data_root);
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(bytes) = serde_json::to_vec_pretty(job) {
        let _ = std::fs::write(dir.join(format!("{}.json", job.id)), bytes);
    }
}

/// 任务 id 只由 `<kind>-<uuid>` 这类字母数字与连字符组成；其他输入一律视为不存在，
/// 防止 `../` 等路径穿越读到任务目录外的文件。
fn is_valid_job_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn load_job(data_root: &Path, id: &str) -> Option<Job> {
    if !is_valid_job_id(id) {
        return None;
    }
    let p = jobs_dir(data_root).join(format!("{id}.json"));
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

/// S04：启动时载入全部持久化任务并规范化状态。
/// Queued/Running 是上个进程的中断遗留 → 标记 interrupted（可继续查看/对成功 plan
/// 再 apply/undo），绝不自动重放执行动作。返回 (任务表, 中断任务 id)。
pub fn load_all_jobs(data_root: &Path) -> (std::collections::BTreeMap<String, Job>, Vec<String>) {
    let mut out = std::collections::BTreeMap::new();
    let mut interrupted = Vec::new();
    let dir = jobs_dir(data_root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (out, interrupted);
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(mut job) = serde_json::from_str::<Job>(&text) else {
            continue;
        };
        if matches!(job.status, JobStatus::Queued | JobStatus::Running) {
            job.status = JobStatus::Interrupted;
            job.progress
                .push("服务重启时任务中断；不自动重放，可重新发起计划/应用".into());
            interrupted.push(job.id.clone());
            let bytes = serde_json::to_vec_pretty(&job).unwrap_or_default();
            let _ = std::fs::write(&p, bytes);
        }
        out.insert(job.id.clone(), job);
    }
    (out, interrupted)
}

/// 计划指纹：profile + 仓库登记 + 资源库内容 + 目标/作用域 + 期望动作摘要。
pub fn compute_fingerprint(
    state: &ServerState,
    root: &Path,
    scope: Option<&str>,
    plan: &crate::sync::plan::SyncPlan,
) -> String {
    let mut material = String::new();
    let profile = crate::profile::PersonalProfile::profile_path(&state.data_root);
    if let Ok(b) = std::fs::read(&profile) {
        material.push_str(&format!("profile:{};", sha256_hex(&b)));
    }
    let lib = crate::personal_library::library_root(&state.data_root);
    if let Ok(b) = std::fs::read(crate::collections::registry_path(&state.data_root)) {
        material.push_str(&format!("collections:{};", sha256_hex(&b)));
    }
    if let Ok(d) = crate::store::dir_digest(&lib) {
        material.push_str(&format!("library:{d};"));
    }
    if let Ok(b) = std::fs::read(root.join(".ailoom").join("project.toml")) {
        material.push_str(&format!("decl:{};", sha256_hex(&b)));
    }
    material.push_str(&format!("root:{};", root.display()));
    material.push_str(&format!("scope:{};", scope.unwrap_or("")));
    for a in &plan.actions {
        let tag = match a.action {
            crate::sync::plan::ActionKind::Create => "create",
            crate::sync::plan::ActionKind::Update => "update",
            crate::sync::plan::ActionKind::Delete => "delete",
            crate::sync::plan::ActionKind::Restore => "restore",
            crate::sync::plan::ActionKind::Conflict => "conflict",
            crate::sync::plan::ActionKind::Noop => "noop",
            crate::sync::plan::ActionKind::Unsupported => "unsupported",
        };
        material.push_str(&format!("{}|{tag}|{};", a.item_key, a.desired_hash));
    }
    sha256_hex(material.as_bytes())
}

/// 创建 plan 任务（后台执行；幂等键复用）。
pub fn spawn_plan(
    state: &Arc<ServerState>,
    root: PathBuf,
    scope: Option<String>,
    idempotency_key: Option<String>,
) -> Result<String> {
    let mut jobs = state.jobs.lock_ok();
    if let Some(key) = &idempotency_key {
        if let Some(existing) = jobs
            .values()
            .find(|j| j.idempotency_key.as_ref() == Some(key))
        {
            let dup = existing.id.clone();
            drop(jobs);
            return Ok(dup);
        }
    }
    let id = format!("plan-{}", new_id());
    let job = Job {
        id: id.clone(),
        kind: JobKind::Plan,
        status: JobStatus::Queued,
        created_at: now_iso(),
        updated_at: now_iso(),
        root: root.clone(),
        scope: scope.clone(),
        fingerprint: None,
        plan_job_id: None,
        idempotency_key,
        progress: vec!["queued".into()],
        result: Value::Null,
        undo: None,
        error: None,
        cancel_requested: false,
    };
    jobs.insert(id.clone(), job.clone());
    persist_job(&state.data_root, &job);
    drop(jobs);
    push_event(
        state,
        json!({ "event": "job-queued", "id": id, "kind": "plan" }),
    );

    let st = Arc::clone(state);
    let thread_id = id.clone();
    std::thread::spawn(move || {
        run_plan_job(&st, &thread_id);
    });
    Ok(id)
}

fn nowstamp() -> String {
    now_iso()
}

fn set_status(state: &ServerState, id: &str, status: JobStatus, note: &str) {
    let mut jobs = state.jobs.lock_ok();
    if let Some(job) = jobs.get_mut(id) {
        if matches!(status, JobStatus::Cancelled) || job.status != JobStatus::Cancelled {
            job.status = status;
        }
        job.updated_at = nowstamp();
        job.progress.push(note.to_string());
        let job = job.clone();
        drop(jobs);
        persist_job(&state.data_root, &job);
        state.events.lock_ok().push(json!({
            "event": "job-progress", "id": id, "status": status, "note": note,
        }));
    }
}

fn is_cancelled(state: &ServerState, id: &str) -> bool {
    state
        .jobs
        .lock()
        .unwrap()
        .get(id)
        .map(|j| j.cancel_requested)
        .unwrap_or(false)
}

/// 同步执行（测试可直接调用；生产经 spawn_plan 后台执行）。
pub fn run_plan_job(state: &Arc<ServerState>, id: &str) {
    set_status(state, id, JobStatus::Running, "计算个人模式计划");
    let (root, scope) = {
        let jobs = state.jobs.lock_ok();
        let Some(job) = jobs.get(id) else { return };
        (job.root.clone(), job.scope.clone())
    };
    let prepared = crate::commands::personal::prepare_personal(
        Some(&state.data_root),
        Some(&root),
        scope.clone(),
        &state.data_root,
    );
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            set_status(state, id, JobStatus::Failed, &format!("计划失败: {e}"));
            return;
        }
    };
    let fingerprint = compute_fingerprint(state, &root, scope.as_deref(), &prepared.plan);
    let summary = prepared.plan.summary();
    let result = json!({
        "summary": summary,
        "actions": prepared.plan.actions,
        "has_conflicts": prepared.plan.has_conflicts(),
        "pending": prepared.plan.actions.iter().filter(|a| !matches!(a.action, crate::sync::plan::ActionKind::Noop)).count(),
        "skipped_company_files": prepared.skipped,
        "notes": prepared.notes,
        "effective_enabled": prepared.effective.enabled_resources(),
        "repo_id": prepared.repo.identity.repo_id,
        "worktree_id": prepared.wt_id,
        // AIL-125：预览必须绑定目标与作用域（前端显示真实作用域，不冒充 Worktree 根）。
        "active_rel": scope,
        "scope_dir": prepared.scope_dir.display().to_string(),
    });
    // 同 AIL-072 竞态修复：指纹/result 与 Success 原子写入
    let mut jobs = state.jobs.lock_ok();
    if let Some(job) = jobs.get_mut(id) {
        job.status = JobStatus::Success;
        job.fingerprint = Some(fingerprint);
        job.result = result;
        job.updated_at = nowstamp();
        job.progress.push("计划完成".into());
        let job = job.clone();
        drop(jobs);
        persist_job(&state.data_root, &job);
    }
}

/// 创建 apply 任务：引用 plan 任务并校验指纹未过期。
pub fn spawn_apply(
    state: &Arc<ServerState>,
    plan_job_id: &str,
    idempotency_key: Option<String>,
) -> std::result::Result<String, String> {
    let (plan_fingerprint, root, scope) = {
        let jobs = state.jobs.lock_ok();
        let Some(plan_job) = jobs.get(plan_job_id) else {
            return Err("计划任务不存在".into());
        };
        if plan_job.status != JobStatus::Success {
            return Err("计划任务未成功完成，不能应用".into());
        }
        (
            plan_job.fingerprint.clone(),
            plan_job.root.clone(),
            plan_job.scope.clone(),
        )
    };
    let Some(plan_fingerprint) = plan_fingerprint else {
        return Err("计划缺少指纹，拒绝应用".into());
    };
    let mut jobs = state.jobs.lock_ok();
    if let Some(key) = &idempotency_key {
        if let Some(existing) = jobs
            .values()
            .find(|j| j.idempotency_key.as_ref() == Some(key))
        {
            return Ok(existing.id.clone());
        }
    }
    let id = format!("apply-{}", new_id());
    let job = Job {
        id: id.clone(),
        kind: JobKind::Apply,
        status: JobStatus::Queued,
        created_at: nowstamp(),
        updated_at: nowstamp(),
        root,
        scope,
        fingerprint: Some(plan_fingerprint),
        plan_job_id: Some(plan_job_id.to_string()),
        idempotency_key,
        progress: vec!["queued".into()],
        result: Value::Null,
        undo: None,
        error: None,
        cancel_requested: false,
    };
    jobs.insert(id.clone(), job.clone());
    persist_job(&state.data_root, &job);
    drop(jobs);
    push_event(
        state,
        json!({ "event": "job-queued", "id": id, "kind": "apply" }),
    );
    let st = Arc::clone(state);
    let thread_id = id.clone();
    std::thread::spawn(move || {
        run_apply_job(&st, &thread_id);
    });
    Ok(id)
}

pub fn run_apply_job(state: &Arc<ServerState>, id: &str) {
    if is_cancelled(state, id) {
        set_status(state, id, JobStatus::Cancelled, "已取消（启动前安全边界）");
        return;
    }
    set_status(state, id, JobStatus::Running, "校验计划指纹");
    let (root, scope, fingerprint) = {
        let jobs = state.jobs.lock_ok();
        let Some(job) = jobs.get(id) else { return };
        (
            job.root.clone(),
            job.scope.clone(),
            job.fingerprint.clone().unwrap_or_default(),
        )
    };
    // 取消边界：执行前
    if is_cancelled(state, id) {
        set_status(state, id, JobStatus::Cancelled, "已取消（安全边界）");
        return;
    }
    let prepared = match crate::commands::personal::prepare_personal(
        Some(&state.data_root),
        Some(&root),
        scope.clone(),
        &state.data_root,
    ) {
        Ok(p) => p,
        Err(e) => {
            set_status(state, id, JobStatus::Failed, &format!("重新准备失败: {e}"));
            return;
        }
    };
    // 指纹校验：配置/源/目标任一变化 → 旧计划拒绝
    let current = compute_fingerprint(state, &root, scope.as_deref(), &prepared.plan);
    if current != fingerprint {
        set_status(
            state,
            id,
            JobStatus::Failed,
            "计划已过期（配置/源/目标已变化），旧计划拒绝应用",
        );
        let mut jobs = state.jobs.lock_ok();
        if let Some(job) = jobs.get_mut(id) {
            job.error = Some("stale-plan".into());
            job.result = json!({ "stale": true, "current_fingerprint": current });
            job.updated_at = nowstamp();
            let job = job.clone();
            drop(jobs);
            // AIL-125：error/result 必须落盘，前端才能区分「过期」与其他失败。
            persist_job(&state.data_root, &job);
        }
        return;
    }
    // 记录撤销前像：仅本任务将写入的目标（AIL-120：捕获逻辑抽为共用函数）
    let mut undo = capture_undo_entries(&root, &prepared);
    // 应用：复用 personal::apply_prepared_personal（相同 lock/journal 管道），但要捕获其结果——
    // 这里直接调用库路径以保证 undo 语义
    set_status(state, id, JobStatus::Running, "应用计划");
    if is_cancelled(state, id) {
        set_status(state, id, JobStatus::Cancelled, "已取消（应用前安全边界）");
        return;
    }
    let apply_result = apply_prepared(state, &prepared, &root, &undo);
    let report = match apply_result {
        Ok(r) => r,
        Err(e) => {
            set_status(state, id, JobStatus::Failed, &format!("应用失败: {e}"));
            return;
        }
    };
    // S01：记录每个写入项的「应用后指纹」。撤销前据此判断当前内容是否仍属于
    // 本次写入——用户事后修改过的项必须冲突保留，绝不以旧前像覆盖。
    if report.ok {
        for entry in &mut undo {
            entry.after_hash =
                Some(current_fingerprint(&root.join(&entry.path)).unwrap_or_else(|| ABSENT.into()));
        }
    }
    // 宿主验证（静态：部署存在性 + 能力矩阵状态；不宣称宿主已加载）
    set_status(state, id, JobStatus::Running, "宿主验证");
    let verification = verify_deployment(&prepared, &root);
    // 终态与 result 必须原子写入：先写 result 再标 Success，
    // 否则轮询方可能读到「Success 但 result 为空」（AIL-072 竞态修复）
    let status = if report.ok {
        JobStatus::Success
    } else {
        JobStatus::Failed
    };
    {
        let mut jobs = state.jobs.lock_ok();
        if let Some(job) = jobs.get_mut(id) {
            job.status = status;
            job.undo = Some(undo);
            // 失败原因与恢复点必须随结果返回，否则界面只能显示「未知错误」且无法撤回。
            job.result = json!({
                "ok": report.ok,
                "applied": report.applied,
                "noop": report.noop,
                "skipped_conflicts": report.skipped_conflicts,
                "skipped_unsupported": report.skipped_unsupported,
                "failed": report.failed,
                "pending_journal": report.pending_journal,
                "skipped_company_files": prepared.skipped,
                "verification": verification,
                "notes": prepared.notes,
                "next": if report.ok {
                    "在宿主新会话中真实调用一次以确认加载（文件落盘不等于宿主已加载）"
                } else {
                    "应用中途失败：已写入的部分保留在恢复点中，可在界面撤回或运行 ailoom personal --action recover"
                },
            });
            job.updated_at = nowstamp();
            job.progress.push(if report.ok {
                format!(
                    "应用完成：写入 {} 项，冲突跳过 {} 项",
                    report.applied.len(),
                    report.skipped_conflicts.len()
                )
            } else {
                format!(
                    "应用中途失败：已写入 {} 项后停止（{}）",
                    report.applied.len(),
                    report
                        .failed
                        .as_ref()
                        .and_then(|f| f["message"].as_str())
                        .unwrap_or("未知错误")
                )
            });
            state.events.lock_ok().push(json!({
                "event": "job-progress", "id": id, "status": status,
            }));
            let job = job.clone();
            drop(jobs);
            persist_job(&state.data_root, &job);
        }
    }
}

fn apply_prepared(
    state: &ServerState,
    prepared: &crate::commands::personal::PersonalPrepare,
    _root: &Path,
    _undo: &[UndoEntry],
) -> Result<crate::sync::apply::ApplyReport> {
    // 与 CLI sync 相同的 apply 管道（lock/journal/managed save + 公司文件复核）
    let _ = state;
    crate::commands::personal::apply_prepared_personal(prepared)
}

/// 宿主验证：区分「已保存/已部署/需新会话/不支持」；不把文件落盘当作宿主已加载。
fn verify_deployment(prepared: &crate::commands::personal::PersonalPrepare, root: &Path) -> Value {
    let mut items = Vec::new();
    for a in &prepared.artifacts {
        let target = root.join(&a.path);
        let deployed = target.symlink_metadata().is_ok();
        let (host_state, note) = match (a.target_tool.as_str(), a.kind.as_str()) {
            ("claude", "skill") | ("codex", "skill") => (
                "needs-new-session",
                "文件/链接已落盘；宿主在**新会话**中重新扫描后可见（真实调用后才算通过）",
            ),
            ("claude", "mcp") => (
                "needs-approval",
                "MCP 配置已写入；首次连接需要用户在宿主内批准；批准并调用成功后才算可用",
            ),
            ("codex", "mcp") => (
                "host-unverified",
                "Codex 0.154.0 实测不加载项目级 MCP（见能力矩阵）；配置保留，宿主修复后自动生效",
            ),
            ("claude", "rule") | ("codex", "rule") => (
                "needs-new-session",
                "指令条目已落盘；新会话加载；Codex 视图包含公司基线全文",
            ),
            ("codex", "skill-config") => {
                ("needs-new-session", "skills.config 已更新；重启宿主后生效")
            }
            _ => ("deployed", "已写入"),
        };
        items.push(json!({
            "resource_id": a.resource_id,
            "tool": a.target_tool,
            "kind": a.kind,
            "path": a.path,
            "deployed": deployed,
            "host_state": if deployed { host_state } else { "missing" },
            "note": if deployed { note } else { "未写入：本次同步中途失败或目标不可写" },
        }));
    }
    json!({ "items": items, "invocation": "manual", "invocation_note": "真实调用验证由用户在宿主内执行（onboarding 提供下一步动作与验证提示）" })
}

/// 取消请求（安全边界生效）。
pub fn request_cancel(state: &Arc<ServerState>, id: &str) -> bool {
    let mut jobs = state.jobs.lock_ok();
    if let Some(job) = jobs.get_mut(id) {
        job.cancel_requested = true;
        true
    } else {
        false
    }
}

fn push_event(state: &ServerState, event: Value) {
    state.events.lock_ok().push(event);
}

/// 撤销：仅回滚本任务写入的项；冲突显式报告。
/// S01：撤销前校验当前内容仍等于应用后指纹（after_hash）；用户事后修改或删除过
/// 的项一律冲突保留——保存前像不等于有权覆盖后来的人工修改。
/// AIL-053：可重入——UndoPartial 后可再次撤销，仅重试剩余未处理项；
/// 已恢复项从清单移除，绝不重复覆盖。
/// 捕获撤销前像：仅统计将写入（Create/Update/Restore）的目标。
fn capture_undo_entries(
    root: &Path,
    prepared: &crate::commands::personal::PersonalPrepare,
) -> Vec<UndoEntry> {
    let mut undo: Vec<UndoEntry> = Vec::new();
    for a in &prepared.plan.actions {
        if !matches!(
            a.action,
            crate::sync::plan::ActionKind::Create
                | crate::sync::plan::ActionKind::Update
                | crate::sync::plan::ActionKind::Restore
                | crate::sync::plan::ActionKind::Delete
        ) {
            continue;
        }
        let target = root.join(&a.path);
        let existed = target.symlink_metadata().is_ok();
        let was_link = target
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink());
        let previous = if existed {
            let meta = std::fs::symlink_metadata(&target).ok();
            if meta.map(|m| m.file_type().is_symlink()).unwrap_or(false) {
                std::fs::read_link(&target)
                    .ok()
                    .map(|l| l.as_os_str().as_encoded_bytes().to_vec())
            } else {
                std::fs::read(&target).ok()
            }
        } else {
            None
        };
        undo.push(UndoEntry {
            path: a.path.clone(),
            existed,
            previous,
            after_hash: None,
            was_link,
            managed_skill: a.item_key.ends_with("#symlink").then(|| UndoManagedSkill {
                manifest_path: prepared.managed_path.clone(),
                item_key: a.item_key.clone(),
                previous: prepared.managed.items.get(&a.item_key).cloned(),
                before_hash: a.precondition_hash.clone(),
                after_hash: (a.action != crate::sync::plan::ActionKind::Delete)
                    .then(|| a.desired_hash.clone()),
            }),
        });
    }
    undo
}

/// CLI 撤销：基于持久化任务（Web 或 CLI sync 创建的 Apply 任务均可撤销）。
pub fn undo_persisted(data_root: &Path, id: &str) -> std::result::Result<Value, String> {
    let mut jobs = load_all_jobs(data_root).0;
    let Some(job) = jobs.get_mut(id) else {
        return Err("任务不存在".into());
    };
    let result = undo_apply(job);
    let j = job.clone();
    persist_job(data_root, &j);
    result
}

/// 撤销核心（CLI 与控制台共用）：校验可撤销性，执行回滚并就地更新任务状态。
pub fn undo_apply(job: &mut Job) -> std::result::Result<Value, String> {
    if job.kind != JobKind::Apply {
        return Err("只有应用任务可撤销".into());
    }
    if !matches!(job.status, JobStatus::Success | JobStatus::UndoPartial) {
        return Err(format!(
            "任务状态 {:?} 不可撤销（仅 success 或可重试的部分撤销）",
            job.status
        ));
    }
    let Some(undo_entries) = &job.undo else {
        return Err("任务缺少撤销清单".into());
    };
    if undo_entries.is_empty() {
        return Err("撤销清单已全部处理（无剩余项）".into());
    }
    let root = job.root.clone();
    let mut restored = Vec::new();
    let mut conflicts = Vec::new();
    for entry in undo_entries {
        match undo_single(&root, entry) {
            Ok(()) => restored.push(entry.path.clone()),
            Err(reason) => conflicts.push(reason),
        }
    }
    job.status = if conflicts.is_empty() {
        JobStatus::Undone
    } else {
        JobStatus::UndoPartial
    };
    let remaining: Vec<UndoEntry> = undo_entries
        .iter()
        .filter(|e| !restored.contains(&e.path))
        .cloned()
        .collect();
    job.undo = Some(remaining);
    job.updated_at = nowstamp();
    job.progress.push(format!(
        "撤销：恢复 {} 项，冲突 {} 项{}",
        restored.len(),
        conflicts.len(),
        if conflicts.is_empty() {
            String::new()
        } else {
            "（剩余项可再次撤销重试）".to_string()
        }
    ));
    Ok(json!({
        "restored": restored,
        "conflicts": conflicts,
        "retryable": !conflicts.is_empty(),
        "note": if conflicts.is_empty() { "本任务写入项已全部回滚".to_string() } else { "部分回滚：冲突项保留（用户事后修改过），不谎称全部恢复；处理冲突后可再次撤销重试剩余项".to_string() },
    }))
}

/// CLI 同步：与 Web 应用同一管道（公司文件守卫 + lock/journal + 托管清单），
/// 并持久化带撤销清单的 Apply 任务，使 CLI 部署同样可撤销（AIL-120 对齐）。
pub fn cli_sync(
    data_root: &Path,
    root: &Path,
    scope: Option<String>,
    data_root_opt: Option<&Path>,
    data_root_resolved: &Path,
) -> Result<Value> {
    use crate::ids::new_id;
    // 保留原始错误码（如 E3004 / E5002），不在 CLI 层包成 E9000。
    let prepared = crate::commands::personal::prepare_personal(
        data_root_opt,
        Some(root),
        scope.clone(),
        data_root_resolved,
    )?;
    // Plan paths are relative to the discovered workspace, not the invoking subdirectory.
    let root = prepared.ctx.workspace.workspace_root.as_path();
    let mut undo = capture_undo_entries(root, &prepared);
    let report = crate::commands::personal::apply_prepared_personal(&prepared)?;
    if report.ok {
        for entry in &mut undo {
            entry.after_hash =
                Some(current_fingerprint(&root.join(&entry.path)).unwrap_or_else(|| ABSENT.into()));
        }
    }
    let verification = verify_deployment(&prepared, root);
    let status = if report.ok {
        JobStatus::Success
    } else {
        JobStatus::Failed
    };
    let result = json!({
        "ok": report.ok,
        "applied": report.applied,
        "noop": report.noop,
        "skipped_conflicts": report.skipped_conflicts,
        "skipped_unsupported": report.skipped_unsupported,
        "failed": report.failed,
        "pending_journal": report.pending_journal,
        "skipped_company_files": prepared.skipped,
        "verification": verification,
        "notes": prepared.notes,
        "next": "在宿主新会话中真实调用一次以确认加载（文件落盘不等于宿主已加载）",
    });
    let stamp = nowstamp();
    let job_id = new_id();
    let mut result = result;
    // 撤销需要任务 ID：CLI 之前不输出，用户只能去数据目录里翻（盲测发现）
    result["job_id"] = json!(job_id);
    if report.ok && !undo.is_empty() {
        result["undo_hint"] = json!(format!(
            "撤销本次同步：ailoom personal --action undo --id {job_id}"
        ));
    }
    let job = Job {
        id: job_id,
        kind: JobKind::Apply,
        status,
        created_at: stamp.clone(),
        updated_at: stamp,
        root: root.to_path_buf(),
        scope,
        fingerprint: None,
        plan_job_id: None,
        idempotency_key: None,
        progress: vec![format!(
            "CLI sync 应用完成：写入 {} 项，冲突跳过 {} 项",
            report.applied.len(),
            report.skipped_conflicts.len()
        )],
        result: result.clone(),
        undo: Some(undo),
        error: None,
        cancel_requested: false,
    };
    persist_job(data_root, &job);
    // 与团队 sync 一致：中途失败时以非 0 退出，并给出恢复路径（已写入部分保留在 journal 中）。
    if !report.ok {
        let reason = report
            .failed
            .as_ref()
            .and_then(|f| f["message"].as_str())
            .unwrap_or("未知错误")
            .to_string();
        return Err(crate::error::Error::new(
            crate::error::code::WRITE_FAILED,
            format!("同步未全部完成：{reason}"),
        )
        .context(result)
        .fix("修正问题后运行 ailoom personal --action recover 撤回已写入的部分，再重新同步"));
    }
    Ok(result)
}

pub fn undo(state: &Arc<ServerState>, id: &str) -> std::result::Result<Value, String> {
    let result = {
        let mut jobs = state.jobs.lock_ok();
        match jobs.get_mut(id) {
            // AIL-120：撤销语义收敛到 undo_apply 核心，CLI 持久化任务走同一条路
            Some(job) => undo_apply(job),
            None => return Err("任务不存在".into()),
        }
    };
    if let Ok(v) = &result {
        if let Some(restored) = v["restored"].as_array() {
            push_event(
                state,
                json!({ "event": "job-undo", "id": id, "restored": restored.len() }),
            );
        }
    }
    let jobs = state.jobs.lock_ok();
    if let Some(j) = jobs.get(id) {
        let j = j.clone();
        drop(jobs);
        persist_job(&state.data_root, &j);
    }
    result
}

/// 单条撤销（AIL-053）：指纹校验 + 类型保持的前像恢复。
/// 返回 Err(冲突原因) 表示该项保留、进入可重试剩余清单。
fn undo_single(root: &Path, entry: &UndoEntry) -> std::result::Result<(), String> {
    let Some(owned) = &entry.managed_skill else {
        return undo_path(root, entry);
    };
    let manifest_dir = owned.manifest_path.parent().ok_or("托管清单缺少父目录")?;
    let lock = crate::sync::lock::SyncLock::acquire(&manifest_dir.join("locks"), "undo")
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let mut manifest = crate::sync::manifest::ManagedManifest::load(&owned.manifest_path)
            .map_err(|e| e.to_string())?
            .ok_or("托管清单已消失，无法安全撤销")?;
        let current = manifest.items.get(&owned.item_key);
        if current.map(|item| &item.content_hash) != owned.after_hash.as_ref() {
            return Err(format!("{}（托管记账已变化，冲突保留）", entry.path));
        }
        let current_hash = crate::sync::plan::current_hash_by_key(root, &owned.item_key, "")
            .map_err(|e| e.to_string())?;
        if current_hash != owned.after_hash {
            return Err(format!(
                "{}（应用后 Skill 内容已变化，冲突保留）",
                entry.path
            ));
        }
        // 旧实体也可能经其他项目的链接被用户修改；这种情况下不能声称恢复旧内容。
        if entry.was_link {
            let prior_hash = entry.previous.as_ref().and_then(|bytes| {
                let link = PathBuf::from(String::from_utf8_lossy(bytes).as_ref());
                let abs = if link.is_absolute() {
                    link
                } else {
                    root.join(&entry.path).parent()?.join(link)
                };
                if !abs.is_dir() {
                    return None;
                }
                let digest = crate::store::dir_digest(&abs).ok()?;
                Some(format!(
                    "sha256:{}",
                    sha256_hex(format!("{}|{digest}", abs.display()).as_bytes())
                ))
            });
            if prior_hash != owned.before_hash {
                return Err(format!("{}（旧 Skill 实体已变化，冲突保留）", entry.path));
            }
        }
        undo_path(root, entry)?;
        let restored_hash = crate::sync::plan::current_hash_by_key(root, &owned.item_key, "")
            .map_err(|e| e.to_string())?;
        if restored_hash != owned.before_hash {
            return Err(format!(
                "{}（恢复后 Skill 摘要不符，未改托管记账）",
                entry.path
            ));
        }
        if let Some(previous) = &owned.previous {
            manifest
                .items
                .insert(owned.item_key.clone(), previous.clone());
        } else {
            manifest.items.remove(&owned.item_key);
        }
        manifest
            .save(&owned.manifest_path)
            .map_err(|e| e.to_string())
    })();
    let released = lock.release().map_err(|e| e.to_string());
    result.and(released)
}

fn undo_path(root: &Path, entry: &UndoEntry) -> std::result::Result<(), String> {
    let target = root.join(&entry.path);
    // 守卫一（AIL-052）：本任务「新建」的路径后来被 Git 跟踪 → 撤销的删除会
    // 销毁公司内容，拒绝；已存在文件恢复前像只是回到公司原有内容，允许。
    if !entry.existed && crate::personal_instructions::path_is_git_tracked(root, &entry.path) {
        return Err(format!(
            "{}（路径现已被 Git 跟踪，公司文件保护：不撤销删除）",
            entry.path
        ));
    }
    // 守卫二（S01）：当前内容与本任务应用后的指纹不一致 → 用户后改，冲突保留
    match (&entry.after_hash, current_fingerprint(&target)) {
        (Some(expected), None) if expected == ABSENT => {}
        (Some(expected), Some(_)) if expected == ABSENT => {
            return Err(format!(
                "{}（删除后该路径又被重新创建，冲突保留不覆盖）",
                entry.path
            ));
        }
        (Some(expected), Some(current)) if expected == &current => {}
        (Some(_), None) => {
            return Err(format!("{}（目标已被删除，不重建旧内容）", entry.path));
        }
        (Some(_), Some(_)) => {
            return Err(format!(
                "{}（应用后被用户修改，冲突保留不覆盖）",
                entry.path
            ));
        }
        // 旧任务（无 after_hash，升级前的持久化数据）：无法证明归属 → 一律冲突
        (None, _) => {
            return Err(format!(
                "{}（任务缺少应用后指纹，无法安全撤销；请手工处理）",
                entry.path
            ));
        }
    }
    let current_exists = target.symlink_metadata().is_ok();
    match (&entry.previous, entry.existed) {
        (Some(prev), true) if !current_exists => {
            // 本任务删除的条目：按原样重建（守卫二已确认删除后没人再放内容）
            if entry.after_hash.as_deref() != Some(ABSENT) {
                return Err(format!("{}（目标已消失）", entry.path));
            }
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let ok = if entry.was_link {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(String::from_utf8_lossy(prev).as_ref(), &target)
                        .is_ok()
                }
                #[cfg(not(unix))]
                {
                    false
                }
            } else {
                std::fs::write(&target, prev).is_ok()
            };
            if ok {
                Ok(())
            } else {
                Err(entry.path.clone())
            }
        }
        (Some(prev), true) => {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // 符号链接：previous 是链接目标字节
            let is_link = std::fs::symlink_metadata(&target)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            let ok = if is_link {
                #[cfg(unix)]
                {
                    let target_str = String::from_utf8_lossy(prev).to_string();
                    let _ = std::fs::remove_file(&target);
                    std::os::unix::fs::symlink(&target_str, &target).is_ok()
                }
                #[cfg(not(unix))]
                {
                    let _ = std::fs::write(&target, prev);
                    true
                }
            } else {
                std::fs::write(&target, prev).is_ok()
            };
            if ok {
                Ok(())
            } else {
                Err(entry.path.clone())
            }
        }
        (None, false) => {
            // 应用前不存在 → 删除本次创建的条目（目录仅在仍为本次内容时删除，
            // 不递归吞掉用户后放入的内容——指纹校验已保证目录属于本任务）
            if !current_exists {
                return Ok(());
            }
            let is_link = std::fs::symlink_metadata(&target)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            let r = if is_link || target.is_file() {
                std::fs::remove_file(&target)
            } else if target.is_dir() {
                std::fs::remove_dir_all(&target)
            } else {
                std::fs::remove_file(&target)
            };
            if r.is_ok() {
                Ok(())
            } else {
                Err(entry.path.clone())
            }
        }
        _ => Err(entry.path.clone()),
    }
}

/// 测试支撑（AIL-053 回归）：以当前盘上内容构造撤销条目（after_hash 取现值）。
pub fn test_support_undo_entry(
    root: &Path,
    path: &str,
    existed: bool,
    previous: Option<Vec<u8>>,
) -> UndoEntry {
    let after_hash = current_fingerprint(&root.join(path));
    UndoEntry {
        path: path.to_string(),
        existed,
        previous,
        after_hash,
        was_link: false,
        managed_skill: None,
    }
}

/// 测试支撑（AIL-053 回归）：对一组撤销条目执行与 undo 相同的单条校验/恢复，
/// 返回冲突原因列表（公开给集成测试验证指纹保护逻辑）。
pub fn test_support_undo_check(root: &Path, entries: &[UndoEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|e| undo_single(root, e).err())
        .collect()
}

/// 从磁盘载入任务（重连恢复视图用）。
pub fn list_persisted(state: &ServerState) -> Vec<Job> {
    state.jobs.lock_ok().values().cloned().collect()
}

#[cfg(test)]
mod job_id_tests {
    use super::*;

    #[test]
    fn load_job_rejects_path_traversal_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        std::fs::create_dir_all(jobs_dir(&data)).unwrap();
        std::fs::write(tmp.path().join("outside.json"), "{}").unwrap();
        for id in ["../../outside", "..", "a/b", "", "x.y"] {
            assert!(load_job(&data, id).is_none(), "{id}");
        }
        assert!(is_valid_job_id(&format!("plan-{}", crate::ids::new_id())));
    }
}
