//! plan 命令（AIL-007 接线）：预览差异，无任何写入。

use super::sync_core::prepare;
use crate::error::Result;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct PlanArgs {
    pub root: Option<PathBuf>,
}

pub fn run(args: &PlanArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let p = prepare(data_root, args.root.as_deref())?;
    let value = json!({
        "workspace_id": p.ctx.workspace.workspace_id,
        "source": { "identity": p.plan.source_identity, "revision": p.plan.revision },
        "actions": p.plan.actions,
        "summary": {
            "create": count(&p.plan, "create"),
            "update": count(&p.plan, "update"),
            "delete": count(&p.plan, "delete"),
            "restore": count(&p.plan, "restore"),
            "conflict": count(&p.plan, "conflict"),
            "noop": count(&p.plan, "noop"),
            "unsupported": count(&p.plan, "unsupported"),
        },
        "selected_resources": p.desired.selected.len(),
        "excluded_resources": p.desired.excluded.len(),
    });
    if !json {
        print!("{}", p.plan.summary());
        for u in &p.unsupported {
            println!("unsupported {} [{}] {}", u.resource_id, u.tool, u.reason);
        }
        if p.plan.has_conflicts() {
            println!("存在冲突：目标不会被覆盖；请处理后重新 sync。");
        }
    }
    Ok(value)
}

fn count(plan: &crate::sync::plan::SyncPlan, kind: &str) -> usize {
    plan.actions
        .iter()
        .filter(|a| serde_json::to_value(a.action).unwrap_or_default().as_str() == Some(kind))
        .count()
}
