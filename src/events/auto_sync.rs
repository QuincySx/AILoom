//! 无感知自动同步：SessionStart / UserPromptSubmit 按 TTL 后台触发 refresh+sync。
//!
//! 默认开启、间隔 1d；可用环境变量覆盖。失败绝不阻塞宿主。

use crate::appctx::AppContext;
use crate::error::Result;
use crate::ids::now_iso;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 默认最小间隔：1 天。
pub const DEFAULT_MIN_INTERVAL_SECS: u64 = 86_400;

const STATE_FILE: &str = "auto_sync.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSyncState {
    pub schema_version: u32,
    /// 本机开关；环境变量 AILOOM_AUTO_SYNC=0 可临时关闭。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 最小间隔（秒）。可用 AILOOM_AUTO_SYNC_INTERVAL=1d|2d|7d 覆盖。
    #[serde(default = "default_interval")]
    pub min_interval_secs: u64,
    #[serde(default)]
    pub last_attempt_at: Option<String>,
    #[serde(default)]
    pub last_success_at: Option<String>,
    /// 有实质变更时留给下一轮 prompt 的轻提示（每条消费一次）。
    #[serde(default)]
    pub pending_notice: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_interval() -> u64 {
    DEFAULT_MIN_INTERVAL_SECS
}

impl Default for AutoSyncState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            enabled: true,
            min_interval_secs: DEFAULT_MIN_INTERVAL_SECS,
            last_attempt_at: None,
            last_success_at: None,
            pending_notice: None,
        }
    }
}

impl AutoSyncState {
    pub fn path(ws_dir: &Path) -> PathBuf {
        ws_dir.join(STATE_FILE)
    }

    pub fn load(ws_dir: &Path) -> AutoSyncState {
        let p = Self::path(ws_dir);
        if !p.is_file() {
            return AutoSyncState::default();
        }
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, ws_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(ws_dir)?;
        crate::sync_common::atomic_write(
            &Self::path(ws_dir),
            serde_json::to_vec_pretty(self)?.as_slice(),
        )
    }
}

/// 解析 `1d` / `2d` / `7d` / `24h` / 纯秒数字。
pub fn parse_interval(raw: &str) -> Option<u64> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    if let Ok(secs) = s.parse::<u64>() {
        return Some(secs);
    }
    let (num, unit) = s.split_at(s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len()));
    let n: u64 = num.parse().ok()?;
    match unit {
        "s" | "sec" | "secs" => Some(n),
        "m" | "min" | "mins" => Some(n.saturating_mul(60)),
        "h" | "hr" | "hour" | "hours" => Some(n.saturating_mul(3600)),
        "d" | "day" | "days" => Some(n.saturating_mul(86_400)),
        _ => None,
    }
}

fn env_disabled() -> bool {
    match std::env::var("AILOOM_AUTO_SYNC") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "0" | "false" | "off" | "no")
        }
        Err(_) => false,
    }
}

fn effective_interval(state: &AutoSyncState) -> u64 {
    if let Ok(v) = std::env::var("AILOOM_AUTO_SYNC_INTERVAL") {
        if let Some(secs) = parse_interval(&v) {
            return secs.max(60); // 下限 60s，避免误配刷爆
        }
    }
    state.min_interval_secs.max(60)
}

fn parse_rfc3339_approx(s: &str) -> Option<SystemTime> {
    // 接受 now_iso() 形态：2026-09-10T12:00:00Z 或带偏移；用 chrono 若已依赖，否则简易解析
    // 项目已有 chrono
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| UNIX_EPOCH + Duration::from_secs(dt.timestamp().max(0) as u64))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|ndt| {
                    let ts = ndt.and_utc().timestamp().max(0) as u64;
                    UNIX_EPOCH + Duration::from_secs(ts)
                })
        })
}

/// 是否已到该触发自动同步。
pub fn is_due(state: &AutoSyncState, now: SystemTime) -> bool {
    if env_disabled() || !state.enabled {
        return false;
    }
    let interval = Duration::from_secs(effective_interval(state));
    let anchor = state
        .last_attempt_at
        .as_deref()
        .and_then(parse_rfc3339_approx)
        .or_else(|| {
            state
                .last_success_at
                .as_deref()
                .and_then(parse_rfc3339_approx)
        });
    match anchor {
        None => true,
        Some(t) => now.duration_since(t).unwrap_or_default() >= interval,
    }
}

/// Hook 侧入口：若到期则抢占 attempt 并后台拉起 `sync --refresh --from-auto`。
/// 返回 JSON 片段字段（是否调度、是否带 pending notice 消费）。
pub fn maybe_schedule(ctx: &AppContext, event: &str) -> serde_json::Value {
    let event = event.to_ascii_lowercase().replace('_', "-");
    let relevant = matches!(
        event.as_str(),
        "sessionstart" | "session-start" | "userpromptsubmit" | "prompt" | "prompt-submit"
    );
    if !relevant {
        return serde_json::json!({ "auto_sync": "skipped_event" });
    }

    let mut state = AutoSyncState::load(&ctx.layout.ws_dir);
    let mut out = serde_json::Map::new();

    // 消费 pending notice（仅 prompt 类事件回注；session-start 也允许）
    if let Some(notice) = state.pending_notice.take() {
        let _ = state.save(&ctx.layout.ws_dir);
        out.insert(
            "additional_context".into(),
            serde_json::Value::String(notice),
        );
    }

    if !is_due(&state, SystemTime::now()) {
        out.insert("auto_sync".into(), serde_json::json!("not_due"));
        return serde_json::Value::Object(out);
    }

    // 抢占：先写 last_attempt，避免并发 prompt 重复 spawn
    state.last_attempt_at = Some(now_iso());
    if state.save(&ctx.layout.ws_dir).is_err() {
        out.insert("auto_sync".into(), serde_json::json!("state_write_failed"));
        return serde_json::Value::Object(out);
    }

    match spawn_auto_sync(ctx) {
        Ok(()) => {
            out.insert("auto_sync".into(), serde_json::json!("scheduled"));
        }
        Err(e) => {
            crate::logging::warn(format!("auto_sync 调度失败（宿主不受影响）: {e}"));
            out.insert("auto_sync".into(), serde_json::json!("spawn_failed"));
        }
    }
    serde_json::Value::Object(out)
}

fn spawn_auto_sync(ctx: &AppContext) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--data-root")
        .arg(&ctx.layout.data_root)
        .arg("sync")
        .arg("--refresh")
        .arg("--from-auto")
        .arg("--root")
        .arg(&ctx.workspace.workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("AILOOM_LOG", "error");

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 新进程组，避免 hook 进程退出带走子进程
        cmd.process_group(0);
    }

    let _child = cmd.spawn()?;
    Ok(())
}

/// sync --from-auto 成功后更新状态；若有写入则留下轻提示。
pub fn record_success(ws_dir: &Path, applied_count: usize, touched_sensitive: bool) {
    let mut state = AutoSyncState::load(ws_dir);
    state.last_success_at = Some(now_iso());
    state.last_attempt_at = state.last_success_at.clone();
    if applied_count > 0 {
        state.pending_notice = Some(if touched_sensitive {
            "ailoom: team resources updated on disk; rules/agents/MCP may need a new session to fully apply.".into()
        } else {
            "ailoom: team resources updated on disk.".into()
        });
    }
    let _ = state.save(ws_dir);
}

/// 单元测试用：给定 anchor 与 now 判断是否 due。
#[cfg(test)]
pub fn is_due_with_anchor(
    interval_secs: u64,
    last_attempt: Option<SystemTime>,
    now: SystemTime,
) -> bool {
    let interval = Duration::from_secs(interval_secs.max(60));
    match last_attempt {
        None => true,
        Some(t) => now.duration_since(t).unwrap_or_default() >= interval,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval_days() {
        assert_eq!(parse_interval("1d"), Some(86_400));
        assert_eq!(parse_interval("2d"), Some(172_800));
        assert_eq!(parse_interval("7d"), Some(604_800));
        assert_eq!(parse_interval("24h"), Some(86_400));
        assert_eq!(parse_interval("3600"), Some(3600));
        assert_eq!(parse_interval("nope"), None);
    }

    #[test]
    fn due_respects_ttl() {
        let t0 = UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(is_due_with_anchor(86_400, None, t0));
        assert!(!is_due_with_anchor(
            86_400,
            Some(t0),
            t0 + Duration::from_secs(3_600)
        ));
        assert!(is_due_with_anchor(
            86_400,
            Some(t0),
            t0 + Duration::from_secs(86_400)
        ));
        assert!(is_due_with_anchor(
            604_800,
            Some(t0),
            t0 + Duration::from_secs(604_800)
        ));
    }
}
