//! 事件文件读写（AIL-018）：JSONL 追加 + 文件锁 + event_id 去重；并发追加安全。

use crate::error::{code, Error, Result};
use crate::events::schema::Event;
use std::io::Write;
use std::path::Path;

/// 追加事件；event_id 已存在时幂等跳过（返回 false）。
pub fn append_event(events_file: &Path, event: &Event) -> Result<bool> {
    if let Some(parent) = events_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(event)?;
    // 文件锁串行化：读去重 + 追加原子化
    let lock_path = events_file.with_extension("lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| Error::new(code::LOCK_HELD, "事件文件忙").fix("稍后重试；宿主工作不受影响"))?;
    let exists = if events_file.is_file() {
        let text = std::fs::read_to_string(events_file)?;
        text.lines()
            .any(|l| l.contains(&format!("\"event_id\":\"{}\"", event.event_id)))
    } else {
        false
    };
    if exists {
        let _ = fs2::FileExt::unlock(&lock);
        return Ok(false);
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_file)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    f.flush()?;
    let _ = fs2::FileExt::unlock(&lock);
    Ok(true)
}

/// 读取全部事件；坏行跳过并计数（不中断）。
pub fn read_events(events_file: &Path) -> Result<(Vec<Event>, usize)> {
    if !events_file.is_file() {
        return Ok((Vec::new(), 0));
    }
    let text = std::fs::read_to_string(events_file)?;
    let mut events = Vec::new();
    let mut bad = 0usize;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Event>(line) {
            Ok(e) => events.push(e),
            Err(_) => bad += 1,
        }
    }
    Ok((events, bad))
}

/// 按会话累计快照去重：仅保留每个 (session, 字段) 的最大值语义在聚合层处理；
/// 此处提供读取会话集合。
pub fn session_ids(events: &[Event]) -> Vec<String> {
    let mut ids: std::collections::BTreeSet<String> = Default::default();
    for e in events {
        ids.insert(e.session_id.clone());
    }
    ids.into_iter().collect()
}
