//! 标准事件协议（AIL-018）：schema_version/event_id/session_id/workspace_id/device_id/
//! tool/time/type；未知字段向前兼容；prompt 默认不落全文（只存哈希与长度）。

use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};

pub const EVENT_SCHEMA_VERSION: u32 = 1;

pub const EVENT_TYPES: [&str; 4] = ["session-start", "prompt", "tool", "stop"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub schema_version: u32,
    pub event_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub device_id: String,
    /// 宿主工具：claude / codex
    pub tool: String,
    /// RFC3339
    pub time: String,
    /// session-start | prompt | tool | stop
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// prompt 隐私保护：只存长度与哈希
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_len: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_hash: Option<String>,
    /// token 事件专用（AIL-019）：累计快照值
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenSnapshot>,
    /// 去重键（可选）：相同键的重复送达在聚合层幂等
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TokenSnapshot {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl Event {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != EVENT_SCHEMA_VERSION {
            return Err(Error::new(
                code::EVENT_PAYLOAD_INVALID,
                format!("不支持的 schema_version: {}", self.schema_version),
            ));
        }
        if !EVENT_TYPES.contains(&self.kind.as_str()) {
            return Err(Error::new(
                code::EVENT_PAYLOAD_INVALID,
                format!("未知事件类型: {}", self.kind),
            )
            .context(serde_json::json!({ "known": EVENT_TYPES })));
        }
        if self.session_id.is_empty() || self.workspace_id.is_empty() {
            return Err(Error::new(
                code::EVENT_PAYLOAD_INVALID,
                "session_id/workspace_id 不能为空",
            ));
        }
        Ok(())
    }
}

/// 未知字段向前兼容：解析到 Value 再挑字段。
pub fn parse_payload(raw: &str) -> Result<Event> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        Error::new(
            code::EVENT_PAYLOAD_INVALID,
            format!("payload 不是合法 JSON: {e}"),
        )
    })?;
    if value.get("schema_version").and_then(|v| v.as_u64()) != Some(EVENT_SCHEMA_VERSION as u64) {
        return Err(Error::new(
            code::EVENT_PAYLOAD_INVALID,
            "payload schema_version 非法",
        ));
    }
    let event: Event = serde_json::from_value(value).map_err(|e| {
        Error::new(
            code::EVENT_PAYLOAD_INVALID,
            format!("payload 字段缺失: {e}"),
        )
    })?;
    event.validate()?;
    Ok(event)
}

/// Claude Code Hook payload（实施时按官方文档核实的字段：session_id/transcript_path/cwd/
/// hook_event_name/tool_name/tool_input 等）。只读取必要字段。
pub fn parse_claude_payload(
    raw: &str,
    workspace_id: &str,
    device_id: &str,
    event_type: &str,
) -> Result<Event> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        Error::new(
            code::EVENT_PAYLOAD_INVALID,
            format!("payload 不是合法 JSON: {e}"),
        )
    })?;
    let session_id = value
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let tool_name = value
        .get("tool_name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let prompt = value.pointer("/tool_input/prompt").and_then(|v| v.as_str());
    let (prompt_len, prompt_hash) = prompt
        .map(|p| {
            (
                Some(p.len()),
                Some(crate::ids::sha256_prefix(p.as_bytes(), 16)),
            )
        })
        .unwrap_or((None, None));
    Ok(Event {
        schema_version: EVENT_SCHEMA_VERSION,
        event_id: crate::ids::new_id(),
        session_id,
        workspace_id: workspace_id.to_string(),
        device_id: device_id.to_string(),
        tool: "claude".into(),
        time: crate::ids::now_iso(),
        kind: event_type.into(),
        tool_name,
        exit_code: None,
        duration_ms: None,
        prompt_len,
        prompt_hash,
        tokens: None,
        dedup_key: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_are_tolerated() {
        let raw = r#"{"schema_version":1,"event_id":"e1","session_id":"s","workspace_id":"w","device_id":"d","tool":"claude","time":"t","type":"stop","brand_new_field":{"x":1}}"#;
        let e = parse_payload(raw).unwrap();
        assert_eq!(e.event_id, "e1");
        assert_eq!(e.kind, "stop");
    }

    #[test]
    fn bad_version_and_type_rejected() {
        let raw = r#"{"schema_version":2,"event_id":"e","session_id":"s","workspace_id":"w","device_id":"d","tool":"t","time":"x","type":"stop"}"#;
        assert!(parse_payload(raw).is_err());
        let raw = r#"{"schema_version":1,"event_id":"e","session_id":"s","workspace_id":"w","device_id":"d","tool":"t","time":"x","type":"bogus"}"#;
        let err = parse_payload(raw).unwrap_err();
        assert_eq!(err.code, "E7001");
    }
}
