use ailoom::cli::{self, Command};
use ailoom::error::Result;
use ailoom::{logging, output};
use clap::{CommandFactory, Parser};

fn main() {
    let cli = cli::Cli::parse();
    let code = match run(&cli) {
        Ok(()) => 0,
        Err(err) => {
            if cli.json {
                // 错误 JSON 只走 stderr，stdout 保持纯净
                let _ = println_err_json(&err.to_json());
            } else {
                logging::error(&err);
            }
            err.exit_code()
        }
    };
    std::process::exit(code);
}

fn println_err_json(value: &serde_json::Value) -> std::io::Result<()> {
    use std::io::Write;
    let mut stderr = std::io::stderr().lock();
    writeln!(
        stderr,
        "{}",
        serde_json::to_string(value).unwrap_or_default()
    )
}

fn run(cli: &cli::Cli) -> Result<()> {
    match &cli.command {
        Some(Command::Version) => {
            let info = cli::version_info();
            if cli.json {
                output::emit_json(&serde_json::to_value(&info)?);
            } else {
                output::emit_text(format!(
                    "{} {} (schema v{}, MSRV {})",
                    info.name, info.version, info.schema_version, info.msrv
                ));
            }
            Ok(())
        }
        Some(Command::Source {
            dir,
            team_id,
            projects,
            roles,
            minimal,
            force,
            git,
        }) => {
            let args = ailoom::commands::source_init::SourceInitArgs {
                dir: dir.clone(),
                team_id: team_id.clone(),
                projects: projects.clone(),
                roles: roles.clone(),
                minimal: *minimal,
                force: *force,
                git: *git,
            };
            let value =
                ailoom::commands::source_init::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Init {
            url,
            ref_,
            name,
            local_path,
            projects,
            roles,
            targets,
            refresh,
            no_builtin,
            root,
        }) => {
            let args = ailoom::commands::init::InitArgs {
                url: url.clone(),
                ref_: ref_.clone(),
                name: name.clone(),
                local_path: local_path.clone(),
                projects: projects.clone(),
                roles: roles.clone(),
                targets: targets.clone(),
                refresh: *refresh,
                no_builtin: *no_builtin,
                root: root.clone(),
            };
            let value = ailoom::commands::init::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Status { root }) => {
            let args = ailoom::commands::status::StatusArgs { root: root.clone() };
            let value = ailoom::commands::status::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Plan { root }) => {
            let args = ailoom::commands::plan::PlanArgs { root: root.clone() };
            let value = ailoom::commands::plan::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Sync {
            root,
            recover,
            refresh,
            from_auto,
        }) => {
            let args = ailoom::commands::sync::SyncArgs {
                root: root.clone(),
                recover: *recover,
                refresh: *refresh,
                from_auto: *from_auto,
            };
            let value = ailoom::commands::sync::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Knowledge {
            action,
            id,
            useful,
            text,
            feedback_id,
            root,
        }) => {
            let args = ailoom::knowledge::feedback::KnowledgeArgs {
                action: action.clone(),
                id: id.clone(),
                useful: *useful,
                text: text.clone(),
                feedback_id: feedback_id.clone(),
                root: root.clone(),
            };
            let value =
                ailoom::knowledge::feedback::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Hook { tool, event, root }) => {
            let args = ailoom::commands::hook_reg::HookArgs {
                tool: tool.clone(),
                event: event.clone(),
                root: root.clone(),
            };
            // Hook 契约：任何失败只诊断到 stderr，退出 0 —— 绝不阻塞宿主
            match ailoom::commands::hook_reg::run_hook_cmd(
                &args,
                cli.data_root.as_deref(),
                !cli.json,
            ) {
                Ok(value) => {
                    if cli.json {
                        output::emit_json(&value);
                    }
                }
                Err(e) => {
                    if cli.json {
                        let _ = println_err_json(&e.to_json());
                    } else {
                        logging::error(&e);
                    }
                }
            }
            Ok(())
        }
        Some(Command::Hooks {
            action,
            id,
            event,
            root,
        }) => {
            let args = ailoom::commands::hook_reg::HooksArgs {
                action: action.clone(),
                id: id.clone(),
                event: event.clone(),
                root: root.clone(),
            };
            let value = ailoom::commands::hook_reg::run_hooks_cmd(
                &args,
                cli.json,
                cli.data_root.as_deref(),
            )?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Session {
            action,
            session,
            file,
            share,
            root,
        }) => {
            let args = ailoom::commands::session::SessionArgs {
                action: action.clone(),
                session: session.clone(),
                file: file.clone(),
                share: share.clone(),
                root: root.clone(),
            };
            let value = ailoom::commands::session::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Migrate {
            from,
            subtree,
            root,
        }) => {
            let args = ailoom::import::self_repo::MigrateArgs {
                from: from.clone(),
                subtree: subtree.clone(),
                root: root.clone(),
            };
            let value =
                ailoom::import::self_repo::run_migrate(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Import {
            project,
            dir,
            repo_list,
            target,
            kind,
            execute,
            root,
        }) => {
            let args = ailoom::import::ImportArgs {
                project: project.clone(),
                dir: dir.clone(),
                repo_list: repo_list.clone(),
                target: target.clone(),
                kind: kind.clone(),
                execute: *execute,
                root: root.clone(),
            };
            let value = ailoom::import::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Pr {
            action,
            url,
            project,
            root,
        }) => {
            let args = ailoom::import::pr::PrArgs {
                action: action.clone(),
                url: url.clone(),
                project: project.clone(),
                root: root.clone(),
            };
            let value = ailoom::import::pr::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Data {
            action,
            out,
            max_size_mb,
            dry_run,
            root,
        }) => {
            let args = ailoom::data::DataArgs {
                action: action.clone(),
                out: out.clone(),
                max_size_mb: *max_size_mb,
                dry_run: *dry_run,
                root: root.clone(),
            };
            let value = ailoom::data::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Members {
            action,
            project,
            member,
            message,
            provider,
            root,
        }) => {
            let args = ailoom::membership::MembersArgs {
                action: action.clone(),
                project: project.clone(),
                member: member.clone(),
                message: message.clone(),
                provider: provider.clone(),
                root: root.clone(),
            };
            let value = ailoom::membership::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Code {
            action,
            query,
            hops,
            root,
        }) => {
            let args = ailoom::code_knowledge::CodeArgs {
                action: action.clone(),
                query: query.clone(),
                hops: *hops,
                root: root.clone(),
            };
            let value = ailoom::code_knowledge::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Packages { action, yes, root }) => {
            let args = ailoom::packages::PackagesArgs {
                action: action.clone(),
                yes: *yes,
                root: root.clone(),
            };
            let value = ailoom::packages::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Dashboard { port, root }) => {
            let args = ailoom::dashboard::server::DashboardArgs {
                port: *port,
                root: root.clone(),
            };
            ailoom::dashboard::server::run(&args, cli.data_root.as_deref())
        }
        Some(Command::Report { action, root }) => {
            let args = ailoom::reporting::ReportArgs {
                action: action.clone(),
                root: root.clone(),
            };
            let value = ailoom::reporting::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            } else {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            Ok(())
        }
        Some(Command::Recall {
            query,
            kind,
            limit,
            rebuild,
            root,
        }) => {
            let args = ailoom::commands::recall::RecallArgs {
                query: query.clone(),
                kind: kind.clone(),
                limit: *limit,
                root: root.clone(),
                rebuild: *rebuild,
            };
            let value = ailoom::commands::recall::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Contribute {
            file,
            project,
            shared,
            namespace,
            message: c_message,
            provider: c_provider,
            root: c_root,
        }) => {
            let args = ailoom::commands::contribute::ContributeArgs {
                file: file.clone(),
                project: project.clone(),
                shared: *shared,
                namespace: namespace.clone(),
                message: c_message.clone(),
                provider: c_provider.clone(),
                root: c_root.clone(),
            };
            let value =
                ailoom::commands::contribute::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::ContributeSelf { message, root }) => {
            let cwd = std::env::current_dir()?;
            let ctx = ailoom::appctx::AppContext::discover(
                cli.data_root.as_deref(),
                &cwd,
                root.as_deref(),
            )?;
            let value = ailoom::import::self_repo::contribute_self(&ctx, message)?;
            if cli.json {
                output::emit_json(&value);
            } else {
                crate::logging::info(format!(
                    "同仓贡献：{}",
                    serde_json::to_string(&value).unwrap_or_default()
                ));
            }
            Ok(())
        }
        Some(Command::Push {
            from,
            message,
            provider,
            root,
        }) => {
            let args = ailoom::contribution::PushArgs {
                from: from.clone(),
                message: message.clone(),
                provider: provider.clone(),
                root: root.clone(),
            };
            let value = ailoom::contribution::run_push(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Library {
            action,
            dir,
            name,
            execute,
            url,
            entry,
            path,
            git_ref,
            skill,
        }) => {
            let value = ailoom::commands::library::run(
                action,
                dir.as_deref(),
                name.as_deref(),
                *execute,
                cli.json,
                cli.data_root.as_deref(),
                url.as_deref().or(entry.as_deref()),
                path.as_deref(),
                git_ref.as_deref(),
                skill.as_deref(),
            )?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Personal {
            action,
            resource,
            host,
            state,
            subproject,
            worktree,
            file,
            clear,
            root,
            scope,
            repo,
        }) => {
            let data_root_resolved = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
            let value = match action.as_str() {
                "effective" => ailoom::commands::personal::effective(
                    root.as_deref(),
                    scope.clone(),
                    cli.data_root.as_deref(),
                    &data_root_resolved,
                )?,
                "plan" => ailoom::commands::personal::plan(
                    root.as_deref(),
                    scope.clone(),
                    cli.data_root.as_deref(),
                    &data_root_resolved,
                )?,
                "sync" => ailoom::commands::personal::sync(
                    root.as_deref(),
                    scope.clone(),
                    cli.data_root.as_deref(),
                    &data_root_resolved,
                )?,
                "select" => {
                    let args = ailoom::commands::personal::SelectArgs {
                        resource: resource.clone(),
                        host: host.clone(),
                        state: state.clone().ok_or_else(|| {
                            ailoom::error::Error::new(
                                ailoom::error::code::USAGE,
                                "select 需要 --state enable|disable|inherit",
                            )
                        })?,
                        subproject: subproject.clone(),
                        worktree: *worktree,
                        repo_root: repo.clone(),
                        base_revision: None,
                    };
                    ailoom::commands::personal::select(&args, &data_root_resolved)?
                }
                "deploy-status" => ailoom::commands::personal::deploy_status(
                    root.as_deref(),
                    cli.data_root.as_deref(),
                    &data_root_resolved,
                )?,
                "migrate-nongit" => ailoom::commands::personal::migrate_nongit(
                    repo.as_deref(),
                    &data_root_resolved,
                )?,
                "instructions" => ailoom::commands::personal::instructions(
                    file.as_deref(),
                    *clear,
                    *worktree,
                    &data_root_resolved,
                )?,
                other => {
                    return Err(ailoom::error::Error::new(
                        ailoom::error::code::USAGE,
                        format!(
                            "未知 personal 动作: {other}（effective | select | instructions | plan | sync | migrate-nongit）"
                        ),
                    ))
                }
            };
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Collection {
            action,
            name,
            url,
            ref_,
            source,
            preview_id,
        }) => {
            let data = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
            let required = |v: &Option<String>, field: &str| {
                v.clone().ok_or_else(|| {
                    ailoom::error::Error::new(ailoom::error::code::USAGE, format!("需要 --{field}"))
                })
            };
            let value = match action.as_str() {
                "list" => ailoom::collections::list(&data)?,
                "preview" => ailoom::collections::preview(
                    &data,
                    &required(name, "name")?,
                    &required(url, "url")?,
                    ref_.as_deref(),
                    source.as_deref(),
                )?,
                "apply" => {
                    ailoom::collections::apply_preview(&data, &required(preview_id, "preview-id")?)?
                }
                _ => {
                    return Err(ailoom::error::Error::new(
                        ailoom::error::code::USAGE,
                        "collection 动作为 list | preview | apply",
                    ))
                }
            };
            if cli.json {
                output::emit_json(&value);
            } else {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            Ok(())
        }
        Some(Command::Console { port, no_open }) => {
            let data_root = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
            let opts = ailoom::console::ConsoleOptions {
                port: *port,
                data_root,
                open_browser: !*no_open,
            };
            ailoom::console::run_blocking(&opts)
        }
        Some(Command::Doctor { root }) => {
            let args = ailoom::commands::doctor::DoctorArgs { root: root.clone() };
            let value = ailoom::commands::doctor::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        Some(Command::Uninstall { root, execute }) => {
            let args = ailoom::commands::uninstall::UninstallArgs {
                root: root.clone(),
                execute: *execute,
            };
            let value =
                ailoom::commands::uninstall::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            Ok(())
        }
        None => {
            // 无子命令：打印帮助到 stderr，用法退出码 2
            let mut cmd = cli::Cli::command();
            let _ = cmd.print_help();
            std::process::exit(2);
        }
    }
}
