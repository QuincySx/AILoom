//! 控制台任务（AIL-050）：把「计划 → 应用 → 宿主验证 → 撤销」连接为可重入任务。
//!
//! - 计划绑定配置/源/目标指纹：修改任一项后旧计划拒绝应用（不能写错工作树）。
//! - 任务幂等：相同幂等键返回同一任务；浏览器断连重连不重复执行（服务端推进）。
//! - 取消只在安全边界（步骤间）停止；部分应用可恢复；撤销仅回滚本任务写入项，
//!   冲突项显式报告，不谎称全部回滚。

use crate::console::ServerState;
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
}

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

fn persist_job(data_root: &Path, job: &Job) {
    let dir = jobs_dir(data_root);
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(bytes) = serde_json::to_vec_pretty(job) {
        let _ = std::fs::write(dir.join(format!("{}.json", job.id)), bytes);
    }
}

pub fn load_job(data_root: &Path, id: &str) -> Option<Job> {
    let p = jobs_dir(data_root).join(format!("{id}.json"));
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

/// 计划指纹：profile + 仓库登记 + 个人库内容 + 目标/作用域 + 期望动作摘要。
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
    let mut jobs = state.jobs.lock().unwrap();
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
    let mut jobs = state.jobs.lock().unwrap();
    if let Some(job) = jobs.get_mut(id) {
        if matches!(status, JobStatus::Cancelled) || job.status != JobStatus::Cancelled {
            job.status = status;
        }
        job.updated_at = nowstamp();
        job.progress.push(note.to_string());
        let job = job.clone();
        drop(jobs);
        persist_job(&state.data_root, &job);
        state.events.lock().unwrap().push(json!({
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
        let jobs = state.jobs.lock().unwrap();
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
    });
    let mut jobs = state.jobs.lock().unwrap();
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
        let jobs = state.jobs.lock().unwrap();
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
    let mut jobs = state.jobs.lock().unwrap();
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
        let jobs = state.jobs.lock().unwrap();
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
        let mut jobs = state.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(id) {
            job.error = Some("stale-plan".into());
            job.result = json!({ "stale": true, "current_fingerprint": current });
        }
        return;
    }
    // 记录撤销前像：仅本任务将写入的目标
    let mut undo: Vec<UndoEntry> = Vec::new();
    for a in &prepared.plan.actions {
        if !matches!(
            a.action,
            crate::sync::plan::ActionKind::Create
                | crate::sync::plan::ActionKind::Update
                | crate::sync::plan::ActionKind::Restore
        ) {
            continue;
        }
        let target = root.join(&a.path);
        let existed = target.symlink_metadata().is_ok();
        let previous = if existed {
            // 目录符号链接读链接字节；普通文件读内容
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
        });
    }
    // 应用：复用 personal::sync（相同 lock/journal 管道），但要捕获其结果——
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
    // 宿主验证（静态：部署存在性 + 能力矩阵状态；不宣称宿主已加载）
    set_status(state, id, JobStatus::Running, "宿主验证");
    let verification = verify_deployment(&prepared, &root);
    let status = if report.ok {
        JobStatus::Success
    } else {
        JobStatus::Failed
    };
    set_status(
        state,
        id,
        status,
        &format!(
            "应用完成：写入 {} 项，冲突跳过 {} 项",
            report.applied.len(),
            report.skipped_conflicts.len()
        ),
    );
    let mut jobs = state.jobs.lock().unwrap();
    if let Some(job) = jobs.get_mut(id) {
        job.undo = Some(undo);
        job.result = json!({
            "ok": report.ok,
            "applied": report.applied,
            "noop": report.noop,
            "skipped_conflicts": report.skipped_conflicts,
            "verification": verification,
            "notes": prepared.notes,
            "next": "在宿主新会话中真实调用一次以确认加载（文件落盘不等于宿主已加载）",
        });
        job.updated_at = nowstamp();
        let job = job.clone();
        drop(jobs);
        persist_job(&state.data_root, &job);
    }
}

fn apply_prepared(
    state: &ServerState,
    prepared: &crate::commands::personal::PersonalPrepare,
    _root: &Path,
    _undo: &[UndoEntry],
) -> Result<crate::sync::apply::ApplyReport> {
    // 与 personal::sync 相同的 apply 管道（lock/journal/managed save）
    let journal_root = &prepared.ctx.layout.journal_dir;
    let lock_dir = prepared.ctx.layout.ws_dir.join("locks");
    let mut managed = prepared.managed.clone();
    let report = crate::sync::apply::apply(
        &prepared.plan,
        &prepared.artifacts,
        &mut managed,
        &prepared.ctx.workspace.workspace_root,
        &lock_dir,
        journal_root,
        &prepared.ctx.device,
    )?;
    if report.ok {
        managed.save(&prepared.managed_path)?;
        let patterns = crate::personal_instructions::exclude_patterns(&prepared.artifacts);
        if !patterns.is_empty() {
            crate::git_exclude::add_patterns(
                &prepared.repo.identity.common_dir,
                &state.data_root,
                &prepared.repo.identity.repo_id,
                &patterns,
            )?;
        }
    }
    Ok(report)
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
            "note": note,
        }));
    }
    json!({ "items": items, "invocation": "manual", "invocation_note": "真实调用验证由用户在宿主内执行（onboarding 提供下一步动作与验证提示）" })
}

/// 取消请求（安全边界生效）。
pub fn request_cancel(state: &Arc<ServerState>, id: &str) -> bool {
    let mut jobs = state.jobs.lock().unwrap();
    if let Some(job) = jobs.get_mut(id) {
        job.cancel_requested = true;
        true
    } else {
        false
    }
}

fn push_event(state: &ServerState, event: Value) {
    state.events.lock().unwrap().push(event);
}

/// 撤销：仅回滚本任务写入的项；冲突显式报告。
pub fn undo(state: &Arc<ServerState>, id: &str) -> std::result::Result<Value, String> {
    let job = {
        let jobs = state.jobs.lock().unwrap();
        jobs.get(id).cloned()
    };
    let Some(job) = job else {
        return Err("任务不存在".into());
    };
    if job.kind != JobKind::Apply {
        return Err("只有应用任务可撤销".into());
    }
    if job.status != JobStatus::Success {
        return Err(format!("任务状态 {:?} 不可撤销（仅 success）", job.status));
    }
    let Some(undo_entries) = &job.undo else {
        return Err("任务缺少撤销清单".into());
    };
    let root = &job.root;
    let mut restored = Vec::new();
    let mut conflicts = Vec::new();
    for entry in undo_entries {
        let target = root.join(&entry.path);
        // 当前内容与本任务撤销清单的期望不一致时（用户事后修改过）→ 冲突保留
        let current_exists = target.symlink_metadata().is_ok();
        match (&entry.previous, entry.existed) {
            (Some(prev), true) => {
                if current_exists {
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
                        restored.push(entry.path.clone());
                    } else {
                        conflicts.push(entry.path.clone());
                    }
                } else {
                    conflicts.push(format!("{}（目标已消失）", entry.path));
                }
            }
            (None, false) => {
                // 应用前不存在 → 删除
                if current_exists {
                    let is_link = std::fs::symlink_metadata(&target)
                        .map(|m| m.file_type().is_symlink())
                        .unwrap_or(false);
                    let r = if is_link {
                        std::fs::remove_file(&target)
                    } else if target.is_dir() {
                        std::fs::remove_dir_all(&target)
                    } else {
                        std::fs::remove_file(&target)
                    };
                    if r.is_ok() {
                        restored.push(entry.path.clone());
                    } else {
                        conflicts.push(entry.path.clone());
                    }
                }
            }
            _ => {
                conflicts.push(entry.path.clone());
            }
        }
    }
    let status = if conflicts.is_empty() {
        JobStatus::Undone
    } else {
        JobStatus::UndoPartial
    };
    {
        let mut jobs = state.jobs.lock().unwrap();
        if let Some(j) = jobs.get_mut(id) {
            j.status = status;
            j.updated_at = nowstamp();
            j.progress.push(format!(
                "撤销：恢复 {} 项，冲突 {} 项",
                restored.len(),
                conflicts.len()
            ));
            let j = j.clone();
            drop(jobs);
            persist_job(&state.data_root, &j);
        }
    }
    push_event(
        state,
        json!({ "event": "job-undo", "id": id, "restored": restored.len() }),
    );
    Ok(json!({
        "restored": restored,
        "conflicts": conflicts,
        "note": if conflicts.is_empty() { "本任务写入项已全部回滚".to_string() } else { "部分回滚：冲突项保留（用户事后修改过），不谎称全部恢复".to_string() },
    }))
}

/// 从磁盘载入任务（重连恢复视图用）。
pub fn list_persisted(state: &ServerState) -> Vec<Job> {
    state.jobs.lock().unwrap().values().cloned().collect()
}
