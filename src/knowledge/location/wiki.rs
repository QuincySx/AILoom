//! Plain Markdown pages with stable paths for people and agents.
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
    if layout::modern(tree) || tree.contains_key(Path::new(".ailoom/wiki.json")) {
        let mut pages = Vec::new();
        for (path, bytes) in tree {
            let Ok(relative) = path.strip_prefix("wiki") else {
                continue;
            };
            if path.extension().is_none_or(|e| e != "md")
                || (!layout::modern(tree) && relative == Path::new("index.md"))
                || relative
                    .components()
                    .next()
                    .and_then(|c| c.as_os_str().to_str())
                    .is_some_and(|c| uuid::Uuid::parse_str(c).is_ok() || c == "editions")
            {
                continue;
            }
            let text = std::str::from_utf8(bytes).map_err(|_| fail("Wiki 文档不是 UTF-8 文本"))?;
            if let Some((fm, _)) = crate::resource::split_frontmatter(text)? {
                let meta: Value = serde_yaml::from_str(&fm)?;
                if meta["project_id"].as_str().is_some_and(|id| id != project) {
                    return Err(fail("Wiki 文档属于其他仓库"));
                }
            }
            let stem = path.file_stem().unwrap().to_string_lossy();
            let kind = if stem.starts_with("review-") {
                "review".into()
            } else if ["sources", "changes", "maintenance", "questions", "review"]
                .contains(&stem.as_ref())
            {
                stem.to_string()
            } else {
                "topic".into()
            };
            let title = text
                .lines()
                .find_map(|l| l.strip_prefix("# "))
                .unwrap_or(&stem)
                .trim()
                .to_string();
            pages.push(Page {
                path: path.clone(),
                title,
                kind,
            });
        }
        return Ok(pages);
    }
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

pub(super) fn navigation(tree: &Tree, path: &Path) -> bool {
    path == Path::new("README.md")
        && (layout::modern(tree)
            || tree.contains_key(Path::new(".ailoom/wiki.json"))
            || tree.contains_key(Path::new(".ailoom/homepage.json")))
}

/// Untagged legacy notes are accepted only for a library with one owner. Shared
/// libraries require an explicit owner or shared:true; UUID ownership is not a team label.
pub(super) fn source_owned(path: &Path, bytes: &[u8], project: &str, single_owner: bool) -> bool {
    if path.starts_with("sources")
        && path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return false;
    }
    if path == Path::new("sources/README.md")
        || path.starts_with(".ailoom")
        || path.starts_with("wiki")
        || path.starts_with("resources")
        || path.starts_with("docs/organized")
    {
        return false;
    }
    if path.starts_with("sources/notes") {
        return true;
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
    if cat.projects.values().any(|b| {
        b.id == binding.id
            && b.project_root != binding.project_root
            && super::super::portable::read(&b.project_root)
                .ok()
                .flatten()
                .is_none_or(|d| d.project_id != project)
    }) {
        return false;
    }
    tree_single_owner(tree, project)
}

pub(super) fn tree_single_owner(tree: &Tree, project: &str) -> bool {
    // Persisted namespaces must also be checked when only one binding is restored.
    for (p, bytes) in tree {
        let owner = p
            .strip_prefix("projects")
            .ok()
            .or_else(|| p.strip_prefix("imports/worktrees").ok())
            .or_else(|| p.strip_prefix("wiki").ok())
            .and_then(|p| p.components().next())
            .and_then(|c| c.as_os_str().to_str());
        if owner.is_some_and(|id| {
            id != project
                && (p.starts_with("projects")
                    || p.starts_with("imports/worktrees")
                    || uuid::Uuid::parse_str(id).is_ok())
        }) {
            return false;
        }
        if !p.starts_with("sources")
            && !p.starts_with(".ailoom")
            && (owner.is_none() || p.starts_with("wiki"))
            && p.extension().is_some_and(|e| e == "md")
        {
            if let Ok(text) = std::str::from_utf8(bytes) {
                if let Ok(Some((fm, _))) = crate::resource::split_frontmatter(text) {
                    if let Ok(meta) = serde_yaml::from_str::<Value>(&fm) {
                        if meta["project_id"]
                            .as_str()
                            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok() && id != project)
                        {
                            return false;
                        }
                    }
                }
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
fn catalog_project(cat: &Catalog, binding: &Binding) -> Result<String> {
    let repo = cat
        .projects
        .iter()
        .find(|(_, b)| b.project_root == binding.project_root)
        .map(|(id, _)| id.as_str())
        .unwrap_or("");
    ingest::project_key(repo, &binding.project_root)
}

pub(super) fn identity_file(cat: &Catalog, binding: &Binding) -> Result<(PathBuf, Vec<u8>)> {
    let project = catalog_project(cat, binding)?;
    let name = project_name(&binding.project_root);
    let raw = layout::source_index_at(&binding.path, &project)
        .strip_prefix(&binding.path)?
        .to_path_buf();
    let metadata = layout::project_dir(&binding.path, &project).join("project.json");
    let declaration = super::super::portable::read(&binding.project_root)?;
    let urls = super::super::identity::repository_urls(
        &binding.project_root,
        declaration
            .as_ref()
            .map(|d| d.repository_urls.as_slice())
            .unwrap_or_default(),
    );
    Ok((
        metadata,
        serde_json::to_vec_pretty(
            &json!({"schema_version":1,"project_id":project,"knowledge_id":binding.id,"name":name,"repository_urls":urls,"declaration":".ailoom/knowledge.json","wiki":"README.md","raw_sources":raw}),
        )?,
    ))
}

pub(super) fn catalog(tree: &Tree, cat: &Catalog, binding: &Binding) -> Result<Tree> {
    let mut files = Tree::new();
    let (metadata, bytes) = identity_file(cat, binding)?;
    let project = catalog_project(cat, binding)?;
    let name = markdown_name(&project_name(&binding.project_root));
    let raw = layout::source_index(tree, &project);
    let guide = layout::project_dir(&binding.path, &project).join("README.md");
    files.insert(metadata, bytes);
    let instructions="直接编辑首页或 wiki/ 下的 Markdown 即可维护知识；新增文档会自动参与检索，无需填写清单。再次整理时 AI 会读取现有正文，整理期间的人工作改动会阻止覆盖。原文快照保留出处，编纂结论须核对适用条件与评估状态。\n\n需要其他仓库知识时：`ailoom --json knowledge --action locate --query '<仓库名、Git 地址或路径>'`。各仓库独立维护。";
    files.insert(
        guide,
        format!("# {name} · 维护说明\n\n{instructions}\n").into_bytes(),
    );
    if !tree.contains_key(Path::new("README.md")) {
        let mut home = format!(
            "# {name} · 开发知识\n\n[原始资料]({})\n\n{instructions}\n\n## 模块与主题\n\n",
            raw.display()
        );
        let pages = current(tree, &project)?;
        if pages.is_empty() {
            home.push_str("尚未整理。可直接在 wiki/ 新建 Markdown，或通过 AILoom 开始整理。\n");
        }
        for page in pages {
            home.push_str(&format!(
                "- [{}]({})\n",
                markdown_name(&page.title),
                markdown_path(&page.path)
            ));
        }
        files.insert("README.md".into(), home.into_bytes());
        files.insert(
            ".ailoom/homepage.json".into(),
            b"{\"generated\":true}".to_vec(),
        );
    }
    let mut raw_index=format!("# {name} · 原始资料\n\n[知识首页]({})\n\n保留原始记录和分支出处，供知识正文追溯。可在本目录补充笔记；哈希命名文件是不可变原文快照，日常维护请编辑 wiki/ 中的正文。\n\n",relative(&raw,Path::new("README.md")));
    for (path, origins) in ingest::current_documents(tree, &project)? {
        let label = origins
            .first()
            .map(|o| o.document.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        raw_index.push_str(&format!(
            "- [{}]({})",
            markdown_name(&label),
            relative(&raw, &path)
        ));
        for origin in origins {
            raw_index.push_str(&format!(
                " · {}（{}，提交 {}）",
                markdown_name(&origin.directory),
                markdown_name(origin.branch.as_deref().unwrap_or("分支未记录")),
                origin.commit.as_deref().unwrap_or("未记录")
            ));
        }
        raw_index.push('\n');
    }
    let origins = ingest::current_documents(tree, &project)?;
    for (path, bytes) in tree
        .iter()
        .filter(|(p, _)| !origins.contains_key(*p) && **p != raw && !navigation(tree, p))
    {
        if path.extension().is_some_and(|e| e == "md") && source_owned(path, bytes, &project, true)
        {
            let title = std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| text.lines().find_map(|line| line.strip_prefix("# ")))
                .unwrap_or("补充记录");
            raw_index.push_str(&format!(
                "- [{}]({})\n",
                markdown_name(title),
                relative(&raw, path)
            ));
        }
    }
    files.insert(raw, raw_index.into_bytes());
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
            let key = line.trim_start_matches("## ").trim().to_ascii_lowercase();
            if SECTIONS.contains(&key.as_str()) {
                parts.entry(key.clone()).or_default();
                current = Some(key);
            } else {
                // A nested/explanatory heading is content, not a reason to discard the draft.
                let key = current.get_or_insert_with(|| "overview".into());
                let body = parts.entry(key.clone()).or_default();
                body.push_str(line);
                body.push('\n');
            }
        } else if let Some(key) = &current {
            let body = parts.get_mut(key).unwrap();
            body.push_str(line);
            body.push('\n');
        }
    }
    if ["topics", "sources"]
        .iter()
        .any(|s| !parts.contains_key(*s))
    {
        return Err(fail("整理稿缺少主题或来源章节，需要 AI 补齐，原有知识保留"));
    }
    for key in SECTIONS {
        parts.entry(key.to_string()).or_insert_with(|| {
            if *key == "validation" {
                "本轮生成稿，尚未经独立评估。AI 未单列自检记录，覆盖及适用性仍需复核。\n".into()
            } else {
                String::new()
            }
        });
    }
    Ok(parts)
}

fn topics(body: &str) -> Result<(String, BTreeMap<String, String>)> {
    let mut fence = None;
    if !body
        .lines()
        .any(|line| !code_line(line, &mut fence) && line.starts_with("### topic-"))
    {
        return Err(fail(
            "AI 未生成知识主题，需要核对资料读取范围并形成可用主题；草稿保留",
        ));
    }
    let mut result = BTreeMap::<String, String>::new();
    let mut key = None;
    let mut fence = None;
    let mut introduction = String::new();
    for line in body.lines() {
        if !code_line(line, &mut fence) && line.starts_with("### topic-") {
            let id = line.trim_start_matches("### ").trim();
            if id.len() > 100
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                || result.contains_key(id)
            {
                return Err(fail(format!("主题 ID 无效或重复：{id}")));
            }
            result.insert(id.into(), String::new());
            key = Some(id.to_string());
        } else if let Some(k) = &key {
            let text = result.get_mut(k).unwrap();
            text.push_str(line);
            text.push('\n');
        } else {
            introduction.push_str(line);
            introduction.push('\n');
        }
    }
    if result.values().any(|t| t.trim().is_empty()) {
        return Err(fail("整理稿包含空主题，需要 AI 补齐或移除空入口"));
    }
    Ok((introduction, result))
}

pub(super) fn markdown_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .chars()
        .map(|c| {
            if " %()#[]<>\"'\t\r\n".contains(c) {
                format!("%{:02X}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}
fn decode_path(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let digit = |v: u8| char::from(v).to_digit(16);
            if let (Some(a), Some(b)) = (digit(bytes[i + 1]), digit(bytes[i + 2])) {
                output.push((a * 16 + b) as u8);
                i += 3;
                continue;
            }
        }
        output.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(output).map_err(|_| fail("原文链接不是有效 UTF-8"))
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
    markdown_path(&p)
}

/// Rebase only Markdown destinations, keeping code/prose and source claims intact.
fn links(
    text: &str,
    from: &Path,
    targets: &BTreeMap<String, PathBuf>,
    input: &OrganizationInput,
    tree: &Tree,
) -> Result<String> {
    let mut text = text.to_string();
    // Legacy v1 ledger used `D/<hash>.md` instead of clickable source links.
    for p in input
        .documents
        .iter()
        .chain(input.historical_origins.keys())
    {
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
                    let p = PathBuf::from(decode_path(p)?);
                    if p.as_os_str().is_empty()
                        || p.components().any(|c| !matches!(c, Component::Normal(_)))
                    {
                        return Err(fail(format!("整理稿原文链接路径无效或越界：{dest}")));
                    }
                    let historical = input.historical_origins.contains_key(&p);
                    if !input.documents.contains(&p) && !historical {
                        let reason = if tree.contains_key(&p) {
                            "文件存在，但未登记为本项目可引用的原文；请核对项目归属、资料类型及停用状态"
                        } else {
                            "文件不存在，也未登记为本项目原文；请核对链接或保存来源"
                        };
                        return Err(fail(format!(
                            "整理稿原文引用无法确认：{}（{reason}）",
                            p.display()
                        )));
                    }
                    let bytes = tree.get(&p).ok_or_else(|| {
                        fail(format!(
                            "已登记的原文缺失：{}；请恢复原文后重试",
                            p.display()
                        ))
                    })?;
                    if historical
                        && p.file_stem().and_then(|s| s.to_str())
                            != Some(crate::ids::sha256_hex(bytes).as_str())
                    {
                        return Err(fail(format!(
                            "历史原文内容与采集记录不符：{}；请恢复原文后重试",
                            p.display()
                        )));
                    }
                    format!(
                        "{}{}",
                        relative(from, &p),
                        fragment.map(|f| format!("#{f}")).unwrap_or_default()
                    )
                } else if dest.starts_with("https://") || dest.starts_with("http://") {
                    // A reference to an upstream issue/spec is data; saving it does not fetch it.
                    crate::gitx::redact_credentials(dest)
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
    let (introduction, topic_parts) = topics(&parts["topics"])?;
    if !introduction.trim().is_empty() {
        parts
            .get_mut("overview")
            .unwrap()
            .push_str(&format!("\n{introduction}"));
    }
    if parts["index"].trim().is_empty() {
        let mut index = "先按问题定位主题，检查适用条件、直接证据与评估状态，并结合当前代码使用；历史、实验、待核实和建议不能直接当作当前执行指令。\n\n".to_string();
        index.extend(
            topic_parts.iter().map(|(id, body)| {
                format!("- [{}](#{id})\n", markdown_name(&topic_title(id, body)))
            }),
        );
        parts.insert("index".into(), index);
    }
    let directory = PathBuf::from("wiki");
    let entry = PathBuf::from("README.md");
    let mut targets = BTreeMap::new();
    for key in SECTIONS
        .iter()
        .filter(|s| !["index", "topics", "overview", "validation"].contains(s))
    {
        targets.insert(key.to_string(), directory.join(format!("{key}.md")));
    }
    for id in topic_parts.keys() {
        targets.insert(id.clone(), directory.join(format!("{id}.md")));
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
        let mut body = links(body, &path, &targets, input, tree)?;
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
        let content=format!("---\nreview_status: unreviewed\nproject_id: {}\nknowledge_id: {}\n---\n\n# {}\n\n<a id=\"{id}\"></a>\n\n[项目索引]({}) · [来源目录]({})\n\n{body}",input.project_id,input.knowledge_id,markdown_name(&title),relative(&path,&entry),relative(&path,&targets["sources"]));
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
        let body = links(&parts[key], &path, &targets, input, tree)?;
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
    let index = links(&parts["index"], &entry, &targets, input, tree)?;
    let overview = links(&parts["overview"], &entry, &targets, input, tree)?;
    let validation = links(&parts["validation"], &entry, &targets, input, tree)?;
    let raw = layout::source_index(tree, &input.project_id);
    let mut content=format!("# {} · 开发知识\n\n[原始资料]({}) · [维护建议](wiki/maintenance.md) · [待核实问题](wiki/questions.md)\n\n可直接编辑本首页或 wiki/ 下的 Markdown，人工与 AI 维护同一份正文。\n\n<a id=\"overview\"></a>\n\n{overview}\n\n<a id=\"index\"></a><a id=\"topics\"></a>\n\n{index}\n\n## 更多入口\n\n",markdown_name(&input.project_name),markdown_path(&raw));
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
    let generated: BTreeMap<_, _> = files
        .iter()
        .map(|(p, b)| (p.clone(), crate::ids::sha256_hex(b)))
        .collect();
    files.insert(
        ".ailoom/wiki.json".into(),
        serde_json::to_vec_pretty(&generated)?,
    );
    Ok(files)
}
