//! 事件文件读写（AIL-018）：JSONL 追加 + 文件锁 + event_id 去重；并发追加安全。

use crate::error::{code, Error, Result};
use crate::events::schema::Event;
use std::io::Write;
use std::path::{Path, PathBuf};

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
    // AIL-037：有限留存的自动轮转触发——设置 AILOOM_EVENTS_ROTATE_MB 后，
    // 追加使文件超过阈值即在同一锁约定下轮转归档
    if let Ok(mb) = std::env::var("AILOOM_EVENTS_ROTATE_MB") {
        if let Ok(mb) = mb.trim().parse::<f64>() {
            if mb > 0.0 {
                if let Ok(Some(_)) = rotate_events_file(events_file, mb) {
                    // 轮转失败不阻断事件追加
                }
            }
        }
    }
    Ok(true)
}

/// AIL-037：日志轮转——events.jsonl 超过阈值（MB）时改名为唯一归档
/// `events-archive-<ts>-<id>.jsonl`（与追加使用同一把 events.jsonl.lock，
/// 连续/并发轮转不覆盖归档）。未超阈值返回 Ok(None)。
pub fn rotate_events_file(events_file: &Path, max_size_mb: f64) -> Result<Option<PathBuf>> {
    if !events_file.is_file() {
        return Ok(None);
    }
    let size = std::fs::metadata(events_file)?.len() as f64;
    let limit = max_size_mb * 1024.0 * 1024.0;
    if size <= limit {
        return Ok(None);
    }
    let lock_path = events_file.with_extension("lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| Error::new(code::LOCK_HELD, "事件文件忙").fix("稍后重试"))?;
    let result = (|| -> Result<Option<PathBuf>> {
        // 双检：持锁后再看大小，避免与刚完成的轮转竞争
        if !events_file.is_file() {
            return Ok(None);
        }
        let size = std::fs::metadata(events_file)?.len() as f64;
        if size <= limit {
            return Ok(None);
        }
        for attempt in 0..8 {
            let ts = crate::ids::now_iso().replace(':', "");
            let suffix = if attempt == 0 {
                crate::ids::new_id()[..8].to_string()
            } else {
                format!("{}-{}", &crate::ids::new_id()[..8], attempt)
            };
            let archive = events_file.with_file_name(format!("events-archive-{ts}-{suffix}.jsonl"));
            if archive.exists() {
                continue;
            }
            match std::fs::rename(events_file, &archive) {
                Ok(()) => return Ok(Some(archive)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => {
                    return Err(crate::error::Error::new(
                        crate::error::code::INTERNAL,
                        format!("轮转失败: {e}"),
                    ))
                }
            }
        }
        Err(crate::error::Error::new(
            crate::error::code::INTERNAL,
            "轮转归档名冲突重试耗尽",
        ))
    })();
    let _ = fs2::FileExt::unlock(&lock);
    result
}

/// AIL-037：统一活动日志/归档读取——events.jsonl + 全部 events-archive-*.jsonl
/// 按时间序（归档名升序 → 活动文件）合并，event_id 去重（重投旧事件不翻倍），
/// 坏行跳过并计数。
pub fn read_all_events(events_dir: &Path) -> Result<(Vec<Event>, usize)> {
    let mut files: Vec<PathBuf> = Vec::new();
    if events_dir.is_dir() {
        for entry in std::fs::read_dir(events_dir)?.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_events = name == "events.jsonl" || name.starts_with("events-archive-");
            if p.is_file() && is_events {
                files.push(p);
            }
        }
    }
    // 归档名含时间戳前缀，字典序=时间序；活动文件最后读（最新）
    files.sort();
    files.sort_by_key(|p| {
        let n = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if n == "events.jsonl" {
            1
        } else {
            0
        }
    });
    let mut events: Vec<Event> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = Default::default();
    let mut bad = 0usize;
    for f in files {
        let (mut evs, bad_lines) = read_events(&f)?;
        bad += bad_lines;
        for e in evs.drain(..) {
            if seen.insert(e.event_id.clone()) {
                events.push(e);
            }
        }
    }
    Ok((events, bad))
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
