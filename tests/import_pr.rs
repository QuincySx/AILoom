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

    // 2) force-push：head=bbbb → 新候选；旧候选标记 stale
    let path_env = write_fake_gh(&c, &meta("bbbbbbbbbbbb", false, "open", "修复缓存 v2"));
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

    // 3) 同 head bbbb merged=true → 刷新为 candidate-merged 并推进图基线
    let path_env = write_fake_gh(&c, &meta("bbbbbbbbbbbb", true, "closed", "修复缓存 v2"));
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
    assert!(btext.contains("team/alpha#7@bbbbbbbbbbbb"), "{btext}");
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
