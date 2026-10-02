//! 渲染产物统一类型（AIL-007 定义，适配卡消费）。
//! 适配器只产出 Artifact，不直接写盘；写入统一经过同步器。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{code, Error, Result};

/// 托管片段标记（契约冻结，AIL-010 起使用）。
pub fn fragment_begin(resource_id: &str) -> String {
    format!("<!-- BEGIN AILOOM MANAGED: {resource_id} v1 -->")
}

pub fn fragment_end(resource_id: &str) -> String {
    format!("<!-- END AILOOM MANAGED: {resource_id} -->")
}

/// 文件体语义：整文件 / JSON 指针合并 / TOML 表合并 / 文本托管片段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ArtifactBody {
    /// 外部管理：只拥有项目中的链接，不拥有目标目录及其内容。
    ExternalSymlink { target: PathBuf },
    /// 整个文件内容
    Full { content: String },
    /// 合并到 JSON 文件的 pointer 处（如 .mcp.json#mcpServers/name）
    JsonPointer {
        pointer: String,
        value: serde_json::Value,
    },
    /// 合并到 TOML 文件的表（如 config.toml 的 mcp_servers.name）
    TomlTable { table: String, value: toml::Value },
    /// 嵌入文本文件的受管片段；无片段时追加，删除时仅移除片段
    Fragment { content: String },
    /// 目录级软链：Workspace 挂载点 → SkillStore 实体目录
    Symlink {
        /// 绝对路径，指向 store 内 skill 目录
        target: PathBuf,
        /// 源仓 skill 目录内容摘要（期望值；apply 前不改写 store）
        content_digest: String,
        /// 快照内源目录，仅在 Create/Update apply 时物化到 target
        source_dir: PathBuf,
        /// 源 identity（写 `.meta/SOURCE.json`）
        source_identity: String,
    },
    /// TOML 数组 of tables 内按 key 字段管理的条目（如 alva 的 [[agent]]，按 name 叠加）
    TomlArrayEntry {
        /// 数组所在表的点分路径（如 "agent" 对应 [[agent]]）
        table: String,
        /// 身份字段（如 "name"）
        key_field: String,
        /// 本条目内容
        entry: toml::Value,
    },
    /// JSON 数组按托管签名合并（如 Claude settings 的 hooks.<Event>）：
    /// 先移除嵌套 hooks[].command 以 signature 开头的条目，再追加 value。
    /// 身份 = (路径, 签名)，与数组下标无关——同事件多 Hook 不冲突、重复同步不漂移。
    JsonArrayMerge {
        /// 数组所在 JSON pointer（如 /hooks/Stop）
        pointer: String,
        /// 托管签名（嵌套 hooks[].command 前缀）
        signature: String,
        /// 本条目内容
        value: serde_json::Value,
    },
}

/// 一个渲染产物。path 相对工作区根；禁止绝对路径与 `..`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub resource_id: String,
    pub target_tool: String,
    pub kind: String,
    pub path: PathBuf,
    pub body: ArtifactBody,
}

impl Artifact {
    /// 托管清单/前置比较用的条目键。
    pub fn item_key(&self) -> String {
        let suffix = match &self.body {
            ArtifactBody::JsonPointer { pointer, .. } => format!("#json:{pointer}"),
            ArtifactBody::TomlTable { table, .. } => format!("#toml:{table}"),
            ArtifactBody::Fragment { .. } => format!("#fragment:{}", self.resource_id),
            ArtifactBody::TomlArrayEntry {
                table,
                key_field,
                entry,
            } => {
                let kv = entry
                    .get(key_field)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                format!("#tomlarr:{table}:{key_field}:{kv}")
            }
            ArtifactBody::JsonArrayMerge {
                pointer, signature, ..
            } => {
                format!("#jsonmerge:{pointer}:{signature}")
            }
            ArtifactBody::Full { .. } => String::new(),
            ArtifactBody::Symlink { .. } => "#symlink".into(),
            ArtifactBody::ExternalSymlink { .. } => "#external-link".into(),
        };
        format!("{}{suffix}", self.path.display())
    }

    /// 期望内容哈希（稳定 canonical 序列化）。
    pub fn desired_hash(&self) -> Result<String> {
        let payload: Vec<u8> = match &self.body {
            ArtifactBody::ExternalSymlink { target } => {
                format!("external|{}", target.display()).into_bytes()
            }
            ArtifactBody::Full { content } => content.clone().into_bytes(),
            ArtifactBody::JsonPointer { value, .. } => serde_json::to_vec(value)
                .map_err(|e| Error::new(code::RENDER_FAILED, format!("JSON 序列化失败: {e}")))?,
            ArtifactBody::TomlTable { value, .. }
            | ArtifactBody::TomlArrayEntry { entry: value, .. } => serde_json::to_vec(value)
                .map_err(|e| Error::new(code::RENDER_FAILED, format!("TOML 值规范化失败: {e}")))?,
            ArtifactBody::JsonArrayMerge { value, .. } => serde_json::to_vec(value)
                .map_err(|e| Error::new(code::RENDER_FAILED, format!("JSON 序列化失败: {e}")))?,
            ArtifactBody::Fragment { content } => content.clone().into_bytes(),
            ArtifactBody::Symlink {
                target,
                content_digest,
                ..
            } => format!("{}|{content_digest}", target.display()).into_bytes(),
        };
        Ok(format!("sha256:{}", crate::ids::sha256_hex(&payload)))
    }

    pub fn validate(&self) -> Result<()> {
        if self.path.is_absolute() || self.path.components().any(|c| c.as_os_str() == "..") {
            return Err(Error::new(
                code::PATH_TRAVERSAL,
                format!("产物路径非法: {}", self.path.display()),
            )
            .context(serde_json::json!({ "resource": self.resource_id })));
        }
        if let Some((pointer, signature)) = match &self.body {
            ArtifactBody::JsonPointer { pointer, .. } => Some((pointer, None)),
            ArtifactBody::JsonArrayMerge {
                pointer, signature, ..
            } => Some((pointer, Some(signature))),
            _ => None,
        } {
            if !pointer.starts_with('/') {
                return Err(Error::new(
                    code::RENDER_FAILED,
                    format!("JSON pointer 必须以 / 开头: {pointer}"),
                ));
            }
            if let Some(sig) = signature {
                if sig.is_empty() {
                    return Err(Error::new(
                        code::RENDER_FAILED,
                        "JsonArrayMerge 托管签名不能为空",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// 读取文本文件的托管片段；无标记时返回 None。
pub fn read_fragment(file_text: &str, resource_id: &str) -> Option<String> {
    let begin = fragment_begin(resource_id);
    let end = fragment_end(resource_id);
    let start = file_text.find(&begin)? + begin.len();
    let stop = file_text[start..].find(&end)? + start;
    // upsert_fragment 只在正文两侧各插入一个分隔换行；正文自身的换行参与哈希。
    let content = &file_text[start..stop];
    let content = content.strip_prefix('\n').unwrap_or(content);
    Some(content.strip_suffix('\n').unwrap_or(content).to_string())
}

/// 从文本中移除托管片段（含标记行），返回 (新文本, 是否存在)。
pub fn remove_fragment(file_text: &str, resource_id: &str) -> (String, bool) {
    let begin = fragment_begin(resource_id);
    let end = fragment_end(resource_id);
    let Some(start) = file_text.find(&begin) else {
        return (file_text.to_string(), false);
    };
    let Some(rel_end) = file_text[start..].find(&end) else {
        return (file_text.to_string(), false);
    };
    let stop = start + rel_end + end.len();
    let mut out = String::new();
    out.push_str(&file_text[..start]);
    // 吃掉片段后紧跟的换行，避免残留空行
    let rest = &file_text[stop..];
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    out.push_str(rest);
    (out, true)
}

/// 在文本中插入/替换托管片段（幂等：重复同步不产生重复片段）。
pub fn upsert_fragment(file_text: &str, resource_id: &str, content: &str) -> String {
    let begin = fragment_begin(resource_id);
    let end = fragment_end(resource_id);
    let block = format!("{begin}\n{content}\n{end}");
    if file_text.contains(&begin) {
        let (cleaned, _) = remove_fragment(file_text, resource_id);
        return append_block(&cleaned, &block);
    }
    append_block(file_text, &block)
}

fn append_block(text: &str, block: &str) -> String {
    if text.is_empty() {
        return format!("{block}\n");
    }
    let mut out = text.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(block);
    out.push('\n');
    out
}

/// JSON 文件当前 pointer 处的值哈希；文件/指针缺失 → None。
pub fn current_json_entry(file: &Path, pointer: &str) -> Result<Option<String>> {
    if !file.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(file)?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        Error::new(
            code::USER_CONTENT_CONFLICT,
            format!("结构化配置解析失败（保留原文件，不自动改写）: {e}"),
        )
        .context(serde_json::json!({ "file": file.display().to_string() }))
    })?;
    Ok(value.pointer(pointer).map(|v| {
        format!(
            "sha256:{}",
            crate::ids::sha256_hex(serde_json::to_vec(v).unwrap_or_default().as_slice())
        )
    }))
}

/// 通用“目标处理结果”占位（导出等场景）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SkippedLike {
    pub path: String,
    pub reason: String,
}

/// 从资源原文提取 `targets` 列表；未声明 → None（对所有启用工具发布）。
pub fn resource_targets(raw: Option<&str>) -> Option<Vec<String>> {
    let text = raw?;
    // markdown frontmatter
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Ok(map) =
            serde_yaml::from_str::<serde_yaml::Value>(rest.split("\n---").next().unwrap_or(""))
        {
            if let Some(t) = map.get("targets") {
                return Some(
                    t.as_sequence()
                        .map(|s| {
                            s.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                );
            }
            return None;
        }
    }
    // toml
    if let Ok(v) = toml::from_str::<toml::Value>(text) {
        if let Some(t) = v.get("targets") {
            return Some(
                t.as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            );
        }
    }
    None
}

/// 从原文提取 frontmatter/toml 中的字符串字段。
pub fn raw_string_field(raw: Option<&str>, field: &str) -> Option<String> {
    let text = raw?;
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Ok(map) =
            serde_yaml::from_str::<serde_yaml::Value>(rest.split("\n---").next().unwrap_or(""))
        {
            return map.get(field).and_then(|v| v.as_str().map(str::to_string));
        }
    }
    if let Ok(v) = toml::from_str::<toml::Value>(text) {
        return v.get(field).and_then(|v| v.as_str().map(str::to_string));
    }
    None
}

/// Codex skills.config 中 AILoom 托管条目的旧 path 前缀（AIL-041 迁移前部署）。
pub const CODEX_MANAGED_SKILL_PREFIX: &str = ".ailoom/skills/";

/// Codex 项目技能原生发现目录（AIL-041 官方文档复核：learn.chatgpt.com/docs/build-skills，
/// 2026-09-16；宿主从 CWD 向上扫描至仓库根，支持符号链接）。
pub const CODEX_NATIVE_SKILLS_DIR: &str = ".agents/skills";

/// 判断 skills.config 条目是否为 AILoom 托管（旧前缀或新原生路径形态）。
pub fn is_codex_managed_config_path(path: &str) -> bool {
    if path.starts_with(CODEX_MANAGED_SKILL_PREFIX) {
        return true;
    }
    // 新形态：.agents/skills/<name>/SKILL.md（官方 skills.config path 指向 SKILL.md）
    path.starts_with(&format!("{CODEX_NATIVE_SKILLS_DIR}/")) && path.ends_with("/SKILL.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_roundtrip_preserves_user_text() {
        let user = "# 我的文件\n\n自定义说明\n";
        let with = upsert_fragment(user, "team/rule/common/std", "- 规范一\n- 规范二");
        assert!(with.contains(&fragment_begin("team/rule/common/std")));
        assert!(with.contains("自定义说明"));
        let again = upsert_fragment(&with, "team/rule/common/std", "- 新规范");
        assert!(again.contains("新规范"));
        assert!(!again.contains("规范一"), "重复同步不产生重复片段");
        assert_eq!(again.matches("BEGIN AILOOM").count(), 1);
        let (removed, existed) = remove_fragment(&again, "team/rule/common/std");
        assert!(existed);
        assert!(!removed.contains("AILOOM"));
        assert!(removed.contains("自定义说明"));
    }

    #[test]
    fn fragment_roundtrip_preserves_payload_boundary_newlines() {
        let id = "personal/rule/personal/chinese";
        for content in ["", "\n", "规则正文", "规则正文\n", "\n规则正文\n\n"] {
            let file = upsert_fragment("用户前文\n", id, content);
            assert_eq!(read_fragment(&file, id).as_deref(), Some(content));
        }
    }

    #[test]
    fn fragment_markers_contain_resource_id() {
        assert!(fragment_begin("a/b/c/d").contains("a/b/c/d"));
    }
}
