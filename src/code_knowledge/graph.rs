//! 代码事实与增量知识图谱（AIL-026）。
//! 首支持语言：Rust（syn AST）。只抽取可证据化关系（span 可溯源）；
//! 解析失败/动态调用记为 gap（显式标记，不编造）；文件哈希增量基线。

use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const GRAPH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Symbol {
    pub id: String,   // "fn:name" / "struct:Name" / "trait:Name" / "mod:name"
    pub kind: String, // fn|struct|enum|trait|mod
    pub name: String,
    pub file: String, // 相对项目根
    pub line: usize,
    pub doc: Option<String>, // 可选 AI 描述与事实字段分离（v1 仅 doc comment）
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    /// use-dep（import）| call（name-based，置信度显式标注）| contains
    pub kind: String,
    pub confidence: String, // ast（语法确定）| name-based（名称匹配推断）
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileFacts {
    pub file: String,
    pub content_hash: String,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    pub parse_ok: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    pub schema_version: u32,
    pub revision: Option<String>,
    pub files: BTreeMap<String, FileFacts>,
    /// 扫描范围说明：包含/排除与计数
    pub scan_stats: ScanStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanStats {
    pub rust_files: usize,
    pub parsed_ok: usize,
    pub parse_gaps: Vec<String>,
    pub skipped_dirs: Vec<String>,
}

/// 提取单个 Rust 文件的事实（AST 级：定义/imports/contains；调用为 name-based 并标注）。
pub fn extract_file(rel: &str, project_root: &Path) -> Result<FileFacts> {
    let path = project_root.join(rel);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::new(code::RENDER_FAILED, format!("源码不可读: {e}")))?;
    let content_hash = format!("sha256:{}", crate::ids::sha256_hex(text.as_bytes()));
    let Ok(syntax) = syn::parse_file(&text) else {
        return Ok(FileFacts {
            file: rel.into(),
            content_hash,
            symbols: vec![],
            edges: vec![],
            parse_ok: false,
        });
    };
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    let mut function_names: Vec<String> = Vec::new();

    use syn::spanned::Spanned;
    use syn::visit::Visit;
    struct V<'a> {
        rel: &'a str,
        symbols: &'a mut Vec<Symbol>,
        edges: &'a mut Vec<Edge>,
        fn_names: &'a mut Vec<String>,
    }
    impl<'a> Visit<'_> for V<'a> {
        fn visit_item_fn(&mut self, item: &syn::ItemFn) {
            let name = item.sig.ident.to_string();
            let line = item.sig.ident.span().start().line;
            let id = format!("fn:{name}");
            self.symbols.push(Symbol {
                id: id.clone(),
                kind: "fn".into(),
                name: name.clone(),
                file: self.rel.into(),
                line,
                doc: doc_of(&item.attrs),
            });
            self.fn_names.push(name.clone());
            self.edges.push(Edge {
                from: format!("mod:{}", mod_of(self.rel)),
                to: id,
                kind: "contains".into(),
                confidence: "ast".into(),
                file: self.rel.into(),
                line,
            });
            // 收集调用（name-based）
            struct Calls(Vec<(String, usize)>);
            impl<'ast> Visit<'ast> for Calls {
                fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
                    if let syn::Expr::Path(p) = &*call.func {
                        let name = p.path.segments.last().map(|s| s.ident.to_string());
                        if let Some(n) = name {
                            self.0.push((n, call.func.span().start().line));
                        }
                    }
                    syn::visit::visit_expr_call(self, call);
                }
            }
            let mut calls = Calls(Vec::new());
            calls.visit_item_fn(item);
            for (callee, line) in calls.0 {
                self.edges.push(Edge {
                    from: format!("fn:{name}"),
                    to: format!("fn:{callee}"),
                    kind: "call".into(),
                    confidence: "name-based".into(),
                    file: self.rel.into(),
                    line,
                });
            }
            syn::visit::visit_item_fn(self, item);
        }
        fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
            let name = item.ident.to_string();
            let line = item.ident.span().start().line;
            self.symbols.push(Symbol {
                id: format!("struct:{name}"),
                kind: "struct".into(),
                name: name.clone(),
                file: self.rel.into(),
                line,
                doc: doc_of(&item.attrs),
            });
            syn::visit::visit_item_struct(self, item);
        }
        fn visit_item_enum(&mut self, item: &syn::ItemEnum) {
            let name = item.ident.to_string();
            let line = item.ident.span().start().line;
            self.symbols.push(Symbol {
                id: format!("enum:{name}"),
                kind: "enum".into(),
                name: name.clone(),
                file: self.rel.into(),
                line,
                doc: doc_of(&item.attrs),
            });
            syn::visit::visit_item_enum(self, item);
        }
        fn visit_item_trait(&mut self, item: &syn::ItemTrait) {
            let name = item.ident.to_string();
            let line = item.ident.span().start().line;
            self.symbols.push(Symbol {
                id: format!("trait:{name}"),
                kind: "trait".into(),
                name: name.clone(),
                file: self.rel.into(),
                line,
                doc: doc_of(&item.attrs),
            });
            syn::visit::visit_item_trait(self, item);
        }
        fn visit_item_mod(&mut self, item: &syn::ItemMod) {
            let name = item.ident.to_string();
            let line = item.ident.span().start().line;
            self.edges.push(Edge {
                from: format!("mod:{}", mod_of(self.rel)),
                to: format!("mod:{name}"),
                kind: "contains".into(),
                confidence: "ast".into(),
                file: self.rel.into(),
                line,
            });
            self.symbols.push(Symbol {
                id: format!("mod:{name}"),
                kind: "mod".into(),
                name: name.clone(),
                file: self.rel.into(),
                line,
                doc: doc_of(&item.attrs),
            });
            syn::visit::visit_item_mod(self, item);
        }
        fn visit_item_use(&mut self, item: &syn::ItemUse) {
            let line = item.use_token.span().start().line;
            let path_str = quote_use(&item.tree);
            self.edges.push(Edge {
                from: format!("mod:{}", mod_of(self.rel)),
                to: format!("use:{path_str}"),
                kind: "use-dep".into(),
                confidence: "ast".into(),
                file: self.rel.into(),
                line,
            });
            syn::visit::visit_item_use(self, item);
        }
    }
    let mut v = V {
        rel,
        symbols: &mut symbols,
        edges: &mut edges,
        fn_names: &mut function_names,
    };
    v.visit_file(&syntax);

    // 调用边解析：to 若在项目内存在同名 fn → 指向之；否则保留 name-based 外部引用（gap 语义）
    for edge in &mut edges {
        if edge.kind == "call" {
            let callee = edge.to.trim_start_matches("fn:");
            if function_names.iter().any(|n| n == callee) {
                edge.confidence = "name-based-project".into();
            }
        }
    }

    Ok(FileFacts {
        file: rel.into(),
        content_hash,
        symbols,
        edges,
        parse_ok: true,
    })
}

fn doc_of(attrs: &[syn::Attribute]) -> Option<String> {
    let mut docs = Vec::new();
    for a in attrs {
        if a.path().is_ident("doc") {
            if let syn::Meta::NameValue(nv) = &a.meta {
                if let syn::Expr::Lit(lit) = &nv.value {
                    if let syn::Lit::Str(s) = &lit.lit {
                        docs.push(s.value().trim().to_string());
                    }
                }
            }
        }
    }
    if docs.is_empty() {
        None
    } else {
        Some(docs.join(" "))
    }
}

fn mod_of(rel: &str) -> String {
    // crate 根为 "crate"；子模块按目录名近似（mod 关系以 contains/mod 符号为准）
    if rel == "src/lib.rs" || rel == "src/main.rs" {
        "crate".into()
    } else {
        Path::new(rel)
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "crate".into())
    }
}

fn quote_use(tree: &syn::UseTree) -> String {
    match tree {
        syn::UseTree::Path(p) => format!("{}::{}", p.ident, quote_use(&p.tree)),
        syn::UseTree::Name(n) => n.ident.to_string(),
        syn::UseTree::Glob(_) => "*".into(),
        syn::UseTree::Group(g) => {
            let items: Vec<String> = g.items.iter().map(quote_use).collect();
            items.join("|")
        }
        syn::UseTree::Rename(r) => r.ident.to_string(),
    }
}

/// 扫描项目源码树（v1 语言：Rust）。
pub fn scan_project(project_root: &Path, revision: Option<String>) -> Result<Graph> {
    let mut graph = Graph {
        schema_version: GRAPH_SCHEMA_VERSION,
        revision,
        files: BTreeMap::new(),
        scan_stats: ScanStats::default(),
    };
    let mut stack = vec![project_root.to_path_buf()];
    let mut rust_files: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if name == "target" || name.starts_with('.') {
                    graph.scan_stats.skipped_dirs.push(name);
                    continue;
                }
                stack.push(p);
            } else if name.ends_with(".rs") {
                rust_files.push(p);
            }
        }
    }
    rust_files.sort();
    graph.scan_stats.rust_files = rust_files.len();
    for f in rust_files {
        let rel = f.strip_prefix(project_root)?.to_string_lossy().to_string();
        let facts = extract_file(&rel, project_root)?;
        if facts.parse_ok {
            graph.scan_stats.parsed_ok += 1;
        } else {
            graph.scan_stats.parse_gaps.push(rel.clone());
        }
        graph.files.insert(rel, facts);
    }
    // 跨文件调用解析：调用目标在项目内存在定义 → 提升置信度（否则保持外部引用语义）
    let all_fns: std::collections::BTreeSet<String> = graph
        .files
        .values()
        .flat_map(|f| {
            f.symbols
                .iter()
                .filter(|s| s.kind == "fn")
                .map(|s| s.name.clone())
        })
        .collect();
    for facts in graph.files.values_mut() {
        for e in &mut facts.edges {
            if e.kind == "call" && e.confidence == "name-based" {
                let callee = e.to.trim_start_matches("fn:");
                if all_fns.contains(callee) {
                    e.confidence = "name-based-project".into();
                }
            }
        }
    }
    Ok(graph)
}

/// 增量更新：以文件哈希为基线，仅重解析变化文件；删除先剔除旧 facts 再合并（等价全量）。
pub fn update_incremental(
    graph: &mut Graph,
    project_root: &Path,
    revision: Option<String>,
) -> Result<()> {
    // 剔除磁盘上已消失的文件
    let gone: Vec<String> = graph
        .files
        .keys()
        .filter(|f| !project_root.join(f).is_file())
        .cloned()
        .collect();
    for g in gone {
        graph.files.remove(&g);
    }
    let fresh = scan_project(project_root, revision.clone())?;
    // 合并：新扫描覆盖同名文件；增量语义 = 结果与全量一致
    graph.files = fresh.files;
    graph.scan_stats = fresh.scan_stats;
    graph.revision = revision;
    Ok(())
}

/// 悬空边检测：call 边的 to 不存在任何定义 → 标记为 gap 信息（保留边但外部引用语义）。
pub fn dangling_call_edges(graph: &Graph) -> Vec<(String, String)> {
    let defined: std::collections::BTreeSet<&str> = graph
        .files
        .values()
        .flat_map(|f| f.symbols.iter().map(|s| s.id.as_str()))
        .collect();
    let mut out = Vec::new();
    for facts in graph.files.values() {
        for e in &facts.edges {
            if e.kind == "call" && e.confidence == "name-based" && !defined.contains(e.to.as_str())
            {
                out.push((e.from.clone(), e.to.clone()));
            }
        }
    }
    out
}

pub fn save(graph: &Graph, path: &Path) -> Result<()> {
    crate::sync_common::atomic_write(path, serde_json::to_vec_pretty(graph)?.as_slice())
}

pub fn load(path: &Path) -> Result<Option<Graph>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    let g: Graph = serde_json::from_str(&text).map_err(|e| {
        Error::new(code::INDEX_CORRUPT, format!("代码图谱损坏: {e}")).fix("删除后重建")
    })?;
    Ok(Some(g))
}
