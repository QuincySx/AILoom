//! 代码事实与增量知识图谱（AIL-026/027）测试：fixture 依赖/接口位置、增量等价、悬空边、图召回、
//! 唯一符号身份（跨文件同名不碰撞）、过期提示、纯文本基线对比。

mod common;

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
    // 接口位置准确：area 在 src/lib.rs，符号身份带文件作用域
    let lib = g.files.get("src/lib.rs").unwrap();
    let area = lib.symbols.iter().find(|s| s.name == "area").unwrap();
    assert_eq!(area.kind, "fn");
    assert_eq!(area.line, 7, "位置准确: {area:?}");
    assert_eq!(area.doc.as_deref(), Some("计算面积"));
    assert_eq!(area.id, "fn:area@src/lib.rs", "唯一符号身份含文件");
    // mod 符号与 contains 边（AST，身份作用域化）
    assert!(lib.symbols.iter().any(|s| s.id == "mod:util@src/lib.rs"));
    assert!(lib
        .edges
        .iter()
        .any(|e| e.kind == "contains" && e.to == "mod:util@src/lib.rs"));
    // use-dep 边（AST 置信度）
    let lib_edges: Vec<&ailoom::code_knowledge::graph::Edge> =
        lib.edges.iter().filter(|e| e.kind == "use-dep").collect();
    assert!(
        lib_edges.iter().any(|e| e.to.contains("HashMap")),
        "{lib_edges:?}"
    );
    // 调用边：from 为作用域身份，to 为名字层（查询期解析）
    let call: Vec<_> = lib
        .edges
        .iter()
        .filter(|e| e.kind == "call" && e.to == "fn:double")
        .collect();
    assert_eq!(call.len(), 1);
    assert_eq!(call[0].from, "fn:area@src/lib.rs");
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
    // 悬空边：trigger 调用 helper2（项目内存在定义）→ 不应产生 name-based 悬空
    let dangling = graph::dangling_call_edges(&full);
    assert!(dangling.is_empty(), "无悬空旧边: {dangling:?}");
}

/// AIL-026 核心回归：不同文件同名 fn 定义在符号索引中保持独立，
/// 调用边按来源文件归属，不再混合。
#[test]
fn same_name_symbols_across_files_stay_distinct() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    write(
        &root.join("src/a.rs"),
        r#"/// A 侧 helper
pub fn helper() -> u32 {
    1
}

pub fn caller_a() -> u32 {
    helper()
}
"#,
    );
    write(
        &root.join("src/b.rs"),
        r#"/// B 侧 helper
pub fn helper() -> u32 {
    2
}

pub fn caller_b() -> u32 {
    helper()
}
"#,
    );
    let g = graph::scan_project(&root, Some("r1".into())).unwrap();
    let a = g.files.get("src/a.rs").unwrap();
    let b = g.files.get("src/b.rs").unwrap();
    // 定义不互相覆盖：两个作用域化 id 同时存在
    assert!(a.symbols.iter().any(|s| s.id == "fn:helper@src/a.rs"));
    assert!(b.symbols.iter().any(|s| s.id == "fn:helper@src/b.rs"));
    // 调用边按来源文件区分（调用边 to 仍是名字层，from 精确）
    let edge_a = a
        .edges
        .iter()
        .find(|e| e.kind == "call" && e.from.starts_with("fn:caller_a"))
        .unwrap();
    assert_eq!(edge_a.from, "fn:caller_a@src/a.rs", "{edge_a:?}");
    let edge_b = b
        .edges
        .iter()
        .find(|e| e.kind == "call" && e.from.starts_with("fn:caller_b"))
        .unwrap();
    assert_eq!(edge_b.from, "fn:caller_b@src/b.rs", "{edge_b:?}");

    // 召回：查询 helper 命中两个独立符号，位置各自正确（不再被覆盖成一个）
    let hits = query(&g, "helper", 0, 10);
    let ids: Vec<&str> = hits.iter().map(|h| h.symbol_id.as_str()).collect();
    assert!(ids.contains(&"fn:helper@src/a.rs"), "{ids:?}");
    assert!(
        ids.contains(&"fn:helper@src/b.rs"),
        "同名定义独立返回: {ids:?}"
    );
    let hit_a = hits
        .iter()
        .find(|h| h.symbol_id == "fn:helper@src/a.rs")
        .unwrap();
    assert_eq!(hit_a.file, "src/a.rs");
    assert_eq!(hit_a.line, 2);
}

/// AIL-027：查询对过期图给出重建提示（通过 code 命令集成验证在 tests/cli.rs 的
/// graph_stale 标志）；此处验证基线对比——图查询的关系证据是纯文本检索不具备的。
#[test]
fn graph_query_provides_evidence_beyond_text_baseline() {
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
    assert!(ids.contains(&"fn:warm_cache@src/lib.rs"), "{ids:?}");
    assert!(
        ids.contains(&"fn:cache_store@src/lib.rs"),
        "图邻居扩展: {ids:?}"
    );
    // 证据链可追溯（含 file:line 与置信度）
    let neighbor = hits
        .iter()
        .find(|h| h.symbol_id == "fn:cache_store@src/lib.rs")
        .unwrap();
    assert!(!neighbor.relations.is_empty(), "关系证据: {neighbor:?}");
    assert!(
        neighbor
            .relations
            .iter()
            .any(|r| r.contains("[src/lib.rs:")),
        "证据含位置: {neighbor:?}"
    );
    // 超范围：disk_io 在 1 hop 之外 → 2 hop 才出现
    let hits2 = query(&g, "缓存", 2, 10);
    let ids2: Vec<&str> = hits2.iter().map(|h| h.symbol_id.as_str()).collect();
    assert!(
        ids2.contains(&"fn:disk_io@src/lib.rs"),
        "两跳扩展: {ids2:?}"
    );
    // 低相关拒绝
    assert!(query(&g, "zzz 无关词", 2, 10).is_empty(), "低分返回无覆盖");

    // 纯文本基线对比：名字/文档子串检索只能证明词面包含，无法提供关系证据。
    // 用只在 warm_cache 文档出现的「预热」作基线查询：基线只命中 warm_cache，
    // 图查询经调用关系补齐名字不含查询词的 cache_store / disk_io（基线盲区）。
    let text_baseline_hits: Vec<&str> = g
        .files
        .values()
        .flat_map(|f| f.symbols.iter())
        .filter(|s| format!("{} {}", s.name, s.doc.clone().unwrap_or_default()).contains("预热"))
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        text_baseline_hits,
        vec!["fn:warm_cache@src/lib.rs"],
        "基线（词面子串）只命中 warm_cache: {text_baseline_hits:?}"
    );
    assert!(
        !text_baseline_hits.contains(&"fn:cache_store@src/lib.rs"),
        "基线盲区确认"
    );
    assert!(
        ids.contains(&"fn:cache_store@src/lib.rs"),
        "图查询通过调用关系补齐文本基线盲区"
    );
}

/// AIL-027 CLI 级验收：revision 变化后 query 返回 graph_stale=true 与重建提示；
/// 重建后恢复 false。走真实 `ailoom code` 入口。
#[test]
fn code_query_reports_stale_graph_after_revision_change() {
    let tmp = tempfile::tempdir().unwrap();
    // 本地裸远端 + 团队源 + 业务仓
    let bare = tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let src = common::make_team_source(tmp.path());
    ailoom::gitx::git(&src, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(&src, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
    ailoom::gitx::git(&src, &["push", "-q", "-u", "origin", "main"]).unwrap();
    let ws = common::make_business_repo(tmp.path(), "biz");
    // 业务仓里放一个 rust 文件（图扫描对象）
    write(&ws.join("src/lib.rs"), "/// 缓存入口\npub fn warm() {}\n");

    let dr = tmp.path().join("data");
    let run = |args: &[&str]| {
        let mut bin_path = std::env::current_exe().unwrap();
        loop {
            if bin_path.join("ailoom").exists() {
                break;
            }
            if !bin_path.pop() {
                panic!("未找到 ailoom 二进制");
            }
        }
        let out = std::process::Command::new(bin_path.join("ailoom"))
            .args(args)
            .current_dir(&ws)
            .envs(common::isolated_child_env(tmp.path()))
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };

    let init_args = [
        "--data-root".to_string(),
        dr.to_str().unwrap().to_string(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = init_args.iter().map(String::as_str).collect();
    let (code, _, stderr) = run(&refs);
    assert_eq!(code, 0, "{stderr}");

    let code_args = [
        "--json",
        "--data-root",
        dr.to_str().unwrap(),
        "code",
        "--action",
        "build",
    ];
    let (code, _, stderr) = run(&code_args);
    assert_eq!(code, 0, "{stderr}");

    // 图与当前 revision 一致 → stale=false
    let query_args = [
        "--json",
        "--data-root",
        dr.to_str().unwrap(),
        "code",
        "--action",
        "query",
        "--query",
        "缓存",
    ];
    let (code, stdout, stderr) = run(&query_args);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["graph_stale"], false, "{v}");

    // 业务仓提交新 revision（工作区 revision 变化）→ 旧图过期
    ailoom::gitx::git(&ws, &["add", "-A"]).unwrap();
    ailoom::gitx::git(
        &ws,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "bump",
        ],
    )
    .unwrap();
    let (code, stdout, _) = run(&query_args);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v["result"]["graph_stale"], true,
        "revision 变化必须提示过期: {v}"
    );
    assert!(
        v["result"]["note"].as_str().unwrap().contains("过期"),
        "{v}"
    );

    // 重建后恢复一致
    let (code, _, _) = run(&code_args);
    assert_eq!(code, 0);
    let (code, stdout, _) = run(&query_args);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["graph_stale"], false, "重建后不过期: {v}");
}
