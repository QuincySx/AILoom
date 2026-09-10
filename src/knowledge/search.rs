//! 关键词检索（AIL-016）：排序结果含来源、得分、scope 与原文摘录。

use super::index::{tokenize, KnowledgeIndex};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub scope: String,
    pub score: f64,
    pub source_path: String,
    pub excerpt: String,
}

/// 查询：多词 OR 计分，按分数降序、同分按 id 稳定排序。
pub fn search(index: &KnowledgeIndex, query: &str, limit: usize) -> Vec<SearchHit> {
    let terms = tokenize(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut scores: std::collections::BTreeMap<usize, f64> = Default::default();
    for term in &terms {
        if let Some(docs) = index.postings.get(term) {
            for (idx, weight) in docs {
                *scores.entry(*idx).or_insert(0.0) += *weight;
            }
        }
    }
    let mut hits: Vec<(usize, f64)> = scores.into_iter().collect();
    hits.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    hits.truncate(limit);
    hits.into_iter()
        .map(|(idx, score)| {
            let doc = &index.documents[idx];
            SearchHit {
                id: doc.id.clone(),
                kind: doc.kind.clone(),
                title: doc.title.clone(),
                scope: doc.scope.clone(),
                score: (score * 100.0).round() / 100.0,
                source_path: doc.source_path.clone(),
                excerpt: excerpt(&doc.body, &terms),
            }
        })
        .collect()
}

/// 原文摘录：首个命中词附近 ±40 字符（按字符边界，中文安全）；未命中取正文开头。
fn excerpt(body: &str, terms: &[String]) -> String {
    let lower = body.to_lowercase();
    let mut best: Option<usize> = None;
    for term in terms {
        if let Some(pos) = lower.find(term.as_str()) {
            best = Some(pos);
            break;
        }
    }
    let total = body.chars().count();
    match best {
        Some(pos) => {
            let char_pos = body[..pos].chars().count();
            let start_char = char_pos.saturating_sub(20);
            let end_char = (char_pos + 40).min(total);
            let raw: String = body
                .chars()
                .skip(start_char)
                .take(end_char - start_char)
                .collect();
            format!("…{}…", raw.trim())
        }
        None => {
            let raw: String = body.chars().take(60).collect();
            format!("…{}…", raw.trim())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_bigrams_and_ascii_words() {
        let tokens = tokenize("缓存击穿 cache miss!");
        assert!(tokens.contains(&"缓存".to_string()));
        assert!(tokens.contains(&"存击".to_string()));
        assert!(tokens.contains(&"cache".to_string()));
        assert!(tokens.contains(&"miss".to_string()));
    }
}
