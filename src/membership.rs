//! 成员名册与项目查询（AIL-024）。
//! 语义（契约）：区分"当前目录激活项目"（binding）与"成员参与项目集合"（团队名册）；
//! 名册是团队共享数据，经 AIL-014 贡献审核路径变更；切换目录不会移除历史参与记录。
//! 名册不提供 Git 内容级保密权限（那是 Git 托管方的能力）。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use crate::manifest::TeamManifest;
use crate::source::SourcesLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const ROSTER_FILE: &str = "membership/roster.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Roster {
    pub schema_version: u32,
    /// 成员 ID → 参与项目集合
    #[serde(default)]
    pub members: BTreeMap<String, MemberEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MemberEntry {
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub archived: bool,
}

impl Roster {
    pub fn parse(text: &str) -> Result<Roster> {
        let r: Roster = toml::from_str(text)
            .map_err(|e| Error::new(code::MANIFEST_MISSING_FIELD, format!("名册解析失败: {e}")))?;
        if r.schema_version != 1 {
            return Err(Error::new(
                code::SCHEMA_VERSION,
                format!("名册 schema_version 不支持: {}", r.schema_version),
            ));
        }
        Ok(r)
    }

    /// 登记成员到项目（幂等、去重、排序）。
    pub fn register(&mut self, member: &str, project: &str) -> bool {
        let entry = self.members.entry(member.to_string()).or_default();
        if entry.projects.iter().any(|p| p == project) {
            return false;
        }
        entry.projects.push(project.to_string());
        entry.projects.sort();
        true
    }

    /// 显式移除（归档语义：记录保留，标记 archived；不因 cwd 变化自动触发）。
    pub fn remove(&mut self, member: &str, project: &str) -> bool {
        let Some(entry) = self.members.get_mut(member) else {
            return false;
        };
        let before = entry.projects.len();
        entry.projects.retain(|p| p != project);
        if entry.projects.len() != before {
            entry.archived = true;
            return true;
        }
        false
    }
}

/// 读取锁定快照中的团队名册（团队侧查询视图）。
pub fn load_roster(ctx: &AppContext, declaration: &ProjectDeclaration) -> Result<Roster> {
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(crate::workspace::AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = SourcesLock::load(&lock_path)?;
    let entry = lock
        .and_then(|l| l.sources.get(&declaration.source.name).cloned())
        .ok_or_else(|| Error::new(code::SOURCE_NOT_CACHED, "源未锁定").fix("先运行 ailoom init"))?;
    let snapshot = if declaration.source.kind == "git" {
        let src = crate::source::GitSource::new(
            entry.identity.trim_start_matches("git+"),
            entry.ref_.as_deref(),
        )?;
        src.resolve(&ctx.source_cache(&src.identity), Some(&entry))?
    } else {
        let p = declaration.source.path.clone().unwrap_or_default();
        let base = if PathBuf::from(&p).is_absolute() {
            PathBuf::from(&p)
        } else {
            ctx.workspace.workspace_root.join(&p)
        };
        crate::source::LocalSource::new(&base)?.resolve()?
    };
    let roster_file = snapshot.root.join(ROSTER_FILE);
    if !roster_file.is_file() {
        return Ok(Roster {
            schema_version: 1,
            members: Default::default(),
        });
    }
    Roster::parse(&std::fs::read_to_string(&roster_file)?)
}

pub struct MembersArgs {
    pub action: String, // list | projects | register | remove
    pub project: Option<String>,
    pub member: Option<String>,
    pub message: String,
    pub provider: String,
    pub root: Option<PathBuf>,
}

pub fn run(args: &MembersArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, args.root.as_deref())?;
    let declaration = ctx
        .declaration_path()
        .and_then(|p| ProjectDeclaration::load(&p).ok().flatten())
        .ok_or_else(|| {
            Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
        })?;

    match args.action.as_str() {
        // 团队侧查询：列出某项目的成员（或全部）
        "list" | "projects" => {
            let roster = load_roster(&ctx, &declaration)?;
            let value = match (&args.action[..], &args.project, &args.member) {
                ("list", Some(project), None) => {
                    let members: Vec<&str> = roster
                        .members
                        .iter()
                        .filter(|(_, e)| e.projects.iter().any(|p| p == project))
                        .map(|(k, _)| k.as_str())
                        .collect();
                    serde_json::json!({ "project": project, "members": members })
                }
                ("projects", _, Some(member)) => {
                    let projects = roster
                        .members
                        .get(member)
                        .map(|e| e.projects.clone())
                        .unwrap_or_default();
                    serde_json::json!({ "member": member, "projects": projects })
                }
                _ => {
                    return Err(Error::new(
                        code::USAGE,
                        "list 需要 --project <id>；projects 需要 --member <id>",
                    ))
                }
            };
            if !json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            Ok(value)
        }
        // 登记/移除：产生名册变更集 → 贡献审核路径（幂等；离线可重试）
        "register" | "remove" => {
            let member = args.member.clone().ok_or_else(|| {
                Error::new(code::USAGE, "需要 --member <id>（成员身份不自动推断）")
            })?;
            let project = args
                .project
                .clone()
                .ok_or_else(|| Error::new(code::USAGE, "需要 --project <id>"))?;
            // 名册校验基于锁定清单（项目必须存在）
            let lock_path = ctx
                .workspace
                .workspace_root
                .join(crate::workspace::AILOOM_DIR)
                .join("machine")
                .join("sources.lock.json");
            let lock = SourcesLock::load(&lock_path)?;
            let entry = lock
                .and_then(|l| l.sources.get(&declaration.source.name).cloned())
                .ok_or_else(|| Error::new(code::SOURCE_NOT_CACHED, "源未锁定"))?;
            let snapshot = if declaration.source.kind == "git" {
                let src = crate::source::GitSource::new(
                    entry.identity.trim_start_matches("git+"),
                    entry.ref_.as_deref(),
                )?;
                src.resolve(&ctx.source_cache(&src.identity), Some(&entry))?
            } else {
                return Err(Error::new(
                    code::USAGE,
                    "本地目录源暂不支持名册登记（需 Git 远端）",
                ));
            };
            let manifest = TeamManifest::load_from(&snapshot.root)?;
            manifest.require_project(&project)?;

            let mut roster = load_roster(&ctx, &declaration)?;
            let changed = if args.action == "register" {
                roster.register(&member, &project)
            } else {
                roster.remove(&member, &project)
            };
            let value = if !changed {
                serde_json::json!({ "changed": false, "member": member, "project": project })
            } else {
                let roster_text = toml::to_string_pretty(&roster)?;
                // 经贡献链路提交（精确路径 membership/roster.toml）
                let req =
                    crate::contribution::prepare_contribution(data_root, args.root.as_deref())?;
                let (wt, cs) = crate::contribution::ensure_worktree(
                    &req.ctx,
                    &req.cache_repo,
                    &req.source_alias,
                    &req.base,
                )?;
                let result = (|| -> Result<Value> {
                    let dest = wt.path.join(ROSTER_FILE);
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&dest, roster_text.as_bytes())?;
                    crate::gitx::git(&wt.path, &["add", "--", ROSTER_FILE])?;
                    let message = format!(
                        "{}: {} {} {}",
                        args.message,
                        member,
                        if args.action == "register" { "+" } else { "-" },
                        project
                    );
                    crate::contribution::commit_and_push(
                        crate::contribution::PushEnv {
                            ctx: &req.ctx,
                            wt: &wt,
                            cs: &cs,
                            identity: &req.source_identity,
                            provider: &args.provider,
                            source_alias: &req.source_alias,
                        },
                        &[ROSTER_FILE.to_string()],
                        None,
                        &message,
                    )
                })();
                let value = match result {
                    Ok(mut v) => {
                        v["changed"] = serde_json::json!(true);
                        v
                    }
                    Err(e) => {
                        wt.cleanup();
                        return Err(e.fix("离线登记可重试：恢复网络后重新执行同一命令"));
                    }
                };
                wt.cleanup();
                value
            };
            if !json {
                crate::logging::info(format!(
                    "名册{}：{member} -> {project}",
                    if args.action == "register" {
                        "登记"
                    } else {
                        "移除"
                    }
                ));
            }
            Ok(value)
        }
        other => Err(Error::new(
            code::USAGE,
            format!("未知 members 动作: {other}（list/projects/register/remove）"),
        )),
    }
}

/// 名册不能控制 Git 文件保密权限（契约提示，供 doctor/文档引用）。
pub const RBAC_DISCLAIMER: &str = "名册是参与记录，不提供 Git 内容级访问控制";
