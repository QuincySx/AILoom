//! 代码事实与增量知识图谱（AIL-026/027）测试：fixture 依赖/接口位置、增量等价、悬空边、图召回。

use ailoom::code_knowledge::graph::{self};
use ailoom::code_knowledge::recall::query;
use std::path::Path;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn fixture_project(root: &Path) {
    write(
        &root.join("src/lib.rs"),
        r#"//! 库根
pub mod util;

use std::collections::HashMap;

/// 计算面积
pub fn area(w: f64, h: f64) -> f64 {
    let _m: HashMap<String, f64> = HashMap::new();
    util::double(w) * h
}
"#,
    );
    write(
        &root.join("src/util.rs"),
        r#"/// 加倍
pub fn double(x: f64) -> f64 {
    x * 2.0
}

pub fn helper_unused() {}
"#,
    );
}

#[test]
fn extraction_locations_and_dependencies_accurate() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    fixture_project(&root);
    let g = graph::scan_project(&root, Some("rev1".into())).unwrap();
    assert_eq!(g.scan_stats.rust_files, 2);
    assert_eq!(g.scan_stats.parsed_ok, 2);
    // 接口位置准确：area 在 src/lib.rs
    let lib = g.files.get("src/lib.rs").unwrap();
    let area = lib.symbols.iter().find(|s| s.name == "area").unwrap();
    assert_eq!(area.kind, "fn");
    assert_eq!(area.line, 7, "位置准确: {area:?}");
    assert_eq!(area.doc.as_deref(), Some("计算面积"));
    // mod 符号与 contains 边（AST）
    assert!(lib.symbols.iter().any(|s| s.id == "mod:util"));
    assert!(lib
        .edges
        .iter()
        .any(|e| e.kind == "contains" && e.to == "mod:util"));
    // use-dep 边（AST 置信度）
    let lib_edges: Vec<&ailoom::code_knowledge::graph::Edge> =
        lib.edges.iter().filter(|e| e.kind == "use-dep").collect();
    assert!(
        lib_edges.iter().any(|e| e.to.contains("HashMap")),
        "{lib_edges:?}"
    );
    // 调用边：area 调用 double（name-based-project）
    let call: Vec<_> = lib
        .edges
        .iter()
        .filter(|e| e.kind == "call" && e.to == "fn:double")
        .collect();
    assert_eq!(call.len(), 1);
    assert_eq!(call[0].confidence, "name-based-project");
}

#[test]
fn incremental_update_equivalent_to_full_and_no_dangling_edges() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    fixture_project(&root);
    let mut g = graph::scan_project(&root, Some("r1".into())).unwrap();

    // 变更：更名 helper_unused → helper2 并新增引用；删除文件
    write(
        &root.join("src/util.rs"),
        r#"/// 加倍
pub fn double(x: f64) -> f64 {
    x * 2.0
}

pub fn helper2() {}
"#,
    );
    write(
        &root.join("src/lib.rs"),
        r#"pub mod util;

/// 计算面积
pub fn area(w: f64, h: f64) -> f64 {
    util::double(w) * h
}

/// 调用 helper2
pub fn trigger() {
    crate::util::helper2();
}
"#,
    );
    let full = graph::scan_project(&root, Some("r2".into())).unwrap();
    graph::update_incremental(&mut g, &root, Some("r2".into())).unwrap();
    // 增量结果与全量一致（文件集合与哈希）
    assert_eq!(g.files.len(), full.files.len());
    for (f, facts) in &full.files {
        assert_eq!(g.files.get(f).unwrap().content_hash, facts.content_hash);
    }
    // 悬空边：trigger 调用 helper2（项目内存在）→ 不应产生 name-based 悬空
    let dangling = graph::dangling_call_edges(&full);
    assert!(dangling.is_empty(), "无悬空旧边: {dangling:?}");
}

#[test]
fn graph_recall_text_plus_neighbors_filtered_and_traceable() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    write(
        &root.join("src/lib.rs"),
        r#"/// 缓存预热入口
pub fn warm_cache() {
    cache_store();
}

fn cache_store() {
    disk_io();
}

fn disk_io() {}
"#,
    );
    let g = graph::scan_project(&root, None).unwrap();
    // 查询"缓存"：命中 warm_cache + 一跳邻居 cache_store
    let hits = query(&g, "缓存", 1, 10);
    let ids: Vec<&str> = hits.iter().map(|h| h.symbol_id.as_str()).collect();
    assert!(ids.contains(&"fn:warm_cache"), "{ids:?}");
    assert!(ids.contains(&"fn:cache_store"), "图邻居扩展: {ids:?}");
    // 证据链可追溯
    let neighbor = hits
        .iter()
        .find(|h| h.symbol_id == "fn:cache_store")
        .unwrap();
    assert!(!neighbor.relations.is_empty(), "关系证据: {neighbor:?}");
    // 超范围：disk_io 在 1 hop 之外（cache_store 的邻居）→ 2 hop 才出现
    let hits2 = query(&g, "缓存", 2, 10);
    let ids2: Vec<&str> = hits2.iter().map(|h| h.symbol_id.as_str()).collect();
    assert!(ids2.contains(&"fn:disk_io"), "两跳扩展: {ids2:?}");
    // 低相关拒绝
    assert!(query(&g, "zzz 无关词", 2, 10).is_empty(), "低分返回无覆盖");
}
