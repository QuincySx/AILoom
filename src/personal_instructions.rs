//! 个人指令层（AIL-042）：公司指令文件只读保护 + 逐宿主本地入口。
//!
//! 语义（按宿主官方行为区分，不能用同一条 override 规则套所有宿主）：
//! - Claude（追加语义）：`.claude/rules/*.md` 与 CLAUDE.md 基线共同加载 → 个人条目
//!   写独立文件 `.claude/rules/ailoom-personal.md`，公司 CLAUDE.md 永不写入。
//! - Codex（同目录替代语义）：每目录至多加载一个文件，`AGENTS.override.md` 优先于
//!   `AGENTS.md` → 个人替代视图**必须包含未调整的公司基线全文** + 个人补充段；
//!   基线变化时 desired 内容随之变化，由普通 plan/apply 重新合成（不冻结旧规范）。
//!   该视图只对加载它的宿主生效，不宣称从其他来源清除指令。
//!
//! 保护：个人模式下，落在 Git 已跟踪路径上的产物一律过滤为显式跳过（说明原因），
//! 不使用 skip-worktree/assume-unchanged/git rm --cached 隐藏修改。

use crate::adapters::common::{Artifact, ArtifactBody};
use crate::error::{code, Error, Result};
use crate::gitx::git_optional;
use std::path::{Path, PathBuf};

pub const CLAUDE_PERSONAL_RULE_FILE: &str = ".claude/rules/ailoom-personal.md";
pub const CODEX_PERSONAL_VIEW_FILE: &str = "AGENTS.override.md";
pub const VIEW_HEADER: &str =
    "<!-- AILoom 个人指令视图：包含公司 AGENTS.md 原文 + 个人补充；基线变化会自动重新合成 -->";

/// 个人指令条目的存放位置（数据区，仓外）。
pub fn entry_path(data_root: &Path, repo_id: &str, worktree_id: Option<&str>) -> PathBuf {
    let base = data_root.join("profile").join("instructions").join(repo_id);
    match worktree_id {
        Some(wt) => base.join(format!("wt-{wt}.md")),
        None => base.join("repo.md"),
    }
}

/// 某仓库全部指令条目的目录（迁移用，AIL-057）。
pub fn entry_base(data_root: &Path, repo_id: &str) -> PathBuf {
    data_root.join("profile").join("instructions").join(repo_id)
}

/// 读取有效个人指令：工作树条目优先于仓库默认（覆盖语义，来源可解释）。
pub fn load_entry(data_root: &Path, repo_id: &str, worktree_id: Option<&str>) -> Option<String> {
    if let Some(wt) = worktree_id {
        let p = entry_path(data_root, repo_id, Some(wt));
        if let Ok(text) = std::fs::read_to_string(&p) {
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }
    let repo_entry = entry_path(data_root, repo_id, None);
    std::fs::read_to_string(repo_entry)
        .ok()
        .filter(|t| !t.trim().is_empty())
}

/// 保存个人指令条目（写入数据区，不触碰仓库）。
pub fn save_entry(
    data_root: &Path,
    repo_id: &str,
    worktree_id: Option<&str>,
    content: &str,
) -> Result<()> {
    let p = entry_path(data_root, repo_id, worktree_id);
    std::fs::create_dir_all(p.parent().unwrap_or_else(|| Path::new(".")))?;
    crate::sync_common::atomic_write(&p, content.as_bytes())
}

/// 删除条目（清空 = 删除文件）。
pub fn clear_entry(data_root: &Path, repo_id: &str, worktree_id: Option<&str>) -> Result<()> {
    let p = entry_path(data_root, repo_id, worktree_id);
    if p.is_file() {
        std::fs::remove_file(&p)?;
    }
    Ok(())
}

/// 公司文件保护检查结果。
#[derive(Debug, serde::Serialize)]
pub struct SkippedTarget {
    pub path: String,
    pub reason: String,
}

/// 路径是否已被 Git 跟踪（工作树根相对路径；非 Git 目录恒为 false）。
/// 个人模式守卫的基础判断：对 create/update/delete/restore/undo 全部生效。
pub fn path_is_git_tracked(ws_root: &Path, rel: &str) -> bool {
    if !ws_root.join(".git").exists() && find_git_boundary(ws_root).is_none() {
        return false;
    }
    let out = git_optional(ws_root, &["ls-files", "--", rel]).unwrap_or_default();
    !out.trim().is_empty()
}

fn find_git_boundary(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    while let Some(current) = dir {
        if current.join(".git").exists() {
            return Some(current);
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    None
}

/// 个人模式守卫：过滤落在 Git 已跟踪路径上的产物 → 显式跳过并说明，
/// 绝不借 skip-worktree/assume-unchanged/rm --cached 隐藏修改。
pub fn guard_company_files(
    ws_root: &Path,
    artifacts: Vec<Artifact>,
) -> Result<(Vec<Artifact>, Vec<SkippedTarget>)> {
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    let mut cache: std::collections::BTreeMap<String, bool> = Default::default();
    for a in artifacts {
        let path_str = a.path.display().to_string();
        let tracked = match cache.get(&path_str) {
            Some(v) => *v,
            None => {
                let t = path_is_git_tracked(ws_root, &path_str);
                cache.insert(path_str.clone(), t);
                t
            }
        };
        if tracked {
            skipped.push(SkippedTarget {
                path: path_str,
                reason: "公司已跟踪文件，个人模式不写入（保持公司内容与暂存区原样）".into(),
            });
        } else {
            kept.push(a);
        }
    }
    Ok((kept, skipped))
}

/// 渲染 Claude 追加语义的个人条目（独立文件，不触碰 CLAUDE.md）。
pub fn render_claude_additive(entry_content: &str) -> Artifact {
    Artifact {
        resource_id: "ailoom-personal/instructions/claude".into(),
        target_tool: "claude".into(),
        kind: "rule".into(),
        path: PathBuf::from(CLAUDE_PERSONAL_RULE_FILE),
        body: ArtifactBody::Full {
            content: format!(
                "# 个人偏好（AILoom，本地管理）\n\n{}\n",
                entry_content.trim()
            ),
        },
    }
}

/// 公司 AGENTS.md 当前工作树内容摘要（不存在 → None）。
pub fn codex_baseline_digest(ws_root: &Path) -> Result<Option<String>> {
    let p = ws_root.join("AGENTS.md");
    if !p.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&p)?;
    Ok(Some(format!("sha256:{}", crate::ids::sha256_hex(&bytes))))
}

/// 渲染 Codex 个人替代视图：未调整的公司基线全文 + 个人补充段。
/// 官方语义（2026-09-16 复核）：每目录至多加载一个文件，override 优先 →
/// 视图必须包含基线，否则个人文件会遮蔽公司指令。
pub fn render_codex_view(ws_root: &Path, entry_content: &str) -> Result<Artifact> {
    let baseline_path = ws_root.join("AGENTS.md");
    let baseline = if baseline_path.is_file() {
        std::fs::read_to_string(&baseline_path)?
    } else {
        String::new()
    };
    let mut content = String::new();
    content.push_str(VIEW_HEADER);
    content.push_str("\n\n");
    if baseline.trim().is_empty() {
        content.push_str("<!-- 本仓库暂无公司 AGENTS.md：以下仅为个人补充 -->\n\n");
    } else {
        content.push_str(baseline.trim_end());
        content.push_str("\n\n---\n\n");
    }
    content.push_str("<!-- BEGIN AILOOM MANAGED: ailoom-personal/instructions/codex v1 -->\n");
    content.push_str("# 个人补充（AILoom 本地视图）\n\n");
    content.push_str(entry_content.trim());
    content.push_str("\n<!-- END AILOOM MANAGED: ailoom-personal/instructions/codex -->\n");
    Ok(Artifact {
        resource_id: "ailoom-personal/instructions/codex".into(),
        target_tool: "codex".into(),
        kind: "rule".into(),
        path: PathBuf::from(CODEX_PERSONAL_VIEW_FILE),
        body: ArtifactBody::Full { content },
    })
}

/// 渲染个人指令产物（按启用的宿主）。产物随后应经过 [`guard_company_files`]。
pub fn render(ws_root: &Path, hosts: &[String], entry_content: &str) -> Result<Vec<Artifact>> {
    if entry_content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for h in hosts {
        match h.as_str() {
            "claude" => out.push(render_claude_additive(entry_content)),
            "codex" => out.push(render_codex_view(ws_root, entry_content)?),
            other => {
                return Err(Error::new(
                    code::HOST_UNSUPPORTED,
                    format!(
                        "宿主 {other} 暂无个人指令入口（未验证加载语义，不盲目套用 override 规则）"
                    ),
                ))
            }
        }
    }
    Ok(out)
}

/// 个人产物部署后的 exclude 登记 patterns（仓内新增文件默认不进 git add）。
pub fn exclude_patterns(artifacts: &[Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .map(|a| a.path.display().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_view_includes_unadjusted_baseline_and_managed_section() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        std::fs::write(ws.join("AGENTS.md"), "# 公司规范\n\n- 不准乱来\n").unwrap();
        let a = render_codex_view(ws, "- 个人：回答用中文\n").unwrap();
        let ArtifactBody::Full { content } = &a.body else {
            panic!("应为 Full");
        };
        assert!(content.contains("# 公司规范"));
        assert!(content.contains("- 不准乱来"), "基线全文原样包含");
        assert!(content.contains("# 个人补充"));
        assert!(content.contains("- 个人：回答用中文"));
        assert!(content.contains("BEGIN AILOOM MANAGED"));
        // 基线变化 → 内容变化（不冻结旧规范）
        std::fs::write(ws.join("AGENTS.md"), "# 公司规范 v2\n").unwrap();
        let b = render_codex_view(ws, "- 个人：回答用中文\n").unwrap();
        assert_ne!(a.desired_hash().unwrap(), b.desired_hash().unwrap());
    }

    #[test]
    fn claude_additive_never_touches_company_files() {
        let a = render_claude_additive("- 偏好 A\n");
        assert_eq!(a.path, PathBuf::from(".claude/rules/ailoom-personal.md"));
        let ArtifactBody::Full { content } = &a.body else {
            panic!()
        };
        assert!(content.contains("- 偏好 A"));
        assert!(!content.contains("CLAUDE.md"));
    }

    #[test]
    fn worktree_entry_overrides_repo_entry() {
        let data = tempfile::tempdir().unwrap();
        save_entry(data.path(), "repo-x", None, "repo 级偏好").unwrap();
        save_entry(data.path(), "repo-x", Some("wt1"), "wt1 覆盖").unwrap();
        assert_eq!(
            load_entry(data.path(), "repo-x", Some("wt1")).as_deref(),
            Some("wt1 覆盖")
        );
        assert_eq!(
            load_entry(data.path(), "repo-x", Some("wt2")).as_deref(),
            Some("repo 级偏好")
        );
        assert_eq!(
            load_entry(data.path(), "repo-x", None).as_deref(),
            Some("repo 级偏好")
        );
        clear_entry(data.path(), "repo-x", Some("wt1")).unwrap();
        assert_eq!(
            load_entry(data.path(), "repo-x", Some("wt1")).as_deref(),
            Some("repo 级偏好")
        );
    }
}
