//! 图关系辅助召回（AIL-027）：文本相关度召回 + 有限 hop 图邻居重排；
//! 项目过滤先于排名；低相关可拒绝；每条关系可追溯（边带 file/line/confidence）。

use super::graph::Graph;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CodeHit {
    pub symbol_id: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub score: f64,
    /// 关系证据链：[(边, 来源文件:行, 置信度)]
    pub relations: Vec<String>,
}

/// 文本召回候选 + 最多 hops 跳邻居加权扩展。
/// 边权重：contains=0.4、use-dep=0.6、call(ast)=0.8、call(name-based)=0.5、
/// call(name-based-project)=0.7 —— 显式定义，非黑盒。
pub fn query(graph: &Graph, text: &str, hops: usize, limit: usize) -> Vec<CodeHit> {
    let hops = hops.min(2); // 限制图扩展范围
    let terms: Vec<String> = text
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut scores: std::collections::BTreeMap<String, f64> = Default::default();
    // 唯一符号身份索引（AIL-026）：同名定义跨文件不再互相覆盖
    let mut symbol_index: std::collections::BTreeMap<String, (String, String, usize)> =
        Default::default();
    // 名字层索引：调用边目标（fn:name）按名字解析到全部候选定义，
    // 歧义保留（每个候选都获得扩展分与证据）
    let mut name_to_ids: std::collections::BTreeMap<String, Vec<String>> = Default::default();

    for facts in graph.files.values() {
        for sym in &facts.symbols {
            symbol_index.insert(
                sym.id.clone(),
                (sym.kind.clone(), sym.file.clone(), sym.line),
            );
            let plain = format!("{}:{}", sym.kind, sym.name);
            name_to_ids.entry(plain).or_default().push(sym.id.clone());
            let hay = format!(
                "{} {} {}",
                sym.name,
                sym.doc.clone().unwrap_or_default(),
                facts.file
            )
            .to_lowercase();
            for t in &terms {
                if hay.contains(t) {
                    *scores.entry(sym.id.clone()).or_insert(0.0) += 1.0;
                }
            }
        }
    }
    if scores.is_empty() {
        return Vec::new(); // 低相关：拒绝返回噪声
    }
    // 邻居扩展
    let mut relation_evidence: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for hop in 0..hops {
        let frontier: Vec<String> = scores.keys().cloned().collect();
        for from in frontier {
            let base = scores.get(&from).copied().unwrap_or(0.0);
            let decay = 0.6f64.powi(hop as i32);
            for facts in graph.files.values() {
                for e in &facts.edges {
                    if e.from != from {
                        continue;
                    }
                    // 解析目标：唯一身份直接命中；名字层目标解析到全部同名候选
                    let targets: Vec<String> = if symbol_index.contains_key(&e.to) {
                        vec![e.to.clone()]
                    } else {
                        name_to_ids.get(&e.to).cloned().unwrap_or_default()
                    };
                    if targets.is_empty() {
                        continue;
                    }
                    let w = weight_of(&e.kind, &e.confidence) * decay * base.max(1.0);
                    if w <= 0.05 {
                        continue;
                    }
                    for to in targets {
                        *scores.entry(to.clone()).or_insert(0.0) += w;
                        relation_evidence
                            .entry(to.clone())
                            .or_default()
                            .push(format!(
                                "{} -{}({})-> {} [{}:{}]",
                                e.from, e.kind, e.confidence, e.to, e.file, e.line
                            ));
                    }
                }
            }
        }
    }
    let mut hits: Vec<CodeHit> = scores
        .into_iter()
        .filter(|(id, score)| {
            // 仅保留可定位的符号（use 引用等无定义位置的低分项丢弃）
            symbol_index.contains_key(id) && *score >= 0.1
        })
        .map(|(id, score)| {
            let (kind, file, line) = symbol_index.get(&id).cloned().unwrap_or_default();
            CodeHit {
                relations: relation_evidence.get(&id).cloned().unwrap_or_default(),
                score: (score * 100.0).round() / 100.0,
                symbol_id: id,
                kind,
                file,
                line,
            }
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(limit);
    hits
}

fn weight_of(kind: &str, confidence: &str) -> f64 {
    match (kind, confidence) {
        ("contains", _) => 0.4,
        ("use-dep", _) => 0.6,
        ("call", "ast") => 0.8,
        ("call", "name-based-project") => 0.7,
        ("call", _) => 0.5,
        _ => 0.3,
    }
}
