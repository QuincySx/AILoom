//! 知识索引与隔离召回（AIL-016）。
//! 索引身份 = schema + 源 revision/digest + 活跃项目/角色；身份变化即重建（原子替换）。
//! 过滤是相关性隔离，不冒充访问控制。

use crate::error::{code, Error, Result};
use crate::resolver::DesiredSet;
use crate::resource::ResourceKind;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const INDEX_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDocument {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub title: String,
    pub scope: String,
    /// 快照内相对路径（供 excerpt 与溯源）
    pub source_path: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub schema_version: u32,
    pub revision: Option<String>,
    pub content_digest: String,
    pub active_projects: Vec<String>,
    pub active_roles: Vec<String>,
    pub identity: String,
    pub built_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeIndex {
    pub meta: IndexMeta,
    pub documents: Vec<IndexDocument>,
    /// term -> {doc 下标, 权重（标题×3 正文×1 累计）}
    pub postings: std::collections::BTreeMap<String, std::collections::BTreeMap<usize, f64>>,
}

/// 分词：ASCII 单词（小写化）+ CJK 二元组（单字成词时保留单字）。
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut word = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphanumeric() {
            word.push(c);
            i += 1;
            continue;
        }
        if is_cjk(c) {
            if !word.is_empty() {
                tokens.push(std::mem::take(&mut word));
            }
            // 与前一个字符构成二元组
            if i > 0 && is_cjk(chars[i - 1]) {
                tokens.push(format!("{}{}", chars[i - 1], c));
            }
            i += 1;
            continue;
        }
        if !word.is_empty() {
            tokens.push(std::mem::take(&mut word));
        }
        i += 1;
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    tokens
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF)
}

/// 从期望资源集合构建索引文档（learnings + rule/doc/skill 均入索引）。
pub fn build_documents(desired: &DesiredSet, _snapshot_root: &Path) -> Result<Vec<IndexDocument>> {
    let mut docs = Vec::new();
    for selected in &desired.selected {
        let entry = &selected.entry;
        let scope = if selected.reason == "shared" {
            "shared".to_string()
        } else {
            selected.reason.clone()
        };
        let title = if entry.description.is_empty() {
            entry.id.name.clone()
        } else {
            entry.description.clone()
        };
        let body = match entry.id.kind {
            ResourceKind::Learning | ResourceKind::Rule | ResourceKind::Doc => {
                strip_fm(entry.raw.as_deref().unwrap_or_default())
            }
            ResourceKind::Skill => {
                // 技能索引 SKILL.md 正文；引用文件列表仅作提示
                strip_fm(entry.raw.as_deref().unwrap_or_default())
            }
            _ => continue, // agent/mcp/env/hook/package 不做全文索引
        };
        docs.push(IndexDocument {
            id: entry.id.to_string(),
            kind: entry.id.kind.as_str().into(),
            name: entry.id.name.clone(),
            title,
            scope,
            source_path: entry.path.clone(),
            body,
        });
    }
    docs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(docs)
}

fn strip_fm(text: &str) -> String {
    crate::resource::split_frontmatter(text)
        .ok()
        .flatten()
        .map(|(_, body)| body)
        .unwrap_or_else(|| text.to_string())
}

/// 构建完整索引对象。
pub fn build(desired: &DesiredSet, snapshot_root: &Path) -> Result<KnowledgeIndex> {
    let documents = build_documents(desired, snapshot_root)?;
    let mut postings: std::collections::BTreeMap<String, std::collections::BTreeMap<usize, f64>> =
        Default::default();
    for (idx, doc) in documents.iter().enumerate() {
        for term in tokenize(&doc.title) {
            *postings.entry(term).or_default().entry(idx).or_insert(0.0) += 3.0;
        }
        for term in tokenize(&doc.body) {
            *postings.entry(term).or_default().entry(idx).or_insert(0.0) += 1.0;
        }
        for term in tokenize(&doc.name) {
            *postings.entry(term).or_default().entry(idx).or_insert(0.0) += 2.0;
        }
    }
    Ok(KnowledgeIndex {
        meta: IndexMeta {
            schema_version: INDEX_SCHEMA_VERSION,
            revision: desired.revision.clone(),
            content_digest: desired.content_digest.clone(),
            active_projects: desired.active_projects.clone(),
            active_roles: desired.active_roles.clone(),
            identity: desired.identity.clone(),
            built_at: crate::ids::now_iso(),
        },
        documents,
        postings,
    })
}

/// 索引身份指纹：决定是否需要重建。
pub fn fingerprint(desired: &DesiredSet) -> String {
    let payload = format!(
        "v{}|{}|{}|{:?}|{:?}",
        INDEX_SCHEMA_VERSION,
        desired.revision.as_deref().unwrap_or(""),
        desired.content_digest,
        desired.active_projects,
        desired.active_roles
    );
    crate::ids::sha256_prefix(payload.as_bytes(), 16)
}

/// 加载索引；身份不符/缺失/损坏 → Ok(None)（调用方重建）。
pub fn load(dir: &Path, desired: &DesiredSet) -> Result<Option<KnowledgeIndex>> {
    let meta_path = dir.join("meta.json");
    if !meta_path.is_file() {
        return Ok(None);
    }
    let want = fingerprint(desired);
    let stored: Result<String> = std::fs::read_to_string(dir.join("fingerprint.txt"))
        .map_err(|e| Error::new(code::INDEX_CORRUPT, format!("索引指纹不可读: {e}")));
    match stored {
        Ok(fp) if fp.trim() == want => {}
        _ => return Ok(None),
    }
    let text = std::fs::read_to_string(dir.join("index.json"))
        .map_err(|e| Error::new(code::INDEX_CORRUPT, format!("索引不可读: {e}")))?;
    let index: KnowledgeIndex = serde_json::from_str(&text)
        .map_err(|_| Error::new(code::INDEX_CORRUPT, "索引损坏").fix("删除索引目录后将自动重建"))?;
    if index.meta.schema_version != INDEX_SCHEMA_VERSION {
        return Ok(None);
    }
    Ok(Some(index))
}

/// 原子保存（临时目录 + rename）。
pub fn save_atomic(index: &KnowledgeIndex, dir: &Path) -> Result<()> {
    let tmp = dir
        .parent()
        .unwrap()
        .join(format!(".index-tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    std::fs::write(tmp.join("index.json"), serde_json::to_vec(index)?)?;
    std::fs::write(tmp.join("fingerprint.txt"), fingerprint_of(index))?;
    std::fs::write(
        tmp.join("meta.json"),
        serde_json::to_vec_pretty(&index.meta)?,
    )?;
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&tmp, dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        Error::new(code::INTERNAL, format!("索引替换失败: {e}"))
    })?;
    Ok(())
}

fn fingerprint_of(index: &KnowledgeIndex) -> String {
    let payload = format!(
        "v{}|{}|{}|{:?}|{:?}",
        INDEX_SCHEMA_VERSION,
        index.meta.revision.as_deref().unwrap_or(""),
        index.meta.content_digest,
        index.meta.active_projects,
        index.meta.active_roles
    );
    crate::ids::sha256_prefix(payload.as_bytes(), 16)
}

/// 索引文件路径集合（清理/诊断用）。
pub fn index_paths(dir: &Path) -> PathBuf {
    dir.join("index.json")
}
