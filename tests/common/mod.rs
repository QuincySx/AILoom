//! 测试共享辅助：构造符合契约 §7 最小验收场景的团队源仓库。
#![allow(dead_code)]

use ailoom::gitx::{git_commit_all, git_init};
use std::path::{Path, PathBuf};

/// 进程内调用（例如在测试进程里启动的 ConsoleServer）不经过 [`isolated_child_env`]，
/// 会直接继承开发者 shell 的 `XDG_DATA_HOME` / `XDG_STATE_HOME`，把 Skill 实体写进
/// 真实的 SkillStore。在启动进程内服务前调用：整个测试进程共用一个专属临时根。
pub fn isolate_in_process_roots() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let root = std::env::temp_dir().join(format!("ailoom-test-{}", std::process::id()));
        std::env::set_var("XDG_DATA_HOME", root.join("xdg-data"));
        std::env::set_var("XDG_STATE_HOME", root.join("xdg-state"));
    });
}

/// AIL-002：子进程测试环境的统一路径隔离。
///
/// 覆盖全部机器根来源：HOME/USERPROFILE、XDG_DATA_HOME、XDG_STATE_HOME、
/// AILOOM_DATA_ROOT、AILOOM_STORE_ROOT，全部指向本次测试临时目录内部。
/// 运行环境即使预置了真实 XDG/AILOOM 覆盖变量（或被测代码忽略 CLI
/// `--data-root`），写入也会落在临时目录，外部路径逐字节不变。
/// 隔离环境只注入 XDG 变量（契约 v1.1：已设 XDG 优先于自有 AILOOM_*），
/// 不再设置 AILOOM_DATA_ROOT / AILOOM_STORE_ROOT，避免双来源误导断言。
pub fn isolated_child_env(root: &Path) -> Vec<(String, String)> {
    let home = root.join("home");
    vec![
        ("HOME".into(), path_str(&home)),
        ("USERPROFILE".into(), path_str(&home)),
        ("XDG_DATA_HOME".into(), path_str(&root.join("xdg-data"))),
        ("XDG_STATE_HOME".into(), path_str(&root.join("xdg-state"))),
        ("AILOOM_LOG".into(), "error".into()),
    ]
}

/// 隔离环境下子进程使用的 SkillStore 根（= `$XDG_DATA_HOME/ailoom/store`）。
pub fn isolated_store_root(root: &Path) -> PathBuf {
    root.join("xdg-data").join("ailoom").join("store")
}

/// 隔离环境下子进程使用的机器数据根（= `$XDG_STATE_HOME/ailoom`）。
pub fn isolated_data_root(root: &Path) -> PathBuf {
    root.join("xdg-state").join("ailoom")
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub const MANIFEST_TOML: &str = r#"
schema_version = 1
team_id = "example-team"

[projects.a]
name = "Project A"

[projects.b]
name = "Project B"

[roles.dev]
description = "Developer"

[roles.pm]
description = "Product manager"

[namespaces]
known = ["common", "team-lib"]
shared = ["common"]
"#;

fn skill(
    name: &str,
    desc: &str,
    shared: &str,
    projects: &str,
    roles: &str,
    ns: &str,
    body: &str,
) -> String {
    format!(
        "---\nname: {name}\ndescription: {desc}\nshared: {shared}\nprojects: {projects}\nroles: {roles}\nnamespace: {ns}\n---\n\n# {name}\n\n{body}\n"
    )
}

/// 搭建团队源仓库并提交；返回仓库路径。
pub fn make_team_source(root: &Path) -> std::path::PathBuf {
    let repo = root.join("team-src");
    let skills = repo.join("resources/skills");
    let rules = repo.join("resources/rules");
    let learnings = repo.join("resources/learnings");
    let agents = repo.join("resources/agents");
    let mcp = repo.join("resources/mcp");
    for d in [&skills, &rules, &learnings, &agents, &mcp] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(repo.join("ailoom.toml"), MANIFEST_TOML).unwrap();

    // skills：契约 §7 期望集合
    std::fs::create_dir_all(skills.join("common-greet")).unwrap();
    std::fs::write(
        skills.join("common-greet/SKILL.md"),
        skill(
            "common-greet",
            "全员问候",
            "true",
            "[]",
            "[]",
            "common",
            "保持礼貌。",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(skills.join("dev-tooling")).unwrap();
    std::fs::write(
        skills.join("dev-tooling/SKILL.md"),
        skill(
            "dev-tooling",
            "开发构建",
            "false",
            "[]",
            "[dev]",
            "common",
            "make build",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(skills.join("pm-checklist")).unwrap();
    std::fs::write(
        skills.join("pm-checklist/SKILL.md"),
        skill(
            "pm-checklist",
            "PM 验收",
            "false",
            "[]",
            "[pm]",
            "common",
            "核对验收标准",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(skills.join("a-deploy")).unwrap();
    std::fs::write(
        skills.join("a-deploy/SKILL.md"),
        skill(
            "a-deploy",
            "A 发布",
            "false",
            "[a]",
            "[]",
            "team-lib",
            "A 专用发布流程",
        ),
    )
    .unwrap();
    std::fs::create_dir_all(skills.join("a-deploy/references")).unwrap();
    std::fs::write(
        skills.join("a-deploy/references/checklist.md"),
        "# A 检查单\n",
    )
    .unwrap();
    std::fs::create_dir_all(skills.join("b-deploy")).unwrap();
    std::fs::write(
        skills.join("b-deploy/SKILL.md"),
        skill(
            "b-deploy",
            "B 发布",
            "false",
            "[b]",
            "[]",
            "team-lib",
            "B 专用发布流程",
        ),
    )
    .unwrap();

    // rules
    std::fs::write(
        rules.join("coding-standards.md"),
        "---\nname: coding-standards\ndescription: 编码规范\nshared: true\nprojects: []\nroles: []\nnamespace: common\n---\n\n# 规范\n\n- 祈使句提交\n",
    )
    .unwrap();

    // learnings：A/B 同关键词 + shared，用于隔离召回
    std::fs::write(
        learnings.join("a-postmortem.md"),
        "---\nname: a-postmortem\ndescription: A 项目缓存事故复盘\nproject: a\nshared: false\nnamespace: team-lib\n---\n\n# A 缓存事故\n\n缓存击穿导致发布窗口故障。\n",
    )
    .unwrap();
    std::fs::write(
        learnings.join("b-postmortem.md"),
        "---\nname: b-postmortem\ndescription: B 项目缓存事故复盘\nproject: b\nshared: false\nnamespace: team-lib\n---\n\n# B 缓存事故\n\nB 项目的缓存与发布窗口问题。\n",
    )
    .unwrap();
    std::fs::write(
        learnings.join("shared-lessons.md"),
        "---\nname: shared-lessons\ndescription: 全员发布经验\nshared: true\nnamespace: common\n---\n\n# 全员经验\n\n发布前核对值班表与缓存预热。\n",
    )
    .unwrap();

    // agent / mcp
    std::fs::write(
        agents.join("release-helper.toml"),
        "name = \"release-helper\"\ndescription = \"发布辅助\"\ninstructions = \"检查发布清单\"\nmodel = \"inherit\"\ntools = [\"Read\"]\nshared = true\nprojects = []\nroles = []\nnamespace = \"common\"\ntargets = [\"claude\"]\n\n[tool_extras.claude]\npermission-mode = \"plan\"\n",
    )
    .unwrap();
    std::fs::write(
        mcp.join("files.toml"),
        "name = \"team-files\"\ntype = \"stdio\"\ncommand = \"uvx\"\nargs = [\"mcp-server-files\"]\nshared = true\nprojects = []\nroles = []\nnamespace = \"common\"\ntargets = [\"claude\"]\n\n[mcp.env]\nTOKEN = \"$ENV:AILOOM_TEST_TOKEN\"\n",
    )
    .unwrap();

    git_init(&repo, false).unwrap();
    git_commit_all(&repo, "init team source", &["ailoom.toml", "resources"]).unwrap();
    repo
}

/// 创建业务工作区 Git 仓库（子目录情形可直接在其中放 .ailoom/project.toml）。
pub fn make_business_repo(root: &Path, name: &str) -> std::path::PathBuf {
    let repo = root.join(name);
    git_init(&repo, false).unwrap();
    std::fs::write(repo.join("README.md"), format!("# {name}\n")).unwrap();
    git_commit_all(&repo, "init business", &["README.md"]).unwrap();
    repo
}

/// 以 file:// URL 形式获取本地源路径（git 接受本地路径）。
pub fn file_url(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

/// 检查路径下 .ailoom/project.toml 是否存在。
pub fn declaration_path(ws: &Path) -> std::path::PathBuf {
    ws.join(".ailoom/project.toml")
}

/// 直接使用 git clone 产生第二个业务 checkout（模拟另一机器/另一目录）。
pub fn clone(src: &Path, dst: &Path) -> Result<(), ailoom::error::Error> {
    ailoom::gitx::git(
        Path::new("."),
        &["clone", "-q", src.to_str().unwrap(), dst.to_str().unwrap()],
    )
    .map(|_| ())
}

pub fn commit_in(repo: &Path, rel: &str, content: &str, msg: &str) {
    let p = repo.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&p, content).unwrap();
    git_commit_all(repo, msg, &[rel]).unwrap();
}

/// 完整源（agent/mcp 面向两个工具发布，用于 Codex Unsupported 场景）。
pub fn make_team_source_full(root: &Path) -> std::path::PathBuf {
    let repo = make_team_source(root);
    let agent = repo.join("resources/agents/release-helper.toml");
    let text = std::fs::read_to_string(&agent).unwrap().replace(
        "targets = [\"claude\"]",
        "targets = [\"claude\", \"codex\"]",
    );
    std::fs::write(&agent, text).unwrap();
    let mcp = repo.join("resources/mcp/files.toml");
    let text = std::fs::read_to_string(&mcp).unwrap().replace(
        "targets = [\"claude\"]",
        "targets = [\"claude\", \"codex\"]",
    );
    std::fs::write(&mcp, text).unwrap();
    git_commit_all(&repo, "publish to both tools", &["resources"]).unwrap();
    repo
}

/// 提交一个文件的删除（git rm + commit）。
pub fn commit_rm(repo: &Path, rel: &str, msg: &str) {
    ailoom::gitx::git(repo, &["rm", "-q", "--", rel]).unwrap();
    git_commit_all(repo, msg, &[]).unwrap();
}

/// 只提交 resources 下的现有改动（不重写文件内容）。
pub fn commit_only(repo: &Path, msg: &str) {
    git_commit_all(repo, msg, &["resources"]).unwrap();
}
