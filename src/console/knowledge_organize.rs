//! 手动知识整理：复用持久化任务账本，CLI 断连后继续执行，重启不自动重放。
use super::jobs::{self, Job, JobKind, JobStatus};
use super::{LockExt, Request, Response, ServerState};
use crate::knowledge::organize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

pub(super) fn status(state: &Arc<ServerState>, root: &Path) -> Response {
    match organize::status(&state.data_root, root) {
        Ok(mut status) => {
            status["latest_job"] = state
                .jobs
                .lock_ok()
                .values()
                .filter(|j| {
                    j.kind == JobKind::KnowledgeOrganize
                        && j.result["knowledge_id"] == status["knowledge_id"]
                        && j.result["report_key"] == status["report_key"]
                })
                .max_by_key(|j| {
                    (
                        matches!(j.status, JobStatus::Queued | JobStatus::Running),
                        chrono::DateTime::parse_from_rfc3339(&j.created_at).ok(),
                    )
                })
                .map(|j| json!(j))
                .unwrap_or(Value::Null);
            Response::json(200, status)
        }
        Err(e) => Response::error(400, &e),
    }
}

pub(super) fn start(state: &Arc<ServerState>, root: &Path, req: &Request) -> Response {
    let runner = req.body["runner"].as_str().unwrap_or("codex");
    let focus = req.body["focus"].as_str().unwrap_or("");
    let prepared = match organize::prepare(&state.data_root, root, runner, focus) {
        Ok(prepared) => prepared,
        Err(e) => return Response::error(400, &e),
    };
    let key = req.body["idempotency_key"].as_str().map(str::to_owned);
    let mut ledger = state.jobs.lock_ok();
    if let Some(existing) = ledger.values().find(|job| {
        job.kind == JobKind::KnowledgeOrganize
            && job.result["knowledge_id"] == prepared.knowledge_id()
            && job.result["report_key"] == json!(prepared.report_key())
            && (matches!(job.status, JobStatus::Queued | JobStatus::Running)
                || key
                    .as_ref()
                    .is_some_and(|k| job.idempotency_key.as_ref() == Some(k)))
    }) {
        return Response::json(202, json!({"job_id":existing.id,"reused":true}));
    }
    let id = format!("knowledge-{}", crate::ids::new_id());
    // 登录等快速失败后可在同一秒重试，不能把新任务误判为旧任务。
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let job = Job {
        id: id.clone(),
        kind: JobKind::KnowledgeOrganize,
        status: JobStatus::Queued,
        created_at: now.clone(),
        updated_at: now,
        root: root.into(),
        scope: None,
        fingerprint: None,
        plan_job_id: None,
        idempotency_key: key,
        progress: vec!["等待开始整理知识库".into()],
        result: json!({"knowledge_id":prepared.knowledge_id(),"report_key":prepared.report_key(),"runner":prepared.runner_id(),"source_count":prepared.source_count()}),
        undo: None,
        error: None,
        cancel_requested: false,
    };
    jobs::persist_job(&state.data_root, &job);
    ledger.insert(id.clone(), job);
    drop(ledger);
    jobs::push_event(
        state,
        json!({"event":"job-queued","id":id,"kind":"knowledge_organize"}),
    );
    let worker = Arc::clone(state);
    let worker_id = id.clone();
    std::thread::spawn(move || run(&worker, &worker_id, prepared));
    Response::json(202, json!({"job_id":id}))
}

fn run(state: &Arc<ServerState>, id: &str, prepared: organize::Prepared) {
    jobs::set_status(state, id, JobStatus::Running, "启动本机 AI 工具");
    let result = organize::generate(
        &prepared,
        || jobs::is_cancelled(state, id),
        |note| jobs::set_status(state, id, JobStatus::Running, note),
    );
    let report = match result {
        Ok(report) => report,
        Err(e) => {
            finish(
                state,
                id,
                if jobs::is_cancelled(state, id) {
                    JobStatus::Cancelled
                } else {
                    JobStatus::Failed
                },
                None,
                Some(e.message),
            );
            return;
        }
    };
    // 结果原文先记入任务账本；发布时发生内容冲突也可以查看本次生成的草稿。
    {
        let mut ledger = state.jobs.lock_ok();
        let job = ledger.get_mut(id).unwrap();
        job.result["report"] = json!(report);
        job.result["saved"] = json!(false);
        jobs::persist_job(&state.data_root, job);
    }
    if jobs::is_cancelled(state, id) {
        finish(
            state,
            id,
            JobStatus::Cancelled,
            None,
            Some("整理已取消，本次草稿保留在操作记录中".into()),
        );
        return;
    }
    jobs::set_status(
        state,
        id,
        JobStatus::Running,
        "核对知识库内容并保存整理结果",
    );
    match organize::publish(&state.data_root, &prepared, &report) {
        Ok(path) => finish(
            state,
            id,
            JobStatus::Success,
            Some(json!({"saved":true,"report_path":path})),
            None,
        ),
        Err(e) => finish(state, id, JobStatus::Failed, None, Some(e.message)),
    }
}

fn finish(
    state: &ServerState,
    id: &str,
    status: JobStatus,
    result: Option<Value>,
    error: Option<String>,
) {
    {
        let mut ledger = state.jobs.lock_ok();
        if let Some(job) = ledger.get_mut(id) {
            job.error = error;
            if let Some(result) = result {
                for (key, value) in result.as_object().unwrap() {
                    job.result[key] = value.clone();
                }
            }
        }
    }
    jobs::set_status(
        state,
        id,
        status,
        match status {
            JobStatus::Success => "整理完成，维护知识与建议已保存，后续检索可用",
            JobStatus::Cancelled => "整理已取消",
            _ => "整理失败，原有知识与上次整理结果保留",
        },
    );
}
