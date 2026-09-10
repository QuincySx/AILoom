//! 会话、Token 与人工干预聚合（AIL-019）。

use crate::error::{code, Error, Result};
use crate::events::schema::{Event, TokenSnapshot};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct TokenUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_creation: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionMetrics {
    pub session_id: String,
    pub workspace_id: String,
    pub tool: String,
    pub prompt_count: u64,
    pub tool_calls: u64,
    /// 工具失败（exit_code 非 0 等）
    pub tool_errors: u64,
    pub stop_count: u64,
    /// 人工打断/拒绝（独立于工具错误）
    pub interventions: u64,
    /// 纠正启发式（时间窗口 + 关键词，标注 heuristic，可配置）
    pub corrections_heuristic: u64,
    pub tokens: TokenUsage,
    pub tokens_availability: String,
    /// 采集覆盖说明
    pub coverage: Coverage,
    pub started_at: Option<String>,
    pub last_event_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Coverage {
    pub prompts: String,
    pub tokens: String,
    pub interventions: String,
}

impl Coverage {
    pub fn full(kind: &str) -> Coverage {
        Coverage {
            prompts: kind.into(),
            tokens: kind.into(),
            interventions: kind.into(),
        }
    }
}

/// 纠正启发式配置（可配置项冻结在 AIL-020 记录；默认窗口 120s）。
#[derive(Debug, Clone)]
pub struct HeuristicConfig {
    pub correction_window_secs: i64,
    pub correction_keywords: Vec<String>,
}

impl Default for HeuristicConfig {
    fn default() -> Self {
        HeuristicConfig {
            correction_window_secs: 120,
            correction_keywords: ["不对", "错了", "not right", "wrong", "revert", "undo"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

fn parse_time(e: &Event) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(&e.time)
        .ok()
        .map(|t| t.timestamp())
}

/// 聚合一个会话的事件序列。
/// 规则（契约 §9）：
/// - Token 累计快照不可相加：每字段取历史最大值（延迟刷新可补齐，重复不翻倍）；
/// - 重复 stop（同 dedup_key/event 重复）通过 event_id 去重已在上游完成，此处按次数统计；
/// - 工具错误与人工干预分开计数；纠正启发式标注 heuristic。
pub fn aggregate_session(
    workspace_id: &str,
    session_id: &str,
    events: &[Event],
    heuristic: &HeuristicConfig,
) -> Result<SessionMetrics> {
    let relevant: Vec<&Event> = events
        .iter()
        .filter(|e| e.session_id == session_id && e.workspace_id == workspace_id)
        .collect();
    if relevant.is_empty() {
        return Err(Error::new(
            code::EVENT_PAYLOAD_INVALID,
            format!("会话 {session_id} 无事件"),
        ));
    }
    let mut m = SessionMetrics {
        session_id: session_id.to_string(),
        workspace_id: workspace_id.to_string(),
        tool: relevant[0].tool.clone(),
        ..Default::default()
    };
    // 累计 token 快照：逐字段最大值
    let mut max_tokens = TokenUsage::default();
    let mut saw_token_event = false;
    let mut last_prompt_time: Option<i64> = None;

    for e in &relevant {
        match e.kind.as_str() {
            "session-start" => {
                if m.started_at.is_none() {
                    m.started_at = Some(e.time.clone());
                }
            }
            "prompt" => {
                m.prompt_count += 1;
                last_prompt_time = parse_time(e);
                // 纠正启发式：prompt 内容哈希不可逆，因此纠正识别需要显式窗口信号：
                // 若 prompt 事件带 dedup_key=correction（宿主端启发式产生），计数
                if e.dedup_key.as_deref() == Some("correction") {
                    m.corrections_heuristic += 1;
                }
            }
            "tool" => {
                m.tool_calls += 1;
                if let Some(code_) = e.exit_code {
                    if code_ != 0 {
                        m.tool_errors += 1;
                    }
                }
            }
            "stop" => {
                m.stop_count += 1;
            }
            _ => {}
        }
        if let Some(t) = &e.tokens {
            saw_token_event = true;
            max_tokens.input = max_tokens.input.max(Some(t.input));
            max_tokens.output = max_tokens.output.max(Some(t.output));
            max_tokens.cache_read = max_tokens.cache_read.max(Some(t.cache_read));
            max_tokens.cache_creation = max_tokens.cache_creation.max(Some(t.cache_creation));
        }
        m.last_event_at = Some(e.time.clone());
    }
    let _ = last_prompt_time;
    let _ = heuristic; // 关键词启发式由宿主端事件标注（dedup_key=correction），窗口在宿主端应用

    m.interventions = count_interventions(&relevant);
    m.tokens = max_tokens;
    m.tokens_availability = if saw_token_event {
        "available".into()
    } else {
        "unavailable".into()
    };
    m.coverage = Coverage {
        prompts: "hook".into(),
        tokens: if saw_token_event {
            "hook".into()
        } else {
            "unavailable".into()
        },
        interventions: "heuristic".into(),
    };
    Ok(m)
}

/// 人工干预：tool 事件带 dedup_key=intervention-<n>（宿主端打断/拒绝启发式）。
fn count_interventions(events: &[&Event]) -> u64 {
    events
        .iter()
        .filter(|e| matches!(e.dedup_key.as_deref(), Some(k) if k.starts_with("intervention-")))
        .count() as u64
}

/// 聚合工作区全部会话。
pub fn aggregate_all(
    workspace_id: &str,
    events: &[Event],
    heuristic: &HeuristicConfig,
) -> Result<BTreeMap<String, SessionMetrics>> {
    let mut out = BTreeMap::new();
    for sid in crate::events::store::session_ids(events) {
        out.insert(
            sid.clone(),
            aggregate_session(workspace_id, &sid, events, heuristic)?,
        );
    }
    Ok(out)
}

/// 解析宿主 transcript（Claude Code JSONL：message.usage.*），产出累计快照事件。
/// 仅在用户显式提供文件时调用（默认不扫描历史）。
pub fn parse_claude_transcript(
    path: &Path,
    workspace_id: &str,
    device_id: &str,
) -> Result<Vec<Event>> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        Error::new(
            code::EVENT_PAYLOAD_INVALID,
            format!("transcript 不可读: {e}"),
        )
    })?;
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(session_id) = v.get("sessionId").and_then(|s| s.as_str()) else {
            continue;
        };
        let Some(usage) = v.pointer("/message/usage") else {
            continue;
        };
        let get = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        out.push(Event {
            schema_version: crate::events::schema::EVENT_SCHEMA_VERSION,
            event_id: crate::ids::new_id(),
            session_id: session_id.to_string(),
            workspace_id: workspace_id.to_string(),
            device_id: device_id.to_string(),
            tool: "claude".into(),
            time: crate::ids::now_iso(),
            kind: "stop".into(),
            tool_name: None,
            exit_code: None,
            duration_ms: None,
            prompt_len: None,
            prompt_hash: None,
            tokens: Some(TokenSnapshot {
                input: get("input_tokens"),
                output: get("output_tokens"),
                cache_read: get("cache_read_input_tokens"),
                cache_creation: get("cache_creation_input_tokens"),
            }),
            dedup_key: Some(format!(
                "token-{}",
                get("input_tokens") + get("output_tokens")
            )),
        });
    }
    Ok(out)
}
