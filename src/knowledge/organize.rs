//! 通过本机 AI CLI 整理仓库知识。固定工具、参数数组、只读分析，结果由 AILoom 保存。
use super::location::{self, OrganizationInput};
use crate::error::{code, Error, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

mod review;
mod runtime;
mod task;
mod workflow;
pub use review::Assessment;
pub use workflow::Checkpoint;

const RUNNERS: &[(&str, &str, &str)] = &[
    ("codex", "Codex", "codex"),
    ("claude", "Claude Code", "claude"),
    ("pi", "Pi", "pi"),
    ("cursor", "Cursor Agent", "agent"),
];
// Matches the existing library storage budget; it does not terminate a running tool.
const OUTPUT_LIMIT: u64 = 128 * 1024 * 1024;
const DIAGNOSTIC_TAIL: usize = 64 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_secs(15);
// 模型返回编纂传输稿，应用按稳定主题 ID 保存为独立 Wiki 页面。
const ORGANIZATION_PROMPT: &str = include_str!("res/organize-wiki.md");
const DEFAULT_FOCUS: &str =
    "全面整理本次全部来源中的业务知识、开发流程、模块约束、决策取舍和问题经验，保留当时背景、适用条件、演变与出处，不额外限定主题。";

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
        "report":input.previous_report.as_ref().map(|text| crate::resource::split_frontmatter(text).ok().flatten().map(|(_,body)|body).unwrap_or_else(||text.clone())),
        "project_id":input.project_id,
        "project_name":input.project_name,
        "raw_path":location::layout::source_index_at(&input.path,&input.project_id).parent(),
        "pages":input.previous_documents,
        "excluded_documents":input.excluded_documents,
        "runners":runners(),
        "report_key":input.report_path,
        "worktrees":input.worktrees,
        "runtime_policy":runtime::policy(),
    }))
}

pub struct Prepared {
    input: OrganizationInput,
    runner: Runner,
    focus: String,
    collection: Option<Value>,
    started: Instant,
    execution_record: Option<Value>,
}

impl Prepared {
    pub fn set_execution_record(&mut self, record: Value) {
        self.execution_record = Some(record);
    }
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
    pub fn worktrees(&self) -> &[location::ingest::Source] {
        &self.input.worktrees
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
    Ok(Prepared {
        input,
        runner,
        focus: focus.into(),
        collection: None,
        started: Instant::now(),
        execution_record: None,
    })
}

pub fn collect_sources(
    data: &Path,
    prepared: &mut Prepared,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
) -> Result<Value> {
    let (input, collection) =
        location::collect_organization_input(data, &prepared.input, &cancelled, &progress)?;
    prepared.input = input;
    prepared.collection = Some(collection.clone());
    Ok(collection)
}

fn manifest(prepared: &Prepared) -> Value {
    let input = &prepared.input;
    let focus = prepared.focus.trim();
    json!({
        "repository":input.root,
        "project_id":input.project_id,
        "project_name":input.project_name,
        "knowledge_directory":input.path,
        "source_documents":input.documents,
        "source_origins":input.origins,
        "historical_source_origins":input.historical_origins,
        "worktrees":input.worktrees,
        "collection":prepared.collection,
        "previous_report":input.previous_report.as_ref().map(|_| if input.path.join(&input.report_path).is_file() { input.report_path.clone() } else if input.path.join(location::legacy_wiki_index(&input.project_id)).is_file() {location::legacy_wiki_index(&input.project_id)} else {PathBuf::from("docs/organized").join(format!("{}.md",input.project_id))}),
        "previous_documents":input.previous_documents,
        "excluded_documents":input.excluded_documents,
        "focus":if focus.is_empty() { DEFAULT_FOCUS } else { focus },
        "generated_at":crate::ids::now_iso(),
        "schema":"ailoom-repository-wiki/v1",
    })
}

#[cfg(test)]
fn prompt(prepared: &Prepared) -> String {
    let manifest = manifest(prepared);
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
                .arg(&prepared.input.path)
                .arg("--add-dir")
                .arg(output.parent().unwrap());
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
    if prepared.runner.id == "claude" {
        for source in prepared.worktrees() {
            command.arg("--add-dir").arg(&source.path);
        }
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
        return Err(fail(
            "整理输出超过知识库 128 MB 存储容量；可保留的正文已留在草稿中",
        ));
    }
    String::from_utf8(bytes).map_err(|_| fail("AI CLI 返回了非 UTF-8 文本"))
}

/// Drain continuously so verbose tools never block or get killed for producing logs.
fn capture(
    mut reader: impl Read + Send + 'static,
    report_path: Option<PathBuf>,
    count: Arc<AtomicU64>,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut report = report_path.map(std::fs::File::create).transpose()?;
        let mut saved = 0;
        let mut tail = Vec::new();
        let mut buffer = [0; 16 * 1024];
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count.fetch_add(n as u64, Ordering::Relaxed);
            if let Some(report) = report.as_mut() {
                let bytes = n.min(OUTPUT_LIMIT.saturating_sub(saved) as usize);
                report.write_all(&buffer[..bytes])?;
                saved += bytes as u64;
            }
            tail.extend_from_slice(&buffer[..n]);
            if tail.len() > DIAGNOSTIC_TAIL {
                tail.drain(..tail.len() - DIAGNOSTIC_TAIL);
            }
        }
        Ok(tail)
    })
}

fn retain_candidate(path: &Path, retain: &impl Fn(&str)) {
    if let Ok(file) = std::fs::File::open(path) {
        let mut bytes = Vec::new();
        if file.take(OUTPUT_LIMIT).read_to_end(&mut bytes).is_ok() && !bytes.is_empty() {
            retain(&String::from_utf8_lossy(&bytes));
        }
    }
}

pub fn generate(
    prepared: &Prepared,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    retain: impl Fn(&str),
) -> Result<String> {
    generate_draft(
        prepared,
        cancelled,
        progress,
        retain,
        prepared.started,
        DraftTask::Compile,
    )
}

pub fn generate_planned(
    prepared: &Prepared,
    resume: Option<Value>,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    retain: impl Fn(&Checkpoint),
) -> Result<String> {
    workflow::run(prepared, resume, cancelled, progress, retain)
}

enum DraftTask<'a> {
    Compile,
    Repair(&'a str, &'a Error),
    Stage(&'a str, &'a Value),
    Review(&'a str, Option<&'a Value>),
}

fn generate_draft(
    prepared: &Prepared,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    retain: impl Fn(&str),
    started: Instant,
    draft_task: DraftTask<'_>,
) -> Result<String> {
    if cancelled() {
        return Err(fail("整理已取消，原有知识和整理结果保留"));
    }
    let work = tempfile::tempdir()?;
    let output_path = work.path().join("report.md");
    let control = work.path().join("run-control.json");
    let stage_started = Instant::now();
    let target = Duration::from_secs(if matches!(draft_task, DraftTask::Stage("plan", _)) {
        5 * 60
    } else {
        runtime::BATCH_MINUTES * 60
    });
    if started.elapsed() >= runtime::MAXIMUM {
        return Err(runtime::paused());
    }
    runtime::write_control(&control, started, stage_started, target)?;
    // Prepare bounded input before launching any tool; all detail remains in task files.
    let prompt = match &draft_task {
        DraftTask::Repair(report, issue) => task::repair(prepared, work.path(), report, issue)?,
        DraftTask::Stage(phase, context) => task::stage(prepared, work.path(), phase, context)?,
        DraftTask::Compile => task::prepare(prepared, work.path())?,
        DraftTask::Review(report, feedback) => {
            task::review(prepared, work.path(), report, *feedback)?
        }
    };
    let prompt = task::with_runtime(prompt, &control, started)?;
    let mut child = command(prepared, &output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(format!("无法启动 {}：{e}", prepared.runner.name)))?;
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(prompt.as_bytes()));
    let stdout_count = Arc::new(AtomicU64::new(0));
    let stderr_count = Arc::new(AtomicU64::new(0));
    let stdout = capture(
        child.stdout.take().unwrap(),
        (prepared.runner.id != "codex").then(|| output_path.clone()),
        Arc::clone(&stdout_count),
    );
    let stderr = capture(
        child.stderr.take().unwrap(),
        None,
        Arc::clone(&stderr_count),
    );
    let mut last_note = Duration::ZERO;
    let mut last_activity = started.elapsed();
    let mut observed = 0;
    let mut retained_size = 0;
    let mut exited = None;
    let phase_label = match &draft_task {
        DraftTask::Stage("plan", _) => "制定整理计划".into(),
        DraftTask::Stage("batch", context) => format!(
            "编纂本批：{}",
            context["next_batch"]["title"]
                .as_str()
                .unwrap_or("模块知识")
        ),
        DraftTask::Stage("synthesize", _) => "汇总阶段稿并核对跨模块关系".into(),
        DraftTask::Repair(..) => "核对诊断并修订整理稿".into(),
        DraftTask::Review(..) => "独立评估资料处理、知识保留与代码依据".into(),
        _ => "阅读资料并核对代码".into(),
    };
    progress(&phase_label);
    let mut wrap_request = "";
    let outcome = loop {
        if cancelled() {
            terminate(&mut child);
            break Err(fail("整理已取消，已有正文保留为未完成草稿"));
        }
        let elapsed = started.elapsed();
        let requested = if elapsed >= runtime::WRAP_UP {
            "round"
        } else if stage_started.elapsed() >= target {
            "stage"
        } else {
            ""
        };
        if !requested.is_empty() && requested != wrap_request {
            if let Err(error) = runtime::write_control(&control, started, stage_started, target) {
                terminate(&mut child);
                break Err(error);
            }
            progress(if requested == "round" {
                "已进入收尾预算：已请求 AI 停止扩大阅读范围，返回阶段稿、未完成范围与下一步；已有批次持续保留"
            } else {
                "本阶段已到预估时间，已请求 AI 返回阶段稿及未完成范围；继续等待收尾，不中途丢弃成果"
            });
            wrap_request = requested;
        }
        let report_size = output_path.metadata().map(|m| m.len()).unwrap_or(0);
        let bytes = stdout_count.load(Ordering::Relaxed)
            + stderr_count.load(Ordering::Relaxed)
            + if prepared.runner.id == "codex" {
                report_size
            } else {
                0
            };
        if bytes != observed {
            last_activity = elapsed;
            observed = bytes;
        }
        if elapsed.saturating_sub(last_note) >= PROGRESS_INTERVAL {
            if let Err(error) = runtime::write_control(&control, started, stage_started, target) {
                terminate(&mut child);
                break Err(error);
            }
            progress(&format!(
                "{}；本轮已运行 {} 分 {} 秒；距最近输出 {} 秒，{}，可随时取消",
                phase_label,
                elapsed.as_secs() / 60,
                elapsed.as_secs() % 60,
                elapsed.saturating_sub(last_activity).as_secs(),
                if elapsed.saturating_sub(last_activity) >= Duration::from_secs(15 * 60) {
                    "长时间未返回输出，请关注工具状态；任务继续"
                } else if !wrap_request.is_empty() {
                    "正在收尾，已有阶段稿保留；任务继续"
                } else {
                    "任务继续"
                }
            ));
            last_note = elapsed;
            if report_size != retained_size {
                retain_candidate(&output_path, &retain);
                retained_size = report_size;
            }
        }
        if exited.is_none() {
            match child.try_wait() {
                Ok(status) => exited = status,
                Err(e) => {
                    terminate(&mut child);
                    break Err(e.into());
                }
            }
        }
        // Descendants may still hold a pipe after the launcher exits; keep cancellation live.
        if exited.is_some() && stdout.is_finished() && stderr.is_finished() && writer.is_finished()
        {
            break Ok(exited.take().unwrap());
        }
        if elapsed >= runtime::MAXIMUM {
            // The agent knew the budget from launch and received a wrap-up request 30 minutes ago.
            // A stuck tool cannot hold the task forever; returned stage results remain in the ledger.
            retain_candidate(&output_path, &retain);
            progress("收尾窗口已结束，工具仍未退出；停止本次进程并保留阶段稿，可继续整理");
            terminate(&mut child);
            break Err(runtime::paused());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let written = writer
        .join()
        .map_err(|_| fail("无法向 AI CLI 传入整理任务"))?;
    let stdout_result = stdout.join();
    let stderr_result = stderr.join();
    retain_candidate(&output_path, &retain);
    let status = outcome?;
    stdout_result.map_err(|_| fail("无法收集 AI CLI 正文"))??;
    let diagnostic = stderr_result.map_err(|_| fail("无法收集 AI CLI 诊断"))??;
    if !status.success() {
        // 不持久化模型的逐步工具日志；失败诊断只保留限长文本并隐藏 URL 内嵌凭据。
        let first = if stderr_count.load(Ordering::Relaxed) > DIAGNOSTIC_TAIL as u64 {
            diagnostic
                .iter()
                .position(|b| *b == b'\n')
                .map_or(diagnostic.len(), |i| i + 1)
        } else {
            0
        };
        let diagnostic =
            crate::gitx::redact_credentials(&String::from_utf8_lossy(&diagnostic[first..]));
        return Err(cli_failure(&prepared.runner.name, status, &diagnostic));
    }
    written.map_err(|e| fail(format!("AI CLI 没有完整读取整理任务：{e}")))?;
    if prepared.runner.id != "codex" && stdout_count.load(Ordering::Relaxed) > OUTPUT_LIMIT {
        return Err(fail(
            "整理输出超过知识库 128 MB 存储容量；可保留的正文已留在草稿中",
        ));
    }
    let report = read_output(&output_path)?;
    if report.trim().is_empty() {
        return Err(fail("AI CLI 未返回整理文档，原有知识保持不变"));
    }
    Ok(report)
}

fn cli_failure(name: &str, status: std::process::ExitStatus, diagnostic: &str) -> Error {
    let lower = diagnostic.to_lowercase();
    let hint = if [
        "input_too_large",
        "input exceeds the maximum length",
        "context_length_exceeded",
        "maximum context length",
        "prompt is too long",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        "整理输入超过工具限制，属于任务输入问题；原有知识与本次原文保留"
    } else if [
        "not logged in",
        "login required",
        "authentication required",
        "unauthorized",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        "工具认证失败，请在终端确认该工具已登录后重试；原有知识保留"
    } else {
        "请查看工具返回的具体错误；原有知识与本次原文保留"
    };
    let detail: String = diagnostic
        .chars()
        .rev()
        .take(1500)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    fail(format!("{name} 整理失败（{status}）：{detail}。{hint}"))
}

pub fn publish(data: &Path, prepared: &Prepared, report: &str) -> Result<PathBuf> {
    if report.trim().is_empty() || report.len() as u64 > OUTPUT_LIMIT {
        return Err(fail(
            "整理文档必须是非空 Markdown，且不超过知识库 128 MB 存储容量",
        ));
    }
    location::publish_organization(data, &prepared.input, report)
}

#[derive(Debug, Serialize)]
pub struct Completion {
    pub saved: bool,
    pub report_path: Option<PathBuf>,
    pub needs_revision: bool,
    pub needs_continuation: bool,
    pub warning: Option<String>,
    pub repair_attempts: usize,
    pub assessment_status: String,
}

/// A fresh evidence review gates publication; structural checks alone never approve content.
pub fn complete(
    data: &Path,
    prepared: &Prepared,
    report: String,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    retain: impl Fn(&str, Option<&Error>, bool),
) -> Result<Completion> {
    review::complete(data, prepared, report, cancelled, progress, retain, |_| {})
}

pub fn complete_with_assessment(
    data: &Path,
    prepared: &Prepared,
    report: String,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(&str),
    retain: impl Fn(&str, Option<&Error>, bool),
    assessed: impl Fn(&Assessment),
) -> Result<Completion> {
    review::complete(
        data, prepared, report, cancelled, progress, retain, assessed,
    )
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
            collection: None,
            started: Instant::now(),
            execution_record: None,
        }
    }

    fn note_path(p: &Prepared) -> PathBuf {
        if p.input.path.join(location::layout::HIDDEN_MARKER).is_file() {
            "sources/notes/migration.md".into()
        } else {
            Path::new("projects")
                .join(&p.input.project_id)
                .join("learnings/migration.md")
        }
    }

    fn test_report(body: &str) -> String {
        format!("# 仓库开发知识\n\n## index\n\n[迁移](#topic-migration)\n\n## overview\n\n测试资料范围。\n\n## topics\n\n### topic-migration\n\n主题：数据库迁移。\n\n{body}\n\n## maintenance\n\n参见[迁移](#topic-migration)。\n\n## questions\n\n无。\n\n## sources\n\n测试来源。\n\n## changes\n\n首次编纂。\n\n## validation\n\n尚未经独立评估。\n")
    }

    fn historical_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Prepared, PathBuf) {
        let (tmp, root, data) = fixture();
        std::fs::write(root.join("decision.md"), "# Decision\nold-quota-constraint").unwrap();
        let mut p = prepared(&data, &root);
        collect_sources(&data, &mut p, || false, |_| {}).unwrap();
        let old = p.input.origins.keys().next().unwrap().clone();
        std::fs::write(root.join("decision.md"), "# Decision\nquota lifted").unwrap();
        collect_sources(&data, &mut p, || false, |_| {}).unwrap();
        (tmp, root, data, p, old)
    }

    #[test]
    fn historical_originals_can_be_cited_without_becoming_current_candidates() {
        let (_tmp, root, data, p, old) = historical_fixture();
        assert_eq!(p.source_count(), 2);
        assert!(!p.input.documents.contains(&old));
        assert!(!p.input.excluded_documents.contains(&old));
        assert_eq!(
            p.input.historical_origins[&old][0].document,
            Path::new("decision.md")
        );
        let work = tempfile::tempdir().unwrap();
        let startup = task::prepare(&p, work.path()).unwrap();
        assert!(startup.len() < 64 * 1024);
        let entry: Value =
            serde_json::from_str(startup.rsplit("## 本次任务入口\n\n").next().unwrap()).unwrap();
        let task: Value =
            serde_json::from_slice(&std::fs::read(entry["task_index"].as_str().unwrap()).unwrap())
                .unwrap();
        let rows: Vec<Value> = task["historical_source_origins"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|file| {
                serde_json::from_slice::<Vec<Value>>(
                    &std::fs::read(file.as_str().unwrap()).unwrap(),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["document"], json!(old));
        assert_eq!(rows[0]["origin"]["document"], "decision.md");
        // The failed real draft linked an old snapshot only to disclose that it was not read/adopted.
        let report = test_report("Current module knowledge").replace(
            "测试来源。",
            &format!(
                "[旧快照](../../{})仍被旧来源页引用；本轮未核正文，不纳当前证据。",
                old.display()
            ),
        );
        publish(&data, &p, &report).unwrap();
        let sources = std::fs::read_to_string(p.input.path.join("wiki/sources.md")).unwrap();
        assert!(sources.contains(&format!("[旧快照](../{})", old.display())));
        assert!(sources.contains("本轮未核正文，不纳当前证据"));
        assert_eq!(prepared(&data, &root).source_count(), 2);
        // Topic citations to historical evidence are also valid, with historical scope retained.
        let p = prepared(&data, &root);
        publish(
            &data,
            &p,
            &test_report(&format!(
                "历史约束，尚未核对当前适用性：[原文](../../{})",
                old.display()
            )),
        )
        .unwrap();
        assert!(
            std::fs::read_to_string(p.input.path.join("wiki/topic-migration.md"))
                .unwrap()
                .contains("历史约束，尚未核对当前适用性")
        );
    }

    #[test]
    fn historical_citations_check_integrity_and_preserve_existing_pages_on_failure() {
        let (_tmp, root, data, p, old) = historical_fixture();
        let snapshot = p.input.path.join(&old);
        let original = std::fs::read(&snapshot).unwrap();
        let home = std::fs::read(p.input.path.join("README.md")).unwrap();
        let report = test_report(&format!("[历史](../../{})", old.display()));
        std::fs::write(&snapshot, "modified after generation started").unwrap();
        assert!(publish(&data, &p, &report)
            .unwrap_err()
            .message
            .contains("内容已变化"));
        let p = prepared(&data, &root);
        let error = publish(&data, &p, &report).unwrap_err();
        assert!(error.message.contains("历史原文内容与采集记录不符"));
        assert!(error.message.contains(old.to_str().unwrap()));
        assert_eq!(std::fs::read(p.input.path.join("README.md")).unwrap(), home);
        // Unused broken history does not prevent organizing intact current evidence.
        publish(&data, &p, &test_report("intact current evidence")).unwrap();
        let home = std::fs::read(p.input.path.join("README.md")).unwrap();
        std::fs::remove_file(&snapshot).unwrap();
        let p = prepared(&data, &root);
        let error = publish(&data, &p, &report).unwrap_err();
        assert!(error.message.contains("已登记的原文缺失"));
        assert!(error.message.contains(old.to_str().unwrap()));
        assert_eq!(std::fs::read(p.input.path.join("README.md")).unwrap(), home);
        let topic = std::fs::read(p.input.path.join("wiki/topic-migration.md")).unwrap();
        std::fs::write(&snapshot, original).unwrap();
        let p = prepared(&data, &root);
        let ctx = location::maintenance_context(&data, &root)
            .unwrap()
            .unwrap();
        super::super::feedback::archive(&ctx, old.to_str().unwrap()).unwrap();
        assert!(publish(&data, &p, &report)
            .unwrap_err()
            .message
            .contains("内容已变化"));
        let p = prepared(&data, &root);
        assert!(!p.input.historical_origins.contains_key(&old));
        let error = publish(&data, &p, &report).unwrap_err();
        assert!(
            error.message.contains("停用状态") && error.message.contains(old.to_str().unwrap())
        );
        assert_eq!(
            std::fs::read(p.input.path.join("wiki/topic-migration.md")).unwrap(),
            topic
        );
    }

    #[test]
    fn unresolved_source_links_identify_the_file_and_reason() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        std::fs::create_dir_all(p.input.path.join("resources")).unwrap();
        std::fs::write(
            p.input.path.join("resources/generated.md"),
            "not original evidence",
        )
        .unwrap();
        let unregistered = format!(
            "sources/{}.md",
            crate::ids::sha256_hex(b"unregistered original")
        );
        std::fs::write(p.input.path.join(&unregistered), "unregistered original").unwrap();
        let p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        for (path, reason) in [
            ("sources/missing.md", "文件不存在"),
            ("resources/generated.md", "文件存在，但未登记"),
            ("projects/another/learnings/private.md", "未登记为本项目"),
            ("%2e%2e/other-project/sources/note.md", "路径无效或越界"),
            (unregistered.as_str(), "文件存在，但未登记"),
        ] {
            let error =
                publish(&data, &p, &test_report(&format!("[来源](../../{path})"))).unwrap_err();
            assert!(error.message.contains(reason), "{}", error.message);
            assert!(error.message.contains(path), "{}", error.message);
            assert_eq!(
                std::fs::read(p.input.path.join("README.md")).unwrap(),
                before
            );
        }
    }

    #[test]
    fn editorial_format_variations_preserve_content_without_arbitrary_page_limits() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let large = "有效模块知识。".repeat(6000);
        let report = test_report(&format!(
            "{large}\n\n[上游 issue](https://github.com/example/project/issues/42#comment)"
        ))
        .replace("## topics\n", "## TOPICS\n\n主题之前的导读应保留。\n")
        .replace(
            "## maintenance\n",
            "## maintenance\n\n建议一。\n\n## 具体背景\n\n背景应保留。\n\n## maintenance\n",
        )
        .replace(
            "[迁移](#topic-migration)\n\n## overview",
            &format!("{}\n\n## overview", "简短入口说明。".repeat(3000)),
        );
        publish(&data, &p, &report).unwrap();
        let home = std::fs::read_to_string(p.input.path.join("README.md")).unwrap();
        assert!(home.contains("主题之前的导读应保留"));
        assert!(home.len() > 24 * 1024);
        let topic = std::fs::read_to_string(p.input.path.join("wiki/topic-migration.md")).unwrap();
        assert!(topic.contains(&large) && topic.len() > 32 * 1024);
        assert!(
            topic.contains("[上游 issue](https://github.com/example/project/issues/42#comment)")
        );
        let maintenance =
            std::fs::read_to_string(p.input.path.join("wiki/maintenance.md")).unwrap();
        assert!(
            maintenance.contains("建议一")
                && maintenance.contains("背景应保留")
                && maintenance.contains("参见")
        );
        let p = prepared(&data, &root);
        let mut report = "## topics\n\n".to_string();
        for i in 0..257 {
            report.push_str(&format!(
                "### topic-module-{i}\n\n主题：模块 {i}。\n来源记录与适用边界。\n\n"
            ));
        }
        report.push_str("## sources\n\n来源由人工后续复核。\n");
        publish(&data, &p, &report).unwrap();
        let home = std::fs::read_to_string(p.input.path.join("README.md")).unwrap();
        assert!(home.contains("topic-module-256.md") && home.contains("尚未经独立评估"));
    }

    #[cfg(unix)]
    #[test]
    fn editorial_issues_are_repaired_by_the_agent_with_bounded_input_and_retained_drafts() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let corrected = test_report("Important recovered knowledge，来源记录，待核实。");
        let body = format!(
            r#"import sys, pathlib, json
text = sys.stdin.read()
assert len(text.encode()) <= 65536
if '## 本轮独立评估入口' in text:
    args=sys.argv[1:]
    pathlib.Path(args[args.index('--output-last-message')+1]).write_text('# 开发知识评估\n\n结论：通过\n关键知识和缺口已核对。\n<!-- ailoom-review: pass -->',encoding='utf-8')
    sys.exit(0)
entry = json.loads(text.split('## 本次任务入口\n\n',1)[1].split('\n## 核对并修订已有整理稿',1)[0])
files = json.loads(text.strip().splitlines()[-1])
draft = pathlib.Path(files['draft']).read_text()
assert len(draft) > 1048576 and 'Important recovered knowledge' in draft
issue = json.loads(pathlib.Path(files['diagnostic']).read_text())
assert issue['context']['kind'] == 'draft_issue'
assert '迁移后核对行数' in (pathlib.Path(entry['knowledge_directory'])/'sources/notes/migration.md').read_text()
assert '不能为通过校验伪造路径' in text
args = sys.argv[1:]
assert args[:6] == ['--no-daemon','-a','never','exec','--sandbox','read-only']
pathlib.Path(args[args.index('--output-last-message')+1]).write_text({corrected:?},encoding='utf-8')
"#
        );
        let (_script, executable) = fake_cli(&body);
        p.runner.executable = Some(executable);
        let drafts = std::cell::RefCell::new(Vec::new());
        let report = "Important recovered knowledge\n".repeat(40000);
        let result = complete(
            &data,
            &p,
            report.clone(),
            || false,
            |_| {},
            |text, issue, _partial| {
                drafts
                    .borrow_mut()
                    .push((text.to_string(), issue.map(|e| e.message.clone())));
            },
        )
        .unwrap();
        assert!(result.saved && !result.needs_revision);
        assert_eq!(result.repair_attempts, 1);
        assert!(drafts
            .borrow()
            .iter()
            .any(|(text, issue)| text == &report && issue.is_some()));
        assert_eq!(drafts.borrow().last().unwrap().0, corrected);
        assert!(
            std::fs::read_to_string(p.input.path.join("wiki/topic-migration.md"))
                .unwrap()
                .contains("Important recovered knowledge")
        );
    }

    #[cfg(unix)]
    #[test]
    fn unsuccessful_repairs_keep_a_reviewable_draft_without_replacing_the_wiki() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        let (_script, executable) = fake_cli(
            r#"import sys, pathlib
sys.stdin.read()
args = sys.argv[1:]
pathlib.Path(args[args.index('--output-last-message')+1]).write_text('# 仍需补齐结构\n保留重要经验。')
"#,
        );
        p.runner.executable = Some(executable);
        let diagnosed = std::cell::Cell::new(0);
        let result = complete(
            &data,
            &p,
            "# 初稿\n保留重要经验。".into(),
            || false,
            |_| {},
            |_, issue, _partial| {
                if issue.is_some() {
                    diagnosed.set(diagnosed.get() + 1);
                }
            },
        )
        .unwrap();
        assert!(!result.saved && result.needs_revision);
        assert_eq!(result.repair_attempts, 2);
        assert_eq!(diagnosed.get(), 3);
        assert!(result.warning.unwrap().contains("草稿及诊断已保留"));
        assert_eq!(
            std::fs::read(p.input.path.join("README.md")).unwrap(),
            before
        );
        let (_script, executable) = fake_cli("import sys\nsys.stdin.read()\nsys.stderr.write('tool temporarily unavailable')\nsys.exit(1)");
        p.runner.executable = Some(executable);
        let result = complete(
            &data,
            &p,
            "# 已有草稿".into(),
            || false,
            |_| {},
            |_, _, _| {},
        )
        .unwrap();
        assert!(result.needs_revision && !result.saved);
        assert!(result
            .warning
            .unwrap()
            .contains("tool temporarily unavailable"));
        assert_eq!(
            std::fs::read(p.input.path.join("README.md")).unwrap(),
            before
        );
    }

    #[test]
    fn content_conflicts_and_cancellation_do_not_trigger_model_rewrites() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        std::fs::write(p.input.path.join(note_path(&p)), "人工修改必须保留").unwrap();
        // No CLI executable is configured: any attempt to rewrite would panic.
        let error = complete(
            &data,
            &p,
            test_report("旧草稿"),
            || false,
            |_| {},
            |_, _, _| {},
        )
        .unwrap_err();
        assert!(error.message.contains("内容已变化"));
        assert_ne!(error.context["kind"], "draft_issue");
        let p = prepared(&data, &root);
        let cancelled = std::cell::Cell::new(false);
        let retained = std::cell::Cell::new(false);
        let error = complete(
            &data,
            &p,
            "# 待修订草稿".into(),
            || cancelled.get(),
            |_| {},
            |_, issue, _partial| {
                retained.set(true);
                if issue.is_some() {
                    cancelled.set(true);
                }
            },
        )
        .unwrap_err();
        assert!(retained.get() && error.message.contains("取消"));
    }

    #[test]
    fn missing_knowledge_topics_keep_the_previous_homepage_and_explain_the_failure() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        for body in ["", "本轮暂无可发布的知识单元。"] {
            let report = test_report("unused");
            let (head, tail) = report.split_once("## topics\n").unwrap();
            let (_, rest) = tail.split_once("## maintenance\n").unwrap();
            let draft = format!("{head}## topics\n\n{body}\n\n## maintenance\n{rest}");
            let error = publish(&data, &p, &draft).unwrap_err();
            assert!(error.message.contains("AI 未生成知识主题"));
            assert_eq!(
                std::fs::read(p.input.path.join("README.md")).unwrap(),
                before
            );
        }
    }

    fn legacy_link(data: &Path, root: &Path, library: &Path) {
        let temporary = root.parent().unwrap().join(format!(
            "{}-old-library",
            root.file_name().unwrap().to_string_lossy()
        ));
        location::run(
            data,
            &location::Request {
                action: "init".into(),
                root: root.into(),
                path: Some(temporary),
                ..Default::default()
            },
        )
        .unwrap();
        // Recreate the old shared on-disk format, which remains readable until separated.
        if library.join(location::layout::HIDDEN_MARKER).is_file() {
            let mut marker: Value = serde_json::from_slice(
                &std::fs::read(library.join(location::layout::HIDDEN_MARKER)).unwrap(),
            )
            .unwrap();
            let owner = marker["project_id"].as_str().unwrap().to_string();
            marker.as_object_mut().unwrap().remove("project_id");
            std::fs::write(
                library.join("ailoom-knowledge.json"),
                serde_json::to_vec(&marker).unwrap(),
            )
            .unwrap();
            std::fs::remove_file(library.join(location::layout::HIDDEN_MARKER)).unwrap();
            let old = library.join("projects").join(owner);
            std::fs::create_dir_all(old.parent().unwrap()).unwrap();
            std::fs::rename(library.join(".ailoom/project"), &old).unwrap();
            if library.join("sources/notes").is_dir() {
                std::fs::rename(library.join("sources/notes"), old.join("learnings")).unwrap();
            }
            std::fs::remove_file(library.join("README.md")).unwrap();
        }
        let catalog = data.join("knowledge/bindings.json");
        let mut cat: Value = serde_json::from_slice(&std::fs::read(&catalog).unwrap()).unwrap();
        let marker: Value =
            serde_json::from_slice(&std::fs::read(library.join("ailoom-knowledge.json")).unwrap())
                .unwrap();
        for binding in cat["projects"].as_object_mut().unwrap().values_mut() {
            if Path::new(binding["project_root"].as_str().unwrap()) == root.canonicalize().unwrap()
            {
                binding["id"] = marker["id"].clone();
                binding["path"] = json!(library);
            }
        }
        std::fs::write(catalog, serde_json::to_vec_pretty(&cat).unwrap()).unwrap();
        let mut d = crate::knowledge::portable::read(root).unwrap().unwrap();
        d.knowledge_id = marker["id"].as_str().unwrap().into();
        crate::knowledge::portable::write(root, &d).unwrap();
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn automatic_collection_starts_empty_and_preserves_all_worktree_origins_and_revisions() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        let feature = tmp.path().join("feature");
        let data = tmp.path().join("data");
        let library = tmp.path().join("knowledge");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "--allow-empty", "-qm", "base"]);
        git(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                feature.to_str().unwrap(),
            ],
        );
        location::run(
            &data,
            &location::Request {
                action: "init".into(),
                root: root.clone(),
                path: Some(library.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        for worktree in [&root, &feature] {
            std::fs::create_dir_all(worktree.join("packages/service/docs")).unwrap();
            std::fs::write(
                worktree.join("packages/service/docs/lesson.MARKDOWN"),
                "# Shared constraint\nserial processing preserves ordering",
            )
            .unwrap();
        }
        std::fs::write(
            feature.join("why.rst"),
            "External quota requires serial delivery",
        )
        .unwrap();
        let mut p = prepared(&data, &root);
        assert_eq!(p.source_count(), 0, "no manual import is needed");
        let late = tmp.path().join("not-approved-after-queue");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "late",
                late.to_str().unwrap(),
            ],
        );
        std::fs::write(late.join("not-approved.md"), "# Outside queued scope").unwrap();
        let result = collect_sources(&data, &mut p, || false, |_| {}).unwrap();
        assert_eq!(result["worktree_count"], 2);
        assert_eq!(result["documents_read"], 3);
        assert_eq!(p.source_count(), 2, "identical bodies are reused");
        assert!(
            p.input.excluded_documents.is_empty(),
            "current manifest-owned snapshots must not also be excluded"
        );
        assert_eq!(
            p.worktrees().len(),
            2,
            "new worktrees do not expand queued authorization"
        );
        assert_eq!(p.input.origins.values().map(Vec::len).sum::<usize>(), 3);
        let work = tempfile::tempdir().unwrap();
        let text = task::prepare(&p, work.path()).unwrap();
        let entry: Value =
            serde_json::from_str(text.rsplit("## 本次任务入口\n\n").next().unwrap()).unwrap();
        let lookup: Value =
            serde_json::from_slice(&std::fs::read(entry["task_index"].as_str().unwrap()).unwrap())
                .unwrap();
        let directories: Vec<Value> = lookup["worktrees"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|file| {
                serde_json::from_slice::<Vec<Value>>(
                    &std::fs::read(file.as_str().unwrap()).unwrap(),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(directories.len(), 2);
        assert!(!directories
            .iter()
            .any(|source| source["path"] == json!(late)));
        let task: Value =
            serde_json::from_str(prompt(&p).rsplit("## 本次任务清单\n\n").next().unwrap()).unwrap();
        assert_eq!(task["collection"]["worktree_count"], 2);
        assert!(task["source_origins"]
            .as_object()
            .unwrap()
            .values()
            .any(|origins| origins
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["branch"] == "feature"
                    && o["document"] == "why.rst"
                    && o["worktree_dirty"] == true)));
        let revisions = || {
            location::ingest::status(&data, &root).unwrap()["revision_count"]
                .as_u64()
                .unwrap()
        };
        assert_eq!(revisions(), 3);
        collect_sources(&data, &mut p, || false, |_| {}).unwrap();
        assert_eq!(
            revisions(),
            3,
            "repeated organization does not duplicate source history"
        );
        std::fs::write(
            feature.join("why.rst"),
            "Quota lifted; ordering still requires serial delivery",
        )
        .unwrap();
        collect_sources(&data, &mut p, || false, |_| {}).unwrap();
        assert_eq!(revisions(), 4);
        assert_eq!(
            p.source_count(),
            2,
            "only current revisions are normal input"
        );
        assert!(p
            .input
            .documents
            .iter()
            .all(|path| !p.input.excluded_documents.contains(path)));
        assert!(
            p.input.excluded_documents.is_empty(),
            "recorded historical snapshots are not ownership-unknown documents"
        );
        let copies = walkdir::WalkDir::new(library.join("sources"))
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|e| {
                e.path()
                    .file_stem()
                    .is_some_and(|s| s.to_string_lossy().len() == 64)
            })
            .count();
        assert_eq!(copies, 3, "old source revisions stay available");
        std::fs::rename(&feature, tmp.path().join("retired")).unwrap();
        collect_sources(&data, &mut p, || false, |_| {}).unwrap();
        assert_eq!(
            p.source_count(),
            2,
            "removed worktree evidence remains available"
        );
        publish(&data, &p, &test_report("Collected development knowledge")).unwrap();
    }

    #[test]
    fn automatic_collection_reports_unreadable_candidates_and_cancellation() {
        let (_tmp, root, data) = fixture();
        std::fs::write(root.join("unreadable.md"), [0xff, 0xfe]).unwrap();
        std::fs::write(
            root.join("design.mdx"),
            "# Module contract\nPreserve message ordering",
        )
        .unwrap();
        std::fs::write(
            root.join("decision.adoc"),
            "= Decision\nQuota restricts concurrency",
        )
        .unwrap();
        let mut p = prepared(&data, &root);
        assert!(collect_sources(&data, &mut p, || true, |_| {})
            .unwrap_err()
            .message
            .contains("取消"));
        assert!(!p.input.path.join("imports").exists());
        let result = collect_sources(&data, &mut p, || false, |_| {}).unwrap();
        assert_eq!(p.source_count(), 3);
        assert_eq!(result["skipped"][0]["path"], "unreadable.md");
        assert!(prompt(&p).contains("不是 UTF-8 文本"));
        assert!(prompt(&p).contains("识别资料是你的工作"));
        assert!(prompt(&p).contains("无关或缺少独立证据的材料只在 sources 中注明处理原因"));
    }

    #[cfg(unix)]
    #[test]
    fn empty_collection_still_allows_local_markdown_discovery_and_keeps_diagnostics() {
        let (_tmp, root, data) = fixture();
        let ctx = location::maintenance_context(&data, &root)
            .unwrap()
            .unwrap();
        let p = prepared(&data, &root);
        super::super::feedback::archive(&ctx, note_path(&p).to_str().unwrap()).unwrap();
        std::fs::write(root.join("unreadable.md"), [0xff, 0xfe]).unwrap();
        let mut p = prepared(&data, &root);
        let result = collect_sources(&data, &mut p, || false, |_| {}).unwrap();
        assert_eq!(p.source_count(), 0);
        assert_eq!(result["skipped"][0]["path"], "unreadable.md");
        assert!(
            p.collection.is_some(),
            "worker can persist collection before discovery"
        );
        // New local documents can be discovered even without an eligible saved snapshot.
        std::fs::create_dir_all(root.join("docs/module")).unwrap();
        std::fs::write(
            root.join("docs/module/decision.MD"),
            "# Local decision\nquota",
        )
        .unwrap();
        let (_script, executable) = fake_cli(
            r#"import sys, pathlib, json
text = sys.stdin.read()
entry = json.loads(text.rsplit('## 本次任务入口\n\n', 1)[1])
assert entry['discovery_mode'] == 'local-markdown'
assert entry['source_count'] == 0
root = pathlib.Path(entry['repository'])
files = [p for p in root.rglob('*') if p.is_file() and p.suffix.lower() == '.md']
assert any('Local decision' in p.read_text(errors='replace') for p in files)
task = json.loads(pathlib.Path(entry['task_index']).read_text())
skipped = [row for name in task['collection']['skipped']['files'] for row in json.loads(pathlib.Path(name).read_text())]
assert skipped[0]['path'] == 'unreadable.md'
args = sys.argv[1:]
pathlib.Path(args[args.index('--output-last-message') + 1]).write_text('# Discovered local document', encoding='utf-8')"#,
        );
        p.runner.executable = Some(executable);
        assert_eq!(
            generate(&p, || false, |_| {}, |_| {}).unwrap(),
            "# Discovered local document"
        );
    }

    #[test]
    fn sources_exclude_recovery_and_generated_reports_and_publish_is_searchable() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let original = std::fs::read(p.input.path.join(note_path(&p))).unwrap();
        for name in [
            ".ailoom/project/recovery/copy.md",
            "resources/snapshot/tool.md",
            "docs/organized/other.md",
        ] {
            let path = p.input.path.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "不应当作为原始知识").unwrap();
        }
        p = prepared(&data, &root);
        assert_eq!(p.source_count(), 1);
        let report = test_report(&format!(
            "运行数据库迁移后核对行数。来源：[迁移](../../{})",
            note_path(&p).display()
        ));
        let dest = publish(&data, &p, &report).unwrap();
        assert!(std::fs::read_to_string(&dest)
            .unwrap()
            .contains("topic-migration.md"));
        assert_eq!(
            std::fs::read(p.input.path.join(note_path(&p))).unwrap(),
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
        assert_eq!(next.input.previous_documents.len(), 5);
        assert!(next
            .input
            .previous_report
            .as_deref()
            .unwrap()
            .contains("可直接编辑"));
        assert!(
            location::recall(&data, &root, "数据库迁移", 20).unwrap()["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hit| hit["source_path"]
                    .as_str()
                    .unwrap()
                    .ends_with("topic-migration.md"))
        );
    }

    #[test]
    fn archived_notes_are_excluded_and_archiving_invalidates_a_running_snapshot() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let ctx = location::maintenance_context(&data, &root)
            .unwrap()
            .unwrap();
        super::super::feedback::archive(&ctx, note_path(&p).to_str().unwrap()).unwrap();
        assert_eq!(status(&data, &root).unwrap()["source_count"], 0);
        assert!(publish(&data, &p, "已过时的整理").is_err());
        assert!(p.input.path.join(note_path(&p)).is_file());
        super::super::feedback::restore(&ctx, note_path(&p).to_str().unwrap()).unwrap();
        assert_eq!(status(&data, &root).unwrap()["source_count"], 1);
    }

    #[test]
    fn shared_knowledge_keeps_each_projects_report_separate() {
        let (tmp, root, data) = fixture();
        let first = prepared(&data, &root);
        let other = tmp.path().join("other-project");
        std::fs::create_dir(&other).unwrap();
        legacy_link(&data, &other, &first.input.path);
        let second = prepared(&data, &other);
        assert_eq!(first.knowledge_id(), second.knowledge_id());
        let first = prepared(&data, &root);
        assert!(publish(&data, &first, &test_report("第一个项目维护建议")).is_err());
        assert!(publish(&data, &second, &test_report("第二个项目维护建议")).is_err());
        for (root, name) in [(&root, "a"), (&other, "b")] {
            let request = location::Request {
                action: "separate".into(),
                root: root.clone(),
                path: Some(tmp.path().join(name)),
                ..Default::default()
            };
            let preview = location::run(&data, &request).unwrap();
            location::run(
                &data,
                &location::Request {
                    execute: true,
                    expected: preview["expected"].as_str().map(String::from),
                    ..request
                },
            )
            .unwrap();
        }
        let first = prepared(&data, &root);
        let second = prepared(&data, &other);
        let a = publish(&data, &first, &test_report("第一个项目维护建议")).unwrap();
        let b = publish(&data, &second, &test_report("第二个项目维护建议")).unwrap();
        assert_ne!(a, b);
        assert_eq!(
            second.source_count(),
            0,
            "another project's notes are not inputs"
        );
        assert!(
            !location::recall(&data, &other, "第一个项目", 50).unwrap()["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["excerpt"].as_str().unwrap().contains("第一个项目"))
        );
    }

    #[test]
    fn project_notes_and_shared_documents_have_explicit_ownership() {
        let (tmp, root, data) = fixture();
        let first = prepared(&data, &root);
        let other = tmp.path().join("sdk");
        std::fs::create_dir(&other).unwrap();
        legacy_link(&data, &other, &first.input.path);
        let draft = tmp.path().join("sdk.md");
        std::fs::write(&draft, "# SDK-only\nSDK boundary").unwrap();
        location::save(&data, &other, &draft, "migration").unwrap();
        for (name, body) in [
            ("legacy.md", "# Ambiguous legacy"),
            ("shared.md", "---\nshared: true\n---\n# Shared rule"),
        ] {
            std::fs::write(first.input.path.join(name), body).unwrap();
        }
        let a = prepared(&data, &root);
        let b = prepared(&data, &other);
        assert_eq!(a.source_count(), 2);
        assert_eq!(b.source_count(), 2);
        assert!(a
            .input
            .excluded_documents
            .contains(&PathBuf::from("legacy.md")));
        assert!(!b.input.documents.contains(&note_path(&a)));
        let hits = location::recall(&data, &root, "SDK boundary", 20).unwrap();
        assert!(!hits["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["excerpt"].as_str().unwrap().contains("SDK-only")));
    }

    #[test]
    fn declared_dependencies_find_another_library_without_recalling_its_private_notes() {
        let (tmp, root, data) = fixture();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"app","dependencies":{"@example/sdk":"1.2.0"}}"#,
        )
        .unwrap();
        let sdk = tmp.path().join("sdk");
        std::fs::create_dir_all(sdk.join("packages/core")).unwrap();
        std::fs::write(
            sdk.join("package.json"),
            r#"{"name":"sdk","workspaces":["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(
            sdk.join("packages/core/package.json"),
            r#"{"name":"@example/sdk","version":"2.0.0"}"#,
        )
        .unwrap();
        location::run(
            &data,
            &location::Request {
                action: "init".into(),
                root: sdk.clone(),
                path: Some(tmp.path().join("sdk-knowledge")),
                ..Default::default()
            },
        )
        .unwrap();
        let draft = tmp.path().join("sdk.md");
        std::fs::write(&draft, "# sdk-private-boundary\nSDK-only rule").unwrap();
        location::save(&data, &sdk, &draft, "migration").unwrap();
        let app = prepared(&data, &root);
        let target = prepared(&data, &sdk);
        let found = location::locate(&data, "sdk", 5).unwrap();
        assert_eq!(found["status"], "found");
        assert_eq!(
            found["candidates"][0]["project_id"],
            target.input.project_id
        );
        assert_ne!(
            found["candidates"][0]["knowledge_id"],
            app.input.knowledge_id
        );
        let recalled = location::recall(&data, &root, "SDK-only", 50).unwrap();
        assert!(recalled.get("related_projects").is_none());
        assert!(!recalled["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["excerpt"].as_str().unwrap().contains("SDK-only")));
        publish(&data, &app, &test_report("App integration contract")).unwrap();
        let guide = app.input.path.join(".ailoom/project/README.md");
        let text = std::fs::read_to_string(guide).unwrap();
        assert!(text.contains("--action locate"));
        assert!(!text.contains(tmp.path().to_str().unwrap()));
    }

    #[test]
    fn stale_wiki_editions_are_not_recalled_and_invalid_drafts_do_not_replace_current_pages() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        publish(&data, &p, &test_report("obsolete-widget-specific-12345")).unwrap();
        let next = prepared(&data, &root);
        publish(
            &data,
            &next,
            &test_report("replacement-contract-specific-98765"),
        )
        .unwrap();
        let hits = location::recall(&data, &root, "obsolete-widget-specific-12345", 100).unwrap();
        assert!(!hits["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["excerpt"].as_str().unwrap().contains("obsolete-widget")));
        let current = prepared(&data, &root);
        let entry = std::fs::read(current.input.path.join(&current.input.report_path)).unwrap();
        assert!(publish(&data, &current, "# incomplete result").is_err());
        assert!(publish(
            &data,
            &current,
            &test_report("[foreign](../../projects/another/learnings/private.md)")
        )
        .is_err());
        assert_eq!(
            std::fs::read(current.input.path.join(&current.input.report_path)).unwrap(),
            entry
        );
    }

    #[test]
    fn people_and_ai_edit_the_same_markdown_and_manual_pages_survive_organization() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        publish(&data, &p, &test_report("generated-contract")).unwrap();
        assert_eq!(p.report_key(), Path::new("README.md"));
        let topic = p.input.path.join("wiki/topic-migration.md");
        let manual = p.input.path.join("wiki/index.md");
        std::fs::write(&manual, "# 人工记录\nhandwritten-unique-contract").unwrap();
        std::fs::write(
            p.input.path.join("wiki/维护 说明.md"),
            "# 维护说明\nhuman-text",
        )
        .unwrap();
        std::fs::write(&topic, "# 修改后的取舍\nmanual-decision-unique").unwrap();
        let hits = location::recall(&data, &root, "handwritten-unique-contract", 5).unwrap();
        assert!(hits["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "wiki/index.md"));
        let p = prepared(&data, &root);
        assert!(p
            .input
            .previous_documents
            .iter()
            .any(|p| p.path == Path::new("wiki/index.md")));
        let stale = std::fs::read(&topic).unwrap();
        std::fs::write(&topic, "# 再次人工更新\nlatest-human-contract").unwrap();
        assert!(publish(&data, &p, &test_report("stale AI result")).is_err());
        assert_ne!(std::fs::read(&topic).unwrap(), stale);
        let next = prepared(&data, &root);
        publish(
            &data,
            &next,
            &test_report("new module").replace("topic-migration", "topic-other"),
        )
        .unwrap();
        assert!(std::fs::read_to_string(&topic)
            .unwrap()
            .contains("latest-human-contract"));
        assert!(std::fs::read_to_string(&manual)
            .unwrap()
            .contains("handwritten-unique-contract"));
        let home = std::fs::read_to_string(p.input.path.join("README.md")).unwrap();
        assert!(home.contains("wiki/index.md") && home.contains("wiki/topic-migration.md"));
        assert!(home.contains("wiki/维护%20说明.md"));
    }

    #[test]
    fn interrupted_publication_recovers_but_never_overwrites_later_human_edits() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let home = p.input.path.join("README.md");
        let before = std::fs::read(&home).unwrap();
        let after = b"# partial AI publication";
        let journal = p.input.path.join(".ailoom/wiki-transaction.json");
        let record = json!({"before":{"README.md":before},"after":{"README.md":crate::ids::sha256_hex(after)}});
        std::fs::write(&journal, serde_json::to_vec(&record).unwrap()).unwrap();
        std::fs::write(&home, after).unwrap();
        prepared(&data, &root);
        assert_eq!(std::fs::read(&home).unwrap(), before);
        assert!(!journal.exists());
        std::fs::write(&journal, serde_json::to_vec(&record).unwrap()).unwrap();
        std::fs::write(&home, "# later human update").unwrap();
        assert!(location::organization_input(&data, &root).is_err());
        assert_eq!(
            std::fs::read_to_string(&home).unwrap(),
            "# later human update"
        );
        assert!(journal.exists());
    }

    #[test]
    fn a_partial_registration_does_not_unisolate_a_shared_library() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let foreign = p.input.path.join("projects/foreign-project");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("project.json"), "{}").unwrap();
        std::fs::write(p.input.path.join("legacy.md"), "# ambiguous-private-legacy").unwrap();
        let current = prepared(&data, &root);
        assert_eq!(current.source_count(), 1);
        assert_eq!(
            current.input.excluded_documents,
            vec![PathBuf::from("legacy.md")]
        );
        assert!(
            location::recall(&data, &root, "ambiguous-private-legacy", 10).unwrap()["results"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn unrelated_broken_declaration_does_not_block_recall_and_catalog_failure_does_not_commit() {
        let (tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let other = tmp.path().join("broken-sdk");
        std::fs::create_dir(&other).unwrap();
        location::run(
            &data,
            &location::Request {
                action: "init".into(),
                root: other.clone(),
                path: Some(tmp.path().join("other-knowledge")),
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::write(other.join(".ailoom/knowledge.json"), "invalid-json").unwrap();
        let current = prepared(&data, &root);
        assert_eq!(
            location::locate(&data, "broken-sdk", 5).unwrap()["warnings"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(location::recall(&data, &root, "迁移", 5).is_ok());
        std::fs::remove_file(p.input.path.join("sources/README.md")).unwrap();
        std::fs::create_dir_all(p.input.path.join("sources/README.md")).unwrap();
        assert!(publish(&data, &current, &test_report("must-not-commit")).is_err());
        assert!(p.input.path.join(&p.input.report_path).exists());
        assert!(prepared(&data, &root).input.previous_documents.is_empty());
    }

    #[test]
    fn changed_knowledge_or_existing_report_prevents_stale_publication() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let dest = p.input.path.join(&p.input.report_path);
        std::fs::write(p.input.path.join(note_path(&p)), "# 新知识\n人工更新").unwrap();
        assert!(publish(&data, &p, "不应覆盖")
            .unwrap_err()
            .message
            .contains("内容已变化"));
        assert!(dest.exists());
        let p = prepared(&data, &root);
        publish(&data, &p, &test_report("上次结果")).unwrap();
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
            assert_eq!(task["source_documents"], json!([note_path(&p)]));
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
        assert!(prompt.contains("sources/notes/migration.md"));
        assert!(prompt.contains("数据库维护"));
        assert!(!prompt.contains("resources/snapshot"));
    }

    #[cfg(unix)]
    fn fake_cli(body: &str) -> (tempfile::TempDir, PathBuf) {
        // 只把场景正文写为数据，用稳定夹具执行，避免并行测试即时写入并
        // 执行临时脚本时的 ETXTBSY；不修改 PATH 或用户安装的 CLI。
        let tmp =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let path = tmp.path().join("fake-cli");
        std::fs::write(path.with_extension("py"), body).unwrap();
        std::os::unix::fs::symlink(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/common/knowledge_cli.py"),
            &path,
        )
        .unwrap();
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
entry = __import__('json').loads(task.rsplit('## 本次任务入口\n\n', 1)[1])
assert entry['discovery_mode'] == 'local-markdown'
assert 'sources/notes/migration.md' not in task
files = [p for name in entry['source_directories'] for p in pathlib.Path(name).rglob('*.md')]
assert any('迁移后核对行数' in p.read_text() for p in files)
assert '数据库维护' in task
pathlib.Path(args[args.index('--output-last-message') + 1]).write_text('# 仓库维护知识与建议\n迁移后验证行数。', encoding='utf-8')
print('this is not the final report')"#,
        );
        p.runner.executable = Some(executable);
        let report = generate(&p, || false, |_| {}, |_| {}).unwrap();
        assert_eq!(report, "# 仓库维护知识与建议\n迁移后验证行数。");
        assert!(
            p.input.path.join(&p.input.report_path).is_file(),
            "生成过程保留初始化首页"
        );
        publish(&data, &p, &test_report(&report)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn multi_megabyte_metadata_uses_bounded_input_without_losing_sources_or_origins() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        p.input.documents.clear();
        for i in 0..2887 {
            let path = PathBuf::from(format!("sources/raw/{i:064}.md"));
            p.input.documents.push(path.clone());
            p.input.origins.insert(
                path,
                (0..4)
                    .map(|j| location::ingest::Origin {
                        worktree_id: format!("worktree-{j}"),
                        directory: format!("工作目录-{j}-{}", "sdk-feature-branch-".repeat(12)),
                        document: format!("packages/module-{i}/docs/开发决策-{j}.md").into(),
                        branch: Some(format!("feature/{j}")),
                        commit: Some("a".repeat(40)),
                        worktree_dirty: j != 0,
                    })
                    .collect(),
            );
        }
        p.input.previous_documents.push(
            serde_json::from_value(json!({
                "path":"wiki/manual-contract.md",
                "title":"人工维护的接口约定",
                "kind":"topic"
            }))
            .unwrap(),
        );
        p.input
            .excluded_documents
            .push("legacy/ambiguous.md".into());
        p.collection = Some(
            json!({"documents_read":11257,"worktree_count":4,"skipped":[{"path":"unreadable.mdx","reason":"不是 UTF-8 文本"}]}),
        );
        assert!(prompt(&p).chars().count() > 4_236_735);
        let (_script, executable) = fake_cli(
            r#"import sys, pathlib, json
text = sys.stdin.read()
assert len(text.encode('utf-8')) <= 65536
entry = json.loads(text.rsplit('## 本次任务入口\n\n', 1)[1])
assert entry['discovery_mode'] == 'local-markdown'
assert '00000000002886.md' not in text
task = json.loads(pathlib.Path(entry['task_index']).read_text())
def rows(field):
    result = []
    for name in field['files']:
        path = pathlib.Path(name)
        assert path.stat().st_size <= 48 * 1024
        result.extend(json.loads(path.read_text()))
    assert len(result) == field['count']
    return result
documents = rows(task['source_documents'])
assert len(documents) == 2887 and documents[-1].endswith('00000000002886.md')
origins = rows(task['source_origins'])
assert len(origins) == 2887 * 4
assert origins[-1]['document'] == documents[-1]
assert origins[-1]['origin']['document'] == 'packages/module-2886/docs/开发决策-3.md'
assert origins[-1]['origin']['branch'] == 'feature/3'
assert origins[-1]['origin']['worktree_dirty'] is True
assert rows(task['previous_documents'])[-1]['path'] == 'wiki/manual-contract.md'
assert rows(task['excluded_documents']) == ['legacy/ambiguous.md']
assert rows(task['collection']['skipped'])[0]['reason'] == '不是 UTF-8 文本'
assert task['collection']['documents_read'] == 11257
assert task['focus'] == '数据库维护'
assert '文档可能已经过时' in text
assert '不能用它的代码冒充来源分支的实现证据' in text
args = sys.argv[1:]
pathlib.Path(args[args.index('--output-last-message') + 1]).write_text('# synthetic complete', encoding='utf-8')"#,
        );
        p.runner.executable = Some(executable);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        assert_eq!(
            generate(&p, || false, |_| {}, |_| {}).unwrap(),
            "# synthetic complete"
        );
        assert_eq!(
            std::fs::read(p.input.path.join("README.md")).unwrap(),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn input_size_authentication_and_other_failures_have_distinct_guidance() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        for (diagnostic, expected, login) in [
            (
                "Error: turn/start: Input exceeds the maximum length of 1048576 characters. input_too_large actual_chars:4236735",
                "任务输入问题",
                false,
            ),
            ("not logged in", "工具认证失败", true),
            ("network connection timed out", "具体错误", false),
        ] {
            let (_script, executable) = fake_cli(&format!(
                "import sys\nsys.stdin.read()\nsys.stderr.write({diagnostic:?})\nsys.exit(1)"
            ));
            p.runner.executable = Some(executable);
            let error = generate(&p, || false, |_| {}, |_| {})
                .unwrap_err();
            assert!(error.message.contains(expected), "{}", error.message);
            assert_eq!(error.message.contains("登录"), login, "{}", error.message);
        }
    }

    #[cfg(unix)]
    #[test]
    fn large_cli_work_logs_do_not_use_the_final_report_limit() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let (_script, executable) = fake_cli(
            r#"import sys, pathlib
sys.stdin.read()
for _ in range(70):
    sys.stderr.write('x' * (1024 * 1024))
    sys.stdout.write('x' * (1024 * 1024))
sys.stderr.write('\nread completed\n')
args = sys.argv[1:]
pathlib.Path(args[args.index('--output-last-message') + 1]).write_text('# Completed', encoding='utf-8')"#,
        );
        p.runner.executable = Some(executable);
        assert_eq!(
            generate(&p, || false, |_| {}, |_| {}).unwrap(),
            "# Completed"
        );
        let (_script, executable) = fake_cli(
            r#"import sys
sys.stdin.read()
sys.stderr.write('x' * (5 * 1024 * 1024) + '\nactual failure\n')
sys.exit(3)"#,
        );
        p.runner.executable = Some(executable);
        let error = generate(&p, || false, |_| {}, |_| {}).unwrap_err();
        assert!(error.message.contains("actual failure"));
        assert!(error.message.len() < 2000);
        let (_script, executable) = fake_cli(
            r#"import sys, pathlib
sys.stdin.read()
args = sys.argv[1:]
pathlib.Path(args[args.index('--output-last-message') + 1]).write_text('x' * (5 * 1024 * 1024))"#,
        );
        p.runner.executable = Some(executable);
        assert_eq!(
            generate(&p, || false, |_| {}, |_| {}).unwrap().len(),
            5 * 1024 * 1024
        );
    }

    #[cfg(unix)]
    #[test]
    fn long_and_quiet_generation_and_repairs_continue_past_the_former_deadline() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let report = test_report("长任务的知识正文。");
        let (_script, executable) = fake_cli(&format!(
            r#"import sys, time, pathlib
sys.stdin.read()
time.sleep(0.6)
args = sys.argv[1:]
pathlib.Path(args[args.index('--output-last-message')+1]).write_text({report:?},encoding='utf-8')
"#
        ));
        p.runner.executable = Some(executable);
        let issue = fail("具体引用需要核对");
        for correction in [DraftTask::Compile, DraftTask::Repair("旧草稿", &issue)] {
            let notes = std::cell::RefCell::new(Vec::new());
            let drafts = std::cell::RefCell::new(Vec::new());
            // Advance the same production clock by 45 minutes without a 45-minute test.
            let started = Instant::now() - Duration::from_secs(45 * 60);
            let result = generate_draft(
                &p,
                || false,
                |note| notes.borrow_mut().push(note.to_string()),
                |draft| drafts.borrow_mut().push(draft.to_string()),
                started,
                correction,
            )
            .unwrap();
            assert_eq!(result, report);
            assert!(notes
                .borrow()
                .iter()
                .any(|note| note.contains("45 分") && note.contains("任务继续")));
            assert_eq!(drafts.borrow().last().unwrap(), &report);
        }
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_and_tool_failure_retain_received_body_without_publishing_it() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        for cancel in [true, false] {
            let (_script, executable) = fake_cli(&format!(
                r#"import sys, pathlib, time
sys.stdin.read()
args = sys.argv[1:]
pathlib.Path(args[args.index('--output-last-message')+1]).write_text('# 未完成正文\n已收到的知识必须保留。',encoding='utf-8')
time.sleep({})
sys.stderr.write('synthetic tool failure')
sys.exit(3)
"#,
                if cancel { 30.0 } else { 0.2 }
            ));
            p.runner.executable = Some(executable);
            let drafts = std::cell::RefCell::new(Vec::new());
            let started = Instant::now();
            let error = generate(
                &p,
                || cancel && started.elapsed() > Duration::from_millis(500),
                |_| {},
                |draft| drafts.borrow_mut().push(draft.to_string()),
            )
            .unwrap_err();
            assert!(error.message.contains(if cancel {
                "取消"
            } else {
                "synthetic tool failure"
            }));
            assert!(drafts
                .borrow()
                .last()
                .unwrap()
                .contains("已收到的知识必须保留"));
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(
                std::fs::read(p.input.path.join("README.md")).unwrap(),
                before
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_still_works_when_a_descendant_keeps_output_pipes_open() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let (_script, executable) = fake_cli("import sys, subprocess\nsys.stdin.read()\nsubprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\n");
        p.runner.executable = Some(executable);
        let started = Instant::now();
        let error = generate(
            &p,
            || started.elapsed() > Duration::from_millis(500),
            |_| {},
            |_| {},
        )
        .unwrap_err();
        assert!(error.message.contains("取消"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn text_stdout_runners_keep_received_body_on_success_and_failure() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        p.runner.id = "pi".into();
        for success in [true, false] {
            let (_script, executable) = fake_cli(&format!("import sys\nsys.stdin.read()\nsys.stdout.write('正文' * (2 * 1024 * 1024))\nsys.stdout.flush()\nsys.exit({})", if success {0} else {3}));
            p.runner.executable = Some(executable);
            let retained = std::cell::RefCell::new(String::new());
            let result = generate(
                &p,
                || false,
                |_| {},
                |draft| *retained.borrow_mut() = draft.to_string(),
            );
            assert!(retained.borrow().len() > 4 * 1024 * 1024);
            if success {
                assert_eq!(result.unwrap(), *retained.borrow());
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn cli_failure_empty_response_and_cancel_preserve_previous_knowledge() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let original = std::fs::read(p.input.path.join(note_path(&p))).unwrap();
        let (_script, executable) = fake_cli(
            "import sys\nsys.stdin.read()\nsys.stderr.write('not logged in')\nsys.exit(3)",
        );
        p.runner.executable = Some(executable);
        assert!(generate(&p, || false, |_| {}, |_| {})
            .unwrap_err()
            .message
            .contains("not logged in"));
        let (_script2, executable) = fake_cli("import sys, pathlib\nsys.stdin.read()\npathlib.Path(sys.argv[sys.argv.index('--output-last-message') + 1]).write_text('')");
        p.runner.executable = Some(executable);
        assert!(generate(&p, || false, |_| {}, |_| {})
            .unwrap_err()
            .message
            .contains("未返回"));
        let (_script3, executable) = fake_cli("import sys, time\nsys.stdin.read()\ntime.sleep(30)");
        p.runner.executable = Some(executable);
        let cancel_after = Instant::now();
        assert!(generate(
            &p,
            || cancel_after.elapsed() > Duration::from_millis(250),
            |_| {},
            |_| {}
        )
        .unwrap_err()
        .message
        .contains("已取消"));
        assert_eq!(
            std::fs::read(p.input.path.join(note_path(&p))).unwrap(),
            original
        );
        assert!(p.input.path.join(&p.input.report_path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn planned_batches_are_checkpointed_and_resume_without_repeating_returned_work() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        let trace = root.join("phases.txt");
        let report = test_report("汇总后的证据与知识。");
        let (_script, executable) = fake_cli(&format!(
            r#"import sys, pathlib, json, time, re
prompt=sys.stdin.read()
assert '总预算 120 分钟' in prompt and '收尾协议' in prompt
output=pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1])
context=json.loads((output.parent/'stage-context.json').read_text())
phase=context['phase']
index=len(context['checkpoint']['completed'])
with open({trace:?},'a') as log: log.write(phase+':'+str(index)+'\n')
if phase=='plan':
    value=json.dumps({{'batches':[{{'title':'模块甲','scope':'先核对模块甲代码与文档'}},{{'title':'模块乙','scope':'核对模块乙约束和历史'}}]}},ensure_ascii=False)
elif phase=='batch':
    value='# 模块'+str(index)+'\n实际证据与未完成范围。'
else:
    assert [pathlib.Path(row['path']).read_text() for row in context['checkpoint']['completed']]==['# 模块0\n实际证据与未完成范围。','# 模块1\n实际证据与未完成范围。']
    value={report:?}
output.write_text(value,encoding='utf-8')
"#,
            trace = trace.to_str().unwrap()
        ));
        p.runner.executable = Some(executable);
        let saved = std::cell::RefCell::new(None);
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let first = workflow::run_with_elapsed(
            &p,
            None,
            || false,
            |_| {},
            |cp| {
                let value = json!(cp);
                if value["completed"].as_array().unwrap().len() == 1 {
                    elapsed.set(runtime::WRAP_UP);
                }
                *saved.borrow_mut() = Some(value);
            },
            || elapsed.get(),
        )
        .unwrap_err();
        assert_eq!(first.context["kind"], "organization_budget");
        let checkpoint = saved.borrow().clone().unwrap();
        assert_eq!(checkpoint["completed"].as_array().unwrap().len(), 1);
        assert!(checkpoint["completed"][0]
            .as_str()
            .unwrap()
            .contains("实际证据"));
        assert_eq!(
            std::fs::read(p.input.path.join("README.md")).unwrap(),
            before
        );
        p.started = Instant::now();
        let result = generate_planned(
            &p,
            Some(checkpoint),
            || false,
            |_| {},
            |cp| *saved.borrow_mut() = Some(json!(cp)),
        )
        .unwrap();
        assert_eq!(result, report);
        assert_eq!(
            std::fs::read_to_string(trace).unwrap(),
            "plan:0\nbatch:0\nbatch:1\nsynthesize:2\n"
        );
        assert_eq!(
            saved.borrow().as_ref().unwrap()["completed"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            std::fs::read(p.input.path.join("README.md")).unwrap(),
            before,
            "stage results never publish themselves"
        );
    }

    #[cfg(unix)]
    #[test]
    fn wrap_up_is_told_to_the_agent_before_the_last_resort_budget_stop() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let (_script, executable) = fake_cli(
            r#"import sys, pathlib, json
prompt=sys.stdin.read()
output=pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1])
control=json.loads((output.parent/'run-control.json').read_text())
assert control['action']=='finish_current_stage'
assert control['remaining_seconds']>0
output.write_text('# 已按收尾要求返回\n未完成范围留待下一轮。',encoding='utf-8')
"#,
        );
        p.runner.executable = Some(executable);
        p.started = Instant::now() - runtime::WRAP_UP;
        let notes = std::cell::RefCell::new(Vec::new());
        assert!(generate(
            &p,
            || false,
            |n| notes.borrow_mut().push(n.to_string()),
            |_| {}
        )
        .unwrap()
        .contains("按收尾要求返回"));
        assert!(notes.borrow().iter().any(|n| n.contains("收尾预算")));

        let (_script, executable) = fake_cli(
            r#"import sys, pathlib, time
sys.stdin.read()
output=pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1])
output.write_text('# 已收到的本阶段正文',encoding='utf-8')
time.sleep(30)
"#,
        );
        p.runner.executable = Some(executable);
        p.started = Instant::now() - runtime::MAXIMUM + Duration::from_millis(1500);
        let retained = std::cell::RefCell::new(String::new());
        let now = Instant::now();
        let error =
            generate(&p, || false, |_| {}, |s| *retained.borrow_mut() = s.into()).unwrap_err();
        assert_eq!(error.context["kind"], "organization_budget");
        assert!(retained.borrow().contains("已收到"));
        assert!(now.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn planning_format_variations_keep_the_plan_and_other_project_checkpoints_are_rejected() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let (_script, executable) = fake_cli(
            r#"import sys, pathlib, json
sys.stdin.read()
output=pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1])
phase=json.loads((output.parent/'stage-context.json').read_text())['phase']
output.write_text('先核对业务规则，再核对历史取舍。' if phase=='plan' else '# 保留实际返回的知识',encoding='utf-8')
"#,
        );
        p.runner.executable = Some(executable);
        let saved = std::cell::RefCell::new(None);
        assert!(generate_planned(
            &p,
            None,
            || false,
            |_| {},
            |s| *saved.borrow_mut() = Some(json!(s))
        )
        .unwrap()
        .contains("实际返回"));
        let mut value = saved.borrow().clone().unwrap();
        assert!(value["plan"].as_str().unwrap().contains("历史取舍"));
        value["project_id"] = json!("another-project");
        assert!(generate_planned(&p, Some(value), || false, |_| {}, |_| {})
            .unwrap_err()
            .message
            .contains("本项目身份"));
    }

    #[test]
    fn repairs_share_the_round_budget_and_keep_the_full_received_draft() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        p.started = Instant::now() - runtime::MAXIMUM;
        // There is no CLI executable: an expired round must not start a fresh repair budget.
        let draft = "# 已收到的完整草稿\n章节格式仍需修订。";
        let retained = std::cell::RefCell::new(Vec::new());
        let result = complete(
            &data,
            &p,
            draft.into(),
            || false,
            |_| {},
            |body, _, _| retained.borrow_mut().push(body.to_string()),
        )
        .unwrap();
        assert!(!result.saved && result.needs_continuation && !result.needs_revision);
        assert!(result.warning.unwrap().contains("本轮预算已到"));
        assert!(retained.borrow().iter().any(|body| body == draft));
        assert_eq!(
            std::fs::read(p.input.path.join("README.md")).unwrap(),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unfinished_batch_keeps_its_evidence_and_continues_the_same_scope() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let report = test_report("最终汇总已有证据和后续证据。");
        let (_script, executable) = fake_cli(&format!(
            r#"import sys, pathlib, json
sys.stdin.read()
output=pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1])
context=json.loads((output.parent/'stage-context.json').read_text())
if context['phase']=='plan':
    text='{{"batches":[{{"title":"同一模块","scope":"先甲资料，再乙资料"}}]}}'
elif context['phase']=='batch':
    pending=context['checkpoint']['pending']
    if not pending:
        text='# 已有证据\n乙资料尚未读取。\n<!-- ailoom:continue -->'
    else:
        assert '已有证据' in pathlib.Path(pending[0]['path']).read_text()
        text='# 后续证据\n乙资料已读取并核对。'
else:
    merged=pathlib.Path(context['checkpoint']['completed'][0]['path']).read_text()
    assert '已有证据' in merged and '后续证据' in merged
    text={report:?}
output.write_text(text,encoding='utf-8')
"#
        ));
        p.runner.executable = Some(executable);
        let saved = std::cell::RefCell::new(None);
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let first = workflow::run_with_elapsed(
            &p,
            None,
            || false,
            |_| {},
            |cp| {
                let value = json!(cp);
                if value["pending"].as_array().unwrap().len() == 1 {
                    elapsed.set(runtime::WRAP_UP);
                }
                *saved.borrow_mut() = Some(value);
            },
            || elapsed.get(),
        )
        .unwrap_err();
        assert_eq!(first.context["kind"], "organization_budget");
        let checkpoint = saved.borrow().clone().unwrap();
        assert!(checkpoint["completed"].as_array().unwrap().is_empty());
        assert!(checkpoint["pending"][0]
            .as_str()
            .unwrap()
            .contains("已有证据"));
        let final_report =
            generate_planned(&p, Some(checkpoint), || false, |_| {}, |_| {}).unwrap();
        assert_eq!(final_report, report);
    }

    #[cfg(unix)]
    #[test]
    fn semantic_review_repairs_and_reassesses_before_atomic_publication() {
        let (_tmp, root, data) = fixture();
        let knowledge = location::organization_input(&data, &root).unwrap().path;
        std::fs::create_dir_all(knowledge.join("wiki")).unwrap();
        std::fs::write(
            knowledge.join("wiki/review.md"),
            "# 人工维护的评审经验\n这些原有内容不能被本轮评估覆盖。\n",
        )
        .unwrap();
        let mut p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        let candidate = test_report("这里错误地把历史做法标为当前规则。");
        let corrected = test_report("历史约束及当时选择应保留；当前适用性待核实。");
        let (_script, executable) = fake_cli(&format!(
            r#"import sys, pathlib, json
text=sys.stdin.read()
assert len(text.encode())<65536
output=pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1])
if '## 本轮独立评估入口' in text:
    candidate=(output.parent/'review-candidate.md').read_text()
    assert '先从原文建立关键知识清单' in text and '未读不能归为无价值' in text
    if '错误地' in candidate:
        value='# 开发知识评估\n结论：需修订\n必须修订项：历史文档不能证明当前实现，补回适用边界。\n<!-- ailoom-review: revise -->'
    else:
        prior=json.loads((output.parent/'previous-assessment.json').read_text())
        assert '历史文档' in pathlib.Path(prior['assessment_file']).read_text()
        value='# 开发知识评估\n结论：通过\n资料处理：已读采用、历史保留及未读范围已区分；关键知识按原文保留。\n<!-- ailoom-review: pass -->'
else:
    issue=json.loads((output.parent/'draft-issue.json').read_text())
    assert issue['context']['stage']=='semantic_review'
    value={corrected:?}
output.write_text(value,encoding='utf-8')
"#
        ));
        p.runner.executable = Some(executable);
        let assessments = std::cell::RefCell::new(Vec::new());
        let result = complete_with_assessment(
            &data,
            &p,
            candidate,
            || false,
            |_| {},
            |_, _, _| {},
            |assessment| {
                assert_eq!(
                    std::fs::read(p.input.path.join("README.md")).unwrap(),
                    before,
                    "no candidate is published during review or repair"
                );
                if assessment.complete {
                    assessments.borrow_mut().push(assessment.clone());
                }
            },
        )
        .unwrap();
        assert!(result.saved && result.assessment_status == "pass");
        assert_eq!(result.repair_attempts, 1);
        let history = assessments.borrow();
        assert_eq!(
            history
                .iter()
                .map(|a| a.verdict.as_str())
                .collect::<Vec<_>>(),
            ["revise", "pass"]
        );
        assert_ne!(history[0].candidate_sha256, history[1].candidate_sha256);
        let topic = std::fs::read_to_string(p.input.path.join("wiki/topic-migration.md")).unwrap();
        assert!(topic.contains("review_status: ai_reviewed") && topic.contains("历史约束"));
        let proof: Value = serde_json::from_slice(
            &std::fs::read(p.input.path.join(".ailoom/review.json")).unwrap(),
        )
        .unwrap();
        assert_ne!(proof["report_path"], "wiki/review.md");
        assert!(std::fs::read_to_string(p.input.path.join("wiki/review.md"))
            .unwrap()
            .contains("人工维护的评审经验"));
        assert!(
            std::fs::read_to_string(p.input.path.join(proof["report_path"].as_str().unwrap()))
                .unwrap()
                .contains("结论：通过")
        );
        assert_eq!(
            proof["candidate_sha256"],
            crate::ids::sha256_hex(corrected.as_bytes())
        );
        for (file, digest) in proof["pages"].as_object().unwrap() {
            assert_eq!(
                digest.as_str().unwrap(),
                crate::ids::sha256_hex(&std::fs::read(p.input.path.join(file)).unwrap())
            );
        }
        assert!(std::fs::read_to_string(p.input.path.join("README.md"))
            .unwrap()
            .contains("wiki/review.md"));
        assert!(!p.input.path.join(".ailoom/wiki-transaction.json").exists());
        let current = prepared(&data, &root);
        publish(&data, &current, &test_report("未经过新评估的后续草稿。")).unwrap();
        let proof: Value = serde_json::from_slice(
            &std::fs::read(p.input.path.join(".ailoom/review.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            proof["verdict"], "unreviewed",
            "old review is not inherited by a different draft"
        );
    }

    #[cfg(unix)]
    #[test]
    fn insufficient_and_ambiguous_assessments_keep_candidates_without_publishing() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        for assessment in ["# 开发知识评估\n结论：评估资料不足\n缺少关键分支的原文，不能确认主张。\n<!-- ailoom-review: insufficient -->","已查看候选稿，但这段没有给出明确结论。"] {
            let (_script,executable)=fake_cli(&format!("import sys,pathlib\nsys.stdin.read()\npathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1]).write_text({assessment:?},encoding='utf-8')"));
            p.runner.executable=Some(executable);
            let reviews=std::cell::RefCell::new(Vec::new());
            let result=complete_with_assessment(&data,&p,test_report("已有知识。"),||false,|_|{},|_,_,_|{},|a|if a.complete {reviews.borrow_mut().push(a.clone());}).unwrap();
            assert!(!result.saved && result.needs_revision);
            assert!(matches!(result.assessment_status.as_str(),"insufficient"|"unconfirmed"));
            assert!(!reviews.borrow().is_empty());
            assert_eq!(std::fs::read(p.input.path.join("README.md")).unwrap(),before);
            assert!(!p.input.path.join("wiki/review.md").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn review_format_variations_are_clarified_instead_of_failing_the_task() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let (_script, executable) = fake_cli(
            r#"import sys,pathlib
sys.stdin.read()
output=pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1])
value='# 开发知识评估\n核对证据的正文，尚未明确给出结论。'
if (output.parent/'previous-assessment.json').exists():
    value='# 开发知识评估\n\n- **结论**：通过\n\n阅读范围、处理记录和缺口已核对。'
output.write_text(value,encoding='utf-8')
"#,
        );
        p.runner.executable = Some(executable);
        let result = complete(
            &data,
            &p,
            test_report("已有知识。"),
            || false,
            |_| {},
            |_, _, _| {},
        )
        .unwrap();
        assert!(result.saved && result.assessment_status == "pass");
        assert_eq!(result.repair_attempts, 0);
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_review_preserves_partial_feedback_without_treating_it_as_a_verdict() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        let (_script, executable) = fake_cli(
            r#"import sys,pathlib,time
sys.stdin.read()
pathlib.Path(sys.argv[sys.argv.index('--output-last-message')+1]).write_text('# 评估写到一半\n<!-- ailoom-review: pass -->',encoding='utf-8')
time.sleep(30)
"#,
        );
        p.runner.executable = Some(executable);
        let now = Instant::now();
        let candidate = test_report("要保留的候选正文。");
        let feedback = std::cell::RefCell::new(Vec::new());
        let drafts = std::cell::RefCell::new(Vec::new());
        let error = complete_with_assessment(
            &data,
            &p,
            candidate.clone(),
            || now.elapsed() > Duration::from_millis(700),
            |_| {},
            |body, _, _| drafts.borrow_mut().push(body.to_string()),
            |a| feedback.borrow_mut().push(a.clone()),
        )
        .unwrap_err();
        assert!(error.message.contains("取消"));
        assert_eq!(drafts.borrow().last().unwrap(), &candidate);
        assert!(feedback
            .borrow()
            .iter()
            .any(|a| !a.complete && a.verdict == "incomplete" && a.report.contains("写到一半")));
        assert_eq!(
            std::fs::read(p.input.path.join("README.md")).unwrap(),
            before
        );
    }

    #[test]
    fn a_review_cannot_be_applied_to_another_candidate_or_input_snapshot() {
        let (_tmp, root, data) = fixture();
        let p = prepared(&data, &root);
        let before = std::fs::read(p.input.path.join("README.md")).unwrap();
        let report = test_report("候选知识。");
        let proof = json!({"schema":"ailoom-ai-review/v1","verdict":"pass","project_id":p.input.project_id,"knowledge_id":p.input.knowledge_id,"input_sha256":p.input.fingerprint,"candidate_sha256":"other-candidate"});
        let error = location::publish_organization_with_review(
            &data,
            &p.input,
            &report,
            Some(("结论：通过", proof)),
        )
        .unwrap_err();
        assert!(error.message.contains("不匹配"));
        assert_eq!(
            std::fs::read(p.input.path.join("README.md")).unwrap(),
            before
        );
    }

    #[test]
    fn large_assessment_feedback_and_work_records_remain_in_readable_task_files() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let large = "证据、跳过理由与缺陷。\n".repeat(100000);
        let report = test_report(&large);
        p.set_execution_record(json!({"plan":"按模块核对","completed":[large],"pending":[],"partial":"","previous_assessment":{"assessment":large}}));
        let work = tempfile::tempdir().unwrap();
        let prompt = task::review(
            &p,
            work.path(),
            &report,
            Some(&json!({"assessment":large,"candidate_sha256":"old-candidate"})),
        )
        .unwrap();
        assert!(prompt.len() < 64 * 1024);
        let metadata = std::fs::read(work.path().join("previous-assessment.json")).unwrap();
        assert!(metadata.len() < 4096);
        let metadata: Value = serde_json::from_slice(&metadata).unwrap();
        assert_eq!(
            std::fs::read_to_string(metadata["assessment_file"].as_str().unwrap()).unwrap(),
            large
        );
        let stage: Value =
            serde_json::from_slice(&std::fs::read(work.path().join("stage-context.json")).unwrap())
                .unwrap();
        assert!(
            std::fs::metadata(
                stage["checkpoint"]["completed"][0]["path"]
                    .as_str()
                    .unwrap()
            )
            .unwrap()
            .len()
                > 1024 * 1024
        );
        assert_eq!(
            std::fs::read_to_string(work.path().join("review-candidate.md")).unwrap(),
            report
        );
    }
}
