//! Persist returned stage results, rather than trusting one unbounded model turn.
use super::{
    fail, generate_draft, incremental, runtime, DraftOutcome, DraftTask, Prepared, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Batch {
    title: String,
    scope: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Checkpoint {
    schema: String,
    project_id: String,
    knowledge_id: String,
    plan: String,
    batches: Vec<Batch>,
    completed: Vec<String>,
    #[serde(default)]
    pending: Vec<String>,
    partial: String,
    phase: String,
}

impl Checkpoint {
    pub fn preview(&self) -> String {
        let mut text = format!("# 整理阶段稿（未发布）\n\n## 整理计划\n\n{}\n", self.plan);
        for (batch, draft) in self.batches.iter().zip(&self.completed) {
            text.push_str(&format!("\n## 已返回批次：{}\n\n{}\n", batch.title, draft));
        }
        text.push_str("\n## 待完成范围\n\n");
        for batch in self.batches.iter().skip(self.completed.len()) {
            text.push_str(&format!("- {}：{}\n", batch.title, batch.scope));
        }
        text.push_str("\n尚需汇总、核对引用与独立评估；阶段稿不是当前仓库知识。\n");
        for draft in &self.pending {
            text.push_str(&format!("\n## 当前批次已返回的部分成果\n\n{draft}\n"));
        }
        if !self.partial.is_empty() {
            text.push_str(&format!(
                "\n## 当前阶段未完成正文：{}\n\n{}\n",
                self.phase, self.partial
            ));
        }
        text
    }
}

fn parse_plan(plan: &str) -> Vec<Batch> {
    #[derive(Deserialize)]
    struct Plan {
        batches: Vec<Batch>,
    }
    let parsed = plan
        .find('{')
        .zip(plan.rfind('}'))
        .and_then(|(start, end)| plan.get(start..=end))
        .and_then(|text| serde_json::from_str::<Plan>(text).ok());
    if let Some(plan) = parsed {
        if !plan.batches.is_empty()
            && plan
                .batches
                .iter()
                .all(|b| !b.title.trim().is_empty() && !b.scope.trim().is_empty())
        {
            return plan.batches;
        }
    }
    // A formatting variation must not discard the plan or fail the whole organization.
    vec![Batch {
        title: "按已返回计划整理".into(),
        scope: "读取 plan_file 中的实际规划；在本阶段预算内优先处理有证据的模块，明确剩余范围。"
            .into(),
    }]
}

fn reuse_reason(plan: &str) -> Option<String> {
    let (start, end) = plan.find('{').zip(plan.rfind('}'))?;
    let plan: Value = serde_json::from_str(plan.get(start..=end)?).ok()?;
    if plan["reuse_existing"] != true || !plan["batches"].as_array()?.is_empty() {
        return None;
    }
    let reason = plan["reason"].as_str()?.trim();
    (!reason.is_empty()).then(|| reason.to_string())
}

pub(super) fn run(
    prepared: &Prepared,
    resume: Option<Value>,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    retain: impl Fn(&Checkpoint),
) -> Result<DraftOutcome> {
    run_with_elapsed(prepared, resume, cancelled, progress, retain, || {
        prepared.started.elapsed()
    })
}

pub(super) fn run_with_elapsed(
    prepared: &Prepared,
    resume: Option<Value>,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    retain: impl Fn(&Checkpoint),
    elapsed: impl Fn() -> std::time::Duration,
) -> Result<DraftOutcome> {
    let resuming = resume.is_some();
    let mut state = match resume {
        Some(value) => {
            let state: Checkpoint = serde_json::from_value(value)
                .map_err(|_| fail("阶段稿记录无法读取；原有记录保留，可重新整理"))?;
            if state.schema != "ailoom-organization-checkpoint/v1"
                || state.project_id != prepared.input.project_id
                || state.knowledge_id != prepared.input.knowledge_id
                || state.completed.len() > state.batches.len()
            {
                return Err(fail(
                    "阶段稿与本项目身份或整理计划不匹配，未采用其他项目的内容",
                ));
            }
            progress("继续已保存的整理计划；阶段稿作为线索，仍需核对当前资料与代码变化");
            state
        }
        None => Checkpoint {
            schema: "ailoom-organization-checkpoint/v1".into(),
            project_id: prepared.input.project_id.clone(),
            knowledge_id: prepared.input.knowledge_id.clone(),
            plan: String::new(),
            batches: Vec::new(),
            completed: Vec::new(),
            pending: Vec::new(),
            partial: String::new(),
            phase: "制定计划".into(),
        },
    };
    retain(&state);
    if state.plan.is_empty() {
        progress("先盘点资料规模、模块与代码核对入口，制定分批整理计划");
        let plan = stage(prepared, &state, "plan", &cancelled, &progress, &retain)?;
        let requested_reuse = reuse_reason(&plan);
        if !resuming {
            if let Some(reason) = &requested_reuse {
                // Missing/changed evidence means normal compilation, not a formatting failure.
                if incremental::can_reuse(prepared, &cancelled, &progress).unwrap_or(false) {
                    if cancelled() {
                        return Err(fail("整理已取消，已有知识保留"));
                    }
                    state.plan = plan;
                    state.phase = "无需更新，沿用已有评估（本轮未重新评估）".into();
                    state.partial.clear();
                    retain(&state);
                    return Ok(DraftOutcome::Unchanged {
                        reason: reason.clone(),
                    });
                }
                progress("沿用条件不成立，继续按资料与代码核对；保留既有知识");
            }
        }
        state.batches = if requested_reuse.is_some() {
            vec![Batch { title: "重新核对变化与未完成项".into(), scope: "原计划建议沿用，但应用复查条件不成立或当前任务正在续做，不能按无需更新结束。任务清单中的变化是开始时快照；重新盘点当前资料、相关代码及人工页面，检查规划期间变化、额外要求和未完成事项，再按证据维护旧主题。超出本批预算时返回部分成果并标记继续。".into() }]
        } else {
            parse_plan(&plan)
        };
        state.plan = plan;
        state.partial.clear();
        retain(&state);
    }
    while state.completed.len() < state.batches.len() {
        if cancelled() {
            return Err(fail("整理已取消，阶段稿与计划已保留"));
        }
        if elapsed() >= runtime::WRAP_UP {
            return Err(runtime::paused());
        }
        let index = state.completed.len();
        state.phase = format!(
            "批次 {}/{}：{}",
            index + 1,
            state.batches.len(),
            state.batches[index].title
        );
        progress(&format!("{}；完成后保存阶段稿", state.phase));
        let draft = stage(prepared, &state, "batch", &cancelled, &progress, &retain)?;
        let continuation = "<!-- ailoom:continue -->";
        if let Some(body) = draft.trim_end().strip_suffix(continuation) {
            state.pending.push(body.trim_end().to_string());
            state.partial.clear();
            retain(&state);
            progress("本批尚有未完成范围，已保留返回的部分成果；继续本批，不跳到下一模块");
            continue;
        }
        let mut drafts = std::mem::take(&mut state.pending);
        drafts.push(draft);
        state.completed.push(drafts.join("\n\n---\n\n"));
        state.partial.clear();
        retain(&state);
    }
    if cancelled() {
        return Err(fail("整理已取消，阶段稿与计划已保留"));
    }
    if elapsed() >= runtime::MAXIMUM {
        return Err(runtime::paused());
    }
    state.phase = "汇总与核对".into();
    progress("各批阶段稿已保存，汇总跨模块知识并核对来源、冲突与实际覆盖");
    stage(
        prepared,
        &state,
        "synthesize",
        &cancelled,
        &progress,
        &retain,
    )
    .map(DraftOutcome::Draft)
}

fn stage(
    prepared: &Prepared,
    state: &Checkpoint,
    phase: &str,
    cancelled: &impl Fn() -> bool,
    progress: &impl Fn(&str),
    retain: &impl Fn(&Checkpoint),
) -> Result<String> {
    let context = json!({"phase":phase,"checkpoint":state,"next_batch":state.batches.get(state.completed.len())});
    generate_draft(
        prepared,
        cancelled,
        progress,
        |partial| {
            let mut snapshot = state.clone();
            snapshot.partial = partial.into();
            retain(&snapshot);
        },
        prepared.started,
        DraftTask::Stage(phase, &context),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unusual_plan_formats_preserve_the_whole_scope_instead_of_panicking() {
        for text in ["} unexpected text {", "```json\n{\"batches\":[]}\n```", "{\"batches\":[{\"title\":\"甲\",\"scope\":\"资料\"},{\"title\":\"乙\",\"scope\":\"\"}]}"] {
            let batches = parse_plan(text);
            assert_eq!(batches.len(),1);
            assert!(batches[0].scope.contains("plan_file"));
        }
        let batches = parse_plan("计划如下：\n```json\n{\"batches\":[{\"title\":\"甲\",\"scope\":\"甲模块的实际范围\"}]}\n```");
        assert_eq!(batches[0].title, "甲");
        assert_eq!(batches[0].scope, "甲模块的实际范围");
    }
}
