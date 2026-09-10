//! 恢复 journal（AIL-008）：apply 每步记录旧值备份与写入结果；失败可检查恢复。

use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub seq: usize,
    pub item_key: String,
    pub path: String,
    pub action: String,
    pub resource_id: String,
    /// 本次写入后的内容哈希（恢复时校验当前内容是否仍是本次写入）
    pub written_hash: String,
    /// 旧内容备份文件名（位于 journal 运行目录），create 无备份
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_file: Option<String>,
    /// 旧内容哈希；create 为 None（恢复=删除）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_hash: Option<String>,
    /// 目标为空文件且无实际内容的标记
    pub done: bool,
}

pub struct JournalRun {
    pub dir: PathBuf,
    writer: std::io::BufWriter<std::fs::File>,
    next_seq: usize,
}

impl JournalRun {
    /// 新建一次 apply 的 journal 目录。
    pub fn start(journal_root: &Path, run_id: &str) -> Result<JournalRun> {
        let dir = journal_root.join(run_id);
        std::fs::create_dir_all(&dir)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("journal.jsonl"))?;
        Ok(JournalRun {
            dir,
            writer: std::io::BufWriter::new(file),
            next_seq: 0,
        })
    }

    pub fn next_seq(&mut self) -> usize {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.dir.join("backup")
    }

    pub fn append(&mut self, entry: &JournalEntry) -> Result<()> {
        use std::io::Write;
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    /// 标记成功完成并移除整个运行目录（无残留恢复点）。
    pub fn complete(self) -> Result<()> {
        drop(self.writer);
        crate::sync_common::remove_dir_all_guarded(&self.dir)?;
        Ok(())
    }

    /// 发现未完成的 journal（崩溃/失败遗留），按 run_id 排序返回。
    pub fn find_pending(journal_root: &Path) -> Result<Vec<PathBuf>> {
        let mut runs = Vec::new();
        if !journal_root.is_dir() {
            return Ok(runs);
        }
        for entry in std::fs::read_dir(journal_root)?.flatten() {
            let dir = entry.path();
            if dir.is_dir() && dir.join("journal.jsonl").is_file() {
                runs.push(dir);
            }
        }
        runs.sort();
        Ok(runs)
    }
}

/// 读取一次运行的全部条目（按 seq）。
pub fn read_entries(run_dir: &Path) -> Result<Vec<JournalEntry>> {
    let text = std::fs::read_to_string(run_dir.join("journal.jsonl"))?;
    let mut entries = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let e: JournalEntry = serde_json::from_str(line).map_err(|e| {
            Error::new(code::JOURNAL_RESTORE_FAILED, format!("journal 行损坏: {e}"))
        })?;
        entries.push(e);
    }
    Ok(entries)
}
