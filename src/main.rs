use ailoom::cli::{self, Command};
use ailoom::error::Result;
use ailoom::{logging, output};
use clap::{CommandFactory, Parser};

fn main() {
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => exit_on_parse_error(e),
    };
    let code = match run(&cli) {
        Ok(()) => 0,
        Err(err) => {
            if cli.json {
                // 错误 JSON 只走 stderr，stdout 保持纯净
                output::emit_error_json(&err);
            } else {
                logging::error(&err);
            }
            err.exit_code()
        }
    };
    std::process::exit(code);
}

/// 参数解析失败：help / version 照常输出；`--json` 模式下按契约输出 E0001 错误 JSON（stderr，退出 2），
/// 否则沿用 clap 的人类提示。
fn exit_on_parse_error(e: clap::Error) -> ! {
    use clap::error::ErrorKind;
    let json = std::env::args().any(|a| a == "--json");
    if json && !matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
        let message = e.render().to_string();
        // 只取 clap 的错误段落（第一个空行之前），去掉后面的 Usage 与帮助提示。
        let summary = message
            .lines()
            .take_while(|l| !l.trim().is_empty())
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ");
        let summary = summary.trim_start_matches("error: ");
        output::emit_error_json(
            &ailoom::error::Error::new(ailoom::error::code::USAGE, summary.to_string())
                .fix("运行 ailoom --help 或 ailoom <命令> --help 查看用法"),
        );
        std::process::exit(2);
    }
    e.exit()
}

/// 复用已在运行的服务时，显式请求的端口可能与实际端口不同：写入结果并提示，而不是静默忽略。
fn note_port_reuse(value: &mut serde_json::Value, requested: Option<u16>) {
    let (Some(requested), Some(actual)) = (requested, value["port"].as_u64()) else {
        return;
    };
    if value["reused"] == true && actual != u64::from(requested) {
        value["requested_port"] = serde_json::json!(requested);
        logging::warn(format!(
            "已有网页服务在端口 {actual} 运行，沿用该端口；如需改用 {requested}，先运行 ailoom service stop"
        ));
    }
}

fn run(cli: &cli::Cli) -> Result<()> {
    match &cli.command {
        Some(Command::Knowledge {
            action,
            root,
            path,
            expected,
            execute,
            sync_after,
            remote,
            branch,
            subdir,
            file,
            name,
            query,
            limit,
            ..
        }) if matches!(
            action.as_str(),
            "status"
                | "init"
                | "move"
                | "save"
                | "recall"
                | "sync"
                | "configure"
                | "recover"
                | "clone"
                | "checkpoint"
        ) =>
        {
            let data = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
            let root = root.clone().unwrap_or(std::env::current_dir()?);
            let required = |v: Option<String>, label: &str| {
                v.ok_or_else(|| {
                    ailoom::error::Error::new(ailoom::error::code::USAGE, format!("缺少 --{label}"))
                })
            };
            let value = match action.as_str() {
                "save" => ailoom::knowledge::location::save(
                    &data,
                    &root,
                    file.as_deref().ok_or_else(|| {
                        ailoom::error::Error::new(ailoom::error::code::USAGE, "缺少 --file")
                    })?,
                    &required(name.clone(), "name")?,
                )?,
                "recall" => ailoom::knowledge::location::recall(
                    &data,
                    &root,
                    &required(query.clone(), "query")?,
                    *limit,
                )?,
                _ => ailoom::knowledge::location::run(
                    &data,
                    &ailoom::knowledge::location::Request {
                        action: action.clone(),
                        root,
                        path: path.clone(),
                        expected: expected.clone(),
                        execute: *execute,
                        sync_after: *sync_after,
                        remote: remote.clone(),
                        branch: branch.clone(),
                        subdir: subdir.clone(),
                    },
                )?,
            };
            if cli.json {
                output::emit_json(&value);
            } else {
                output::emit_text(serde_json::to_string_pretty(&value)?);
            }
            Ok(())
        }
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
            knowledge_path,
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
            if let Some(path) = knowledge_path {
                if url.is_some()
                    || local_path.is_some()
                    || !projects.is_empty()
                    || !roles.is_empty()
                    || !targets.is_empty()
                    || *refresh
                    || *no_builtin
                    || ref_.is_some()
                    || name.is_some()
                {
                    return Err(ailoom::error::Error::new(
                        ailoom::error::code::USAGE,
                        "--knowledge-path 单独初始化知识库；团队资源与工具设置请另行执行 init",
                    ));
                }
                let data = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
                let value = ailoom::knowledge::location::run(
                    &data,
                    &ailoom::knowledge::location::Request {
                        action: "init".into(),
                        root: root.clone().unwrap_or(std::env::current_dir()?),
                        path: Some(path.clone()),
                        ..Default::default()
                    },
                )?;
                if cli.json {
                    output::emit_json(&value);
                } else {
                    output::emit_text("项目知识库已初始化");
                }
                return Ok(());
            }
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
            ..
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
                        output::emit_error_json(&e);
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
            } else {
                output::emit_human(&value);
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
                output::emit_human(&value);
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
            preview_id,
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
                preview_id.as_deref(),
            )?;
            if cli.json {
                output::emit_json(&value);
            } else {
                output::emit_human(&value);
            }
            Ok(())
        }
        Some(Command::Personal {
            action,
            id,
            sub,
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
                // AIL-120：CLI sync 与 Web 应用同一管道，并持久化撤销清单（可 undo）
                "sync" => ailoom::console::jobs::cli_sync(
                    &data_root_resolved,
                    root.as_deref().unwrap_or(&std::env::current_dir()?),
                    scope.clone(),
                    cli.data_root.as_deref(),
                    &data_root_resolved,
                )
                .map_err(|e| {
                    ailoom::error::Error::new(ailoom::error::code::INTERNAL, e)
                })?,
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
                    None,
                    cli.data_root.as_deref(),
                    &data_root_resolved,
                )?,
                // AIL-120：CLI 撤销持久化 Apply 任务（Web 与 CLI sync 均可撤销）
                "undo" => {
                    let id = id.clone().ok_or_else(|| {
                        ailoom::error::Error::new(ailoom::error::code::USAGE, "undo 需要 --id <任务ID>")
                    })?;
                    if ailoom::console::jobs::load_job(&data_root_resolved, &id).is_none() {
                        return Err(ailoom::error::Error::new(
                            ailoom::error::code::USAGE,
                            format!("任务不存在: {id}"),
                        )
                        .fix("在网页「操作记录」中查看可撤销的任务 ID"));
                    }
                    ailoom::console::jobs::undo_persisted(&data_root_resolved, &id).map_err(
                        |e| ailoom::error::Error::new(ailoom::error::code::INTERNAL, e),
                    )?
                }
                // AIL-120：CLI 只读扫描（与控制台 /api/project/scan-skills 共用实现）
                "scan-skills" => {
                    let root_path = root.clone().ok_or_else(|| {
                        ailoom::error::Error::new(ailoom::error::code::USAGE, "scan-skills 需要 --root")
                    })?;
                    let store_root = ailoom::paths::resolve_store_root()?;
                    let store_root = store_root.canonicalize().unwrap_or(store_root);
                    let root_canon = root_path.canonicalize().map_err(|e| {
                        ailoom::error::Error::new(ailoom::error::code::USAGE, format!("路径不可用: {e}"))
                    })?;
                    let mut v = ailoom::commands::scan_skills::scan_project_skills(
                        &root_canon,
                        sub.as_deref(),
                        &store_root,
                    );
                    v["root"] = serde_json::json!(root_canon.display().to_string());
                    v["scanned_sub"] = serde_json::json!(sub);
                    v
                }
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
                            "未知 personal 动作: {other}（effective | select | instructions | plan | sync | deploy-status | undo | scan-skills | migrate-nongit）"
                        ),
                    ))
                }
            };
            if cli.json {
                output::emit_json(&value);
            } else {
                output::emit_personal(action, &value);
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
            execute,
        }) => {
            let data = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
            let required = |v: &Option<String>, field: &str| {
                v.clone().ok_or_else(|| {
                    ailoom::error::Error::new(ailoom::error::code::USAGE, format!("需要 --{field}"))
                })
            };
            let value = match action.as_str() {
                "list" => ailoom::collections::list(&data)?,
                "check" => ailoom::collections::check_updates(&data, source.as_deref())?,
                "remove" => {
                    ailoom::collections::remove(&data, &required(source, "source")?, *execute)?
                }
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
                        "collection 动作为 list | preview | apply | check | remove",
                    ))
                }
            };
            if cli.json {
                output::emit_json(&value);
            } else {
                output::emit_collection(action, &value);
            }
            Ok(())
        }
        Some(Command::Web { port, no_open }) => {
            let root = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
            let mut value = ailoom::service::web(
                &root,
                port.unwrap_or(ailoom::console::DEFAULT_PORT),
                !*no_open,
            )?;
            note_port_reuse(&mut value, *port);
            if cli.json {
                output::emit_json(&value);
            } else {
                println!("{}", value["url"].as_str().unwrap_or_default());
            }
            Ok(())
        }
        Some(Command::Service { action }) => {
            use ailoom::cli::ServiceCommand;
            let root = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
            let value = match action {
                ServiceCommand::Start { port } => {
                    let mut v = ailoom::service::start(
                        &root,
                        port.unwrap_or(ailoom::console::DEFAULT_PORT),
                    )?;
                    note_port_reuse(&mut v, *port);
                    v
                }
                ServiceCommand::Stop => ailoom::service::stop(&root)?,
                ServiceCommand::Status => ailoom::service::status(&root)?,
                ServiceCommand::Enable { port } => {
                    ailoom::service::set_autostart(&root, true, *port)?
                }
                ServiceCommand::Disable => {
                    ailoom::service::set_autostart(&root, false, ailoom::console::DEFAULT_PORT)?
                }
                ServiceCommand::Run { port } => {
                    return ailoom::service::run(&root, *port, false, false)
                }
            };
            if cli.json {
                output::emit_json(&value);
            } else {
                match action {
                    ServiceCommand::Enable { .. } => {
                        println!("已开启登录自启动，下次登录生效；当前运行状态不变。")
                    }
                    ServiceCommand::Disable => println!("已关闭登录自启动；当前运行状态不变。"),
                    _ => {
                        let state = match value["state"].as_str() {
                            Some("running") => "运行中",
                            Some("stopping") => "正在停止，等待任务完成",
                            Some("unreachable") => "暂时无响应",
                            _ => "已停止",
                        };
                        println!("网页服务：{state}");
                        println!(
                            "登录自启动：{}",
                            if value["autostart"]["enabled"] == true {
                                "已开启"
                            } else {
                                "已关闭"
                            }
                        );
                        if let Some(port) = value["port"].as_u64() {
                            println!("端口：{port}；打开网页：ailoom web");
                        }
                    }
                }
            }
            Ok(())
        }
        Some(Command::Console { port, no_open }) => {
            logging::info(
                "ailoom console 是前台调试入口；日常使用请运行 ailoom web（后台服务，可复用）",
            );
            let data_root = ailoom::paths::resolve_data_root(cli.data_root.as_deref())?;
            let opts = ailoom::console::ConsoleOptions {
                port: *port,
                data_root,
                open_browser: !*no_open,
            };
            ailoom::console::run_blocking(&opts)
        }
        Some(Command::Doctor { root, strict }) => {
            let args = ailoom::commands::doctor::DoctorArgs { root: root.clone() };
            let value = ailoom::commands::doctor::run(&args, cli.json, cli.data_root.as_deref())?;
            if cli.json {
                output::emit_json(&value);
            }
            if *strict && value["ok"] == false {
                std::process::exit(10);
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
            eprintln!("{}", cmd.render_help());
            std::process::exit(2);
        }
    }
}
