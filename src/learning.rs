//! 经验文档解析与归属校验（AIL-015）。契约：learning 只能 project 或 shared；
//! 多项目绑定必须显式指定目标；稳定 ID 不依赖标题。

use crate::error::{code, Error, Result};
use crate::resource::split_frontmatter;
use serde::{Deserialize, Serialize};
use serde_yaml::Value as Yaml;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningDoc {
    /// 人类可读标题（frontmatter title，缺省用 name）
    pub title: String,
    /// 交给团队的唯一名（name 字段或由内容哈希生成）
    pub name: Option<String>,
    pub description: String,
    pub source_ref: Option<String>,
    pub tags: Vec<String>,
    /// 正文（不含 frontmatter）
    pub body: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawLearning {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    source_ref: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

/// 解析并基础校验经验文档：UTF-8 由调用方保证；标题、正文非空。
pub fn parse(text: &str) -> Result<LearningDoc> {
    let (meta_yaml, body) = match split_frontmatter(text)? {
        Some(pair) => pair,
        None => (String::new(), text.to_string()),
    };
    let raw: RawLearning = serde_yaml::from_str::<Yaml>(&meta_yaml)
        .ok()
        .and_then(|v| serde_json::from_value(serde_json::to_value(&v).ok()?).ok())
        .unwrap_or_default();
    let title = raw
        .title
        .clone()
        .or_else(|| raw.name.clone())
        .unwrap_or_default();
    let body_trimmed = body.trim();
    if title.trim().is_empty() {
        return Err(Error::new(
            code::LEARNING_SCOPE_INVALID,
            "经验缺少标题（frontmatter title 或 name）",
        )
        .fix("在文档 frontmatter 增加 title: <标题>"));
    }
    if body_trimmed.is_empty() {
        return Err(Error::new(code::LEARNING_SCOPE_INVALID, "经验正文为空"));
    }
    Ok(LearningDoc {
        title: title.trim().to_string(),
        name: raw.name,
        description: raw.description.unwrap_or_else(|| title.trim().to_string()),
        source_ref: raw.source_ref,
        tags: raw.tags.unwrap_or_default(),
        body: body.to_string(),
    })
}

/// 稳定经验 ID：文档 frontmatter 已带 `name`（learn-…）则直接沿用——标题/正文
/// 修改不产生新身份；否则按当前内容铸造 `learn-<sha256(title\0body)[0..16]>`，
/// 由贡献链路把铸造结果写回草稿（见 `persist_identity`）。
/// 兼容性：铸造格式与旧内容哈希 ID 完全一致，同内容首次重算得到相同 ID，
/// 既有团队源文件与 learnings.json 记录无需迁移。
pub fn stable_id(doc: &LearningDoc) -> String {
    if let Some(name) = doc.name.as_deref() {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let payload = format!("{}\0{}", doc.title, doc.body.trim());
    format!(
        "learn-{}",
        crate::ids::sha256_prefix(payload.as_bytes(), 16)
    )
}

/// 把稳定 ID 写回草稿 frontmatter 的 `name` 字段（幂等；保留其余键）。
/// 无 frontmatter 或 frontmatter 不是映射时原样返回（parse 已在此前拒绝缺标题草稿）。
pub fn persist_identity(text: &str, id: &str) -> Result<String> {
    let Some((meta_yaml, body)) = split_frontmatter(text)? else {
        return Ok(text.to_string());
    };
    let mut value: Yaml = serde_yaml::from_str(&meta_yaml).map_err(|e| {
        Error::new(
            code::LEARNING_SCOPE_INVALID,
            format!("草稿 frontmatter 解析失败: {e}"),
        )
    })?;
    let Some(mapping) = value.as_mapping_mut() else {
        return Ok(text.to_string());
    };
    mapping.insert(Yaml::from("name"), Yaml::from(id));
    let serialized = serde_yaml::to_string(&value).map_err(|e| {
        Error::new(
            code::LEARNING_SCOPE_INVALID,
            format!("frontmatter 序列化失败: {e}"),
        )
    })?;
    Ok(format!("---\n{serialized}---\n{body}"))
}

/// 渲染为源仓库内的经验文档（frontmatter 用 serde_yaml 规范序列化，
/// 冒号/引号/换行/中文等字符都被正确转义；调用方在提交前按资源契约复验）。
pub fn render_source_file(
    doc: &LearningDoc,
    id: &str,
    target: &LearningTarget,
    namespace: &str,
) -> String {
    let mut fm = serde_yaml::Mapping::new();
    fm.insert(Yaml::from("name"), Yaml::from(id));
    fm.insert(Yaml::from("title"), Yaml::from(doc.title.as_str()));
    fm.insert(
        Yaml::from("description"),
        Yaml::from(doc.description.as_str()),
    );
    match target {
        LearningTarget::Shared => {
            fm.insert(Yaml::from("shared"), Yaml::from(true));
        }
        LearningTarget::Project(p) => {
            fm.insert(Yaml::from("project"), Yaml::from(p.as_str()));
            fm.insert(Yaml::from("shared"), Yaml::from(false));
        }
    }
    fm.insert(Yaml::from("namespace"), Yaml::from(namespace));
    if let Some(src) = &doc.source_ref {
        fm.insert(Yaml::from("source_ref"), Yaml::from(src.as_str()));
    }
    if !doc.tags.is_empty() {
        fm.insert(
            Yaml::from("tags"),
            Yaml::from(doc.tags.iter().map(String::as_str).collect::<Vec<_>>()),
        );
    }
    let serialized = serde_yaml::to_string(&fm).unwrap_or_default();
    // 标题中的换行不能进入正文一级标题；压平成空格仅影响展示，frontmatter 保留原文
    let heading = doc.title.replace(['\r', '\n'], " ");
    let mut out = String::from("---\n");
    out.push_str(&serialized);
    out.push_str("---\n\n");
    out.push_str(&format!("# {heading}\n\n"));
    out.push_str(doc.body.trim());
    out.push('\n');
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum LearningTarget {
    Shared,
    Project(String),
}

/// 归属决策：显式参数 > 唯一活跃项目默认；多项目未指定 → E6002，绝不静默共享。
pub fn decide_target(
    explicit_project: Option<&str>,
    explicit_shared: bool,
    active_projects: &[String],
) -> Result<LearningTarget> {
    if explicit_shared {
        if let Some(p) = explicit_project {
            return Err(Error::new(
                code::LEARNING_SCOPE_INVALID,
                format!("--shared 与 --project `{p}` 不能同时使用"),
            ));
        }
        return Ok(LearningTarget::Shared);
    }
    if let Some(p) = explicit_project {
        return Ok(LearningTarget::Project(p.to_string()));
    }
    match active_projects {
        [only] => Ok(LearningTarget::Project(only.clone())),
        [] => Err(Error::new(
            code::LEARNING_SCOPE_INVALID,
            "当前工作区没有绑定项目：共享必须显式 --shared",
        )),
        many => Err(Error::new(
            code::LEARNING_SCOPE_INVALID,
            format!(
                "绑定多个项目 {:?} 时必须显式指定 --project 或 --shared，不能静默归入任一项目",
                many
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_requires_title_and_body() {
        assert_eq!(parse("---\ntitle: A\n---\n\n正文\n").unwrap().title, "A");
        assert!(parse("---\ndescription: 无标题\n---\n\n正文\n").is_err());
        let doc = parse("---\ntitle: A\n---\n\n   \n").unwrap_err();
        assert_eq!(doc.code, "E6002");
    }

    #[test]
    fn stable_id_ignores_whitespace_differences_only_in_body_trim() {
        let a = parse("---\ntitle: T\n---\n\nbody\n").unwrap();
        let b = parse("---\ntitle: T\n---\n\nbody\n\n").unwrap();
        assert_eq!(stable_id(&a), stable_id(&b));
        let c = parse("---\ntitle: Different\n---\n\nbody\n").unwrap();
        assert_ne!(stable_id(&a), stable_id(&c));
    }

    #[test]
    fn target_decision_matrix() {
        assert_eq!(
            decide_target(None, true, &["a".into(), "b".into()]).unwrap(),
            LearningTarget::Shared
        );
        assert_eq!(
            decide_target(Some("b"), false, &["a".into(), "b".into()]).unwrap(),
            LearningTarget::Project("b".into())
        );
        assert_eq!(
            decide_target(None, false, &["a".into()]).unwrap(),
            LearningTarget::Project("a".into())
        );
        assert!(decide_target(None, false, &["a".into(), "b".into()]).is_err());
        assert!(decide_target(None, false, &[]).is_err());
        assert!(decide_target(Some("a"), true, &["a".into()]).is_err());
    }

    /// R05 回归：冒号/单双引号/换行/中文的标题与描述 解析→渲染→重新解析 内容不丢，
    /// 且渲染产物能通过资源 frontmatter 解析器（不再是手拼非法 YAML）。
    #[test]
    fn render_roundtrip_preserves_hostile_strings() {
        let text = concat!(
            "---\n",
            "title: \"Fix: cache '单' \\\"双\\\" 中文\\n第二行\"\n",
            "description: \"desc: a: b\\n换行后\"\n",
            "source_ref: \"postmortem: 2026-09-12\"\n",
            "tags: [\"缓存\", \"incident: p1\"]\n",
            "---\n\n",
            "正文 body: a: b\n"
        );
        let doc = parse(text).unwrap();
        assert_eq!(doc.title, "Fix: cache '单' \"双\" 中文\n第二行");
        let rendered = render_source_file(
            &doc,
            "learn-abc",
            &LearningTarget::Project("a".into()),
            "common",
        );
        // 渲染产物按资源契约重新解析
        let (meta, _) =
            crate::resource::parse_frontmatter(&rendered).expect("渲染产物 frontmatter 必须可解析");
        let meta = meta.expect("渲染产物必须有 frontmatter");
        assert_eq!(meta.name.as_deref(), Some("learn-abc"));
        assert_eq!(meta.project.as_deref(), Some("a"));
        // 再按经验解析器重读：标题/描述/引用/标签逐字相等
        let doc2 = parse(&rendered).unwrap();
        assert_eq!(doc2.title, doc.title);
        assert_eq!(doc2.description, doc.description);
        assert_eq!(doc2.source_ref, doc.source_ref);
        assert_eq!(doc2.tags, doc.tags);
        // 渲染会补充一级标题，正文原文完整保留在其后
        assert!(
            doc2.body.trim().ends_with(doc.body.trim()),
            "正文内容不丢: {:?}",
            doc2.body
        );
        // 共享目标的布尔渲染同样可回读
        let rendered_shared =
            render_source_file(&doc, "learn-abc", &LearningTarget::Shared, "common");
        let (meta2, _) = crate::resource::parse_frontmatter(&rendered_shared).unwrap();
        assert_eq!(meta2.unwrap().shared, Some(true));
    }

    /// 稳定身份：frontmatter 带 name 时沿用；persist_identity 幂等写回且保留其余键。
    #[test]
    fn identity_prefers_name_and_persist_is_idempotent() {
        let draft = "---\ntitle: 第一版\ndescription: d\n---\n\nbody v1\n";
        let doc = parse(draft).unwrap();
        let minted = stable_id(&doc);

        let with_name = persist_identity(draft, &minted).unwrap();
        assert!(
            with_name.contains(&format!("name: {minted}")),
            "{with_name}"
        );
        // 写回后重新解析：同 ID
        let doc2 = parse(&with_name).unwrap();
        assert_eq!(stable_id(&doc2), minted);
        // 改标题/改正文：身份不变
        let edited = with_name
            .replace("body v1", "body v2")
            .replace("第一版", "改名后");
        let doc3 = parse(&edited).unwrap();
        assert_eq!(stable_id(&doc3), minted, "标题/正文修改不隐式创建新经验");
        // 幂等：再次写回内容不变（键序稳定）
        let again = persist_identity(&with_name, &minted).unwrap();
        assert_eq!(again, with_name);
        // 不同经验（无 name 且内容不同）不会误合并
        let other = parse("---\ntitle: 另一篇\n---\n\n别的经验\n").unwrap();
        assert_ne!(stable_id(&other), minted);
    }
}
