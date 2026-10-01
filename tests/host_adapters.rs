use ailoom::adapters::{
    self,
    common::{Artifact, ArtifactBody},
    hosts, ToolTargets,
};
use ailoom::resolver::DesiredSet;
use ailoom::resource::{ResourceEntry, ResourceId, ResourceKind, ResourceMeta};
use std::path::Path;

fn entry(kind: ResourceKind, raw: &str) -> ResourceEntry {
    ResourceEntry {
        id: ResourceId {
            source: "test".into(),
            namespace: "common".into(),
            name: "review".into(),
            kind,
        },
        meta: ResourceMeta {
            shared: true,
            projects: vec![],
            roles: vec![],
            namespace: "common".into(),
            tags: vec![],
        },
        path: "skills/review".into(),
        description: "Review code".into(),
        raw: Some(raw.into()),
    }
}
fn desired() -> DesiredSet {
    DesiredSet {
        source: "test".into(),
        identity: "test".into(),
        skills_root: "skills".into(),
        revision: None,
        content_digest: "test".into(),
        active_projects: vec![],
        active_roles: vec![],
        selected: vec![],
        excluded: vec![],
    }
}
fn render(
    tool: &str,
    e: &ResourceEntry,
    root: &Path,
) -> (Vec<Artifact>, Vec<adapters::UnsupportedItem>) {
    let (mut out, mut missing) = (vec![], vec![]);
    hosts::render(
        hosts::lookup(tool).unwrap(),
        e,
        &desired(),
        root,
        root,
        &mut out,
        &mut missing,
    )
    .unwrap();
    (out, missing)
}
fn frontmatter(a: &Artifact) -> serde_yaml::Value {
    let ArtifactBody::Full { content } = &a.body else {
        panic!("expected Markdown")
    };
    let (yaml, body) = ailoom::resource::split_frontmatter(content)
        .unwrap()
        .unwrap();
    assert!(body.contains("Review carefully"));
    serde_yaml::from_str(&yaml).unwrap()
}
#[test]
fn native_agents_escape_yaml_and_keep_host_specific_options() {
    let t = tempfile::tempdir().unwrap();
    let e=entry(ResourceKind::Agent, "description = 'Review: security # first'\ninstructions = 'Review carefully'\n[tool_extras.cursor]\nreadonly = true\n[tool_extras.opencode.permission]\nedit = 'deny'\n");
    for tool in ["grok", "cursor", "opencode", "pi"] {
        let (out, missing) = render(tool, &e, t.path());
        assert!(missing.is_empty());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, Path::new(&format!(".{tool}/agents/review.md")));
        let fm = frontmatter(&out[0]);
        assert_eq!(fm["description"].as_str(), Some("Review: security # first"));
        if tool == "opencode" {
            assert_eq!(fm["mode"].as_str(), Some("subagent"));
            assert_eq!(fm["permission"]["edit"].as_str(), Some("deny"));
        }
        if tool == "cursor" {
            assert_eq!(fm["readonly"].as_bool(), Some(true));
            assert!(fm.get("permission").is_none());
        }
    }
}
#[test]
fn pi_mcp_still_requires_a_separate_extension() {
    let t = tempfile::tempdir().unwrap();
    let (out, missing) = render("pi", &entry(ResourceKind::Mcp, ""), t.path());
    assert!(out.is_empty());
    assert_eq!(missing.len(), 1);
    assert!(missing[0].reason.contains("扩展"));
}
#[test]
fn cross_host_model_and_permission_mismatches_are_not_silently_dropped() {
    let t = tempfile::tempdir().unwrap();
    for raw in [
        "instructions='Review carefully'\nmodel='sonnet'",
        "instructions='Review carefully'\ntools=['Read']",
    ] {
        let (out, missing) = render("cursor", &entry(ResourceKind::Agent, raw), t.path());
        assert!(out.is_empty());
        assert_eq!(missing.len(), 1);
    }
    let (out,missing)=render("cursor",&entry(ResourceKind::Agent,"instructions='Review carefully'\nmodel='sonnet'\n[tool_extras.cursor]\nmodel='inherit'"),t.path());
    assert!(missing.is_empty());
    assert_eq!(frontmatter(&out[0])["model"].as_str(), Some("inherit"));
}
#[test]
fn mcp_formats_keep_environment_references_and_escape_json_keys() {
    let t = tempfile::tempdir().unwrap();
    let e=entry(ResourceKind::Mcp,"name='my/server~one'\ntype='stdio'\ncommand='server'\nargs=['--stdio']\n[env]\nTOKEN='$ENV:TEST_TOKEN'");
    for (tool, expected, path, key) in [
        ("cursor", "${env:TEST_TOKEN}", ".cursor/mcp.json", "env"),
        (
            "opencode",
            "{env:TEST_TOKEN}",
            ".opencode/opencode.json",
            "environment",
        ),
    ] {
        let (out, missing) = render(tool, &e, t.path());
        assert!(missing.is_empty());
        assert_eq!(out[0].path, Path::new(path));
        let ArtifactBody::JsonPointer { pointer, value } = &out[0].body else {
            panic!()
        };
        assert!(pointer.ends_with("my~1server~0one"));
        assert_eq!(value[key]["TOKEN"], expected);
        if tool == "opencode" {
            assert_eq!(value["command"], serde_json::json!(["server", "--stdio"]));
        }
    }
    let (out, missing) = render("grok", &e, t.path());
    assert!(missing.is_empty());
    let ArtifactBody::TomlTable { value, .. } = &out[0].body else {
        panic!()
    };
    assert_eq!(value["env"]["TOKEN"].as_str(), Some("${TEST_TOKEN}"));
}
#[test]
fn remote_mcp_and_jsonc_conflicts_are_explicit() {
    let t = tempfile::tempdir().unwrap();
    let e = entry(
        ResourceKind::Mcp,
        "type='http'\nurl='https://example.com/mcp'\n[headers]\nAuthorization='$ENV:TOKEN'",
    );
    let (out, _) = render("opencode", &e, t.path());
    let ArtifactBody::JsonPointer { value, .. } = &out[0].body else {
        panic!()
    };
    assert_eq!(value["type"], "remote");
    assert_eq!(value["headers"]["Authorization"], "{env:TOKEN}");
    std::fs::create_dir_all(t.path().join(".opencode")).unwrap();
    std::fs::write(
        t.path().join(".opencode/opencode.jsonc"),
        "{ /* user's configuration */ }",
    )
    .unwrap();
    let (out, missing) = render("opencode", &e, t.path());
    assert!(out.is_empty());
    assert_eq!(missing.len(), 1);
}
#[test]
fn all_hosts_link_the_whole_skill_directory() {
    let t = tempfile::tempdir().unwrap();
    let src = t.path().join("skills/review");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: review\ndescription: review\n---\nReview",
    )
    .unwrap();
    for tool in ["grok", "pi", "opencode", "cursor"] {
        let (out, missing) = render(tool, &entry(ResourceKind::Skill, ""), t.path());
        assert!(missing.is_empty());
        assert_eq!(out[0].path, Path::new(&format!(".{tool}/skills/review")));
        assert!(matches!(&out[0].body,ArtifactBody::Symlink{source_dir,..} if source_dir==&src));
    }
}
#[test]
fn cursor_keeps_conditional_rules_conditional() {
    let t = tempfile::tempdir().unwrap();
    let e = entry(
        ResourceKind::Rule,
        "---\npaths: ['src/**/*.ts']\n---\nReview carefully",
    );
    let (out, missing) = render("cursor", &e, t.path());
    assert!(missing.is_empty());
    let fm = frontmatter(&out[0]);
    assert_eq!(fm["alwaysApply"].as_bool(), Some(false));
    assert_eq!(fm["globs"][0].as_str(), Some("src/**/*.ts"));
    let (out, missing) = render("pi", &e, t.path());
    assert!(out.is_empty());
    assert_eq!(missing.len(), 1);
}
#[test]
fn claude_default_fallback_respects_ancestor_and_local_files() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    assert_eq!(adapters::docs::claude_entry_file(&root), "AGENTS.md");
    std::fs::write(root.join("AGENTS.md"), "shared").unwrap();
    assert_eq!(adapters::docs::claude_entry_file(&root), "AGENTS.md");
    std::fs::write(t.path().join("CLAUDE.local.md"), "local").unwrap();
    assert_eq!(adapters::docs::claude_entry_file(&root), "CLAUDE.md");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join(".claude/CLAUDE.md"), "team").unwrap();
    assert_eq!(
        adapters::docs::claude_entry_file(&root),
        ".claude/CLAUDE.md"
    );
}
#[test]
fn personal_instructions_are_additive_and_do_not_shadow_claude_fallback() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("AGENTS.md"), "Company rules").unwrap();
    let hosts: Vec<String> = ["claude", "pi", "grok", "cursor", "opencode"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let out = ailoom::personal_instructions::render(t.path(), &hosts, "Speak Chinese").unwrap();
    assert_eq!(out.len(), 5);
    assert!(!out.iter().any(|a| a.path == Path::new("CLAUDE.md")));
    assert!(out
        .iter()
        .any(|a| a.path == Path::new(".pi/APPEND_SYSTEM.md")));
    assert_eq!(
        std::fs::read_to_string(t.path().join("AGENTS.md")).unwrap(),
        "Company rules"
    );
    assert!(!ailoom::personal_instructions::exclude_patterns(&out)
        .contains(&".grok/rules/ailoom-personal.md".into()));
}
#[test]
fn discovery_and_capabilities_cover_each_new_host() {
    for tool in ["grok", "pi", "opencode", "cursor"] {
        assert!(adapters::is_extra_target(tool));
        assert!(adapters::discovery::directory_markers()
            .iter()
            .any(|m| m.agent == tool));
        for kind in ["skill", "agent", "mcp", "rule"] {
            assert!(adapters::capability::query(tool, kind, "project").is_some());
        }
    }
    let _ = ToolTargets::default();
}

#[test]
fn pi_official_subagent_keeps_model_tools_and_prompt() {
    let t = tempfile::tempdir().unwrap();
    let e = entry(ResourceKind::Agent, "instructions='Review carefully'\nmodel='sonnet'\ntools=['Read']\n[tool_extras.pi]\nmodel='provider/model'\ntools=['read','grep','find','ls']");
    let (out, missing) = render("pi", &e, t.path());
    assert!(missing.is_empty());
    assert_eq!(out[0].path, Path::new(".pi/agents/review.md"));
    let fm = frontmatter(&out[0]);
    assert_eq!(fm["name"].as_str(), Some("review"));
    assert_eq!(fm["model"].as_str(), Some("provider/model"));
    assert_eq!(fm["tools"][0].as_str(), Some("read"));
    assert_eq!(fm["tools"].as_sequence().unwrap().len(), 4);
    let cap = adapters::capability::query("pi", "agent", "project").unwrap();
    assert_eq!(cap.support, adapters::capability::Support::Generated);
    assert!(cap.notes.contains("agentScope"));
    assert_eq!(cap.required_extension, Some("Pi 官方 subagent"));
    let payload = serde_json::to_value(&cap).unwrap();
    assert_eq!(payload["required_extension"], "Pi 官方 subagent");
    assert!(
        cap.verified.is_none(),
        "Generated config does not prove the extension ran"
    );
    assert!(adapters::capability::query("cursor", "agent", "project")
        .unwrap()
        .required_extension
        .is_none());
}
