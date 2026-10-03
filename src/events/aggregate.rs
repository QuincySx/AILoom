//! 会话、Token 与人工干预聚合（AIL-019）。

use crate::error::{code, Error, Result};
use crate::events::schema::{Event, TokenSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TokenUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_creation: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub session_id: String,
    pub workspace_id: String,
    pub tool: String,
    /// 完整会话身份的设备维度（RW-06/R02）：workspace+device+tool+session
    /// 四元组唯一确定一条会话；跨设备同名 session 不合并。
    #[serde(default)]
    pub device_id: String,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Coverage {
    pub prompts: String,
    pub tokens: String,
    pub interventions: String,
    /// 纠正启发式来源（hook 关键词识别，标注 heuristic）
    pub corrections: String,
}

impl Coverage {
    pub fn full(kind: &str) -> Coverage {
        Coverage {
            prompts: kind.into(),
            tokens: kind.into(),
            interventions: kind.into(),
            corrections: "heuristic".into(),
        }
    }
}

/// 纠正启发式配置：可经 `<ws_dir>/heuristic.toml` 覆盖（enabled/窗口/关键词，
/// 更改配置确实影响聚合结果）；默认窗口 120s、启用。
#[derive(Debug, Clone)]
pub struct HeuristicConfig {
    /// 总开关：false 时聚合不产生任何纠正计数
    pub enabled: bool,
    pub correction_window_secs: i64,
    pub correction_keywords: Vec<String>,
}

impl Default for HeuristicConfig {
    fn default() -> Self {
        HeuristicConfig {
            enabled: true,
            correction_window_secs: 120,
            correction_keywords: ["不对", "错了", "not right", "wrong", "revert", "undo"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl HeuristicConfig {
    /// 从工作区机器目录加载 heuristic.toml（缺省/坏文件回退默认值）。
    /// 关键词只在 Hook 受控入口（parse_claude_payload）对单条 prompt 文本即时匹配，
    /// 不落盘原文、不扫描历史。
    pub fn load(dir: &Path) -> HeuristicConfig {
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct File {
            enabled: Option<bool>,
            correction_window_secs: Option<i64>,
            correction_keywords: Option<Vec<String>>,
        }
        let f: File = std::fs::read_to_string(dir.join("heuristic.toml"))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        let d = HeuristicConfig::default();
        HeuristicConfig {
            enabled: f.enabled.unwrap_or(d.enabled),
            correction_window_secs: f.correction_window_secs.unwrap_or(d.correction_window_secs),
            correction_keywords: f.correction_keywords.unwrap_or(d.correction_keywords),
        }
    }

    /// 关键词命中（大小写不敏感）。仅用于 Hook 入口对单条 prompt 的受控识别。
    pub fn matches_keyword(&self, text: &str) -> bool {
        Self::matches_keywords(text, &self.correction_keywords)
    }

    /// 无配置实例的关键词匹配入口（Hook payload 解析使用）。
    pub fn matches_keywords(text: &str, keywords: &[String]) -> bool {
        if keywords.is_empty() {
            return false;
        }
        let lower = text.to_lowercase();
        keywords
            .iter()
            .any(|k| !k.is_empty() && lower.contains(&k.to_lowercase()))
    }
}

/// RW-09/R06：累计基线文件（cleanup 前持久化，正常读入口消费）。
/// identities 键 = aggregate_all 的完整身份键 `<tool>/<session>@<device>`；
/// accounted_event_ids = 已计入基线的已清理事件 id（重投不重复计数）。
pub type MetricsBaseline = (BTreeMap<String, SessionMetrics>, BTreeSet<String>);

pub fn metrics_baseline_path(ws_dir: &Path) -> PathBuf {
    ws_dir.join("metrics-baseline.json")
}

/// 读取基线：文件不存在 → None；存在但损坏 → Err（调用方拒绝破坏性操作）。
pub fn load_metrics_baseline(ws_dir: &Path) -> Result<Option<MetricsBaseline>> {
    let path = metrics_baseline_path(ws_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::new(code::INTERNAL, format!("累计基线不可读: {e}")))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| {
        Error::new(code::INDEX_CORRUPT, format!("累计基线损坏: {e}"))
            .fix("删除该文件将丢失已清理会话的累计视图；请先修复或人工确认")
    })?;
    let mut identities = BTreeMap::new();
    if let Some(map) = v.get("identities").and_then(|x| x.as_object()) {
        for (k, m) in map {
            if let Ok(met) = serde_json::from_value::<SessionMetrics>(m.clone()) {
                identities.insert(k.clone(), met);
            }
        }
    }
    let accounted: BTreeSet<String> = v
        .get("accounted_event_ids")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(Some((identities, accounted)))
}

/// 保存基线（原子写）。
pub fn save_metrics_baseline(
    ws_dir: &Path,
    identities: &BTreeMap<String, SessionMetrics>,
    accounted: &BTreeSet<String>,
) -> Result<()> {
    let v = serde_json::json!({
        "schema_version": 1,
        "identities": identities,
        "accounted_event_ids": accounted,
    });
    crate::sync_common::atomic_write(
        metrics_baseline_path(ws_dir).as_path(),
        serde_json::to_vec_pretty(&v)?.as_slice(),
    )
}

/// 逐字段把基线快照累加进实时聚合（计数相加；token 快照取最大值；
/// started_at 取更早、last_event_at 取更晚）。
pub fn add_metrics(dst: &mut SessionMetrics, src: &SessionMetrics) {
    dst.prompt_count += src.prompt_count;
    dst.tool_calls += src.tool_calls;
    dst.tool_errors += src.tool_errors;
    dst.stop_count += src.stop_count;
    dst.interventions += src.interventions;
    dst.corrections_heuristic += src.corrections_heuristic;
    dst.tokens.input = dst.tokens.input.max(src.tokens.input);
    dst.tokens.output = dst.tokens.output.max(src.tokens.output);
    dst.tokens.cache_read = dst.tokens.cache_read.max(src.tokens.cache_read);
    dst.tokens.cache_creation = dst.tokens.cache_creation.max(src.tokens.cache_creation);
    if dst.started_at.is_none()
        || src
            .started_at
            .as_deref()
            .map(|s| dst.started_at.as_deref().map(|d| s < d).unwrap_or(true))
            .unwrap_or(false)
    {
        dst.started_at = src.started_at.clone();
    }
    if src.last_event_at.is_some() {
        dst.last_event_at = src.last_event_at.clone();
    }
}

/// RW-09/R06：正常读入口的有效聚合 = 实时事件（剔除已清算 id，重投不重复计数）
/// + 累计基线（清理前的会话累计）。
pub fn effective_all(
    ws_dir: &Path,
    workspace_id: &str,
    events: &[Event],
    heuristic: &HeuristicConfig,
) -> Result<BTreeMap<String, SessionMetrics>> {
    let baseline = load_metrics_baseline(ws_dir)?;
    let accounted: BTreeSet<String> = baseline
        .as_ref()
        .map(|(_, a)| a.clone())
        .unwrap_or_default();
    let live: Vec<Event> = events
        .iter()
        .filter(|e| !accounted.contains(&e.event_id))
        .cloned()
        .collect();
    let mut all = aggregate_all(workspace_id, &live, heuristic)?;
    if let Some((identities, _)) = baseline {
        for (k, bm) in &identities {
            let entry = all.entry(k.clone()).or_default();
            if entry.session_id.is_empty() {
                *entry = bm.clone();
            } else {
                add_metrics(entry, bm);
            }
        }
    }
    Ok(all)
}

/// 单 session 的有效聚合：实时（剔除已清算）+ 该 session 全部身份的基线累加；
/// 返回 (聚合, 完整身份列表)——身份多于一个时由调用者呈现歧义。
pub fn effective_session(
    ws_dir: &Path,
    workspace_id: &str,
    session_id: &str,
    events: &[Event],
    heuristic: &HeuristicConfig,
) -> Result<(SessionMetrics, Vec<String>)> {
    let baseline = load_metrics_baseline(ws_dir)?;
    let accounted: BTreeSet<String> = baseline
        .as_ref()
        .map(|(_, a)| a.clone())
        .unwrap_or_default();
    let live: Vec<Event> = events
        .iter()
        .filter(|e| !accounted.contains(&e.event_id))
        .cloned()
        .collect();
    let mut m = aggregate_session(workspace_id, session_id, &live, heuristic)?;
    let identities = session_identities(workspace_id, session_id, &live);
    if let Some((identities, _)) = baseline {
        for (k, bm) in &identities {
            let in_baseline = k
                .split('@')
                .next()
                .map(|head| head.ends_with(&format!("/{session_id}")))
                .unwrap_or(false);
            if in_baseline {
                add_metrics(&mut m, bm);
                m.session_id = session_id.to_string();
                m.workspace_id = workspace_id.to_string();
            }
        }
    }
    Ok((m, identities))
}

/// 事件时间解析（RFC3339 → 秒时间戳）；供看板等按最新事件排序的消费者复用。
pub fn parse_time(e: &Event) -> Option<i64> {
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
    aggregate_session_scoped(workspace_id, session_id, None, None, events, heuristic)
}

/// RW-06/R02：列出同名 session 在该工作区下的完整身份（tool/device 组合）。
/// 多于一个即“同名歧义”，单 session 查询应向调用者呈现而非静默合并。
pub fn session_identities(workspace_id: &str, session_id: &str, events: &[Event]) -> Vec<String> {
    let mut ids: BTreeMap<String, ()> = Default::default();
    for e in events
        .iter()
        .filter(|e| e.session_id == session_id && e.workspace_id == workspace_id)
    {
        ids.insert(format!("{}/{}", e.tool, e.device_id), ());
    }
    ids.into_keys().collect()
}

/// 带 provider 过滤的聚合内部入口：`tool` 限定会话事件的宿主工具。
fn aggregate_session_scoped(
    workspace_id: &str,
    session_id: &str,
    tool: Option<&str>,
    device: Option<&str>,
    events: &[Event],
    heuristic: &HeuristicConfig,
) -> Result<SessionMetrics> {
    let relevant: Vec<&Event> = events
        .iter()
        .filter(|e| {
            e.session_id == session_id
                && e.workspace_id == workspace_id
                && tool.map(|t| e.tool == t).unwrap_or(true)
                && device.map(|d| e.device_id == d).unwrap_or(true)
        })
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
        device_id: relevant[0].device_id.clone(),
        ..Default::default()
    };
    // 累计 token 快照：逐字段最大值
    let mut max_tokens = TokenUsage::default();
    let mut saw_token_event = false;
    let mut last_prompt_time: Option<i64> = None;

    let mut last_tool_time: Option<i64> = None;
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
                // 纠正启发式（AIL-019 接通）：识别在 Hook 受控入口完成——关键词命中的
                // prompt 由 parse_claude_payload 标注 dedup_key=correction（文本不落盘）；
                // 聚合在此应用可配置时间窗口：仅统计距最近一次 tool 事件
                // ≤ correction_window_secs 的纠正；enabled=false 完全关闭。
                if heuristic.enabled && e.dedup_key.as_deref() == Some("correction") {
                    if let (Some(et), Some(lt)) = (parse_time(e), last_tool_time) {
                        if et >= lt && et - lt <= heuristic.correction_window_secs {
                            m.corrections_heuristic += 1;
                        }
                    }
                }
            }
            "tool" => {
                m.tool_calls += 1;
                if let Some(code_) = e.exit_code {
                    if code_ != 0 {
                        m.tool_errors += 1;
                    }
                }
                last_tool_time = parse_time(e);
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
        corrections: "heuristic".into(),
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

/// 聚合工作区全部会话。完整会话身份 = workspace+device+tool+session（RW-06/R02
/// 冻结）：键为 `<tool>/<session_id>@<device_id>`——不同 provider **或不同设备**
/// 的同名字符串会话互不合并；workspace 已按事件目录天然隔离。
pub fn aggregate_all(
    workspace_id: &str,
    events: &[Event],
    heuristic: &HeuristicConfig,
) -> Result<BTreeMap<String, SessionMetrics>> {
    let mut out = BTreeMap::new();
    let mut groups: BTreeMap<(String, String, String), ()> = Default::default();
    for e in events.iter().filter(|e| e.workspace_id == workspace_id) {
        groups.insert(
            (e.tool.clone(), e.session_id.clone(), e.device_id.clone()),
            (),
        );
    }
    for (tool, sid, device) in groups.into_keys() {
        let key = format!("{tool}/{sid}@{device}");
        out.insert(
            key.clone(),
            aggregate_session_scoped(
                workspace_id,
                &sid,
                Some(&tool),
                Some(&device),
                events,
                heuristic,
            )?,
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
            // 相同 transcript 行在重导、换文件名或追加导入时仍是同一事件；
            // 用完整规范化记录区分相同 token 数量的不同真实消息。
            event_id: format!(
                "transcript-{}",
                crate::ids::sha256_hex(&serde_json::to_vec(&serde_json::json!([
                    workspace_id,
                    device_id,
                    v
                ]))?)
            ),
            session_id: session_id.to_string(),
            workspace_id: workspace_id.to_string(),
            device_id: device_id.to_string(),
            tool: "claude".into(),
            time: v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .map(str::to_string)
                .unwrap_or_else(crate::ids::now_iso),
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
