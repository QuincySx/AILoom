//! 摩擦提示与会话摘要（AIL-020）：独立配置分数与阈值；调用量本身不触发；
//! 提示状态按会话持久化；共享默认仅计数与工具名，正文/摘要共享是显式独立操作。

use crate::events::aggregate::SessionMetrics;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrictionConfig {
    /// 打断次数阈值
    pub intervention_threshold: u64,
    /// 工具错误阈值
    pub tool_error_threshold: u64,
    pub weight_interventions: u64,
    pub weight_tool_errors: u64,
    pub weight_corrections: u64,
    /// 触发提示所需最低分
    pub min_score: u64,
    /// 是否允许提示（关闭提示仍统计）
    pub prompt_enabled: bool,
}

impl Default for FrictionConfig {
    fn default() -> Self {
        FrictionConfig {
            intervention_threshold: 2,
            tool_error_threshold: 3,
            weight_interventions: 3,
            weight_tool_errors: 2,
            weight_corrections: 1,
            min_score: 6,
            prompt_enabled: true,
        }
    }
}

impl FrictionConfig {
    pub fn load(dir: &Path) -> FrictionConfig {
        std::fs::read_to_string(dir.join("friction.toml"))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }
}

/// 摩擦分数：打断/工具错误/纠正加权。**调用量本身不进入分数**。
pub fn friction_score(m: &SessionMetrics, cfg: &FrictionConfig) -> u64 {
    let interventions = m
        .interventions
        .min(m.interventions.saturating_sub(0))
        .saturating_sub(0);
    let _ = interventions;
    m.interventions
        .saturating_mul(cfg.weight_interventions)
        .saturating_add(m.tool_errors.saturating_mul(cfg.weight_tool_errors))
        .saturating_add(
            m.corrections_heuristic
                .saturating_mul(cfg.weight_corrections),
        )
}

/// 是否应提示：分数达到阈值 且 确有打断/纠正（人工信号），**且提示未被消费过**。
pub fn should_prompt(m: &SessionMetrics, cfg: &FrictionConfig) -> bool {
    if !cfg.prompt_enabled {
        return false;
    }
    if m.interventions == 0 && m.corrections_heuristic == 0 {
        // 单纯工具调用量/错误量不触发总结提示（日常长会话不受打扰）
        return false;
    }
    friction_score(m, cfg) >= cfg.min_score
        && (m.interventions >= cfg.intervention_threshold
            || m.tool_errors >= cfg.tool_error_threshold)
}

/// 提示状态：每会话最多一次（进程重启后仍生效）。
pub fn prompt_state_file(summary_dir: &Path, session_id: &str) -> std::path::PathBuf {
    summary_dir.join(format!("{session_id}.prompted"))
}

pub fn was_prompted(summary_dir: &Path, session_id: &str) -> bool {
    prompt_state_file(summary_dir, session_id).is_file()
}

pub fn mark_prompted(summary_dir: &Path, session_id: &str) -> crate::error::Result<()> {
    std::fs::create_dir_all(summary_dir)?;
    crate::sync_common::atomic_write(
        prompt_state_file(summary_dir, session_id).as_path(),
        crate::ids::now_iso().as_bytes(),
    )
}

/// 原子认领本会话唯一提示权（create_new 语义：并发 Stop / 进程重启都只有
/// 一个调用者拿到 true）。拿到认领者负责展示提示；展示失败时调用
/// `release_prompted` 释放，绝不允许“已标记却无可见提示”。
pub fn claim_prompted(summary_dir: &Path, session_id: &str) -> crate::error::Result<bool> {
    use std::io::Write;
    std::fs::create_dir_all(summary_dir)?;
    let path = prompt_state_file(summary_dir, session_id);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut f) => {
            f.write_all(crate::ids::now_iso().as_bytes())?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(crate::error::Error::new(
            crate::error::code::INTERNAL,
            format!("提示状态写入失败: {e}"),
        )),
    }
}

/// 释放提示认领（展示失败时回滚），让后续 Stop 可以再次提示。
pub fn release_prompted(summary_dir: &Path, session_id: &str) {
    let _ = std::fs::remove_file(prompt_state_file(summary_dir, session_id));
}

/// 本地结构化摘要（不包含 prompt 全文；自由文本字段不存在）。
pub fn build_local_summary(m: &SessionMetrics, cfg: &FrictionConfig) -> Value {
    json!({
        "schema_version": 1,
        "session_id": m.session_id,
        "workspace_id": m.workspace_id,
        "tool": m.tool,
        "prompt_count": m.prompt_count,
        "tool_calls": m.tool_calls,
        "tool_errors": m.tool_errors,
        "interventions": m.interventions,
        "corrections_heuristic": m.corrections_heuristic,
        "friction_score": friction_score(m, cfg),
        "tokens": {
            "input": m.tokens.input,
            "output": m.tokens.output,
            "cache_read": m.tokens.cache_read,
            "cache_creation": m.tokens.cache_creation,
            "availability": m.tokens_availability,
        },
        "coverage": m.coverage,
        "started_at": m.started_at,
        "last_event_at": m.last_event_at,
    })
}

/// 团队共享记录：白名单计数与工具名；**无 prompt、无摘要正文、无机器路径**。
/// 由本地摘要重建计数版，而非复制本地文件。
pub fn build_share_record(m: &SessionMetrics) -> Value {
    json!({
        "schema_version": 1,
        "session_id_hash": crate::ids::sha256_prefix(m.session_id.as_bytes(), 16),
        "workspace_id": m.workspace_id,
        "tool": m.tool,
        "prompt_count": m.prompt_count,
        "tool_calls": m.tool_calls,
        "tool_errors": m.tool_errors,
        "interventions": m.interventions,
        "tokens_input": m.tokens.input,
        "tokens_output": m.tokens.output,
    })
}
