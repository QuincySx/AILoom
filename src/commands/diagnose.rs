//! `ailoom diagnose`：生成一份可以直接发给维护者的诊断报告。
//!
//! 内容只有排查需要的事实：版本、系统、数据位置、服务与自启动状态、资源库 / 全局 Skill
//! 概况、当前目录的体检结果，以及服务日志末尾。不含任何资源正文与配置内容；
//! 用户目录替换为 `~`，用户名与令牌类字段抹掉。

use crate::error::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const LOG_TAIL_LINES: usize = 200;

/// 抹掉用户目录、用户名与令牌类值。报告会被转发给别人，宁可多抹。
pub fn redact(text: &str, home: Option<&Path>) -> String {
    let mut out = text.to_string();
    if let Some(home) = home {
        let mut homes = vec![home.to_string_lossy().to_string()];
        if let Ok(canon) = home.canonicalize() {
            homes.push(canon.to_string_lossy().to_string());
        }
        homes.sort_by_key(|h| std::cmp::Reverse(h.len()));
        for h in homes.iter().filter(|h| h.len() > 1) {
            out = out.replace(h.as_str(), "~");
        }
        if let Some(user) = home.file_name().map(|n| n.to_string_lossy().to_string()) {
            if user.len() >= 3 {
                out = out.replace(&user, "<user>");
            }
        }
    }
    redact_secrets(&out)
}

/// `token` / `secret` / `password` / `x-ailoom-session` 之后的值替换为 `<redacted>`。
fn redact_secrets(text: &str) -> String {
    const KEYS: [&str; 5] = [
        "token",
        "secret",
        "password",
        "x-ailoom-session",
        "authorization",
    ];
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let hit = KEYS
            .iter()
            .find(|k| lower[i..].starts_with(*k))
            .map(|k| k.len());
        let Some(key_len) = hit else {
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        };
        out.push_str(&text[i..i + key_len]);
        i += key_len;
        // 跳过分隔符（引号、冒号、等号、空白），值本身换成 <redacted>
        let mut j = i;
        while j < text.len() && matches!(bytes[j], b'"' | b'\'' | b':' | b'=' | b' ') {
            j += 1;
        }
        let value_start = j;
        while j < text.len()
            && !matches!(bytes[j], b'"' | b'\'' | b',' | b'}' | b' ' | b'\n' | b'&')
        {
            j += 1;
        }
        out.push_str(&text[i..value_start]);
        if j > value_start {
            out.push_str("<redacted>");
        }
        i = j;
    }
    out
}

fn log_tail(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(LOG_TAIL_LINES)..]
        .iter()
        .map(|l| l.to_string())
        .collect()
}

/// 报告本身（CLI 写文件、网页直接下载共用）。
pub fn report(data_root: &Path) -> Value {
    let section = |r: Result<Value>| r.unwrap_or_else(|e| json!({ "error": e.to_json() }));
    let (entries, issues) = crate::personal_library::list_tolerant(data_root);
    let library = json!({
        "entries": entries.len(),
        "skills": entries.iter().filter(|e| e.kind == "skill").count(),
        "issues": issues.iter().map(|i| json!({"path": i.path, "error": i.error})).collect::<Vec<_>>(),
    });
    let collections = section(crate::collections::list(data_root))
        .get("sources")
        .and_then(|s| s.as_array())
        .map(|s| json!({ "sources": s.len() }))
        .unwrap_or_else(|| json!({ "sources": 0 }));
    let global = section(crate::global_skills::status(data_root));
    let global = json!({
        "targets": global["targets"],
        "enabled": global["skills"].as_array().map(|s| s.iter().filter(|x| x["global"] == true).count()),
        "deployed": global["deployed"].as_array().map(Vec::len),
        "other_entries": global["foreign"].as_array().map(Vec::len),
        "pending": global["pending"],
        "notes": global["notes"],
        "error": global["error"],
    });
    let doctor = section(crate::commands::doctor::run(
        &crate::commands::doctor::DoctorArgs { root: None },
        true,
        Some(data_root),
    ));
    let store = crate::paths::resolve_store_root()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    json!({
        "schema_version": 1,
        "generated_at": crate::ids::now_iso(),
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "data_root": data_root.display().to_string(),
        "store_root": store,
        "service": section(crate::service::status(data_root)),
        "library": library,
        "collections": collections,
        "global_skills": global,
        "doctor_here": doctor,
        "service_log_tail": log_tail(&data_root.join("service/service.log")),
    })
}

/// 脱敏后的报告文本。
pub fn redacted_report(data_root: &Path) -> Result<String> {
    let text = serde_json::to_string_pretty(&report(data_root))?;
    Ok(redact(&text, crate::paths::user_home().as_deref()))
}

/// `ailoom diagnose`：写入 `ailoom-diagnose-<时间>.json`（默认当前目录）。
pub fn run(data_root: &Path, out_dir: Option<&Path>) -> Result<Value> {
    let dir: PathBuf = match out_dir {
        Some(d) => d.to_path_buf(),
        None => std::env::current_dir()?,
    };
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("ailoom-diagnose-{stamp}.json"));
    crate::sync_common::atomic_write(&path, redacted_report(data_root)?.as_bytes())?;
    Ok(json!({
        "path": path,
        "note": "已去掉用户目录、用户名与令牌；发送前可以打开看一眼",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_removes_home_user_and_token_values() {
        let home = Path::new("/Users/alice");
        let text = r#"{"path":"/Users/alice/.claude/skills/x","token":"abc123","log":"x-ailoom-session: deadbeef ok","pw":"password=hunter2&n=1"}"#;
        let out = redact(text, Some(home));
        assert!(!out.contains("alice"), "{out}");
        assert!(out.contains("~/.claude/skills/x"), "{out}");
        for secret in ["abc123", "deadbeef", "hunter2"] {
            assert!(!out.contains(secret), "{secret} 未脱敏: {out}");
        }
        assert!(
            out.contains(" ok") && out.contains("n=1"),
            "非秘密内容保留: {out}"
        );
    }
}
