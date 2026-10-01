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
            if let Some(next) = v["next"].as_str() {
                println!("{next}");
            }
            print_notes(v);
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
