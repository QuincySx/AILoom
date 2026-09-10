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

/// 稳定经验 ID：`learn-<sha256(title\0body)[0..16]>`，不依赖创建时间与顺序。
pub fn stable_id(doc: &LearningDoc) -> String {
    let payload = format!("{}\0{}", doc.title, doc.body.trim());
    format!(
        "learn-{}",
        crate::ids::sha256_prefix(payload.as_bytes(), 16)
    )
}

/// 渲染为源仓库内的经验文档（frontmatter 完整）。
pub fn render_source_file(
    doc: &LearningDoc,
    id: &str,
    target: &LearningTarget,
    namespace: &str,
) -> String {
    let mut fm = String::from("---\n");
    fm.push_str(&format!("name: {id}\n"));
    fm.push_str(&format!("title: {}\n", doc.title));
    fm.push_str(&format!("description: {}\n", doc.description));
    match target {
        LearningTarget::Shared => {
            fm.push_str("shared: true\n");
        }
        LearningTarget::Project(p) => {
            fm.push_str(&format!("project: {p}\nshared: false\n"));
        }
    }
    fm.push_str(&format!("namespace: {namespace}\n"));
    if let Some(src) = &doc.source_ref {
        fm.push_str(&format!("source_ref: \"{src}\"\n"));
    }
    if !doc.tags.is_empty() {
        fm.push_str(&format!("tags: [{}]\n", doc.tags.join(", ")));
    }
    fm.push_str("---\n\n");
    fm.push_str(&format!("# {}\n\n", doc.title));
    fm.push_str(doc.body.trim());
    fm.push('\n');
    fm
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
}
