//! 适配器（AIL-009—012）与 plan/sync 命令集成测试。
//! 能力矩阵：docs/capabilities/{claude-code,codex}.md（官方来源与核实日期）。

mod common;

use std::path::Path;
use std::process::Command;

fn bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    while path.pop() {
        let p = if cfg!(windows) {
            "ailoom.exe"
        } else {
            "ailoom"
        };
        if path.join(p).exists() {
            return path.join(p);
        }
    }
    panic!("未找到 ailoom 二进制");
}

use std::path::PathBuf;

struct Ctx {
    tmp: tempfile::TempDir,
}

impl Ctx {
    fn new() -> Ctx {
        Ctx {
            tmp: tempfile::tempdir().unwrap(),
        }
    }
    fn run(&self, cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .envs(common::isolated_child_env(self.tmp.path()))
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn dr(&self) -> String {
        self.tmp.path().join("data").to_string_lossy().to_string()
    }
}

fn init_and_sync(c: &Ctx, ws: &Path, extra_init: &[&str]) {
    let src = c.tmp.path().join("src");
    let url = common::file_url(&common::make_team_source_full(&src));
    let dr = c.dr();
    let mut args: Vec<String> = vec![
        "--data-root".into(),
        dr.clone(),
        "init".into(),
        "--url".into(),
        url.clone(),
    ];
    args.extend(extra_init.iter().map(|s| s.to_string()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(ws, &refs);
    assert_eq!(code, 0, "init: {stderr}");
    let args = ["--data-root", dr.as_str(), "sync"];
    let (code, _, stderr) = c.run(ws, &args);
    assert_eq!(code, 0, "sync: {stderr}");
}

#[test]
fn skills_deploy_to_both_tools_with_references() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_and_sync(&c, &ws, &["--project", "a", "--role", "dev"]);

    let claude_skill = ws.join(".claude/skills/a-deploy/SKILL.md");
    let codex_skill = ws.join(".agents/skills/a-deploy/SKILL.md");
    assert!(claude_skill.is_file(), "Claude 落盘");
    assert!(codex_skill.is_file(), "Codex 落盘");
    // 引用目录随技能整体复制
    assert!(ws
        .join(".claude/skills/a-deploy/references/checklist.md")
        .is_file());
    // 同一技能两工具内容一致
    assert_eq!(
        std::fs::read_to_string(claude_skill).unwrap(),
        std::fs::read_to_string(codex_skill).unwrap()
    );
    // Codex skills.config 数组指向托管前缀
    let cfg = std::fs::read_to_string(ws.join(".codex/config.toml")).unwrap();
    assert!(
        cfg.contains("path = \".agents/skills/a-deploy/SKILL.md\""),
        "{cfg}"
    );
    assert!(cfg.contains("enabled = true"));
    // shared 技能两工具都有
    assert!(ws.join(".claude/skills/common-greet/SKILL.md").is_file());
    // ADR-0001：工作区是 symlink，实体在受控 Store 根（隔离环境的 AILOOM_STORE_ROOT）下
    #[cfg(unix)]
    {
        let link = ws.join(".claude/skills/a-deploy");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        let target = std::fs::read_link(&link).unwrap();
        let t = target.to_string_lossy();
        let store_root = common::isolated_store_root(c.tmp.path());
        assert!(
            target.starts_with(&store_root),
            "{t} 应位于 {store_root:?} 下"
        );
        assert!(!t.contains("/sources/"), "{t}");
        assert!(t.ends_with("a-deploy") || t.contains("/a-deploy"), "{t}");
        assert!(!t.contains("resources/skills"), "{t}");
    }
}

#[test]
fn closed_tool_gets_no_directory() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_and_sync(&c, &ws, &["--project", "a", "--target", "claude"]);
    assert!(ws.join(".claude").is_dir());
    assert!(!ws.join(".codex").exists(), "关闭的工具不写其目录");
}

#[test]
fn rules_preserve_user_content_and_no_duplicate_fragments() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    std::fs::create_dir_all(ws.join(".claude/rules")).unwrap();
    // 用户已有正文
    std::fs::write(ws.join("AGENTS.md"), "# 我的工作区说明\n\n自定义内容\n").unwrap();
    std::fs::write(ws.join(".claude/rules/user-own.md"), "# 用户自己的规则\n").unwrap();
    init_and_sync(&c, &ws, &["--project", "a", "--role", "dev"]);

    let agents = std::fs::read_to_string(ws.join("AGENTS.md")).unwrap();
    assert!(agents.contains("自定义内容"), "用户正文保留");
    assert!(agents.contains("BEGIN AILOOM MANAGED: team/rule/common/coding-standards"));
    // Claude：原生规则文件
    let rule = std::fs::read_to_string(ws.join(".claude/rules/coding-standards.md")).unwrap();
    assert!(rule.contains("祈使句"), "{rule}");

    // 两次 sync 无重复片段
    let args = ["--data-root", &c.dr(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    let agents2 = std::fs::read_to_string(ws.join("AGENTS.md")).unwrap();
    assert_eq!(
        agents2
            .matches("BEGIN AILOOM MANAGED: team/rule/common/coding-standards")
            .count(),
        1,
        "重复同步不产生重复片段"
    );
}

#[test]
fn conditional_rule_is_unsupported_for_codex_not_silently_global() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    // 在源里加一条条件规则
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    std::fs::write(
        src.join("resources/rules/scoped-rule.md"),
        "---\nname: scoped-rule\ndescription: 条件规则\nshared: true\nnamespace: common\npaths:\n  - \"src/**\"\n---\n\n条件内容\n",
    )
    .unwrap();
    common::commit_only(&src, "add scoped rule");
    let url = common::file_url(&src);
    let dr = c.dr();
    let args = vec![
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
        "--target".to_string(),
        "codex".to_string(),
    ];
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &arg_refs);
    assert_eq!(code, 0, "{stderr}");
    let args = ["--data-root", &c.dr(), "sync"];
    let (code, _stdout, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    // JSON plan 中显式列出 unsupported
    let args = ["--json", "--data-root", &c.dr(), "plan"];
    let (_, stdout, _) = c.run(&ws, &args);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let has_unsupported = v["result"]["actions"].as_array().unwrap().iter().any(|a| {
        a["action"] == "unsupported" && a["resource_id"].as_str().unwrap().contains("scoped-rule")
    });
    assert!(has_unsupported, "条件规则必须显式 unsupported: {v}");
}

#[test]
fn agents_render_for_claude_and_unsupported_for_codex() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_and_sync(&c, &ws, &["--project", "a", "--role", "dev"]);

    let agent = ws.join(".claude/agents/release-helper.md");
    assert!(agent.is_file(), "Claude agent 落盘");
    let text = std::fs::read_to_string(&agent).unwrap();
    assert!(text.starts_with("---\nname: release-helper"));
    assert!(text.contains("description: 发布辅助"));
    assert!(text.contains("tools: Read"));
    assert!(text.contains("model: inherit"));
    assert!(text.contains("permission-mode: plan"), "tool_extras 透传");
    assert!(text.contains("检查发布清单"), "多行 instructions 正文");
    assert!(
        !ws.join(".codex/agents").exists(),
        "Codex 不支持 agent，不创建目录"
    );

    // Codex agent 显式 unsupported
    let args = ["--json", "--data-root", &c.dr(), "plan"];
    let (_, stdout, _) = c.run(&ws, &args);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        v["result"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["action"] == "unsupported"
                && a["kind"] == "agent"
                && a["target_tool"] == "codex"),
        "Codex agent 应显式 unsupported: {v}"
    );
}

#[test]
fn agent_missing_instructions_fails_render() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    std::fs::write(
        src.join("resources/agents/broken.toml"),
        "name = \"broken\"\ndescription = \"缺 instructions\"\nshared = true\nnamespace = \"common\"\ntargets = [\"claude\"]\n",
    )
    .unwrap();
    common::commit_only(&src, "add broken agent");
    let url = common::file_url(&src);
    let dr = c.dr();
    let args = [
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let args = ["--data-root", &c.dr(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_ne!(code, 0, "未知 required 字段（缺 instructions）必须失败");
    assert!(
        stderr.contains("E5002") || stderr.contains("instructions"),
        "{stderr}"
    );
}

#[test]
fn mcp_json_and_toml_merge_preserve_user_entries() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    // 用户已有 .mcp.json（含个人 server）
    std::fs::write(
        ws.join(".mcp.json"),
        r#"{"mcpServers":{"my-own":{"command":"echo"}},"other":1}"#,
    )
    .unwrap();
    init_and_sync(&c, &ws, &["--project", "a", "--target", "claude"]);

    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".mcp.json")).unwrap()).unwrap();
    // 个人条目保留；托管条目加入
    assert!(
        mcp["mcpServers"]["my-own"].is_object(),
        "个人同名文件不覆盖"
    );
    assert_eq!(mcp["other"], 1, "无关字段保留");
    let server = &mcp["mcpServers"]["team-files"];
    assert_eq!(server["command"], "uvx");
    assert_eq!(server["args"][0], "mcp-server-files");
    // 秘密引用 → 原生插值，不落明文
    assert_eq!(server["env"]["TOKEN"], "${AILOOM_TEST_TOKEN}");
    assert!(!std::fs::read_to_string(ws.join(".mcp.json"))
        .unwrap()
        .contains("$ENV:"));

    // 只删除托管条目：源里移除 mcp
    let src = c.tmp.path().join("src/team-src");
    common::commit_rm(&src, "resources/mcp/files.toml", "remove mcp");
    // 更新是明确动作：--refresh 推进锁
    let url = common::file_url(&src);
    let dr = c.dr();
    let args = [
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url,
        "--refresh".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let args = ["--data-root", &c.dr(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".mcp.json")).unwrap()).unwrap();
    assert!(
        mcp["mcpServers"].get("team-files").is_none(),
        "托管条目被移除"
    );
    assert!(mcp["mcpServers"].get("my-own").is_some(), "个人条目仍在");
}

#[test]
fn mcp_unmanaged_same_name_conflicts_not_overwritten() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    // 同名 server 已存在但未被托管
    std::fs::write(
        ws.join(".mcp.json"),
        r#"{"mcpServers":{"team-files":{"command":"mine"}}}"#,
    )
    .unwrap();
    let src = common::file_url(&common::make_team_source_full(&c.tmp.path().join("src")));
    let dr = c.dr();
    let args = vec![
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        src,
        "--project".to_string(),
        "a".to_string(),
        "--target".to_string(),
        "claude".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    let args = ["--json", "--data-root", &c.dr(), "plan"];
    let (_, stdout, _) = c.run(&ws, &args);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        v["result"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["action"] == "conflict" && a["path"] == ".mcp.json"),
        "未托管同名条目必须冲突: {v}"
    );
    // sync 跳过冲突，用户内容不变
    let args = ["--data-root", &c.dr(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(
        mcp["mcpServers"]["team-files"]["command"], "mine",
        "不被覆盖"
    );
}

#[test]
fn mcp_corrupt_config_not_clobbered() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    std::fs::write(ws.join(".mcp.json"), "{ corrupt json").unwrap();
    let src = c.tmp.path().join("src");
    let url = common::file_url(&common::make_team_source_full(&src));
    let dr = c.dr();
    let args = vec![
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
        "--target".to_string(),
        "claude".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    // 解析失败 → sync 拒绝执行且不清空文件
    let args = ["--data-root", dr.as_str(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_ne!(code, 0, "损坏配置必须拒绝执行");
    assert!(stderr.contains("E5004"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(ws.join(".mcp.json")).unwrap(),
        "{ corrupt json"
    );
}

#[test]
fn mcp_codex_http_and_stdio_and_secret_ref_rejected() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    // http + 无秘密的 stdio 可以写 codex；含秘密引用的默认 files.toml 被拒
    std::fs::write(
        src.join("resources/mcp/http-svc.toml"),
        "name = \"http-svc\"\ntype = \"http\"\nurl = \"https://svc.example.internal/mcp\"\nshared = true\nnamespace = \"common\"\ntargets = [\"codex\"]\n\n[headers]\nX-Mode = \"test\"\n",
    )
    .unwrap();
    std::fs::write(
        src.join("resources/mcp/plain.toml"),
        "name = \"plain\"\ntype = \"stdio\"\ncommand = \"cat\"\nargs = [\"--flag\", \"a b\", \"c\\\"d\"]\nshared = true\nnamespace = \"common\"\ntargets = [\"codex\"]\n",
    )
    .unwrap();
    common::commit_only(&src, "add http and stdio mcp");
    let url = common::file_url(&src);
    let dr = c.dr();
    let args = vec![
        "--data-root".to_string(),
        dr,
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
        "--target".to_string(),
        "codex".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, stderr) = c.run(&ws, &refs);
    assert_eq!(code, 0, "{stderr}");
    let args = ["--data-root", &c.dr(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "{stderr}");

    let cfg = std::fs::read_to_string(ws.join(".codex/config.toml")).unwrap();
    assert!(cfg.contains("[mcp_servers.http-svc]"), "{cfg}");
    assert!(cfg.contains("url = \"https://svc.example.internal/mcp\""));
    assert!(cfg.contains("[mcp_servers.plain]"));
    // 参数数组逐项保留（含空格与引号，不经 shell 解释）
    assert!(cfg.contains("\"a b\""), "{cfg}");
    assert!(
        !cfg.contains("mcp_servers.team-files"),
        "含秘密引用的条目不写入 Codex"
    );
    // 显式 unsupported 可见
    let args = ["--json", "--data-root", &c.dr(), "plan"];
    let (_, stdout, _) = c.run(&ws, &args);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(v["result"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["action"] == "unsupported"
            && a["resource_id"].as_str().unwrap().contains("team-files")));
}

#[test]
fn user_level_host_configs_never_touched() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_and_sync(&c, &ws, &["--project", "a", "--role", "dev"]);
    let home = c.tmp.path().join("home");
    // HOME 下不应出现 .codex / .claude / .mcp.json（项目级能力不回落全局）
    assert!(!home.join(".codex").exists(), "不写用户级 Codex 配置");
    assert!(!home.join(".claude").exists(), "不写用户级 Claude 配置");
    assert!(!home.join(".mcp.json").exists());
}

#[test]
fn selected_env_kind_deploys_literal_vars_now_supported() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    let src = common::make_team_source_full(&c.tmp.path().join("src"));
    std::fs::create_dir_all(src.join("resources/env")).unwrap();
    std::fs::write(
        src.join("resources/env/team-env.toml"),
        "name = \"team-env\"\nshared = true\nnamespace = \"common\"\ntargets = [\"claude\"]\n\n[vars]\nAPI = \"https://x\"\n",
    )
    .unwrap();
    common::commit_only(&src, "add env");
    let url = common::file_url(&src);
    let dr = c.dr();
    let args = [
        "--data-root".to_string(),
        dr.clone(),
        "init".to_string(),
        "--url".to_string(),
        url,
        "--project".to_string(),
        "a".to_string(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, _, _) = c.run(&ws, &refs);
    assert_eq!(code, 0);
    // AIL-031 落地后：字面量 env 变量部署到 .claude/settings.json /env
    let args = ["--data-root", dr.as_str(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    eprintln!("SYNC {code} {stderr}");
    assert_eq!(code, 0);
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["env"]["API"], "https://x");
}

// ---------------------------------------------------------------------------
// AIL-041：Codex 原生 .agents/skills 部署与旧 .ailoom/skills 迁移
// ---------------------------------------------------------------------------

/// 新部署落在官方原生目录 `.agents/skills/<name>`；skills.config 条目 path 指向
/// SKILL.md（官方示例形态）；capability 查询接口给出 native + 需新会话。
#[test]
fn ail041_codex_native_skill_path_and_capability_query() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_and_sync(&c, &ws, &["--project", "a", "--role", "dev"]);

    let native = ws.join(".agents/skills/a-deploy");
    assert!(
        native.join("SKILL.md").is_file(),
        "Codex 部署到 .agents/skills"
    );
    assert!(!ws.join(".ailoom/skills").exists(), "新部署不再使用旧前缀");
    let cfg = std::fs::read_to_string(ws.join(".codex/config.toml")).unwrap();
    assert!(
        cfg.contains("path = \".agents/skills/a-deploy/SKILL.md\""),
        "config 条目指向 SKILL.md（官方形态）: {cfg}"
    );

    // 能力查询接口：native、需新会话、带实测引用
    let cap = ailoom::adapters::capability::query("codex", "skill", "project").unwrap();
    assert_eq!(cap.support, ailoom::adapters::capability::Support::Native);
    assert!(cap.requires_new_session);
    assert!(cap.verified.is_some());
}

/// 旧版 `.ailoom/skills/<name>` 部署在再次 sync 时自动迁移：
/// 旧链接按托管清单过期清理删除，新原生链接创建，config 条目更新。
#[test]
fn ail041_legacy_codex_path_migrates_on_resync() {
    let c = Ctx::new();
    let ws = common::make_business_repo(c.tmp.path(), "biz");
    init_and_sync(&c, &ws, &["--project", "a", "--role", "dev"]);
    // 模拟旧版部署（真实升级路径的形态）：旧链接指向同一实体、
    // 旧 config 条目、托管清单中记录旧 item key（旧版 sync 写入的状态）
    #[cfg(unix)]
    {
        std::fs::create_dir_all(ws.join(".ailoom/skills")).unwrap();
        std::os::unix::fs::symlink(
            ".agents/skills/a-deploy",
            ws.join(".ailoom/skills/a-deploy"),
        )
        .unwrap();
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(
            ws.join(".agents/skills/a-deploy"),
            ws.join(".ailoom/skills/a-deploy"),
        )
        .unwrap();
    }
    let cfg_path = ws.join(".codex/config.toml");
    let cfg = std::fs::read_to_string(&cfg_path).unwrap();
    let legacy_cfg = cfg.replace(
        "path = \".agents/skills/a-deploy/SKILL.md\"",
        "path = \".ailoom/skills/a-deploy\"",
    );
    std::fs::write(&cfg_path, &legacy_cfg).unwrap();
    // 托管清单：克隆原生 symlink 条目为旧路径 key（旧版清单状态）
    let manifest_path = {
        let wid = {
            let ws_canon = ws.canonicalize().unwrap();
            ailoom::ids::workspace_id_from_root(&ws_canon)
        };
        c.tmp
            .path()
            .join("data")
            .join("ws")
            .join(wid)
            .join("managed-manifest.json")
    };
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let native_key = ".agents/skills/a-deploy#symlink";
    let legacy_item = manifest["items"][native_key].clone();
    assert!(!legacy_item.is_null(), "原生条目应在清单中");
    manifest["items"][".ailoom/skills/a-deploy#symlink"] = legacy_item;
    // 旧版 sync 写入的 config 内容即 legacy_cfg：清单哈希与其一致（非用户篡改）
    manifest["items"][".codex/config.toml"]["content_hash"] = serde_json::Value::String(format!(
        "sha256:{}",
        ailoom::ids::sha256_hex(legacy_cfg.as_bytes())
    ));
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // 再次 sync：旧前缀条目应被清除/迁移
    let dr = c.dr();
    let args = ["--data-root", dr.as_str(), "sync"];
    let (code, _, stderr) = c.run(&ws, &args);
    assert_eq!(code, 0, "resync: {stderr}");
    assert!(
        !ws.join(".ailoom/skills/a-deploy").exists(),
        "旧链接被迁移清理"
    );
    assert!(
        ws.join(".agents/skills/a-deploy/SKILL.md").is_file(),
        "新原生链接保留"
    );
    let cfg = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(
        !cfg.contains(".ailoom/skills/a-deploy"),
        "旧 config 条目被清除: {cfg}"
    );
    assert!(
        cfg.contains("path = \".agents/skills/a-deploy/SKILL.md\""),
        "config 条目更新为新形态: {cfg}"
    );
}
