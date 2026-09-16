//! 代码事实与增量知识图谱（AIL-026）。
//! 首支持语言：Rust（syn AST）。只抽取可证据化关系（span 可溯源）；
//! 解析失败/动态调用记为 gap（显式标记，不编造）；文件哈希增量基线。

use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// v3（RW-10/R07）：符号身份加入模块作用域——`fn:<scope::name>@相对路径`
/// （scope 为所在嵌套 mod 链，根为空）；同文件嵌套模块同名符号不再碰撞。
/// 旧 schema（v2 及以前）身份不含作用域，build 时整体重建（schema 不符 → 全量）。
pub const GRAPH_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Symbol {
    /// 唯一符号身份：`fn:<scope::name>@相对路径`（scope=嵌套 mod 链；AIL-026
    /// 同名定义跨文件不碰撞，RW-10 同文件不同模块也不碰撞）；
    /// 调用边目标：作用域可解析时为完整身份（ast-scoped），否则保持名字层
    /// （fn:name）查询期按名字解析到候选定义并保留歧义证据。
    pub id: String,
    pub kind: String, // fn|struct|enum|trait|mod
    pub name: String, // 简名（不含作用域前缀）
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
    /// 扫描内容指纹（RW-11/R08）：对本次扫描全部 .rs 文件的
    /// `<相对路径>\0<内容 sha256>` 再做摘要。与 Git HEAD/源锁无关，
    /// 未提交的修改/删除/新增都会改变它——查询据此判定工作树过期。
    #[serde(default)]
    pub content_fingerprint: Option<String>,
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
        /// 嵌套 mod 链（RW-10）：进入 `mod x` 压栈、离开弹栈；
        /// 符号身份 = `<链>::<简名>@<文件>`。
        mod_stack: Vec<String>,
        /// 作用域化调用记录：(被调简名, 行, 调用方 id, 调用点 mod 链快照)
        scoped_calls: Vec<(String, usize, String, Vec<String>)>,
    }
    impl<'a> V<'a> {
        fn scope_prefix(&self) -> Option<String> {
            if self.mod_stack.is_empty() {
                None
            } else {
                Some(self.mod_stack.join("::"))
            }
        }
        fn qual(&self, name: &str) -> String {
            match self.scope_prefix() {
                Some(s) => format!("{s}::{name}"),
                None => name.to_string(),
            }
        }
        /// 内层包含者（mod 符号身份）：根模块用 crate/目录名近似。
        fn container_id(&self) -> String {
            match self.scope_prefix() {
                Some(s) => format!("mod:{s}@{}", self.rel),
                None => format!("mod:{}@{}", mod_of(self.rel), self.rel),
            }
        }
    }
    impl<'a> Visit<'_> for V<'a> {
        fn visit_item_fn(&mut self, item: &syn::ItemFn) {
            let name = item.sig.ident.to_string();
            let line = item.sig.ident.span().start().line;
            let id = format!("fn:{}@{}", self.qual(&name), self.rel);
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
                from: self.container_id(),
                to: id.clone(),
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
                self.scoped_calls
                    .push((callee.clone(), line, id.clone(), self.mod_stack.clone()));
                self.edges.push(Edge {
                    from: id.clone(),
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
                id: format!("struct:{}@{}", self.qual(&name), self.rel),
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
                id: format!("enum:{}@{}", self.qual(&name), self.rel),
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
                id: format!("trait:{}@{}", self.qual(&name), self.rel),
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
            let parent = self.container_id();
            let mod_id = format!("mod:{}@{}", self.qual(&name), self.rel);
            self.edges.push(Edge {
                from: parent,
                to: mod_id.clone(),
                kind: "contains".into(),
                confidence: "ast".into(),
                file: self.rel.into(),
                line,
            });
            self.symbols.push(Symbol {
                id: mod_id,
                kind: "mod".into(),
                name: name.clone(),
                file: self.rel.into(),
                line,
                doc: doc_of(&item.attrs),
            });
            self.mod_stack.push(name);
            syn::visit::visit_item_mod(self, item);
            self.mod_stack.pop();
        }
        fn visit_item_use(&mut self, item: &syn::ItemUse) {
            let line = item.use_token.span().start().line;
            let path_str = quote_use(&item.tree);
            self.edges.push(Edge {
                from: self.container_id(),
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
        mod_stack: Vec::new(),
        scoped_calls: Vec::new(),
    };
    v.visit_file(&syntax);
    let scoped_calls = v.scoped_calls;

    // 调用边解析（RW-10）：
    // 1) 作用域链可确定目标（同文件内从调用点最内层 mod 向外查同名 fn）
    //    → to 改写为完整符号身份，confidence=ast-scoped（限定调用可追溯）；
    // 2) 否则保持名字层目标：文件内有同名 fn → name-based-project（跨模块歧义
    //    保留在名字层）；都没有 → name-based 外部引用（gap 语义，不静默选错）。
    for (callee, line, from_id, stack) in &scoped_calls {
        let raw = format!("fn:{callee}");
        let mut resolved: Option<String> = None;
        for i in (0..stack.len()).rev() {
            let candidate = format!("fn:{}::{}@{}", stack[..=i].join("::"), callee, rel);
            if symbols.iter().any(|s| s.id == candidate) {
                resolved = Some(candidate);
                break;
            }
        }
        if resolved.is_none() {
            let root_candidate = format!("fn:{callee}@{rel}");
            if symbols.iter().any(|s| s.id == root_candidate) {
                resolved = Some(root_candidate);
            }
        }
        let edge = edges
            .iter_mut()
            .find(|e| e.kind == "call" && e.from == *from_id && e.line == *line && e.to == raw);
        if let Some(edge) = edge {
            if let Some(target) = resolved {
                edge.to = target;
                edge.confidence = "ast-scoped".into();
            } else if function_names.iter().any(|n| n == callee) {
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
/// 工作树内容指纹（RW-11/R08）：按 scan_project 相同的遍历/过滤规则
/// （跳过 `target`、`.` 开头目录，只看 `.rs`）对每个文件**内容字节**做
/// sha256，再对 `<相对路径>\0<内容哈希>\n`（按路径排序）整体摘要。
/// 只依赖内容本身，不使用 mtime；不要求 Git。
pub fn worktree_fingerprint(project_root: &Path) -> Result<String> {
    let mut stack = vec![project_root.to_path_buf()];
    let mut rust_files: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if name == "target" || name.starts_with('.') {
                    continue;
                }
                stack.push(p);
            } else if name.ends_with(".rs") {
                rust_files.push(p);
            }
        }
    }
    rust_files.sort();
    let mut lines = String::new();
    for f in &rust_files {
        let rel = f.strip_prefix(project_root)?.to_string_lossy().to_string();
        let bytes = std::fs::read(f)?;
        lines.push_str(&format!("{}\0{}\n", rel, crate::ids::sha256_hex(&bytes)));
    }
    Ok(format!(
        "sha256:{}",
        crate::ids::sha256_hex(lines.as_bytes())
    ))
}

pub fn scan_project(project_root: &Path, revision: Option<String>) -> Result<Graph> {
    let fingerprint = worktree_fingerprint(project_root)?;
    let mut graph = Graph {
        schema_version: GRAPH_SCHEMA_VERSION,
        revision,
        content_fingerprint: Some(fingerprint),
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
    graph.content_fingerprint = fresh.content_fingerprint;
    Ok(())
}

/// 悬空边检测：call 边的 to 不存在任何定义 → 标记为 gap 信息（保留边但外部引用语义）。
pub fn dangling_call_edges(graph: &Graph) -> Vec<(String, String)> {
    // 调用边目标为名字层（fn:name）；按名字判断项目内是否有定义
    let defined: std::collections::BTreeSet<String> = graph
        .files
        .values()
        .flat_map(|f| {
            f.symbols
                .iter()
                .filter(|s| s.kind == "fn")
                .map(|s| format!("fn:{}", s.name))
        })
        .collect();
    let mut out = Vec::new();
    for facts in graph.files.values() {
        for e in &facts.edges {
            if e.kind == "call" && e.confidence == "name-based" && !defined.contains(&e.to) {
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
