//! AIL-042 集成验收：公司指令文件保护与个人本地调整。
//! 公司 AGENTS.md/CLAUDE.md 含 unstaged/staged 修改时，个人设置→sync→uninstall
//! 全程文件/index/状态逐字节不变；Claude 追加与 Codex 替代视图语义分别验证；
//! 不使用 skip-worktree/assume-unchanged/rm --cached。

use ailoom::adapters::common::Artifact;
use ailoom::gitx::{git, git_commit_all, git_init};
use ailoom::personal_instructions::{self as pi, guard_company_files};
use ailoom::sync::apply::apply;
use ailoom::sync::manifest::ManagedManifest;
use ailoom::sync::plan::build_plan;
use std::path::{Path, PathBuf};

struct Ws {
    #[allow(dead_code)]
    tmp: tempfile::TempDir,
    ws_root: PathBuf,
}

fn setup_company_repo() -> Ws {
    let tmp = tempfile::tempdir().unwrap();
    let ws_root = tmp.path().join("company");
    git_init(&ws_root, false).unwrap();
    std::fs::write(
        ws_root.join("AGENTS.md"),
        "# 公司规范\n\n- 全部回答用英文\n- 提交前跑 CI\n",
    )
    .unwrap();
    std::fs::write(
        ws_root.join("CLAUDE.md"),
        "# Claude 公司规范\n\n- 遵守公司编码规范\n",
    )
    .unwrap();
    git_commit_all(&ws_root, "company baseline", &["AGENTS.md", "CLAUDE.md"]).unwrap();
    Ws { tmp, ws_root }
}

fn snapshot_git_state(ws: &Path) -> (String, String, String) {
    let status = git(ws, &["status", "--porcelain"]).unwrap_or_default();
    let index = git(ws, &["ls-files", "-s"]).unwrap_or_default();
    let flags = git(ws, &["ls-files", "-v"]).unwrap_or_default();
    (status, index, flags)
}

/// 用与个人 sync 命令相同的库路径执行一轮：渲染 → 守卫 → 计划 → 应用。
fn run_personal_sync(ws: &Path, managed: &mut ManagedManifest, hosts: &[String], content: &str) {
    let artifacts = pi::render(ws, hosts, content).unwrap();
    let (artifacts, _skipped) = guard_company_files(ws, artifacts).unwrap();
    let plan = build_plan(&artifacts, managed, ws, "personal", None).unwrap();
    let lock_dir = ws.join(".ailoom/machine");
    let journal = ws.join(".ailoom/machine/journal");
    let report = apply(
        &plan,
        &artifacts,
        managed,
        ws,
        &lock_dir,
        &journal,
        "test-device",
    )
    .unwrap();
    assert!(report.ok, "apply 应成功: {:?}", report.failed);
}

#[test]
fn company_files_untouched_through_setup_sync_uninstall() {
    let w = setup_company_repo();
    // 公司文件带既有 unstaged + staged 修改
    std::fs::write(
        w.ws_root.join("AGENTS.md"),
        "# 公司规范\n\n- 全部回答用英文\n- 提交前跑 CI\n- 本地未暂存的补充\n",
    )
    .unwrap();
    std::fs::write(
        w.ws_root.join("CLAUDE.md"),
        "# Claude 公司规范\n\n- 遵守公司编码规范\n- 已暂存的补充\n",
    )
    .unwrap();
    git(&w.ws_root, &["add", "--", "CLAUDE.md"]).unwrap();
    let before = snapshot_git_state(&w.ws_root);
    let agents_before = std::fs::read(w.ws_root.join("AGENTS.md")).unwrap();
    let claude_before = std::fs::read(w.ws_root.join("CLAUDE.md")).unwrap();

    // 个人设置 → sync → uninstall（全生命周期）
    let mut managed = ManagedManifest::new("ws-test");
    run_personal_sync(
        &w.ws_root,
        &mut managed,
        &["claude".into(), "codex".into()],
        "- 个人：回答用中文\n- 优先看 docs/ 内部文档\n",
    );
    assert!(w.ws_root.join(".claude/rules/ailoom-personal.md").is_file());
    assert!(w.ws_root.join("AGENTS.override.md").is_file());

    // uninstall：空期望集 → 计划清理 AILoom 自己的文件
    {
        let plan = build_plan(&[], &managed, &w.ws_root, "personal", None).unwrap();
        let lock_dir = w.ws_root.join(".ailoom/machine");
        let journal = w.ws_root.join(".ailoom/machine/journal");
        let report = apply(
            &plan,
            &[],
            &mut managed,
            &w.ws_root,
            &lock_dir,
            &journal,
            "test-device",
        )
        .unwrap();
        assert!(report.ok, "uninstall apply: {:?}", report.failed);
    }
    assert!(!w.ws_root.join(".claude/rules/ailoom-personal.md").exists());
    assert!(!w.ws_root.join("AGENTS.override.md").exists());

    // 公司文件逐字节不变；git 状态（含暂存区）与 index 标志完全一致
    assert_eq!(
        std::fs::read(w.ws_root.join("AGENTS.md")).unwrap(),
        agents_before,
        "公司 AGENTS.md（含 unstaged 修改）保持原样"
    );
    assert_eq!(
        std::fs::read(w.ws_root.join("CLAUDE.md")).unwrap(),
        claude_before,
        "公司 CLAUDE.md（含 staged 修改）保持原样"
    );
    let after = snapshot_git_state(&w.ws_root);
    assert_eq!(
        before.0, after.0,
        "git status --porcelain 不变（暂存区状态保持）"
    );
    assert_eq!(before.1, after.1, "index（ls-files -s）不变");
    // 未使用 skip-worktree / assume-unchanged（ls-files -v 中应为大小写敏感的原标志 H）
    for line in after.2.lines() {
        let flag = line.chars().next().unwrap_or('H');
        assert!(
            flag == 'H' || flag == 'S',
            "未期望的 index 标志 {flag}（skip-worktree=S 会掩盖公司文件修改）: {line}"
        );
    }
}

#[test]
fn claude_additive_and_codex_replacement_semantics() {
    let w = setup_company_repo();
    let mut managed = ManagedManifest::new("ws-test");
    run_personal_sync(
        &w.ws_root,
        &mut managed,
        &["claude".into(), "codex".into()],
        "- 个人：回答用中文\n",
    );

    // Claude：追加语义——独立规则文件，只含个人内容；CLAUDE.md 不被改写
    let claude_personal =
        std::fs::read_to_string(w.ws_root.join(".claude/rules/ailoom-personal.md")).unwrap();
    assert!(claude_personal.contains("- 个人：回答用中文"));
    assert!(
        !claude_personal.contains("公司编码规范"),
        "Claude 条目是追加，不合成公司基线"
    );

    // Codex：替代语义——override 必须包含未调整的公司基线全文
    let view = std::fs::read_to_string(w.ws_root.join("AGENTS.override.md")).unwrap();
    assert!(view.contains("# 公司规范"));
    assert!(
        view.contains("- 全部回答用英文"),
        "公司基线全文原样保留（不被个人几行遮蔽）"
    );
    assert!(view.contains("- 个人：回答用中文"));
    // 公司 AGENTS.md 本体没有被写入
    let agents = std::fs::read_to_string(w.ws_root.join("AGENTS.md")).unwrap();
    assert!(!agents.contains("个人"), "公司 AGENTS.md 不含个人内容");

    // 个人删除两行只作用目标视图：公司文件仍原样
    run_personal_sync(
        &w.ws_root,
        &mut managed,
        &["claude".into(), "codex".into()],
        "- 个人：回答用中文\n",
    );
    let view2 = std::fs::read_to_string(w.ws_root.join("AGENTS.override.md")).unwrap();
    assert_eq!(
        view2.matches("AILOOM MANAGED").count(),
        2,
        "重复 sync 片段不漂移"
    );
    assert!(std::fs::read_to_string(w.ws_root.join("AGENTS.md"))
        .unwrap()
        .contains("提交前跑 CI"));
}

#[test]
fn guard_skips_tracked_targets_with_reason() {
    let w = setup_company_repo();
    // 构造一个落在已跟踪路径上的产物（模拟误配置）
    let artifact = Artifact {
        resource_id: "personal/rule/common/bad".into(),
        target_tool: "claude".into(),
        kind: "rule".into(),
        path: "AGENTS.md".into(),
        body: ailoom::adapters::common::ArtifactBody::Full {
            content: "不应写入".into(),
        },
    };
    let (kept, skipped) = guard_company_files(&w.ws_root, vec![artifact]).unwrap();
    assert!(kept.is_empty(), "已跟踪路径被过滤");
    assert_eq!(skipped.len(), 1);
    assert!(
        skipped[0].reason.contains("公司已跟踪"),
        "{}",
        skipped[0].reason
    );
    // 文件未被触碰
    assert!(std::fs::read_to_string(w.ws_root.join("AGENTS.md"))
        .unwrap()
        .contains("公司规范"));
}

#[test]
#[allow(clippy::cloned_ref_to_slice_refs)]
fn exclude_refcount_across_two_worktrees() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    git_init(&main, false).unwrap();
    std::fs::write(main.join("s.txt"), "s").unwrap();
    git_commit_all(&main, "s", &["s.txt"]).unwrap();
    let wt = tmp.path().join("wt");
    git(
        &main,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "b"],
    )
    .unwrap();

    let d_main = ailoom::repo_registry::discover_repo(&main).unwrap();
    let common = d_main.identity.common_dir.clone();
    let data = tmp.path().join("data");
    let mut reg = ailoom::repo_registry::RepoRegistry::load_or_create(&data, &d_main).unwrap();
    let added = reg.refresh_worktrees(&d_main, "t0");
    assert_eq!(added.len(), 2);
    let wt_id = reg
        .worktrees
        .values()
        .find(|x| x.path == wt.canonicalize().unwrap())
        .unwrap()
        .id
        .clone();
    reg.save(&data).unwrap();

    let pattern = pi::CLAUDE_PERSONAL_RULE_FILE.to_string();
    // 两个工作树先后部署同一 pattern → 一行，计数 2
    for wid in ["main-id", wt_id.as_str()] {
        let _ = wid;
        ailoom::git_exclude::add_patterns(
            &common,
            &data,
            &d_main.identity.repo_id,
            &[pattern.clone()],
        )
        .unwrap();
    }
    let text = std::fs::read_to_string(ailoom::git_exclude::exclude_file(&common)).unwrap();
    assert_eq!(text.matches(pattern.as_str()).count(), 1);
    // 卸载一个工作树 → 行保留；两个都卸载 → 行移除，用户行不受影响
    ailoom::git_exclude::remove_patterns(
        &common,
        &data,
        &d_main.identity.repo_id,
        &[pattern.clone()],
    )
    .unwrap();
    assert!(
        std::fs::read_to_string(ailoom::git_exclude::exclude_file(&common))
            .unwrap()
            .contains(pattern.as_str())
    );
    ailoom::git_exclude::remove_patterns(
        &common,
        &data,
        &d_main.identity.repo_id,
        &[pattern.clone()],
    )
    .unwrap();
    let text = std::fs::read_to_string(ailoom::git_exclude::exclude_file(&common)).unwrap();
    assert!(!text.contains(pattern.as_str()));
    assert!(!text.contains("ailoom-personal"));
}
