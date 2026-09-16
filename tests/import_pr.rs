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
            .envs(common::isolated_child_env(self.tmp.path()))
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
        v["result"]["skipped_unchanged"].as_u64().unwrap() >= 2,
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
    // 候选身份含 repo（AIL-035）：文件名带 owner__repo
    assert!(draft_path.contains("team__repo-42-abc123"), "{draft_path}");
    // 显式数据根贯通：候选落在指定 data root 下
    assert!(
        draft_path.starts_with(c.tmp.path().join("data").to_str().unwrap()),
        "{draft_path}"
    );
    let text = std::fs::read_to_string(&draft_path).unwrap();
    assert!(text.contains("candidate-unverified"), "草稿标注未验证候选");
    assert!(text.contains("source_pr: https://github.com/team/repo/pull/42"));
    assert!(text.contains("status: candidate-unverified"));

    // 重复导入：PR id + head sha 去重
    let (code, stdout, _) = c.run_with_path(&ws, &refs, &path_env);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["deduplicated"], true, "重复导入去重");

    // 草稿只是本地候选：未自动评论/发布（无网络调用痕迹）
    let _ = bare;
}

// ---------- AIL-034 返工回归（R17） ----------

fn import_args(c: &Ctx, dir: &Path, target: &str, kind: &str, execute: bool) -> Vec<String> {
    let mut args = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "import".to_string(),
        "--dir".to_string(),
        dir.to_str().unwrap().to_string(),
        "--target".to_string(),
        target.to_string(),
        "--kind".to_string(),
        kind.to_string(),
    ];
    if execute {
        args.push("--execute".to_string());
    }
    args
}

fn run_refs(c: &Ctx, ws: &Path, args: &[String]) -> (i32, String, String) {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    c.run(ws, &refs)
}

/// 不存在/未允许项目在任何写入前被拒绝。
#[test]
fn import_rejects_unknown_project_before_any_write() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);
    let dir = c.tmp.path().join("d1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.md"), "# 标题\n\n内容\n").unwrap();

    // project:b 未在绑定 [a] 中
    let args = import_args(&c, &dir, "project:b", "doc", true);
    let (code, _, stderr) = run_refs(&c, &ws, &args);
    assert_ne!(code, 0, "未绑定项目必须拒绝: {stderr}");
    assert!(
        stderr.contains("b") && stderr.contains("活跃绑定"),
        "{stderr}"
    );

    // project:ghost 格式合法但不存在
    let (code, _, stderr) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:ghost", "doc", false),
    );
    assert_ne!(code, 0);
    assert!(stderr.contains("ghost"), "{stderr}");

    // checkpoint 未产生（写入前拒绝）
    let wsid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    assert!(
        !c.tmp
            .path()
            .join("data")
            .join("ws")
            .join(wsid)
            .join("import-checkpoint.json")
            .exists(),
        "拒绝时不得写入 checkpoint"
    );
}

/// 同来源同目标同类型重复导入幂等；同内容换目标/换类型按显式操作成功（不被误跳过）；
/// 内容更新 → 同名候选原位更新。
#[test]
fn import_dedup_scope_and_inplace_update() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);
    // 绑定需要两个项目才能测 shared/project 双目标
    let dir = c.tmp.path().join("d2");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("postmortem.md"), "# 复盘\n\n缓存抖动复盘\n").unwrap();

    let (code, _, stderr) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:a", "learning", true),
    );
    assert_eq!(code, 0, "{stderr}");

    // 同来源同目标同类型：重复 → 全部跳过
    let (code, stdout, stderr) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:a", "learning", true),
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["imported"], 0, "{v}");

    // 同内容 → shared 目标：显式操作成功，不被误跳过
    let (code, stdout, stderr) =
        run_refs(&c, &ws, &import_args(&c, &dir, "shared", "learning", true));
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["imported"], 1, "换目标必须重新导入: {v}");

    // 同内容 → doc 类型：同理成功
    let (code, stdout, stderr) =
        run_refs(&c, &ws, &import_args(&c, &dir, "project:a", "doc", true));
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["imported"], 1, "换类型必须重新导入: {v}");

    // 内容更新：同名候选原位更新（name 不含内容哈希，不再无限增生）
    std::fs::write(
        dir.join("postmortem.md"),
        "# 复盘\n\n缓存抖动复盘（补充根因）\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:a", "learning", true),
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["imported"], 1, "{v}");
    let planned = v["result"]["planned"].as_array().unwrap();
    let name = planned[0]["name"].as_str().unwrap().to_string();
    assert!(name.starts_with("d2-postmortem-"), "稳定候选名: {name}");
    // 再次导入更新后的内容：幂等
    let (code, stdout, _) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:a", "learning", true),
    );
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["imported"], 0, "更新后内容再导入应去重: {v}");
    assert_eq!(
        v["result"]["planned"].as_array().unwrap().len(),
        0,
        "无新增候选（原位更新，不增生）"
    );
}

/// 显式仓库列表：两个仓库同名文档不覆盖；列表导入走真实本地 Git 链路。
#[test]
fn import_repo_list_same_name_docs_not_overwritten() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);

    let mk_repo = |name: &str| {
        let p = c.tmp.path().join(name);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(
            p.join("README.md"),
            format!("# 运维手册 {name}\n\n{name} 的内容\n"),
        )
        .unwrap();
        ailoom::gitx::git_init(&p, false).unwrap();
        ailoom::gitx::git(&p, &["add", "-A"]).unwrap();
        ailoom::gitx::git(
            &p,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "init",
            ],
        )
        .unwrap();
        p
    };
    let repo_a = mk_repo("ops-docs");
    let repo_b = mk_repo("ops-docs-b");
    // 重命名目录制造同名 basename 场景：两个仓库名相同 → 前缀必须区分
    let repo_b2 = c.tmp.path().join("dup");
    std::fs::rename(&repo_b, &repo_b2).unwrap();
    let repo_a2 = c.tmp.path().join("dup2");
    std::fs::rename(&repo_a, &repo_a2).unwrap();
    // 让两个目录 basename 相同
    let d1 = c.tmp.path().join("list/ops");
    let d2 = c.tmp.path().join("list2/ops");
    std::fs::create_dir_all(c.tmp.path().join("list")).unwrap();
    std::fs::create_dir_all(c.tmp.path().join("list2")).unwrap();
    std::fs::rename(&repo_a2, &d1).unwrap();
    std::fs::rename(&repo_b2, &d2).unwrap();

    let list = c.tmp.path().join("repos.txt");
    std::fs::write(
        &list,
        format!("# 注释行\n{}\n{}\n", d1.display(), d2.display()),
    )
    .unwrap();

    let args = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "import".to_string(),
        "--dir".to_string(),
        d1.to_str().unwrap().to_string(),
        "--repo-list".to_string(),
        list.to_str().unwrap().to_string(),
        "--target".to_string(),
        "project:a".to_string(),
        "--kind".to_string(),
        "doc".to_string(),
        "--execute".to_string(),
    ];
    let (code, stdout, stderr) = run_refs(&c, &ws, &args);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let planned = v["result"]["planned"].as_array().unwrap();
    assert_eq!(planned.len(), 2, "两个仓库各导入一篇: {v}");
    let n1 = planned[0]["name"].as_str().unwrap();
    let n2 = planned[1]["name"].as_str().unwrap();
    assert_ne!(
        n1,
        n2,
        "同名文档不得互相覆盖: {}",
        serde_json::to_string(planned).unwrap()
    );
    assert!(
        n1.contains("ops-") || n2.contains("ops-"),
        "{}",
        serde_json::to_string(planned).unwrap()
    );
}

/// 源删除 → 生成审核删除建议（不自动删除）。
#[test]
fn import_source_deletion_generates_review_suggestion() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);
    let dir = c.tmp.path().join("d3");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keep.md"), "# 保留\n\n内容\n").unwrap();
    std::fs::write(dir.join("gone.md"), "# 将被删除\n\n内容\n").unwrap();

    let (code, _, stderr) = run_refs(&c, &ws, &import_args(&c, &dir, "project:a", "doc", true));
    assert_eq!(code, 0, "{stderr}");

    // 源中删除 gone.md
    std::fs::remove_file(dir.join("gone.md")).unwrap();
    let (code, stdout, stderr) =
        run_refs(&c, &ws, &import_args(&c, &dir, "project:a", "doc", true));
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let sugg = v["result"]["deletion_suggestions"].as_array().unwrap();
    assert!(
        sugg.iter()
            .any(|s| s["name"].as_str().unwrap_or("").contains("gone")),
        "应生成删除建议: {v}"
    );
    // 删除建议文件落盘（execute 模式）
    let wsid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    let del = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(wsid)
        .join("import-deletions.json");
    assert!(del.is_file(), "删除建议应落盘供审核");
}

/// 无权限子目录 → 明确失败而非静默成功。
#[cfg(unix)]
#[test]
fn import_scan_permission_error_fails_loudly() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);
    let dir = c.tmp.path().join("d4");
    let denied = dir.join("secret");
    std::fs::create_dir_all(&denied).unwrap();
    std::fs::write(dir.join("ok.md"), "# OK\n\n内容\n").unwrap();
    std::fs::write(denied.join("hidden.md"), "# 隐藏\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = std::panic::catch_unwind(|| {
        let (code, _, stderr) =
            run_refs(&c, &ws, &import_args(&c, &dir, "project:a", "doc", false));
        (code, stderr)
    });
    // 恢复权限再断言（保证 tempdir 可清理）
    let _ = std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o755));
    let (code, stderr) = result.unwrap();
    assert_ne!(code, 0, "无权限子目录必须明确失败: {stderr}");
    assert!(
        stderr.contains("扫描") || stderr.contains("错误"),
        "{stderr}"
    );
}

/// 含冒号/中文标题的文档：渲染走规范 YAML 序列化，产物过资源契约校验。
#[test]
fn import_hostile_title_renders_valid_resource() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);
    let dir = c.tmp.path().join("d5");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("weird.md"),
        "# Fix: cache '失效' \"双\" 中文\n\n正文：内容\n",
    )
    .unwrap();

    let (code, _, stderr) = run_refs(&c, &ws, &import_args(&c, &dir, "project:a", "doc", true));
    assert_eq!(code, 0, "合法标题不得产生非法 YAML: {stderr}");
}

// ---------- AIL-035 返工回归（R18） ----------

/// 可变元数据的 fake gh：从 $GH_META_FILE 读响应（pulls 与 reviews 同响应）。
fn write_fake_gh(c: &Ctx, meta: &str) -> String {
    let meta_file = c.tmp.path().join("gh-meta.json");
    std::fs::write(&meta_file, meta).unwrap();
    let fake_bin = c.tmp.path().join("fake-bin-pr");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let gh = fake_bin.join("gh");
    let script = format!("#!/bin/sh\ncat \"{}\"\n", meta_file.display());
    std::fs::write(&gh, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn pr_draft(c: &Ctx, ws: &Path, path_env: &str, url: &str) -> (i32, serde_json::Value, String) {
    let args = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "pr".to_string(),
        "--action".to_string(),
        "draft".to_string(),
        "--url".to_string(),
        url.to_string(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run_with_path(ws, &refs, path_env);
    let v = serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap_or_default();
    (code, v, stderr)
}

fn meta(head: &str, merged: bool, state: &str, title: &str) -> String {
    format!(
        r#"{{"title":"{title}","state":"{state}","merged":{merged},"body":"描述正文","head":{{"sha":"{head}"}}}}"#
    )
}

/// 状态转换全链路：force-push 失效旧候选；同 head merged 刷新并推进图基线；
/// 重复运行幂等；关闭未合并不推进。
#[test]
fn pr_state_transitions_stale_merge_and_idempotent_baseline() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);
    let url = "https://github.com/team/alpha/pull/7";

    // 1) head=aaaa merged=false → candidate-unverified
    let path_env = write_fake_gh(&c, &meta("aaaaaaaaaaaa", false, "open", "修复缓存"));
    let (code, v, stderr) = pr_draft(&c, &ws, &path_env, url);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(v["result"]["status"], "candidate-unverified", "{v}");
    let p1 = v["result"]["draft_path"].as_str().unwrap().to_string();

    // 2) force-push：head=真实提交 → 新候选；旧候选标记 stale
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/fix.rs"), "pub fn fixed() -> u32 { 1 }\n").unwrap();
    ailoom::gitx::git(&ws, &["add", "-A"]).unwrap();
    ailoom::gitx::git(
        &ws,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "fix",
        ],
    )
    .unwrap();
    let merged_head = ailoom::gitx::git(&ws, &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let path_env = write_fake_gh(&c, &meta(&merged_head, false, "open", "修复缓存 v2"));
    let (code, v, stderr) = pr_draft(&c, &ws, &path_env, url);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(v["result"]["stale_marked"], 1, "旧 head 候选被失效: {v}");
    let p2 = v["result"]["draft_path"].as_str().unwrap().to_string();
    assert_ne!(p1, p2);
    let old_text = std::fs::read_to_string(&p1).unwrap();
    assert!(
        old_text.contains("status: candidate-stale"),
        "旧候选可查询为过期: {old_text}"
    );
    assert!(std::path::Path::new(&p1).is_file(), "旧候选保留可查询");

    // 3) 同 head merged=true → 刷新为 candidate-merged 并推进图基线
    let path_env = write_fake_gh(&c, &meta(&merged_head, true, "closed", "修复缓存 v2"));
    let (code, v, stderr) = pr_draft(&c, &ws, &path_env, url);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        v["result"]["updated"], true,
        "同 head merged 变化必须刷新: {v}"
    );
    assert_eq!(v["result"]["status"], "candidate-merged", "{v}");
    let text = std::fs::read_to_string(&p2).unwrap();
    assert!(text.contains("status: candidate-merged"), "{text}");

    // 图基线记录落盘且含正确身份
    let wsid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    let baseline = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(&wsid)
        .join("graph-baseline.jsonl");
    assert!(baseline.is_file(), "合并应推进图基线记录");
    let btext = std::fs::read_to_string(&baseline).unwrap();
    assert!(
        btext.contains(&format!("team/alpha#7@{merged_head}")),
        "{btext}"
    );
    assert!(
        btext.contains("\"state\": \"success\"") || btext.contains("\"state\":\"success\""),
        "{btext}"
    );
    // 构图确实执行：项目图文件生成
    let graph = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(&wsid)
        .join("index")
        .join("codegraph-a.json");
    assert!(graph.is_file(), "合并后应触发项目构图: {}", graph.display());

    // 4) 重复运行同 head merged → 幂等去重，基线不重复
    let (code, v, _) = pr_draft(&c, &ws, &path_env, url);
    assert_eq!(code, 0);
    assert_eq!(v["result"]["deduplicated"], true, "{v}");
    assert_eq!(v["result"]["updated"], false, "{v}");
    let btext2 = std::fs::read_to_string(&baseline).unwrap();
    assert_eq!(btext2.lines().count(), 1, "重复运行不重复推进: {btext2}");

    // 5) 关闭未合并：不推进正式知识（status 仍为 unverified，pr_state=closed）
    let path_env = write_fake_gh(&c, &meta("cccccccccccc", false, "closed", "放弃的方案"));
    let (code, v, _) = pr_draft(&c, &ws, &path_env, url);
    assert_eq!(code, 0);
    assert_eq!(v["result"]["status"], "candidate-unverified", "{v}");
    assert_eq!(v["result"]["pr_state"], "closed", "{v}");
    assert_eq!(
        v["result"]["baseline"]["graph_baseline"], "not-merged",
        "{v}"
    );
}

/// 两个仓库相同 PR 号与相同 head 前缀：互不覆盖、互不误去重。
#[test]
fn pr_same_number_and_head_prefix_across_repos_distinct() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);

    let path_env = write_fake_gh(&c, &meta("abc123ff", false, "open", "同名候选"));
    let (code, v1, stderr) = pr_draft(&c, &ws, &path_env, "https://github.com/team/alpha/pull/7");
    assert_eq!(code, 0, "{stderr}");
    let (code, v2, stderr) = pr_draft(&c, &ws, &path_env, "https://github.com/team/beta/pull/7");
    assert_eq!(code, 0, "{stderr}");
    let p1 = v1["result"]["draft_path"].as_str().unwrap();
    let p2 = v2["result"]["draft_path"].as_str().unwrap();
    assert_ne!(p1, p2, "跨仓同号同 head 前缀不得互相覆盖: {p1} vs {p2}");
    assert!(
        p1.contains("team__alpha-7") && p2.contains("team__beta-7"),
        "{p1} {p2}"
    );
    assert_eq!(v1["result"]["deduplicated"], false);
    assert_eq!(v2["result"]["deduplicated"], false, "不得跨仓误去重");

    // 引号/换行标题可序列化（JSON 文件内 title 含 \" 与 \n 转义）
    let path_env = write_fake_gh(
        &c,
        &meta(
            "ddd123ff",
            false,
            "open",
            "Fix: \\\"cache\\\" 失效\\n第二行",
        ),
    );
    let (code, v3, stderr) = pr_draft(&c, &ws, &path_env, "https://github.com/team/alpha/pull/9");
    assert_eq!(code, 0, " hostile 标题不得失败: {stderr}");
    let text = std::fs::read_to_string(v3["result"]["draft_path"].as_str().unwrap()).unwrap();
    assert!(text.contains("status: candidate-unverified"), "{text}");
}

/// RW-12/R09：同一文档跨目标导入各自独立（候选名/落盘路径含 target，
/// 不覆盖原归属）；内容更新只更新对应目标且原位进行；旧版本 checkpoint
/// （名字哈希不含 target）被识别并沿用旧名，不重复发布。
#[test]
fn import_target_isolation_and_legacy_checkpoint_compat() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);
    let dir = c.tmp.path().join("d3");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("postmortem.md"), "# 复盘\n\n内容 v1\n").unwrap();

    // 目标 A：导入
    let (code, stdout, stderr) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:a", "learning", true),
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let name_a = v["result"]["planned"][0]["name"]
        .as_str()
        .unwrap()
        .to_string();

    // 目标 shared：名字必须不同（R09：不再覆盖 A 的归属）
    let (code, stdout, stderr) =
        run_refs(&c, &ws, &import_args(&c, &dir, "shared", "learning", true));
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let name_shared = v["result"]["planned"][0]["name"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(
        name_a, name_shared,
        "跨目标候选名必须隔离: {name_a} vs {name_shared}"
    );

    // 重复导入不增生
    let (_, stdout, _) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:a", "learning", true),
    );
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["imported"], 0, "{v}");

    // 内容更新：A 原位更新（名字不变），shared 独立更新
    std::fs::write(dir.join("postmortem.md"), "# 复盘\n\n内容 v2\n").unwrap();
    let (code, stdout, stderr) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:a", "learning", true),
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["imported"], 1, "{v}");
    assert_eq!(
        v["result"]["planned"][0]["name"].as_str().unwrap(),
        name_a,
        "A 原位更新名字不变"
    );
    let (code, stdout, stderr) =
        run_refs(&c, &ws, &import_args(&c, &dir, "shared", "learning", true));
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["result"]["imported"], 1, "{v}");
    assert_eq!(
        v["result"]["planned"][0]["name"].as_str().unwrap(),
        name_shared,
        "shared 独立原位更新"
    );

    // 旧 checkpoint 兼容：把 A 的记录键改写为旧版名字（哈希不含 target），
    // 再次导入应识别为已导入（跳过），不改名、不重复发布
    let wsid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    let cp_path = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(wsid)
        .join("import-checkpoint.json");
    let cp_text = std::fs::read_to_string(&cp_path).unwrap();
    let mut cp: serde_json::Value = serde_json::from_str(&cp_text).unwrap();
    let keys: Vec<String> = cp["imported"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let key_a = keys
        .iter()
        .find(|k| k.ends_with(&format!("|{name_a}")))
        .expect("checkpoint 应含 A 记录")
        .clone();
    let parts: Vec<&str> = key_a.split('|').collect();
    let identity = parts[0];
    let legacy_name = format!(
        "d3-postmortem-{}",
        ailoom::ids::sha256_prefix(format!("{identity}\0postmortem.md").as_bytes(), 8)
    );
    let legacy_key = format!("{identity}|learning|project:a|{legacy_name}");
    let rec = cp["imported"][&key_a].clone();
    cp["imported"].as_object_mut().unwrap().remove(&key_a);
    cp["imported"]
        .as_object_mut()
        .unwrap()
        .insert(legacy_key.clone(), rec);
    std::fs::write(&cp_path, serde_json::to_string_pretty(&cp).unwrap()).unwrap();

    let (code, stdout, stderr) = run_refs(
        &c,
        &ws,
        &import_args(&c, &dir, "project:a", "learning", true),
    );
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v["result"]["skipped_unchanged"], 1,
        "旧名候选应按 digest 识别为已导入（兼容，不重复发布）: {v}"
    );
    assert_eq!(v["result"]["imported"], 0, "{v}");
    assert!(
        !v["result"]["planned"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == name_a),
        "不得再以新名重复发布同一（来源,路径,目标）: {v}"
    );
}

// ---------- AIL-035 返工回归（RW-14/R12+R13）：候选完整身份与编号边界 ----------

fn gh_runner(
    c: &Ctx,
    ws: &Path,
    path_env: &str,
    json_file: std::path::PathBuf,
    url: &str,
) -> (i32, serde_json::Value, String) {
    let out = std::process::Command::new(bin())
        .args([
            "--json",
            "--data-root",
            c.dr().as_str(),
            "pr",
            "--action",
            "draft",
            "--url",
            url,
            "--project",
            "a",
        ])
        .current_dir(ws)
        .env("PATH", path_env)
        .env("PR_JSON_FILE", json_file.to_str().unwrap())
        .envs(common::isolated_child_env(c.tmp.path()))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let v = serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap_or_default();
    (out.status.code().unwrap_or(-1), v, stderr)
}

fn pr_json(head: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("gh-{}-{head}.json", std::process::id()));
    std::fs::write(
        &p,
        format!(
            r#"{{"title":"t-{head}","state":"open","merged":false,"body":"b","head":{{"sha":"{head}"}}}}"#
        ),
    )
    .unwrap();
    p
}

/// R12：同 repo 的 #1/#10/#11 并存；更新 #1 只作废 #1 的旧 head，
/// #10/#11 候选不受影响。R13：同 8 位前缀不同完整 SHA 是不同候选（不误去重）；
/// 相同完整 head 幂等；force-push 后旧候选标记 stale。
#[test]
fn pr_candidate_identity_boundaries_and_full_sha() {
    let c = Ctx::new();
    let (_bare, ws) = setup(&c);
    let fake_bin = c.tmp.path().join("fake-bin2");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let gh = fake_bin.join("gh");
    std::fs::write(&gh, "#!/bin/sh\ncat \"$PR_JSON_FILE\"\n").unwrap();
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

    let url = |n: u64| format!("https://github.com/team/repo/pull/{n}");
    let draft_dir = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(ailoom::ids::workspace_id_from_root(
            &ws.canonicalize().unwrap(),
        ))
        .join("pr-candidates");

    // R12：#1、#10、#11 并存
    for (n, head) in [(1u64, "aaaa0000"), (10, "bbbb1111"), (11, "cccc2222")] {
        let (code, v, stderr) = gh_runner(&c, &ws, &path_env, pr_json(head), &url(n));
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(v["result"]["deduplicated"], false, "{v}");
    }
    let f = |n: u64, head: &str| draft_dir.join(format!("pr-github-team__repo-{n}-{head}.md"));
    assert!(f(1, "aaaa0000").is_file());
    assert!(f(10, "bbbb1111").is_file());
    assert!(f(11, "cccc2222").is_file());

    // 更新 #1（force-push 到新 head）：只作废 #1 的旧候选，#10/#11 不受影响
    let (code, v, stderr) = gh_runner(&c, &ws, &path_env, pr_json("dddd3333"), &url(1));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(v["result"]["stale_marked"], 1, "只应标记 #1 旧候选: {v}");
    assert!(f(1, "dddd3333").is_file());
    assert!(
        !f(1, "aaaa0000").is_file() || {
            // 旧文件保留但应标记 stale
            std::fs::read_to_string(f(1, "aaaa0000"))
                .map(|t| t.contains("candidate-stale"))
                .unwrap_or(false)
        }
    );
    for (n, head) in [(10u64, "bbbb1111"), (11, "cccc2222")] {
        let text = std::fs::read_to_string(f(n, head)).unwrap();
        assert!(
            !text.contains("candidate-stale"),
            "{n} 不应被误标 stale: {text}"
        );
    }

    // R13：同 8 位前缀（deadbeef）、不同完整 SHA → 不同身份，不误去重
    let (code, v1, stderr) = gh_runner(&c, &ws, &path_env, pr_json("deadbeef1111"), &url(20));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(v1["result"]["deduplicated"], false);
    let (code, v2, _) = gh_runner(&c, &ws, &path_env, pr_json("deadbeef2222"), &url(20));
    assert_eq!(code, 0);
    assert_eq!(
        v2["result"]["deduplicated"], false,
        "同前缀不同完整 SHA 不得误去重: {v2}"
    );
    assert_ne!(
        v1["result"]["draft_path"].as_str().unwrap(),
        v2["result"]["draft_path"].as_str().unwrap()
    );
    // 同完整 head 再次 draft → 幂等
    let (code, v3, _) = gh_runner(&c, &ws, &path_env, pr_json("deadbeef1111"), &url(20));
    assert_eq!(code, 0);
    assert_eq!(v3["result"]["deduplicated"], true, "同完整 head 幂等");
    // force-push 语义：后 draft 的 head 使先前的同 PR 候选 stale
    let old =
        std::fs::read_to_string(draft_dir.join("pr-github-team__repo-20-deadbeef1111.md")).unwrap();
    assert!(old.contains("candidate-stale"), "旧 head 候选应标记 stale");

    // 旧截断文件名兼容：legacy 文件记录同一完整 head → 迁移到新名且幂等去重
    let legacy = draft_dir.join("pr-github-team__repo-30-deadbeef.md");
    std::fs::write(
        &legacy,
        "---\nstatus: candidate-unverified\nhead_sha: deadbeef9999\n---\n\n# PR #30\n",
    )
    .unwrap();
    let (code, v4, stderr) = gh_runner(&c, &ws, &path_env, pr_json("deadbeef9999"), &url(30));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        v4["result"]["deduplicated"], true,
        "legacy 同 head 应迁移并去重: {v4}"
    );
    assert!(
        draft_dir
            .join("pr-github-team__repo-30-deadbeef9999.md")
            .is_file(),
        "迁移到完整 SHA 文件名"
    );
    assert!(!legacy.exists(), "legacy 文件已迁移删除");
}

// ---------- AIL-035 返工回归（RW-13/R10+R11）：合并构图快照与重试 ----------

fn draft_dir2(c: &Ctx, ws: &Path) -> std::path::PathBuf {
    c.tmp
        .path()
        .join("data")
        .join("ws")
        .join(ailoom::ids::workspace_id_from_root(
            &ws.canonicalize().unwrap(),
        ))
        .join("pr-candidates")
}

/// R10：目标项目显式化（不隐式选第一个绑定）、未绑定/未指定显式跳过；
/// 图内容来自合并 head 的受控快照（checkout 落后不冒充）；快照不可得 → pending。
/// R11：构建失败不写成功标记、重试可完成；成功后重复幂等；closed 未合并不推进。
#[test]
fn pr_merge_graph_baseline_targeting_snapshot_and_retry() {
    let c = Ctx::new();
    let (bare, ws) = setup(&c);
    // 绑定 a、b 两个项目（与 setup 相同的源 URL，避免锁身份漂移）
    let args = vec![
        "--json".to_string(),
        "--data-root".to_string(),
        c.dr(),
        "init".to_string(),
        "--url".to_string(),
        bare.to_str().unwrap().to_string(),
        "--project".to_string(),
        "a".to_string(),
        "--project".to_string(),
        "b".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");

    // 业务仓 main：lib.rs（warm）；分支提交 adds extra.rs（cold）→ 合并 head C2
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/lib.rs"), "pub fn warm() -> u32 { 1 }\n").unwrap();
    ailoom::gitx::git(&ws, &["add", "-A"]).unwrap();
    ailoom::gitx::git(
        &ws,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "base",
        ],
    )
    .unwrap();
    ailoom::gitx::git(&ws, &["checkout", "-q", "-b", "prbranch"]).unwrap();
    std::fs::write(ws.join("src/extra.rs"), "pub fn cold() -> u32 { 2 }\n").unwrap();
    ailoom::gitx::git(&ws, &["add", "-A"]).unwrap();
    ailoom::gitx::git(
        &ws,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "pr",
        ],
    )
    .unwrap();
    let head = ailoom::gitx::git(&ws, &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    ailoom::gitx::git(&ws, &["checkout", "-q", "main"]).unwrap();

    let url = "https://github.com/team/repo/pull/9";
    let draft_with = |project: Option<&str>, head_sha: &str| {
        let path_env = write_fake_gh(&c, &meta(head_sha, true, "closed", "合并"));
        let mut args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            c.dr(),
            "pr".to_string(),
            "--action".to_string(),
            "draft".to_string(),
            "--url".to_string(),
            url.to_string(),
        ];
        if let Some(p) = project {
            args.push("--project".to_string());
            args.push(p.to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, stdout, stderr) = c.run_with_path(&ws, &refs, &path_env);
        let v = serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap_or_default();
        (code, v, stderr)
    };
    let wsid = ailoom::ids::workspace_id_from_root(&ws.canonicalize().unwrap());
    let index = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(&wsid)
        .join("index");
    let baseline = c
        .tmp
        .path()
        .join("data")
        .join("ws")
        .join(&wsid)
        .join("graph-baseline.jsonl");
    let marker_has_success = |sha: &str| {
        baseline.is_file()
            && std::fs::read_to_string(&baseline)
                .map(|t| t.lines().any(|l| l.contains(sha) && l.contains("success")))
                .unwrap_or(false)
    };

    // 1) 未指定 --project：显式跳过，不隐式选第一个绑定项目
    let (code, v, stderr) = draft_with(None, &head);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        v["result"]["baseline"].is_null()
            || v["result"]["baseline"]["graph_baseline"] == "not-merged"
            || v["result"]["baseline"]["graph_baseline"] == "skipped",
        "未指定项目不得隐式推进: {v}"
    );

    // 用新 PR 编号验证 baseline 细节（同 head 不同 PR 即不同 identity）
    let url2 = "https://github.com/team/repo/pull/91";
    let draft_url = |project: Option<&str>, head_sha: &str, u: &str| {
        let path_env = write_fake_gh(&c, &meta(head_sha, true, "closed", "合并"));
        let mut args = vec![
            "--json".to_string(),
            "--data-root".to_string(),
            c.dr(),
            "pr".to_string(),
            "--action".to_string(),
            "draft".to_string(),
            "--url".to_string(),
            u.to_string(),
        ];
        if let Some(p) = project {
            args.push("--project".to_string());
            args.push(p.to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, stdout, stderr) = c.run_with_path(&ws, &refs, &path_env);
        let v = serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap_or_default();
        (code, v, stderr)
    };
    let (code, v, stderr) = draft_url(None, &head, url2);
    assert_eq!(code, 0, "{stderr}");
    let b = v["result"]["baseline"].clone();
    assert_eq!(b["graph_baseline"], "skipped", "未指定项目应跳过: {v}");
    assert!(b["reason"].as_str().unwrap().contains("未指定"), "{b}");

    // 1b) 未绑定项目 c → skipped
    let (code, v, stderr) = draft_url(Some("c"), &head, "https://github.com/team/repo/pull/92");
    assert_eq!(code, 0, "{stderr}");
    let b = v["result"]["baseline"].clone();
    assert_eq!(b["graph_baseline"], "skipped", "{v}");
    assert!(
        b["reason"].as_str().unwrap().contains("未在工作区绑定"),
        "{b}"
    );

    // 2) 明确目标 b：推进 b；图内容来自合并 head 快照（含 extra.rs，工作区没有）
    let (code, v, stderr) = draft_url(Some("b"), &head, "https://github.com/team/repo/pull/93");
    assert_eq!(code, 0, "{stderr}");
    let b = v["result"]["baseline"].clone();
    assert_eq!(b["graph_baseline"], "advanced", "{v}");
    assert_eq!(b["graph"]["project"], "b");
    assert!(
        !index.join("codegraph-a.json").exists(),
        "不得推进未指定项目 a"
    );
    let g: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(index.join("codegraph-b.json")).unwrap())
            .unwrap();
    assert_eq!(g["revision"], head, "图 revision = 合并 head");
    assert!(
        g["files"].get("src/extra.rs").is_some(),
        "图内容来自合并快照（含工作区没有的 extra.rs）: {g}"
    );
    assert!(marker_has_success(&head), "成功标记落盘");

    // 3) 成功后重复 → already-advanced（幂等）
    {
        let head2 = {
            std::fs::write(ws.join("src/lib.rs"), "pub fn warm() -> u32 { 3 }\n").unwrap();
            ailoom::gitx::git(&ws, &["add", "-A"]).unwrap();
            ailoom::gitx::git(
                &ws,
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "-qm",
                    "again",
                ],
            )
            .unwrap();
            ailoom::gitx::git(&ws, &["rev-parse", "HEAD"])
                .unwrap()
                .trim()
                .to_string()
        };
        let (code, v, stderr) =
            draft_url(Some("b"), &head2, "https://github.com/team/repo/pull/94");
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(v["result"]["baseline"]["graph_baseline"], "advanced", "{v}");
        // 幂等：删除草稿文件后重放同 (PR, head)（dedup 分支不再短路）
        std::fs::remove_file(
            draft_dir2(&c, &ws).join(format!("pr-github-team__repo-94-{head2}.md")),
        )
        .unwrap();
        let (code, v, _) = draft_url(Some("b"), &head2, "https://github.com/team/repo/pull/94");
        assert_eq!(code, 0);
        assert_eq!(
            v["result"]["baseline"]["graph_baseline"], "already-advanced",
            "成功后重复幂等: {v}"
        );
    }

    // 4) 快照不可得（伪造 head 不在对象库）→ pending，不写成功标记
    let (code, v, stderr) = draft_url(
        Some("b"),
        "feedface0000",
        "https://github.com/team/repo/pull/96",
    );
    assert_eq!(code, 0, "{stderr}");
    let b = v["result"]["baseline"].clone();
    assert_eq!(b["graph_baseline"], "pending", "{v}");
    assert!(
        !marker_has_success("feedface0000"),
        "pending 不得写成功标记"
    );

    // 5) 构建失败（图损坏）→ CLI 非零且不写成功标记；修复后重试成功
    std::fs::write(index.join("codegraph-b.json"), "{corrupt").unwrap();
    let head3 = {
        std::fs::write(ws.join("src/lib.rs"), "pub fn warm() -> u32 { 4 }\n").unwrap();
        ailoom::gitx::git(&ws, &["add", "-A"]).unwrap();
        ailoom::gitx::git(
            &ws,
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "r3",
            ],
        )
        .unwrap();
        ailoom::gitx::git(&ws, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string()
    };
    let (code, _, stderr) = draft_url(Some("b"), &head3, "https://github.com/team/repo/pull/97");
    assert_ne!(code, 0, "图损坏时合并推进必须失败: {stderr}");
    assert!(!marker_has_success(&head3), "失败不得写成功标记");
    // 修复后重试可完成
    std::fs::remove_file(index.join("codegraph-b.json")).unwrap();
    let (code, v, stderr) = draft_url(Some("b"), &head3, "https://github.com/team/repo/pull/97");
    assert_eq!(code, 0, "重试应成功: {stderr}");
    assert_eq!(v["result"]["baseline"]["graph_baseline"], "advanced", "{v}");
    assert!(marker_has_success(&head3));
}
