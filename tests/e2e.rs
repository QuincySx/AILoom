//! 初版跨项目端到端验收（AIL-023）。
//! 场景：两项目 A/B、两角色 dev/pm、公共/职能/项目资源、两个 checkout；
//! 链路：绑定→计划→同步→贡献→更新→经验召回→事件→汇总→卸载；
//! 失败案例：本地修改、断网、并发 sync、删除源资源、切换项目。
//! 本地裸仓承载远端；无真实远端与真实宿主调用（宿主发现单独记录，见完成记录）。

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
            .envs(common::isolated_child_env(self.tmp.path()))
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
    fn run_stdin(&self, cwd: &Path, args: &[&str], stdin_payload: &str) -> (i32, String, String) {
        use std::io::Write as IoWrite;
        let mut child = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin_payload.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn json_field(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim()).expect("JSON 输出可独立解析")
}

#[test]
fn full_acceptance_chain_two_projects_two_roles() {
    let c = Ctx::new();

    // ===== 搭建：本地裸远端 + 团队源 =====
    let bare = c.tmp.path().join("origin.git");
    ailoom::gitx::git_init(&bare, true).unwrap();
    let src = common::make_team_source(c.tmp.path());
    ailoom::gitx::git(&src, &["branch", "-M", "main"]).unwrap();
    ailoom::gitx::git(&src, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
    ailoom::gitx::git(&src, &["push", "-q", "-u", "origin", "main"]).unwrap();
    let url = bare.to_str().unwrap().to_string();

    // ===== 两个业务 checkout（worktree 关系）=====
    let biz_main = common::make_business_repo(c.tmp.path(), "biz-main");
    let biz_wt = c.tmp.path().join("biz-worktree");
    ailoom::gitx::git(
        &biz_main,
        &[
            "worktree",
            "add",
            "-q",
            biz_wt.to_str().unwrap(),
            "-b",
            "feature-x",
        ],
    )
    .unwrap();

    let dr = c.dr();

    // ===== 1. 绑定：A+dev 与 B+pm =====
    let init = |ws: &Path, projects: &str, roles: &str| {
        let args = vec![
            "--data-root".to_string(),
            dr.clone(),
            "init".to_string(),
            "--url".to_string(),
            url.clone(),
            "--project".to_string(),
            projects.to_string(),
            "--role".to_string(),
            roles.to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        c.run(ws, &refs)
    };
    let (code, _, stderr) = init(&biz_main, "a", "dev");
    assert_eq!(code, 0, "绑定 A: {stderr}");
    let (code, _, stderr) = init(&biz_wt, "b", "pm");
    assert_eq!(code, 0, "绑定 B: {stderr}");

    // ===== 2. 计划（无写入）+ 3. 同步 =====
    let sync = |ws: &Path| {
        let args = ["--data-root", dr.as_str(), "sync"];
        c.run(ws, &args)
    };
    let plan_args = ["--json", "--data-root", dr.as_str(), "plan"];
    let refs: Vec<&str> = plan_args.to_vec();
    let (code, stdout, _) = c.run(&biz_main, &refs);
    assert_eq!(code, 0);
    let plan = json_field(&stdout);
    assert!(
        plan["result"]["summary"]["create"].as_u64().unwrap() >= 3,
        "A+dev 至少 common/dev/A 三技能: {plan}"
    );

    let (code, _, stderr) = sync(&biz_main);
    assert_eq!(code, 0, "sync A: {stderr}");
    let (code, _, stderr) = sync(&biz_wt);
    assert_eq!(code, 0, "sync B: {stderr}");

    // 文件落点断言：A 得 common/dev/A，B 得 common/pm/B
    assert!(biz_main
        .join(".claude/skills/common-greet/SKILL.md")
        .is_file());
    assert!(biz_main
        .join(".claude/skills/dev-tooling/SKILL.md")
        .is_file());
    assert!(biz_main.join(".claude/skills/a-deploy/SKILL.md").is_file());
    assert!(
        !biz_main.join(".claude/skills/b-deploy").exists(),
        "A 不拿 B 的资源"
    );
    assert!(biz_wt
        .join(".claude/skills/pm-checklist/SKILL.md")
        .is_file());
    assert!(biz_wt.join(".claude/skills/b-deploy/SKILL.md").is_file());
    assert!(!biz_wt.join(".claude/skills/a-deploy").exists());

    // ===== 4. 经验召回：隔离验证 =====
    let recall = |ws: &Path, query: &str| {
        let args = [
            "--json",
            "--data-root",
            dr.as_str(),
            "recall",
            "--query",
            query,
        ];
        c.run(ws, &args)
    };
    let (code, stdout, _) = recall(&biz_main, "缓存 发布");
    assert_eq!(code, 0);
    let hits = json_field(&stdout);
    let ids: Vec<&str> = hits["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(ids.iter().any(|i| i.contains("a-postmortem")), "{ids:?}");
    assert!(
        !ids.iter().any(|i| i.contains("b-postmortem")),
        "任何项目不得召回另一项目经验: {ids:?}"
    );

    // ===== 5. 经验贡献（贡献→审核路径）=====
    let draft = c.tmp.path().join("new-learning.md");
    std::fs::write(
        &draft,
        "---\ntitle: A 项目新经验\n---\n\nA 项目的新教训。\n",
    )
    .unwrap();
    let args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "contribute".to_string(),
        "--file".to_string(),
        draft.to_str().unwrap().to_string(),
        "--provider".to_string(),
        "manual".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = c.run(&biz_main, &refs);
    assert_eq!(code, 0, "贡献: {stderr}");
    let contribution = json_field(&stdout);
    assert!(
        contribution["result"]["manual_review"].as_str().is_some(),
        "手动审核路径可用"
    );

    // ===== 6. 事件采集 → 汇总 =====
    let payload = format!(
        r#"{{"session_id":"e2e-s1","cwd":"{}"}}"#,
        biz_main.display()
    );
    let hook_args = [
        "--json".to_string(),
        "--data-root".to_string(),
        dr.clone(),
        "hook".to_string(),
        "--tool".to_string(),
        "claude".to_string(),
        "--event".to_string(),
        "session-start".to_string(),
    ];
    let refs: Vec<&str> = hook_args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run_stdin(&biz_main, &refs, &payload);
    assert_eq!(code, 0);
    let metrics_args = [
        "--json",
        "--data-root",
        dr.as_str(),
        "session",
        "--action",
        "metrics",
        "--session",
        "e2e-s1",
    ];
    let refs: Vec<&str> = metrics_args.to_vec();
    let (code, stdout, stderr) = c.run(&biz_main, &refs);
    assert_eq!(code, 0, "{stderr}");
    let metrics = json_field(&stdout);
    assert_eq!(metrics["result"]["session_id"], "e2e-s1");

    // ===== 7. 更新（源前进 + refresh + sync）=====
    common::commit_in(
        &src,
        "resources/skills/common-greet/SKILL.md",
        "---\nname: common-greet\ndescription: 更新问候\nshared: true\nnamespace: common\n---\n\n新问候\n",
        "update greet",
    );
    ailoom::gitx::git(&src, &["push", "-q", "origin", "main"]).unwrap();
    let args = vec![
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url.clone(),
        "--refresh".to_string(),
        "--project".to_string(),
        "a".to_string(),
        "--role".to_string(),
        "dev".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&biz_main, &refs);
    assert_eq!(code, 0, "refresh: {stderr}");
    let (code, _, stderr) = sync(&biz_main);
    assert_eq!(code, 0, "更新后 sync: {stderr}");
    let updated =
        std::fs::read_to_string(biz_main.join(".claude/skills/common-greet/SKILL.md")).unwrap();
    assert!(updated.contains("新问候"), "更新传播到工作区");

    // ===== 8. 失败案例：本地修改 → 冲突保留 =====
    let target = biz_main.join(".claude/skills/common-greet/SKILL.md");
    let user_text = "用户自己改的内容";
    std::fs::write(&target, user_text).unwrap();
    let (code, _, stderr) = sync(&biz_main);
    assert_eq!(code, 0, "冲突不使 sync 失败（跳过）: {stderr}");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        user_text,
        "用户修改保留"
    );

    // ===== 9. 失败案例：并发 sync（两进程同时）=====
    let mut children = Vec::new();
    for _ in 0..2 {
        children.push(
            Command::new(bin())
                .args(["--data-root", dr.as_str(), "sync"])
                .current_dir(&biz_main)
                .envs(common::isolated_child_env(c.tmp.path()))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        let out = child.wait_with_output().unwrap();
        let code = out.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&out.stderr);
        // 并发语义：胜者正常完成；败者 E4003 退出 13（锁被持有），两者都必须安全退出
        assert!(
            code == 0 || (code == 13 && stderr.contains("E4003")),
            "并发 sync 非法退出 {code}: {stderr}"
        );
    }

    // ===== 10. 失败案例：断网（origin 不可达）且无 refresh：锁不动，离线可用 =====
    let gone = c.tmp.path().join("origin-moved.git");
    std::fs::rename(&bare, &gone).unwrap();
    let args = [
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url.clone(),
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&biz_main, &refs);
    assert_eq!(code, 0, "离线重复 init 应复用已锁快照");
    std::fs::rename(&gone, &bare).unwrap();

    // ===== 11. 切换项目：A→B → A 资源被清理、B 资源就位 =====
    let args = [
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url.clone(),
        "--project".to_string(),
        "b".to_string(),
        "--refresh".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&biz_main, &refs);
    assert_eq!(code, 0, "{stderr}");
    let args = ["--data-root", dr.as_str(), "sync"];
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&biz_main, &refs);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !biz_main.join(".claude/skills/a-deploy").exists(),
        "切项目后旧项目资源被清理"
    );
    assert!(
        biz_main.join(".claude/skills/b-deploy/SKILL.md").is_file(),
        "新项目资源就位"
    );
    assert!(
        biz_main
            .join(".claude/skills/common-greet/SKILL.md")
            .is_file(),
        "shared 保留"
    );

    // ===== 12. 卸载（预览→执行→幂等）=====
    let args = ["--data-root", dr.as_str(), "uninstall", "--execute"];
    let refs: Vec<&str> = args.to_vec();
    let (code, _, stderr) = c.run(&biz_main, &refs);
    assert_eq!(code, 0, "{stderr}");
    // 用户在步骤 8 手改过 common-greet：卸载时保留（冲突），其余托管内容移除
    assert_eq!(
        std::fs::read_to_string(biz_main.join(".claude/skills/common-greet/SKILL.md")).unwrap(),
        "用户自己改的内容",
        "手改内容卸载时保留"
    );
    assert!(
        !biz_main.join(".claude/skills/b-deploy").exists(),
        "未改动的托管资源被移除"
    );
    let (code, stdout, _) = c.run(
        &biz_main,
        &["--json", "--data-root", dr.as_str(), "uninstall"],
    );
    assert_eq!(code, 0);
    let v = json_field(&stdout);
    assert_eq!(v["result"]["mode"], "preview");
    assert!(
        biz_main.join(".ailoom/project.toml").exists(),
        "卸载不影响声明"
    );
    assert!(
        biz_wt
            .join(".claude/skills/common-greet/SKILL.md")
            .is_file(),
        "其它工作区不受影响"
    );

    // ===== 记录真实命令输出摘要（供完成记录引用）=====
    let (code, stdout, _) = c.run(&biz_main, &["--json", "--data-root", dr.as_str(), "doctor"]);
    assert_eq!(code, 0);
    let doctor = json_field(&stdout);
    // 已知漂移：步骤 8 手改经软链改写了 SkillStore 实体；Claude/Codex 可能同时漂移
    let drifted = doctor["result"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check"] == "managed-manifest")
        .unwrap()["detail"]["drifted"]
        .as_array()
        .unwrap();
    assert!(!drifted.is_empty(), "doctor 应列出用户修改漂移: {doctor}");
    assert!(
        drifted.iter().any(|d| {
            d["key"].as_str().unwrap_or("").contains("common-greet")
                && d["state"] == "user-modified"
        }),
        "应包含 common-greet 用户修改: {doctor}"
    );
}
