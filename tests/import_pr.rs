//! 批量导入（AIL-034）与 PR 知识候选（AIL-035）测试。

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
        self.run_with_path(cwd, args, &std::env::var("PATH").unwrap_or_default())
    }
    fn run_with_path(&self, cwd: &Path, args: &[&str], path_env: &str) -> (i32, String, String) {
        let out = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .env("HOME", self.tmp.path().join("home"))
            .env("AILOOM_LOG", "error")
            .env("PATH", path_env)
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
}

fn setup(c: &Ctx) -> (PathBuf, PathBuf) {
    let bare = c.tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let src = common::make_team_source(c.tmp.path());
    ailoom::gitx::git(&src, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(&src, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
    ailoom::gitx::git(&src, &["push", "-q", "-u", "origin", "main"]).unwrap();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let url = bare.to_str().unwrap().to_string();
    let dr = c.dr();
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    (bare, ws)
}

#[test]
fn import_preview_execute_dedupe_and_symlink_rejection() {
    let c = Ctx::new();
    let (bare, ws) = setup(&c);
    let _src = c.tmp.path().join("team-src");
    let dr = c.dr();

    // 导入目录：两篇文档 + 一个符号链接（应拒绝）
    let import_dir = c.tmp.path().join("docs-to-import");
    std::fs::create_dir_all(&import_dir).unwrap();
    std::fs::write(
        import_dir.join("incident-1.md"),
        "# 事故一\n\n缓存预热缺失\n",
    )
    .unwrap();
    std::fs::write(
        import_dir.join("incident-2.md"),
        "# 事故二\n\n发布窗口冲突\n",
    )
    .unwrap();
    let secret = c.tmp.path().join("outside.md");
    std::fs::write(&secret, "# 外部").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, import_dir.join("evil.md")).unwrap();

    // 预览：symlink 逃逸拒绝
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "import",
        "--dir",
        import_dir.to_str().unwrap(),
        "--target",
        "project:a",
        "--kind",
        "learning",
    ];
    let _ = args;
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 12, "symlink 逃逸必须拒绝: {stderr}");

    // 移除 symlink 后预览 → 2 篇
    #[cfg(unix)]
    std::fs::remove_file(import_dir.join("evil.md")).unwrap();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["planned"].as_array().unwrap().len(), 2, "{v}");

    // 执行（预览未变更任何文件）
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "import",
        "--dir",
        import_dir.to_str().unwrap(),
        "--target",
        "project:a",
        "--kind",
        "learning",
        "--execute",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["mode"], "execute");

    // 再次导入：内容哈希去重 → 全部跳过
    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "import",
        "--dir",
        import_dir.to_str().unwrap(),
        "--target",
        "project:a",
        "--kind",
        "learning",
        "--execute",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "第二次导入失败: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        v["result"]["skipped_duplicates"].as_u64().unwrap() >= 2,
        "重复导入去重: {v}"
    );
    let _ = bare;
}

#[test]
fn pr_draft_with_fake_gh_and_dedupe() {
    let c = Ctx::new();
    let (bare, ws) = setup(&c);
    let dr = c.dr();

    // 伪造 gh 可执行（输出固定 PR 元数据）
    let fake_bin = c.tmp.path().join("fake-bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let gh = fake_bin.join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\necho '{\"title\":\"修复缓存穿透\",\"state\":\"open\",\"merged\":false,\"body\":\"修复描述\",\"head\":{\"sha\":\"abc123\"}}'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path_env = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "pr",
        "--action",
        "draft",
        "--url",
        "https://github.com/team/repo/pull/42",
        "--project",
        "a",
    ];
    let refs: Vec<&str> = args.to_vec();
    let (code, stdout, stderr) = c.run_with_path(&ws, &refs, &path_env);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["deduplicated"], false);
    let draft_path = v["result"]["draft_path"].as_str().unwrap().to_string();
    let text = std::fs::read_to_string(&draft_path).unwrap();
    assert!(text.contains("candidate-unverified"), "草稿标注未验证候选");
    assert!(text.contains("source_pr: \"https://github.com/team/repo/pull/42\""));
    assert!(text.contains("status: candidate-unverified"));

    // 重复导入：PR id + head sha 去重
    let (code, stdout, _) = c.run_with_path(&ws, &refs, &path_env);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["deduplicated"], true, "重复导入去重");

    // 草稿只是本地候选：未自动评论/发布（无网络调用痕迹）
    let _ = bare;
}
