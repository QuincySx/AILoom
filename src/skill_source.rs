//! Skill 来源身份与版本（AIL-063）：每个库内 skill 记录发现入口、实际上游、
//! 仓内路径、请求 ref、已解析 commit、内容摘要与获取时间。同名不同来源不混同：
//! 冲突拒绝时展示既有来源。旧数据（无来源记录）按「来源未知/本地管理」解释，
//! 仍可用，但不伪称能上游更新。

use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const IMPORT_META_FILE: &str = ".ailoom-import.json";

/// 来源提供方发现入口的规范化：保留用户输入，同时解析出实际上游。
/// provider 映射可扩展（skill.sh/skills.sh 等由 AIL-065 按官方证据核实后登记）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillSourceMeta {
    /// local | git | provider:<name> | unknown
    pub source_kind: String,
    /// 用户使用的发现入口（原样保留，如 URL 或本地路径）
    pub discovery_entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    /// 仓库内 skill 相对路径（skill 根，含 SKILL.md）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    /// 导入时的内容摘要（dir digest），用于上游比较
    pub imported_digest: String,
    pub fetched_at: String,
}

impl SkillSourceMeta {
    pub fn unknown(discovery_entry: &str, digest: &str) -> SkillSourceMeta {
        SkillSourceMeta {
            source_kind: "unknown".into(),
            discovery_entry: discovery_entry.into(),
            repo_url: None,
            repo_path: None,
            ref_: None,
            resolved_commit: None,
            imported_digest: digest.into(),
            fetched_at: crate::ids::now_iso(),
        }
    }

    pub fn updatable(&self) -> bool {
        self.source_kind == "git" || self.source_kind.starts_with("provider:")
    }
}

/// 读取库内 skill 的来源元数据；无记录 → None（旧数据：来源未知，不伪造）。
pub fn read_meta(skill_dir: &Path) -> Option<SkillSourceMeta> {
    let text = std::fs::read_to_string(skill_dir.join(IMPORT_META_FILE)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let meta: SkillSourceMeta = serde_json::from_value(v.get("source").cloned()?).ok()?;
    Some(meta)
}

/// 把来源元数据写进 .ailoom-import.json（保留其他字段）。
pub fn write_meta(skill_dir: &Path, meta: &SkillSourceMeta) -> Result<()> {
    let p = skill_dir.join(IMPORT_META_FILE);
    let mut root: serde_json::Value = if p.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&p)?).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    root["source"] = serde_json::to_value(meta)?;
    crate::sync_common::atomic_write(&p, serde_json::to_vec_pretty(&root)?.as_slice())
}

/// 同名不同来源不混同：导入冲突时给出既有来源（AIL-063 验收）。
pub fn existing_source_note(skill_dir: &Path) -> Option<String> {
    read_meta(skill_dir).map(|m| {
        format!(
            "库内同名 skill 来自 {}（入口 {}）",
            m.source_kind, m.discovery_entry
        )
    })
}

/// 本地目录导入的来源元数据（来源=本地路径，如实标注，不编造远端）。
pub fn local_meta(source_dir: &Path, digest: &str) -> SkillSourceMeta {
    SkillSourceMeta {
        source_kind: "local".into(),
        discovery_entry: format!("local-dir:{}", source_dir.display()),
        repo_url: None,
        repo_path: None,
        ref_: None,
        resolved_commit: None,
        imported_digest: digest.into(),
        fetched_at: crate::ids::now_iso(),
    }
}

/// Git 来源的元数据。
pub fn git_meta(
    discovery_entry: &str,
    repo_url: &str,
    repo_path: &str,
    ref_: Option<&str>,
    resolved_commit: Option<&str>,
    digest: &str,
) -> SkillSourceMeta {
    SkillSourceMeta {
        source_kind: "git".into(),
        discovery_entry: discovery_entry.into(),
        repo_url: Some(repo_url.into()),
        repo_path: Some(repo_path.into()),
        ref_: ref_.map(str::to_string),
        resolved_commit: resolved_commit.map(str::to_string),
        imported_digest: digest.into(),
        fetched_at: crate::ids::now_iso(),
    }
}

/// 上游检查结果（AIL-066）。
#[derive(Debug, Serialize)]
pub struct UpdateStatus {
    pub skill: String,
    pub source_kind: String,
    pub discovery_entry: String,
    /// up-to-date | local-modified | upstream-new | conflict | upstream-missing | not-applicable(unknown/local)
    pub state: String,
    pub imported_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 校验更新状态可否直接更新（本地未改才允许静默替换；冲突需显式处理）。
pub fn ensure_updatable(status: &UpdateStatus) -> Result<()> {
    match status.state.as_str() {
        "up-to-date" => Err(Error::new(
            code::SOURCE_CONFLICT,
            "上游无变化，无需更新",
        )),
        "local-modified" | "conflict" => Err(Error::new(
            code::SOURCE_CONFLICT,
            format!(
                "本地与上游均有差异（{}）：为保留本地修改，请先处理冲突（删除库内副本改名重导，或放弃本地修改后强制更新）",
                status.state
            ),
        )),
        "upstream-missing" => Err(Error::new(
            code::UNKNOWN_REFERENCE,
            "上游已不存在该 skill（可能被删除/改名）；确认后可删除库内副本",
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_roundtrip_and_legacy_semantics() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("my-skill");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(
            read_meta(&dir).is_none(),
            "旧数据无来源记录 → 来源未知，不伪造"
        );
        let m = git_meta(
            "github:https://github.com/o/r",
            "https://github.com/o/r",
            "skills/my-skill",
            Some("main"),
            Some("abc123"),
            "digest-1",
        );
        write_meta(&dir, &m).unwrap();
        let back = read_meta(&dir).unwrap();
        assert_eq!(back.source_kind, "git");
        assert_eq!(back.resolved_commit.as_deref(), Some("abc123"));
        assert!(back.updatable());
        assert!(
            !SkillSourceMeta::unknown("x", "d").updatable(),
            "unknown 不伪称可上游更新"
        );
    }
}

// ---------------------------------------------------------------------------
// 发现入口解析（AIL-065）：skills.sh（Vercel Agent Skills Directory，已核实
// 2026-09-17）条目格式为 /<owner>/<repo>[/<skill>]，上游即 GitHub 仓库；
// 安装机制为 `npx skills add <owner/repo>`——AILoom 不复用其安装器，
// 只解析出实际上游后走自己的 git 导入（仅复制文件，绝不执行 npx/脚本）。
// ---------------------------------------------------------------------------

/// 解析结果：实际上游仓库 + 可选 skill 名提示。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderRef {
    pub discovery_entry: String,
    pub repo_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint_name: Option<String>,
    pub provider: String,
}

/// 规范化发现入口。已核实提供方：skills.sh（含常见误写 skill.sh）。
/// GitHub 直链直接透传；未知提供方显式报错（不猜测 API，可改填 GitHub 地址）。
pub fn normalize_discovery_entry(input: &str) -> Result<ProviderRef> {
    let raw = input
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let (provider, rest) = if let Some(r) = raw.strip_prefix("skills.sh/") {
        ("skills.sh", r)
    } else if let Some(r) = raw.strip_prefix("skill.sh/") {
        // 用户常见误写：保留输入并按 skills.sh 解析（同一站点），记录依据
        ("skills.sh(skill.sh 输入)", r)
    } else if raw.starts_with("github.com/") || raw.starts_with("github:") {
        let repo = raw
            .strip_prefix("github:")
            .unwrap_or(raw)
            .trim_start_matches("github.com/");
        return Ok(ProviderRef {
            discovery_entry: format!("github:{input}"),
            repo_url: format!("https://github.com/{}", repo.trim_end_matches('/')),
            hint_name: None,
            provider: "github".into(),
        });
    } else {
        return Err(Error::new(
            code::USAGE,
            format!(
                "未知发现入口: {input}。已核实提供方：skills.sh/<owner>/<repo>[/<skill>]；或直接提供 GitHub 地址（github.com/<owner>/<repo>）"
            ),
        ));
    };
    let segs: Vec<&str> = rest
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if segs.len() < 2 {
        return Err(Error::new(
            code::USAGE,
            format!("{provider} 条目至少需要 <owner>/<repo>: {input}"),
        ));
    }
    let hint = segs.get(2).map(|s| s.to_string());
    Ok(ProviderRef {
        discovery_entry: format!("skills.sh:{input}"),
        repo_url: format!("https://github.com/{}/{}", segs[0], segs[1]),
        hint_name: hint,
        provider: "skills.sh".into(),
    })
}

/// 已核实的公开样例（解析回归用）：skills.sh/vercel-labs/skills/find-skills
/// → github.com/vercel-labs/skills，skill 名 find-skills（2026-09-17 抓取核实）。
#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn verified_samples_parse() {
        let p = normalize_discovery_entry("skills.sh/vercel-labs/skills/find-skills").unwrap();
        assert_eq!(p.repo_url, "https://github.com/vercel-labs/skills");
        assert_eq!(p.hint_name.as_deref(), Some("find-skills"));
        let p = normalize_discovery_entry("https://skills.sh/anthropics/skills").unwrap();
        assert_eq!(p.repo_url, "https://github.com/anthropics/skills");
        assert!(p.hint_name.is_none());
        // 常见误写 skill.sh → 同一站点解析，输入保留
        let p = normalize_discovery_entry("skill.sh/vercel-labs/skills/find-skills").unwrap();
        assert_eq!(p.repo_url, "https://github.com/vercel-labs/skills");
        assert!(
            p.discovery_entry.contains("skill.sh/"),
            "输入原样保留: {}",
            p.discovery_entry
        );
        // GitHub 直链
        let p = normalize_discovery_entry("github.com/o/r").unwrap();
        assert_eq!(p.repo_url, "https://github.com/o/r");
        // 未知提供方显式报错
        assert!(normalize_discovery_entry("example.com/x").is_err());
    }
}
