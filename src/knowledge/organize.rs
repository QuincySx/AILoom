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
    }))
}

pub struct Prepared {
    input: OrganizationInput,
    runner: Runner,
    focus: String,
    collection: Option<Value>,
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

fn prompt(prepared: &Prepared) -> String {
    let input = &prepared.input;
    let focus = prepared.focus.trim();
    let manifest = json!({
        "repository":input.root,
        "project_id":input.project_id,
        "project_name":input.project_name,
        "knowledge_directory":input.path,
        "source_documents":input.documents,
        "source_origins":input.origins,
        "collection":prepared.collection,
        "previous_report":input.previous_report.as_ref().map(|_| if input.path.join(&input.report_path).is_file() { input.report_path.clone() } else if input.path.join(location::legacy_wiki_index(&input.project_id)).is_file() {location::legacy_wiki_index(&input.project_id)} else {PathBuf::from("docs/organized").join(format!("{}.md",input.project_id))}),
        "previous_documents":input.previous_documents,
        "excluded_documents":input.excluded_documents,
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
    if prepared.input.documents.is_empty() {
        return Err(fail("本次没有找到可整理的资料，可查看本次资料读取情况中的未读取原因；配置恢复副本与工具资源不参与整理"));
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
    progress("正在读取资料并编纂仓库开发知识");
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
            collection: None,
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
        assert_eq!(
            p.worktrees().len(),
            2,
            "new worktrees do not expand queued authorization"
        );
        assert_eq!(p.input.origins.values().map(Vec::len).sum::<usize>(), 3);
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

    #[test]
    fn empty_collection_keeps_diagnostics_without_starting_an_ai_process() {
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
            "worker can persist collection before the no-input error"
        );
        // The test runner has no executable; attempting to spawn it would panic.
        assert!(generate(&p, || false, |_| {})
            .unwrap_err()
            .message
            .contains("资料读取情况"));
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
assert 'sources/notes/migration.md' in task
assert '数据库维护' in task
pathlib.Path(args[args.index('--output-last-message') + 1]).write_text('# 仓库维护知识与建议\n迁移后验证行数。', encoding='utf-8')
print('this is not the final report')"#,
        );
        p.runner.executable = Some(executable);
        let report = generate_with_timeout(&p, || false, |_| {}, Duration::from_secs(5)).unwrap();
        assert_eq!(report, "# 仓库维护知识与建议\n迁移后验证行数。");
        assert!(
            p.input.path.join(&p.input.report_path).is_file(),
            "生成过程保留初始化首页"
        );
        publish(&data, &p, &test_report(&report)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cli_failure_empty_response_timeout_and_cancel_preserve_previous_knowledge() {
        let (_tmp, root, data) = fixture();
        let mut p = prepared(&data, &root);
        let original = std::fs::read(p.input.path.join(note_path(&p))).unwrap();
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
            std::fs::read(p.input.path.join(note_path(&p))).unwrap(),
            original
        );
        assert!(p.input.path.join(&p.input.report_path).exists());
    }
}
