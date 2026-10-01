//! 团队资源源脚手架（AIL-038）：生成 `ailoom.toml` + `resources/` 骨架，
//! 替代手写契约文件。生成后用真实清单加载器自校验，保证产出可直接被
//! `ailoom init --local-path` 绑定。

use crate::error::{code, Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct SourceInitArgs {
    /// 生成目录（不存在则创建）
    pub dir: PathBuf,
    /// team_id：`[a-z0-9-]{1,64}`
    pub team_id: String,
    /// 声明的逻辑项目（可重复）
    pub projects: Vec<String>,
    /// 声明的职能角色（可重复）
    pub roles: Vec<String>,
    /// 只生成最小骨架（不带示例资源）
    pub minimal: bool,
    /// 目录非空时合并生成（默认拒绝覆盖已有文件）
    pub force: bool,
    /// 目录还不是 Git 仓库时执行 git init + 初始提交
    pub git: bool,
}

const RESOURCE_DIRS: [&str; 9] = [
    "skills",
    "rules",
    "docs",
    "agents",
    "mcp",
    "learnings",
    "env",
    "hooks",
    "packages",
];

/// 团队 id / project id / role id 共用字符集（契约 §1）。
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn run(
    args: &SourceInitArgs,
    json: bool,
    _data_root: Option<&std::path::Path>,
) -> Result<Value> {
    if !valid_id(&args.team_id) {
        return Err(Error::new(
            code::USAGE,
            format!("team_id `{}` 非法（需 [a-z0-9-]{{1,64}}）", args.team_id),
        ));
    }
    for p in &args.projects {
        if !valid_id(p) {
            return Err(Error::new(code::USAGE, format!("project id `{p}` 非法")));
        }
    }
    for r in &args.roles {
        if !valid_id(r) {
            return Err(Error::new(code::USAGE, format!("role id `{r}` 非法")));
        }
    }

    let dir = if args.dir.is_absolute() {
        args.dir.clone()
    } else {
        std::env::current_dir()?.join(&args.dir)
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Err(Error::new(
            code::WORKSPACE_INVALID,
            format!("无法创建目录 {}: {e}", dir.display()),
        ));
    }
    // 非空且未 --force：拒绝（绝不静默覆盖已有团队源）
    let non_empty = std::fs::read_dir(&dir)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if non_empty && !args.force && dir.join(crate::manifest::MANIFEST_FILE).is_file() {
        return Err(Error::new(
            code::TARGET_CONFLICT,
            format!(
                "{} 已存在 ailoom.toml；确认要合并生成请加 --force",
                dir.display()
            ),
        )
        .fix("先查看现有清单，或换一个目录"));
    }

    let projects: Vec<String> = if args.projects.is_empty() {
        vec!["a".into()]
    } else {
        args.projects.clone()
    };
    let roles: Vec<String> = if args.roles.is_empty() {
        vec!["dev".into()]
    } else {
        args.roles.clone()
    };

    let mut written: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    write_file(
        &dir,
        "ailoom.toml",
        &manifest_toml(&args.team_id, &projects, &roles),
        args.force,
        &mut written,
        &mut skipped,
    )?;
    for sub in RESOURCE_DIRS {
        fs_create_dir_all(&dir.join("resources").join(sub))
            .map_err(|e| Error::new(code::INTERNAL, format!("创建 resources/{sub} 失败: {e}")))?;
    }
    if !args.minimal {
        write_file(
            &dir,
            "resources/skills/common-greet/SKILL.md",
            &example_skill(),
            args.force,
            &mut written,
            &mut skipped,
        )?;
        write_file(
            &dir,
            "resources/rules/team-basics.md",
            &example_rule(),
            args.force,
            &mut written,
            &mut skipped,
        )?;
        write_file(
            &dir,
            "resources/learnings/example-postmortem.md",
            &example_learning(),
            args.force,
            &mut written,
            &mut skipped,
        )?;
    }

    // 自校验：真实清单加载器 + 资源枚举必须通过，否则本次脚手架就是坏的
    let manifest = crate::manifest::TeamManifest::load_from(&dir)?;
    let entries = crate::resource::enumerate(&dir, &manifest, "scaffold", &mut Vec::new())?;
    if !args.minimal {
        assert!(
            entries.iter().any(|e| e.id.name == "common-greet"),
            "示例技能应可通过资源枚举"
        );
    }

    let mut git_inited = false;
    if args.git && !dir.join(".git").exists() {
        crate::gitx::git_init(&dir, false)?;
        crate::gitx::git(
            &dir,
            &[
                "-c",
                "user.name=ailoom-scaffold",
                "-c",
                "user.email=scaffold@ailoom.invalid",
                "-c",
                "commit.gpgsign=false",
                "add",
                "-A",
            ],
        )?;
        crate::gitx::git(
            &dir,
            &[
                "-c",
                "user.name=ailoom-scaffold",
                "-c",
                "user.email=scaffold@ailoom.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "ailoom: 团队资源源初始化",
            ],
        )?;
        git_inited = true;
    }

    let value = json!({
        "dir": dir.display().to_string(),
        "team_id": args.team_id,
        "projects": projects,
        "roles": roles,
        "written": written,
        "skipped_existing": skipped,
        "resources_enumerated": entries.len(),
        "git_inited": git_inited,
        "next": format!(
            "成员接入：ailoom init --local-path {} --project {}（或先 push 到远端用 --url）",
            dir.display(),
            projects[0]
        ),
    });
    if !json {
        println!(
            "团队源骨架已生成：{}（{} 个文件，清单自校验通过）",
            dir.display(),
            written.len()
        );
    }
    Ok(value)
}

fn manifest_toml(team_id: &str, projects: &[String], roles: &[String]) -> String {
    let mut s = String::new();
    s.push_str("schema_version = 1\n");
    s.push_str(&format!("team_id = \"{team_id}\"\n\n"));
    for p in projects {
        s.push_str(&format!("[projects.{p}]\nname = \"{p}\"\n"));
    }
    s.push('\n');
    for r in roles {
        s.push_str(&format!("[roles.{r}]\ndescription = \"{r}\"\n"));
    }
    s.push_str("\n[namespaces]\nknown = [\"common\", \"team-lib\"]\nshared = [\"common\"]\n");
    s
}

fn example_skill() -> String {
    "---\nname: common-greet\ndescription: 团队标准问候（示例，可修改/删除）\nshared: true\nprojects: []\nroles: []\nnamespace: common\n---\n\n# common-greet\n\n调用本 skill 时输出一行：大家好，这是来自团队技能库的问候。\n".into()
}

fn example_rule() -> String {
    "---\nname: team-basics\ndescription: 团队协作基本约定（示例）\nshared: true\nprojects: []\nroles: []\nnamespace: common\n---\n\n# 团队基本约定\n\n- 提交信息使用祈使句。\n- 变更先过评审再合并。\n".into()
}

fn example_learning() -> String {
    "---\nname: example-postmortem\ndescription: 复盘示例（shared 全员可见）\nshared: true\nnamespace: common\n---\n\n# 示例复盘\n\n现象 → 根因 → 对策。把真实事故的结论沉淀在这里。\n".into()
}

fn write_file(
    base: &Path,
    rel: &str,
    content: &str,
    force: bool,
    written: &mut Vec<String>,
    skipped: &mut Vec<String>,
) -> Result<()> {
    let path = base.join(rel);
    if let Some(parent) = path.parent() {
        fs_create_dir_all(parent).map_err(|e| {
            Error::new(
                code::INTERNAL,
                format!("创建目录 {} 失败: {e}", parent.display()),
            )
        })?;
    }
    if path.is_file() {
        if force {
            skipped.push(rel.to_string()); // 合并生成：保留已有文件
            return Ok(());
        }
        return Err(Error::new(
            code::TARGET_CONFLICT,
            format!("拒绝覆盖已有文件: {}", path.display()),
        )
        .fix("确认内容或换目录；合并生成请加 --force"));
    }
    crate::sync_common::atomic_write(&path, content.as_bytes())?;
    written.push(rel.to_string());
    Ok(())
}

fn fs_create_dir_all(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)
}
