//! Local change evidence, never a semantic decision about whether knowledge is finished.
use super::{fail, location, Prepared, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const SCHEMA: &str = "ailoom-organization-baseline/v1";

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
struct Snapshot {
    sources: BTreeMap<String, String>,
    provenance: BTreeMap<String, String>,
    code: BTreeMap<String, String>,
    wiki: BTreeMap<String, String>,
    uncertainties: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Baseline {
    schema: String,
    project_id: String,
    knowledge_id: String,
    policy: String,
    published_at: String,
    snapshot: Snapshot,
}

pub(super) struct Context {
    current: Snapshot,
    pub summary: Value,
    pub changes: Vec<Value>,
    eligible: bool,
}

fn policy() -> String {
    crate::ids::sha256_hex(
        format!(
            "{SCHEMA}\n{}\n{}\n{}",
            super::ORGANIZATION_PROMPT,
            super::review::PROMPT,
            include_str!("task.rs")
        )
        .as_bytes(),
    )
}

fn path(data: &Path, input: &location::OrganizationInput) -> PathBuf {
    let key =
        crate::ids::sha256_hex(format!("{}\n{}", input.project_id, input.knowledge_id).as_bytes());
    data.join("knowledge/organization-baselines")
        .join(format!("{key}.json"))
}

fn hash(path: &Path, cancelled: &impl Fn() -> bool) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        if cancelled() {
            return Err(fail("整理已取消，已有知识与增量基线保留"));
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn remember(
    map: &mut BTreeMap<String, String>,
    errors: &mut Vec<String>,
    key: String,
    path: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    match hash(path, cancelled) {
        Ok(value) => {
            map.insert(key, value);
        }
        Err(error) if cancelled() => return Err(error),
        Err(_) => errors.push(format!("无法计算文件指纹：{}", path.display())),
    }
    Ok(())
}

fn excluded(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    (name.starts_with('.')
        && !matches!(
            name.as_str(),
            ".github"
                | ".changeset"
                | ".husky"
                | ".gitignore"
                | ".gitattributes"
                | ".gitmodules"
                | ".editorconfig"
        ))
        || name.contains("credential")
        || name.contains("secret")
        || name.contains("password")
        || name.contains("api_key")
        || name.contains("api-key")
        || name.rsplit_once('.').is_some_and(|(stem, extension)| {
            matches!(stem, "auth" | "token" | "tokens" | "key" | "keys")
                && matches!(extension, "json" | "yaml" | "yml" | "toml")
        })
        || matches!(
            name.as_str(),
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "vendor"
                | "venv"
                | "__pycache__"
                | "pods"
                | "carthage"
                | "coverage"
                | "logs"
                | "auth.json"
                | "tokens.json"
                | "key.json"
                | "keys.json"
        )
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
}

fn code_file(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "ts" | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "rs"
            | "go"
            | "py"
            | "java"
            | "kt"
            | "kts"
            | "swift"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "vue"
            | "svelte"
            | "sql"
            | "graphql"
            | "prisma"
            | "proto"
            | "sh"
            | "zsh"
            | "bash"
            | "ps1"
            | "xml"
            | "gradle"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "md"
            | "mdx"
            | "markdown"
            | "rst"
            | "adoc"
            | "lock"
    ) || matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some(
            "Makefile"
                | "Dockerfile"
                | "Justfile"
                | ".gitignore"
                | ".gitattributes"
                | ".gitmodules"
                | ".editorconfig"
        )
    )
}

fn wiki(
    input: &location::OrganizationInput,
    snapshot: &mut Snapshot,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let mut paths = input
        .previous_documents
        .iter()
        .map(|p| p.path.clone())
        .collect::<Vec<_>>();
    paths.extend([
        input.report_path.clone(),
        PathBuf::from(".ailoom/review.json"),
        PathBuf::from(".ailoom/wiki.json"),
    ]);
    paths.sort();
    paths.dedup();
    for relative in paths {
        let file = input.path.join(&relative);
        if file.is_file() {
            remember(
                &mut snapshot.wiki,
                &mut snapshot.uncertainties,
                relative.to_string_lossy().into_owned(),
                &file,
                cancelled,
            )?;
        }
    }
    Ok(())
}

fn capture(
    data: &Path,
    input: &location::OrganizationInput,
    cancelled: &impl Fn() -> bool,
    progress: &impl Fn(&str),
) -> Result<Snapshot> {
    let mut snapshot = Snapshot::default();
    for document in &input.documents {
        let key = document.to_string_lossy().into_owned();
        remember(
            &mut snapshot.sources,
            &mut snapshot.uncertainties,
            key.clone(),
            &input.path.join(document),
            cancelled,
        )?;
        // Capture time, HEAD and dirty status change on unrelated edits; actual code is hashed below.
        let origins = input.origins.get(document).into_iter().flatten().map(|o| json!({"worktree":o.worktree_id,"directory":o.directory,"document":o.document,"branch":o.branch})).collect::<Vec<_>>();
        snapshot.provenance.insert(
            key,
            crate::ids::sha256_hex(serde_json::to_string(&origins)?.as_bytes()),
        );
    }
    for (document, origins) in &input.historical_origins {
        let key = format!("historical:{}", document.display());
        remember(
            &mut snapshot.provenance,
            &mut snapshot.uncertainties,
            key.clone(),
            &input.path.join(document),
            cancelled,
        )?;
        snapshot.provenance.insert(
            format!("{key}:origins"),
            crate::ids::sha256_hex(&serde_json::to_vec(origins)?),
        );
    }
    snapshot.provenance.insert(
        "excluded_documents".into(),
        crate::ids::sha256_hex(&serde_json::to_vec(&input.excluded_documents)?),
    );
    wiki(input, &mut snapshot, cancelled)?;
    for source in &input.worktrees {
        progress(&format!(
            "比较代码与文档指纹：{}（包括未提交文件）",
            source.label
        ));
        let walker = walkdir::WalkDir::new(&source.path)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || (!excluded(&entry.file_name().to_string_lossy())
                        && !entry.path().starts_with(&input.path)
                        && !entry.path().starts_with(data)
                        && !(entry.file_type().is_dir()
                            && (entry.path().join(".git").exists()
                                || entry.path().join(".ailoom/library.json").exists())))
            });
        for entry in walker {
            if cancelled() {
                return Err(fail("整理已取消，已有知识与增量基线保留"));
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    snapshot
                        .uncertainties
                        .push(format!("无法盘点工作目录：{error}"));
                    continue;
                }
            };
            if !entry.file_type().is_file() || !code_file(entry.path()) {
                continue;
            }
            remember(
                &mut snapshot.code,
                &mut snapshot.uncertainties,
                entry.path().to_string_lossy().into_owned(),
                entry.path(),
                cancelled,
            )?;
        }
    }
    Ok(snapshot)
}

pub(super) fn prepare(
    data: &Path,
    prepared: &Prepared,
    cancelled: &impl Fn() -> bool,
    progress: &impl Fn(&str),
) -> Result<Context> {
    let current = capture(data, &prepared.input, cancelled, progress)?;
    let old = std::fs::read(path(data, &prepared.input))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Baseline>(&bytes).ok())
        .filter(|b| {
            b.schema == SCHEMA
                && b.project_id == prepared.input.project_id
                && b.knowledge_id == prepared.input.knowledge_id
                && b.policy == policy()
        });
    let mut changes = Vec::new();
    for (kind, now, before) in [
        (
            "source",
            &current.sources,
            old.as_ref().map(|b| &b.snapshot.sources),
        ),
        (
            "provenance",
            &current.provenance,
            old.as_ref().map(|b| &b.snapshot.provenance),
        ),
        (
            "code",
            &current.code,
            old.as_ref().map(|b| &b.snapshot.code),
        ),
        (
            "wiki",
            &current.wiki,
            old.as_ref().map(|b| &b.snapshot.wiki),
        ),
    ] {
        for (path, digest) in now {
            let change = match before.and_then(|files| files.get(path)) {
                None => "added",
                Some(old) if old != digest => "changed",
                _ => continue,
            };
            changes.push(json!({"kind":kind,"path":path,"change":change}));
        }
        for path in before
            .into_iter()
            .flat_map(|files| files.keys())
            .filter(|p| !now.contains_key(*p))
        {
            changes.push(json!({"kind":kind,"path":path,"change":"deleted"}));
        }
    }
    let eligible = old
        .as_ref()
        .is_some_and(|b| b.snapshot.uncertainties.is_empty())
        && current.uncertainties.is_empty()
        && changes.is_empty();
    let summary = json!({
        "mode":if old.is_some() {"incremental"} else {"bootstrap"},
        "baseline_published_at":old.as_ref().map(|b| &b.published_at),
        "changes":changes.len(),
        "source_changes":changes.iter().filter(|r| r["kind"]=="source" || r["kind"]=="provenance").count(),
        "code_changes":changes.iter().filter(|r| r["kind"]=="code").count(),
        "wiki_changes":changes.iter().filter(|r| r["kind"]=="wiki").count(),
        "unchanged_review_available":eligible,
        "uncertainties":current.uncertainties,
        "scope":"允许的工作目录内常见代码、文档和配置文本；含未提交/未跟踪文件。跳过工具/认证隐藏目录、依赖、构建、嵌套仓库、符号链接和知识库目录；未覆盖文件不能推断无变化。",
    });
    Ok(Context {
        current,
        summary,
        changes,
        eligible,
    })
}

/// Even an unchanged input needs the planner to consider unread sources and open questions.
pub(super) fn can_reuse(
    prepared: &Prepared,
    cancelled: &impl Fn() -> bool,
    progress: &impl Fn(&str),
) -> Result<bool> {
    let Some(context) = &prepared.incremental else {
        return Ok(false);
    };
    if !context.eligible || !prepared.focus.trim().is_empty() {
        return Ok(false);
    }
    progress("AI 建议沿用已有知识；重新检查规划期间的资料、代码和人工页面变化");
    let fresh = location::organization_input(&prepared.data_root, &prepared.input.root)?;
    if fresh.project_id != prepared.input.project_id
        || fresh.knowledge_id != prepared.input.knowledge_id
        || fresh.path != prepared.input.path
        || serde_json::to_value(&fresh.worktrees)?
            != serde_json::to_value(&prepared.input.worktrees)?
    {
        return Ok(false);
    }
    let now = capture(&prepared.data_root, &fresh, cancelled, progress)?;
    Ok(now == context.current && now.uncertainties.is_empty())
}

/// Only the independently reviewed publication advances the baseline. Failure leaves the prior baseline.
pub(super) fn published(data: &Path, prepared: &Prepared, candidate_sha256: &str) -> Result<()> {
    let Some(context) = &prepared.incremental else {
        return Ok(());
    };
    let input = location::organization_input(data, &prepared.input.root)?;
    if input.project_id != prepared.input.project_id
        || input.knowledge_id != prepared.input.knowledge_id
        || input.path != prepared.input.path
    {
        return Err(fail("发布后项目绑定已变化，未将增量基线写入其他知识库"));
    }
    let mut snapshot = context.current.clone();
    snapshot.wiki.clear();
    wiki(&input, &mut snapshot, &|| false)?;
    let encoded = std::fs::read(input.path.join(".ailoom/review.json"))?;
    let proof: Value = serde_json::from_slice(&encoded)?;
    if proof["verdict"] != "pass"
        || proof["project_id"] != input.project_id
        || proof["knowledge_id"] != input.knowledge_id
        || proof["candidate_sha256"] != candidate_sha256
        || proof["input_sha256"] != prepared.input.fingerprint
        || snapshot.wiki.get(".ailoom/review.json") != Some(&crate::ids::sha256_hex(&encoded))
    {
        return Err(fail("发布后的评估记录已变化，未更新增量基线"));
    }
    let pages = proof["pages"]
        .as_object()
        .ok_or_else(|| fail("评估缺少发布文件摘要，未更新增量基线"))?;
    for (path, digest) in &snapshot.wiki {
        if path.starts_with(".ailoom/") {
            continue;
        }
        let expected = pages
            .get(path)
            .and_then(Value::as_str)
            .or_else(|| context.current.wiki.get(path).map(String::as_str));
        if expected != Some(digest.as_str()) {
            return Err(fail(
                "发布后页面出现人工改动，未将这些改动计入已评估的增量基线",
            ));
        }
    }
    if pages
        .iter()
        .any(|(path, digest)| snapshot.wiki.get(path).map(String::as_str) != digest.as_str())
    {
        return Err(fail("发布后的页面与评估摘要不匹配，未更新增量基线"));
    }
    let baseline = Baseline {
        schema: SCHEMA.into(),
        project_id: input.project_id.clone(),
        knowledge_id: input.knowledge_id.clone(),
        policy: policy(),
        published_at: crate::ids::now_iso(),
        snapshot,
    };
    let path = path(data, &input);
    std::fs::create_dir_all(path.parent().unwrap())?;
    crate::sync_common::atomic_write(&path, &serde_json::to_vec_pretty(&baseline)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authentication_and_generated_paths_are_filtered_before_any_content_is_opened() {
        for name in [
            ".env",
            ".ssh",
            ".codex",
            ".claude",
            ".aws",
            "credentials.json",
            "client-secret.json",
            "auth.json",
            "tokens.json",
            "identity.pem",
            "node_modules",
            "target",
        ] {
            assert!(excluded(name), "{name}");
        }
        for name in ["module.ts", ".github", ".gitignore"] {
            assert!(!excluded(name));
        }
    }
}
