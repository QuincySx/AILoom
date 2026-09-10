//! 知识索引与隔离召回（AIL-016）集成测试。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

struct Ctx {
    tmp: tempfile::TempDir,
}

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    loop {
        path.pop();
        let p = if cfg!(windows) {
            "ailoom.exe"
        } else {
            "ailoom"
        };
        if path.join(p).exists() {
            return path.join(p);
        }
    }
}

impl Ctx {
    fn new() -> Ctx {
        Ctx {
            tmp: tempfile::tempdir().unwrap(),
        }
    }
    fn run(&self, cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .env("HOME", self.tmp.path().join("home"))
            .env("AILOOM_LOG", "error")
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn dr(&self) -> String {
        self.tmp.path().join("data").to_string_lossy().to_string()
    }
    fn setup(&self) -> (PathBuf, PathBuf, PathBuf) {
        let bare = self.tmp.path().join("origin.git");
        ailoom::gitx::git_init(&bare, true).unwrap();
        let team_src = common::make_team_source(self.tmp.path());
        ailoom::gitx::git(&team_src, &["branch", "-M", "main"]).unwrap();
        ailoom::gitx::git(
            &team_src,
            &["remote", "add", "origin", bare.to_str().unwrap()],
        )
        .unwrap();
        ailoom::gitx::git(&team_src, &["push", "-q", "-u", "origin", "main"]).unwrap();
        let ws = common::make_business_repo(self.tmp.path(), "biz");
        (bare, team_src, ws)
    }
    fn init(&self, ws: &Path, url: &str, projects: &[&str], refresh: bool) {
        let dr = self.dr();
        let mut args: Vec<String> = vec![
            "--data-root".into(),
            dr.clone(),
            "init".into(),
            "--url".into(),
            url.to_string(),
        ];
        if refresh {
            args.push("--refresh".into());
        }
        for p in projects {
            args.push("--project".into());
            args.push(p.to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, _, stderr) = self.run(ws, &refs);
        assert_eq!(code, 0, "{stderr}");
    }
    fn recall(&self, ws: &Path, query: &str) -> serde_json::Value {
        let dr = self.dr();
        let args = [
            "--json",
            "--data-root",
            dr.as_str(),
            "recall",
            "--query",
            query,
            "--limit",
            "10",
        ];
        let (code, stdout, stderr) = self.run(ws, &args);
        assert_eq!(code, 0, "{stderr}");
        serde_json::from_str(stdout.trim()).unwrap()
    }
}

#[test]
fn isolated_recall_between_projects_with_same_keywords() {
    let c = Ctx::new();
    let (bare, _src, ws) = c.setup();
    let url = bare.to_str().unwrap().to_string();
    c.init(&ws, &url, &["a"], false);

    // A 项目绑定：命中 A 经验与 shared，不命中 B（B 与 A 关键词刻意重叠：缓存/发布）
    let v = c.recall(&ws, "缓存 发布");
    let results = v["result"]["results"].as_array().unwrap();
    let ids: Vec<&str> = results.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert!(ids.iter().any(|i| i.contains("a-postmortem")), "{ids:?}");
    assert!(ids.iter().any(|i| i.contains("shared-lessons")), "{ids:?}");
    assert!(
        !ids.iter().any(|i| i.contains("b-postmortem")),
        "B 经验不得串项目: {ids:?}"
    );

    // 切换到 B：A 的经验消失
    c.init(&ws, &url, &["b"], false);
    let v = c.recall(&ws, "缓存 发布");
    let ids: Vec<&str> = v["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(ids.iter().any(|i| i.contains("b-postmortem")), "{ids:?}");
    assert!(
        !ids.iter().any(|i| i.contains("a-postmortem")),
        "切换项目后不得复用 A 的索引: {ids:?}"
    );
}

#[test]
fn zero_project_binding_indexes_shared_only() {
    let c = Ctx::new();
    let (bare, _src, ws) = c.setup();
    let url = bare.to_str().unwrap().to_string();
    c.init(&ws, &url, &[], false);
    let v = c.recall(&ws, "缓存");
    let ids: Vec<&str> = v["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(ids.iter().any(|i| i.contains("shared-lessons")), "{ids:?}");
    assert!(!ids
        .iter()
        .any(|i| i.contains("a-postmortem") || i.contains("b-postmortem")));
}

#[test]
fn deleted_document_leaves_no_residue_after_refresh() {
    let c = Ctx::new();
    let (bare, src, ws) = c.setup();
    let url = bare.to_str().unwrap().to_string();
    c.init(&ws, &url, &["a"], false);
    let v = c.recall(&ws, "缓存");
    let ids: Vec<&str> = v["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(ids.iter().any(|i| i.contains("a-postmortem")));

    // 源删除 a-postmortem
    std::fs::remove_file(src.join("resources/learnings/a-postmortem.md")).unwrap();
    common::commit_rm(
        &src,
        "resources/learnings/a-postmortem.md",
        "remove learning",
    );
    // 删除提交需推送到远端，init --refresh 才能看到
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    c.init(&ws, &url, &["a"], true);

    let v = c.recall(&ws, "缓存");
    let ids: Vec<&str> = v["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(
        !ids.iter().any(|i| i.contains("a-postmortem")),
        "删除文档后索引无残留: {ids:?}"
    );
}

#[test]
fn corrupt_or_missing_index_is_rebuilt() {
    let c = Ctx::new();
    let (bare, _src, ws) = c.setup();
    let url = bare.to_str().unwrap().to_string();
    c.init(&ws, &url, &["a"], false);
    assert!(!c.recall(&ws, "缓存")["result"]["results"]
        .as_array()
        .unwrap()
        .is_empty());

    // 找到索引目录并损坏
    let ws_root = c.dr();
    let data_ws = PathBuf::from(&ws_root).join("ws");
    let mut index_dir = None;
    for entry in std::fs::read_dir(&data_ws).unwrap().flatten() {
        let idx = entry.path().join("index");
        if idx.is_dir() {
            std::fs::write(idx.join("index.json"), "{ corrupt").unwrap();
            index_dir = Some(idx);
        }
    }
    assert!(index_dir.is_some());
    let v = c.recall(&ws, "缓存");
    assert!(
        !v["result"]["results"].as_array().unwrap().is_empty(),
        "损坏索引自动重建"
    );

    // 删除索引目录 → 重建
    std::fs::remove_dir_all(index_dir.unwrap()).unwrap();
    let v = c.recall(&ws, "缓存");
    assert!(
        !v["result"]["results"].as_array().unwrap().is_empty(),
        "缺失索引能恢复"
    );
}

#[test]
fn zero_matches_returns_empty_no_random_scores() {
    let c = Ctx::new();
    let (bare, _src, ws) = c.setup();
    let url = bare.to_str().unwrap().to_string();
    c.init(&ws, &url, &["a"], false);
    let v = c.recall(&ws, "zzz 完全不存在的词汇 xyzzyx");
    let results = v["result"]["results"].as_array().unwrap();
    assert!(results.is_empty(), "零匹配不返回随机高分: {v}");
}

#[test]
fn chinese_query_matches_via_bigrams() {
    let c = Ctx::new();
    let (bare, _src, ws) = c.setup();
    let url = bare.to_str().unwrap().to_string();
    c.init(&ws, &url, &["a"], false);
    // 中文查询命中（bigram 分词）
    let v = c.recall(&ws, "缓存");
    let results = v["result"]["results"].as_array().unwrap();
    assert!(!results.is_empty(), "中文查询应有命中");
    assert!(results[0]["score"].as_f64().unwrap() > 0.0);
    // 规则文档也在索引内
    let v2 = c.recall(&ws, "祈使句");
    assert!(v2["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "rule"));
}
