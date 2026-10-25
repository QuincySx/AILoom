//! Project-owned, linked wiki pages. Immutable editions are committed by one index write.
use super::*;
use std::collections::BTreeSet;

const SECTIONS: &[&str] = &[
    "index",
    "overview",
    "topics",
    "maintenance",
    "questions",
    "sources",
    "changes",
    "validation",
];

#[derive(Clone, Serialize, Deserialize)]
pub struct Page {
    pub path: PathBuf,
    pub title: String,
    pub kind: String,
}

pub(super) fn base(project: &str) -> PathBuf {
    Path::new("wiki").join(project)
}

/// The entry carries the exact page set. Old editions never enter current recall.
pub(super) fn current(tree: &Tree, project: &str) -> Result<Vec<Page>> {
    let entry = base(project).join("index.md");
    let Some(bytes) = tree.get(&entry) else {
        return Ok(Vec::new());
    };
    let text = std::str::from_utf8(bytes).map_err(|_| fail("Wiki 索引不是 UTF-8 文本"))?;
    let (fm, _) =
        crate::resource::split_frontmatter(text)?.ok_or_else(|| fail("Wiki 索引缺少页面清单"))?;
    let meta: Value = serde_yaml::from_str(&fm)?;
    if meta["schema"] != "ailoom-project-wiki/v1" || meta["project_id"] != project {
        return Err(fail("Wiki 索引与项目身份不一致"));
    }
    let pages: Vec<Page> = if meta["pages"].is_array() {
        serde_json::from_value(meta["pages"].clone())?
    } else {
        let rel = PathBuf::from(
            meta["manifest"]
                .as_str()
                .ok_or_else(|| fail("Wiki 索引缺少页面清单"))?,
        );
        if !rel.starts_with("editions")
            || rel.components().any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(fail("Wiki 页面清单位置越界"));
        }
        serde_json::from_slice(
            tree.get(&base(project).join(rel))
                .ok_or_else(|| fail("Wiki 页面清单缺失"))?,
        )?
    };
    let mut seen = BTreeSet::new();
    for page in &pages {
        if !page.path.starts_with(base(project).join("editions"))
            || page
                .path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || page.path.extension().is_none_or(|e| e != "md")
            || !seen.insert(page.path.clone())
            || !tree.contains_key(&page.path)
        {
            return Err(fail("Wiki 页面清单包含越界、重复或缺失页面"));
        }
    }
    Ok(pages)
}

pub(super) fn project_name(root: &Path) -> String {
    root.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into()
}

/// Untagged legacy notes are accepted only for a library with one owner. Shared
/// libraries require an explicit owner or shared:true; UUID ownership is not a team label.
pub(super) fn source_owned(path: &Path, bytes: &[u8], project: &str, single_owner: bool) -> bool {
    if path.starts_with("wiki")
        || path.starts_with("resources")
        || path.starts_with("docs/organized")
    {
        return false;
    }
    if path.starts_with("projects") {
        return path.starts_with(Path::new("projects").join(project).join("learnings"));
    }
    if path.starts_with("imports") {
        return false;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        if let Ok(Some((fm, _))) = crate::resource::split_frontmatter(text) {
            if let Ok(meta) = serde_yaml::from_str::<Value>(&fm) {
                if meta["shared"] == true {
                    return true;
                }
                if let Some(owner) = meta["project_id"].as_str() {
                    return owner == project;
                }
                // Resource project labels are not portable knowledge UUIDs.
                if meta.get("project").is_some() || meta.get("projects").is_some() {
                    return false;
                }
            }
        }
    }
    single_owner
}

pub(super) fn single_owner(cat: &Catalog, binding: &Binding, tree: &Tree, project: &str) -> bool {
    if cat.projects.values().filter(|b| b.id == binding.id).count() > 1 {
        return false;
    }
    // A newly restored machine may know only one of the library's projects.
    for p in tree.keys() {
        let owner = p
            .strip_prefix("projects")
            .ok()
            .filter(|p| p.ends_with("state.json") || p.ends_with("project.json"))
            .or_else(|| {
                p.strip_prefix("imports/worktrees")
                    .ok()
                    .filter(|p| p.ends_with("manifest.json"))
            })
            .or_else(|| {
                p.strip_prefix("wiki")
                    .ok()
                    .filter(|p| p.ends_with("index.md"))
            });
        if let Some(owner) = owner
            .and_then(|p| p.components().next())
            .and_then(|c| c.as_os_str().to_str())
        {
            if owner != project && owner != "index.md" {
                return false;
            }
        }
    }
    true
}

fn markdown_name(name: &str) -> String {
    name.chars()
        .map(|c| if "[]()<>`*\\\r\n".contains(c) { ' ' } else { c })
        .collect()
}

/// Human navigation is derived from the authoritative local binding + portable identity.
/// It contains no machine paths and is not original evidence.
pub(super) fn catalog(tree: &Tree, cat: &Catalog, binding: &Binding) -> Result<Tree> {
    let mut files = Tree::new();
    let mut index = String::from("# 项目开发知识\n\n原始资料在 `imports/worktrees/<project-id>/`，项目经验在 `projects/<project-id>/learnings/`，整理结果在 `wiki/<project-id>/`。先按项目和主题阅读；原文、历史稿及未评估稿不等于当前约定。\n\n| 项目 | 整理知识 | 原始资料 | 项目关联 |\n|---|---|---|---|\n");
    let mut owners = BTreeSet::new();
    for (repo, b) in cat.projects.iter().filter(|(_, b)| b.id == binding.id) {
        let id = match ingest::project_key(repo, &b.project_root) {
            Ok(id) => id,
            Err(_) => continue,
        };
        if id.is_empty() || !owners.insert(id.clone()) {
            continue;
        }
        let name = project_name(&b.project_root);
        let metadata = Path::new("projects").join(&id).join("project.json");
        let entry = base(&id).join("index.md");
        let raw = Path::new(ingest::PREFIX).join(&id).join("index.md");
        let related = relations::related(cat, &id, &b.project_root);
        let relations_path = Path::new("projects").join(&id).join("relations.json");
        files.insert(
            relations_path,
            serde_json::to_vec_pretty(&relations::portable(&related))?,
        );
        let mut project_readme=format!("# {} · 项目关联\n\n项目 ID：`{id}`\n\n知识库 ID：`{}`\n\n本项目仓库的 `.ailoom/knowledge.json` 声明同一组身份；本机位置由 AILoom 的项目登记解析，名称和目录不是身份。\n\n[全部项目](../../wiki/index.md) · [原文与出处](../../{}) · [机器可读关联](project.json) · [依赖依据](relations.json)\n\n## 依赖项目\n\n依赖关系只提供导航，不合并两边知识。下列关系来自当前项目 package.json 的声明与已登记项目包名匹配；不证明当前安装版本、代码兼容性或业务采用情况。使用目标知识前核对版本、分支、接口及评估状态；不能直接把目标项目约定套到本项目。\n\n",markdown_name(&name),binding.id,raw.display());
        if related.projects.is_empty() {
            project_readme.push_str(
                "未发现可匹配的 npm 包依赖项目；这不表示没有其他语言、运行时或未登记依赖。\n",
            );
        }
        for warning in &related.warnings {
            project_readme.push_str(&format!("- {}\n", markdown_name(warning)));
        }
        for related in &related.projects {
            let target = base(&related.project_id).join("index.md");
            let target = if tree.contains_key(&target) {
                target
            } else {
                Path::new("projects")
                    .join(&related.project_id)
                    .join("README.md")
            };
            let label = if related.knowledge_id == binding.id {
                format!(
                    "[{}](../../{})",
                    markdown_name(&related.name),
                    target.display()
                )
            } else {
                format!(
                    "{}（另一知识库 `{}`，通过 AILoom 的 related_projects 按项目 ID 解析位置）",
                    markdown_name(&related.name),
                    related.knowledge_id
                )
            };
            project_readme.push_str(&format!("### {label}\n\n目标项目 ID：`{}`。对方尚未整理时入口仅说明身份与资料状态。\n\n| 依赖包 | 声明版本 | 本项目依据 | 对方包依据 | 对方工作副本版本 |\n|---|---|---|---|---|\n",related.project_id));
            for package in &related.packages {
                project_readme.push_str(&format!(
                    "| `{}` | `{}` | `{}:{}` | `{}` | `{}` |\n",
                    markdown_name(&package.package),
                    markdown_name(&package.required),
                    package.manifest.display(),
                    package.field,
                    package.provider_manifest.display(),
                    markdown_name(package.provider_version.as_deref().unwrap_or("未记录"))
                ));
            }
        }
        files.insert(
            Path::new("projects").join(&id).join("README.md"),
            project_readme.into_bytes(),
        );
        files.insert(
            metadata.clone(),
            serde_json::to_vec_pretty(&json!({
                "schema_version":1,"project_id":id,"knowledge_id":binding.id,
                "name":name,"declaration":".ailoom/knowledge.json",
                "raw_sources":Path::new(ingest::PREFIX).join(&id),
                "notes":Path::new("projects").join(&id).join("learnings"),"wiki":entry,
            "relations":Path::new("projects").join(&id).join("relations.json"),"guide":Path::new("projects").join(&id).join("README.md"),
            }))?,
        );
        let knowledge = if tree.contains_key(&entry) {
            format!("[模块与主题]({id}/index.md)")
        } else {
            "尚未整理".into()
        };
        index.push_str(&format!(
            "| {} | {knowledge} | [原文与出处](../{}) | [关联信息](../{}) |\n",
            markdown_name(&name),
            raw.display(),
            Path::new("projects").join(&id).join("README.md").display()
        ));
        let origins = ingest::current_documents(tree, &id)?;
        let mut raw_index = format!("# {} · 原始资料\n\n项目 ID：`{id}`。知识库 ID：`{}`。\n\n这些文件是不可变的原文快照，供溯源；不代表已整理、已验证或适用于当前分支。\n\n[项目目录](../../../wiki/index.md)\n\n", markdown_name(&name), binding.id);
        for (path, values) in origins {
            raw_index.push_str(&format!(
                "- [原文 {}]({})：{}\n",
                path.file_stem().unwrap_or_default().to_string_lossy(),
                path.file_name().unwrap_or_default().to_string_lossy(),
                values
                    .iter()
                    .map(|o| format!(
                        "{} / `{}`（分支：{}；提交：{}；未提交修改：{}）",
                        markdown_name(&o.directory),
                        markdown_name(&o.document.to_string_lossy()),
                        markdown_name(o.branch.as_deref().unwrap_or("未记录")),
                        o.commit.as_deref().unwrap_or("未记录"),
                        o.worktree_dirty
                    ))
                    .collect::<Vec<_>>()
                    .join("；")
            ));
        }
        files.insert(raw, raw_index.into_bytes());
    }
    files.insert(PathBuf::from("wiki/index.md"), index.into_bytes());
    Ok(files)
}

fn code_line(line: &str, fence: &mut Option<(char, usize)>) -> bool {
    let trimmed = line.trim_start();
    let mark = trimmed.chars().next().unwrap_or(' ');
    let length = trimmed.chars().take_while(|c| *c == mark).count();
    if let Some((active, size)) = *fence {
        if mark == active && length >= size && trimmed[length..].trim().is_empty() {
            *fence = None;
        }
        return true;
    }
    if line.len() - trimmed.len() <= 3 && ['`', '~'].contains(&mark) && length >= 3 {
        *fence = Some((mark, length));
        return true;
    }
    false
}

fn sections(report: &str) -> Result<BTreeMap<String, String>> {
    let mut parts = BTreeMap::<String, String>::new();
    let mut current = None;
    let mut fence = None;
    for line in report.lines() {
        if !code_line(line, &mut fence) && line.starts_with("## ") {
            let key = line.trim_start_matches("## ").trim();
            if !SECTIONS.contains(&key) || parts.contains_key(key) {
                return Err(fail("整理稿包含未知或重复章节，请重新整理"));
            }
            parts.insert(key.into(), String::new());
            current = Some(key.to_string());
        } else if let Some(key) = &current {
            let body = parts.get_mut(key).unwrap();
            body.push_str(line);
            body.push('\n');
        }
    }
    if SECTIONS.iter().any(|s| !parts.contains_key(*s)) {
        return Err(fail("整理稿缺少索引、主题、来源或自检章节，未替换原有知识"));
    }
    Ok(parts)
}

fn topics(body: &str) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::<String, String>::new();
    let mut key = None;
    let mut fence = None;
    for line in body.lines() {
        if !code_line(line, &mut fence) && line.starts_with("### topic-") {
            let id = line.trim_start_matches("### ").trim();
            if id.len() > 100
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                || result.contains_key(id)
            {
                return Err(fail("主题 ID 无效或重复"));
            }
            result.insert(id.into(), String::new());
            key = Some(id.to_string());
        } else if let Some(k) = &key {
            let text = result.get_mut(k).unwrap();
            text.push_str(line);
            text.push('\n');
        } else if !line.trim().is_empty() {
            return Err(fail("主题正文必须放在独立 topic 标题下"));
        }
    }
    if result.len() > 256
        || result
            .values()
            .any(|t| t.trim().is_empty() || t.len() > 32 * 1024)
    {
        return Err(fail("主题为空或超过 32 KB，请按模块或独立问题继续拆分"));
    }
    Ok(result)
}

fn relative(from: &Path, to: &Path) -> String {
    let a: Vec<_> = from.parent().unwrap().components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let mut p = PathBuf::new();
    for _ in common..a.len() {
        p.push("..");
    }
    for c in &b[common..] {
        p.push(c.as_os_str());
    }
    p.to_string_lossy().replace('\\', "/")
}

/// Rebase only Markdown destinations, keeping code/prose and source claims intact.
fn links(
    text: &str,
    from: &Path,
    targets: &BTreeMap<String, PathBuf>,
    sources: &[PathBuf],
    tree: &Tree,
) -> Result<String> {
    let mut text = text.to_string();
    // Legacy v1 ledger used `D/<hash>.md` instead of clickable source links.
    for p in sources {
        let short = format!("`D/{}`", p.file_name().unwrap().to_string_lossy());
        text = text.replace(&short, &format!("[原文](../../{})", p.display()));
    }
    let mut out = String::new();
    let mut fence = None;
    for line in text.split_inclusive('\n') {
        if code_line(line.trim_end_matches('\n'), &mut fence) {
            out.push_str(line);
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut code = None;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                let n = bytes[i..].iter().take_while(|c| **c == b'`').count();
                if code == Some(n) {
                    code = None;
                } else if code.is_none() {
                    code = Some(n);
                }
                out.push_str(&line[i..i + n]);
                i += n;
                continue;
            }
            if code.is_none() && bytes[i] == b'\\' && i + 1 < bytes.len() {
                let n = line[i + 1..].chars().next().unwrap().len_utf8() + 1;
                out.push_str(&line[i..i + n]);
                i += n;
                continue;
            }
            if code.is_none() && line[i..].starts_with("](") {
                out.push_str("](");
                i += 2;
                let end = line[i..]
                    .find(')')
                    .ok_or_else(|| fail("整理稿的链接未闭合"))?
                    + i;
                let dest = &line[i..end];
                let target = if let Some(anchor) = dest.strip_prefix('#') {
                    let p = targets.get(anchor).ok_or_else(|| {
                        fail(format!("整理稿包含不存在的主题或章节链接：{anchor}"))
                    })?;
                    format!("{}#{anchor}", relative(from, p))
                } else if let Some(p) = dest.strip_prefix("../../") {
                    let (p, fragment) = p.split_once('#').map_or((p, None), |(p, f)| (p, Some(f)));
                    let p = PathBuf::from(p);
                    if p.components().any(|c| !matches!(c, Component::Normal(_)))
                        || !sources.contains(&p)
                        || !tree.contains_key(&p)
                    {
                        return Err(fail("整理稿引用了本项目来源清单以外的文件"));
                    }
                    format!(
                        "{}{}",
                        relative(from, &p),
                        fragment.map(|f| format!("#{f}")).unwrap_or_default()
                    )
                } else {
                    return Err(fail(format!("整理稿包含不受支持的链接：{dest}")));
                };
                out.push_str(&target);
                out.push(')');
                i = end + 1;
                continue;
            }
            let c = line[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    Ok(out)
}

fn topic_title(id: &str, body: &str) -> String {
    body.lines()
        .find(|l| l.contains("主题") && l.contains('：'))
        .and_then(|l| l.split_once('：').map(|(_, s)| s))
        .map(|s| {
            s.split(['。', '；', '\n'])
                .next()
                .unwrap_or(s)
                .trim_matches(['*', ' '])
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.to_string())
}

fn citations(text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'S' && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j - i >= 4 && (j == bytes.len() || !bytes[j].is_ascii_alphanumeric()) {
                ids.insert(text[i..j].into());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    ids
}

pub(super) fn compile(input: &OrganizationInput, report: &str, tree: &Tree) -> Result<Tree> {
    let mut parts = sections(report)?;
    let topic_parts = topics(&parts["topics"])?;
    let revision = crate::ids::sha256_hex(&serde_json::to_vec(&json!({
        "format":2,"report":report,"input":input.fingerprint,"project_name":input.project_name,
    }))?);
    let edition = base(&input.project_id).join("editions").join(&revision);
    let entry = base(&input.project_id).join("index.md");
    let mut targets = BTreeMap::new();
    for key in SECTIONS
        .iter()
        .filter(|s| !["index", "topics", "overview", "validation"].contains(s))
    {
        targets.insert(key.to_string(), edition.join(format!("{key}.md")));
    }
    for id in topic_parts.keys() {
        targets.insert(id.clone(), edition.join("topics").join(format!("{id}.md")));
    }
    for key in ["index", "topics", "overview", "validation"] {
        targets.insert(key.into(), entry.clone());
    }
    let source_ids: BTreeSet<_> = citations(&parts["sources"])
        .into_iter()
        .filter(|id| {
            parts["sources"].contains(&format!("| {id} "))
                || parts["sources"].contains(&format!("id=\"source-{}\"", id.to_ascii_lowercase()))
        })
        .collect();
    for id in &source_ids {
        let anchor = format!("source-{}", id.to_ascii_lowercase());
        targets.insert(anchor.clone(), targets["sources"].clone());
        if !parts["sources"].contains(&format!("id=\"{anchor}\"")) {
            let body = parts.get_mut("sources").unwrap();
            // Preserve the recorded title, processing depth and all source claims.
            *body = body.replacen(
                &format!("| {id} "),
                &format!("| <a id=\"{anchor}\"></a>{id} "),
                1,
            );
        }
    }
    let mut files = Tree::new();
    let mut pages = Vec::new();
    for (id, body) in &topic_parts {
        let path = targets[id].clone();
        let title = topic_title(id, body);
        let mut body = links(body, &path, &targets, &input.documents, tree)?;
        let cited = citations(&body);
        if !cited.is_empty() {
            body.push_str("\n\n## 本页来源定位\n\n");
            for id in cited.intersection(&source_ids) {
                body.push_str(&format!(
                    "- [{id}]({}#source-{})\n",
                    relative(&path, &targets["sources"]),
                    id.to_ascii_lowercase()
                ));
            }
        }
        let content=format!("---\nschema: ailoom-wiki-topic/v1\nproject_id: {}\nknowledge_id: {}\nreview_status: unreviewed\n---\n\n# {}\n\n<a id=\"{id}\"></a>\n\n[项目索引]({}) · [来源目录]({})\n\n{body}",input.project_id,input.knowledge_id,markdown_name(&title),relative(&path,&entry),relative(&path,&targets["sources"]));
        files.insert(path.clone(), content.into_bytes());
        pages.push(Page {
            path,
            title,
            kind: "topic".into(),
        });
    }
    for (key, title) in [
        ("maintenance", "维护建议"),
        ("questions", "待核实问题"),
        ("sources", "来源与阅读范围"),
        ("changes", "本轮变更"),
    ] {
        let path = targets[key].clone();
        let body = links(&parts[key], &path, &targets, &input.documents, tree)?;
        let mut content = format!(
            "# {} · {title}\n\n<a id=\"{key}\"></a>\n\n[项目索引]({})\n\n{body}",
            markdown_name(&input.project_name),
            relative(&path, &entry)
        );
        if key == "sources" {
            content.push_str(
                "\n## 原文快照入口\n\n以下清单由应用按项目归属生成；阅读深度以编纂记录为准。\n\n",
            );
            for p in &input.documents {
                content.push_str(&format!("- [{}]({})\n", p.display(), relative(&path, p)));
            }
        }
        files.insert(path.clone(), content.into_bytes());
        pages.push(Page {
            path,
            title: title.into(),
            kind: key.into(),
        });
    }
    if parts["index"].len() + parts["overview"].len() + parts["validation"].len() > 24 * 1024 {
        return Err(fail("项目索引超过 24 KB，请将正文移至模块主题页"));
    }
    let manifest = edition.join("pages.json");
    files.insert(manifest.clone(), serde_json::to_vec_pretty(&pages)?);
    let fm = serde_yaml::to_string(
        &json!({"schema":"ailoom-project-wiki/v1","project_id":input.project_id,"knowledge_id":input.knowledge_id,"review_status":"unreviewed","manifest":manifest.strip_prefix(base(&input.project_id))?}),
    )?;
    let index = links(&parts["index"], &entry, &targets, &input.documents, tree)?;
    let overview = links(&parts["overview"], &entry, &targets, &input.documents, tree)?;
    let validation = links(
        &parts["validation"],
        &entry,
        &targets,
        &input.documents,
        tree,
    )?;
    let mut content=format!("---\n{fm}---\n\n# {} · 开发知识\n\n项目 ID：`{}` · 知识库 ID：`{}`\n\n[全部项目](../index.md) · [原文与出处](../../imports/worktrees/{}/index.md) · [项目关联与依赖](../../projects/{}/README.md)\n\n<a id=\"overview\"></a>\n\n{overview}\n\n<a id=\"index\"></a><a id=\"topics\"></a>\n\n{index}\n\n## 更多入口\n\n",markdown_name(&input.project_name),input.project_id,input.knowledge_id,input.project_id,input.project_id);
    for p in pages.iter().filter(|p| p.kind != "topic") {
        content.push_str(&format!(
            "- [{}]({})\n",
            markdown_name(&p.title),
            relative(&entry, &p.path)
        ));
    }
    content.push_str(&format!(
        "\n<a id=\"validation\"></a>\n\n## 自检与覆盖边界\n\n{validation}"
    ));
    files.insert(entry, content.into_bytes());
    Ok(files)
}
