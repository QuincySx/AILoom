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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub backup_symlink: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub written_symlink: bool,
    /// 意图已持久化后，目标写入是否完成。
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
        self.writer.get_ref().sync_all()?;
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
    let bytes = std::fs::read(run_dir.join("journal.jsonl"))?;
    let mut entries = std::collections::BTreeMap::new();
    // 最后一个未换行的记录可能是在写 journal 时被杀死；写入意图尚未
    // flush+sync 就不会改目标，完成记录前也已有完整意图可供恢复。
    for line in bytes
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| line.last() == Some(&b'\n') && !line.iter().all(u8::is_ascii_whitespace))
    {
        let e: JournalEntry = serde_json::from_slice(line).map_err(|e| {
            Error::new(code::JOURNAL_RESTORE_FAILED, format!("journal 行损坏: {e}"))
        })?;
        entries.insert(e.seq, e);
    }
    Ok(entries.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_completion_retains_durable_intent_and_bad_complete_line_fails() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let mut run = JournalRun::start(tmp.path(), "test").unwrap();
        let entry = JournalEntry {
            seq: 0,
            item_key: "r.md".into(),
            path: "r.md".into(),
            action: "create".into(),
            resource_id: "r".into(),
            written_hash: "sha256:new".into(),
            backup_file: None,
            backup_hash: None,
            backup_symlink: false,
            written_symlink: false,
            done: false,
        };
        run.append(&entry).unwrap();
        let path = run.dir.join("journal.jsonl");
        let dir = run.dir.clone();
        drop(run);
        let durable = std::fs::read(&path).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"seq\":0,\"done\":")
            .unwrap();
        let entries = read_entries(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].done);
        std::fs::write(
            &path,
            [durable.as_slice(), b"{\"path\":\"\xe4\xb8"].concat(),
        )
        .unwrap();
        let entries = read_entries(&dir).unwrap();
        assert_eq!(entries.len(), 1, "UTF-8 中途截断也保留完整意图");
        assert!(!entries[0].done);
        std::fs::write(&path, [durable.as_slice(), b"{invalid}\n"].concat()).unwrap();
        assert_eq!(
            read_entries(&dir).unwrap_err().code,
            code::JOURNAL_RESTORE_FAILED
        );
    }
}
