//! 通过本机 AI CLI 整理仓库知识。固定工具、参数数组、只读分析，结果由 AILoom 保存。
use super::location::{self, OrganizationInput};
use crate::error::{code, Error, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUNNERS: &[(&str, &str, &str)] = &[
    ("codex", "Codex", "codex"),
    ("claude", "Claude Code", "claude"),
    ("pi", "Pi", "pi"),
    ("cursor", "Cursor Agent", "agent"),
];
const OUTPUT_LIMIT: u64 = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30 * 60);
// Karpathy LLM Wiki 的单文档适配依据见 docs/research/KARPATHY-LLM-WIKI.md。
const ORGANIZATION_PROMPT: &str = include_str!("res/organize-wiki.md");
const DEFAULT_FOCUS: &str =
    "全面整理本次全部来源，提炼可复用的仓库知识、开发约定、排障经验与维护建议，不额外限定主题。";

fn fail(message: impl Into<String>) -> Error {
    Error::new(code::PRECONDITION_FAILED, message)
}

#[derive(Serialize)]
pub struct Runner {
    pub id: String,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub available: bool,
}

fn executable_in(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if !candidate
                    .metadata()
                    .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
                {
                    continue;
                }
            }
            // mise 等多调用 shim 依赖文件名，不 canonicalize 可执行文件。
            return Some(candidate);
        }
        #[cfg(windows)]
        for extension in ["exe", "cmd", "bat"] {
            let candidate = dir.join(format!("{name}.{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn runners() -> Vec<Runner> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|p| p.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    if let Some(home) = crate::paths::user_home() {
        for suffix in [
            ".local/bin",
            ".local/share/mise/shims",
            ".cargo/bin",
            ".bun/bin",
        ] {
            dirs.push(home.join(suffix));
        }
    }
    dirs.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    RUNNERS
        .iter()
        .map(|(id, name, command)| {
            let executable = executable_in(command, &dirs);
            Runner {
                id: (*id).into(),
                name: (*name).into(),
                available: executable.is_some(),
                executable,
            }
        })
        .collect()
}

pub fn status(data: &Path, root: &Path) -> Result<Value> {
    let input = location::organization_input(data, root)?;
    Ok(json!({
        "knowledge_id":input.knowledge_id,
        "source_count":input.documents.len(),
        "report_path":input.path.join(&input.report_path),
        "report":input.previous_report,
        "runners":runners(),
        "report_key":input.report_path,
    }))
}

pub struct Prepared {
    input: OrganizationInput,
    runner: Runner,
    focus: String,
}

impl Prepared {
    pub fn knowledge_id(&self) -> &str {
        &self.input.knowledge_id
    }
    pub fn source_count(&self) -> usize {
        self.input.documents.len()
    }
    pub fn runner_id(&self) -> &str {
        &self.runner.id
    }
    pub fn report_key(&self) -> &Path {
        &self.input.report_path
    }
}

pub fn prepare(data: &Path, root: &Path, runner: &str, focus: &str) -> Result<Prepared> {
    if focus.chars().count() > 2000 {
        return Err(fail("整理重点最多 2000 字"));
    }
    let runner = runners()
        .into_iter()
        .find(|r| r.id == runner)
        .ok_or_else(|| fail("请选择支持的本机 AI 工具"))?;
    if !runner.available {
        return Err(fail(format!(
            "没有找到 {} CLI，请安装并在终端登录后重新检测",
            runner.name
        )));
    }
    let input = location::organization_input(data, root)?;
    if input.documents.is_empty() {
        return Err(fail(
            "知识库还没有可整理的 Markdown 知识文档；配置恢复副本和资源快照不参与整理",
        ));
    }
    Ok(Prepared {
        input,
        runner,
        focus: focus.into(),
    })
}

fn prompt(prepared: &Prepared) -> String {
    let input = &prepared.input;
    let focus = prepared.focus.trim();
    let manifest = json!({
        "repository":input.root,
        "knowledge_directory":input.path,
        "source_documents":input.documents,
        "source_origins":input.origins,
        "previous_report":input.previous_report.as_ref().map(|_| &input.report_path),
        "focus":if focus.is_empty() { DEFAULT_FOCUS } else { focus },
        "generated_at":crate::ids::now_iso(),
        "schema":"ailoom-repository-wiki/v1",
    });
    format!("{ORGANIZATION_PROMPT}\n\n## 本次任务清单\n\n{manifest}\n")
}

fn command(prepared: &Prepared, output: &Path) -> Command {
    let mut command = Command::new(prepared.runner.executable.as_ref().unwrap());
    match prepared.runner.id.as_str() {
        "codex" => {
            command
                .args([
                    "--no-daemon",
                    "-a",
                    "never",
                    "exec",
                    "--sandbox",
                    "read-only",
                    "--skip-git-repo-check",
                    "--ephemeral",
                    "--color",
                    "never",
                    "--output-last-message",
                ])
                .arg(output)
                .arg("-");
        }
        "claude" => {
            command
                .args([
                    "--print",
                    "--output-format",
                    "text",
                    "--permission-mode",
                    "dontAsk",
                    "--no-session-persistence",
                    "--strict-mcp-config",
                    "--mcp-config",
                    r#"{"mcpServers":{}}"#,
                    "--tools",
                    "Read,Glob,Grep",
                    "--add-dir",
                ])
                .arg(&prepared.input.path);
        }
        "pi" => {
            command.args([
                "--print",
                "--mode",
                "text",
                "--no-session",
                "--no-extensions",
                "--no-mcp",
                "--offline",
                "--tools",
                "read,grep,find,ls",
            ]);
        }
        "cursor" => {
            command.args([
                "--print",
                "--output-format",
                "text",
                "--mode",
                "ask",
                "--sandbox",
                "enabled",
            ]);
        }
        _ => unreachable!("runner 仅来自固定列表"),
    }
    command.current_dir(&prepared.input.root);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        // 自建子进程组：取消只作用于本次 CLI 及其后代，不碰用户的其他 Agent。
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_output(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err(fail("整理输出超过 4 MB，请缩小整理重点后重试"));
    }
    String::from_utf8(bytes).map_err(|_| fail("AI CLI 返回了非 UTF-8 文本"))
}

pub fn generate(
    prepared: &Prepared,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
) -> Result<String> {
    generate_with_timeout(prepared, cancelled, progress, TIMEOUT)
}

fn generate_with_timeout(
    prepared: &Prepared,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    timeout: Duration,
) -> Result<String> {
    if cancelled() {
        return Err(fail("整理已取消，原有知识和整理结果保留"));
    }
    let work = tempfile::tempdir()?;
    let output_path = work.path().join("report.md");
    let stdout_path = work.path().join("stdout.txt");
    let stderr_path = work.path().join("stderr.txt");
    let mut child = command(prepared, &output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(std::fs::File::create(&stdout_path)?))
        .stderr(Stdio::from(std::fs::File::create(&stderr_path)?))
        .spawn()
        .map_err(|e| fail(format!("无法启动 {}：{e}", prepared.runner.name)))?;
    let mut stdin = child.stdin.take().unwrap();
    let prompt = prompt(prepared);
    let writer = std::thread::spawn(move || stdin.write_all(prompt.as_bytes()));
    let started = Instant::now();
    progress("正在读取知识文档并提炼仓库维护建议");
    let status = loop {
        if cancelled() {
            terminate(&mut child);
            let _ = writer.join();
            return Err(fail("整理已取消，原有知识和整理结果保留"));
        }
        if started.elapsed() > timeout {
            terminate(&mut child);
            let _ = writer.join();
            return Err(fail(
                "整理超过 30 分钟，已停止本次 CLI，请检查工具连接或缩小整理重点",
            ));
        }
        if [&stdout_path, &stderr_path, &output_path].iter().any(|p| {
            p.metadata()
                .map(|m| m.len() > OUTPUT_LIMIT)
                .unwrap_or(false)
        }) {
            terminate(&mut child);
            let _ = writer.join();
            return Err(fail("AI CLI 输出过大，已停止本次整理"));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => {
                terminate(&mut child);
                let _ = writer.join();
                return Err(e.into());
            }
        }
    };
    let written = writer
        .join()
        .map_err(|_| fail("无法向 AI CLI 传入整理任务"))?;
    if !status.success() {
        // 不持久化模型的逐步工具日志；失败诊断只保留限长文本并隐藏 URL 内嵌凭据。
        let diagnostic = read_output(&stderr_path).unwrap_or_default();
        let diagnostic = crate::gitx::redact_credentials(&diagnostic);
        return Err(fail(format!(
            "{} 整理失败（{}）：{}。请先在终端确认该工具已登录且可正常运行",
            prepared.runner.name,
            status,
            diagnostic
                .chars()
                .rev()
                .take(1500)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )));
    }
    written.map_err(|e| fail(format!("AI CLI 没有完整读取整理任务：{e}")))?;
    let report = read_output(if prepared.runner.id == "codex" {
        &output_path
    } else {
        &stdout_path
    })?;
    if report.trim().is_empty() {
        return Err(fail("AI CLI 未返回整理文档，原有知识保持不变"));
    }
    Ok(report)
}

pub fn publish(data: &Path, prepared: &Prepared, report: &str) -> Result<PathBuf> {
    if report.trim().is_empty() || report.len() as u64 > OUTPUT_LIMIT {
        return Err(fail("整理文档必须是非空 Markdown，且不超过 4 MB"));
    }
    location::publish_organization(data, &prepared.input, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        let data = tmp.path().join("data");
        std::fs::create_dir(&root).unwrap();
        location::run(
            &data,
            &location::Request {
                action: "init".into(),
                root: root.clone(),
                path: Some(tmp.path().join("knowledge")),
                ..Default::default()
            },
        )
        .unwrap();
        let draft = tmp.path().join("draft.md");
        std::fs::write(&draft, "# 数据库迁移\n迁移后核对行数，先验证再切换。").unwrap();
        location::save(&data, &root, &draft, "migration").unwrap();
        (tmp, root, data)
    }

    fn prepared(data: &Path, root: &Path) -> Prepared {
        Prepared {
            input: location::organization_input(data, root).unwrap(),
            runner: Runner {
                id: "codex".into(),
                name: "test CLI".into(),
                executable: None,
                available: true,
            },
            focus: "数据库维护".into(),
        }
    }

    #[test]
    fn sources_exclude_recovery_and_generated_reports_and_publish_is_searchable() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let original = std::fs::read(p.input.path.join("learnings/migration.md")).unwrap();
        for name in [
            "projects/recovery/copy.md",
            "resources/snapshot/tool.md",
            "docs/organized/other.md",
        ] {
            let path = p.input.path.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "不应当作为原始知识").unwrap();
        }
        p = prepared(&data, &root);
        assert_eq!(p.source_count(), 1);
        let report = "# 仓库维护知识与建议\n\n运行数据库迁移后核对行数。\n来源：[迁移](../../learnings/migration.md)";
        let dest = publish(&data, &p, report).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), report);
        assert_eq!(
            std::fs::read(p.input.path.join("learnings/migration.md")).unwrap(),
            original
        );
        let next = prepared(&data, &root);
        assert_eq!(
            next.source_count(),
            1,
            "重复整理不会把旧整理结果反复当作原始知识"
        );
        assert_eq!(
            next.input.report_path, p.input.report_path,
            "每个项目稳定更新同一份维护文档"
        );
        assert_eq!(next.input.previous_report.as_deref(), Some(report));
        assert!(
            location::recall(&data, &root, "数据库迁移", 20).unwrap()["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hit| hit["source_path"] == dest.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn archived_notes_are_excluded_and_archiving_invalidates_a_running_snapshot() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let ctx = location::maintenance_context(&data, &root)
            .unwrap()
            .unwrap();
        super::super::feedback::archive(&ctx, "learnings/migration.md").unwrap();
        assert_eq!(status(&data, &root).unwrap()["source_count"], 0);
        assert!(publish(&data, &p, "已过时的整理").is_err());
        assert!(p.input.path.join("learnings/migration.md").is_file());
        super::super::feedback::restore(&ctx, "learnings/migration.md").unwrap();
        assert_eq!(status(&data, &root).unwrap()["source_count"], 1);
    }

    #[test]
    fn shared_knowledge_keeps_each_projects_report_separate() {
        let (tmp, root, data) = fixture();
        let first = prepared(&data, &root);
        let other = tmp.path().join("other-project");
        std::fs::create_dir(&other).unwrap();
        location::run(
            &data,
            &location::Request {
                action: "init".into(),
                root: other.clone(),
                path: Some(first.input.path.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let second = prepared(&data, &other);
        assert_eq!(first.knowledge_id(), second.knowledge_id());
        assert_ne!(first.report_key(), second.report_key());
        let a = publish(&data, &first, "# 第一个项目维护建议").unwrap();
        let b = publish(&data, &second, "# 第二个项目维护建议").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read_to_string(a).unwrap(), "# 第一个项目维护建议");
        assert_eq!(std::fs::read_to_string(b).unwrap(), "# 第二个项目维护建议");
    }

    #[test]
    fn changed_knowledge_or_existing_report_prevents_stale_publication() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let dest = p.input.path.join(&p.input.report_path);
        std::fs::write(
            p.input.path.join("learnings/migration.md"),
            "# 新知识\n人工更新",
        )
        .unwrap();
        assert!(publish(&data, &p, "不应覆盖")
            .unwrap_err()
            .message
            .contains("内容已变化"));
        assert!(!dest.exists());
        let p = prepared(&data, &root);
        publish(&data, &p, "上次结果").unwrap();
        let p = prepared(&data, &root);
        std::fs::write(&dest, "用户手工修订").unwrap();
        assert!(publish(&data, &p, "不应覆盖").is_err());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "用户手工修订");
        assert!(publish(&data, &p, "").is_err());
    }

    #[test]
    fn imported_documents_supply_origins_to_the_actual_task_manifest() {
        let (_tmp, root, data) = fixture();
        std::fs::write(
            root.join("worktree-lesson.md"),
            "# Branch context\nKeep provenance",
        )
        .unwrap();
        let sources = location::ingest::status(&data, &root).unwrap();
        let id = sources["sources"][0]["id"].as_str().unwrap();
        let preview = location::ingest::preview(&data, &root, id).unwrap();
        location::ingest::apply(
            &data,
            &root,
            id,
            preview["expected"].as_str().unwrap(),
            &["worktree-lesson.md".into()],
        )
        .unwrap();
        let p = prepared(&data, &root);
        let text = prompt(&p);
        let task: serde_json::Value =
            serde_json::from_str(text.rsplit("## 本次任务清单\n\n").next().unwrap()).unwrap();
        assert_eq!(task["source_documents"].as_array().unwrap().len(), 2);
        let origins = task["source_origins"].as_object().unwrap();
        assert_eq!(origins.len(), 1);
        let (path, source) = origins.iter().next().unwrap();
        assert!(task["source_documents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d == path));
        assert_eq!(source[0]["document"], "worktree-lesson.md");
        assert!(text.contains("不能用它的代码冒充来源分支的实现证据"));
    }

    #[test]
    fn empty_focus_uses_the_builtin_wiki_rules_without_user_instructions() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        for focus in ["", " \n\t "] {
            p.focus = focus.into();
            let text = prompt(&p);
            let task: serde_json::Value =
                serde_json::from_str(text.rsplit("## 本次任务清单\n\n").next().unwrap()).unwrap();
            assert_eq!(task["focus"], DEFAULT_FOCUS);
            assert_eq!(task["source_documents"], json!(["learnings/migration.md"]));
            assert!(text.starts_with(ORGANIZATION_PROMPT));
            assert!(text.contains("处理冲突与演进"));
            assert!(text.contains("不同证据状态不可混写成统一的确定结论"));
        }
    }

    #[test]
    fn runner_ids_and_focus_are_validated_without_shell_interpolation() {
        let (_tmp, root, data) = fixture();
        assert!(prepare(&data, &root, "codex; touch /tmp/unsafe", "").is_err());
        assert!(prepare(&data, &root, "/bin/sh", "").is_err());
        assert!(prepare(&data, &root, "codex", &"长".repeat(2001)).is_err());
        let p = prepared(&data, &root);
        let prompt = prompt(&p);
        assert!(prompt.contains("learnings/migration.md"));
        assert!(prompt.contains("数据库维护"));
        assert!(!prompt.contains("resources/snapshot"));
    }

    #[cfg(unix)]
    fn fake_cli(body: &str) -> (tempfile::TempDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        // 测试可执行文件只在本仓 target 内创建，不修改 PATH 或用户安装的 CLI。
        let tmp =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let path = tmp.path().join("fake-cli");
        std::fs::write(&path, format!("#!/usr/bin/env python3\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (tmp, path)
    }

    #[cfg(unix)]
    #[test]
    fn real_subprocess_uses_stdin_read_only_arguments_and_final_message_file() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let (_script, executable) = fake_cli(
            r#"import sys, pathlib
args = sys.argv[1:]
assert args[:6] == ['--no-daemon', '-a', 'never', 'exec', '--sandbox', 'read-only']
assert args[-1] == '-'
task = sys.stdin.read()
assert 'learnings/migration.md' in task
assert '数据库维护' in task
pathlib.Path(args[args.index('--output-last-message') + 1]).write_text('# 仓库维护知识与建议\n迁移后验证行数。', encoding='utf-8')
print('this is not the final report')"#,
        );
        p.runner.executable = Some(executable);
        let report = generate_with_timeout(&p, || false, |_| {}, Duration::from_secs(5)).unwrap();
        assert_eq!(report, "# 仓库维护知识与建议\n迁移后验证行数。");
        assert!(
            !p.input.path.join(&p.input.report_path).exists(),
            "生成过程不写入知识库"
        );
        publish(&data, &p, &report).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cli_failure_empty_response_timeout_and_cancel_preserve_previous_knowledge() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let original = std::fs::read(p.input.path.join("learnings/migration.md")).unwrap();
        let (_script, executable) = fake_cli(
            "import sys\nsys.stdin.read()\nsys.stderr.write('not logged in')\nsys.exit(3)",
        );
        p.runner.executable = Some(executable);
        assert!(
            generate_with_timeout(&p, || false, |_| {}, Duration::from_secs(5))
                .unwrap_err()
                .message
                .contains("not logged in")
        );
        let (_script2, executable) = fake_cli("import sys, pathlib\nsys.stdin.read()\npathlib.Path(sys.argv[sys.argv.index('--output-last-message') + 1]).write_text('')");
        p.runner.executable = Some(executable);
        assert!(
            generate_with_timeout(&p, || false, |_| {}, Duration::from_secs(5))
                .unwrap_err()
                .message
                .contains("未返回")
        );
        let (_script3, executable) = fake_cli("import sys, time\nsys.stdin.read()\ntime.sleep(30)");
        p.runner.executable = Some(executable);
        let started = Instant::now();
        assert!(
            generate_with_timeout(&p, || false, |_| {}, Duration::from_millis(250))
                .unwrap_err()
                .message
                .contains("已停止")
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        let cancel_after = Instant::now();
        assert!(generate_with_timeout(
            &p,
            || cancel_after.elapsed() > Duration::from_millis(250),
            |_| {},
            Duration::from_secs(5)
        )
        .unwrap_err()
        .message
        .contains("已取消"));
        assert_eq!(
            std::fs::read(p.input.path.join("learnings/migration.md")).unwrap(),
            original
        );
        assert!(!p.input.path.join(&p.input.report_path).exists());
    }
}
