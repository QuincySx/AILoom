//! One budget spans discovery, batches, synthesis and corrective passes.
use super::{fail, Error};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) const BATCH_MINUTES: u64 = 20;
pub(super) const WRAP_UP: Duration = Duration::from_secs(90 * 60);
pub(super) const MAXIMUM: Duration = Duration::from_secs(120 * 60);

pub(super) fn policy() -> Value {
    json!({"batch_minutes":BATCH_MINUTES,"wrap_up_minutes":WRAP_UP.as_secs()/60,"maximum_minutes":MAXIMUM.as_secs()/60})
}

pub(super) fn control(started: Instant, stage_started: Instant, target: Duration) -> Value {
    let elapsed = started.elapsed();
    let stage_elapsed = stage_started.elapsed();
    json!({
        "policy":policy(),
        "elapsed_seconds":elapsed.as_secs(),
        "remaining_seconds":MAXIMUM.saturating_sub(elapsed).as_secs(),
        "stage_elapsed_seconds":stage_elapsed.as_secs(),
        "stage_target_seconds":target.as_secs(),
        "action":if elapsed >= WRAP_UP || stage_elapsed >= target {"finish_current_stage"} else {"continue"},
        "instruction":"每组读取前检查本文件；进入收尾时停止扩大阅读范围，返回本阶段已有证据、未完成范围和下一步。不能以赶进度为由伪造覆盖或删掉有效知识。"
    })
}

pub(super) fn write_control(
    path: &Path,
    started: Instant,
    stage_started: Instant,
    target: Duration,
) -> super::Result<()> {
    // Atomic replacement: the agent must never read partially written JSON.
    crate::sync_common::atomic_write(
        path,
        &serde_json::to_vec_pretty(&control(started, stage_started, target))?,
    )
}

pub(super) fn paused() -> Error {
    let mut error =
        fail("本轮预算已到，整理计划、阶段稿与未完成范围已保留；可继续整理，当前知识未替换");
    error.context = json!({"kind":"organization_budget"});
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_target_requests_a_checkpoint_before_the_round_budget() {
        let now = Instant::now();
        let target = Duration::from_secs(BATCH_MINUTES * 60);
        let before = control(now - Duration::from_secs(40 * 60), now, target);
        assert_eq!(before["action"], "continue");
        let checkpoint = control(now - Duration::from_secs(40 * 60), now - target, target);
        assert_eq!(checkpoint["action"], "finish_current_stage");
        assert!(checkpoint["remaining_seconds"].as_u64().unwrap() > 60 * 60);
        let round = control(now - WRAP_UP, now, target);
        assert_eq!(round["action"], "finish_current_stage");
    }
}
