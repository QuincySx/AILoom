//! Independent evidence review gates publication; returned drafts and feedback remain inspectable.
use super::{
    fail, generate_draft, location, runtime, Completion, DraftTask, Error, Prepared, Result,
};
use serde::Serialize;
use serde_json::{json, Value};

pub(super) const PROMPT: &str = include_str!("../res/review-wiki.md");

#[derive(Clone, Debug, Serialize)]
pub struct Assessment {
    pub complete: bool,
    pub verdict: String,
    pub report: String,
    pub candidate: String,
    pub candidate_sha256: String,
    pub input_sha256: String,
    pub project_id: String,
    pub knowledge_id: String,
    pub evaluated_at: String,
}

fn verdict(text: &str) -> &'static str {
    let mut marker = Vec::new();
    let mut conclusions = Vec::new();
    let lines: Vec<_> = text.lines().collect();
    let mut fence = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let kind = &trimmed[..3];
            if fence == Some(kind) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(kind);
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        match trimmed {
            "<!-- ailoom-review: pass -->" => marker.push("pass"),
            "<!-- ailoom-review: revise -->" => marker.push("revise"),
            "<!-- ailoom-review: insufficient -->" => marker.push("insufficient"),
            _ => {}
        }
        let plain = line
            .trim()
            .trim_start_matches(['#', '-', '*', ' '])
            .replace("**", "");
        let tail = if let Some(tail) = plain
            .strip_prefix("结论：")
            .or_else(|| plain.strip_prefix("结论:"))
        {
            Some(tail.trim())
        } else if plain == "结论" {
            lines
                .iter()
                .skip(i + 1)
                .find(|s| !s.trim().is_empty())
                .map(|s| s.trim())
        } else {
            None
        };
        if let Some(tail) = tail {
            let value = if tail.starts_with("评估资料不足") {
                "insufficient"
            } else if tail.starts_with("需修订") {
                "revise"
            } else if tail.starts_with("通过") {
                "pass"
            } else {
                "unconfirmed"
            };
            conclusions.push(value);
        }
    }
    let all = marker.into_iter().chain(conclusions).collect::<Vec<_>>();
    match all.first() {
        Some(value) if all.iter().all(|v| v == value) => value,
        _ => "unconfirmed",
    }
}

impl Assessment {
    fn new(prepared: &Prepared, candidate: &str, report: String) -> Self {
        Self {
            complete: true,
            verdict: verdict(&report).into(),
            report,
            candidate: candidate.into(),
            candidate_sha256: crate::ids::sha256_hex(candidate.as_bytes()),
            input_sha256: prepared.input.fingerprint.clone(),
            project_id: prepared.input.project_id.clone(),
            knowledge_id: prepared.input.knowledge_id.clone(),
            evaluated_at: crate::ids::now_iso(),
        }
    }
    fn proof(&self) -> Value {
        json!({"schema":"ailoom-ai-review/v1","verdict":self.verdict,"candidate_sha256":self.candidate_sha256,"input_sha256":self.input_sha256,"project_id":self.project_id,"knowledge_id":self.knowledge_id,"evaluated_at":self.evaluated_at,"review_sha256":crate::ids::sha256_hex(self.report.as_bytes()),"review_prompt_sha256":crate::ids::sha256_hex(PROMPT.as_bytes()),"organization_prompt_sha256":crate::ids::sha256_hex(super::ORGANIZATION_PROMPT.as_bytes())})
    }
    fn partial(prepared: &Prepared, candidate: &str, report: &str) -> Self {
        let mut value = Self::new(prepared, candidate, report.into());
        value.complete = false;
        value.verdict = "incomplete".into();
        value
    }
}

fn pending(attempts: usize, status: &str, continuation: bool, message: String) -> Completion {
    Completion {
        saved: false,
        report_path: None,
        needs_revision: !continuation,
        needs_continuation: continuation,
        warning: Some(message),
        repair_attempts: attempts,
        assessment_status: status.into(),
    }
}

pub(super) fn complete(
    data: &std::path::Path,
    prepared: &Prepared,
    mut report: String,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    retain: impl Fn(&str, Option<&Error>, bool),
    assessed: impl Fn(&Assessment),
) -> Result<Completion> {
    let mut attempts = 0;
    let mut structural_repairs = 0;
    let mut assessment_status = "not_started".to_string();
    let mut feedback = prepared
        .execution_record
        .as_ref()
        .and_then(|r| r.get("previous_assessment"))
        .cloned();
    if report.trim().is_empty() || report.len() as u64 > super::OUTPUT_LIMIT {
        return Err(fail("候选稿为空或超过知识库存储容量，原有知识保留"));
    }
    retain(&report, None, false);
    loop {
        if cancelled() {
            return Err(fail("整理已取消，本次候选稿、阶段记录与评估记录保留"));
        }
        progress("检查候选稿结构与引用，当前 Wiki 保持不变");
        let issue = match location::validate_organization(data, &prepared.input, &report) {
            Err(error) if error.context["kind"] == "draft_issue" => {
                retain(&report, Some(&error), false);
                if structural_repairs == 2 {
                    return Ok(pending(
                        attempts,
                        &assessment_status,
                        false,
                        format!(
                            "AI 已修订两次，草稿及诊断已保留，仍待修订：{}",
                            error.message
                        ),
                    ));
                }
                structural_repairs += 1;
                error
            }
            Err(error) => return Err(error),
            Ok(()) => {
                if prepared.started.elapsed() >= runtime::MAXIMUM {
                    return Ok(pending(
                        attempts,
                        &assessment_status,
                        true,
                        runtime::paused().message,
                    ));
                }
                progress("独立 AI 评估：先核对原文与跳过原因，再检查候选稿、旧稿演变和相关代码");
                let response = generate_draft(
                    prepared,
                    &cancelled,
                    &progress,
                    |body| assessed(&Assessment::partial(prepared, &report, body)),
                    prepared.started,
                    DraftTask::Review(&report, feedback.as_ref()),
                );
                // An incomplete review never replaces the candidate or becomes a passing verdict.
                let response = match response {
                    Ok(value) => value,
                    Err(error) if !cancelled() => {
                        return Ok(pending(
                            attempts,
                            &assessment_status,
                            error.context["kind"] == "organization_budget",
                            format!(
                                "独立评估未完成，候选稿保留，当前 Wiki 未替换：{}",
                                error.message
                            ),
                        ))
                    }
                    Err(error) => return Err(error),
                };
                let mut assessment = Assessment::new(prepared, &report, response);
                assessed(&assessment);
                if assessment.verdict == "unconfirmed"
                    && prepared.started.elapsed() < runtime::WRAP_UP
                {
                    progress("评估已返回，但结论尚不明确；请评估者核对并说明结论，保留原评估记录");
                    let context = json!({"assessment":assessment.report});
                    match generate_draft(
                        prepared,
                        &cancelled,
                        &progress,
                        |body| assessed(&Assessment::partial(prepared, &report, body)),
                        prepared.started,
                        DraftTask::Review(&report, Some(&context)),
                    ) {
                        Ok(response) => {
                            assessment = Assessment::new(prepared, &report, response);
                            assessed(&assessment);
                        }
                        Err(error) if !cancelled() => {
                            return Ok(pending(
                                attempts,
                                "unconfirmed",
                                error.context["kind"] == "organization_budget",
                                format!("评估结论尚待确认，已有记录保留：{}", error.message),
                            ))
                        }
                        Err(error) => return Err(error),
                    }
                }
                assessment_status = assessment.verdict.clone();
                if assessment.verdict == "pass" {
                    if cancelled() {
                        return Err(fail("整理已取消，候选稿与评估保留"));
                    }
                    progress("独立 AI 评估通过，重新核对资料快照与人工改动后保存");
                    let path = location::publish_organization_with_review(
                        data,
                        &prepared.input,
                        &report,
                        Some((&assessment.report, assessment.proof())),
                    )?;
                    return Ok(Completion {
                        saved: true,
                        report_path: Some(path),
                        needs_revision: false,
                        needs_continuation: false,
                        warning: None,
                        repair_attempts: attempts,
                        assessment_status,
                    });
                }
                if assessment.verdict != "revise" {
                    return Ok(pending(
                        attempts,
                        &assessment_status,
                        false,
                        "评估资料不足或结论待确认，候选稿与评估记录保留，当前 Wiki 未替换".into(),
                    ));
                }
                feedback = Some(
                    json!({"assessment":assessment.report,"candidate_sha256":assessment.candidate_sha256,"input_sha256":assessment.input_sha256,"complete":true,"verdict":assessment.verdict,"reason":"reassess"}),
                );
                let issue=fail("独立评估发现需要修订的内容，按具体证据与缺陷修订，不降低评估标准").context(json!({"kind":"draft_issue","stage":"semantic_review","assessment":assessment.report}));
                retain(&report, Some(&issue), false);
                issue
            }
        };
        if prepared.started.elapsed() >= runtime::WRAP_UP {
            return Ok(pending(
                attempts,
                &assessment_status,
                true,
                format!(
                    "{}；候选稿与评估已保留，继续时按缺陷修订，不从头重生成",
                    runtime::paused().message
                ),
            ));
        }
        attempts += 1;
        progress("按诊断修订候选稿，保留有效知识、人工内容和处理记录；修订后重新独立评估");
        report = match generate_draft(
            prepared,
            &cancelled,
            &progress,
            |draft| retain(draft, None, true),
            prepared.started,
            DraftTask::Repair(&report, &issue),
        ) {
            Ok(value) => value,
            Err(error) if !cancelled() => {
                return Ok(pending(
                    attempts,
                    &assessment_status,
                    error.context["kind"] == "organization_budget",
                    format!("修订未完成，候选稿与已有评估保留：{}", error.message),
                ))
            }
            Err(error) => return Err(error),
        };
        retain(&report, None, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_outcomes_are_tolerant_but_conflicting_or_missing_outcomes_never_pass() {
        assert_eq!(
            verdict("# 开发知识评估\n\n**结论**：通过，证据见下文。"),
            "pass"
        );
        assert_eq!(
            verdict("<!-- ailoom-review: revise -->\n结论：需修订"),
            "revise"
        );
        assert_eq!(verdict("结论：评估资料不足"), "insufficient");
        assert_eq!(
            verdict("<!-- ailoom-review: pass -->\n结论：需修订"),
            "unconfirmed"
        );
        assert_eq!(verdict("只讨论何时通过，并未给出结论。"), "unconfirmed");
        assert_eq!(verdict("结论：不通过"), "unconfirmed");
        assert_eq!(
            verdict("```md\n结论：通过\n<!-- ailoom-review: pass -->\n```"),
            "unconfirmed"
        );
        assert_eq!(
            verdict("这只是标签示例 <!-- ailoom-review: pass -->，没有评估结论。"),
            "unconfirmed"
        );
    }
}
