//! 输出约定：成功 JSON envelope 走 stdout，人类文本走 stdout，错误/日志一律 stderr。

use serde_json::{json, Value};

/// 输出成功 JSON envelope：`{"schema_version":1,"result":…}`，单行、无日志混入。
pub fn emit_json(result: &Value) {
    println!("{}", json!({ "schema_version": 1, "result": result }));
}

/// 输出错误 JSON（只走 stderr）：错误字段平铺，并带顶层 `schema_version`（契约 §5）。
pub fn emit_error_json(err: &crate::error::Error) {
    use std::io::Write;
    let mut value = err.to_json();
    value["schema_version"] = json!(1);
    let _ = writeln!(std::io::stderr().lock(), "{value}");
}

/// 人类模式的通用结果输出（走 stdout）：优先打印结果自带的 `summary`/`note` 与提示，
/// 没有摘要字段的结果打印格式化 JSON，保证命令不会静默结束。
pub fn emit_human(result: &Value) {
    let mut printed = false;
    for key in ["summary", "note", "message"] {
        if let Some(text) = result[key].as_str().filter(|t| !t.is_empty()) {
            println!("{text}");
            printed = true;
        }
    }
    if !printed {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
    for key in ["warnings", "notes"] {
        for item in result[key].as_array().into_iter().flatten() {
            if let Some(text) = item.as_str() {
                println!("提示：{text}");
            }
        }
    }
}

/// 输出人类可读成功信息。
pub fn emit_text(message: impl std::fmt::Display) {
    println!("{message}");
}

fn origin_label(origin: &Value) -> String {
    let key = origin
        .as_str()
        .map(str::to_string)
        .or_else(|| origin.as_object().and_then(|o| o.keys().next().cloned()))
        .unwrap_or_default();
    match key.as_str() {
        "team_declaration" => "团队声明",
        "repo_default" => "项目共享设置",
        "repo_subproject" => "项目子目录模板",
        "worktree_override" => "当前 Worktree",
        "worktree_subproject" => "Worktree 子目录",
        "" => "未设置",
        other => return other.to_string(),
    }
    .to_string()
}

fn print_notes(v: &Value) {
    for key in ["warnings", "notes"] {
        for item in v[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            println!("提示：{item}");
        }
    }
}

/// `personal` 的人类可读输出；没有专门格式的动作回落到 [`emit_human`]。
pub fn emit_personal(action: &str, v: &Value) {
    match action {
        "effective" => {
            let mut head = format!(
                "仓库 {} · Worktree {}",
                v["repo_id"].as_str().unwrap_or("?"),
                v["worktree_id"].as_str().unwrap_or("?")
            );
            if let Some(rel) = v["active_rel"].as_str() {
                head.push_str(&format!(" · 子目录 {rel}"));
            }
            println!("{head}");
            let hosts: Vec<String> = v["hosts"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, h)| h["enabled"] == true)
                .map(|(id, h)| format!("{id}（{}）", origin_label(&h["origin"])))
                .collect();
            println!(
                "AI 工具：{}",
                if hosts.is_empty() {
                    "未启用".into()
                } else {
                    hosts.join("、")
                }
            );
            let resources: Vec<(&String, &Value)> = v["resources"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, r)| r["deployed"] == true)
                .collect();
            println!("已启用资源：{} 项", resources.len());
            for (id, r) in resources {
                println!("  {id}（{}）", origin_label(&r["origin"]));
            }
            println!(
                "待应用改动：{} 项",
                v["pending_actions"].as_u64().unwrap_or(0)
            );
            print_notes(v);
        }
        "plan" => {
            match v["summary"].as_str().filter(|s| !s.is_empty()) {
                Some(summary) => print!("{summary}"),
                None => println!("没有待应用的改动"),
            }
            if v["has_conflicts"] == true {
                println!("{}", crate::sync::plan::CONFLICT_HELP);
            }
            print_notes(v);
        }
        "instructions" if v["cleared"] == true => {
            println!("已清除个人指令；运行 ailoom personal --action sync 从项目中移除");
        }
        "select" => {
            let target = v["resource"].as_str().or(v["host"].as_str()).unwrap_or("?");
            println!(
                "已保存：{target} → {}（{}）",
                v["state"].as_str().unwrap_or("?"),
                v["scope"].as_str().unwrap_or("?")
            );
            println!("运行 ailoom personal --action plan 预览、--action sync 应用。");
            print_notes(v);
        }
        "sync" => {
            let len = |k: &str| v[k].as_array().map_or(0, Vec::len);
            println!(
                "{}：写入 {} 项，无变化 {} 项，冲突跳过 {} 项{}",
                if v["ok"] == true {
                    "同步完成"
                } else {
                    "同步未完成"
                },
                len("applied"),
                v["noop"].as_u64().unwrap_or(0),
                len("skipped_conflicts"),
                if len("failed") > 0 {
                    format!("，失败 {} 项", len("failed"))
                } else {
                    String::new()
                }
            );
            for path in v["skipped_conflicts"].as_array().into_iter().flatten() {
                println!("  冲突 {}", path.as_str().unwrap_or("?"));
            }
            if len("skipped_conflicts") > 0 {
                println!("{}", crate::sync::plan::CONFLICT_HELP);
            }
            if let Some(next) = v["next"].as_str() {
                println!("{next}");
            }
            if let Some(hint) = v["undo_hint"].as_str() {
                println!("{hint}");
            }
            print_notes(v);
        }
        _ => emit_human(v),
    }
}

/// `global` 的人类可读输出；没有专门格式的动作回落到 [`emit_human`]。
pub fn emit_global(action: &str, v: &Value) {
    let list = |k: &str| v[k].as_array().cloned().unwrap_or_default();
    match action {
        "status" => {
            for t in list("targets") {
                let state = if t["supported"] != true {
                    "不在用户目录下，暂不支持"
                } else if t["enabled"] == true {
                    "开启"
                } else {
                    "关闭"
                };
                println!(
                    "目标 {}：{}（{}）",
                    t["label"].as_str().unwrap_or("?"),
                    t["dir"].as_str().unwrap_or("?"),
                    state
                );
            }
            let skills = list("skills");
            let on: Vec<_> = skills.iter().filter(|s| s["global"] == true).collect();
            println!(
                "全局启用 {} 个 Skill（可选 {} 个）：",
                on.len(),
                skills.len()
            );
            for s in &on {
                println!("  {}", s["id"].as_str().unwrap_or("?"));
            }
            let foreign = list("foreign");
            if !foreign.is_empty() {
                println!("目录里已有（非 AILoom 管理，只列出不改动）：");
            }
            for f in foreign {
                let loc = &f["location"];
                let kind = match loc["kind"].as_str() {
                    Some("link") => "链接",
                    Some("dir") => "文件夹",
                    Some("broken_link") => "失效链接",
                    _ => "其他",
                };
                println!("  {}（{kind}）", f["name"].as_str().unwrap_or("?"));
                println!("    位置：{}", f["path"].as_str().unwrap_or("?"));
                for via in loc["via"].as_array().into_iter().flatten() {
                    println!("    中间经过：{}", via.as_str().unwrap_or("?"));
                }
                match (loc["kind"].as_str(), loc["real_path"].as_str()) {
                    (Some("link"), Some(real)) => println!("    真实目录：{real}"),
                    (Some("dir"), Some(real)) => {
                        println!("    真实目录：{real}（本身就是文件夹，不是链接）")
                    }
                    _ => {}
                }
                if let Some(id) = f["conflicts_with"].as_str() {
                    println!("    与全局启用的 {id} 同名：可 takeover 接管");
                }
            }
            if v["pending"].as_u64().unwrap_or(0) > 0 {
                println!(
                    "有 {} 项待应用：运行 ailoom global --action plan 预览、--action sync 应用",
                    v["pending"]
                );
            }
            print_notes(v);
        }
        "plan" => {
            print!("{}", v["summary"].as_str().unwrap_or(""));
            if v["has_conflicts"] == true {
                println!("{}", crate::sync::plan::CONFLICT_HELP);
                println!("全局目录中已有的同名条目可用 ailoom global --action takeover --target <claude|agents> --name <名字> 接管（原条目移入归档，可还原）");
            }
            print_notes(v);
        }
        "sync" => {
            let len = |k: &str| v[k].as_array().map_or(0, Vec::len);
            println!(
                "全局同步完成：写入 {} 项，无变化 {} 项，冲突跳过 {} 项",
                len("applied"),
                v["noop"].as_u64().unwrap_or(0),
                len("skipped_conflicts")
            );
            for path in list("skipped_conflicts") {
                println!("  冲突 {}", path.as_str().unwrap_or("?"));
            }
            if let Some(next) = v["next"].as_str() {
                println!("{next}");
            }
            print_notes(v);
        }
        "takeover" => {
            let a = &v["archived"];
            println!(
                "已移入归档：{} → {}（归档 ID {}）",
                a["original"].as_str().unwrap_or("?"),
                a["archived"].as_str().unwrap_or("?"),
                a["id"].as_str().unwrap_or("?")
            );
            if let Some(next) = v["next"].as_str() {
                println!("{next}");
            }
        }
        "restore" => println!(
            "已还原：{}",
            v["restored"]["original"].as_str().unwrap_or("?")
        ),
        "select" => {
            let what = v["skill"]
                .as_str()
                .map(|s| format!("Skill {s}"))
                .or_else(|| v["target"].as_str().map(|t| format!("目标 {t}")))
                .unwrap_or_default();
            println!(
                "已{}全局{what}",
                if v["enabled"] == true {
                    "启用"
                } else {
                    "停用"
                }
            );
            if let Some(next) = v["next"].as_str() {
                println!("{next}");
            }
        }
        _ => emit_human(v),
    }
}

/// `library` 的人类可读输出；没有专门格式的动作回落到 [`emit_human`]。
pub fn emit_library(action: &str, v: &Value) {
    let list = |k: &str| v[k].as_array().cloned().unwrap_or_default();
    match action {
        "list" => {
            let entries = list("entries");
            println!("资源库 {} 项", entries.len());
            for e in &entries {
                println!(
                    "  {}  {}",
                    e["id"].as_str().unwrap_or("?"),
                    e["description"].as_str().unwrap_or("")
                );
            }
            let issues = list("issues");
            if !issues.is_empty() {
                println!("无法读取 {} 项（同步时跳过）：", issues.len());
                for i in &issues {
                    println!(
                        "  {}：{}",
                        i["path"].as_str().unwrap_or("?"),
                        i["error"].as_str().unwrap_or("")
                    );
                }
            }
        }
        "import" | "import-git" | "import-entry" if v["executed"] == true => {
            println!(
                "已导入 {}（复制 {} 个文件；脚本只复制、未执行）",
                v["skill_id"].as_str().unwrap_or("?"),
                v["files_copied"].as_u64().unwrap_or(0)
            );
        }
        "import" | "import-git" | "import-entry" => {
            let p = &v["preview"];
            let candidates = p["candidates"].as_array().cloned().unwrap_or_default();
            if !candidates.is_empty() {
                println!("仓库内有多个 Skill，用 --path 选择其一：");
                for c in &candidates {
                    println!("  {}", c.as_str().unwrap_or("?"));
                }
                return;
            }
            println!(
                "预览：{} · {} 个文件",
                p["skill_name"].as_str().unwrap_or("?"),
                p["files"].as_array().map_or(0, Vec::len)
            );
            if let Some(commit) = p["resolved_commit"].as_str() {
                println!("  版本 {}", commit.chars().take(12).collect::<String>());
            }
            let scripts = p["scripts"].as_array().cloned().unwrap_or_default();
            if !scripts.is_empty() {
                println!("  含 {} 个脚本（只复制，不执行）", scripts.len());
            }
            for c in p["conflicts"].as_array().into_iter().flatten() {
                println!("  冲突：{}", c.as_str().unwrap_or("?"));
            }
            println!("确认后加 --execute 执行导入");
        }
        "check-update" => {
            let st = &v["status"];
            println!(
                "{}：{}",
                st["skill"].as_str().unwrap_or("?"),
                st["state"].as_str().unwrap_or("?")
            );
            if let Some(id) = st["preview_id"].as_str() {
                println!(
                    "应用更新：ailoom library --action update --skill {} --preview-id {id} --execute",
                    st["skill"].as_str().unwrap_or("?")
                );
            } else if let Some(note) = st["note"].as_str() {
                println!("{note}");
            }
        }
        "update" if v["executed"] == true => {
            let r = &v["result"];
            println!(
                "已更新 {}；{}",
                r["skill"].as_str().unwrap_or("?"),
                r["note"].as_str().unwrap_or("")
            );
        }
        "update" => {
            println!(
                "{}：{}",
                v["status"]["skill"].as_str().unwrap_or("?"),
                v["status"]["state"].as_str().unwrap_or("?")
            );
            if let Some(note) = v["note"].as_str() {
                println!("{note}");
            }
        }
        "delete" if v["executed"] == false => {
            let p = &v["preview"];
            if p["exists"] != true {
                println!("资源不存在：{}", p["resource_id"].as_str().unwrap_or("?"));
                return;
            }
            let scopes = p["affected_scopes"].as_array().cloned().unwrap_or_default();
            if scopes.is_empty() {
                println!("{}：没有项目在用", p["resource_id"].as_str().unwrap_or("?"));
            } else {
                println!(
                    "{} 仍被这些作用域启用：",
                    p["resource_id"].as_str().unwrap_or("?")
                );
                for s in &scopes {
                    println!("  {}", s.as_str().unwrap_or("?"));
                }
            }
            println!("{}；确认后加 --execute", p["note"].as_str().unwrap_or(""));
        }
        _ => emit_human(v),
    }
}

/// `collection` 的人类可读输出；没有专门格式的动作回落到 [`emit_human`]。
pub fn emit_collection(action: &str, v: &Value) {
    let short = |c: &Value| {
        c.as_str()
            .map(|s| s.chars().take(12).collect::<String>())
            .unwrap_or_else(|| "-".into())
    };
    match action {
        "list" => {
            let sources = v["sources"].as_array().cloned().unwrap_or_default();
            println!("{} 个来源", sources.len());
            for s in &sources {
                println!(
                    "  {}（{}）· {} 项资源 · 版本 {}",
                    s["name"].as_str().unwrap_or("?"),
                    s["id"].as_str().unwrap_or("?"),
                    s["resources"].as_array().map_or(0, Vec::len),
                    short(&s["lock"]["resolved_commit"])
                );
                // 启用资源需要完整 ID（personal --action select --resource），普通输出也要给出
                for r in s["resources"].as_array().into_iter().flatten() {
                    println!(
                        "    {}  {}",
                        r["id"].as_str().unwrap_or("?"),
                        r["description"].as_str().unwrap_or("")
                    );
                }
            }
        }
        "preview" => {
            let src = &v["source"];
            let resources = v["resources"].as_array().cloned().unwrap_or_default();
            println!(
                "来源 {}（{}）· 版本 {} · {} 项资源",
                src["name"].as_str().unwrap_or("?"),
                src["id"].as_str().unwrap_or("?"),
                short(&src["lock"]["resolved_commit"]),
                resources.len()
            );
            for r in &resources {
                println!("    {}", r["id"].as_str().unwrap_or("?"));
            }
            let removed = v["removed"].as_array().cloned().unwrap_or_default();
            if !removed.is_empty() {
                println!("上游已移除 {} 项：", removed.len());
                for id in &removed {
                    println!("    {}", id.as_str().unwrap_or("?"));
                }
            }
            if let Some(id) = v["preview_id"].as_str() {
                println!("确认登记：ailoom collection --action apply --preview-id {id}");
            }
        }
        "check" => {
            let items = v["items"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                println!("没有可检查的来源");
            }
            for i in &items {
                let state = match i["state"].as_str().unwrap_or("") {
                    "current" => "已是最新".to_string(),
                    "available" => format!(
                        "有可用更新（preview_id {}）",
                        i["preview"]["preview_id"].as_str().unwrap_or("?")
                    ),
                    "external" => "外部目录，由原位置维护".to_string(),
                    "error" => format!("检查失败：{}", i["error"].as_str().unwrap_or("")),
                    other => other.to_string(),
                };
                println!("  {}：{state}", i["name"].as_str().unwrap_or("?"));
            }
        }
        "apply" => {
            let src = &v["source"];
            println!(
                "已登记来源 {}（{}），版本 {}",
                src["name"].as_str().unwrap_or("?"),
                src["id"].as_str().unwrap_or("?"),
                short(&src["lock"]["resolved_commit"])
            );
            if let Some(note) = v["note"].as_str() {
                println!("{note}");
            }
        }
        _ => emit_human(v),
    }
}
