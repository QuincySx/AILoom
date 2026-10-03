//! Git 子进程辅助：参数数组调用、固定 cwd、检查退出码；错误输出做凭据脱敏。

use crate::error::{code, Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 脱敏文本中 URL 内嵌的凭据：`scheme://user:pass@host/…` → `scheme://***@host/…`。
/// 非 ASCII 与多 URL 均按 UTF-8 语义处理。
pub fn redact_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find("://") {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + 3..];
        let end = after
            .find(|c: char| c == '/' || c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(after.len());
        let authority = &after[..end];
        let tail = &after[end..];
        match authority.rsplit_once('@') {
            Some((_, host)) => {
                out.push_str("://***@");
                out.push_str(host);
            }
            None => {
                out.push_str("://");
                out.push_str(authority);
            }
        }
        out.push_str(tail);
        rest = "";
    }
    out.push_str(rest);
    out
}

/// 规范化 Git 远端 URL 为缓存 key 用的稳定身份：`git+<scheme://host/path>`。
/// 小写 scheme 与 host；去凭据；scp 形式 `git@host:path` 转 `ssh://…`；去尾部 `/`。
pub fn normalize_remote_url(raw: &str) -> String {
    let url = raw.trim();
    // scp 形式：user@host:path（无 scheme、host 部分无 '/'）
    let scp = url.find(':').and_then(|colon| {
        let head = &url[..colon];
        let has_scheme = head.contains("://");
        let looks_like_path = head.contains('/');
        if !has_scheme && !looks_like_path && head.contains('@') {
            Some((head.to_string(), url[colon + 1..].to_string()))
        } else {
            None
        }
    });
    let normalized = match scp {
        Some((user_host, path)) => {
            let host = user_host.rsplit('@').next().unwrap_or(&user_host);
            format!("ssh://{host}/{path}")
        }
        None => url.to_string(),
    };
    let (scheme, rest) = match normalized.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r.to_string()),
        None => return format!("git+{normalized}"),
    };
    let (authority, path) = match rest.find('/') {
        Some(idx) => (rest[..idx].to_string(), rest[idx..].to_string()),
        None => (rest.clone(), String::new()),
    };
    let host = match authority.rsplit_once('@') {
        Some((_, h)) => h.to_string(),
        None => authority,
    }
    .to_ascii_lowercase();
    let mut out = format!("git+{scheme}://{host}{path}");
    while out.ends_with('/') {
        out.pop();
    }
    out
}

/// 运行 git，成功返回 stdout（UTF-8、去尾换行）。失败 → E2007，stderr 脱敏后入 context。
pub fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| Error::new(code::GIT_COMMAND_FAILED, format!("无法启动 git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::new(
            code::GIT_COMMAND_FAILED,
            format!("git {} 失败", args.join(" ")),
        )
        .context(serde_json::json!({
            "cwd": cwd.display().to_string(),
            "stderr": redact_credentials(stderr.trim()),
            "exit": output.status.code(),
        })));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

/// 运行 git，命令失败（如无远端）时返回 None 而非错误。
pub fn git_optional(cwd: &Path, args: &[&str]) -> Option<String> {
    git(cwd, args).ok()
}

/// `git rev-parse --path-format=absolute <args>` 的绝对路径。
/// `--path-format` 是 git 2.31 才有的参数；更早的 git 不报错，而是把它原样作为一行输出，
/// 再输出相对当前目录的路径。识别这种情况并自行拼成绝对路径；新版 git 的结果与原来一致。
pub fn rev_parse_path(cwd: &Path, args: &[&str]) -> Result<PathBuf> {
    let mut full = vec!["rev-parse", "--path-format=absolute"];
    full.extend_from_slice(args);
    let out = git(cwd, &full)?;
    let line = out
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && *l != "--path-format=absolute")
        .ok_or_else(|| Error::new(code::GIT_COMMAND_FAILED, "git rev-parse 没有输出路径"))?;
    let legacy = out.lines().any(|l| l.trim() == "--path-format=absolute");
    let path = PathBuf::from(line);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    // 新版 git 给的是规范化的真实路径；旧版的相对 / 未规范化路径（如 `sub/../.git`）在这里补齐，
    // 让同一仓库从不同目录得到相同结果。路径还不存在时（如 info/exclude）保持拼接结果。
    Ok(if legacy {
        path.canonicalize().unwrap_or(path)
    } else {
        path
    })
}

/// 远端默认分支（远端 `HEAD` 指向的分支名）；远端不可达或没有 HEAD 时返回 None。
pub fn remote_default_branch(url: &str) -> Option<String> {
    let out = git_optional(
        &std::env::temp_dir(),
        &["ls-remote", "--symref", url, "HEAD"],
    )?;
    out.lines().find_map(|l| {
        let target = l.strip_prefix("ref:")?.split_whitespace().next()?;
        target.strip_prefix("refs/heads/").map(str::to_string)
    })
}

/// `git init` 包装（测试与脚手架复用）。默认分支固定为 `main`：
/// 不依赖本机 `init.defaultBranch`（未设置时 git 用 master，而 `ailoom init` 默认找 main）。
pub fn git_init(path: &Path, bare: bool) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let mut args = vec!["init", "-q"];
    if bare {
        args.push("--bare");
    }
    args.push(path.to_str().ok_or_else(|| {
        Error::new(code::ILLEGAL_PATH, "路径不是合法 UTF-8").context(serde_json::json!({
            "path": path.display().to_string()
        }))
    })?);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    git(parent, &args)?;
    git(path, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    Ok(())
}

/// `git add` 指定路径并提交，返回新 HEAD commit。路径数组逐个传入，绝不 `git add .`。
pub fn git_commit_all(cwd: &Path, message: &str, paths: &[&str]) -> Result<String> {
    for p in paths {
        git(cwd, &["add", "--", p])?;
    }
    git(
        cwd,
        &[
            "-c",
            "user.name=ailoom",
            "-c",
            "user.email=ailoom@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-q",
            "-m",
            message,
        ],
    )?;
    git(cwd, &["rev-parse", "HEAD"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_urls_strips_credentials_and_case() {
        assert_eq!(
            normalize_remote_url("https://user:hunter2@Example.COM/team/res.git/"),
            "git+https://example.com/team/res.git"
        );
        assert_eq!(
            normalize_remote_url("git@github.com:team/res.git"),
            "git+ssh://github.com/team/res.git"
        );
        assert_eq!(
            normalize_remote_url("/tmp/local repo"),
            "git+/tmp/local repo"
        );
        assert_eq!(
            normalize_remote_url("ssh://git@Host.Example/tmp.git"),
            "git+ssh://host.example/tmp.git"
        );
    }

    #[test]
    fn redact_hides_password_keeps_utf8() {
        let red = redact_credentials("fatal: https://user:hunter2@example.com/x 仓库不可达");
        assert!(!red.contains("hunter2"), "{red}");
        assert!(red.contains("example.com/x"), "{red}");
        assert!(red.contains("仓库不可达"), "{red}");
        assert_eq!(redact_credentials("no url here"), "no url here");
    }

    #[test]
    fn git_command_uses_arg_array_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        // 含空格文件名不会被 shell 拆分
        let f = dir.path().join("a b.txt");
        std::fs::write(&f, "x").unwrap();
        git_init(dir.path(), false).unwrap();
        git_commit_all(dir.path(), "init", &["a b.txt"]).unwrap();
        let log = git(dir.path(), &["log", "--oneline"]).unwrap();
        assert!(log.contains("init"));
    }

    #[test]
    fn remote_default_branch_follows_remote_head() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        git_init(dir.path(), false).unwrap();
        git(dir.path(), &["symbolic-ref", "HEAD", "refs/heads/master"]).unwrap();
        git_commit_all(dir.path(), "init", &["a.txt"]).unwrap();
        let url = format!("file://{}", dir.path().display());
        assert_eq!(remote_default_branch(&url).as_deref(), Some("master"));
        assert_eq!(
            remote_default_branch("file:///nonexistent/ailoom-repo"),
            None
        );
    }
}
